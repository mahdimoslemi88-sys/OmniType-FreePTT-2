//! Central state machine: Idle → Recording → Processing → Typing → Idle.
//!
//! Event sources:
//! - hotkey events (forwarded from the polling thread into a Tokio channel)
//! - a 20 ms tick that pulls audio from the ring buffer and runs VAD frames
//!
//! This file is the *machinery*: it owns the microphone, the recogniser and the
//! keyboard, and performs what the rules decide. The rules themselves live in
//! [`super::session`] (what an event means, when a chunk is cut, when an
//! utterance ends) and [`super::utterance`] (what a transcript is worth), both
//! pure and unit-tested. Nothing in `run` decides anything.
//!
//! Finalization policy (from the research design):
//! - trailing silence ≥ `silence_timeout_ms` after speech → finalize, or
//! - record key released (hold-to-talk) → finalize, or
//! - ring buffer approached capacity (safety valve) → finalize.
//!
//! Utterances with less speech than `min_speech_ms` are discarded (clicks,
//! key taps), matching the VAD research's false-positive mitigation.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::{mpsc, watch, Mutex};

use crate::asr::engine::AudioUtterance;
use crate::asr::router::AsrRouter;
use crate::audio::AudioCapture;
use crate::config::Settings;
use crate::hotkey::HotkeyEvent;
use crate::output::{inject_backspaces, inject_text};
use crate::processing::seam::{SeamMerge, SeamOptions, SeamStitcher};
use crate::processing::{process_text, Dictionary, Normalizer};
use crate::vad::{AnyVad, Endpoint, VadConfig};

use super::session::{
    endpoint_decision, frames_to_feed, split_chunk, Effect, EndpointDecision, SessionBuffers,
    SessionDriver,
};
use super::status::{AppState, AppStatus, StatusChannel};
use super::utterance::{
    is_transient, plan_typing, AudioLevels, SkipReason, TypePlan, ERROR_READABLE,
};

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
    pub normalizer: Arc<Normalizer>,
    pub dictionary: Arc<RwLock<Dictionary>>,
    pub settings: Arc<Settings>,
}

/// What came out of trying to type one piece of text.
#[derive(Debug)]
enum EmitOutcome {
    /// The transcript had nothing in it worth typing.
    Skipped(SkipReason),
    /// The text reached the focused window.
    Typed { chars: usize },
    /// The keystrokes themselves failed.
    InjectionFailed { error: String },
}

/// The running application.
pub struct StateMachine {
    services: Arc<AppServices>,
    /// Owns the UI status channel and the rules for changing it (`state/status`).
    status: StatusChannel,
    /// Chunk-seam repair state for the running dictation session. A plain mutex
    /// (never held across an `await`) is enough: `stitch` is pure string work.
    seam: std::sync::Mutex<SeamStitcher>,
    /// The session's own rules (latch, recording flag, effect plan). Plain mutex
    /// for the same reason as `seam`, and because `run` holds `Arc<Self>` so the
    /// GUI can read the machine while it runs.
    session: std::sync::Mutex<SessionDriver>,
}

impl StateMachine {
    pub fn new(services: AppServices) -> Self {
        let vad_engine = match &services.vad.try_lock() {
            Ok(u) => u.engine_name(),
            Err(_) => "…",
        };
        let status = StatusChannel::new(vad_engine);
        let seam = SeamStitcher::new(SeamOptions::from_streaming(&services.settings.streaming));
        let session = SessionDriver::new(&services.settings.hotkey);
        Self {
            services: Arc::new(services),
            status,
            seam: std::sync::Mutex::new(seam),
            session: std::sync::Mutex::new(session),
        }
    }

    /// Stitches one chunk transcript onto the running session (see
    /// [`crate::processing::seam`]) and remembers what was typed.
    fn stitch_seam(&self, text: &str) -> SeamMerge {
        let mut seam = self
            .seam
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let merge = seam.stitch(text);
        if merge.dropped_words > 0 || merge.backspaces > 0 {
            tracing::info!(%merge, "chunk seam repaired");
        }
        merge
    }

    /// A new dictation session starts with an empty seam memory: two dictations
    /// in the same document may legitimately begin with the same words.
    fn reset_seam(&self) {
        self.seam
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reset();
    }

