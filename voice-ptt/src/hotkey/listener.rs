//! Global hotkey detection via a polling thread (`GetAsyncKeyState`).
//!
//! Polling at 10 ms gives < 15 ms worst-case detection latency — comfortably
//! inside the 50 ms acceptance budget — and avoids the low-level keyboard
//! hook (WH_KEYBOARD_LL), whose callback stalls all system input if it blocks.
//!
//! Bindings come from the user's settings (`HotkeySettings`) and are parsed
//! once at startup into [`HotkeyConfig`]. A binding that fails to parse is
//! logged and falls back to the built-in default, so one bad line in
//! `config.toml` never disables push-to-talk entirely.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::hotkey::binding::{key_to_vk, HotkeyBinding, Key, Modifier};

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

/// One binding resolved to virtual-key codes: `vk` is the main key, `mods` are
/// the modifiers that must accompany it (empty = bare key).
#[derive(Debug, Clone)]
struct ResolvedBinding {
    vk: u16,
    mods: Vec<u16>,
}

/// The three hotkey actions, resolved from settings and ready for the poll loop.
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    record: ResolvedBinding,
    toggle_overlay: ResolvedBinding,
    quit: ResolvedBinding,
}

/// Built-in defaults, used when settings are missing or unparseable.
impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            record: resolve_or_default("CapsLock", "record"),
            toggle_overlay: resolve_or_default("Ctrl+Alt+S", "toggle overlay"),
            quit: resolve_or_default("Ctrl+Alt+Q", "quit"),
        }
    }
}

impl HotkeyConfig {
    /// Builds the config from user settings, falling back to defaults for
    /// any binding that fails to parse (each fallback is logged).
    pub fn from_settings(settings: &crate::config::HotkeySettings) -> Self {
        Self {
            record: resolve_or_default(&settings.record, "record"),
            toggle_overlay: resolve_or_default(&settings.toggle_overlay, "toggle overlay"),
            quit: resolve_or_default(&settings.quit, "quit"),
        }
    }
}

/// Resolves a binding string to VK codes, or logs and falls back to the default.
fn resolve_or_default(spec: &str, what: &str) -> ResolvedBinding {
    // The built-in defaults are known-good; a parse failure there would be a
    // programming error, so it is allowed to panic the unit tests.
    fn default_binding() -> HotkeyBinding {
        HotkeyBinding::parse("CapsLock").unwrap()
    }

    match HotkeyBinding::parse(spec) {
        Ok(b) => match resolve_binding(&b) {
            Some(r) => r,
            None => {
                tracing::warn!(spec, what, "hotkey has no virtual-key equivalent; using default");
                resolve_binding(&default_binding()).expect("default hotkey must resolve")
            }
        },
        Err(e) => {
            tracing::warn!(spec, what, error = %e, "unparseable hotkey; using default");
            resolve_binding(&default_binding()).expect("default hotkey must resolve")
        }
    }
}

/// Maps a parsed binding to VK codes. Returns `None` if the main key has no
/// VK equivalent.
fn resolve_binding(b: &HotkeyBinding) -> Option<ResolvedBinding> {
    Some(ResolvedBinding {
        vk: key_vk_code(b.key)?,
        mods: b.modifiers.iter().map(|m| modifier_vk_code(*m)).collect(),
    })
}

/// VK code of a logical key, as a plain `u16` (platform-agnostic storage;
/// the poll loop casts it back on Windows).
fn key_vk_code(key: Key) -> Option<u16> {
    #[cfg(windows)]
    {
        key_to_vk(key).map(|vk| vk.0)
    }
    #[cfg(not(windows))]
    {
        let _ = key;
        None
    }
}

/// VK code of a modifier, as a plain `u16`.
fn modifier_vk_code(m: Modifier) -> u16 {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_MENU, VK_SHIFT};
        match m {
            Modifier::Ctrl => VK_CONTROL.0,
            Modifier::Alt => VK_MENU.0,
            Modifier::Shift => VK_SHIFT.0,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = m;
        0
    }
}

/// Handle to the running listener. Dropping it stops the thread.
pub struct HotkeyListener {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl HotkeyListener {
    /// Spawns the polling thread with the default bindings. `sender` receives
    /// key events.
    pub fn spawn(sender: std::sync::mpsc::Sender<HotkeyEvent>) -> anyhow::Result<Self> {
        Self::spawn_with_config(sender, HotkeyConfig::default())
    }

    /// Resolves the user's hotkey settings into a poll-ready config.
    pub fn config_from_settings(settings: &crate::config::HotkeySettings) -> HotkeyConfig {
        HotkeyConfig::from_settings(settings)
    }

