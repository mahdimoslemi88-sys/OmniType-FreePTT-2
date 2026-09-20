//! Silero VAD (v5) via ONNX Runtime.
//!
//! Model: `silero_vad.onnx` (v5, ~2 MB), downloaded at first run.
//!
//! **Calling convention (matches the official Python `OnnxWrapper`, which is
//! the source of truth for this export):** the ONNX graph expects `input` of
//! shape `[1, 576]` — a 64-sample **context** prefix prepended to each
//! 512-sample chunk at 16 kHz — plus the recurrent `state` `[2, 1, 128]` and
//! `sr` as a 0-d i64 scalar. It returns `[speech_prob, new_state]`.
//!
//! Feeding bare 512-sample chunks (no context) makes the graph take a
//! degenerate path: it returns a constant ~0.001 probability or fails inside
//! its internal STFT (`Invalid input shape: {190}`) — the VAD then never
//! fires and every utterance is discarded as "not enough speech". The
//! context is carried across chunks inside this wrapper and cleared by
//! [`SileroVad::reset`].

use std::path::Path;

use anyhow::{bail, Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Value;

use crate::vad::{FrameResult, VadConfig};

/// Samples per ONNX call at 16 kHz (official wrapper constant).
const CHUNK_SAMPLES: usize = 512;
/// Context samples prepended to every chunk (official wrapper: 64 @ 16 kHz).
const CONTEXT_SAMPLES: usize = 64;
/// Recurrent state size (2 layers × 128 hidden units).
const STATE_ELEMS: usize = 2 * 128;

/// Silero VAD v5 wrapper.
pub struct SileroVad {
    session: Session,
    /// Recurrent state `[2, 1, 128]`, flattened row-major.
    state: Vec<f32>,
    /// Trailing `CONTEXT_SAMPLES` samples of the previous (context-included)
    /// input, prepended to the next chunk.
    context: Vec<f32>,
    /// Carry buffer for inputs that are not multiples of `CHUNK_SAMPLES`.
    carry: Vec<f32>,
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
            state: vec![0.0; STATE_ELEMS],
            context: vec![0.0; CONTEXT_SAMPLES],
            carry: Vec::new(),
            threshold: cfg.threshold,
            sample_rate: 16_000,
        })
    }

    /// Runs one 512-sample chunk (plus the 64-sample context prefix) through
    /// the ONNX graph and returns the speech probability.
    fn infer_chunk(&mut self, chunk: &[f32]) -> Result<f32> {
        debug_assert_eq!(chunk.len(), CHUNK_SAMPLES);

        let mut input = Vec::with_capacity(CONTEXT_SAMPLES + CHUNK_SAMPLES);
        input.extend_from_slice(&self.context);
        input.extend_from_slice(chunk);

        let input = Value::from_array((
            vec![1i64, (CONTEXT_SAMPLES + CHUNK_SAMPLES) as i64],
            input,
        ))
        .context("failed to create VAD input tensor")?;
        let state =
            Value::from_array((vec![2i64, 1, 128], self.state.clone()))
                .context("failed to create state tensor")?;
        // The graph expects `sr` as a 0-d scalar, not a rank-1 tensor
        // (official wrapper passes `np.array(sr, dtype='int64')`).
        let sr = Value::from_array((Vec::<i64>::new(), vec![self.sample_rate]))
            .context("failed to create sample-rate scalar")?;

        // Outputs are positional: [prob, new_state]. Looking them up by the
        // names we guessed has already caused panics on naming drift
        // (`no output named 'state'`); positional access matches the official
        // wrapper's `session.run(None, …)` semantics.
        let outputs = self
            .session
            .run(ort::inputs![
                "input" => input,
                "state" => state,
                "sr" => sr,
            ])
            .context("Silero VAD inference failed")?;

        if outputs.len() < 2 {
            bail!(
                "Silero VAD model returned {} outputs (expected 2)",
                outputs.len()
            );
        }
        let prob = outputs[0]
            .try_extract_array::<f32>()
            .context("failed to read VAD probability output")?
            .iter()
            .copied()
            .next()
            .context("VAD probability output is empty")?;
        self.state = outputs[1]
            .try_extract_array::<f32>()
            .context("failed to read VAD state output")?
            .iter()
            .copied()
            .collect();
        if self.state.len() != STATE_ELEMS {
            bail!(
                "Silero VAD state has {} elements (expected {STATE_ELEMS})",
                self.state.len()
            );
        }

        // Next call's context = last 64 samples of the *context-included*
        // input, i.e. the trailing 64 samples of this chunk.
        self.context.copy_from_slice(&chunk[chunk.len() - CONTEXT_SAMPLES..]);
        Ok(prob)
    }

    /// Analyzes one chunk of mono f32 samples (any length ≥ 0 at 16 kHz),
    /// buffering until full 512-sample windows are available, and returns
    /// the probability of the **last** window analyzed.
    pub fn speech_probability(&mut self, chunk: &[f32]) -> Result<f32> {
        self.carry.extend_from_slice(chunk);
        let mut prob = None;
        while self.carry.len() >= CHUNK_SAMPLES {
            let window: Vec<f32> = self.carry.drain(..CHUNK_SAMPLES).collect();
            prob = Some(self.infer_chunk(&window)?);
        }
        match prob {
            Some(p) => Ok(p),
            // Not enough samples yet: report silence-like probability so the
            // caller sees a well-defined value without inventing speech.
            None => Ok(0.0),
        }
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

    /// Clears all cross-utterance state: recurrent state, context, carry.
    /// Must be called when a new utterance starts so stale context from the
    /// previous utterance cannot bias the first frames.
    pub fn reset(&mut self) {
        self.state.iter_mut().for_each(|s| *s = 0.0);
        self.context.iter_mut().for_each(|s| *s = 0.0);
        self.carry.clear();
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
        let Some(path) = find_model() else {
            eprintln!("skipping: silero_vad.onnx not present");
            return;
        };
        let mut vad = SileroVad::new(&path, &VadConfig::default()).unwrap();
        let silence = vec![0.0f32; 512];
        let prob = vad.speech_probability(&silence).unwrap();
        assert!(prob < 0.5, "digital silence must not be speech (prob={prob})");
    }

    /// Two inference round-trips must refresh the 256-float recurrent state.
    #[test]
    fn state_roundtrip_across_calls() {
        let Some(path) = find_model() else {
            eprintln!("skipping: silero_vad.onnx not present");
            return;
        };
        let mut vad = SileroVad::new(&path, &VadConfig::default()).unwrap();
        let silence = vec![0.0f32; 512];
        let _ = vad.speech_probability(&silence).unwrap();
        let _ = vad.speech_probability(&silence).unwrap();
        assert_eq!(vad.state.len(), STATE_ELEMS, "state must be refreshed");
    }

    /// reset() must clear context, carry and recurrent state.
    #[test]
    fn reset_clears_all_state() {
        let Some(path) = find_model() else {
            eprintln!("skipping: silero_vad.onnx not present");
            return;
        };
        let mut vad = SileroVad::new(&path, &VadConfig::default()).unwrap();
        let _ = vad.speech_probability(&vec![0.1f32; 300]).unwrap();
        vad.reset();
        assert!(vad.carry.is_empty());
        assert!(vad.context.iter().all(|&s| s == 0.0));
        assert!(vad.state.iter().all(|&s| s == 0.0));
    }

    /// Candidate model locations: dev assets or the dist folder.
    fn find_model() -> Option<std::path::PathBuf> {
        [
            std::path::Path::new("assets/silero_vad.onnx"),
            std::path::Path::new("../voice-ptt-dist/assets/silero_vad.onnx"),
        ]
        .into_iter()
        .find(|p| p.exists())
        .map(std::path::Path::to_path_buf)
    }
}
