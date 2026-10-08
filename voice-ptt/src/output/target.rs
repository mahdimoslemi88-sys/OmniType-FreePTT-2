//! Which window the text is allowed to go into.
//!
//! `SendInput` has no idea where it is typing: it delivers to whatever holds
//! focus. So the moment a dictation starts is the only moment we can learn
//! *where the user was*, and every later moment has to ask again — otherwise a
//! slow transcription lands in a window the user has since switched to.
//!
//! The user explicitly requested returning to the original application on
//! insertion. `restore_target` implements that policy, while `validate_target`
//! remains an observation-only check. Native focus and an accessibility element
//! are captured when available. Neither proves that an application preserved
//! the caret position inside its document; that needs application acceptance.
//!
//! The decision lives in [`classify`], a pure function over [`Observation`],
//! so the table below is tested without a desktop session. The Win32 calls are
//! a thin shim that only fills in [`observe`].

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Mutex;

/// Who the user was dictating into, captured when recording started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetIdentity {
    /// The foreground window handle at capture time.
    pub hwnd: isize,
    /// The process that owns `hwnd`.
    pub pid: u32,
    /// The executable behind `hwnd`, read from the process. `None` when access
    /// is denied — the handle and pid still identify the window, so this is
    /// never a reason to reject a target.
    pub exe_path: Option<PathBuf>,
    /// The title as it read at capture. **Not** an identifier: titles repeat
    /// and change. It is kept for the log line, and program identity comes
    /// from `exe_path`.
    pub title_at_capture: String,
    /// Native focused child captured with the destination, when Windows exposes it.
    pub focus_hwnd: Option<isize>,
    /// Exact accessibility element, when the application exposes one.
    pub focus_element: Option<std::sync::Arc<super::focus::FocusLease>>,
}

/// Whether the recorded destination is still the one in front of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetValidity {
    /// Same window, same process. Insert.
    Valid,
    /// Focus moved, or the handle now belongs to something else. Keep the text,
    /// do not insert.
    Changed,
    /// Cannot be determined — the window is gone, or there is no foreground
    /// window at all. Keep the text, do not insert.
    Unknown,
}

impl TargetValidity {
    /// Whether text may be delivered. Only [`TargetValidity::Valid`] may.
    pub fn allows_insert(self) -> bool {
        matches!(self, TargetValidity::Valid)
    }
}

/// What the platform was asked, and what it answered.
///
/// Split out from the decision so the decision is a table that can be tested,
/// and so a missing answer has its own case instead of collapsing into
/// "changed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// `IsWindow` for the captured handle. False means the window closed.
    pub window_alive: bool,
    /// `GetForegroundWindow` and the process that owns it, or `None` when
    /// there is no foreground window to ask about.
    pub foreground: Option<(isize, u32)>,
}

/// Decides whether `expected` is still the window in front of the user.
///
/// The order matters and is the whole point of the split: a closed window is
/// `Unknown` (we do not know where the text should go), while a live window
/// that is simply not the foreground one is `Changed` (we know exactly where
/// the user went). Both keep the text, and they are told apart so the log can
/// say which happened.
pub fn classify(expected: &TargetIdentity, observed: Observation) -> TargetValidity {
    if !observed.window_alive {
        return TargetValidity::Unknown;
    }
    match observed.foreground {
        // No foreground window to compare against: the desktop may be locked,
        // or another session may hold it. Not knowing is not "changed".
        None => TargetValidity::Unknown,
        Some((hwnd, pid)) if hwnd == expected.hwnd && pid == expected.pid => TargetValidity::Valid,
        Some(_) => TargetValidity::Changed,
    }
}

/// Records the window currently in front, to be re-checked before every insert.
///
/// **One dictation at a time, and the loop does not use it.** A single tracker
/// holds one destination, so a second recording would overwrite the first one's
/// — and an older answer can land after a newer dictation has started. The
/// loop keeps one destination *per session id* instead
/// ([`crate::state::coordinator`]), and captures through [`capture_target`] and
/// re-checks with [`validate_target`]. What stays here is the decision table and
/// this type, which is still the right shape for a single dictation — a tray
/// action or a script, say — and is left alone so it cannot be mistaken for the
/// loop's own bookkeeping.
#[derive(Debug, Default)]
pub struct TargetTracker {
    current: Option<TargetIdentity>,
}

impl TargetTracker {
    pub fn new() -> Self {
        Self { current: None }
    }

    /// The destination of the running dictation, if one was captured.
    pub fn current(&self) -> Option<&TargetIdentity> {
        self.current.as_ref()
    }

    /// Captures the foreground window as the destination for a new dictation.
    ///
    /// A dictation with no capturable destination is *not* an error state: the
    /// text is still produced, and the insert is refused later with a reason.
    pub fn capture(&mut self) -> Result<()> {
        self.current = capture_target().ok();
        Ok(())
    }

