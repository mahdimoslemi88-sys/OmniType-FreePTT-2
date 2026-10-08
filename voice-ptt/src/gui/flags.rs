//! The six dashboard toggles, which always travel together.
//!
//! These are plain `Arc<AtomicBool>`s because the tray menu and the hotkey
//! bridge write them from foreign threads while the GUI polls them once a
//! frame. That is a deliberate choice, not laziness: a request has to be able
//! to arrive *before* the dashboard exists, which a channel would not allow.
//!
//! They are grouped because passing them as six positional parameters was
//! error-prone in a way the compiler could not catch: `tray::spawn` and
//! `OverlayApp::new` took the same six in the same order, so swapping two of
//! them type-checked perfectly and toggled the wrong panel. One named struct
//! makes the group explicit and removes the swap.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A request raised by the tray menu or a hotkey, to be observed by the GUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Overlay,
    Dictionary,
    Engine,
    History,
    Settings,
    Quit,
}

impl Toggle {
    /// The dashboard request a hotkey event implies, if any.
    ///
    /// This mapping used to be an `if matches!(ev, …)` inside the bridge
    /// thread in `run()`: an event nobody matched was forwarded to the state
    /// machine and produced no dashboard request, silently. Making it a
    /// function means the exhaustive set of events is visible in one place, and
    /// a new event variant is a compile error here rather than a no-op.
    pub fn for_hotkey_event(ev: &crate::hotkey::HotkeyEvent) -> Option<Toggle> {
        use crate::hotkey::HotkeyEvent;
        match ev {
            HotkeyEvent::ToggleOverlay => Some(Toggle::Overlay),
            // A quit **hotkey** has to reach the GUI as well as the machine.
            //
            // It used to map to `None` here, on the reasoning that the tray
            // already raises the flag. That is true of the tray and false of
            // the hotkey, and the difference is the bug this comment exists to
            // prevent: the state machine consumes `Quit` and ends, which drops
            // the receiver end of `events_tx`. The GUI keeps its own sender, so
            // every later `send` still *succeeds* — into a channel nobody reads.
            // The window stays up, the orb keeps accepting clicks, and pressing
            // record does nothing at all, with no error anywhere. The only way
            // out was a full restart.
            //
            // So: the machine is told, and so is the window.
            HotkeyEvent::Quit => Some(Toggle::Quit),
            // Record and Cancel are the state machine's business; the overlay
            // reacts to them through `StatusChannel`, not through a request.
            HotkeyEvent::OrbToggle
            | HotkeyEvent::RecordDown
            | HotkeyEvent::RecordUp
            | HotkeyEvent::Cancel => None,
        }
    }
}

/// The dashboard's request flags. `Arc` so each consumer holds its own handle.
#[derive(Clone)]
pub struct DashboardFlags {
    /// Set by the state machine's exit path. The GUI watches it and closes.
    ///
    /// This is the answer to a failure mode the request flags cannot cover.
    /// The GUI holds its own `events_tx` sender for as long as the window is
    /// open, so once the machine's `run` returns — quit, error or panic — the
    /// receiver is dropped and every later `send` still *succeeds*, into a
    /// channel nobody is reading. Nothing about the window changes: the orb
    /// keeps painting, keeps accepting clicks, and the record key does nothing,
    /// with no error shown anywhere. The app looks alive and is not.
    ///
    /// So the machine announces its own ending instead of leaving the window to
    /// infer it, and the GUI treats "the thing that answers me is gone" as a
    /// reason to close rather than as a state to sit in. A half-dead window
    /// that silently swallows the record key is worse than a closed one,
    /// because it looks like the app is working.
    pub machine_alive: Arc<AtomicBool>,
    /// Set externally (tray menu / hotkey) to request a visibility toggle.
    pub overlay: Arc<AtomicBool>,
    /// Set externally (tray menu) to request opening the dictionary window.
    pub dictionary: Arc<AtomicBool>,
    /// Set externally (tray menu) to request opening the AI engine window.
    pub engine: Arc<AtomicBool>,
    /// Set externally (tray menu) to request opening the history window.
    pub history: Arc<AtomicBool>,
    /// Set externally (tray menu) to request opening the settings window.
    pub settings: Arc<AtomicBool>,
    /// Set externally (tray menu / hotkey) to request application quit.
    pub quit: Arc<AtomicBool>,
}

