//! Output layer: injecting transcribed text into the focused application.

pub mod injector;
pub mod target;

pub use injector::{
    inject_backspaces, inject_backspaces_with, inject_text, inject_text_paced,
    inject_text_paced_with, inject_text_with, press_enter, Injection,
};
pub use target::{
    capture_target, classify, validate_target, Observation, TargetIdentity, TargetTracker,
    TargetValidity,
};
