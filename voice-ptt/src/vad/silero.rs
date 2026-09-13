//! Silero VAD (v5) via ONNX Runtime.
//!
//! Model: `silero_vad.onnx` (v5, ~2 MB), downloaded at first run.
//! The ONNX graph exposes three inputs — `input` ([1, N] f32 chunks of 512
//! samples at 16 kHz), `state` ([2, 1, 128] f32 recurrent state) and `sr`
//! (sample rate as i64) — and returns `output` ([1, 1] speech probability)
//! plus the new `state`.

use std::path::Path;

use anyhow::{Context, Result};
use ndarray::Array1;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Value;

use crate::vad::{FrameResult, VadConfig};

/// Silero VAD v5 wrapper.
pub struct SileroVad {
    session: Session,
    state: Vec<f32>,
    threshold: f32,
    sample_rate: i64,
}

impl SileroVad {
    /// Loads the model from `model_path` (typically `assets/silero_vad.onnx`).
    pub fn new(model_path: &Path, cfg: &VadConfig) -> Result<Self> {
        let session = Session::builder()
            .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
            .and_then(|b| b.with_intra_threads(1))
            .and_then(|b| b.commit_from_file(model_path))
            .with_context(|| {
                format!(
                    "failed to load Silero VAD model from {}",
                    model_path.display()
                )
            })?;

        Ok(Self {
            session,
            state: vec![0.0; 2 * 128],
            threshold: cfg.threshold,
            sample_rate: 16_000,
        })
    }

    /// Analyzes one chunk (should be 512 samples at 16 kHz) and returns speech probability.
    pub fn speech_probability(&mut self, chunk: &[f32]) -> Result<f32> {
        let input = Value::from_array((vec![1i64, chunk.len() as i64], chunk.to_vec()))
            .context("failed to create VAD input tensor")?;
        let state = Value::from_array((vec![2i64, 1, 128], self.state.clone()))
            .context("failed to create state tensor")?;
        let sr = Value::from_array(
            Array1::from_vec(vec![self.sample_rate]),
        )
        .context("failed to create sample-rate tensor")?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input" => input,
                "state" => state,
                "sr" => sr,
            ])
            .context("Silero VAD inference failed")?;

        let output = outputs["output"]
            .try_extract_array::<f32>()
            .context("failed to read VAD output")?;
        let state_out = outputs["state"]
            .try_extract_array::<f32>()
            .context("failed to read VAD state")?;

        let prob = output[0];
        self.state = state_out.iter().copied().collect();
        Ok(prob)
    }

    /// Analyzes one chunk, returning a [`FrameResult`] for the endpoint machine.
    pub fn process(&mut self, chunk: &[f32]) -> Result<FrameResult> {
        let prob = self.speech_probability(chunk)?;
        Ok(FrameResult {
            is_speech: prob > self.threshold,
            speech_probability: prob,
            samples: chunk.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only runs when the model file exists locally (downloaded via
    /// `scripts` or first-run). Skipped silently otherwise so CI without
    /// assets still passes.
    #[test]
    fn silero_silence_near_zero() {
        let path = std::path::Path::new("assets/silero_vad.onnx");
        if !path.exists() {
            eprintln!("skipping: silero_vad.onnx not present");
            return;
        }
        let mut vad = SileroVad::new(path, &VadConfig::default()).unwrap();
        let silence = vec![0.0f32; 512];
        let prob = vad.speech_probability(&silence).unwrap();
        assert!(prob < 0.5, "digital silence must not be speech (prob={prob})");
    }
}
