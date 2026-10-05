//! Application state machine.

pub(crate) mod coordinator;
pub mod machine;
pub(crate) mod machine_run;
pub(crate) mod review;
pub(crate) mod review_channel;
pub(crate) mod session;
mod status;
pub(crate) mod utterance;

pub use machine::{AppServices, StateMachine};
// The review wire, for whoever draws the review window. `ReviewChannel` is
// crate-visible for the same reason `StatusChannel` is: the dashboard and the
// coordinator must share one instance rather than each opening their own.
pub(crate) use review_channel::ReviewChannel;
pub use review_channel::ReviewSnapshot;
// `StatusChannel` is the writer side, kept crate-visible: the mic gate has to
// read the same channel the loop publishes to, and opening a second one would
// be a second truth that can disagree with the orb.
pub(crate) use status::StatusChannel;
pub use status::{AppState, AppStatus};
