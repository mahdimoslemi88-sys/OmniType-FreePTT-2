//! Hotkey layer: global key detection for push-to-talk.

pub mod binding;
pub mod listener;

pub use binding::{HotkeyBinding, HotkeyParseError};
pub use listener::{HotkeyEvent, HotkeyListener};
