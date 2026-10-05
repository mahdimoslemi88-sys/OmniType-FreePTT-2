//! The real [`MicUseGate`]: the only thing in the program allowed to hand the
//! microphone to a diagnostic test.
//!
//! [`crate::audio::diagnostics`] defines the ownership contract as types and a
//! trait, and deliberately stops there — it cannot know whether a dictation is
//! running. This file is the answer to that question, and it is the reason
//! [`MicTestPermit::issue`] has a caller in production instead of only in tests.
//!
//! Two decisions are worth stating outright, because they are the ones a reader
//! would otherwise have to guess:
//!
//! * **A test may not start while any dictation work is in flight.** Not just
//!   `Recording`: `Processing` means a conversion is holding audio to turn into
//!   text, and `Typing` means keystrokes are being written into a foreign
//!   window. Opening a second capture under any of the three would interleave a
//!   diagnostic stream with a real one, and the resulting level report would be
//!   about the mixture rather than about the microphone.
//! * **The refusal path must not leave the flag set.** The flag is claimed with
//!   a compare-exchange *before* the dictation is checked, so two callers
//!   racing on the same frame cannot both be told "no test is running"; if the
//!   dictation check then refuses, the flag is put back before returning.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::audio::diagnostics::{MicGateRefusal, MicTestPermit, MicUseGate};
use crate::state::{AppState, StatusChannel};

/// Decides whether a diagnostic test may open the microphone right now.
///
/// Cheap to ask and cheap to clone-share: the panel holds an `Arc` of this, and
/// the answer is two atomics plus a watch-channel borrow that never blocks.
///
/// The type is public because it travels in [`crate::gui::bootstrap::GuiStartup`]
/// and in [`crate::state::machine::StateMachine::mic_gate`]; its **constructor**
/// is not, because building one needs the crate-private status channel and there
/// is exactly one such channel in the program.
pub struct LiveMicGate {
    /// Where "is a dictation live" is read from. The same channel the orb
    /// subscribes to, so the gate cannot drift from what the user sees.
    status: Arc<StatusChannel>,
    /// Whether a test already holds the device. `true` between a granted
    /// permit and the owner's confirmation of release.
    test_running: Arc<AtomicBool>,
    /// Set once at shutdown; a test must not start on the way out.
    shutting_down: Arc<AtomicBool>,
}

impl LiveMicGate {
    pub(crate) fn new(status: Arc<StatusChannel>) -> Self {
        Self {
            status,
            test_running: Arc::new(AtomicBool::new(false)),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Whether a dictation is live, in the broad sense described above.
    ///
    /// `Error` is **not** a blocker: an engine that failed leaves no audio and
    /// no keystrokes behind it, so refusing a test because of a stale error
    /// badge would be refusing for the wrong reason.
    pub fn dictation_is_live(&self) -> bool {
        matches!(
            self.status.snapshot().state,
            AppState::Recording | AppState::Processing | AppState::Typing
        )
    }

    /// Whether a test currently holds the device.
    pub fn test_is_running(&self) -> bool {
        self.test_running.load(Ordering::SeqCst)
    }

    /// The owner confirms the device is physically free again.
    ///
    /// Called with the acknowledgement the machine produced, never on a guess:
    /// until the owner says it has the device back, the gate keeps refusing.
    pub fn confirm_release(&self) {
        self.test_running.store(false, Ordering::SeqCst);
    }

    /// The program is going down; no new test may start.
    pub fn mark_shutting_down(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.test_running.store(false, Ordering::SeqCst);
    }
}

impl MicUseGate for LiveMicGate {
    fn try_begin_test(&self) -> Result<MicTestPermit, MicGateRefusal> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(MicGateRefusal::ShuttingDown);
        }
        // Claim the device before anything else can be refused, so a second
        // caller on the same frame is told the truth instead of racing us.
        if self
            .test_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(MicGateRefusal::TestAlreadyRunning);
        }
        if self.dictation_is_live() {
            // Hand the claim straight back: refusing must not reserve anything.
            self.test_running.store(false, Ordering::SeqCst);
            return Err(MicGateRefusal::RecordingActive);
        }
        Ok(MicTestPermit::issue())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> (LiveMicGate, Arc<StatusChannel>) {
        let status = Arc::new(StatusChannel::new("test"));
        (LiveMicGate::new(status.clone()), status)
    }

    #[test]
    fn a_fresh_gate_lets_the_first_test_in() {
        let (gate, _status) = gate();
        assert!(gate.try_begin_test().is_ok());
        assert!(gate.test_is_running());
    }

    #[test]
    fn a_second_test_is_refused_while_the_first_holds_the_device() {
        let (gate, _status) = gate();
        let _permit = gate.try_begin_test().expect("first test wins");
        assert_eq!(
            gate.try_begin_test().unwrap_err(),
            MicGateRefusal::TestAlreadyRunning
        );
    }

    #[test]
    fn a_live_dictation_refuses_the_test_and_reserves_nothing() {
        let (gate, status) = gate();
        for busy in [
            AppState::Recording,
            AppState::Processing,
            AppState::Typing,
        ] {
            status.set_state(busy.clone());
            assert_eq!(
                gate.try_begin_test().unwrap_err(),
                MicGateRefusal::RecordingActive,
                "{busy:?} must block a test"
            );
            assert!(!gate.test_is_running(), "a refusal reserves nothing");
        }
    }

    /// The trap this guards: if the flag stayed set after a refusal, the *next*
    /// test would be told a test was already running when none was.
    #[test]
    fn a_test_may_start_as_soon_as_the_dictation_is_done() {
        let (gate, status) = gate();
        status.set_state(AppState::Recording);
        assert!(gate.try_begin_test().is_err());
        status.set_state(AppState::Idle);
        assert!(
            gate.try_begin_test().is_ok(),
            "the earlier refusal must not have kept the device claimed"
        );
    }

    /// An error badge left over from a failed engine is not a live dictation.
    #[test]
    fn an_error_state_does_not_block_a_test() {
        let (gate, status) = gate();
        status.set_state(AppState::Error("engine failed".into()));
        assert!(gate.try_begin_test().is_ok());
    }

    #[test]
    fn release_frees_the_device_and_shutdown_closes_it_for_good() {
        let (gate, _status) = gate();
        let _permit = gate.try_begin_test().expect("first test");
        gate.confirm_release();
        assert!(!gate.test_is_running());
        assert!(gate.try_begin_test().is_ok(), "released means available");

        gate.mark_shutting_down();
        assert_eq!(
            gate.try_begin_test().unwrap_err(),
            MicGateRefusal::ShuttingDown
        );
    }

    /// Idempotent on purpose: the machine may confirm a release on a path that
    /// also runs during shutdown, and a second confirmation must not be an
    /// error the caller has to handle.
    #[test]
    fn confirming_a_release_twice_is_harmless() {
        let (gate, _status) = gate();
        gate.confirm_release();
        gate.confirm_release();
        assert!(!gate.test_is_running());
    }
}
