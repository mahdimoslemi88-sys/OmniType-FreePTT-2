//! GUI layer: floating overlay and system tray.

pub mod bootstrap;
pub mod flags;
pub mod orb;
pub mod orb_animation;
pub(crate) mod orb_idle_adapter;
pub mod orb_idle_policy;
pub mod orb_palette;
pub mod overlay;
pub mod preview_window;
pub mod tray;
pub mod tray_warning;
pub mod window_shape;

pub use flags::{DashboardFlags, Toggle};
pub use overlay::{OverlayApp, StatusClient};
pub use tray::spawn as spawn_tray;
