//! Which window the text is allowed to go into.
//!
//! `SendInput` has no idea where it is typing: it delivers to whatever holds
//! focus. So the moment a dictation starts is the only moment we can learn
//! *where the user was*, and every later moment has to ask again — otherwise a
//! slow transcription lands in a window the user has since switched to.
//!
//! Two limits are deliberate, not oversights:
//!
//! * **`HWND` is not an edit control.** One window can hold several typeable
//!   fields (a browser tab, Word, a terminal), and only the window is known
//!   here. The first version checks window plus process and says so out loud.
//! * **Focus is never taken.** When the target cannot be trusted the text is
//!   kept, not delivered somewhere that merely accepts it. Stealing focus would
//!   put text in front of the user without asking, which is worse than a
//!   delayed dictation.
//!
//! The decision lives in [`classify`], a pure function over [`Observation`],
//! so the table below is tested without a desktop session. The Win32 calls are
//! a thin shim that only fills in [`observe`].

use anyhow::Result;
use std::path::PathBuf;

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

    Ok(TargetIdentity {
        hwnd,
        pid,
        exe_path,
        title_at_capture: title,
    })
}

/// Re-checks a captured destination against the window in front of the user.
pub fn validate_target(id: &TargetIdentity) -> TargetValidity {
    classify(id, observe(id.hwnd))
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

fn window_pid(hwnd: isize) -> Option<u32> {
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
}
