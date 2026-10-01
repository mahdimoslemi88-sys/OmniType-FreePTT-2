//! voice-ptt — high-performance Push-to-Talk voice typing for Windows.
//!
//! Pipeline: WASAPI shared capture → lock-free ring buffer → VAD endpointing
//! (Silero/RMS) → whisper.cpp → Persian normalizer → dictionary → SendInput.
//!
//! This crate is organized as a library so components are unit-testable
//! without spawning the GUI.

pub mod asr;
pub mod audio;
pub mod config;
pub mod doctor;
pub mod gui;
pub mod hotkey;
pub mod logging;
pub mod output;
pub mod paths;
pub mod processing;
pub mod state;
pub mod updates;
pub mod vad;

use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};

use crate::asr::downloader;
use crate::asr::router::AsrRouter;
use crate::asr::whisper::{WhisperEngine, WhisperOptions};
use crate::audio::{AudioCapture, CaptureConfig};
use crate::config::dirs_or_cwd;
use crate::config::Settings;

use crate::hotkey::{HotkeyEvent, HotkeyListener};
use crate::processing::{Dictionary, Normalizer};
use crate::state::machine::AppServices;
use crate::state::StateMachine;
use crate::vad::{AnyVad, VadConfig};

/// Prevents a second instance from racing the first on model downloads
/// (`.part` file corruption) and text injection (every dictation typed
/// twice). On Windows this is a named mutex held for the process lifetime;
/// the second instance shows a short message and exits. Other platforms
/// are unaffected.
#[cfg(windows)]
mod single_instance {
    use anyhow::{Context, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    /// Owns the instance mutex; dropping it releases the instance.
    pub struct Guard(HANDLE);

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn acquire() -> Result<Guard> {
        const NAME: &str = "Local\\OmniTypeFreePTT.SingleInstance";
        let wide: Vec<u16> = NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(wide.as_ptr())) }
            .context("failed to create single-instance mutex")?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // Tell the user why "nothing happened" instead of exiting blind.
            const MSG: &str = "OmniType FreePTT is already running.\r\n\
                Check the system tray for its capsule icon.";
            let text: Vec<u16> = MSG.encode_utf16().chain(std::iter::once(0)).collect();
            let title: Vec<u16> = "OmniType FreePTT"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                let _ = MessageBoxW(
                    None,
                    PCWSTR(text.as_ptr()),
                    PCWSTR(title.as_ptr()),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            anyhow::bail!("another instance is already running");
        }
        Ok(Guard(handle))
    }
}

#[cfg(not(windows))]
mod single_instance {
    use anyhow::Result;

    /// No-op guard on non-Windows platforms.
    pub struct Guard;

    pub fn acquire() -> Result<Guard> {
        Ok(Guard)
    }
}

