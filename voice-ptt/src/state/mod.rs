//! Application state machine.

pub mod machine;
mod status;

pub use machine::{AppServices, StateMachine};
pub use status::{AppState, AppStatus};
