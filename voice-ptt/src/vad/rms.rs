//! RMS-energy fallback VAD.
//!
//! Much dumber than Silero (cannot tell noise from speech), but it needs no
//! model file, has sub-microsecond latency, and is a solid fallback when the
//! ONNX model or runtime is unavailable — matching the research report's
//! "Fallback: RMS Threshold" recommendation.

use super::{FrameResult, VadConfig};

/// RMS VAD with a simple noise-floor tracker.
pub struct RmsVad {
    threshold: f32,
    /// Exponentially-smoothed noise floor; adapts to ambient noise.
    noise_floor: f32,
}

impl RmsVad {
    pub fn new(_cfg: &VadConfig) -> Self {
        Self {
            threshold: 0.012,
            noise_floor: 0.005,
        }
    }

    /// Analyzes one chunk of mono f32 samples.
    pub fn process(&mut self, samples: &[f32]) -> FrameResult {
        if samples.is_empty() {
            return FrameResult::from_bool(false, 0);
        }
        let rms = (samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32).sqrt();

        // Slowly track the noise floor during quiet periods.
        if rms < self.noise_floor * 1.5 {
            self.noise_floor = 0.99 * self.noise_floor + 0.01 * rms;
        }

        let is_speech = rms > self.threshold.max(self.noise_floor * 3.0);
        FrameResult::from_bool(is_speech, samples.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_not_speech() {
        let mut vad = RmsVad::new(&VadConfig::default());
        let silence = vec![0.0f32; 512];
        assert!(!vad.process(&silence).is_speech);
    }

    #[test]
    fn loud_signal_is_speech() {
        let mut vad = RmsVad::new(&VadConfig::default());
        // 0.4 amplitude sine-ish signal → rms ≈ 0.28, far above threshold.
        let loud: Vec<f32> = (0..512)
            .map(|i| (i as f32 * 0.1).sin() * 0.4)
            .collect();
        assert!(vad.process(&loud).is_speech);
    }
}
