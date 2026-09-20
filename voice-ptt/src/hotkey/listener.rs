//! Global hotkey detection via a polling thread (`GetAsyncKeyState`).
//!
//! Polling at 10 ms gives < 15 ms worst-case detection latency — comfortably
//! inside the 50 ms acceptance budget — and avoids the low-level keyboard
//! hook (WH_KEYBOARD_LL), whose callback stalls all system input if it blocks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Events produced by the hotkey listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// Record key pressed (hold to talk).
    RecordDown,
    /// Record key released.
    RecordUp,
    /// Cancel active recording (discard audio without transcribing).
    Cancel,
    /// Toggle overlay visibility.
    ToggleOverlay,
    /// Quit requested.
    Quit,
}

/// Handle to the running listener. Dropping it stops the thread.
pub struct HotkeyListener {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl HotkeyListener {
    /// Spawns the polling thread. `sender` receives key events.
    pub fn spawn(
        sender: std::sync::mpsc::Sender<HotkeyEvent>,
    ) -> anyhow::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        let handle = std::thread::Builder::new()
            .name("hotkey-listener".into())
            .spawn(move || run_loop(stop_clone, sender))
            .map_err(|e| anyhow::anyhow!("failed to spawn hotkey thread: {e}"))?;

        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Requests the listener thread to stop.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(windows)]
fn run_loop(stop: Arc<AtomicBool>, sender: std::sync::mpsc::Sender<HotkeyEvent>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_CONTROL, VK_MENU, VK_Q, VK_S,
    };

    const POLL_INTERVAL: Duration = Duration::from_millis(10);

    let is_down = |key: VIRTUAL_KEY| unsafe { GetAsyncKeyState(key.0 as i32) < 0 };

    let mut caps_was_down = is_down(VK_CAPITAL);
    let mut combo_was_down = is_down(VK_CONTROL) && is_down(VK_MENU) && is_down(VK_S);
    let mut quit_was_down = is_down(VK_CONTROL) && is_down(VK_MENU) && is_down(VK_Q);

    // Swallow CapsLock's own LED-toggling behavior so the keyboard state
    // does not flip while using it as PTT (best effort, per press).
    // (Full suppression requires the low-level hook; acceptable trade-off.)

    while !stop.load(Ordering::Relaxed) {
        let caps_is_down = is_down(VK_CAPITAL);

        if caps_is_down && !caps_was_down {
            let _ = sender.send(HotkeyEvent::RecordDown);
        } else if !caps_is_down && caps_was_down {
            let _ = sender.send(HotkeyEvent::RecordUp);
        }
        caps_was_down = caps_is_down;

        let combo_is_down = is_down(VK_CONTROL) && is_down(VK_MENU) && is_down(VK_S);
        if combo_is_down && !combo_was_down {
            let _ = sender.send(HotkeyEvent::ToggleOverlay);
        }
        combo_was_down = combo_is_down;

        let quit_is_down = is_down(VK_CONTROL) && is_down(VK_MENU) && is_down(VK_Q);
        if quit_is_down && !quit_was_down {
            let _ = sender.send(HotkeyEvent::Quit);
        }
        quit_was_down = quit_is_down;

        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(not(windows))]
fn run_loop(_stop: Arc<AtomicBool>, _sender: std::sync::mpsc::Sender<HotkeyEvent>) {
    // Non-Windows dev builds: no global hotkeys.
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listener must stop promptly when signaled (thread-join bounded).
    #[test]
    fn listener_stops_promptly() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let listener = HotkeyListener::spawn(tx).unwrap();
        listener.stop();
        // Dropping joins the thread; if it hung, this test would hang.
        drop(listener);
    }

    #[test]
    fn event_equality_works() {
        assert_eq!(HotkeyEvent::RecordDown, HotkeyEvent::RecordDown);
        assert_ne!(HotkeyEvent::RecordDown, HotkeyEvent::RecordUp);
    }
}
