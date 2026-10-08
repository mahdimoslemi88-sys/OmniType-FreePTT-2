//! The microphone, the VAD, and the wiring that starts the coordinator.
//!
//! Event sources:
//! - hotkey events (forwarded from the polling thread into a Tokio channel)
//! - a 20 ms tick that pulls audio from the ring buffer and runs VAD frames
//!
//! This file is the *hardware half*: it owns the capture device and the voice
//! activity detector, and reports what they made of the audio as plain data —
//! [`PollOutcome`] / `Option<AudioUtterance>`. It decides nothing about
//! sessions, about text, or about the keyboard.
//!
//! The decisions live in [`super::session`] (what an event means, when a chunk
//! is cut, when an utterance ends) and [`super::utterance`] (what a transcript
//! is worth), both pure and unit-tested. The loop that asks those questions and
//! applies the answers — including "is this text still wanted?" — is
//! [`super::coordinator`]. There is exactly one of it, and
//! [`StateMachine::run`] starts that one.
//!
//! Finalization policy (from the research design):
//! - trailing silence ≥ `silence_timeout_ms` after speech → finalize, or
//! - record key released (hold-to-talk) → finalize, or
//! - ring buffer approached capacity (safety valve) → finalize.
//!
//! Utterances with less speech than `min_speech_ms` are discarded (clicks,
//! key taps), matching the VAD research's false-positive mitigation — which is
//! also why [`MachinePort::take_utterance`] answers with `None` instead of
//! handing the coordinator an empty recording to convert.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{mpsc, watch, Mutex};

use crate::asr::engine::AudioUtterance;
use crate::asr::router::AsrRouter;
use crate::audio::AudioCapture;
use crate::config::Settings;
use crate::hotkey::HotkeyEvent;
use crate::vad::{AnyVad, Endpoint, VadConfig};

use super::coordinator::{
    Coordinator, Input, KeystrokeSink, PollOutcome, Port, Speech, SystemClock, WindowTargets,
};
use super::session::{
    endpoint_decision, frames_to_feed, should_flush_chunk, split_chunk, EndpointDecision,
    SessionBuffers, SessionDriver,
};
use super::status::{AppStatus, StatusChannel};
use super::utterance::AudioLevels;

/// VAD unit: engine + endpoint state guarded by one mutex.
pub struct VadUnit {
    pub vad: AnyVad,
    pub endpoint: Endpoint,
}

impl VadUnit {
    /// Builds a VAD unit with a fresh endpoint state machine.
    pub fn new(vad: AnyVad, cfg: VadConfig, sample_rate: u32) -> Self {
        Self {
            vad,
            endpoint: Endpoint::new(cfg, sample_rate),
        }
    }

    pub fn engine_name(&self) -> &'static str {
        match &self.vad {
            #[cfg(feature = "silero-vad")]
            AnyVad::Silero(_) => "silero",
            AnyVad::Rms(_) => "rms",
        }
    }
}

/// All shared services the machine operates on.
pub struct AppServices {
    pub capture: Arc<AudioCapture>,
    pub vad: Mutex<VadUnit>,
    pub router: AsrRouter,
    pub normalizer: Arc<crate::processing::Normalizer>,
    pub dictionary: Arc<RwLock<crate::processing::Dictionary>>,
    pub settings: Arc<Settings>,
}

/// The real [`Port`]: capture in, plain audio out.
///
/// Its only job is to be honest about the hardware. Everything a cancelled
/// result could have damaged — seam, keyboard, badge, session rules — is on the
/// coordinator's side of the boundary, and a `None` here means "the VAD says
/// there was nothing to transcribe", not "nothing happened": the session still
/// has to close.
pub struct MachinePort {
    mouse_recording: AtomicBool,
    services: Arc<AppServices>,
    /// The utterance's accumulation buffer and how far the VAD has read into it.
    ///
    /// Behind a lock rather than a field because the coordinator holds the port
    /// by shared reference; only that one task ever touches it.
    bufs: Mutex<SessionBuffers>,
    frame: usize,
}