    /// Forgets the destination. Called when a session ends or is cancelled,
    /// so text from the next dictation is never judged against this one.
    pub fn clear(&mut self) {
        self.current = None;
    }

    /// Asks whether the captured destination may still receive text.
    ///
    /// With nothing captured the answer is `Unknown`: inserting into an
    /// unknown destination is exactly what this module exists to prevent.
    pub fn validate(&self) -> TargetValidity {
        match &self.current {
            Some(id) => validate_target(id),
            None => TargetValidity::Unknown,
        }
    }
}

/// The last window in front that belonged to **another** program.
///
/// Why it exists: the orb is a control, and clicking it must not throw away the
/// destination of the dictation it starts. With no memory to fall back on, a
/// click that leaves this program's own window in front produced a dictation
/// with no destination at all — the user's log has four orb clicks in a row,
/// each followed by `the window in front belongs to this program; no
/// destination captured` and `destination refused ... validity=Unknown`, and not
/// one character typed anywhere. Sampling the foreground while the user works
/// means that click still knows which window they came from.
///
/// Only ever a **fallback**: a live foreground window that is not ours always
/// wins, so this cannot aim text at a window the user has since left unless they
/// have left it for this program's own UI.
static LAST_EXTERNAL: Mutex<Option<TargetIdentity>> = Mutex::new(None);

/// Samples the window in front and keeps it when it is somebody else's.
///
/// Called on a timer rather than at capture time on purpose: by the time a
/// dictation starts, the window in front may already be the orb the user just
/// clicked. Ours is never stored, and neither is "no foreground window" — both
/// leave the previous answer standing, which is exactly the document the user
/// was working in.
///
/// Cheap in the steady state: when the same window is already remembered the
/// work stops after `GetForegroundWindow` and one pid lookup, so a 20 ms caller
/// does not re-read a title or open a process handle per tick.
pub fn remember_foreground() {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return;
    }
    let hwnd = hwnd.0 as isize;
    // This program's own windows — the orb, the dashboard, the review box, the
    // transcript card — are not documents.
    if owns_window(hwnd) {
        return;
    }
    let Some(pid) = window_pid(hwnd) else {
        return;
    };
    if lock()
        .as_ref()
        .is_some_and(|prev| prev.hwnd == hwnd && prev.pid == pid)
    {
        return;
    }
    let remembered = TargetIdentity {
        hwnd,
        pid,
        exe_path: process_path(pid).ok().flatten(),
        title_at_capture: window_title(hwnd).unwrap_or_default(),
        focus_hwnd: focused_child(hwnd),
        // No accessibility element here: those live on the focus thread's own
        // COM apartment and are captured with a dictation, not with a sample.
        // The native window and its focused child are the documented path when
        // UI Automation is not consulted, and `restore_target` re-checks both.
        focus_element: None,
    };
    *lock() = Some(remembered);
}

/// The remembered window, while it is still alive and still somebody else's.
///
/// A window that closed, or whose handle Windows has recycled to another
/// process, is forgotten rather than handed back: text aimed at it would be text
/// aimed at nothing.
pub fn remembered_target() -> Option<TargetIdentity> {
    let id = lock().clone()?;
    let alive = observe(id.hwnd).window_alive;
    let pid_now = window_pid(id.hwnd);
    if !remembered_is_usable(owns_window(id.hwnd), alive, pid_now, id.pid) {
        return None;
    }
    Some(id)
}

/// The pure half of [`remembered_target`]'s verdict.
///
/// Kept separate so the rule can be asserted without a desktop session, and so
/// there is exactly one place that decides what "still usable" means.
fn remembered_is_usable(ours: bool, alive: bool, pid_now: Option<u32>, pid_then: u32) -> bool {
    !ours && alive && pid_now == Some(pid_then)
}

/// The remembered window, with a poisoned lock treated as "empty".
///
/// A panicking sampler must not take the destination of every later dictation
/// down with it: an empty slot is the honest answer, and the next sample refills
/// it.
fn lock() -> std::sync::MutexGuard<'static, Option<TargetIdentity>> {
    LAST_EXTERNAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Reads the window currently in front of the user.
pub fn capture_target() -> Result<TargetIdentity> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        anyhow::bail!("no foreground window to capture");
    }
    let hwnd = hwnd.0 as isize;

    let pid = window_pid(hwnd).unwrap_or(0);
    let title = window_title(hwnd).unwrap_or_default();
    let exe_path = process_path(pid).ok().flatten();

    let focus_hwnd = focused_child(hwnd);
    let focus_element = super::focus::capture(pid);
    if !classify(
        &TargetIdentity {
            hwnd,
            pid,
            exe_path: None,
            title_at_capture: String::new(),
            focus_hwnd: None,
            focus_element: None,
        },
        observe(hwnd),
    )
    .allows_insert()
    {
        anyhow::bail!("foreground changed while capturing the destination");
    }
    Ok(TargetIdentity {
        hwnd,
        pid,
        exe_path,
        title_at_capture: title,
        focus_hwnd,
        focus_element,
    })
}

