//! whisper.cpp local ASR engine via `whisper-rs`.
//!
//! Key differences from the draft spec's API (spec pinned `0.3`, which is
//! years old): modern `whisper-rs` (0.13+) requires
//! - `SamplingStrategy` passed to `FullParams::new`,
//! - `Option<&str>` for `set_language`,
//! - `set_initial_prompt(Option<&str>)`,
//! - separate `WhisperState` per transcription (`create_state`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::engine::{AsrEngine, AsrHealth, AudioUtterance};

/// Whisper runtime options.
#[derive(Debug, Clone)]
pub struct WhisperOptions {
    /// ISO language code ("fa").
    pub language: String,
    /// Translate to English instead of transcribing.
    pub translate: bool,
    /// Beam size (spec: 5).
    pub beam_size: i32,
    /// CPU threads (spec: 8).
    pub n_threads: i32,
    /// Domain hint prepended to guide recognition of technical terms.
    pub initial_prompt: Option<String>,
}

impl Default for WhisperOptions {
    fn default() -> Self {
        Self {
            language: "fa".into(),
            translate: false,
            beam_size: 5,
            n_threads: 8,
            initial_prompt: Some(
                "گفتار فارسی محاوره‌ای با اصطلاحات فنی: Python، JavaScript، Docker، \
                 API، Database، Git، Server، Compiler، Runtime."
                    .into(),
            ),
        }
    }
}

/// Local whisper.cpp engine. Cheap to clone (Arc'd context).
#[derive(Clone)]
pub struct WhisperEngine {
    context: Option<Arc<WhisperContext>>,
    model_path: PathBuf,
    opts: WhisperOptions,
    active_transcriptions: Arc<AtomicUsize>,
    load_error: Option<String>,
}

impl WhisperEngine {
    /// Loads a ggml model file. Returns an engine with `health() == NoModel`
    /// semantics via `load_error` when loading fails, so the router can
    /// degrade gracefully instead of crashing.
    pub fn load(model_path: &Path, opts: WhisperOptions) -> Self {
        match Self::load_inner(model_path) {
            Ok(context) => Self {
                context: Some(context),
                model_path: model_path.to_path_buf(),
                opts,
                active_transcriptions: Arc::new(AtomicUsize::new(0)),
                load_error: None,
            },
            Err(e) => Self {
                context: None,
                model_path: model_path.to_path_buf(),
                opts,
                active_transcriptions: Arc::new(AtomicUsize::new(0)),
                load_error: Some(format!("{e:#}")),
            },
        }
    }

    fn load_inner(model_path: &Path) -> Result<Arc<WhisperContext>> {
        let ctx_params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .context("model path is not valid UTF-8")?,
            ctx_params,
        )
        .with_context(|| format!("failed to load whisper model {}", model_path.display()))?;
        Ok(Arc::new(ctx))
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Number of transcriptions currently running (for the overlay).
    pub fn active_transcriptions(&self) -> usize {
        self.active_transcriptions.load(Ordering::Relaxed)
    }

    fn transcribe_blocking(&self, audio: &AudioUtterance) -> Result<String> {
        let context = self
            .context
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("whisper model not loaded"))?;

        let samples = &audio.samples;
        if samples.is_empty() {
            return Ok(String::new());
        }
        // whisper requires at least 100 ms of audio (1_600 samples @16 kHz);
        // pad shorter clips with silence to avoid a hard error.
        const MIN_SAMPLES: usize = 1_600;
        let mut samples = samples.clone();
        if samples.len() < MIN_SAMPLES {
            samples.resize(MIN_SAMPLES, 0.0);
        }

        let mut state = context.create_state().context("failed to create state")?;

        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: self.opts.beam_size,
            patience: -1.0,
        });
        params.set_language(Some(&self.opts.language));
        params.set_translate(self.opts.translate);
        params.set_n_threads(self.opts.n_threads.max(1));
        params.set_suppress_blank(true);
        params.set_single_segment(false);
        if let Some(prompt) = &self.opts.initial_prompt {
            params.set_initial_prompt(prompt);
        }
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, &samples)
            .context("whisper inference failed")?;

        let n = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n {
            let seg = state
                .get_segment(i)
                .context("failed to read segment")?;
            text.push_str(seg.to_str().context("failed to read segment text")?);
        }
        Ok(text.trim().to_string())
    }
}

impl AsrEngine for WhisperEngine {
    fn name(&self) -> &'static str {
        "whisper.cpp"
    }

    fn health(&self) -> AsrHealth {
        match (&self.context, &self.load_error) {
            (Some(_), _) => AsrHealth::Ready,
            (None, Some(e)) => AsrHealth::Failed {
                reason: e.clone(),
            },
            (None, None) => AsrHealth::NoModel,
        }
    }

    fn transcribe(&self, audio: &AudioUtterance) -> Result<String> {
        self.active_transcriptions.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let result = self.transcribe_blocking(audio);
        let elapsed = started.elapsed();
        self.active_transcriptions
            .fetch_sub(1, Ordering::Relaxed);

        match &result {
            Ok(_) => tracing::info!(
                engine = self.name(),
                audio_secs = audio.duration_secs(),
                elapsed_ms = elapsed.as_millis() as u64,
                "transcription finished"
            ),
            Err(e) => tracing::warn!(engine = self.name(), %e, "transcription failed"),
        }
        result
    }
}

/// Detects a usable discrete GPU by probing driver DLLs (best effort).
#[cfg(windows)]
pub fn detect_gpu() -> Option<&'static str> {
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};

    fn has_module(name: &str) -> bool {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        unsafe {
            if GetModuleHandleW(PCWSTR(wide.as_ptr())).is_ok() {
                return true;
            }
            LoadLibraryW(PCWSTR(wide.as_ptr())).is_ok()
        }
    }

    if has_module("nvcuda.dll") {
        Some("nvidia")
    } else if has_module("amdhip64.dll") {
        Some("amd")
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn detect_gpu() -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_reports_failed_health_not_panic() {
        let engine = WhisperEngine::load(
            Path::new("models/does-not-exist.bin"),
            WhisperOptions::default(),
        );
        assert!(!engine.health().is_available());
        match engine.health() {
            AsrHealth::Failed { .. } => {}
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn transcribe_without_model_is_error() {
        let engine = WhisperEngine::load(
            Path::new("models/does-not-exist.bin"),
            WhisperOptions::default(),
        );
        let utt = AudioUtterance {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        };
        assert!(AsrEngine::transcribe(&engine, &utt).is_err());
    }
}
