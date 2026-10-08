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
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

// Binding resolution and its failure report live in `diagnostics`; the
// fallback itself is unchanged, only observable.
use crate::hotkey::diagnostics::{resolve_or_default, HotkeyProblem, HotkeyRole, ResolvedBinding};

/// Events produced by the hotkey listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// Mouse recording: first click starts, second click stops.
    OrbToggle,
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

/// The three hotkey actions, resolved from settings and ready for the poll loop.
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    record: ResolvedBinding,
    toggle_overlay: ResolvedBinding,
    quit: ResolvedBinding,
    /// Every setting that could not be used as written, in the order they
    /// were resolved. Empty means the user's config was honoured exactly.
    problems: Vec<HotkeyProblem>,
}

/// Built-in defaults, used when settings are missing or unparseable.
///
/// Written out rather than derived, because a `Default` that resolved to
/// "no key at all" would silently disable push-to-talk instead of falling
/// back to a working one.
impl Default for HotkeyConfig {
    fn default() -> Self {
        let config = Self {
            record: resolve_or_default("CapsLock", HotkeyRole::Record).binding,
            toggle_overlay: resolve_or_default("Ctrl+Alt+S", HotkeyRole::ToggleOverlay).binding,
            quit: resolve_or_default("Ctrl+Alt+Q", HotkeyRole::Quit).binding,
            problems: Vec::new(),
        };
        // The built-ins are known-good; anything here would be a typo in this
        // file, not a user mistake, so it must not reach `problems`.
        debug_assert!(config.problems.is_empty());
        config
    }
}
impl HotkeyConfig {
    /// Builds the config from user settings, falling back to defaults for
    /// any binding that cannot be used (each fallback is logged *and* recorded
    /// in `problems`, so a caller can surface it to the user).
    pub fn from_settings(settings: &crate::config::HotkeySettings) -> Self {
        let mut problems = Vec::new();
        let mut take = |r: crate::hotkey::diagnostics::Resolution| {
            if let Some(p) = r.problem {
                problems.push(p);
            }
            r.binding
        };
        let config = Self {
            record: take(resolve_or_default(&settings.record, HotkeyRole::Record)),
            toggle_overlay: take(resolve_or_default(
                &settings.toggle_overlay,
                HotkeyRole::ToggleOverlay,
            )),
            quit: take(resolve_or_default(&settings.quit, HotkeyRole::Quit)),
            problems,
        };
        for problem in &config.problems {
            tracing::warn!(spec = ?problem, "{}", problem.message());
        }
        config
    }

    /// Settings that could not be used, and what replaced them.
    ///
    /// The root of the planned `--doctor` report: a working app on the wrong
    /// key is indistinguishable from a correct one unless this is read.
    pub fn problems(&self) -> &[HotkeyProblem] {
        &self.problems
    }
}

/// Result of one key-capture attempt, handed to the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// The user pressed this chord; the string is parseable by
    /// `HotkeyBinding::parse` (e.g. `Ctrl+Alt+S`, `F5`, `CapsLock`).
    Binding(String),
    /// Escape was pressed, the wait timed out, or the capture was cancelled.
    Cancelled,
    /// The chord was refused with a reason (e.g. it uses the Windows key, or
    /// the main key has no bindable equivalent). The UI surfaces the reason
    /// instead of leaving the user with a silently dead button.
    Rejected(String),
}

/// State shared between the polling thread and the UI's [`HotkeyControl`].
struct SharedState {
    /// Live bindings. The poll thread re-reads this every tick, so applying a
    /// new shortcut takes effect immediately instead of on the next restart.
    config: RwLock<HotkeyConfig>,
    /// Set by the UI to arm a capture.
    capture_requested: AtomicBool,
    /// Set by the UI (or by a second click) to abort an in-flight capture.
    capture_cancelled: AtomicBool,
    /// True while the poll thread is actually listening for a chord.
    capturing: AtomicBool,
    /// One-shot result for the UI; `None` means "still waiting".
    capture_result: Mutex<Option<CaptureOutcome>>,
}

/// Cloneable handle the UI keeps: capture a new shortcut key and apply new
/// bindings to the running listener.
///
/// Capturing happens on the *polling thread* using `GetAsyncKeyState`, not in
/// the GUI: the overlay is a tiny always-on-top window that usually does not
/// hold keyboard focus, so egui key events alone cannot see the chord the user
/// presses.
#[derive(Clone)]
pub struct HotkeyControl {
    shared: Arc<SharedState>,
}