fn recording_streaming(
    settings: &crate::config::settings::StreamingSettings,
    mouse: bool,
) -> crate::config::settings::StreamingSettings {
    let mut cfg = settings.clone();
    if mouse {
        cfg.enabled = true;
        cfg.strategy = "silence".into();
        cfg.chunk_seconds = cfg.chunk_seconds.clamp(1, 20);
        cfg.min_chunk_seconds = cfg.min_chunk_seconds.clamp(1, cfg.chunk_seconds);
        cfg.overlap_ms = cfg.overlap_ms.min(cfg.min_chunk_seconds * 500);
        if !settings.enabled || !settings.seam_merge {
            // Forced mouse chunking must not repeat audio when the text-side
            // seam deduplicator is disabled by the user's settings.
            cfg.overlap_ms = 0;
        }
    }
    cfg
}

impl MachinePort {
    pub fn new(services: Arc<AppServices>) -> Self {
        let frame = services.settings.vad.chunk_size;
        Self {
            mouse_recording: AtomicBool::new(false),
            services,
            bufs: Mutex::new(SessionBuffers::default()),
            frame,
        }
    }

    /// Feeds every complete window between the read cursor and the end of the
    /// buffer to the VAD — each exactly once, so the Silero recurrent state and
    /// the endpoint's sample count stay honest (see [`frames_to_feed`]).
    async fn feed_vad(&self, bufs: &mut SessionBuffers) {
        let start = bufs.vad_cursor;
        let frames = frames_to_feed(start, bufs.audio.len(), self.frame);
        if frames == 0 {
            return;
        }
        let mut unit = self.services.vad.lock().await;
        for i in 0..frames {
            let from = start + i * self.frame;
            let result = unit.vad.process(&bufs.audio[from..from + self.frame]);
            unit.endpoint.feed(&result);
        }
        bufs.vad_cursor = start + frames * self.frame;
    }

    /// Turns accumulated samples into the one thing the coordinator may hand to a
    /// worker — or `None` when the endpoint says there was nothing worth an
    /// engine call.
    ///
    /// The endpoint's numbers are read *here*, before the audio is handed over,
    /// so the worker is handed a decision instead of being asked to make one.
    /// That is also what keeps the VAD out of the worker's reach.
    async fn take_utterance(&self, audio: Vec<f32>) -> Option<AudioUtterance> {
        let sample_rate = self.services.capture.pipeline_sample_rate();

        // Audio-level diagnostics: distinguishes "mic delivered silence"
        // (peak ≈ −∞ dBFS → wrong/muted device) from "audio arrived but the VAD
        // called it non-speech" (healthy levels, discarded).
        let levels = AudioLevels::measure(&audio);
        tracing::info!(
            samples = audio.len(),
            peak_dbfs = format!("{:.1}", levels.peak_dbfs()),
            rms_dbfs = format!("{:.1}", levels.rms_dbfs()),
            "utterance audio levels"
        );

        let enough = {
            let mut unit = self.services.vad.lock().await;
            let ok = unit.endpoint.has_enough_speech();
            let speech_secs = unit.endpoint.speech_samples() as f64 / f64::from(sample_rate);
            let total_secs = unit.endpoint.total_samples() as f64 / f64::from(sample_rate);
            tracing::info!(
                speech_secs = format!("{speech_secs:.2}"),
                total_secs = format!("{total_secs:.2}"),
                "endpoint summary"
            );
            unit.endpoint.reset();
            unit.vad.reset();
            ok
        };
        if !enough {
            tracing::info!("utterance discarded: not enough speech");
            return None;
        }
        if audio.is_empty() {
            // A recogniser handed nothing answers "empty transcription", which
            // the router reports as a failure — an error badge for a recording
            // that never contained anything.
            tracing::info!("utterance discarded: no audio");
            return None;
        }
        let utterance = AudioUtterance {
            samples: audio,
            sample_rate,
        };
        tracing::info!(
            audio_secs = utterance.duration_secs(),
            "handing utterance over"
        );
        Some(utterance)
    }

    /// Samples of audio repeated at the start of the next chunk (seam safety).
    fn chunk_overlap_samples(&self) -> usize {
        let rate = u64::from(self.services.capture.pipeline_sample_rate());
        let cfg = recording_streaming(
            &self.services.settings.streaming,
            self.mouse_recording.load(Ordering::Relaxed),
        );
        (rate * cfg.overlap_ms / 1000) as usize
    }

