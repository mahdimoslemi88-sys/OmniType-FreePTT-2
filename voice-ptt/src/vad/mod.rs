//! Voice Activity Detection layer.
//!
//! Two engines behind one enum:
//! - `silero` — Silero v5 ONNX model via `ort` (feature `silero-vad`)
//! - `rms`    — zero-dependency energy fallback
//!
//! Plus a shared [`Endpoint`] state machine implementing the research
//! endpointing rules (1500 ms silence timeout, minimum-speech filter).

pub mod rms;
#[cfg(feature = "silero-vad")]
pub mod silero;

pub use rms::RmsVad;

#[cfg(feature = "silero-vad")]
pub use silero::SileroVad;

/// VAD configuration (spec defaults).
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Speech probability threshold (spec: 0.5).
    pub threshold: f32,
    /// Continuous silence that ends an utterance (spec: 1500 ms).
    pub silence_timeout_ms: u64,
    /// Minimum speech length before we accept an utterance (rejects clicks).
    pub min_speech_ms: u64,
    /// Speech probability that counts as "speaking" for the endpoint timer.
    pub speech_start_probability: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            silence_timeout_ms: 1_500,
            min_speech_ms: 150,
            speech_start_probability: 0.5,
        }
    }
}

/// One analyzed frame of audio.
#[derive(Debug, Clone, Copy)]
pub struct FrameResult {
    pub is_speech: bool,
    pub speech_probability: f32,
    pub samples: usize,
}

impl FrameResult {
    /// A frame result for analyzers that only produce a boolean decision.
    pub fn from_bool(is_speech: bool, samples: usize) -> Self {
        Self {
            is_speech,
            speech_probability: if is_speech { 1.0 } else { 0.0 },
            samples,
        }
    }
}

/// Incremental endpointing state machine fed one frame at a time.
///
/// It tracks whether we have seen speech, how much speech we collected, and
/// how long we have been silent, so the recorder can ask
/// [`Endpoint::should_finalize`] after every frame.
#[derive(Debug)]
pub struct Endpoint {
    cfg: VadConfig,
    saw_speech: bool,
    speech_samples: usize,
    silence_samples: usize,
    /// Every sample fed, regardless of classification (utterance length).
    all_samples: usize,
    sample_rate: u32,
}

impl Endpoint {
    pub fn new(cfg: VadConfig, sample_rate: u32) -> Self {
        Self {
            cfg,
            saw_speech: false,
            speech_samples: 0,
            silence_samples: 0,
            all_samples: 0,
            sample_rate,
        }
    }

    /// Feeds one frame result into the state machine.
    pub fn feed(&mut self, frame: &FrameResult) {
        self.all_samples += frame.samples;
        if frame.is_speech {
            self.saw_speech = true;
            self.speech_samples += frame.samples;
            self.silence_samples = 0;
        } else if self.saw_speech {
            self.silence_samples += frame.samples;
        }
    }

    /// Whether speech has started at all.
    pub fn has_speech(&self) -> bool {
        self.saw_speech
    }

    /// Total speech samples observed (ignoring trailing silence).
    pub fn speech_samples(&self) -> usize {
        self.speech_samples
    }

    /// Total samples fed since the last reset (speech + silence + leading
    /// quiet). Approximates elapsed recording time.
    pub fn total_samples(&self) -> usize {
        self.all_samples
    }

    /// Whether we have enough speech to bother transcribing.
    pub fn has_enough_speech(&self) -> bool {
        self.speech_samples >= self.min_speech_samples()
    }

    fn min_speech_samples(&self) -> usize {
        (self.cfg.min_speech_ms * u64::from(self.sample_rate) / 1000) as usize
    }

    /// Whether trailing silence has exceeded the configured timeout.
    pub fn should_finalize(&self) -> bool {
        self.saw_speech
            && self.silence_samples
                >= (self.cfg.silence_timeout_ms * u64::from(self.sample_rate) / 1000) as usize
    }

