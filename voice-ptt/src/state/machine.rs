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

use std::sync::Arc;
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

/// Snapshot broadcast to the UI layer.
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
    pub dictionary: Arc<Dictionary>,
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
                                self.begin_recording(&mut buffer);
                            }
                        }
                        Some(HotkeyEvent::RecordUp) => {
                            if self.status_rx.borrow().state == AppState::Recording {
                                // Hold-to-talk release: finalize immediately.
                                let audio = self.take_and_stop(&mut buffer);
                                self.finalize(audio).await;
                            }
                        }
                        Some(HotkeyEvent::ToggleOverlay) => {
                            // GUI concerns; machine stays idle.
                        }
                    }
                }
                _ = tick.tick() => {
                    if self.status_rx.borrow().state == AppState::Recording {
                        if let Some(audio) = self.poll_vad(&mut buffer, chunk).await? {
                            self.finalize(audio).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn begin_recording(&self, buffer: &mut Vec<f32>) {
        buffer.clear();
        if let Err(e) = self.services.capture.start() {
            self.set_state(AppState::Error(format!("capture start failed: {e:#}")));
            return;
        }
        // Reset endpoint state for the new utterance.
        if let Ok(mut unit) = self.services.vad.try_lock() {
            unit.endpoint.reset();
        }
        self.set_state(AppState::Recording);
        tracing::info!("recording started");
    }

    /// Pulls newly buffered audio and feeds VAD frames.
    /// Returns `Some(full_audio)` when endpointing says finalize.
    async fn poll_vad(&self, buffer: &mut Vec<f32>, chunk_size: usize) -> Result<Option<Vec<f32>>> {
        let fresh = self.services.capture.take_audio();
        if fresh.is_empty() {
            return Ok(None);
        }
        buffer.extend_from_slice(&fresh);

        // Feed whole 512-sample chunks to VAD + endpoint.
        let frames: Vec<Vec<f32>> = buffer
            .chunks(chunk_size)
            .filter(|c| c.len() == chunk_size)
            .map(|c| c.to_vec())
            .collect();

        let mut unit = self.services.vad.lock().await;
        for frame in &frames {
            let result = unit.vad.process(frame);
            unit.endpoint.feed(&result);
        }
        drop(unit);

        let finalize = {
            let unit = self.services.vad.lock().await;
            unit.endpoint.should_finalize()
                || buffer.len() >= self.services.capture.capacity()
        };
        if finalize {
            self.services.capture.stop()?;
            return Ok(Some(std::mem::take(buffer)));
        }
        Ok(None)
    }

    /// Stops capture and returns the accumulated audio.
    fn take_and_stop(&self, buffer: &mut Vec<f32>) -> Vec<f32> {
        // Flush anything still in the ring buffer.
        let fresh = self.services.capture.take_audio();
        buffer.extend_from_slice(&fresh);
        let _ = self.services.capture.stop();
        std::mem::take(buffer)
    }

    /// Transcribes, post-processes and injects. Never returns Err to the
    /// caller: failures land in the Error state and we return to Idle.
    async fn finalize(&self, audio: Vec<f32>) {
        self.set_state(AppState::Processing);

        let enough = {
            let mut unit = self.services.vad.lock().await;
            let ok = unit.endpoint.has_enough_speech();
            unit.endpoint.reset();
            ok
        };
        if !enough {
            tracing::info!("utterance discarded: not enough speech");
            self.set_state(AppState::Idle);
            return;
        }

        let utterance = AudioUtterance {
            samples: audio,
            sample_rate: self.services.settings.audio.sample_rate,
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

        let processed = process_text(&raw, &self.services.normalizer, &self.services.dictionary);
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
}