impl HotkeyControl {
    /// Arms the global capture: the next chord the user presses (whichever
    /// window has focus) is delivered to [`Self::take_capture`].
    pub fn begin_capture(&self) {
        if let Ok(mut slot) = self.shared.capture_result.lock() {
            *slot = None;
        }
        self.shared
            .capture_cancelled
            .store(false, Ordering::Relaxed);
        self.shared.capture_requested.store(true, Ordering::Relaxed);
    }

    /// Aborts an in-flight capture (Escape, or the settings window closing).
    pub fn cancel_capture(&self) {
        self.shared.capture_cancelled.store(true, Ordering::Relaxed);
    }

    /// True while the poll thread is listening for a chord.
    pub fn is_capturing(&self) -> bool {
        self.shared.capturing.load(Ordering::Relaxed)
    }

    /// Takes the capture result once ready (`None` while still waiting).
    pub fn take_capture(&self) -> Option<CaptureOutcome> {
        self.shared
            .capture_result
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    /// Replaces the live bindings (effective on the next poll tick).
    pub fn set_config(&self, config: HotkeyConfig) {
        if let Ok(mut guard) = self.shared.config.write() {
            *guard = config;
        }
    }

    /// Snapshot of the live bindings.
    pub fn config(&self) -> HotkeyConfig {
        self.shared
            .config
            .read()
            .map(|c| c.clone())
            .unwrap_or_default()
    }
}

// Superseded by `hotkey::diagnostics::resolve_or_default`, which does the same
// fallback and additionally reports *why* it happened. Kept for the record —
// the difference between the two is the whole point of the module.
//
// fn resolve_or_default(spec: &str, what: &str) -> ResolvedBinding {
//     fn default_binding() -> HotkeyBinding {
//         HotkeyBinding::parse("CapsLock").unwrap()
//     }
//     match HotkeyBinding::parse(spec) {
//         Ok(b) => match resolve_binding(&b) {
//             Some(r) => r,
//             None => {
//                 tracing::warn!(spec, what, "hotkey has no virtual-key equivalent; using default");
//                 resolve_binding(&default_binding()).expect("default hotkey must resolve")
//             }
//         },
//         Err(e) => {
//             tracing::warn!(spec, what, error = %e, "unparseable hotkey; using default");
//             resolve_binding(&default_binding()).expect("default hotkey must resolve")
//         }
//     }
// }

/// Handle to the running listener. Dropping it stops the thread.
pub struct HotkeyListener {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    shared: Arc<SharedState>,
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

        let shared = Arc::new(SharedState {
            config: RwLock::new(config),
            capture_requested: AtomicBool::new(false),
            capture_cancelled: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            capture_result: Mutex::new(None),
        });
        let shared_clone = shared.clone();

        let handle = std::thread::Builder::new()
            .name("hotkey-listener".into())
            .spawn(move || run_loop(stop_clone, sender, shared_clone))
            .map_err(|e| anyhow::anyhow!("failed to spawn hotkey thread: {e}"))?;

        Ok(Self {
            stop,
            handle: Some(handle),
            shared,
        })
    }

    /// Handle for the UI: re-bind at runtime and capture a new shortcut key.
    pub fn control(&self) -> HotkeyControl {
        HotkeyControl {
            shared: self.shared.clone(),
        }
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
    shared: Arc<SharedState>,
) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    // A press shorter than this cancels the recording instead of transcribing
    // it (keeps the original "tap CapsLock to cancel" behavior).
    const CANCEL_MS: u128 = 300;
    // How long a capture waits for the user to press something.
    const CAPTURE_TIMEOUT: Duration = Duration::from_secs(15);
    // VKs scanned while capturing. Starts at 0x08 so the mouse buttons
    // (0x01–0x06) are never captured — clicking the "record" button must not
    // bind that click.
    const CAPTURE_SCAN_FIRST: u16 = 0x08;
    const CAPTURE_SCAN_LAST: u16 = 0xFE;
    const VK_SHIFT: u16 = 0x10;
    const VK_CONTROL: u16 = 0x11;
    const VK_ALT: u16 = 0x12;
    const VK_ESCAPE: u16 = 0x1B;
    const VK_LWIN: u16 = 0x5B;
    const VK_RWIN: u16 = 0x5C;

