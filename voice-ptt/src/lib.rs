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
pub mod output;
pub mod paths;
pub mod processing;
pub mod state;
pub mod vad;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
    // ---- logging ----------------------------------------------------------
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // ---- settings ---------------------------------------------------------
    let data_dir = dirs_or_cwd();
    let settings = Arc::new(Settings::load_or_create(&data_dir.join("config.toml")).context(
        "failed to load settings",
    )?);
    tracing::info!(?settings.audio, ?settings.vad, "settings loaded");

    // ---- models (the only network access in the app) ----------------------
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let gpu = asr::whisper::detect_gpu();
    let model_name = settings.resolve_model_name(gpu, cores);
    tracing::info!(model = %model_name, gpu = gpu.unwrap_or("none"), "resolved ASR model");

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
    };
    let capture = Arc::new(AudioCapture::new(&capture_cfg).context("failed to open microphone")?);
    tracing::info!(
        native_rate = capture.native_sample_rate(),
        "microphone ready"
    );

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

    // ---- ASR --------------------------------------------------------------
    let opts = WhisperOptions {
        language: settings.asr.language.clone(),
        beam_size: settings.asr.beam_size,
        n_threads: settings.asr.n_threads,
        initial_prompt: settings.asr.initial_prompt.clone(),
        translate: false,
    };
    let whisper = Arc::new(WhisperEngine::load(&model_path, opts));
    if !asr::AsrEngine::health(whisper.as_ref()).is_available() {
        tracing::error!("whisper engine failed to load; transcription will error until fixed");
    }

    // Engine priority: cloud first (opt-in: faster + more accurate), local
    // whisper as the always-available fallback. The router's health/cooldown
    // logic handles the switch automatically.
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
            data_dir.join("cloud_usage.json"),
        )));
    } else if settings.cloud.enabled {
        tracing::warn!(
            "cloud engine enabled but not configured (missing API key); using local whisper only"
        );
    }
    engines.push(whisper);
    let router = AsrRouter::new(engines);

    // ---- text processing ---------------------------------------------------
    let normalizer = Arc::new(Normalizer::new());
    let dictionary = Arc::new(Dictionary::load_or_default(&data_dir.join("dictionary.toml")));
    tracing::info!(rules = dictionary.len(), "dictionary loaded");

    // ---- state machine -----------------------------------------------------
    let machine = Arc::new(StateMachine::new(AppServices {
        capture: capture.clone(),
        vad: tokio::sync::Mutex::new(state::machine::VadUnit::new(
            any_vad,
            vad_cfg,
            settings.audio.sample_rate,
        )),
        router,
        normalizer,
        dictionary,
        settings: settings.clone(),
    }));

    // ---- hotkeys -------------------------------------------------------------
    let (hk_tx, hk_rx) = std::sync::mpsc::channel::<HotkeyEvent>();
    let listener = HotkeyListener::spawn(hk_tx)?;

    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel::<HotkeyEvent>();

    // ---- tray ----------------------------------------------------------------
    let overlay_flag = Arc::new(AtomicBool::new(false));
    let quit_flag = Arc::new(AtomicBool::new(false));
    gui::spawn_tray(events_tx.clone(), overlay_flag.clone(), quit_flag.clone())?;

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
        rt.spawn(async move { machine.run(events_rx).await });
    }

    // ---- GUI (main thread) ------------------------------------------------------
    let status_client = Arc::new(StatusClient::new(machine.subscribe()));
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size([240.0, 120.0]),
        ..Default::default()
    };

    let gui_status = status_client.clone();
    let gui_quit = quit_flag.clone();
    let gui_overlay = overlay_flag.clone();
    eframe::run_native(
        "voice-ptt",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(OverlayApp::new(gui_status.clone(), gui_overlay, gui_quit))
                as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI failed: {e}"))?;

    // GUI closed → clean shutdown.
    listener.stop();
    let _ = events_tx.send(HotkeyEvent::Quit);
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    tracing::info!("bye");
    Ok(())
}
