//! GUI layer: floating overlay and system tray.

pub mod orb;
pub mod orb_animation;
pub mod orb_palette;
pub mod overlay;
pub mod preview_window;
pub mod tray;
pub mod window_shape;

pub use overlay::{OverlayApp, StatusClient};
pub use tray::spawn as spawn_tray;
