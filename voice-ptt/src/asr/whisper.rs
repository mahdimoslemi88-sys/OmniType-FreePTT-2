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
use std::sync::{Arc, Mutex, RwLock};
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
pub struct WhisperEngine {
    context: RwLock<Option<Arc<WhisperContext>>>,
    model_path: RwLock<PathBuf>,
    opts: WhisperOptions,
    active_transcriptions: Arc<AtomicUsize>,
    load_error: RwLock<Option<String>>,
    /// Serializes the one-time lazy model load: concurrent transcriptions must
    /// not each map the ~1.6 GB model file into RAM. Kept separate from
    /// `context` so the (very common) read of an already-loaded context stays
    /// a cheap read lock.
    load_lock: Mutex<()>,
}

impl Clone for WhisperEngine {
    fn clone(&self) -> Self {
        Self {
            context: RwLock::new(self.context.read().expect("context lock poisoned").clone()),
            model_path: RwLock::new(self.model_path().clone()),
            opts: self.opts.clone(),
            active_transcriptions: self.active_transcriptions.clone(),
            load_error: RwLock::new(self.load_error.read().expect("load_error lock poisoned").clone()),
            load_lock: Mutex::new(()),
        }
    }
}

impl WhisperEngine {
    /// Loads a ggml model file. Returns an engine with `health() == NoModel`
    /// semantics via `load_error` when loading fails, so the router can
    /// degrade gracefully instead of crashing. The engine also starts cold
    /// (`load_error = Some("model file not present yet …")`) when the file
    /// does not exist — a background download can then hot-reload it with
    /// [`WhisperEngine::reload`] once it completes.
    pub fn load(model_path: &Path, opts: WhisperOptions) -> Self {
        if !model_path.is_file() {
            tracing::info!(
                model = %model_path.display(),
                "model file not present yet; engine starts cold and will hot-reload after download"
            );
            return Self {
                context: RwLock::new(None),
                model_path: RwLock::new(model_path.to_path_buf()),
                opts,
                active_transcriptions: Arc::new(AtomicUsize::new(0)),
                load_error: RwLock::new(Some(
                    "model file not present yet (background download pending)".into(),
                )),
                load_lock: Mutex::new(()),
            };
        }
        match Self::load_inner(model_path) {
            Ok(context) => Self {
                context: RwLock::new(Some(context)),
                model_path: RwLock::new(model_path.to_path_buf()),
                opts,
                active_transcriptions: Arc::new(AtomicUsize::new(0)),
                load_error: RwLock::new(None),
                load_lock: Mutex::new(()),
            },
            Err(e) => Self {
                context: RwLock::new(None),
                model_path: RwLock::new(model_path.to_path_buf()),
                opts,
                active_transcriptions: Arc::new(AtomicUsize::new(0)),
                load_error: RwLock::new(Some(format!("{e:#}"))),
                load_lock: Mutex::new(()),
            },
        }
    }

