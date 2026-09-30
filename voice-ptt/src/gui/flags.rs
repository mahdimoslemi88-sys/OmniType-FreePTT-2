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

/// The dashboard's request flags. `Arc` so each consumer holds its own handle.
#[derive(Clone)]
pub struct DashboardFlags {
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
    /// All flags start clear: nothing is requested until the user asks.
    pub fn new() -> Self {
        Self {
            overlay: Arc::new(AtomicBool::new(false)),
            dictionary: Arc::new(AtomicBool::new(false)),
            engine: Arc::new(AtomicBool::new(false)),
            history: Arc::new(AtomicBool::new(false)),
            settings: Arc::new(AtomicBool::new(false)),
            quit: Arc::new(AtomicBool::new(false)),
        }
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
}