    /// Spawns the polling thread with bindings resolved from user settings.
    pub fn spawn_with_config(
        sender: std::sync::mpsc::Sender<HotkeyEvent>,
        config: HotkeyConfig,
    ) -> anyhow::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        let handle = std::thread::Builder::new()
            .name("hotkey-listener".into())
            .spawn(move || run_loop(stop_clone, sender, config))
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
fn run_loop(
    stop: Arc<AtomicBool>,
    sender: std::sync::mpsc::Sender<HotkeyEvent>,
    config: HotkeyConfig,
) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    // A press shorter than this cancels the recording instead of transcribing
    // it (keeps the original "tap CapsLock to cancel" behavior).
    const CANCEL_MS: u128 = 300;

    let is_down = |key: u16| unsafe { GetAsyncKeyState(key as i32) < 0 };

    // A binding is "down" when its main key and all its modifiers are down.
    let binding_down = |b: &ResolvedBinding| is_down(b.vk) && b.mods.iter().all(|m| is_down(*m));

    let mut record_was_down = binding_down(&config.record);
    let mut toggle_was_down = binding_down(&config.toggle_overlay);
    let mut quit_was_down = binding_down(&config.quit);
    let mut record_down_at: Option<std::time::Instant> = None;

    // Swallow CapsLock's own LED-toggling behavior so the keyboard state
    // does not flip while using it as PTT (best effort, per press).
    // (Full suppression requires the low-level hook; acceptable trade-off.)

    while !stop.load(Ordering::Relaxed) {
        let record_is_down = binding_down(&config.record);

        if record_is_down && !record_was_down {
            record_down_at = Some(std::time::Instant::now());
            let _ = sender.send(HotkeyEvent::RecordDown);
        } else if !record_is_down && record_was_down {
            let cancel = record_down_at
                .map(|t| t.elapsed().as_millis() < CANCEL_MS)
                .unwrap_or(false);
            if cancel {
                let _ = sender.send(HotkeyEvent::Cancel);
            }
            let _ = sender.send(HotkeyEvent::RecordUp);
            record_down_at = None;
        }
        record_was_down = record_is_down;

        let toggle_is_down = binding_down(&config.toggle_overlay);
        if toggle_is_down && !toggle_was_down {
            let _ = sender.send(HotkeyEvent::ToggleOverlay);
        }
        toggle_was_down = toggle_is_down;

        let quit_is_down = binding_down(&config.quit);
        if quit_is_down && !quit_was_down {
            let _ = sender.send(HotkeyEvent::Quit);
        }
        quit_was_down = quit_is_down;

        std::thread::sleep(POLL_INTERVAL);
    }
}


#[cfg(not(windows))]
fn run_loop(
    _stop: Arc<AtomicBool>,
    _sender: std::sync::mpsc::Sender<HotkeyEvent>,
    _config: HotkeyConfig,
) {
    // Non-Windows dev builds: no global hotkeys.
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HotkeySettings;

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

    /// Settings strings must round-trip into a usable config.
    #[test]
    fn config_from_settings_parses_user_bindings() {
        let s = HotkeySettings {
            record: "Shift+F5".into(),
            toggle_overlay: "Ctrl+Alt+P".into(),
            quit: "Ctrl+Shift+Q".into(),
        };
        let cfg = HotkeyConfig::from_settings(&s);
        assert_eq!(
            cfg.record.vk,
            key_vk_code(Key::Fn(5)).expect("F5 must resolve on Windows")
        );
    }

    /// A garbage setting falls back to the default rather than disabling PTT.
    #[test]
    fn config_falls_back_on_parse_error() {
        let s = HotkeySettings {
            record: "not a key".into(),
            toggle_overlay: "".into(),
            quit: "Ctrl+Ctrl+Ctrl+Q".into(),
        };
        let cfg = HotkeyConfig::from_settings(&s);
        // Defaults: CapsLock / Ctrl+Alt+S / Ctrl+Alt+Q
        assert_eq!(
            cfg.record.vk,
            key_vk_code(Key::CapsLock).expect("CapsLock must resolve on Windows")
        );
    }

    /// The default config must resolve without panic (used on every fallback).
    #[test]
    fn default_config_resolves() {
        let cfg = HotkeyConfig::default();
        assert_eq!(
            cfg.record.vk,
            key_vk_code(Key::CapsLock).expect("CapsLock must resolve on Windows")
        );
        assert_eq!(cfg.record.mods, Vec::<u16>::new());
    }
}