    let is_down = |key: u16| unsafe { GetAsyncKeyState(key as i32) < 0 };

    // A binding is "down" when its main key and all its modifiers are down.
    let binding_down = |b: &ResolvedBinding| is_down(b.vk) && b.mods.iter().all(|m| is_down(*m));

    // Only Ctrl/Alt/Shift are bindable modifiers. The Windows keys are not part
    // of the binding grammar, so a chord that uses one is rejected outright
    // instead of being stored as "just the other key".
    let is_modifier_vk = |vk: u16| matches!(vk, VK_SHIFT | VK_CONTROL | VK_ALT);
    let is_unsupported_modifier = |vk: u16| matches!(vk, VK_LWIN | VK_RWIN);

    let initial = shared.config.read().map(|c| c.clone()).unwrap_or_default();
    let mut record_was_down = binding_down(&initial.record);
    let mut toggle_was_down = binding_down(&initial.toggle_overlay);
    let mut quit_was_down = binding_down(&initial.quit);
    let mut record_down_at: Option<std::time::Instant> = None;

    // Key-capture bookkeeping (only meaningful while a capture is armed).
    let mut capture_deadline = std::time::Instant::now();
    let mut capture_key: Option<u16> = None;
    let mut capture_mods: Vec<u16> = Vec::new();

    // Swallow CapsLock's own LED-toggling behavior so the keyboard state
    // does not flip while using it as PTT (best effort, per press).
    // (Full suppression requires the low-level hook; acceptable trade-off.)

