//! Output layer: injecting transcribed text into the focused application.

pub mod injector;
pub mod target;

pub use injector::{inject_backspaces, inject_text, press_enter};
pub use target::{TargetIdentity, TargetTracker, TargetValidity};