/// Re-checks a captured destination against the window in front of the user.
pub fn validate_target(id: &TargetIdentity) -> TargetValidity {
    classify(id, observe(id.hwnd))
}

pub(super) fn focused_child(hwnd: isize) -> Option<isize> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };
    let thread = unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if thread == 0
        || unsafe { GetGUIThreadInfo(thread, &mut info) }.is_err()
        || info.hwndFocus.0.is_null()
    {
        return None;
    }
    Some(info.hwndFocus.0 as isize)
}

/// Return to the explicitly captured destination and verify actual focus before typing.
/// Browser/editor DOM caret positions are retained by the application itself;
/// a native child handle does not identify a DOM input or a terminal prompt.
pub fn restore_target(id: &TargetIdentity) -> TargetValidity {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsChild, IsIconic, IsWindow, SetForegroundWindow, ShowWindowAsync, SW_RESTORE,
    };
    let raw = HWND(id.hwnd as *mut _);
    if !unsafe { IsWindow(raw) }.as_bool()
        || window_pid(id.hwnd) != Some(id.pid)
        || owns_window(id.hwnd)
    {
        return TargetValidity::Unknown;
    }
    if let Some(child) = id.focus_hwnd {
        let child_raw = HWND(child as *mut _);
        if !unsafe { IsWindow(child_raw) }.as_bool()
            || window_pid(child) != Some(id.pid)
            || (child != id.hwnd && !unsafe { IsChild(raw, child_raw) }.as_bool())
        {
            return TargetValidity::Changed;
        }
    }
    if !validate_target(id).allows_insert() {
        if unsafe { IsIconic(raw) }.as_bool() {
            let _ = unsafe { ShowWindowAsync(raw, SW_RESTORE) };
        }
        let _ = unsafe { SetForegroundWindow(raw) };
    }
    // Activation can be asynchronous, but a delay is never proof of success.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
    while !validate_target(id).allows_insert() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if !validate_target(id).allows_insert() {
        return TargetValidity::Changed;
    }
    if let Some(child) = id.focus_hwnd {
        if focused_child(id.hwnd) != Some(child)
            && !super::focus::restore_native(id.hwnd, child, id.pid)
        {
            return TargetValidity::Changed;
        }
    }
    if let Some(element) = &id.focus_element {
        if !super::focus::restore(element, id.pid) {
            return TargetValidity::Changed;
        }
    }
    validate_target(id)
}

/// Whether `hwnd` is a window of **this process**.
///
/// The app's own windows — the orb, the dashboard, the review window, the
/// transcript card — are not documents. Text dictated while one of them is in
/// front has no destination at all, and typing it there would put the user's
/// words into a window they cannot edit. It used to happen: a mouse click on the
/// orb made the overlay, not the user's editor, the window in front, and the
/// dictation came back as "focus moved" with an error badge.
pub fn owns_window(hwnd: isize) -> bool {
    is_own_pid(window_pid(hwnd), std::process::id())
}

/// The pure half of [`owns_window`].
///
/// A pid that cannot be read is **not** treated as ours: an unknown window is a
/// real window until proven otherwise, and the alternative would silently throw
/// away a destination the user could have kept.
fn is_own_pid(pid: Option<u32>, own: u32) -> bool {
    pid == Some(own)
}

/// The platform half of [`classify`]: asks Windows three questions and
/// reports the answers. It makes no decision of its own.
fn observe(hwnd: isize) -> Observation {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};

    let raw = HWND(hwnd as *mut std::ffi::c_void);
    let window_alive = unsafe { IsWindow(raw) }.as_bool();

    let foreground_hwnd = unsafe { GetForegroundWindow() };
    let foreground = if foreground_hwnd.0.is_null() {
        None
    } else {
        Some((
            foreground_hwnd.0 as isize,
            window_pid(foreground_hwnd.0 as isize).unwrap_or(0),
        ))
    };

    Observation {
        window_alive,
        foreground,
    }
}

pub(super) fn window_pid(hwnd: isize) -> Option<u32> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut std::ffi::c_void), Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn window_title(hwnd: isize) -> Option<String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;

    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(HWND(hwnd as *mut std::ffi::c_void), &mut buf) };
    if n <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..n as usize]))
}

