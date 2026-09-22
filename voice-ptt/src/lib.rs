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
pub mod gui;
pub mod hotkey;
pub mod logging;
pub mod output;
pub mod paths;
pub mod processing;
pub mod state;
pub mod vad;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};

use crate::asr::downloader;
use crate::asr::router::AsrRouter;
use crate::asr::whisper::{WhisperEngine, WhisperOptions};
use crate::audio::{AudioCapture, CaptureConfig};
use crate::config::dirs_or_cwd;
use crate::config::Settings;
use crate::gui::overlay::{OverlayApp, StatusClient};
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
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_OK,
    };

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
            let title: Vec<u16> = "OmniType FreePTT".encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let _ = MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONINFORMATION);
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

/// Progress callback that logs download status.
fn log_progress(phase: &'static str) -> impl Fn(u64, Option<u64>) {
    move |done, total| match total {
        Some(t) => tracing::info!(phase, done, total = t, "download progress"),
        None => tracing::info!(phase, done, "download progress"),
    }
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
    let loaded_settings = Settings::load_or_create(&config_path).context("failed to load settings")?;
    let settings = Arc::new(loaded_settings.clone());
    let settings_rwlock = Arc::new(RwLock::new(loaded_settings));
    tracing::info!(?settings.audio, ?settings.vad, config = %config_path.display(), "settings loaded");
    logging::stage("settings", &format!("config path: {}", config_path.display()));

    // ---- models (the only network access in the app) ----------------------
    // The model NAME is resolved up front, but the (possibly very large)
    // whisper model downloads in the BACKGROUND after the tray is up, so a
    // fresh install is immediately usable (tray, hotkeys, cloud engines)
    // instead of sitting blind on a multi-hundred-MB fetch. Only the tiny
    // Silero VAD model is fetched synchronously here.
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
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
    let listener = HotkeyListener::spawn(hk_tx)?;

    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel::<HotkeyEvent>();
    let overlay_flag = Arc::new(AtomicBool::new(false));
    let dict_flag = Arc::new(AtomicBool::new(false));
    let engine_flag = Arc::new(AtomicBool::new(false));
    let history_flag = Arc::new(AtomicBool::new(false));
    let settings_flag = Arc::new(AtomicBool::new(false));
    let quit_flag = Arc::new(AtomicBool::new(false));
    gui::spawn_tray(
        events_tx.clone(),
        overlay_flag.clone(),
        dict_flag.clone(),
        engine_flag.clone(),
        history_flag.clone(),
        settings_flag.clone(),
        quit_flag.clone(),
    )?;

    // ---- bridge: std channel → tokio channel ---------------------------------
    let bridge_tx = events_tx.clone();
    let bridge_overlay = overlay_flag.clone();
    std::thread::Builder::new()
        .name("hotkey-bridge".into())
        .spawn(move || {
            for ev in hk_rx {
                if matches!(ev, HotkeyEvent::ToggleOverlay) {
                    bridge_overlay.store(true, std::sync::atomic::Ordering::Relaxed);
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
    let whisper = Arc::new(WhisperEngine::load(&model_path, opts));
    let health = asr::AsrEngine::health(whisper.as_ref());
    let engine_ready = health.is_available();
    tracing::info!(?health, ready = engine_ready, "whisper engine status");
    if !engine_ready {
        tracing::info!("whisper model not loaded yet; background download will hot-reload it");
    }
    logging::stage("asr", "whisper engine initialized");

    // Engine priority: cloud first (if configured with API key), then Google Free Speech
    // (no key needed, fast online), custom providers, then local whisper as the always-available fallback.
    let usage_path = paths::resolve_usage_path();
    let mut engines: Vec<Arc<dyn asr::AsrEngine>> = Vec::new();
    if settings.cloud.is_configured() {
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

    engines.push(whisper.clone());
    let router = AsrRouter::new_with_active(engines, settings.active_engine.clone());

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
            // A panic inside the machine must never die silently again:
            // catch it, log it (the panic hook also writes crash.log).
            let result = tokio::task::spawn(std::panic::AssertUnwindSafe(async move {
                machine.run(events_rx).await
            }))
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!(error = %e, "state machine exited with error"),
                Err(join_err) => tracing::error!(
                    error = %join_err,
                    "state machine task PANICKED — hotkeys/transcription are down until restart"
                ),
            }
        });
    }

    logging::stage("gui", "entering GUI event loop");

    // ---- GUI (main thread) ------------------------------------------------------
    let status_client = Arc::new(StatusClient::new(machine.subscribe()));
    let (icon_rgba, icon_w, icon_h) = gui::tray::app_icon_rgba();
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_visible(settings.gui.show_overlay)
            .with_inner_size([38.0, 6.0])
            .with_title("OmniType")
            .with_icon(std::sync::Arc::new(eframe::egui::IconData {
                rgba: icon_rgba,
                width: icon_w,
                height: icon_h,
            })),
        ..Default::default()
    };

    let gui_status = status_client.clone();
    let gui_quit = quit_flag.clone();
    let gui_overlay = overlay_flag.clone();
    let gui_dict_flag = dict_flag.clone();
    let gui_engine_flag = engine_flag.clone();
    let gui_history_flag = history_flag.clone();
    let gui_settings_flag = settings_flag.clone();
    let gui_dict = dictionary.clone();
    let gui_router = router.clone();
    let gui_settings = settings_rwlock.clone();
    let gui_config_path = config_path.clone();
    let gui_events_tx = events_tx.clone();
    eframe::run_native(
        "voice-ptt",
        native_options,
        Box::new(move |cc| {
            // Load native Windows fonts with 100% complete Persian/Arabic glyph coverage
            let mut fonts = eframe::egui::FontDefinitions::default();
            if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
                fonts.font_data.insert(
                    "segoe_ui".to_owned(),
                    eframe::egui::FontData::from_owned(data),
                );
                fonts
                    .families
                    .entry(eframe::egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "segoe_ui".to_owned());
            }
            if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\tahoma.ttf") {
                fonts.font_data.insert(
                    "tahoma".to_owned(),
                    eframe::egui::FontData::from_owned(data),
                );
                fonts
                    .families
                    .entry(eframe::egui::FontFamily::Proportional)
                    .or_default()
                    .push("tahoma".to_owned());
            }
            if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
                fonts.font_data.insert(
                    "segoe_ui_symbol".to_owned(),
                    eframe::egui::FontData::from_owned(data),
                );
                fonts
                    .families
                    .entry(eframe::egui::FontFamily::Proportional)
                    .or_default()
                    .push("segoe_ui_symbol".to_owned());
            }
            // Phosphor icon font — appended to the *same* definitions so it
            // joins the fallback chain after Segoe UI: glyphs missing from the
            // system fonts (the PUA icon codepoints) resolve to it, while
            // Persian coverage from Segoe UI stays intact.
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(OverlayApp::new(
                gui_status.clone(),
                gui_events_tx,
                gui_overlay,
                gui_dict_flag,
                gui_engine_flag,
                gui_history_flag,
                gui_settings_flag,
                gui_quit,
                gui_dict,
                gui_router,
                gui_settings,
                gui_config_path,
            )) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI failed: {e}"))?;

    // GUI closed → clean shutdown.
    listener.stop();
    let _ = events_tx.send(HotkeyEvent::Quit);
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    logging::log_session_end(started);
    tracing::info!("bye");
    Ok(())
}