    /// Resets for the next utterance.
    pub fn reset(&mut self) {
        self.saw_speech = false;
        self.speech_samples = 0;
        self.silence_samples = 0;
        self.all_samples = 0;
    }
}

/// Which VAD engine is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEngine {
    Silero,
    Rms,
}

/// Engine-agnostic VAD dispatcher (Silero when available, RMS otherwise).
pub enum AnyVad {
    #[cfg(feature = "silero-vad")]
    Silero(SileroVad),
    Rms(RmsVad),
}

impl AnyVad {
    /// Builds the best available engine. Falls back to RMS when the Silero
    /// model file is missing or the ONNX runtime fails to initialize.
    pub fn auto(model_path: &std::path::Path, cfg: &VadConfig) -> (Self, VadEngine) {
        #[cfg(feature = "silero-vad")]
        match SileroVad::new(model_path, cfg) {
            Ok(s) => return (AnyVad::Silero(s), VadEngine::Silero),
            Err(e) => tracing::warn!(%e, "Silero VAD unavailable, falling back to RMS"),
        }
        #[cfg(not(feature = "silero-vad"))]
        let _ = (model_path, cfg);
        (AnyVad::Rms(RmsVad::new(cfg)), VadEngine::Rms)
    }

    /// Analyzes one chunk of mono samples.
    pub fn process(&mut self, chunk: &[f32]) -> FrameResult {
        match self {
            #[cfg(feature = "silero-vad")]
            AnyVad::Silero(s) => match s.process(chunk) {
                Ok(fr) => fr,
                Err(e) => {
                    tracing::warn!(%e, "VAD inference error; treating as silence");
                    FrameResult::from_bool(false, chunk.len())
                }
            },
            AnyVad::Rms(r) => r.process(chunk),
        }
    }

    /// Clears cross-utterance engine state (recurrent state + context for
    /// Silero; no-op for the stateless RMS engine).
    pub fn reset(&mut self) {
        match self {
            #[cfg(feature = "silero-vad")]
            AnyVad::Silero(s) => s.reset(),
            AnyVad::Rms(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> VadConfig {
        VadConfig {
            threshold: 0.5,
            silence_timeout_ms: 1_000,
            min_speech_ms: 100,
            speech_start_probability: 0.5,
        }
    }

    #[test]
    fn endpoint_requires_speech_before_finalizing() {
        let mut ep = Endpoint::new(cfg(), 16_000);
        let silence = FrameResult::from_bool(false, 512);
        ep.feed(&silence);
        assert!(!ep.should_finalize());
    }

    #[test]
    fn endpoint_finalizes_after_silence_timeout() {
        let mut ep = Endpoint::new(cfg(), 16_000);
        // 200 ms of speech (> min_speech_ms = 100 ms)
        for _ in 0..10 {
            ep.feed(&FrameResult::from_bool(true, 320)); // 20 ms each
        }
        assert!(!ep.should_finalize());
        // 1.1 s of silence (> timeout = 1000 ms)
        for _ in 0..55 {
            ep.feed(&FrameResult::from_bool(false, 320));
        }
        assert!(ep.should_finalize());
        assert!(ep.has_enough_speech());
    }

    #[test]
    fn endpoint_rejects_clicks() {
        let mut ep = Endpoint::new(cfg(), 16_000);
        ep.feed(&FrameResult::from_bool(true, 320)); // one 20 ms blip
        for _ in 0..60 {
            ep.feed(&FrameResult::from_bool(false, 320));
        }
        assert!(ep.should_finalize());
        assert!(
            !ep.has_enough_speech(),
            "a single click must not be transcribed"
        );
    }

    #[test]
    fn speech_resets_silence_timer() {
        let mut ep = Endpoint::new(cfg(), 16_000);
        ep.feed(&FrameResult::from_bool(true, 320));
        for _ in 0..30 {
            ep.feed(&FrameResult::from_bool(false, 320)); // 600 ms
        }
        ep.feed(&FrameResult::from_bool(true, 320)); // speech again
        assert!(
            !ep.should_finalize(),
            "new speech must reset the silence timer"
        );
    }
}
