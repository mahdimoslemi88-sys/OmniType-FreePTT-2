//! Output layer: injecting transcribed text into the focused application.

pub mod injector;

pub use injector::{inject_text, press_enter};
