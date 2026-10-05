//! Shared fixtures for the overlay's tests.
//!
//! Building an [`OverlayApp`] takes fourteen constructor arguments, so before
//! this module every test that needed one pasted the same forty-five lines.
//! With panels living in their own modules, that boilerplate would have been
//! copied into each of them — so it lives here once, and a panel test states
//! only what it actually cares about.
//!
//! Nothing here talks to the network, the registry or the real filesystem: the
//! config path is a relative literal and the ASR router has no engines, so a
//! panel test cannot accidentally depend on the developer's machine.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use eframe::egui;

use super::history_panel::HistoryItem;
use super::{AsrRouter, Dictionary, OverlayApp, Settings, StatusClient};
use crate::gui::flags::DashboardFlags;
use crate::state::{AppState, AppStatus};

/// An idle app with every window closed and no history.
///
/// This is the baseline: open exactly the window under test, set exactly the
/// state that window cares about, and leave the other 62 fields alone.
pub(crate) fn app() -> OverlayApp {
    let (_tx, rx) = tokio::sync::watch::channel(AppStatus {
        state: AppState::Idle,
        last_text: None,
        vad_engine: "silero",
        partial: None,
        latched: false,
        chunk_busy: false,
    });
    let (events_tx, _events_rx) = tokio::sync::mpsc::unbounded_channel();

    OverlayApp::new(
        Arc::new(StatusClient::new(rx)),
        events_tx,
        DashboardFlags::new(),
        Arc::new(RwLock::new(Dictionary::with_defaults())),
        AsrRouter::new(vec![]),
        Arc::new(RwLock::new(Settings::default())),
        PathBuf::from("config.toml"),
        crate::updates::new_shared_state(),
        None,
        None,
        Arc::new(crate::audio::gate::LiveMicGate::new(Arc::new(
            crate::state::StatusChannel::new("silero"),
        ))),
        crate::state::ReviewChannel::new(),
    )
}

/// [`app`] with the dashboard open on one history row, so panels that render
/// an empty list never exercise their populated branch.
pub(crate) fn app_with_history() -> OverlayApp {
    let mut app = app();
    app.show_dashboard = true;
    app.history.push(HistoryItem {
        id: 1,
        text: "نمونه متن تستی در تاریخچه".into(),
        timestamp: "12:00:00".into(),
        engine: "google".into(),
    });
    app
}

/// Pointer positions used by [`sweep`]: a coarse 5x5 grid, not the corners
/// only, because egui lays out lazily and a widget is only built when the
/// pointer has been somewhere its rect could be.
const SWEEP_POSITIONS: [f32; 5] = [50.0, 150.0, 300.0, 450.0, 600.0];

/// Runs `frame` once per position on a 5x5 pointer grid.
///
/// A single pass can miss a panic: egui only builds the widgets under the
/// pointer, so hover-dependent code (tooltips, `on_hover_text`, context menus)
/// is never reached unless the pointer actually travels. Twenty-five passes is
/// the cheapest way to make "does not panic" mean what it says.
pub(crate) fn sweep(mut frame: impl FnMut(&mut OverlayApp, &egui::Context)) {
    let ctx = egui::Context::default();
    let mut app = app_with_history();
    for x in SWEEP_POSITIONS {
        for y in SWEEP_POSITIONS {
            let mut input = egui::RawInput::default();
            input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(x, y)));
            let _ = ctx.run(input, |ctx| frame(&mut app, ctx));
        }
    }
}