    /// Whether a mid-session chunk is due right now (VAD metrics included).
    async fn chunk_flush_due(&self, buffer_len: usize) -> bool {
        let cfg = recording_streaming(
            &self.services.settings.streaming,
            self.mouse_recording.load(Ordering::Relaxed),
        );
        if !cfg.enabled {
            return false;
        }
        let rate = self.services.capture.pipeline_sample_rate();
        let (silence, speech) = {
            let unit = self.services.vad.lock().await;
            (
                unit.endpoint.trailing_silence_samples(),
                unit.endpoint.speech_samples(),
            )
        };
        should_flush_chunk(&cfg, rate, buffer_len, silence, speech)
    }

    /// Removes the finished chunk from the accumulation buffer and keeps the
    /// configured overlap tail for the next one, so a seam cannot clip a word.
    fn take_chunk(&self, bufs: &mut SessionBuffers) -> AudioUtterance {
        let rate = self.services.capture.pipeline_sample_rate();
        let split = split_chunk(
            bufs.audio.len(),
            bufs.vad_cursor,
            self.chunk_overlap_samples(),
        );
        let chunk: Vec<f32> = bufs.audio[..split.keep_from].to_vec();
        bufs.audio.drain(..split.keep_from);
        bufs.vad_cursor = split.cursor;
        // Per-chunk endpoint state: the next chunk is judged on its own speech.
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
        }
        AudioUtterance {
            samples: chunk,
            sample_rate: rate,
        }
    }
}

impl Port for MachinePort {
    fn set_mouse_recording(&self, enabled: bool) {
        self.mouse_recording.store(enabled, Ordering::Relaxed);
    }
    async fn begin(&self) -> Option<String> {
        let mut bufs = self.bufs.lock().await;
        bufs.audio.clear();
        bufs.vad_cursor = 0;
        if let Err(e) = self.services.capture.start() {
            return Some(format!("{e:#}"));
        }
        // Reset endpoint state for the new utterance, and the engine's own
        // cross-utterance state (Silero recurrent state + context), so stale
        // audio cannot bias the first frames.
        let mut unit = self.services.vad.lock().await;
        unit.endpoint.reset();
        unit.vad.reset();
        None
    }

    async fn poll(&self) -> Result<PollOutcome> {
        let mut bufs = self.bufs.lock().await;
        let fresh = self.services.capture.take_audio();
        if !fresh.is_empty() {
            bufs.audio.extend_from_slice(&fresh);
        }
        self.feed_vad(&mut bufs).await;

        let streaming = &self.services.settings.streaming;
        let rate = self.services.capture.pipeline_sample_rate();
        let decision = {
            let unit = self.services.vad.lock().await;
            let mouse = self.mouse_recording.load(Ordering::Relaxed);
            let cutoff = self.services.settings.vad.cutoff_on_hold && !mouse;
            endpoint_decision(
                bufs.audio.len(),
                unit.endpoint.should_finalize(),
                cutoff,
                streaming.enabled || mouse,
                self.services.capture.capacity(),
                if mouse {
                    u64::MAX
                } else {
                    u64::from(rate) * streaming.max_utterance_seconds
                },
            )
        };
        if let EndpointDecision::Finalize(reason) = decision {
            tracing::debug!(reason = reason.as_str(), "ending utterance");
            self.services.capture.stop()?;
            let audio = std::mem::take(&mut bufs.audio);
            return Ok(PollOutcome::Finished {
                audio: self.take_utterance(audio).await,
            });
        }
        if self.chunk_flush_due(bufs.audio.len()).await {
            let enough_speech = self.services.vad.lock().await.endpoint.has_enough_speech();
            let chunk = self.take_chunk(&mut bufs);
            if enough_speech && !chunk.samples.is_empty() {
                return Ok(PollOutcome::Chunk { audio: chunk });
            }
        }
        Ok(PollOutcome::More)
    }

    async fn finish(&self) -> Result<Option<AudioUtterance>> {
        let mut bufs = self.bufs.lock().await;
        // Flush anything still in the ring buffer.
        let fresh = self.services.capture.take_audio();
        if !fresh.is_empty() {
            bufs.audio.extend_from_slice(&fresh);
        }
        let _ = self.services.capture.stop();
        // Feed any remaining complete frames to the VAD, so the endpoint's
        // speech total is accurate before the verdict is read.
        self.feed_vad(&mut bufs).await;
        let audio = std::mem::take(&mut bufs.audio);
        Ok(self.take_utterance(audio).await)
    }