impl DashboardFlags {
    /// All request flags start clear: nothing is requested until the user asks.
    ///
    /// `machine_alive` starts `true` because that is the truth at this point —
    /// the machine is spawned before the GUI — and only the machine's own exit
    /// path clears it.
    pub fn new() -> Self {
        Self {
            machine_alive: Arc::new(AtomicBool::new(true)),
            overlay: Arc::new(AtomicBool::new(false)),
            dictionary: Arc::new(AtomicBool::new(false)),
            engine: Arc::new(AtomicBool::new(false)),
            history: Arc::new(AtomicBool::new(false)),
            settings: Arc::new(AtomicBool::new(false)),
            quit: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Records that the state machine is no longer running.
    ///
    /// Called from the machine task's exit path, including the panic case: a
    /// panicked machine is the one that most needs the window to stop pretending,
    /// because it leaves no error state behind for the user to notice.
    pub fn mark_machine_dead(&self) {
        self.machine_alive.store(false, Ordering::SeqCst);
    }

    /// Whether the state machine is still there to answer a record key.
    pub fn machine_is_alive(&self) -> bool {
        self.machine_alive.load(Ordering::SeqCst)
    }

    pub(crate) fn raise(&self, which: Toggle) {
        match which {
            Toggle::Overlay => self.overlay.store(true, Ordering::Relaxed),
            Toggle::Dictionary => self.dictionary.store(true, Ordering::Relaxed),
            Toggle::Engine => self.engine.store(true, Ordering::Relaxed),
            Toggle::History => self.history.store(true, Ordering::Relaxed),
            Toggle::Settings => self.settings.store(true, Ordering::Relaxed),
            Toggle::Quit => self.quit.store(true, Ordering::Relaxed),
        }
    }

    /// Reads a flag and clears it in one step.
    ///
    /// Taking and clearing separately would let a request raised between the
    /// two be lost, so the GUI must never do that.
    pub(crate) fn take(&self, which: Toggle) -> bool {
        let flag = match which {
            Toggle::Overlay => &self.overlay,
            Toggle::Dictionary => &self.dictionary,
            Toggle::Engine => &self.engine,
            Toggle::History => &self.history,
            Toggle::Settings => &self.settings,
            Toggle::Quit => &self.quit,
        };
        flag.swap(false, Ordering::Relaxed)
    }
}

impl Default for DashboardFlags {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_flag_is_clear_and_take_reports_nothing() {
        let flags = DashboardFlags::new();
        for which in [
            Toggle::Overlay,
            Toggle::Dictionary,
            Toggle::Engine,
            Toggle::History,
            Toggle::Settings,
            Toggle::Quit,
        ] {
            assert!(!flags.take(which), "{which:?} must start clear");
        }
    }

    /// `Relaxed` is safe here and cheaper, but only because each flag has a
    /// single writer and a single reader with the `AtomicBool` providing the
    /// synchronisation. This test pins the write-then-read path.
    #[test]
    fn a_raised_flag_is_seen_once_and_then_cleared() {
        let flags = DashboardFlags::new();
        flags.raise(Toggle::Engine);
        assert!(flags.take(Toggle::Engine));
        assert!(
            !flags.take(Toggle::Engine),
            "a request must not be replayed"
        );
    }

    /// The bug this file's `Quit` arm exists for, as a test.
    ///
    /// A quit pressed as a **hotkey** used to reach the state machine and stop
    /// it, while the GUI was never told — the mapping returned `None`. The
    /// window then stayed up with a dead machine behind it: the orb kept
    /// painting and accepting clicks, and the record key did nothing, with no
    /// error anywhere. Only a full restart recovered it.
    ///
    /// Both halves matter, so both are asserted: the GUI must be asked to close,
    /// *and* the machine must still be told (the bridge forwards the event
    /// either way, which is why the fix belongs in the mapping and not in a
    /// `send` guard).
    #[test]
    fn a_quit_hotkey_closes_the_window_too() {
        assert_eq!(
            Toggle::for_hotkey_event(&crate::hotkey::HotkeyEvent::Quit),
            Some(Toggle::Quit),
            "a quit hotkey that does not raise the flag leaves a window with a dead machine behind it"
        );
    }

    /// The liveness signal, and why it is a flag rather than an inference.
    ///
    /// The GUI holds its own `events_tx` sender, so a stopped machine cannot be
    /// detected by a failing `send`. This is the whole reason the flag exists:
    /// something has to say out loud that the answering half is gone.
    #[test]
    fn the_machine_is_alive_until_something_says_otherwise() {
        let flags = DashboardFlags::new();
        assert!(
            flags.machine_is_alive(),
            "the machine is spawned before the GUI, so alive is the true starting answer"
        );
        flags.mark_machine_dead();
        assert!(
            !flags.machine_is_alive(),
            "a dead machine must be visible to the window, or the orb swallows every click"
        );
    }

    /// The death flag is shared, like every other flag: the machine task holds
    /// one handle and the GUI another, and they must be talking about the same
    /// bit. This is the property the fix actually depends on.
    #[test]
    fn machine_death_reaches_a_second_holder() {
        let flags = DashboardFlags::new();
        let gui_side = flags.clone();
        flags.mark_machine_dead();
        assert!(
            !gui_side.machine_is_alive(),
            "the GUI's clone must observe the machine task's announcement"
        );
    }

    /// The six are independent: raising one must not make another readable.
    /// This is the property that the old six-positional-argument signature
    /// could not protect.
    #[test]
    fn flags_do_not_leak_into_each_other() {
        let flags = DashboardFlags::new();
        flags.raise(Toggle::Dictionary);
        assert!(flags.take(Toggle::Dictionary));
        for other in [
            Toggle::Overlay,
            Toggle::Engine,
            Toggle::History,
            Toggle::Settings,
            Toggle::Quit,
        ] {
            assert!(!flags.take(other), "{other:?} must be unaffected");
        }
    }

    /// Handing the same flags to a second consumer must share one state —
    /// that is the whole point of the `Arc`.
    #[test]
    fn a_clone_shares_state_with_the_original() {
        let flags = DashboardFlags::new();
        let other = flags.clone();
        other.raise(Toggle::Settings);
        assert!(flags.take(Toggle::Settings));
    }

    /// The overlay hotkey must actually reach the dashboard. Before this was a
    /// function, dropping the match arm meant the overlay hotkey silently did
    /// nothing while still working for the state machine — a failure with no
    /// error and no symptom anywhere but "the overlay never appears".
    #[test]
    fn the_overlay_hotkey_raises_the_overlay_request() {
        use crate::hotkey::HotkeyEvent;
        assert_eq!(
            Toggle::for_hotkey_event(&HotkeyEvent::ToggleOverlay),
            Some(Toggle::Overlay)
        );
    }

    /// Record and cancel drive the state machine, not the dashboard. If one of
    /// them ever started raising a request it would be a new decision, and this
    /// test is where that decision gets written down.
    ///
    /// `Quit` was in this list and was **wrong**: it made a quit hotkey stop the
    /// machine without closing the window, which is the half-dead state
    /// `a_quit_hotkey_closes_the_window_too` now guards. It is deliberately not
    /// asserted here any more.
    #[test]
    fn the_state_machine_events_raise_no_dashboard_request() {
        use crate::hotkey::HotkeyEvent;
        for ev in [
            HotkeyEvent::RecordDown,
            HotkeyEvent::RecordUp,
            HotkeyEvent::Cancel,
        ] {
            assert_eq!(Toggle::for_hotkey_event(&ev), None, "{ev:?}");
        }
    }
}
