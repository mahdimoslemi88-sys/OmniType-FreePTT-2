//! Central state machine: Idle → Recording → Processing → Typing → Idle.
//!
//! Event sources:
//! - hotkey events (forwarded from the polling thread into a Tokio channel)
//! - a 20 ms tick that pulls audio from the ring buffer and runs VAD frames
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
use crate::config::settings::StreamingSettings;
use crate::config::Settings;
use crate::hotkey::HotkeyEvent;
use crate::output::{inject_backspaces, inject_text};
use crate::processing::seam::{SeamMerge, SeamOptions, SeamStitcher};
use crate::processing::{process_text, Dictionary, Normalizer};
use crate::vad::{AnyVad, Endpoint, VadConfig};

use super::status::{AppState, AppStatus, StatusChannel};

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

/// What the record key's down/up edges mean for the running session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatchAction {
    /// Nothing to do for this edge (e.g. the key was released while latched).
    None,
    /// Start a fresh recording.
    Start,
    /// Finalise the running recording (transcribe + inject) and go idle.
    Finish,
    /// Finalise the pending tap and immediately start a new recording.
    FinishAndRestart,
    /// A double-tap just latched the recording: keep running hands-free.
    Latched,
    /// A short tap ended: keep recording, but leave the finalise decision open
    /// for a second tap until [`LatchPolicy::tick`] times the window out.
    AwaitSecondTap,
}

/// Hands-free latch policy for the record key (pure logic, unit-tested).
///
/// * **hold** the key → recording runs while held, finalises on release (the
///   long-standing hold-to-talk behaviour, unchanged),
/// * **tap once** → the recording stays open for `double_tap_window_ms` and then
///   finalises as before (a lone tap carries no speech, so it is discarded by
///   the endpoint's min-speech filter either way),
/// * **tap twice quickly** → the recording is *latched*: it keeps running after
///   the key is released, and the next press ends it.
#[derive(Debug)]
struct LatchPolicy {
    enabled: bool,
    tap_max_ms: u64,
    window_ms: u64,
    press_started_at: Option<Instant>,
    pending_tap_at: Option<Instant>,
    latched: bool,
}

impl LatchPolicy {
    fn new(hotkey: &crate::config::HotkeySettings) -> Self {
        Self {
            enabled: hotkey.double_tap_latch,
            tap_max_ms: hotkey.tap_max_ms,
            window_ms: hotkey.double_tap_window_ms,
            press_started_at: None,
            pending_tap_at: None,
            latched: false,
        }
    }

    fn is_latched(&self) -> bool {
        self.latched
    }

    /// Drops all latch state (cancel, shutdown, external finalise).
    fn reset(&mut self) {
        self.press_started_at = None;
        self.pending_tap_at = None;
        self.latched = false;
    }

    /// Record key pressed.
    fn press(&mut self, now: Instant, recording: bool) -> LatchAction {
        // While latched, the next press ends the hands-free session.
        if self.latched {
            self.reset();
            return LatchAction::Finish;
        }
        // A pending tap plus a new press is the second tap of a double-tap.
        if let Some(tap_at) = self.pending_tap_at.take() {
            self.press_started_at = Some(now);
            if self.enabled && now.duration_since(tap_at) <= Duration::from_millis(self.window_ms) {
                self.latched = true;
                return LatchAction::Latched;
            }
            // Too slow to be a double-tap: close the first tap and start fresh.
            return LatchAction::FinishAndRestart;
        }
        self.press_started_at = Some(now);
        if recording {
            LatchAction::None
        } else {
            LatchAction::Start
        }
    }

    /// Record key released.
    fn release(&mut self, now: Instant, recording: bool) -> LatchAction {
        if !recording {
            self.press_started_at = None;
            return LatchAction::None;
        }
        // Hands-free: releasing the key must not stop the recording.
        if self.latched {
            return LatchAction::None;
        }
        let held = self
            .press_started_at
            .map(|started| now.duration_since(started));
        self.press_started_at = None;
        let is_tap = self.enabled
            && held
                .map(|held| held <= Duration::from_millis(self.tap_max_ms))
                .unwrap_or(false);
        if is_tap {
            self.pending_tap_at = Some(now);
            LatchAction::AwaitSecondTap
        } else {
            self.pending_tap_at = None;
            LatchAction::Finish
        }
    }