    async fn discard(&self) -> Result<()> {
        let mut bufs = self.bufs.lock().await;
        bufs.audio.clear();
        bufs.vad_cursor = 0;
        let _ = self.services.capture.stop();
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
            unit.vad.reset();
        }
        Ok(())
    }

    fn capture_live(&self) -> bool {
        self.services.capture.is_recording()
    }
}

/// How often the heartbeat beats.
///
/// One value, shared by the app and by the tests that drive the beat with a
/// clock they control: a schedule pinned to a different number in a test is a
/// schedule nobody measured.
pub(crate) const HEARTBEAT: Duration = Duration::from_millis(20);

/// The one producer of [`Input`]: hotkey events from the source thread, and
/// the heartbeat beat.
///
/// It stops the moment the event source is closed, and stopping means **dropping
/// the sender** — a closed channel is the only shutdown signal the loop has
/// ([`super::coordinator::Coordinator::run`]), so a beat that went on ticking
/// would hide the end of the program: the loop would sit in its `select!`
/// forever with a heartbeat it did not need and a source that will never speak
/// again.
///
/// Both producers live in one task for that reason. Split across two, each held
/// its own clone of the sender, and the beat's clone was the reason a closed
/// source changed nothing at all.
async fn forward_until_closed(
    events: &mut mpsc::UnboundedReceiver<HotkeyEvent>,
    input: &mpsc::UnboundedSender<Input>,
    period: Duration,
) {
    // When the next beat is due. It moves in **exactly one place**: the branch
    // that has just sent a beat.
    //
    // That is not a style preference. The wait below is rebuilt on every turn of
    // this loop, and every event that arrives first throws the previous one away
    // — so a schedule that moved while the wait was *built* would be moved by
    // every keystroke: the heartbeat would be pushed later by the user who is
    // typing, which is the opposite of what a heartbeat is for. Reading it and
    // writing it are two separate statements here so that the difference stays
    // visible.
    //
    // The first beat is one period out rather than immediate: a tick with no
    // open session has no audio to pump.
    let mut due = tokio::time::Instant::now() + period;
    loop {
        tokio::select! {
            // Event first, for the same reason the loop prefers its input: what
            // the user did outranks a background tick.
            biased;
            event = events.recv() => match event {
                Some(ev) => {
                    if input.send(Input::Event(ev)).is_err() {
                        // The loop is gone; nothing left to feed.
                        return;
                    }
                }
                // The source is closed: no hotkey can ever arrive again, so the
                // beat has nothing left to keep alive either. Returning drops
                // the last sender, and the loop hears the end as the closure it
                // was written to hear.
                None => return,
            },
            _ = tokio::time::sleep_until(due) => {
                if input.send(Input::Tick).is_err() {
                    return;
                }
                // Only now, with a beat behind us, does the schedule move.
                let now = tokio::time::Instant::now();
                due = next_beat_at(due, now, period);
            }
        }
    }
}

/// Where the schedule goes after a beat that was due at `at` and happened at
/// `now`: `Skip`, exactly as `MissedTickBehavior::Skip`.
///
/// Keep the period while the loop kept up, and start again from `now` when it did
/// not. A long stall therefore arrives as one beat instead of the backlog it was —
/// and re-basing to `now` rather than to `now + period` would deliver a second
/// beat at that same instant, which is a burst of two.
///
/// A function rather than an inline expression for one honest reason: this is the
/// one rule the loop's own clock cannot demonstrate. A paused clock steps from one
/// timer deadline to the next, so the producer is never late and both branches
/// agree; a scenario that wants a stalled task would have to stop the runtime from
/// polling it, which is not something a test here can arrange. So the rule is
/// tested as arithmetic ([`tests::a_late_beat_is_followed_by_one_a_full_period_later`])
/// and the loop's use of it is read in [`forward_until_closed`].
fn next_beat_at(
    at: tokio::time::Instant,
    now: tokio::time::Instant,
    period: Duration,
) -> tokio::time::Instant {
    std::cmp::max(at + period, now + period)
}

