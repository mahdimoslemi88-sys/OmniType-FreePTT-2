//! GUI layer: floating overlay and system tray.

pub mod orb;
pub mod orb_animation;
pub mod orb_palette;
pub mod overlay;
pub mod tray;

pub use overlay::{OverlayApp, StatusClient};
pub use tray::spawn as spawn_tray;