    /// Creates an engine that starts **cold**: the model file is *not*
    /// loaded until the first transcription that actually routes to this
    /// engine ([`Self::ensure_loaded`]).
    ///
    /// Why: a `large-v3-turbo` ggml model maps almost 1:1 into private
    /// process memory (~1.6 GB). Loading it eagerly at startup commits all
    /// of that RAM even when the user has selected a cloud engine and the
    /// local one never runs. Starting cold keeps the memory uncommitted
    /// until it is genuinely needed.
    pub fn cold(model_path: PathBuf, opts: WhisperOptions) -> Self {
        Self {
            context: RwLock::new(None),
            model_path: RwLock::new(model_path),
            opts,
            active_transcriptions: Arc::new(AtomicUsize::new(0)),
            load_error: RwLock::new(None),
            load_lock: Mutex::new(()),
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

    /// Swaps in a freshly downloaded model without restarting the app. In-flight
    /// transcriptions keep their `Arc` to the old context; the next utterance
    /// uses the new one. Returns whether the model actually loaded.
    pub fn reload(&self, model_path: &Path) -> bool {
        match Self::load_inner(model_path) {
            Ok(context) => {
                *self.model_path.write().expect("model_path lock poisoned") =
                    model_path.to_path_buf();
                *self.context.write().expect("context lock poisoned") = Some(context);
                *self.load_error.write().expect("load_error lock poisoned") = None;
                true
            }
            Err(e) => {
                tracing::error!(model = %model_path.display(), error = %e, "model reload failed");
                false
            }
        }
    }

    /// Loads the model on first use. On a cold engine ([`Self::cold`]) the
    /// model file is deliberately absent from memory until the router actually
    /// routes an utterance to this engine — committing ~1.6 GB of RAM only
    /// then. Idempotent once loaded, and never panics.
    fn ensure_loaded(&self) {
        // Fast path: already loaded (cheap read lock, no contention).
        if self.context.read().expect("context lock poisoned").is_some() {
            return;
        }

        // Only the first caller maps the model; the rest wait on this guard
        // and then see the context the winner installed.
        let _guard = self.load_lock.lock().unwrap_or_else(|e| e.into_inner());

        // Re-check under the exclusive lock (double-checked locking).
        if self.context.read().expect("context lock poisoned").is_some() {
            return;
        }
        // A previous load failed: keep its reason instead of hammering the
        // file on every transcription attempt.
        if self.load_error.read().map(|e| e.is_some()).unwrap_or(false) {
            return;
        }

        let path = self.model_path();
        if !path.is_file() {
            if let Ok(mut err) = self.load_error.write() {
                *err = Some("model file not present yet (background download pending)".into());
            }
            return;
        }

        tracing::info!(model = %path.display(), "loading whisper model on first use");
        match Self::load_inner(&path) {
            Ok(context) => {
                if let Ok(mut ctx) = self.context.write() {
                    *ctx = Some(context);
                }
                if let Ok(mut err) = self.load_error.write() {
                    *err = None;
                }
                tracing::info!(model = %path.display(), "whisper model loaded (lazy)");
            }
            Err(e) => {
                if let Ok(mut err) = self.load_error.write() {
                    *err = Some(format!("{e:#}"));
                }
                tracing::error!(model = %path.display(), error = %e, "lazy whisper model load failed");
            }
        }
    }

    pub fn model_path(&self) -> PathBuf {
        self.model_path.read().expect("model_path lock poisoned").clone()
    }

    /// Number of transcriptions currently running (for the overlay).
    pub fn active_transcriptions(&self) -> usize {
        self.active_transcriptions.load(Ordering::Relaxed)
    }

    fn transcribe_blocking(&self, audio: &AudioUtterance) -> Result<String> {
        // Load the model now, on the transcription's blocking thread. This is
        // the whole point of the cold-start engine: the ~1.6 GB mapping lands
        // only when a user actually needs local ASR.
        self.ensure_loaded();

        let context = self
            .context
            .read()
            .expect("context lock poisoned")
            .as_ref()
            .ok_or_else(|| {
                let reason = self
                    .load_error
                    .read()
                    .ok()
                    .and_then(|e| e.clone())
                    .unwrap_or_else(|| "whisper model not loaded".to_string());
                anyhow::anyhow!("{reason}")
            })?
            .clone();

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

    fn id(&self) -> String {
        "local_whisper".to_string()
    }

    fn display_name(&self) -> String {
        "Local Whisper".to_string()
    }

    fn kind(&self) -> &'static str {
        "Local"
    }

    fn health(&self) -> AsrHealth {
        let ctx = self.context.read().expect("context lock poisoned");
        match &*ctx {
            // Loaded.
            Some(_) => AsrHealth::Ready,
            // Cold: report Ready whenever the model file is on disk and no
            // load has failed yet — the ~1.6 GB is simply not mapped until
            // the first transcription. Otherwise the router/dashboard would
            // treat a perfectly usable engine as permanently offline.
            None => {
                let err = self.load_error.read().expect("load_error lock poisoned");
                match &*err {
                    None if self.model_path().is_file() => AsrHealth::Ready,
                    Some(e) => AsrHealth::Failed { reason: e.clone() },
                    None => AsrHealth::NoModel,
                }
            }
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

    #[test]
    fn cold_engine_hot_reloads_when_model_appears() {
        let dir = std::env::temp_dir().join("voice-ptt-whisper-hotreload");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.bin");

        // Start cold (file absent).
        let engine = WhisperEngine::load(&path, WhisperOptions::default());
        assert!(!engine.health().is_available());

        // Still cold even though the path now exists but is empty/invalid.
        std::fs::write(&path, b"not a ggml model").unwrap();
        assert!(!engine.health().is_available());

        // A failed reload keeps the engine cold and doesn't panic.
        assert!(!engine.reload(&path));
        assert!(!engine.health().is_available());
    }

    /// A cold engine whose model file *does* exist advertises Ready: the file
    /// is loadable, it just has not been mapped into RAM yet.
    #[test]
    fn cold_engine_with_file_present_reports_ready() {
        let dir = std::env::temp_dir().join("voice-ptt-whisper-cold-ready");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.bin");
        std::fs::write(&path, b"not a ggml model").unwrap();

        let engine = WhisperEngine::cold(path, WhisperOptions::default());
        assert_eq!(
            engine.health(),
            AsrHealth::Ready,
            "a loadable model file must not look offline to the router"
        );

        // The first transcription triggers the load, records the parse
        // failure, and then correctly reports a failed engine.
        let utt = AudioUtterance {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        };
        assert!(AsrEngine::transcribe(&engine, &utt).is_err());
        assert!(!engine.health().is_available());
    }

    /// A cold engine with no file on disk stays NoModel, not Failed.
    #[test]
    fn cold_engine_without_file_reports_no_model() {
        let dir = std::env::temp_dir().join("voice-ptt-whisper-cold-nomodel");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let engine = WhisperEngine::cold(dir.join("model.bin"), WhisperOptions::default());
        assert_eq!(engine.health(), AsrHealth::NoModel);
    }

    /// Repeated health() calls on a cold engine must be stable and cheap
    /// (each one probes the file only until a load has been attempted).
    #[test]
    fn cold_engine_health_is_stable() {
        let dir = std::env::temp_dir().join("voice-ptt-whisper-cold-stable");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let engine = WhisperEngine::cold(dir.join("model.bin"), WhisperOptions::default());
        assert_eq!(engine.health(), AsrHealth::NoModel);
        assert_eq!(engine.health(), AsrHealth::NoModel);
    }

    /// `cold` and `load` see the same model file; only the eager load maps it.
    #[test]
    fn cold_and_load_report_consistently_for_present_file() {
        let dir = std::env::temp_dir().join("voice-ptt-whisper-cold-vs-load");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.bin");
        std::fs::write(&path, b"not a ggml model").unwrap();

        // Both engines will fail to parse this file, so both end up Failed;
        // the point is they agree after a load attempt.
        let cold = WhisperEngine::cold(path.clone(), WhisperOptions::default());
        assert!(AsrEngine::transcribe(&cold, &AudioUtterance {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        })
        .is_err());
        let eager = WhisperEngine::load(&path, WhisperOptions::default());
        assert!(AsrEngine::transcribe(&eager, &AudioUtterance {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        })
        .is_err());
        assert_eq!(cold.health(), eager.health());
    }
}