/// The running application.
///
/// Deliberately thin now: it builds the hardware port and the coordinator and
/// gets out of the way. The rules live in [`super::session`], the loop in
/// [`super::coordinator`], and this type only owns the two things the GUI
/// reaches for — a status subscription and the 20 ms heartbeat.
pub struct StateMachine {
    services: Arc<AppServices>,
    /// The UI status channel, shared with the coordinator: both write it, and
    /// the GUI reads it.
    status: Arc<StatusChannel>,
    /// Text waiting on the user, and the answers the loop will read.
    ///
    /// Owned here for the same reason the status channel is: it is the type that
    /// builds the coordinator, and the dashboard needs the *same* instance. A
    /// second one would be a second truth about whether text is waiting.
    review: Arc<crate::state::ReviewChannel>,
}

impl StateMachine {
    pub fn new(services: AppServices) -> Self {
        let vad_engine = match &services.vad.try_lock() {
            Ok(u) => u.engine_name(),
            Err(_) => "…",
        };
        Self {
            services: Arc::new(services),
            status: Arc::new(StatusChannel::new(vad_engine)),
            review: crate::state::ReviewChannel::new(),
        }
    }

    /// Subscribe to status updates (for the overlay/tray).
    pub fn subscribe(&self) -> watch::Receiver<AppStatus> {
        self.status.subscribe()
    }

    /// The microphone gate, for whoever runs a diagnostic test.
    ///
    /// Handed out from here because this is the type that *has* the status
    /// channel the answer comes from. Building it anywhere else would mean
    /// opening a second channel, and a second channel is a second truth: a
    /// gate watching a status nobody publishes to would happily hand the
    /// microphone over in the middle of a dictation.
    pub(crate) fn mic_gate(&self) -> std::sync::Arc<crate::audio::gate::LiveMicGate> {
        std::sync::Arc::new(crate::audio::gate::LiveMicGate::new(self.status.clone()))
    }

    /// The review/recovery wire, for whoever draws the review window.
    ///
    /// Handed out from here for the same reason as [`Self::mic_gate`]: the
    /// dashboard must read the *same* drafts the loop raised, and building a
    /// second channel would be a second answer to "is any text waiting?".
    pub(crate) fn review_channel(&self) -> Arc<crate::state::ReviewChannel> {
        self.review.clone()
    }

    /// Forwards a live partial transcript from a streaming engine.
    pub fn publish_partial(&self, text: &str) {
        self.status.publish_partial(text);
    }

