//! GUI layer: floating overlay and system tray.

pub mod overlay;
pub mod tray;

pub use overlay::{OverlayApp, StatusClient};
pub use tray::{spawn as spawn_tray, TrayCommand};