fn process_path(pid: u32) -> Result<Option<PathBuf>> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return Ok(None);
    }
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            // Access denied on someone else's process is an ordinary answer,
            // not a failure: the handle and pid still identify the window.
            Err(_) => return Ok(None),
        };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        match result {
            // The API writes the length back including the terminator, so the
            // string is everything before it — reading the whole buffer would
            // trail a NUL through the path.
            Ok(()) => {
                let used = (len as usize).min(buf.len());
                let text = String::from_utf16_lossy(&buf[..used]);
                let text = text.trim_end_matches('\0');
                if text.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(PathBuf::from(text)))
                }
            }
            Err(e) => Err(anyhow::anyhow!("QueryFullProcessImageNameW failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(hwnd: isize, pid: u32) -> TargetIdentity {
        TargetIdentity {
            hwnd,
            pid,
            exe_path: None,
            title_at_capture: "t".into(),
            focus_hwnd: None,
            focus_element: None,
        }
    }

    fn live(foreground: Option<(isize, u32)>) -> Observation {
        Observation {
            window_alive: true,
            foreground,
        }
    }

    #[test]
    fn the_captured_window_itself_is_valid() {
        assert_eq!(
            classify(&target(10, 20), live(Some((10, 20)))),
            TargetValidity::Valid
        );
    }

    #[test]
    fn focus_moving_to_another_window_is_changed_not_unknown() {
        // The distinction the log depends on: we know exactly where the user
        // went, so this is not a "cannot tell".
        assert_eq!(
            classify(&target(10, 20), live(Some((11, 20)))),
            TargetValidity::Changed
        );
    }

    #[test]
    fn the_same_handle_now_owned_by_another_process_is_changed() {
        // Windows recycles handles. Comparing the handle alone would accept a
        // window that is no longer the one the dictation started in.
        assert_eq!(
            classify(&target(10, 20), live(Some((10, 99)))),
            TargetValidity::Changed
        );
    }

    #[test]
    fn a_closed_window_is_unknown_even_with_a_foreground_to_compare() {
        let observed = Observation {
            window_alive: false,
            foreground: Some((10, 20)),
        };
        assert_eq!(classify(&target(10, 20), observed), TargetValidity::Unknown);
    }

    /// The case that motivated splitting `Unknown` out: a lock screen or a
    /// secure desktop has no foreground window, and reporting "changed" there
    /// would claim knowledge nobody has.
    #[test]
    fn no_foreground_window_is_unknown() {
        assert_eq!(
            classify(&target(10, 20), live(None)),
            TargetValidity::Unknown
        );
    }

    #[test]
    fn only_valid_may_insert() {
        assert!(TargetValidity::Valid.allows_insert());
        assert!(!TargetValidity::Changed.allows_insert());
        assert!(!TargetValidity::Unknown.allows_insert());
    }

    /// Nothing captured means nothing may be inserted — the module's whole
    /// reason to exist. A tracker that never captured must not default to
    /// "valid" because that is what an unchecked bool would do.
    #[test]
    fn a_tracker_with_no_capture_refuses_to_insert() {
        let tracker = TargetTracker::new();
        assert!(tracker.current().is_none());
        assert_eq!(tracker.validate(), TargetValidity::Unknown);
    }

    #[test]
    fn clearing_forgets_the_destination() {
        let mut tracker = TargetTracker::new();
        tracker.clear();
        assert!(tracker.current().is_none());
    }

    /// The limit is part of the contract, so it is asserted rather than
    /// described: the check is window plus process, never the edit control.
    #[test]
    fn a_matching_window_and_process_is_enough_and_that_is_the_whole_limit() {
        // Two different typeable fields of one window (two browser tabs, Word
        // and its find bar) are indistinguishable here by design. The
        // contract calls this an accepted limitation, not a hidden gap, so the
        // test states exactly what is and is not compared.
        let id = target(10, 20);
        assert_eq!(classify(&id, live(Some((10, 20)))), TargetValidity::Valid);
    }

    // ── the remembered destination ───────────────────────────────────────

    /// The remembered window is a fallback, so it has exactly four ways to be
    /// unusable and each one has to refuse — the alternative is aiming a
    /// dictation at a window that is not there, or at this program's own canvas.
    #[test]
    fn a_remembered_window_is_usable_only_while_it_is_alive_and_not_ours() {
        assert!(remembered_is_usable(false, true, Some(20), 20));
        assert!(
            !remembered_is_usable(true, true, Some(20), 20),
            "this program's own window is not a document"
        );
        assert!(
            !remembered_is_usable(false, false, Some(20), 20),
            "a closed window cannot receive text"
        );
        assert!(
            !remembered_is_usable(false, true, Some(99), 20),
            "Windows recycles handles; a different pid is a different window"
        );
        assert!(
            !remembered_is_usable(false, true, None, 20),
            "a pid that cannot be read is not a match"
        );
    }
}
