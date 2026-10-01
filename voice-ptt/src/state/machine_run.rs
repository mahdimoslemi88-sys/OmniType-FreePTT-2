//! Running the state machine, and surviving how it ends.
//!
//! `StateMachine::run` is a loop that owns the microphone, the router and the
//! event channel. It cannot be unit-tested: a test would need a real capture
//! device and a real engine. What *can* be tested, and what used not to be, is
//! the wrapper around it — the part that exists so that a panic inside the
//! machine cannot take the process down silently, which is a bug this app had.
//!
//! That wrapper was thirty lines of nested `match` in `run()` with no caller
//! that could exercise it. The panic path in particular was asserted by
//! *comment*: "A panic inside the machine must never die silently again".
//!
//! [`run_guarded`] is that comment, written as code that a test can call. It
//! runs the future on a separate task, so a panic becomes a `JoinError` instead
//! of unwinding into the runtime, and it reports which of the three ways the
//! machine ended. The tests below drive all three, including a real panic.

use std::future::Future;
use std::panic::AssertUnwindSafe;

use crate::logging::Severity;

/// How the state machine's run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineExit {
    /// The event channel closed or a quit arrived: the ordinary end.
    Clean,
    /// The machine returned an error.
    Failed(String),
    /// The machine panicked. The task was caught; the app is still running but
    /// push-to-talk and transcription are down until restart.
    Panicked(String),
}

impl MachineExit {
    /// The severity this ending deserves.
    ///
    /// Read off [`Self::machine_is_live`] rather than off the variants: a panic
    /// is not louder than an error — it is a different failure, and one the
    /// user cannot diagnose — so the level is driven by the single question
    /// that actually matters, and the wording of [`Self::message`] does the
    /// distinguishing.
    pub fn level(&self) -> Severity {
        if self.machine_is_live() {
            Severity::Info
        } else {
            Severity::Error
        }
    }

    /// One line describing the ending, without the level decoration.
    pub fn message(&self) -> String {
        match self {
            MachineExit::Clean => "state machine exited".to_string(),
            MachineExit::Failed(e) => format!("state machine exited with error: {e}"),
            MachineExit::Panicked(e) => format!(
                "state machine task PANICKED — hotkeys/transcription are down until restart: {e}"
            ),
        }
    }

    /// Whether push-to-talk still works.
    ///
    /// The one question a caller has after this returns. `Clean` is the only
    /// yes: after an error or a panic the loop is gone, and nothing restarts it.
    pub fn machine_is_live(&self) -> bool {
        matches!(self, MachineExit::Clean)
    }
}

/// Runs `fut` on its own task and reports how it ended.
///
/// The `spawn` is the whole point: an `async fn` awaited directly unwinds a
/// panic into its caller, so a bug in the loop would kill whichever task happened
/// to be polling — silently, since `run()` was a `tokio::spawn` whose result
/// nobody looked at. Spawned, the panic is caught and arrives here as a
/// `JoinError`.
///
/// `AssertUnwindSafe` is required and safe: the state machine holds no
/// invariant this guard would protect. The panic is not repaired — the machine
/// is left dead, and [`MachineExit::machine_is_live`] says so.
pub async fn run_guarded<F, E>(fut: F) -> MachineExit
where
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    match tokio::task::spawn(AssertUnwindSafe(fut)).await {
        Ok(Ok(())) => MachineExit::Clean,
        Ok(Err(e)) => MachineExit::Failed(e.to_string()),
        Err(join_err) => MachineExit::Panicked(join_err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_machine_that_returns_ok_exits_clean() {
        let exit = run_guarded(async { Ok::<(), String>(()) }).await;
        assert_eq!(exit, MachineExit::Clean);
        assert!(exit.machine_is_live());
        assert_eq!(exit.level(), Severity::Info);
    }

    /// The ordinary failure: an error, not a panic.
    #[tokio::test]
    async fn a_machine_that_errors_is_reported_with_the_cause() {
        let exit = run_guarded(async { Err::<(), String>("router exploded".into()) }).await;
        assert_eq!(exit, MachineExit::Failed("router exploded".into()));
        assert!(!exit.machine_is_live());
        assert_eq!(exit.level(), Severity::Error);
        assert!(
            exit.message().contains("router exploded"),
            "{}",
            exit.message()
        );
    }

    /// The reason this function exists, and the bug it is here to prevent: a
    /// panic in the machine must arrive here as a value, not unwind into the
    /// caller. Before it was extracted, this path had no test at all — the
    /// guarantee was a comment.
    #[tokio::test]
    async fn a_panicking_machine_is_caught_not_propagated() {
        let exit = run_guarded(async {
            panic!("microphone driver exploded");
            #[allow(unreachable_code)]
            Ok::<(), String>(())
        })
        .await;
        assert!(
            matches!(exit, MachineExit::Panicked(_)),
            "the panic escaped the guard: {exit:?}"
        );
        assert!(!exit.machine_is_live());
        assert_eq!(exit.level(), Severity::Error);
    }

    /// The panic message has to reach the log, or the guard converts a visible
    /// crash into an unexplained silence — the exact failure it was written to
    /// prevent.
    #[tokio::test]
    async fn the_panic_message_survives_into_the_report() {
        let exit = run_guarded(async {
            panic!("microphone driver exploded");
            #[allow(unreachable_code)]
            Ok::<(), String>(())
        })
        .await;
        let message = exit.message();
        assert!(message.contains("PANICKED"), "{message}");
        assert!(
            message.contains("microphone driver exploded"),
            "the cause was dropped: {message}"
        );
        assert!(
            message.contains("until restart"),
            "the user must know what is broken: {message}"
        );
    }

    /// Only the ordinary ending leaves a working push-to-talk.
    #[tokio::test]
    async fn only_a_clean_exit_leaves_the_machine_live() {
        let endings = [
            MachineExit::Clean,
            MachineExit::Failed("e".into()),
            MachineExit::Panicked("p".into()),
        ];
        let live: Vec<bool> = endings.iter().map(MachineExit::machine_is_live).collect();
        assert_eq!(live, vec![true, false, false]);
    }
}