    /// Starts the one coordination loop and feeds it until it stops.
    ///
    /// Both producers land on the same `Input` channel: forwarded hotkey
    /// events, and a 20 ms heartbeat. The loop cannot tell them apart, which is
    /// what lets a test beat by hand instead of sleeping — and it means there is
    /// one place where "what the user did" enters the program.
    pub async fn run(
        self: Arc<Self>,
        mut events: mpsc::UnboundedReceiver<HotkeyEvent>,
    ) -> Result<()> {
        let (input_tx, input_rx) = mpsc::unbounded_channel::<Input>();
        let producer = tokio::spawn(async move {
            forward_until_closed(&mut events, &input_tx, HEARTBEAT).await;
        });

        let settings = self.services.settings.clone();
        let coordinator = Coordinator::new(
            MachinePort::new(self.services.clone()),
            Speech {
                // `AsrRouter` is a cheap handle over shared state, so cloning it
                // costs an allocation and not a second copy of anything.
                router: Arc::new(self.services.router.clone()),
                normalizer: self.services.normalizer.clone(),
                dictionary: self.services.dictionary.clone(),
                settings: settings.clone(),
            },
            SessionDriver::new(&settings.hotkey),
            self.status.clone(),
            // The user's delivery choice, read once here. Pacing is decided at
            // startup rather than per call so that one dictation can never be
            // half burst and half paced.
            Arc::new(KeystrokeSink {
                pacing: crate::state::coordinator::TypePacing::from_settings(&settings.gui),
            }),
            // The window each dictation is for, read when recording starts and
            // re-checked before every insert.
            Arc::new(WindowTargets),
            Arc::new(SystemClock),
            self.review.clone(),
        );
        let result = coordinator.run(input_rx).await;

        // The producer stops with the loop. After a closed source it has already
        // returned; after a Quit it is parked on a channel that will never
        // speak, and its sends only fail once `input_rx` is dropped — there is
        // no reason to wait for that to be noticed.
        producer.abort();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::StreamingSettings;
    use crate::processing::seam::SeamOptions;

    #[test]
    fn mouse_policy_chunks_even_when_streaming_is_disabled_and_bounds_bad_config() {
        let original = StreamingSettings {
            enabled: false,
            chunk_seconds: u64::MAX,
            min_chunk_seconds: u64::MAX,
            overlap_ms: u64::MAX,
            ..StreamingSettings::default()
        };
        let cfg = recording_streaming(&original, true);
        assert!(cfg.enabled);
        assert_eq!(cfg.chunk_seconds, 20);
        assert_eq!(cfg.min_chunk_seconds, 20);
        assert!(cfg.overlap_ms < cfg.chunk_seconds * 1000);
        assert_eq!(cfg.overlap_ms, 0);
        assert!(should_flush_chunk(&cfg, 16000, 20 * 16000, 0, 16000));
        assert_eq!(recording_streaming(&original, false), original);
    }

    #[test]
    fn seam_repair_can_be_switched_off_from_settings() {
        let cfg = StreamingSettings {
            seam_merge: false,
            ..StreamingSettings::default()
        };
        assert_eq!(SeamOptions::from_streaming(&cfg), SeamOptions::off());

        let cfg = StreamingSettings {
            seam_backspace: false,
            seam_max_words: 3,
            ..StreamingSettings::default()
        };
        let opts = SeamOptions::from_streaming(&cfg);
        assert!(opts.dedupe && !opts.backspace && opts.max_overlap_words == 3);
    }

    /// Pure policy check mirroring the endpoint rules used by the port.
    #[test]
    fn endpoint_policy_finalizes_on_timeout_or_release() {
        // This mirrors VadUnit logic; the port's own transitions are
        // integration-tested with real devices (see tests/).
        let mut ep = Endpoint::new(crate::vad::VadConfig::default(), 16_000);
        ep.feed(&crate::vad::FrameResult::from_bool(true, 16_000));
        // 1.5 s of trailing silence (≥ default 1500 ms timeout).
        ep.feed(&crate::vad::FrameResult::from_bool(false, 24_001));
        assert!(ep.should_finalize());
    }

    /// A running producer with both of its ends: `events` is the source the test
    /// closes, `input` is what the loop reads.
    ///
    /// The period is [`HEARTBEAT`] — the app's own number — so what these
    /// scenarios measure is the schedule that ships, not a convenient one.
    fn producer() -> (
        mpsc::UnboundedSender<HotkeyEvent>,
        mpsc::UnboundedReceiver<Input>,
        tokio::task::JoinHandle<()>,
    ) {
        let (events, mut events_rx) = mpsc::unbounded_channel::<HotkeyEvent>();
        let (input, input_rx) = mpsc::unbounded_channel::<Input>();
        let handle = tokio::spawn(async move {
            forward_until_closed(&mut events_rx, &input, HEARTBEAT).await;
        });
        (events, input_rx, handle)
    }

    /// One event through the producer, so that its schedule already exists before
    /// the test reads the clock.
    ///
    /// The producer computes its first deadline when it first runs. A test that
    /// moved the clock before that would be comparing its own `start` with a
    /// deadline the producer counted from a *later* instant, and every assertion
    /// would be off by exactly that difference — which is the kind of wrong that
    /// still looks like a number.
    async fn handshake(
        events: &mpsc::UnboundedSender<HotkeyEvent>,
        input: &mut mpsc::UnboundedReceiver<Input>,
    ) {
        events
            .send(HotkeyEvent::RecordDown)
            .expect("the producer is running");
        assert!(
            matches!(
                input.recv().await,
                Some(Input::Event(HotkeyEvent::RecordDown))
            ),
            "the handshake never reached the loop's input"
        );
    }

    /// Whether the producer has stopped, asked without a clock.
    ///
    /// A paused clock advances only when the runtime is idle, so a producer that
    /// never stops would freeze time and turn a `timeout` here into a hang — on the
    /// very mutation this is meant to catch. A bounded number of yields asks the
    /// question directly instead.
    async fn settled(producer: &tokio::task::JoinHandle<()>) -> bool {
        for _ in 0..1_000 {
            if producer.is_finished() {
                return true;
            }
            tokio::task::yield_now().await;
        }
        producer.is_finished()
    }

    /// Waits for the next beat and reports **when it landed**.
    ///
    /// The clock is the virtual one, so "when" is an exact instant rather than a
    /// duration somebody hoped was close enough — which is the whole reason these
    /// scenarios can say anything about a *schedule*. Events are skipped rather
    /// than mistaken for beats, and a closed channel fails instead of waiting
    /// forever.
    async fn beat_at(input: &mut mpsc::UnboundedReceiver<Input>) -> tokio::time::Instant {
        loop {
            match input.recv().await {
                Some(Input::Tick) => return tokio::time::Instant::now(),
                Some(Input::Event(_)) => continue,
                None => panic!("the producer stopped before the beat arrived"),
            }
        }
    }

    /// Several events before the first deadline must not push the beat later.
    ///
    /// This is the whole point of the schedule living in one variable that only
    /// the beat branch writes: the wait is rebuilt on every one of these events
    /// and thrown away, and a heartbeat that moved when the wait was *built* would
    /// end up `HEARTBEAT` later for every keystroke the user pressed first.
    ///
    /// Three events, no time passing, then the beat — and the claim is the
    /// instant it lands on, not that it arrived at all. A schedule that slid by a
    /// period per event would land at four periods instead of one.
    #[tokio::test(start_paused = true)]
    async fn several_events_before_the_deadline_leave_the_beat_where_it_was() {
        let (events, mut input, producer) = producer();
        handshake(&events, &mut input).await;
        let start = tokio::time::Instant::now();

        // Two more before the first deadline, so three events have been handled
        // and three waits built and thrown away.
        for _ in 0..2 {
            events
                .send(HotkeyEvent::RecordUp)
                .expect("the producer is running");
            assert!(
                matches!(
                    input.recv().await,
                    Some(Input::Event(HotkeyEvent::RecordUp))
                ),
                "an event never reached the loop's input"
            );
        }
        assert_eq!(
            tokio::time::Instant::now(),
            start,
            "the test let time pass on its own"
        );

        let landed = beat_at(&mut input).await;
        assert_eq!(
            landed,
            start + HEARTBEAT,
            "the events moved the beat off the deadline it had already announced"
        );

        producer.abort();
    }

    /// One event, arriving while the wait is pending, cancels that wait — and
    /// cancelling is not the same as rescheduling.
    ///
    /// The event lands halfway to the deadline, which is where a cancellation
    /// would do the most damage if it were treated as a fresh beat.
    ///
    /// The two steps are in this order on purpose. The event is **queued before
    /// the clock moves**, so the producer finds it waiting the next time it runs
    /// rather than racing the deadline; advancing first would let the clock reach
    /// the deadline while the event was still in the channel, and the scenario
    /// would stop being about a cancellation at all.
    #[tokio::test(start_paused = true)]
    async fn an_event_that_cancels_the_wait_does_not_move_the_beat() {
        let (events, mut input, producer) = producer();
        handshake(&events, &mut input).await;
        let start = tokio::time::Instant::now();

        events
            .send(HotkeyEvent::RecordUp)
            .expect("the producer is running");
        tokio::time::advance(HEARTBEAT / 2).await;
        assert!(
            matches!(
                input.recv().await,
                Some(Input::Event(HotkeyEvent::RecordUp))
            ),
            "the cancelling event never reached the loop's input"
        );

        let landed = beat_at(&mut input).await;
        assert_eq!(
            landed,
            start + HEARTBEAT,
            "cancelling the wait pushed the beat half a period later"
        );

        producer.abort();
    }

    /// A late beat is followed by one a full period later, not by the backlog —
    /// the part of `Skip` that a clock cannot show, so it is tested as
    /// arithmetic.
    ///
    /// All three wrong answers are here because they are all different: `now`
    /// (no period at all) delivers a second beat at the same instant, a plain
    /// `at + period` walks the backlog, and a bare `max(at, now) + period` of the
    /// *due* time keeps the lateness instead of measuring the quiet from now.
    #[test]
    fn a_late_beat_is_followed_by_one_a_full_period_later() {
        let t0 = tokio::time::Instant::now();
        let due = t0 + HEARTBEAT;

        // On time: the period is kept, so the schedule neither drifts nor slips.
        assert_eq!(next_beat_at(due, due, HEARTBEAT), t0 + HEARTBEAT * 2);
        // A hair late: still a full period of quiet, measured from the beat that
        // actually happened — a heartbeat that pumps audio is better slightly
        // off-phase than back-to-back.
        assert_eq!(
            next_beat_at(due, due + Duration::from_millis(1), HEARTBEAT),
            due + HEARTBEAT + Duration::from_millis(1)
        );
        // Twenty-five periods late: the backlog is dropped, and the next beat is
        // one period after the one that happened.
        let late = due + Duration::from_millis(500);
        assert_eq!(next_beat_at(due, late, HEARTBEAT), late + HEARTBEAT);
        assert_ne!(
            next_beat_at(due, late, HEARTBEAT),
            late,
            "re-basing to the stall itself would deliver a second beat at once"
        );
        assert_ne!(
            next_beat_at(due, late, HEARTBEAT),
            due + HEARTBEAT,
            "the backlog was walked instead of dropped"
        );
    }

    /// The schedule keeps its period over several beats — the clock-level half of
    /// what `Skip` promises, and the part a loop can be caught on.
    ///
    /// What this cannot cover is spelled out in [`next_beat_at`]: a paused clock
    /// never lets the producer be late, so the "long stall" half of the rule is
    /// arithmetic over there, not a scenario in here.
    #[tokio::test(start_paused = true)]
    async fn beats_keep_the_period_they_were_given() {
        let (events, mut input, producer) = producer();
        handshake(&events, &mut input).await;
        let start = tokio::time::Instant::now();

        for beat in 1..=5u32 {
            tokio::time::advance(HEARTBEAT).await;
            assert_eq!(
                beat_at(&mut input).await,
                start + HEARTBEAT * beat,
                "beat {beat} landed off the schedule"
            );
        }

        producer.abort();
    }

    /// A closed event source has to reach the loop, and a heartbeat must not be
    /// able to hide it.
    ///
    /// That is not a promise about the loop — it is about the *channel*. The
    /// loop stops when its input closes, so anything still holding a sender
    /// after the source is gone keeps the loop waiting for a hotkey that can
    /// never arrive. The old wiring had two senders: the forwarder returned
    /// quietly on a closed source while the heartbeat's clone went on beating
    /// every 20 ms, and the program never learned its input had ended.
    ///
    /// No microphone, and neither wait below asks the clock. That is not a
    /// detail: a paused clock only moves when the runtime has nothing else to
    /// do, so a producer that never stops — which is exactly what this scenario
    /// exists to catch — would keep the runtime busy forever and the virtual
    /// `timeout` would never fire. The first version of this test therefore
    /// *hung* on the mutation it was written for, which is the worst possible way
    /// to be green. Yielding a bounded number of times asks the same question
    /// with no clock involved.
    #[tokio::test(start_paused = true)]
    async fn a_closed_event_source_drops_the_last_sender_while_the_beat_is_alive() {
        let (events, mut input, producer) = producer();
        handshake(&events, &mut input).await;

        // A beat happens, so the heartbeat is a live one: the source is about to
        // close with the beat still able to fire, which is the case that used to
        // hide the shutdown.
        tokio::time::advance(HEARTBEAT).await;
        assert!(
            matches!(input.recv().await, Some(Input::Tick)),
            "the heartbeat never reached the loop's input"
        );

        drop(events);
        assert!(
            settled(&producer).await,
            "the producer kept running after its event source closed"
        );
        producer.await.expect("the producer task itself");

        // With the last sender gone, the loop's `recv` is the closure it was
        // written to hear. This is the whole propagation: a channel with a sender
        // still in it would read as empty-here, and that is the shape the bug
        // had.
        assert!(
            input.try_recv().is_err_and(|closed| {
                matches!(closed, tokio::sync::mpsc::error::TryRecvError::Disconnected)
            }),
            "the loop's input is still open after the source closed"
        );
    }
}
