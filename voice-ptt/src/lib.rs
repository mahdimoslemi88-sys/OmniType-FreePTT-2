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
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let gpu = asr::whisper::detect_gpu();
    let model_name = settings.resolve_model_name(gpu, cores);
    tracing::info!(model = %model_name, gpu = gpu.unwrap_or("none"), "resolved ASR model");
    logging::stage("models", &format!("resolved model: {model_name}"));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?;

    let (model_path, vad_path) = {
        let models_dir = paths::resolve_models_dir();
        let assets_dir = paths::resolve_assets_dir();
        tracing::info!(
            models = %models_dir.display(),
            assets = %assets_dir.display(),
            "data directories resolved"
        );
        rt.block_on(async move {
            let model_path =
                downloader::ensure_model(&models_dir, &model_name, Some(&log_progress("whisper")))
                    .await
                    .context("whisper model unavailable (download failed?)");
            #[cfg(feature = "silero-vad")]
            let vad_path =
                downloader::ensure_vad_model(&assets_dir, Some(&log_progress("vad")))
                    .await
                    .ok();
            #[cfg(not(feature = "silero-vad"))]
            let vad_path: Option<std::path::PathBuf> = None;
            (model_path, vad_path)
        })
    };
    let model_path = model_path?;
    tracing::info!(model = %model_path.display(), "whisper model ready");
    logging::stage("models", "model file ready");

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
    let opts = WhisperOptions {
        language: settings.asr.language.clone(),
        beam_size: settings.asr.beam_size,
        n_threads: settings.asr.n_threads,
        initial_prompt: settings.asr.initial_prompt.clone(),
        translate: false,
    };
    let whisper = Arc::new(WhisperEngine::load(&model_path, opts));
    let health = asr::AsrEngine::health(whisper.as_ref());
    tracing::info!(?health, "whisper engine status");
    if !health.is_available() {
        tracing::error!("whisper engine failed to load; transcription will error until fixed");
    }
    logging::stage("asr", "whisper engine loaded");

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

    engines.push(whisper);
    let router = AsrRouter::new_with_active(engines, settings.active_engine.clone());

    // ---- text processing ---------------------------------------------------
    let normalizer = Arc::new(Normalizer::new());
    let dict_path = paths::resolve_dictionary_path();
    let dictionary = Arc::new(RwLock::new(Dictionary::load_or_create(&dict_path)));
    tracing::info!(
        rules = dictionary.read().map(|d| d.len()).unwrap_or(0),
        path = %dict_path.display(),
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

    // ---- hotkeys -------------------------------------------------------------
    let (hk_tx, hk_rx) = std::sync::mpsc::channel::<HotkeyEvent>();
    let listener = HotkeyListener::spawn(hk_tx)?;

    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel::<HotkeyEvent>();

    // ---- tray ----------------------------------------------------------------
    let overlay_flag = Arc::new(AtomicBool::new(false));
    let dict_flag = Arc::new(AtomicBool::new(false));
    let engine_flag = Arc::new(AtomicBool::new(false));
    let history_flag = Arc::new(AtomicBool::new(false));
    let quit_flag = Arc::new(AtomicBool::new(false));
    gui::spawn_tray(
        events_tx.clone(),
        overlay_flag.clone(),
        dict_flag.clone(),
        engine_flag.clone(),
        history_flag.clone(),
        quit_flag.clone(),
        dict_path,
        config_path.clone(),
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