/// Whether the cloud API key came from the environment rather than the config
/// file. Read once, at the edge, so the engine planner stays pure and testable.
fn cloud_key_in_env() -> bool {
    std::env::var("VOICE_PTT_CLOUD_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

/// Progress callback that logs download status.
fn log_progress(phase: &'static str) -> impl Fn(u64, Option<u64>) {
    move |done, total| match total {
        Some(t) => tracing::info!(phase, done, total = t, "download progress"),
        None => tracing::info!(phase, done, "download progress"),
    }
}

/// Whether this run is a diagnostic request rather than a normal launch.
///
/// `main.rs` builds with `#![windows_subsystem = "windows"]`, so there is no
/// stdout and a flag would otherwise be invisible. The single argument form is
/// checked on the process command line instead.
pub fn doctor_requested() -> bool {
    std::env::args().skip(1).any(|a| a == "--doctor")
}

/// Writes the diagnostic report and returns it, without starting the app.
///
/// Must stay free of the things that stop a real startup: no microphone, no
/// model mapping, no window. It is needed exactly when the app cannot start.
pub fn run_doctor() -> std::path::PathBuf {
    let config_path = paths::resolve_config_path();
    let settings = Settings::load_or_create(&config_path).unwrap_or_default();
    let hotkeys = HotkeyListener::config_from_settings(&settings.hotkey);
    let diagnosis = doctor::diagnose(&settings, &hotkeys, cloud_key_in_env(), &config_path);
    doctor::write_default(&diagnosis)
}

/// Bootstraps the whole application. Blocks until the GUI closes.
pub fn run() -> Result<()> {
    let started = std::time::Instant::now();

    // ---- logging ----------------------------------------------------------
    // First thing we do: durable file + console logging. From here on every
    // event (including native whisper.cpp logs and panics) is observable in
    // <data dir>\logs\ even for console-less double-click launches.
    logging::init(&dirs_or_cwd());
    logging::log_session_start();

    // ---- single instance ------------------------------------------------------
    // Must come before everything else: two instances would fight over the
    // model download and inject every dictation twice.
    let _single_instance = single_instance::acquire()?;
    tracing::info!("single-instance lock acquired");

    // Route native whisper.cpp / ggml stderr logs into `tracing` (and thus
    // into the log file) — plain stderr is invisible in a GUI app.
    logging::install_whisper_log_redirect();

    // ---- settings ---------------------------------------------------------
    let _data_dir = dirs_or_cwd();
    let config_path = paths::resolve_config_path();
    let loaded_settings =
        Settings::load_or_create(&config_path).context("failed to load settings")?;
    let settings = Arc::new(loaded_settings.clone());
    let settings_rwlock = Arc::new(RwLock::new(loaded_settings));
    tracing::info!(?settings.audio, ?settings.vad, config = %config_path.display(), "settings loaded");
    logging::stage(
        "settings",
        &format!("config path: {}", config_path.display()),
    );

    // ---- models (the only network access in the app) ----------------------
    // The model NAME is resolved up front, but the (possibly very large)
    // whisper model downloads in the BACKGROUND after the tray is up, so a
    // fresh install is immediately usable (tray, hotkeys, cloud engines)
    // instead of sitting blind on a multi-hundred-MB fetch. Only the tiny
    // Silero VAD model is fetched synchronously here.
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let gpu = asr::whisper::detect_gpu();
    let model_name = settings.resolve_model_name(gpu, cores);
    tracing::info!(model = %model_name, gpu = gpu.unwrap_or("none"), "resolved ASR model");
    logging::stage("models", &format!("resolved model: {model_name}"));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?;

    #[cfg(feature = "silero-vad")]
    let vad_path: Option<std::path::PathBuf> = {
        let assets_dir = paths::resolve_assets_dir();
        tracing::info!(assets = %assets_dir.display(), "assets directory resolved");
        rt.block_on(async {
            downloader::ensure_vad_model(&assets_dir, Some(&log_progress("vad")))
                .await
                .ok()
        })
    };
    #[cfg(not(feature = "silero-vad"))]
    let vad_path: Option<std::path::PathBuf> = None;

    // ---- tray + hotkeys (before any large download) ---------------------------
    // Spawned before the whisper download so first-launch users see the tray
    // right away; the big model streams in behind the UI.
    let (hk_tx, hk_rx) = std::sync::mpsc::channel::<HotkeyEvent>();
    let hotkey_config = HotkeyListener::config_from_settings(&settings.hotkey);
    // A hotkey the settings could not supply is replaced by the built-in
    // default so push-to-talk still works, but the user gets a working app on
    // the wrong key. Written to a readable file, not just a log nobody opens.
    let diagnosis = doctor::diagnose(&settings, &hotkey_config, cloud_key_in_env(), &config_path);
    let report_path = doctor::write_default(&diagnosis);
    let tray_warning = gui::tray_warning::TrayWarning::new(&diagnosis, report_path.clone());
    if diagnosis.verdict != doctor::Verdict::Clean {
        tracing::warn!(
            report = %report_path.display(),
            problems = diagnosis.problems.len(),
            "the configuration is not being used as written — see the diagnostic report"
        );
    }
    let listener = HotkeyListener::spawn_with_config(hk_tx, hotkey_config)?;
    // Handle the dashboard keeps: live re-bind of shortcuts and the
    // system-wide key capture used by the settings UI.
    let hotkey_control = listener.control();

    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel::<HotkeyEvent>();
    // One handle, cloned to the tray, the hotkey bridge and the GUI. They all
    // raise the same six requests; see `gui::flags` for why they are grouped.
    let flags = gui::DashboardFlags::new();
    let update_state = updates::new_shared_state();
    gui::spawn_tray(
        events_tx.clone(),
        flags.clone(),
        update_state.clone(),
        tray_warning.clone(),
    )?;

    // Spawn background update checker (honors settings.updates.check_on_startup).
    // Spawned onto the runtime handle (tokio::spawn needs runtime context).
    updates::spawn_background_checker(
        &rt,
        update_state.clone(),
        settings_rwlock.clone(),
        env!("CARGO_PKG_VERSION"),
    );

    // ---- bridge: std channel → tokio channel ---------------------------------
    let bridge_tx = events_tx.clone();
    let bridge_overlay = flags.clone();
    std::thread::Builder::new()
        .name("hotkey-bridge".into())
        .spawn(move || {
            for ev in hk_rx {
                if let Some(toggle) = gui::Toggle::for_hotkey_event(&ev) {
                    bridge_overlay.raise(toggle);
                }
                if bridge_tx.send(ev).is_err() {
                    break; // machine gone → shutting down
                }
            }
        })?;

    // ---- audio ------------------------------------------------------------
    let capture_cfg = CaptureConfig {
        sample_rate: settings.audio.sample_rate,
        channels: settings.audio.channels,
        buffer_frames: settings.audio.buffer_frames,
        ring_seconds: settings.audio.ring_seconds,
        device_name: if settings.audio.device == "default" {
            None
        } else {
            Some(settings.audio.device.clone())
        },
        gain_db: settings.audio.gain_db,
    };
    let capture = Arc::new(AudioCapture::new(&capture_cfg).context("failed to open microphone")?);
    tracing::info!(
        native_rate = capture.native_sample_rate(),
        "microphone ready"
    );
    logging::stage("audio", "microphone ready");

    // ---- VAD --------------------------------------------------------------
    let vad_cfg = VadConfig {
        threshold: settings.vad.threshold,
        silence_timeout_ms: settings.vad.silence_timeout_ms,
        min_speech_ms: settings.vad.min_speech_ms,
        speech_start_probability: settings.vad.threshold,
    };
    let vad_probe = vad_path
        .clone()
        .unwrap_or_else(|| paths::resolve_assets_dir().join("silero_vad.onnx"));
    let (any_vad, vad_kind) = AnyVad::auto(&vad_probe, &vad_cfg);
    tracing::info!(?vad_kind, "VAD engine selected");
    logging::stage("vad", &format!("engine: {vad_kind:?}"));

    // ---- ASR --------------------------------------------------------------
    // The model file may not exist yet on a fresh install: the engine loads
    // cold and the background task (after the state machine below) hot-reloads
    // it the moment the download completes.
    let opts = WhisperOptions {
        language: settings.asr.language.clone(),
        beam_size: settings.asr.beam_size,
        n_threads: settings.asr.n_threads,
        initial_prompt: settings.asr.initial_prompt.clone(),
        translate: false,
    };
    let models_dir = paths::resolve_models_dir();
    tracing::info!(models = %models_dir.display(), "models directory resolved");
    let model_path = models_dir.join(format!("ggml-{model_name}.bin"));
    // Cold start: the ggml model file (up to ~1.6 GB for large-v3-turbo) is
    // deliberately NOT loaded here. `WhisperEngine::cold` maps it only on the
    // first transcription that actually routes to local ASR, so a user on a
    // cloud engine (Google/Groq/…) never pays that memory cost at all.
    let whisper = Arc::new(WhisperEngine::cold(model_path.clone(), opts));
    let health = asr::AsrEngine::health(whisper.as_ref());
    let engine_ready = health.is_available();
    tracing::info!(?health, ready = engine_ready, "whisper engine status");
    if !engine_ready {
        tracing::info!("whisper model not loaded yet; background download will hot-reload it");
    }
    logging::stage("asr", "whisper engine initialized");

    // Engine priority: cloud first (if configured with API key), then Google Free Speech
    // (no key needed, fast online), custom providers, then local whisper as the always-available fallback.
    // Which engines exist, in what order, is decided by `asr::plan`: a pure
    // function over the settings with its own unit tests, instead of sixty
    // lines of interleaved logging and `push` that nothing could check. The
    // environment read it needs happens here, at the one place that may touch
    // the environment.
    let plan = asr::plan::engine_plan(&settings, cloud_key_in_env());
    let want_cloud = plan.cloud_provider().is_some();
    let want_custom: Vec<String> = plan.custom_ids().iter().map(|s| s.to_string()).collect();
    let want_antigravity = plan.wants_antigravity_probe();

    let usage_path = paths::resolve_usage_path();
    let mut engines: Vec<Arc<dyn asr::AsrEngine>> = Vec::new();
    if want_cloud {
        tracing::info!(
            provider = %settings.cloud.provider,
            model = %settings.cloud.model,
            daily_limit = settings.cloud.daily_limit,
            "cloud ASR engine enabled"
        );
        engines.push(Arc::new(asr::CloudEngine::new(
            settings.cloud.clone(),
            usage_path.clone(),
        )));
    } else if settings.cloud.enabled {
        tracing::warn!(
            "cloud engine enabled but not configured (missing API key); checking other engines"
        );
    }

    if settings.google.enabled {
        tracing::info!(
            language = %settings.google.language,
            "Google Free Speech ASR engine enabled (no API key required)"
        );
        engines.push(Arc::new(asr::GoogleEngine::new(settings.google.clone())));
    }

    // Register user-defined custom cloud / local API endpoints
    for custom in &settings.custom_providers {
        // The plan is the single source of truth for which providers exist.
        if !want_custom.iter().any(|id| id == &custom.id) {
            continue;
        }
        tracing::info!(
            id = %custom.id,
            name = %custom.name,
            model = %custom.model,
            "custom ASR provider registered"
        );
        engines.push(Arc::new(asr::CloudEngine::new_custom(
            custom,
            usage_path.clone(),
        )));
    }

    // Antigravity live dictation: borrow the cloud speech-to-text of a locally
    // running Antigravity over loopback (gRPC-Web + the CSRF token from its own
    // command line). It never outranks the engines above in auto mode — select
    // «Antigravity Live» explicitly, or let auto fall through to it.
    let mut antigravity_probe: Option<Arc<asr::AntigravityEngine>> = None;
    if want_antigravity {
        let antigravity = Arc::new(asr::AntigravityEngine::new(settings.antigravity.clone()));
        tracing::info!(
            ready_timeout_secs = settings.antigravity.ready_timeout_secs,
            "Antigravity live dictation engine enabled (requires a running Antigravity); \
             note: a session takes ~13 s to open on this machine"
        );
        antigravity_probe = Some(antigravity.clone());
        engines.push(antigravity);
    }

    engines.push(whisper.clone());
    let router = AsrRouter::new_with_active(engines, settings.active_engine.clone());
    // The local whisper model commits ~1.6 GB when it runs, so the `auto` chain
    // only falls through to it when the user explicitly allows that.
    router.set_allow_local_fallback(settings.asr.auto_local_fallback);

    // A selection naming an engine that never registered is the one ASR failure
    // the user cannot see: `transcribe` refuses to fall back (by design), so
    // every dictation errors and the only clue is a log line nobody opens. Say
    // it at startup instead.
    if plan.selection == asr::plan::ActiveSelection::Missing {
        tracing::warn!(
            selected = %settings.active_engine,
            registered = ?plan.engines.iter().map(|e| e.id()).collect::<Vec<_>>(),
            "the selected ASR engine is not registered — dictation will fail until it is \
             re-enabled or the selection is changed to auto"
        );
    }

    // Antigravity discovery spawns PowerShell + netstat (a real subprocess per
    // probe), so it must stay off the UI thread — and phase 2 gates it on the
    // engine actually being selected: probing forever for an engine the user
    // never chose was measurable churn (handles/threads/private bytes jumping
    // every 30–60 s). Re-probe while unavailable because the app may start later.
    if let Some(probe) = antigravity_probe {
        let probe_router = router.clone();
        std::thread::spawn(move || loop {
            let selected = probe_router.active_engine() == "antigravity";
            if selected {
                probe.maintain();
            }
            let pause = asr::plan::probe_pause_secs(
                selected,
                asr::AsrEngine::health(probe.as_ref()).is_available(),
            );
            std::thread::sleep(std::time::Duration::from_secs(pause));
        });
    }

    // ---- text processing ---------------------------------------------------
    let normalizer = Arc::new(Normalizer::new());
    let dictionary = Arc::new(RwLock::new(Dictionary::load_or_create(
        &paths::resolve_dictionary_path(),
    )));
    tracing::info!(
        rules = dictionary.read().map(|d| d.len()).unwrap_or(0),
        path = %paths::resolve_dictionary_path().display(),
        "dictionary loaded and ready"
    );
    logging::stage("text-processing", "normalizer + dictionary ready");

    // ---- state machine -----------------------------------------------------
    let machine = Arc::new(StateMachine::new(AppServices {
        capture: capture.clone(),
        vad: tokio::sync::Mutex::new(state::machine::VadUnit::new(
            any_vad,
            vad_cfg,
            settings.audio.sample_rate,
        )),
        router: router.clone(),
        normalizer,
        dictionary: dictionary.clone(),
        settings: settings.clone(),
    }));

    // Streaming engines (Antigravity) publish partial transcripts while they
    // work; route them to the capsule through the state machine.
    {
        let machine = machine.clone();
        asr::progress::set_sink(Arc::new(move |text: &str| machine.publish_partial(text)));
    }

    // ---- hotkeys were registered above (tray-first startup) -------------------

    // ---- whisper model: background download + hot reload ----------------------
    // The tray and hotkeys are already live (see the tray-first startup), so
    // a first launch never sits blind on a multi-hundred-MB download. Cloud
    // engines (Google Free Speech etc.) serve transcriptions meanwhile; the
    // local engine is hot-swapped the moment the model file completes.
    if !engine_ready {
        let engine = whisper.clone();
        let models_dir = models_dir.clone();
        let model_name = model_name.clone();
        rt.spawn(async move {
            let progress_cb = log_progress("whisper");
            match downloader::ensure_model(&models_dir, &model_name, Some(&progress_cb)).await {
                Ok(path) => {
                    if engine.reload(&path) {
                        tracing::info!(
                            model = %path.display(),
                            "whisper model ready (background download)"
                        );
                        logging::stage("models", "model file ready");
                    }
                }
                Err(e) => tracing::error!(
                    error = %e,
                    "whisper model download failed; local engine stays offline (cloud engines still work)"
                ),
            }
        });
    }

    // ---- run the state machine ------------------------------------------------
    {
        let machine = machine.clone();
        rt.spawn(async move {
            // The panic guard is `state::machine_run::run_guarded`, and it is
            // unit-tested: a panic in the machine now has a test, which the
            // inline `match` this replaced never did.
            let exit = state::machine_run::run_guarded(machine.run(events_rx)).await;
            match exit.level() {
                logging::Severity::Info => tracing::info!("{}", exit.message()),
                logging::Severity::Warn => tracing::warn!("{}", exit.message()),
                logging::Severity::Error => tracing::error!("{}", exit.message()),
            }
        });
    }

    logging::stage("gui", "entering GUI event loop");

    // ---- GUI (main thread) ------------------------------------------------------
    // Window geometry, fonts and visuals live in `gui::bootstrap` so they can
    // be unit-tested; this only hands over the live dependencies. `machine` is
    // still borrowed here for its background download + run loop above.
    gui::bootstrap::run_gui(gui::bootstrap::GuiStartup {
        status: machine.subscribe(),
        settings: settings.clone(),
        settings_rwlock: settings_rwlock.clone(),
        config_path: config_path.clone(),
        flags: flags.clone(),
        events_tx: events_tx.clone(),
        update_state: update_state.clone(),
        hotkey_control: hotkey_control.clone(),
        boot_warning: tray_warning.clone(),
        dictionary: dictionary.clone(),
        router: router.clone(),
    })?;

    // GUI closed → clean shutdown.
    listener.stop();
    let _ = events_tx.send(HotkeyEvent::Quit);
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    logging::log_session_end(started);
    tracing::info!("bye");
    Ok(())
}