    /// Called from the 20 ms tick: closes a tap whose second press never came.
    fn tick(&mut self, now: Instant, recording: bool) -> LatchAction {
        if !recording {
            // A finalise that came from another path (VAD endpoint, ring-buffer
            // valve, cancel) ends the hands-free session too — otherwise a stale
            // latch would swallow the next press.
            self.reset();
            return LatchAction::None;
        }
        if let Some(tap_at) = self.pending_tap_at {
            if now.duration_since(tap_at) >= Duration::from_millis(self.window_ms) {
                self.pending_tap_at = None;
                return LatchAction::Finish;
            }
        }
        LatchAction::None
    }
}

/// Chunked streaming: should the audio accumulated so far be flushed as a
/// mid-session chunk?
///
/// Pure (no state, no hardware) so the policy is unit-tested:
/// * never flush before `min_chunk_seconds`,
/// * always flush once the chunk reaches `chunk_seconds` — mid-phrase if the
///   speaker never pauses (a free cloud endpoint must never see a long upload),
/// * otherwise, in the `silence` strategy, flush at a real pause: enough trailing
///   silence **and** some actual speech inside this chunk.
fn should_flush_chunk(
    cfg: &StreamingSettings,
    sample_rate: u32,
    chunk_samples: usize,
    trailing_silence_samples: usize,
    speech_samples_in_chunk: usize,
) -> bool {
    if !cfg.enabled || sample_rate == 0 {
        return false;
    }
    let rate = f64::from(sample_rate);
    let chunk_secs = chunk_samples as f64 / rate;
    if chunk_secs < cfg.min_chunk_seconds as f64 {
        return false;
    }
    if chunk_secs >= cfg.chunk_seconds as f64 {
        return true;
    }
    if cfg.strategy.trim().eq_ignore_ascii_case("fixed") {
        return false;
    }
    let silence_secs = trailing_silence_samples as f64 / rate;
    // ≥ 250 ms of speech: a chunk of pure pause is not worth a round trip.
    let has_speech = speech_samples_in_chunk >= sample_rate as usize / 4;
    silence_secs * 1000.0 >= cfg.silence_ms as f64 && has_speech
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

/// The running application.
pub struct StateMachine {
    services: Arc<AppServices>,
    /// Owns the UI status channel and the rules for changing it (`state/status`).
    status: StatusChannel,
    /// Chunk-seam repair state for the running dictation session. A plain mutex
    /// (never held across an `await`) is enough: `stitch` is pure string work.
    seam: std::sync::Mutex<SeamStitcher>,
}

impl StateMachine {
    pub fn new(services: AppServices) -> Self {
        let vad_engine = match &services.vad.try_lock() {
            Ok(u) => u.engine_name(),
            Err(_) => "…",
        };
        let status = StatusChannel::new(vad_engine);
        let seam = SeamStitcher::new(SeamOptions::from_streaming(&services.settings.streaming));
        Self {
            services: Arc::new(services),
            status,
            seam: std::sync::Mutex::new(seam),
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
    pub async fn run(
        self: Arc<Self>,
        mut events: mpsc::UnboundedReceiver<HotkeyEvent>,
    ) -> Result<()> {
        // Audio accumulated for the current utterance.
        let mut buffer: Vec<f32> = Vec::new();
        let mut vad_cursor: usize = 0;
        let chunk = self.services.settings.vad.chunk_size;

        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Hands-free latch (double-tap) state for the record key.
        let mut latch = LatchPolicy::new(&self.services.settings.hotkey);

        self.status.set_state(AppState::Idle);

        loop {
            tokio::select! {
                ev = events.recv() => {
                    match ev {
                        Some(HotkeyEvent::Quit) | None => {
                            tracing::info!("quit requested");
                            break;
                        }
                        Some(HotkeyEvent::RecordDown) => {
                            // `Error` is a resting state, not a dead end: one
                            // failed utterance (offline engine, provider 403)
                            // must never freeze push-to-talk until restart.
                            let recording =
                                self.status.snapshot().state == AppState::Recording;
                            match latch.press(Instant::now(), recording) {
                                LatchAction::Start => {
                                    self.begin_recording(&mut buffer, &mut vad_cursor);
                                }
                                LatchAction::Latched => {
                                    self.status.set_latched(true);
                                    tracing::info!("hands-free latch engaged (double-tap)");
                                }
                                LatchAction::Finish => {
                                    let audio = self
                                        .take_and_stop(&mut buffer, &mut vad_cursor, chunk)
                                        .await;
                                    self.status.set_latched(false);
                                    self.finalize(audio).await;
                                }
                                LatchAction::FinishAndRestart => {
                                    let audio = self
                                        .take_and_stop(&mut buffer, &mut vad_cursor, chunk)
                                        .await;
                                    self.status.set_latched(false);
                                    self.finalize(audio).await;
                                    self.begin_recording(&mut buffer, &mut vad_cursor);
                                }
                                LatchAction::AwaitSecondTap | LatchAction::None => {}
                            }
                        }
                        Some(HotkeyEvent::RecordUp) => {
                            let recording =
                                self.status.snapshot().state == AppState::Recording;
                            match latch.release(Instant::now(), recording) {
                                LatchAction::AwaitSecondTap => {
                                    tracing::debug!(
                                        "short tap detected; waiting for a second tap to latch"
                                    );
                                }
                                LatchAction::Finish => {
                                    // Hold-to-talk release: finalize immediately.
                                    let audio = self
                                        .take_and_stop(&mut buffer, &mut vad_cursor, chunk)
                                        .await;
                                    self.status.set_latched(false);
                                    self.finalize(audio).await;
                                }
                                // Hands-free: the release is deliberately ignored.
                                LatchAction::None
                                | LatchAction::Start
                                | LatchAction::Latched
                                | LatchAction::FinishAndRestart => {}
                            }
                        }
                        Some(HotkeyEvent::Cancel) => {
                            if self.status.snapshot().state == AppState::Recording {
                                // Cancel speech: discard buffer and stop capture without transcribing
                                latch.reset();
                                self.status.set_latched(false);
                                buffer.clear();
                                vad_cursor = 0;
                                let _ = self.services.capture.stop();
                                if let Ok(mut unit) = self.services.vad.try_lock() {
                                    unit.endpoint.reset();
                                    unit.vad.reset();
                                }
                                self.status.set_state(AppState::Idle);
                                tracing::info!("speech recording cancelled by user");
                            }
                        }
                        Some(HotkeyEvent::ToggleOverlay) => {
                            // GUI concerns; machine stays idle.
                        }
                    }
                }
                _ = tick.tick() => {
                    if self.status.snapshot().state == AppState::Recording {
                        match latch.tick(Instant::now(), true) {
                            // A lone tap whose second press never arrived.
                            LatchAction::Finish => {
                                let audio = self
                                    .take_and_stop(&mut buffer, &mut vad_cursor, chunk)
                                    .await;
                                self.status.set_latched(false);
                                self.finalize(audio).await;
                            }
                            _ => {
                                if let Some(audio) =
                                    self.poll_vad(&mut buffer, &mut vad_cursor, chunk).await?
                                {
                                    self.status.set_latched(false);
                                    self.finalize(audio).await;
                                } else if self.chunk_flush_due(buffer.len()).await {
                                    // phase 3: hand this chunk over while the
                                    // microphone keeps running; the session only
                                    // ends on key release (or the safety cap).
                                    let chunk_audio =
                                        self.take_chunk(&mut buffer, &mut vad_cursor).await;
                                    self.process_chunk(chunk_audio).await;
                                }
                            }
                        }
                    } else {
                        // A finalise that came from another path (VAD endpoint,
                        // ring-buffer valve, cancel) also ends the hands-free
                        // session, so no stale latch survives it.
                        if latch.is_latched() {
                            self.status.set_latched(false);
                        }
                        latch.tick(Instant::now(), false);
                    }
                }
            }
        }

        Ok(())
    }

    fn begin_recording(&self, buffer: &mut Vec<f32>, vad_cursor: &mut usize) {
        buffer.clear();
        *vad_cursor = 0;
        // Fresh session: the previous dictation's tail must not swallow the
        // words this one starts with.
        self.reset_seam();
        if let Err(e) = self.services.capture.start() {
            self.status
                .set_state(AppState::Error(format!("capture start failed: {e:#}")));
            return;
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
    }

    /// Pulls newly buffered audio and feeds VAD frames.
    /// Returns `Some(full_audio)` when endpointing says finalize.
    async fn poll_vad(
        &self,
        buffer: &mut Vec<f32>,
        vad_cursor: &mut usize,
        chunk_size: usize,
    ) -> Result<Option<Vec<f32>>> {
        let fresh = self.services.capture.take_audio();
        if !fresh.is_empty() {
            buffer.extend_from_slice(&fresh);
        }

        // Analyze every complete `chunk_size` window exactly once from `vad_cursor`:
        // the read cursor advances by `chunk_size` after each window, so unconsumed
        // tail stays for the next poll and nothing is ever re-fed. Re-feeding
        // would corrupt the Silero recurrent state and quadratic-scale endpoint samples.
        {
            let mut unit = self.services.vad.lock().await;
            while *vad_cursor + chunk_size <= buffer.len() {
                let frame = &buffer[*vad_cursor..*vad_cursor + chunk_size];
                let result = unit.vad.process(frame);
                unit.endpoint.feed(&result);
                *vad_cursor += chunk_size;
            }
        }

        let streaming = &self.services.settings.streaming;
        let rate = self.services.capture.pipeline_sample_rate();
        let max_utterance_samples = u64::from(rate) * streaming.max_utterance_seconds;
        let finalize = {
            let unit = self.services.vad.lock().await;
            let cutoff = self.services.settings.vad.cutoff_on_hold;
            // phase 3: with chunked streaming on, reaching the ring-buffer
            // capacity no longer ends the session — that valve used to cut every
            // dictation at ~30 s (``audio_secs = 30.01`` in the logs). Long
            // sessions are flushed as ordered chunks instead
            // (`should_flush_chunk` / `take_chunk`), so the old valve only applies
            // when streaming is disabled. The absolute `max_utterance_seconds`
            // cap stays as a hard safety net either way.
            (cutoff && unit.endpoint.should_finalize())
                || (!streaming.enabled && buffer.len() >= self.services.capture.capacity())
                || buffer.len() as u64 >= max_utterance_samples
        };
        if finalize {
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
        should_flush_chunk(cfg, rate, buffer_len, silence, speech)
    }

    /// Removes the finished chunk from the accumulation buffer and keeps the
    /// configured overlap tail for the next one, so a seam cannot clip a word.
    async fn take_chunk(&self, buffer: &mut Vec<f32>, vad_cursor: &mut usize) -> Vec<f32> {
        let overlap = self.chunk_overlap_samples();
        let keep_from = buffer.len().saturating_sub(overlap);
        let chunk: Vec<f32> = buffer[..keep_from].to_vec();
        buffer.drain(..keep_from);
        // `vad_cursor` indexes into `buffer`, so it shifts back by the same
        // amount. The configured overlap is far larger than the VAD frame, so the
        // retained tail was already analyzed — no sample is ever fed twice.
        *vad_cursor = (*vad_cursor).saturating_sub(keep_from);
        // Per-chunk endpoint state: the next chunk is judged on its own speech.
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
        }
        chunk
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
        if raw.trim().is_empty() {
            tracing::info!("chunk transcript empty; nothing to type");
            self.resume_after_chunk();
            return;
        }

        let processed = {
            let dict = self.services.dictionary.read().unwrap();
            process_text(&raw, &self.services.normalizer, &dict)
        };
        // Seam repair: the audio overlap that keeps the cut from clipping a word
        // also makes the recogniser repeat the previous chunk's tail — and a cut
        // mid-word leaves a fragment behind. Both are repaired here, so a long
        // dictation reads as one continuous text instead of stuttering.
        let merge = self.stitch_seam(&processed);
        if merge.text.is_empty() {
            tracing::info!("chunk was pure seam overlap; nothing new to type");
            self.resume_after_chunk();
            return;
        }
        tracing::info!(raw = %raw, typed = %merge.text, dropped = merge.dropped_words, backspaces = merge.backspaces, "chunk text ready");
        if merge.backspaces > 0 {
            if let Err(e) = inject_backspaces(merge.backspaces) {
                tracing::warn!(error = %e, "seam backspaces failed");
            }
        }
        match inject_text(&merge.text) {
            Ok(chars) => tracing::info!(chars, "chunk text injected"),
            Err(e) => tracing::warn!(error = %e, "chunk injection failed"),
        }
        self.status.set_last_text(merge.text);
        self.resume_after_chunk();
    }

    /// Back to `Recording` while the microphone is still live (chunked session),
    /// otherwise `Idle`.
    fn resume_after_chunk(&self) {
        self.status.set_chunk_busy(false);
        if self.services.capture.is_recording() {
            self.status.set_state(AppState::Recording);
        } else {
            self.status.set_state(AppState::Idle);
        }
    }

    /// Stops capture, feeds remaining complete frames to VAD, and returns the accumulated audio.
    async fn take_and_stop(
        &self,
        buffer: &mut Vec<f32>,
        vad_cursor: &mut usize,
        chunk_size: usize,
    ) -> Vec<f32> {
        // Flush anything still in the ring buffer.
        let fresh = self.services.capture.take_audio();
        buffer.extend_from_slice(&fresh);
        let _ = self.services.capture.stop();

        // Feed any remaining complete frames to VAD so endpoint speech
        // calculations are accurate before finalize checks has_enough_speech().
        {
            let mut unit = self.services.vad.lock().await;
            while *vad_cursor + chunk_size <= buffer.len() {
                let frame = &buffer[*vad_cursor..*vad_cursor + chunk_size];
                let result = unit.vad.process(frame);
                unit.endpoint.feed(&result);
                *vad_cursor += chunk_size;
            }
        }

        std::mem::take(buffer)
    }

    /// Transcribes, post-processes and injects. Never returns Err to the
    /// caller: failures land in the Error state and we return to Idle.
    async fn finalize(&self, audio: Vec<f32>) {
        self.status.set_state(AppState::Processing);

        // Audio-level diagnostics: distinguishes "mic delivered silence"
        // (peak ≈ −∞ dBFS → wrong/muted device) from "audio arrived but VAD
        // called it non-speech" (healthy levels, discarded). Full scale = 1.0.
        let peak = audio.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        let rms = if audio.is_empty() {
            0.0
        } else {
            (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt()
        };
        tracing::info!(
            samples = audio.len(),
            peak_dbfs = format!("{:.1}", 20.0 * peak.max(1e-6).log10()),
            rms_dbfs = format!("{:.1}", 20.0 * rms.max(1e-6).log10()),
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
        if raw.trim().is_empty() {
            tracing::info!("transcription empty; nothing to type");
            self.status.set_state(AppState::Idle);
            return;
        }

        let processed = {
            let dict = self.services.dictionary.read().unwrap();
            process_text(&raw, &self.services.normalizer, &dict)
        };
        // The final chunk of a streamed session sits on the same seam as the
        // mid-session ones, so it is stitched the same way.
        let merge = self.stitch_seam(&processed);
        if merge.text.is_empty() {
            tracing::info!("final chunk was pure seam overlap; nothing to type");
            self.status.set_state(AppState::Idle);
            return;
        }
        tracing::info!(raw = %raw, typed = %merge.text, dropped = merge.dropped_words, backspaces = merge.backspaces, "text ready");

        self.status.set_state(AppState::Typing);
        if merge.backspaces > 0 {
            if let Err(e) = inject_backspaces(merge.backspaces) {
                tracing::warn!(error = %e, "seam backspaces failed");
            }
        }
        match inject_text(&merge.text) {
            Ok(n) => {
                tracing::info!(chars = n, "text injected");
                self.status.set_last_text(merge.text);
                self.status.set_state(AppState::Idle);
            }
            Err(e) => {
                self.status
                    .set_state(AppState::Error(format!("injection failed: {e:#}")));
            }
        }

        // `Error` is transient: keep it visible just long enough to read, then
        // return to Idle so the next push-to-talk press starts a fresh
        // recording instead of being swallowed by a stale failure.
        if matches!(self.status.snapshot().state, AppState::Error(_)) {
            tokio::time::sleep(Duration::from_secs(3)).await;
            self.status.set_state(AppState::Idle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── chunked streaming policy ─────────────────────────────────────────

    #[test]
    fn streaming_never_flushes_below_the_minimum_chunk() {
        let cfg = StreamingSettings::default();
        // 3 s of audio with a long pause: still too short to cut.
        assert!(!should_flush_chunk(&cfg, 16_000, 48_000, 16_000, 32_000));
    }

    #[test]
    fn streaming_always_flushes_at_the_hard_chunk_cap() {
        let cfg = StreamingSettings::default();
        // 20 s with zero silence: flushed mid-phrase on purpose — a free cloud
        // endpoint is never handed a long upload.
        assert!(should_flush_chunk(&cfg, 16_000, 320_000, 0, 320_000));
    }

    #[test]
    fn streaming_cuts_at_a_pause_in_silence_strategy() {
        let cfg = StreamingSettings::default();
        // 8 s with 1.3 s of trailing silence and plenty of speech → clean cut
        // (phase 3.2 raised the pause from 600 ms, which cut on every breath).
        assert!(should_flush_chunk(&cfg, 16_000, 128_000, 20_800, 100_000));
        // A short breath (900 ms) is deliberately no longer a boundary.
        assert!(!should_flush_chunk(&cfg, 16_000, 128_000, 14_400, 100_000));
        // Same length, but the pause is only 100 ms → keep accumulating.
        assert!(!should_flush_chunk(&cfg, 16_000, 128_000, 1_600, 110_000));
        // Long pause but no speech in this chunk (room noise) → not worth a trip.
        assert!(!should_flush_chunk(&cfg, 16_000, 128_000, 60_000, 1_000));
    }

    #[test]
    fn streaming_fixed_strategy_ignores_pauses_but_keeps_the_cap() {
        let cfg = StreamingSettings {
            strategy: "fixed".into(),
            ..StreamingSettings::default()
        };
        assert!(!should_flush_chunk(&cfg, 16_000, 128_000, 40_000, 100_000));
        assert!(should_flush_chunk(&cfg, 16_000, 320_000, 40_000, 300_000));
    }

    #[test]
    fn streaming_disabled_never_flushes() {
        let cfg = StreamingSettings {
            enabled: false,
            ..StreamingSettings::default()
        };
        assert!(!should_flush_chunk(&cfg, 16_000, 640_000, 0, 600_000));
    }

    /// Long-dictation shape without hardware or network: feed frames into the
    /// same policy the loop uses, apply the same `take_chunk` overlap rule, and
    /// stitch each chunk like the loop does.
    ///
    /// This is the regression proof for the old 30 s ceiling: the session is not
    /// ended, it is *cut into ordered chunks* — and they read as one text.
    #[test]
    fn a_sixty_second_session_is_chunked_and_stitched_in_order() {
        let cfg = StreamingSettings::default();
        let rate: u32 = 16_000;
        let frame = 240usize; // 15 ms of audio per simulated tick
        let overlap = rate as usize * cfg.overlap_ms as usize / 1000;

        let mut buffer_len = 0usize;
        let mut elapsed_ms = 0u64;
        let mut cuts: Vec<u64> = Vec::new();
        let mut stitcher = SeamStitcher::new(SeamOptions::default());
        let mut typed = String::new();

        for _ in 0..4_000 {
            buffer_len += frame;
            elapsed_ms += 15;
            // Unbroken speech: no pause is ever available, so only the hard cap
            // can cut — exactly the case that used to run into the valve.
            if !should_flush_chunk(&cfg, rate, buffer_len, 0, buffer_len) {
                continue;
            }
            cuts.push(elapsed_ms);
            buffer_len = overlap; // `take_chunk` keeps the seam overlap

            let transcript = format!("قسمت {} ادامه دارد", cuts.len());
            let merge = stitcher.stitch(&transcript);
            if !merge.text.is_empty() {
                if !typed.is_empty() {
                    typed.push(' ');
                }
                typed.push_str(&merge.text);
            }
        }

        // Three cuts inside 60 s, one per chunk cap; the old safety valve would
        // have ended the whole session at 30 s, after the first one.
        assert_eq!(cuts.len(), 3, "cuts: {cuts:?}");
        // Frame-quantised, so allow the tick that crosses the cap.
        assert!(
            (20_000..20_100).contains(&cuts[0]),
            "first cut: {}",
            cuts[0]
        );
        assert!(
            (39_000..=40_000).contains(&cuts[1]),
            "second cut: {}",
            cuts[1]
        );
        assert!(cuts[2] >= 58_000, "third cut: {}", cuts[2]);
        // Past the old ceiling the microphone is still live and audio is still
        // accumulating (the overlap tail plus everything since the last cut).
        assert!(buffer_len > overlap);

        // Ordered, complete text — and no false dedupe between chunks that
        // happen to start with the same word.
        assert_eq!(
            typed,
            "قسمت 1 ادامه دارد قسمت 2 ادامه دارد قسمت 3 ادامه دارد"
        );
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

    // ── hands-free latch policy ──────────────────────────────────────────

    #[test]
    fn latch_policy_hold_to_talk_is_unchanged() {
        let mut p = LatchPolicy::new(&crate::config::HotkeySettings::default());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        assert_eq!(
            p.release(t0 + Duration::from_millis(900), true),
            LatchAction::Finish
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_single_tap_waits_for_the_window_then_finalises() {
        let mut p = LatchPolicy::new(&crate::config::HotkeySettings::default());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        let tap_end = t0 + Duration::from_millis(80);
        assert_eq!(p.release(tap_end, true), LatchAction::AwaitSecondTap);
        // Still inside the double-tap window: keep holding the decision open.
        assert_eq!(
            p.tick(tap_end + Duration::from_millis(300), true),
            LatchAction::None
        );
        // Window expired with no second tap: behave like a plain tap did before.
        assert_eq!(
            p.tick(tap_end + Duration::from_millis(650), true),
            LatchAction::Finish
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_double_tap_goes_hands_free_until_the_next_press() {
        let mut p = LatchPolicy::new(&crate::config::HotkeySettings::default());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        let first_up = t0 + Duration::from_millis(90);
        assert_eq!(p.release(first_up, true), LatchAction::AwaitSecondTap);

        let second_down = first_up + Duration::from_millis(200);
        assert_eq!(p.press(second_down, true), LatchAction::Latched);
        assert!(p.is_latched());

        // Releasing the key must not stop a latched session…
        assert_eq!(
            p.release(second_down + Duration::from_millis(90), true),
            LatchAction::None
        );
        assert!(p.is_latched());
        // …and the tick keeps it alive (no pending tap to time out).
        assert_eq!(
            p.tick(second_down + Duration::from_secs(5), true),
            LatchAction::None
        );

        // The next press ends it.
        assert_eq!(
            p.press(second_down + Duration::from_secs(6), true),
            LatchAction::Finish
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_second_tap_too_late_restarts() {
        let mut p = LatchPolicy::new(&crate::config::HotkeySettings::default());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        let first_up = t0 + Duration::from_millis(90);
        assert_eq!(p.release(first_up, true), LatchAction::AwaitSecondTap);
        // 2 s later is way past the 600 ms window.
        assert_eq!(
            p.press(first_up + Duration::from_millis(2_000), true),
            LatchAction::FinishAndRestart
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_disabled_keeps_plain_hold_to_talk() {
        let hotkey = crate::config::HotkeySettings {
            double_tap_latch: false,
            ..Default::default()
        };
        let mut p = LatchPolicy::new(&hotkey);
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        // Even a very short tap finalises immediately when the latch is off.
        assert_eq!(
            p.release(t0 + Duration::from_millis(50), true),
            LatchAction::Finish
        );
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

    /// Validates that the window cursor processes each chunk exactly once
    /// with O(N) complexity instead of the quadratic O(N^2) re-feed bug.
    #[test]
    fn cursor_advances_without_refeeding() {
        let mut buffer: Vec<f32> = Vec::new();
        let mut vad_cursor = 0usize;
        let chunk_size = 512;

        let mut frames_fed = 0usize;
        // Simulate 10 polls of 320 samples (20 ms at 16 kHz)
        for _ in 0..10 {
            buffer.extend_from_slice(&vec![0.1f32; 320]);
            while vad_cursor + chunk_size <= buffer.len() {
                frames_fed += 1;
                vad_cursor += chunk_size;
            }
        }
        // Total samples added = 3200.
        // Complete 512-sample chunks = 3200 / 512 = 6 chunks (3072 samples).
        // vad_cursor must be 3072, and frames_fed must be exactly 6 (not 1+2+3... = 21).
        assert_eq!(vad_cursor, 3072);
        assert_eq!(frames_fed, 6);
        assert_eq!(buffer.len() - vad_cursor, 128); // unconsumed tail
    }
}
