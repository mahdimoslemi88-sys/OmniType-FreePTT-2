//! ASR engine abstraction and shared types.

use anyhow::Result;

/// One captured utterance, ready for transcription.
#[derive(Debug, Clone)]
pub struct AudioUtterance {
    /// Mono samples at `sample_rate` (expected: 16 kHz, per the audio layer).
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioUtterance {
    /// Duration of the utterance in seconds (0 when the rate is unset).
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f32 / self.sample_rate as f32
    }
}

/// Health of an ASR engine at a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsrHealth {
    /// Model loaded and usable.
    Ready,
    /// No model file available yet.
    NoModel,
    /// Temporary failure; `retry_after` is when it becomes eligible again.
    Cooldown { reason: String, retry_after_ms: u64 },
    /// Permanent failure (e.g. unsupported hardware).
    Failed { reason: String },
}

impl AsrHealth {
    pub fn is_available(&self) -> bool {
        matches!(self, AsrHealth::Ready)
    }
}

/// A speech-to-text engine. Implementations are expected to be `Send + Sync`
/// and internally synchronized; heavy work is executed on blocking threads by
/// the [`crate::asr::router::AsrRouter`].
pub trait AsrEngine: Send + Sync {
    /// Human-readable engine name for logs/UI.
    fn name(&self) -> &'static str;

    /// Current health/availability.
    fn health(&self) -> AsrHealth;

    /// Transcribes an utterance. Blocking; keep it off async contexts.
    fn transcribe(&self, audio: &AudioUtterance) -> Result<String>;
}