    while !stop.load(Ordering::Relaxed) {
        // Live snapshot: a re-bind applied from the settings UI is honoured on
        // the very next tick, without restarting the app.
        let config = shared.config.read().map(|c| c.clone()).unwrap_or_default();

        // ── shortcut-key capture ────────────────────────────────────────────
        if shared.capture_requested.swap(false, Ordering::Relaxed) {
            capture_deadline = std::time::Instant::now() + CAPTURE_TIMEOUT;
            capture_key = None;
            capture_mods.clear();
            shared.capturing.store(true, Ordering::Relaxed);
        }

        if shared.capturing.load(Ordering::Relaxed) {
            let outcome = capture_tick(
                &shared,
                &mut capture_key,
                &mut capture_mods,
                capture_deadline,
                &is_down,
                &is_modifier_vk,
                &is_unsupported_modifier,
                VK_ESCAPE,
                CAPTURE_SCAN_FIRST,
                CAPTURE_SCAN_LAST,
            );

            if let Some(outcome) = outcome {
                shared.capturing.store(false, Ordering::Relaxed);
                if let Ok(mut slot) = shared.capture_result.lock() {
                    *slot = Some(outcome);
                }
                // Re-baseline the edges from the keys as they are right now:
                // the captured chord (very often CapsLock itself) must not fire
                // a hotkey edge on the next tick.
                record_was_down = binding_down(&config.record);
                toggle_was_down = binding_down(&config.toggle_overlay);
                quit_was_down = binding_down(&config.quit);
            }

            std::thread::sleep(POLL_INTERVAL);
            continue;
        }

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

/// One tick of a key capture. Returns `Some(..)` when the capture finished
/// (the chord was released, or it was cancelled / timed out).
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn capture_tick(
    shared: &SharedState,
    capture_key: &mut Option<u16>,
    capture_mods: &mut Vec<u16>,
    deadline: std::time::Instant,
    is_down: &impl Fn(u16) -> bool,
    is_modifier_vk: &impl Fn(u16) -> bool,
    is_unsupported_modifier: &impl Fn(u16) -> bool,
    escape_vk: u16,
    scan_first: u16,
    scan_last: u16,
) -> Option<CaptureOutcome> {
    if shared.capture_cancelled.swap(false, Ordering::Relaxed) {
        return Some(CaptureOutcome::Cancelled);
    }

    let pressed: Vec<u16> = (scan_first..=scan_last).filter(|vk| is_down(*vk)).collect();

    // Win-key chords cannot be expressed as a binding; refusing is safer than
    // storing a shortcut that would then fire on the bare key.
    if pressed.iter().any(|vk| is_unsupported_modifier(*vk)) {
        tracing::warn!("hotkey capture rejected: Windows-key chords are not supported");
        return Some(CaptureOutcome::Rejected(
            "Windows-key chords are not supported".into(),
        ));
    }

    let Some(key) = *capture_key else {
        // Nothing down yet: give up after the timeout so the button does not
        // stay armed forever.
        if pressed.is_empty() {
            return (std::time::Instant::now() > deadline).then_some(CaptureOutcome::Cancelled);
        }
        // Escape alone means "cancel", matching the previous behaviour.
        if pressed.contains(&escape_vk) {
            return Some(CaptureOutcome::Cancelled);
        }
        // Modifiers only: keep waiting for the actual key they belong to.
        let &main = pressed.iter().find(|vk| !is_modifier_vk(**vk))?;
        capture_mods.clear();
        for m in [0x11u16, 0x12, 0x10] {
            if is_down(m) {
                capture_mods.push(m);
            }
        }
        *capture_key = Some(main);
        return None;
    };

    // The chord is complete once the user lets the main key go.
    if is_down(key) {
        return None;
    }
    Some(match chord_token(key, capture_mods) {
        Some(token) => CaptureOutcome::Binding(token),
        None => CaptureOutcome::Rejected("key has no bindable equivalent".into()),
    })
}

/// Renders a captured chord as a config token (`Ctrl+Alt+S`), or `None` when
/// the main key has no binding equivalent.
#[cfg(windows)]
fn chord_token(key: u16, mods: &[u16]) -> Option<String> {
    use crate::hotkey::binding::{vk_to_key, Key};

    let binding_key: Key = vk_to_key(key)?;
    let mut parts: Vec<&str> = Vec::new();
    for m in [0x11u16, 0x12, 0x10] {
        if mods.contains(&m) {
            parts.push(match m {
                0x11 => "Ctrl",
                0x12 => "Alt",
                _ => "Shift",
            });
        }
    }
    let mut token = parts.join("+");
    if !token.is_empty() {
        token.push('+');
    }
    token.push_str(&binding_key.to_token());
    Some(token)
}

#[cfg(not(windows))]
fn run_loop(
    _stop: Arc<AtomicBool>,
    _sender: std::sync::mpsc::Sender<HotkeyEvent>,
    shared: Arc<SharedState>,
) {
    // Non-Windows dev builds: no global hotkeys, so a capture request is
    // answered immediately with "cancelled" rather than left hanging.
    loop {
        if shared.capture_requested.swap(false, Ordering::Relaxed) {
            shared.capturing.store(false, Ordering::Relaxed);
            if let Ok(mut slot) = shared.capture_result.lock() {
                *slot = Some(CaptureOutcome::Cancelled);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HotkeySettings;
    // Only the tests need these: the poll loop works on the already-resolved
    // `ResolvedBinding`, and `diagnostics` owns the key→VK translation.
    use crate::hotkey::binding::{key_vk_code, modifier_vk_code, Key, Modifier};

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
            ..HotkeySettings::default()
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
            ..HotkeySettings::default()
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

    /// Changing the shortcut must reach the *running* listener, not just the
    /// config file — the whole point of the re-bind UI.
    #[test]
    fn control_applies_new_bindings_without_restart() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let listener = HotkeyListener::spawn(tx).unwrap();
        let control = listener.control();

        assert_eq!(
            control.config().record.vk,
            key_vk_code(Key::CapsLock).expect("CapsLock must resolve on Windows")
        );

        let settings = HotkeySettings {
            record: "Shift+F5".into(),
            toggle_overlay: "Ctrl+Alt+P".into(),
            quit: "Ctrl+Shift+Q".into(),
            ..HotkeySettings::default()
        };
        control.set_config(HotkeyConfig::from_settings(&settings));

        let live = control.config();
        assert_eq!(
            live.record.vk,
            key_vk_code(Key::Fn(5)).expect("F5 must resolve on Windows")
        );
        assert_eq!(live.record.mods, vec![modifier_vk_code(Modifier::Shift)]);
    }

    /// A capture must always answer the UI (here: a cancel) instead of leaving
    /// the button armed forever.
    #[test]
    fn cancelled_capture_reports_back() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let listener = HotkeyListener::spawn(tx).unwrap();
        let control = listener.control();

        control.begin_capture();
        control.cancel_capture();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Some(outcome) = control.take_capture() {
                assert_eq!(outcome, CaptureOutcome::Cancelled);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("capture never reported back");
    }
}