    /// Asks the session's rules one question.
    ///
    /// The guard never spans an `await`: every decision is a handful of integer
    /// comparisons, so this is cheaper than it looks and it can never become the
    /// thing that serialises the 20 ms tick.
    fn with_session<R>(&self, f: impl FnOnce(&mut SessionDriver) -> R) -> R {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut session)
    }

    /// Subscribe to status updates (for the overlay/tray).
    pub fn subscribe(&self) -> watch::Receiver<AppStatus> {
        self.status.subscribe()
    }

    /// Forwards a live partial transcript from a streaming engine.
    ///
    /// The caller holds the machine, not the channel, so this stays a method on
    /// `StateMachine` even though the rule it enforces now lives in
    /// [`StatusChannel`].
    pub fn publish_partial(&self, text: &str) {
        self.status.publish_partial(text);
    }

    /// Main loop. Consumes hotkey events until Quit or channel close.
    ///
    /// The loop itself decides nothing: every event is turned into a list of
    /// [`Effect`]s by [`SessionDriver`], and each effect is performed below.
    pub async fn run(
        self: Arc<Self>,
        mut events: mpsc::UnboundedReceiver<HotkeyEvent>,
    ) -> Result<()> {
        let mut bufs = SessionBuffers::default();
        let frame = self.services.settings.vad.chunk_size;

        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        self.status.set_state(AppState::Idle);

        loop {
            tokio::select! {
                ev = events.recv() => {
                    let effects = match ev {
                        Some(HotkeyEvent::Quit) | None => {
                            tracing::info!("quit requested");
                            break;
                        }
                        Some(HotkeyEvent::RecordDown) => {
                            self.with_session(|s| s.on_record_down(Instant::now()))
                        }
                        Some(HotkeyEvent::RecordUp) => {
                            self.with_session(|s| s.on_record_up(Instant::now()))
                        }
                        Some(HotkeyEvent::Cancel) => self.with_session(|s| s.on_cancel()),
                        // GUI concerns; the machine stays idle.
                        Some(HotkeyEvent::ToggleOverlay) => continue,
                    };
                    self.perform(effects, &mut bufs, frame).await?;
                }
                _ = tick.tick() => {
                    let effects = self.with_session(|s| s.on_tick(Instant::now()));
                    self.perform(effects, &mut bufs, frame).await?;
                }
            }
        }

        Ok(())
    }

    /// Performs the effects the session decided on, in order.
    ///
    /// Every way out of a session — key release, VAD endpoint, safety cap,
    /// cancel — goes through this one function, so none of them can forget the
    /// hands-free badge or the bookkeeping that tells the rules whether the
    /// microphone is still live.
    async fn perform(
        &self,
        effects: Vec<Effect>,
        bufs: &mut SessionBuffers,
        frame: usize,
    ) -> Result<()> {
        for effect in effects {
            match effect {
                Effect::BeginRecording => {
                    let open = self.begin_recording(&mut bufs.audio, &mut bufs.vad_cursor);
                    self.with_session(|s| s.began_recording(open));
                }
                Effect::FinishSession => {
                    let audio = self
                        .take_and_stop(&mut bufs.audio, &mut bufs.vad_cursor, frame)
                        .await;
                    self.end_session();
                    self.finalize(audio).await;
                }
                Effect::DiscardSession => {
                    // No await follows, so the end-of-batch sync below takes
                    // the badge down just as promptly.
                    self.discard_recording(&mut bufs.audio, &mut bufs.vad_cursor);
                    self.with_session(|s| s.note_capture_live(false));
                }
                Effect::PollAudio => {
                    // The driver only ever asks for this while it believes a
                    // session is live, so pumping a buffer nobody owns would mean
                    // the two had drifted apart.
                    debug_assert!(self.with_session(|s| s.is_recording()));
                    if let Some(audio) = self
                        .poll_vad(&mut bufs.audio, &mut bufs.vad_cursor, frame)
                        .await?
                    {
                        self.end_session();
                        self.finalize(audio).await;
                    } else if self.chunk_flush_due(bufs.audio.len()).await {
                        // phase 3: hand this chunk over while the microphone
                        // keeps running; the session only ends on key release
                        // (or the safety cap).
                        let chunk_audio =
                            self.take_chunk(&mut bufs.audio, &mut bufs.vad_cursor).await;
                        self.process_chunk(chunk_audio).await;
                    }
                }
            }
        }
        // The badge is also synced here so the paths that only *raise* it (a
        // double-tap) can never be forgotten.
        self.sync_latch_badge();
        Ok(())
    }

    /// Publishes whatever the rules currently say about the hands-free badge.
    ///
    /// `StatusChannel::set_latched` ignores a repeat, so calling this
    /// unconditionally is free — the loop does not have to ask whether anything
    /// changed.
    fn sync_latch_badge(&self) {
        self.status.set_latched(self.with_session(|s| s.latched()));
    }

    /// Closes the session and takes the badge down with it.
    ///
    /// The badge has to go *before* `finalize`, which can spend seconds in the
    /// recogniser, and it goes down here rather than on the next tick so a press
    /// that lands right now still starts a recording.
    fn end_session(&self) {
        self.with_session(|s| s.ended_session());
        self.sync_latch_badge();
    }

    /// Clears the buffer and the seam memory, then opens the microphone.
    /// Returns whether capture is really live, which the session rules need in
    /// order to stop believing in a recording that never started.
    fn begin_recording(&self, buffer: &mut Vec<f32>, vad_cursor: &mut usize) -> bool {
        buffer.clear();
        *vad_cursor = 0;
        // Fresh session: the previous dictation's tail must not swallow the
        // words this one starts with.
        self.reset_seam();
        if let Err(e) = self.services.capture.start() {
            self.status
                .set_state(AppState::Error(format!("capture start failed: {e:#}")));
            return false;
        }
        // Reset endpoint state for the new utterance.
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
            // Also clear the engine's cross-utterance state (Silero recurrent
            // state + context) so stale audio cannot bias the first frames.
            unit.vad.reset();
        }
        self.status.set_state(AppState::Recording);
        tracing::info!("recording started");
        true
    }

    /// Throws the utterance away without transcribing it (user cancel).
    fn discard_recording(&self, buffer: &mut Vec<f32>, vad_cursor: &mut usize) {
        buffer.clear();
        *vad_cursor = 0;
        let _ = self.services.capture.stop();
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
            unit.vad.reset();
        }
        self.status.set_state(AppState::Idle);
        tracing::info!("speech recording cancelled by user");
    }

    /// Feeds every complete window between the read cursor and the end of the
    /// buffer to the VAD — each exactly once, so the Silero recurrent state and
    /// the endpoint's sample count stay honest (see [`frames_to_feed`]).
    async fn feed_vad(&self, buffer: &[f32], vad_cursor: &mut usize, frame: usize) {
        let start = *vad_cursor;
        let frames = frames_to_feed(start, buffer.len(), frame);
        if frames == 0 {
            return;
        }
        let mut unit = self.services.vad.lock().await;
        for i in 0..frames {
            let from = start + i * frame;
            let result = unit.vad.process(&buffer[from..from + frame]);
            unit.endpoint.feed(&result);
        }
        *vad_cursor = start + frames * frame;
    }

    /// Pulls newly buffered audio and feeds VAD frames.
    /// Returns `Some(full_audio)` when the rules say the utterance is over.
    async fn poll_vad(
        &self,
        buffer: &mut Vec<f32>,
        vad_cursor: &mut usize,
        frame: usize,
    ) -> Result<Option<Vec<f32>>> {
        let fresh = self.services.capture.take_audio();
        if !fresh.is_empty() {
            buffer.extend_from_slice(&fresh);
        }

        self.feed_vad(buffer, vad_cursor, frame).await;

        let streaming = &self.services.settings.streaming;
        let rate = self.services.capture.pipeline_sample_rate();
        let decision = {
            let unit = self.services.vad.lock().await;
            let cutoff = self.services.settings.vad.cutoff_on_hold;
            endpoint_decision(
                buffer.len(),
                unit.endpoint.should_finalize(),
                cutoff,
                streaming.enabled,
                self.services.capture.capacity(),
                u64::from(rate) * streaming.max_utterance_seconds,
            )
        };
        if let EndpointDecision::Finalize(reason) = decision {
            tracing::debug!(reason = reason.as_str(), "ending utterance");
            self.services.capture.stop()?;
            return Ok(Some(std::mem::take(buffer)));
        }
        Ok(None)
    }

    /// Samples of audio repeated at the start of the next chunk (seam safety).
    fn chunk_overlap_samples(&self) -> usize {
        let rate = u64::from(self.services.capture.pipeline_sample_rate());
        (rate * self.services.settings.streaming.overlap_ms / 1000) as usize
    }

    /// Whether a mid-session chunk is due right now (VAD metrics included).
    async fn chunk_flush_due(&self, buffer_len: usize) -> bool {
        let cfg = &self.services.settings.streaming;
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
        super::session::should_flush_chunk(cfg, rate, buffer_len, silence, speech)
    }

    /// Removes the finished chunk from the accumulation buffer and keeps the
    /// configured overlap tail for the next one, so a seam cannot clip a word.
    async fn take_chunk(&self, buffer: &mut Vec<f32>, vad_cursor: &mut usize) -> Vec<f32> {
        let split = split_chunk(buffer.len(), *vad_cursor, self.chunk_overlap_samples());
        let chunk: Vec<f32> = buffer[..split.keep_from].to_vec();
        buffer.drain(..split.keep_from);
        *vad_cursor = split.cursor;
        // Per-chunk endpoint state: the next chunk is judged on its own speech.
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
        }
        chunk
    }

    /// Erases the seam artefact, then types the planned text.
    ///
    /// The mid-session chunk and the final utterance differ in what a failure
    /// *means*, not in how text reaches the window, so the backspace-then-type
    /// sequence exists once. A failed backspace is only a warning — the text
    /// still goes in, exactly as it did before this was extracted.
    fn emit(&self, kind: &'static str, raw: &str, plan: &TypePlan) -> EmitOutcome {
        let (text, backspaces) = match plan {
            TypePlan::Skip(reason) => return EmitOutcome::Skipped(*reason),
            TypePlan::Type { text, backspaces } => (text, *backspaces),
        };
        tracing::info!(kind, raw = %raw, typed = %text, backspaces, "text ready");
        if backspaces > 0 {
            if let Err(e) = inject_backspaces(backspaces) {
                tracing::warn!(error = %e, "seam backspaces failed");
            }
        }
        match inject_text(text) {
            Ok(chars) => EmitOutcome::Typed { chars },
            Err(e) => EmitOutcome::InjectionFailed {
                error: format!("{e:#}"),
            },
        }
    }

    /// Transcribes and injects one mid-session chunk **without leaving the
    /// recording state**: the microphone keeps running and the session continues.
    ///
    /// A failed chunk is logged and skipped — a long dictation must never be
    /// aborted because one round trip failed (the final chunk still reports
    /// errors through [`Self::finalize`]).
    async fn process_chunk(&self, audio: Vec<f32>) {
        if audio.is_empty() {
            return;
        }
        let sample_rate = self.services.capture.pipeline_sample_rate();
        tracing::info!(
            audio_secs = audio.len() as f32 / sample_rate as f32,
            still_recording = self.services.capture.is_recording(),
            "flushing mid-session chunk"
        );

        // phase 3.2: no `set_state(Processing)` here. A chunk boundary must not
        // look like the recording stopped and restarted.
        self.status.set_chunk_busy(true);
        let utterance = AudioUtterance {
            samples: audio,
            sample_rate,
        };
        let raw = match self.services.router.transcribe(&utterance).await {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(error = %e, "chunk transcription failed; recording continues");
                self.resume_after_chunk();
                return;
            }
        };

        let processed = {
            let dict = self.services.dictionary.read().unwrap();
            process_text(&raw, &self.services.normalizer, &dict)
        };
        // Seam repair: the audio overlap that keeps the cut from clipping a word
        // also makes the recogniser repeat the previous chunk's tail — and a cut
        // mid-word leaves a fragment behind. Both are repaired here, so a long
        // dictation reads as one continuous text instead of stuttering.
        let plan = plan_typing(&raw, &self.stitch_seam(&processed));
        match self.emit("chunk", &raw, &plan) {
            EmitOutcome::Skipped(reason) => {
                tracing::info!(reason = reason.as_str(), "chunk produced nothing to type");
            }
            EmitOutcome::Typed { chars } => {
                tracing::info!(chars, "chunk text injected");
                if let TypePlan::Type { text, .. } = &plan {
                    self.status.set_last_text(text.clone());
                }
            }
            EmitOutcome::InjectionFailed { error } => {
                tracing::warn!(%error, "chunk injection failed");
            }
        }
        self.resume_after_chunk();
    }

    /// Back to `Recording` while the microphone is still live (chunked session),
    /// otherwise `Idle`.
    fn resume_after_chunk(&self) {
        self.status.set_chunk_busy(false);
        let live = self.services.capture.is_recording();
        if live {
            self.status.set_state(AppState::Recording);
        } else {
            self.status.set_state(AppState::Idle);
        }
        // A capture device that disappeared mid-chunk ends the session here, not
        // on the next key press: the rules must not keep believing we are
        // recording, or that press would find a session already open.
        self.with_session(|s| s.note_capture_live(live));
    }

    /// Stops capture, feeds remaining complete frames to VAD, and returns the accumulated audio.
    async fn take_and_stop(
        &self,
        buffer: &mut Vec<f32>,
        vad_cursor: &mut usize,
        frame: usize,
    ) -> Vec<f32> {
        // Flush anything still in the ring buffer.
        let fresh = self.services.capture.take_audio();
        buffer.extend_from_slice(&fresh);
        let _ = self.services.capture.stop();

        // Feed any remaining complete frames to VAD so endpoint speech
        // calculations are accurate before finalize checks has_enough_speech().
        self.feed_vad(buffer, vad_cursor, frame).await;

        std::mem::take(buffer)
    }

    /// Transcribes, post-processes and injects. Never returns Err to the
    /// caller: failures land in the Error state and we return to Idle.
    async fn finalize(&self, audio: Vec<f32>) {
        self.status.set_state(AppState::Processing);

        // Audio-level diagnostics: distinguishes "mic delivered silence"
        // (peak ≈ −∞ dBFS → wrong/muted device) from "audio arrived but VAD
        // called it non-speech" (healthy levels, discarded).
        let levels = AudioLevels::measure(&audio);
        tracing::info!(
            samples = audio.len(),
            peak_dbfs = format!("{:.1}", levels.peak_dbfs()),
            rms_dbfs = format!("{:.1}", levels.rms_dbfs()),
            "utterance audio levels"
        );

        let sample_rate = self.services.capture.pipeline_sample_rate();
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
            self.status.set_state(AppState::Idle);
            return;
        }

        let utterance = AudioUtterance {
            samples: audio,
            sample_rate,
        };
        tracing::info!(
            audio_secs = utterance.duration_secs(),
            "transcribing utterance"
        );

        let raw = match self.services.router.transcribe(&utterance).await {
            Ok(t) => t,
            Err(e) => {
                self.status
                    .set_state(AppState::Error(format!("ASR failed: {e:#}")));
                return;
            }
        };

        let processed = {
            let dict = self.services.dictionary.read().unwrap();
            process_text(&raw, &self.services.normalizer, &dict)
        };
        // The final chunk of a streamed session sits on the same seam as the
        // mid-session ones, so it is stitched the same way.
        let plan = plan_typing(&raw, &self.stitch_seam(&processed));
        // The visible state moves to `Typing` *before* the keystrokes go out, so
        // the orb shows it. A mid-session chunk deliberately does not (phase
        // 3.2), which is why this stays in the finalise path and not in `emit`.
        match &plan {
            TypePlan::Skip(reason) => {
                tracing::info!(reason = reason.as_str(), "nothing to type");
                self.status.set_state(AppState::Idle);
                return;
            }
            TypePlan::Type { .. } => self.status.set_state(AppState::Typing),
        }
        match self.emit("final", &raw, &plan) {
            EmitOutcome::Typed { chars } => {
                tracing::info!(chars, "text injected");
                if let TypePlan::Type { text, .. } = &plan {
                    self.status.set_last_text(text.clone());
                }
                self.status.set_state(AppState::Idle);
            }
            EmitOutcome::InjectionFailed { error } => {
                self.status
                    .set_state(AppState::Error(format!("injection failed: {error}")));
            }
            EmitOutcome::Skipped(_) => unreachable!("plan was checked above"),
        }

        // `Error` is transient: keep it visible just long enough to read, then
        // return to Idle so the next push-to-talk press starts a fresh
        // recording instead of being swallowed by a stale failure.
        if is_transient(&self.status.snapshot().state) {
            tokio::time::sleep(ERROR_READABLE).await;
            self.status.set_state(AppState::Idle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::StreamingSettings;

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

    /// Pure policy check mirroring the endpoint rules used by the machine.
    #[test]
    fn endpoint_policy_finalizes_on_timeout_or_release() {
        // This mirrors VadUnit logic; the machine's own transitions are
        // integration-tested with real devices (see tests/).
        let mut ep = Endpoint::new(crate::vad::VadConfig::default(), 16_000);
        ep.feed(&crate::vad::FrameResult::from_bool(true, 16_000));
        // 1.5 s of trailing silence (≥ default 1500 ms timeout).
        ep.feed(&crate::vad::FrameResult::from_bool(false, 24_001));
        assert!(ep.should_finalize());
    }
}
