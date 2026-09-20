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
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{mpsc, watch, Mutex};

use crate::asr::engine::AudioUtterance;
use crate::asr::router::AsrRouter;
use crate::audio::AudioCapture;
use crate::config::Settings;
use crate::hotkey::HotkeyEvent;
use crate::output::inject_text;
use crate::processing::{process_text, Dictionary, Normalizer};
use crate::vad::{AnyVad, Endpoint, VadConfig};

/// Visible application state (also mirrored to the UI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Recording,
    Processing,
    Typing,
    Error(String),
}

/// External status packet emitted by the state machine on every transition.
#[derive(Debug, Clone)]
pub struct AppStatus {
    pub state: AppState,
    pub last_text: Option<String>,
    pub vad_engine: &'static str,
}

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

/// The running application.
pub struct StateMachine {
    services: Arc<AppServices>,
    status_tx: watch::Sender<AppStatus>,
    status_rx: watch::Receiver<AppStatus>,
}

impl StateMachine {
    pub fn new(services: AppServices) -> Self {
        let vad_engine = match &services.vad.try_lock() {
            Ok(u) => u.engine_name(),
            Err(_) => "…",
        };
        let (status_tx, status_rx) = watch::channel(AppStatus {
            state: AppState::Idle,
            last_text: None,
            vad_engine,
        });
        Self {
            services: Arc::new(services),
            status_tx,
            status_rx,
        }
    }

    /// Subscribe to status updates (for the overlay/tray).
    pub fn subscribe(&self) -> watch::Receiver<AppStatus> {
        self.status_rx.clone()
    }

    fn set_state(&self, state: AppState) {
        let _ = self.status_tx.send_if_modified(|s| {
            if s.state != state {
                s.state = state.clone();
                true
            } else {
                false
            }
        });
        tracing::debug!(?state, "state");
    }

    fn set_last_text(&self, text: String) {
        let _ = self.status_tx.send_if_modified(|s| {
            s.last_text = Some(text);
            true
        });
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

        self.set_state(AppState::Idle);

        loop {
            tokio::select! {
                ev = events.recv() => {
                    match ev {
                        Some(HotkeyEvent::Quit) | None => {
                            tracing::info!("quit requested");
                            break;
                        }
                        Some(HotkeyEvent::RecordDown) => {
                            if self.status_rx.borrow().state == AppState::Idle {
                                self.begin_recording(&mut buffer, &mut vad_cursor);
                            }
                        }
                        Some(HotkeyEvent::RecordUp) => {
                            if self.status_rx.borrow().state == AppState::Recording {
                                // Hold-to-talk release: finalize immediately.
                                let audio = self.take_and_stop(&mut buffer, &mut vad_cursor, chunk).await;
                                self.finalize(audio).await;
                            }
                        }
                        Some(HotkeyEvent::Cancel) => {
                            if self.status_rx.borrow().state == AppState::Recording {
                                // Cancel speech: discard buffer and stop capture without transcribing
                                buffer.clear();
                                vad_cursor = 0;
                                let _ = self.services.capture.stop();
                                if let Ok(mut unit) = self.services.vad.try_lock() {
                                    unit.endpoint.reset();
                                    unit.vad.reset();
                                }
                                self.set_state(AppState::Idle);
                                tracing::info!("speech recording cancelled by user");
                            }
                        }
                        Some(HotkeyEvent::ToggleOverlay) => {
                            // GUI concerns; machine stays idle.
                        }
                    }
                }
                _ = tick.tick() => {
                    if self.status_rx.borrow().state == AppState::Recording {
                        if let Some(audio) = self.poll_vad(&mut buffer, &mut vad_cursor, chunk).await? {
                            self.finalize(audio).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn begin_recording(&self, buffer: &mut Vec<f32>, vad_cursor: &mut usize) {
        buffer.clear();
        *vad_cursor = 0;
        if let Err(e) = self.services.capture.start() {
            self.set_state(AppState::Error(format!("capture start failed: {e:#}")));
            return;
        }
        // Reset endpoint state for the new utterance.
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
            // Also clear the engine's cross-utterance state (Silero recurrent
            // state + context) so stale audio cannot bias the first frames.
            unit.vad.reset();
        }
        self.set_state(AppState::Recording);
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

        let finalize = {
            let unit = self.services.vad.lock().await;
            let cutoff = self.services.settings.vad.cutoff_on_hold;
            (cutoff && unit.endpoint.should_finalize())
                || buffer.len() >= self.services.capture.capacity()
        };
        if finalize {
            self.services.capture.stop()?;
            return Ok(Some(std::mem::take(buffer)));
        }
        Ok(None)
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
        self.set_state(AppState::Processing);

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
            self.set_state(AppState::Idle);
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
                self.set_state(AppState::Error(format!("ASR failed: {e:#}")));
                return;
            }
        };
        if raw.trim().is_empty() {
            tracing::info!("transcription empty; nothing to type");
            self.set_state(AppState::Idle);
            return;
        }

        let processed = {
            let dict = self.services.dictionary.read().unwrap();
            process_text(&raw, &self.services.normalizer, &dict)
        };
        tracing::info!(raw = %raw, typed = %processed, "text ready");

        self.set_state(AppState::Typing);
        match inject_text(&processed) {
            Ok(n) => {
                tracing::info!(chars = n, "text injected");
                self.set_last_text(processed);
                self.set_state(AppState::Idle);
            }
            Err(e) => {
                self.set_state(AppState::Error(format!("injection failed: {e:#}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
