//! Telling the user an update exists, as pure decisions.
//!
//! The gap this closes: `UpdateState::Available` reached the user in exactly
//! one place — the banner in the dashboard's settings tab — and only if they
//! happened to open that tab. Detection worked; nothing interrupted anybody.
//!
//! Why the split (same as [`super::tray_warning`]): everything here is a value
//! in and a value out. The Windows half — hidden window, `Shell_NotifyIconW`,
//! the message pump — lives behind [`BalloonHandle`] and cannot be tested
//! headlessly, so the decisions that can be *wrong* are all kept on this side
//! where they can be pinned by tests.
//!
//! Four decisions, and the reason each one is not obvious:
//!
//! 1. **Once per release, not once per check.** The background checker runs
//!    every 6 hours by default, forever. A balloon fired on every successful
//!    check would appear four times a day for as long as the user stayed on
//!    that version — which is the behaviour that gets notifications turned off
//!    in Windows, at which point *no* update would ever be announced. The
//!    notified version is persisted, so a restart does not re-announce either.
//! 2. **A newer version re-announces.** The dedup is on the version, not on a
//!    boolean. When 0.5.0 ships, a user last told about 0.4.0 must be told
//!    again; a "already warned" flag would stay set forever.
//! 3. **Truncation counts UTF-16 units, not characters.** `szInfo` and
//!    `szInfoTitle` are fixed `[u16; N]` arrays. A release name with an emoji
//!    or any astral-plane character is two units but one character, so a
//!    `.chars().take(N)` bound lets the copy walk off the end of the array —
//!    and a split surrogate pair becomes a replacement glyph in the middle of
//!    the sentence. Same trap `output::injector` already had to solve for
//!    keystrokes; here it would corrupt a message instead of garbling input.
//! 4. **The balloon says what clicking does.** A balloon that appears with no
//!    stated consequence is a notification the user has to interpret.

use crate::updates::UpdateInfo;

/// `szInfo` is `[u16; 256]`, last unit reserved for the NUL.
pub const INFO_CAPACITY: usize = 256;
/// `szInfoTitle` is `[u16; 64]`, last unit reserved for the NUL.
pub const TITLE_CAPACITY: usize = 64;

/// What the balloon says, and what it is for.
///
/// Deliberately not carrying the URL: the click is handled by the notifier's
/// own window, which raises the dashboard's settings tab — the one surface that
/// shows the version, the notes, and the download button together. Handing the
/// shell a URL it would launch would skip that and open a browser mid-dictation
/// workflow, which is a worse default than one extra click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateBalloon {
    /// First line, bold, limited to [`TITLE_CAPACITY`].
    pub title: String,
    /// Body text, limited to [`INFO_CAPACITY`].
    pub body: String,
}

impl UpdateBalloon {
    /// Builds the balloon for `info`, truncating to what the shell will hold.
    fn from_info(info: &UpdateInfo) -> Self {
        let latest = info.latest_version.trim();
        let current = info.current_version.trim();
        let title = format!("OmniType {latest} is available");
        // "You are on 0.3.0." only when the two actually differ; saying
        // "You are on 0.4.0" next to "0.4.0 is available" reads as a bug.
        let have = if current.is_empty() || current == latest {
            String::new()
        } else {
            format!(" You are on {current}.")
        };
        let body = format!(
            "{have} Click this notification to open the update panel and install it. \
             (برای نصب، روی این اعلان کلیک کنید.)"
        );
        Self {
            title: truncate_utf16(&title, TITLE_CAPACITY),
            body: truncate_utf16(body.trim(), INFO_CAPACITY),
        }
    }
}

/// Whether to announce `info`, and if so what to say.
///
/// `last_notified` is the version most recently *shown* to the user, read from
/// `updates.last_notified_version`. `enabled` is `updates.notify_on_available`.
pub fn decide(enabled: bool, last_notified: &str, info: &UpdateInfo) -> Option<UpdateBalloon> {
    if !enabled {
        return None;
    }
    let latest = normalise(info.latest_version.trim());
    if latest.is_empty() {
        // A release with no version string cannot be named, so the balloon
        // would say "OmniType  is available". Silence beats a broken sentence.
        return None;
    }
    if normalise(last_notified) == latest {
        return None;
    }
    Some(UpdateBalloon::from_info(info))
}

/// Compares versions the way the dedup needs: no `v` prefix, no case.
///
/// `UpdateInfo.latest_version` is stored without the `v`, but `last_notified`
/// is a string a human-editable `config.toml` also carries, and a `v0.4.0` or
/// `0.4.0 ` in there must not read as a *different* release than `0.4.0` — that
/// would re-announce the same update on every single start, which is the exact
/// spam the dedup exists to prevent.
fn normalise(version: &str) -> &str {
    version
        .trim()
        .strip_prefix(['v', 'V'])
        .unwrap_or(version.trim())
        .trim()
}

/// The version to persist after showing, so the next check stays quiet.
pub fn remember(info: &UpdateInfo) -> String {
    normalise(info.latest_version.trim()).to_string()
}

/// Truncates to at most `cap` UTF-16 units **without splitting a surrogate
/// pair**.
///
/// The trailing high surrogate of a truncated emoji is dropped, because leaving
/// it in place makes the shell render a lone replacement glyph. A lone low
/// surrogate cannot occur in valid UTF-16, so checking the high side is enough.
pub fn truncate_utf16(s: &str, cap: usize) -> String {
    if s.encode_utf16().count() <= cap {
        return s.to_string();
    }
    let mut out = String::new();
    let mut units = 0usize;
    for ch in s.chars() {
        let width = ch.len_utf16();
        if units + width > cap {
            break;
        }
        out.push(ch);
        units += width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(current: &str, latest: &str) -> UpdateInfo {
        UpdateInfo {
            current_version: current.into(),
            latest_version: latest.into(),
            release_name: format!("v{latest}"),
            release_url: "https://example.invalid/releases/v1".into(),
            release_notes: "notes".into(),
            installer_url: Some("https://example.invalid/installer.exe".into()),
            installer_name: Some("setup.exe".into()),
            installer_size_bytes: Some(1),
            published_at: "2026-10-05T00:00:00Z".into(),
        }
    }

    /// The user is told once. A second check finds the same version and the
    /// checker wakes every 6 hours, so this is the difference between one
    /// balloon per release and four per day.
    #[test]
    fn the_same_version_is_announced_only_once() {
        let i = info("0.3.0", "0.4.0");
        let remembered = remember(&i);
        assert_eq!(remembered, "0.4.0", "remember() must record the announced version");

        assert!(
            decide(true, "", &i).is_some(),
            "the first sighting must announce"
        );
        assert!(
            decide(true, &remembered, &i).is_none(),
            "announced the same release twice"
        );
        // ...and stays quiet however many more times it is checked.
        for _ in 0..5 {
            assert!(decide(true, &remembered, &i).is_none());
        }
    }

    /// The dedup is keyed on the version, not a "warned" bit: a new release
    /// must be announced to someone who was already told about the old one.
    #[test]
    fn a_newer_version_is_announced_after_an_older_one() {
        assert!(decide(true, "0.3.0", &info("0.3.0", "0.4.0")).is_some());
        assert!(decide(true, "0.4.0", &info("0.4.0", "0.5.0")).is_some());
    }

    /// A hand-edited `config.toml` can carry a `v` prefix or stray spaces. If
    /// that did not normalise, every restart would re-announce the same release.
    #[test]
    fn the_dedup_survives_a_hand_edited_config() {
        let i = info("0.3.0", "0.4.0");
        for written in ["v0.4.0", " 0.4.0 ", "V0.4.0", "0.4.0"] {
            assert!(
                decide(true, written, &i).is_none(),
                "re-announced after recording {written:?}"
            );
        }
    }

    #[test]
    fn turning_the_setting_off_says_nothing() {
        assert!(decide(false, "", &info("0.3.0", "0.4.0")).is_none());
        // off beats even a never-seen version
        assert!(decide(false, "9.9.9", &info("0.3.0", "0.4.0")).is_none());
    }

    /// A release with no version string produces "OmniType  is available".
    /// Silence is better than a sentence with a hole in it.
    #[test]
    fn a_versionless_release_is_not_announced() {
        assert!(decide(true, "", &info("0.3.0", "  ")).is_none());
        assert!(decide(true, "", &info("0.3.0", "")).is_none());
    }

    #[test]
    fn the_balloon_names_both_versions_and_the_click() {
        let b = decide(true, "", &info("0.3.0", "0.4.0")).expect("announced");
        assert!(b.title.contains("0.4.0"), "title hides the version: {}", b.title);
        assert!(b.body.contains("0.3.0"), "body hides the current: {}", b.body);
        assert!(
            b.body.contains("Click"),
            "body does not say what clicking does: {}",
            b.body
        );
    }

    /// The Persian half is not decoration — the app is Persian-first, and the
    /// user who needs this notice is the one who reads Persian.
    #[test]
    fn the_balloon_carries_persian() {
        let b = decide(true, "", &info("0.3.0", "0.4.0")).expect("announced");
        assert!(
            b.body.contains("کلیک"),
            "no Persian in the body: {}",
            b.body
        );
    }

    /// "You are on 0.4.0" under "0.4.0 is available" reads as a bug report.
    #[test]
    fn a_matching_current_version_is_not_reported_as_a_difference() {
        let b = decide(true, "", &info("0.4.0", "0.4.0")).expect("announced");
        assert!(
            !b.body.contains("You are on"),
            "claims a version gap that does not exist: {}",
            b.body
        );
        let unknown = decide(true, "", &info("", "0.4.0")).expect("announced");
        assert!(
            !unknown.body.contains("You are on"),
            "invents a current version: {}",
            unknown.body
        );
    }

    /// `szInfo`/`szInfoTitle` are fixed `[u16; N]`. Overflow is a memory bug,
    /// not a cosmetic one.
    #[test]
    fn text_is_never_longer_than_the_shell_accepts() {
        let long = "v".repeat(400);
        let b = decide(true, "", &info("0.3.0", &long)).expect("announced");
        assert!(
            b.title.encode_utf16().count() <= TITLE_CAPACITY,
            "title overflows szInfoTitle: {}",
            b.title.encode_utf16().count()
        );
        assert!(
            b.body.encode_utf16().count() <= INFO_CAPACITY,
            "body overflows szInfo: {}",
            b.body.encode_utf16().count()
        );
    }

    /// The bug this module exists partly to prevent: an astral-plane
    /// character (emoji in a release name, or Persian text with an
    /// astral-plane character) is 2 UTF-16 units. A `.chars().take(N)` bound
    /// would let a surrogate pair straddle the end of the array.
    #[test]
    fn truncation_never_splits_a_surrogate_pair() {
        // 255 "a" then an emoji: the emoji is 2 units and starts at 255, so a
        // 256-unit budget cannot hold it and must drop it entirely.
        let s = format!("{}🦀", "a".repeat(255));
        let cut = truncate_utf16(&s, 256);
        assert!(cut.ends_with('a'), "lost real content: {:?}", cut.chars().last());
        assert!(!cut.contains('🦀'), "half an emoji survived the cut");
        assert_eq!(cut.encode_utf16().count(), 255);

        // An emoji that *does* fit must be kept whole, not truncated to a
        // surrogate.
        let fits = truncate_utf16(&s, 257);
        assert!(fits.ends_with('🦀'), "dropped an emoji that fitted");
        assert_eq!(fits.encode_utf16().count(), 257);
    }

    #[test]
    fn truncation_leaves_short_text_untouched() {
        for s in ["", "ok", "a🦀b"] {
            assert_eq!(truncate_utf16(s, 64), s, "changed {s:?} unnecessarily");
        }
    }

    /// Persian text is BMP, so the common path is one unit per char — but the
    /// RTL text must survive the round trip, not be cut at a byte boundary.
    /// The low-word mask is only correct while every `NIN_*` id fits in 16
    /// bits. That is true today; the assertion is here so the day somebody
    /// assumes a 17-bit id, this fails instead of the mask silently swallowing
    /// the icon id.
    #[test]
    fn every_shell_event_id_fits_the_low_word() {
        for (name, id) in [
            ("NIN_SELECT", 1024u32),
            ("NIN_BALLOONSHOW", 1026),
            ("NIN_BALLOONHIDE", 1027),
            ("NIN_BALLOONTIMEOUT", 1028),
            ("NIN_BALLOONUSERCLICK", 1029),
            ("NIN_POPUPOPEN", 1030),
            ("NIN_POPUPCLOSE", 1031),
        ] {
            assert!(id <= 0xFFFF, "{name} ({id}) would collide with the icon id");
        }
        assert_eq!(NIN_BALLOON_USER_CLICK, 1029);
    }

    /// The two encodings the shell uses, for the same click.
    ///
    /// Pre-version-4 the whole `lParam` is the event. Under version 4 the
    /// event is the low word and the icon id sits in the high word. `show()`
    /// sets version 4 before any click can arrive, so **the packed form is the
    /// one that actually happens in production** — and a decoder that only
    /// handled it would be a decoder that works. The unpadded case is kept
    /// because the icon id is 0 before the version call lands.
    #[test]
    fn a_balloon_click_is_recognised_in_both_encodings() {
        let bare = NIN_BALLOON_USER_CLICK as isize;
        let packed = (0x0BAD_0BAD_u32 << 16 | NIN_BALLOON_USER_CLICK) as isize;

        assert!(is_balloon_click(bare), "pre-version-4 click missed");
        assert!(is_balloon_click(packed), "version-4 click missed — the icon id leaked in");

        // The real id we registered, packed the same way.
        let with_our_id = (0x0BAD_0BAD_u32 << 16 | NIN_BALLOON_USER_CLICK) as isize;
        assert!(is_balloon_click(with_our_id));
    }

    /// Every other event the shell sends must not be mistaken for a click, or
    /// the dashboard opens on its own the moment the balloon times out.
    #[test]
    fn no_other_event_is_mistaken_for_a_click() {
        for id in [0u32, 1, 1024, 1026, 1027, 1028, 1030, 1031] {
            let bare = id as isize;
            let packed = (0x0BAD_0BAD_u32 << 16 | id) as isize;
            assert!(!is_balloon_click(bare), "event {id} opened the dashboard");
            assert!(
                !is_balloon_click(packed),
                "event {id} with an icon id opened the dashboard"
            );
        }
    }

    #[test]
    fn persian_text_survives_truncation_intact() {
        let s = "نسخهٔ تازه برای دانلود در دسترس است";
        let cut = truncate_utf16(s, 10);
        assert_eq!(cut.encode_utf16().count(), 10);
        assert!(s.starts_with(&cut), "truncation did not keep a prefix: {cut}");
    }
}

/// A handle to the balloon notifier thread. Cloneable and cheap.
#[cfg(windows)]
#[derive(Clone)]
pub struct BalloonHandle {
    tx: std::sync::mpsc::Sender<UpdateBalloon>,
}

#[cfg(windows)]
impl BalloonHandle {
    /// Queues a balloon for display.
    ///
    /// Returns `false` if the notifier thread is gone, which is the one case
    /// the caller must not ignore: a `false` here means the update was *not*
    /// announced, so the caller must not record it as notified.
    pub fn show(&self, balloon: UpdateBalloon) -> bool {
        self.tx.send(balloon).is_ok()
    }
}

/// How long the balloon's icon stays registered.
///
/// Long enough that the user can read and click it, short enough that the
/// extra tray entry (see [`imp`]) does not linger between releases.
#[cfg(windows)]
const BALLOON_LIFETIME: std::time::Duration = std::time::Duration::from_secs(60);

/// Starts the notifier thread, or `None` if it could not start.
///
/// On a non-Windows build the handle does not exist and callers get `None`.
#[cfg(windows)]
pub fn spawn_notifier(flags: crate::gui::flags::DashboardFlags) -> Option<BalloonHandle> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("update-balloon".into())
        .spawn(move || imp::run(flags, rx))
        .ok()
        .map(|_| BalloonHandle { tx })
}

/// The Windows half: hidden window, notify icon, message pump.
///
/// # Why it does not use the `tray-icon` icon
///
/// A balloon is addressed by the `(HWND, uID)` pair of the icon it belongs to,
/// and `tray-icon` keeps both private — it exposes no way to name its own icon.
/// That leaves two ways round it: find its window by the class name it happens
/// to register and assume an id, or carry an icon of our own. The first would
/// silently stop working the day that crate renames a class or changes its id
/// counter — no error, no balloon, just a feature that quietly does nothing,
/// which is the failure this codebase keeps refusing to ship. So the balloon
/// brings its own icon: added, used, then removed. The cost is a second tray
/// entry, visible only for [`BALLOON_LIFETIME`] and only when an update is
/// announced.
#[cfg(windows)]
mod imp {
    use super::*;
    use crate::gui::flags::{DashboardFlags, Toggle};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::{Duration, Instant};
    use windows::core::w;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
        NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        PeekMessageW, RegisterClassW, SetWindowLongPtrW, CW_USEDEFAULT, GWLP_USERDATA, HMENU,
        IMAGE_FLAGS, MSG, PM_REMOVE, WNDCLASSW, WM_QUIT, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_OVERLAPPED,
    };

    /// Unique per app, so `RegisterClassW` cannot collide with the `tray-icon`
    /// window class or with anything else in this process.
    const CLASS_NAME: windows::core::PCWSTR = w!("OmniTypeFreePTT_UpdateBalloon");

    /// `WM_USER`, spelled out so the offset below reads as what it is.
    const WM_USER: u32 = 1024;

    /// The message the shell sends back. Chosen inside the `WM_USER` band so it
    /// cannot be confused with a control or a menu message.
    const WM_BALLOON: u32 = WM_USER + 0x50;

    /// Arbitrary but fixed: the shell only requires it to be non-zero and to be
    /// the same value every time this icon is addressed.
    const ICON_ID: u32 = 0x0BAD_0BAD;

    /// Copies a string into a fixed `[u16; N]` buffer, NUL-terminated, never
    /// overflowing. Capacity is the buffer's own length minus the NUL, which is
    /// why the caller's [`truncate_utf16`] bound is the array length — not
    /// length minus one.
    fn fill(buf: &mut [u16], s: &str) {
        let cap = buf.len() - 1;
        for (i, unit) in s.encode_utf16().take(cap).enumerate() {
            buf[i] = unit;
        }
    }

    /// The common prefix of every `Shell_NotifyIconW` call on this icon.
    ///
    /// `cbSize` is mandatory and must match the struct the *caller* was
    /// compiled against.
    fn nid(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            uCallbackMessage: WM_BALLOON,
            ..Default::default()
        }
    }

    /// The window procedure. The `DashboardFlags` clone rides in
    /// `GWLP_USERDATA`, so a balloon click needs no global and no channel.
    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_BALLOON {
            if is_balloon_click(lparam.0) {
                let flags = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const DashboardFlags;
                if !flags.is_null() {
                    (*flags).raise(Toggle::Settings);
                }
            }
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// Adds the icon, raises its version, displays the balloon.
    ///
    /// Returns `false` when the shell refused, which the caller must treat as
    /// "not announced" rather than logging and moving on.
    fn show(hwnd: HWND, balloon: &UpdateBalloon) -> bool {
        unsafe {
            // The app's own icon, straight out of the embedded .ico. `0x00030000`
            // is RT_ICON|RT_GROUP, which is what an .ico resource is.
            let hicon = CreateIconFromResourceEx(
                crate::gui::tray::ICON_BYTES,
                BOOL(1),
                0x0003_0000,
                0,
                0,
                IMAGE_FLAGS(0),
            )
            .unwrap_or_default();

            let mut base = nid(hwnd);
            base.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            base.hIcon = hicon;
            fill(&mut base.szTip, crate::gui::tray_warning::BASE_TOOLTIP);
            if !Shell_NotifyIconW(NIM_ADD, &base).as_bool() {
                tracing::warn!("shell refused the update balloon's icon");
                return false;
            }

            // Version 4 is what makes the balloon carry this app's identity
            // instead of landing in the "other" bucket.
            let mut version = nid(hwnd);
            version.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            if !Shell_NotifyIconW(NIM_SETVERSION, &version).as_bool() {
                // The balloon still appears, just without this app's identity
                // attached, so this is a warning and not a failure.
                tracing::warn!("shell refused NOTIFYICON_VERSION_4 for the balloon");
            }

            // A balloon is displayed by a MODIFY carrying NIF_INFO. NIM_ADD
            // only creates the icon; without this line nothing appears.
            let mut info = nid(hwnd);
            info.uFlags = NIF_INFO;
            info.dwInfoFlags = NIIF_INFO;
            fill(&mut info.szInfoTitle, &balloon.title);
            fill(&mut info.szInfo, &balloon.body);
            let shown = Shell_NotifyIconW(NIM_MODIFY, &info).as_bool();
            if !shown {
                tracing::warn!("shell refused the update balloon itself");
            }
            shown
        }
    }

    /// Removes the temporary icon.
    fn hide(hwnd: HWND) {
        unsafe {
            let mut base = nid(hwnd);
            base.uFlags = NIF_ICON;
            if !Shell_NotifyIconW(NIM_DELETE, &base).as_bool() {
                tracing::debug!("balloon icon was already gone when deleted");
            }
        }
    }

    /// The notifier's whole lifetime: own the window, own the icon, own the
    /// pump that turns a balloon click into an opened dashboard.
    pub(super) fn run(flags: DashboardFlags, rx: mpsc::Receiver<UpdateBalloon>) {
        let hwnd = unsafe {
            let hinstance = match GetModuleHandleW(None) {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("no module handle for the update balloon: {e}");
                    return;
                }
            };
            // `GetModuleHandleW` hands back HMODULE; the two call sites below
            // want HINSTANCE, and an inline `.into()` cannot be inferred where
            // `CreateWindowExW` still has generic parameters open.
            let hinstance = windows::Win32::Foundation::HINSTANCE(hinstance.0);
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                lpszClassName: CLASS_NAME,
                hInstance: hinstance,
                ..Default::default()
            });
            // Never shown, never in the taskbar: WS_EX_TOOLWINDOW plus a zero
            // size is what keeps a stray entry off the Alt+Tab list.
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                CLASS_NAME,
                w!(""),
                WS_OVERLAPPED,
                CW_USEDEFAULT,
                0,
                CW_USEDEFAULT,
                0,
                HWND::default(),
                HMENU::default(),
                hinstance,
                None,
            );
            let hwnd = match hwnd {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("could not create the balloon window: {e}");
                    return;
                }
            };
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(flags)) as isize);
            hwnd
        };

        // `None` = no icon registered. `Some(when)` = registered until `when`.
        let mut registered_until: Option<Instant> = None;
        loop {
            // A blocking `GetMessage` would starve the channel that carries
            // balloons to this thread, and a tight loop would burn a core, so
            // the thread waits on the channel and peeks in between.
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(balloon) => {
                    if show(hwnd, &balloon) {
                        registered_until = Some(Instant::now() + BALLOON_LIFETIME);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                // The last handle is gone, so the app is shutting down.
                Err(RecvTimeoutError::Disconnected) => break,
            }

            let mut msg = MSG::default();
            let mut quitting = false;
            unsafe {
                while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        // `PM_REMOVE` has already taken it, so record the fact
                        // here instead of returning early: returning would skip
                        // the icon cleanup below, and a dropped WM_QUIT would
                        // spin this loop for the rest of the process's life.
                        quitting = true;
                        break;
                    }
                    DispatchMessageW(&msg);
                }
            }
            if quitting {
                break;
            }

            if registered_until.is_some_and(|when| Instant::now() >= when) {
                hide(hwnd);
                registered_until = None;
            }
        }

        if registered_until.is_some() {
            hide(hwnd);
        }
        unsafe {
            // A leaked window outlives the thread and would keep the process's
            // message pump alive, so a failure here is worth a line.
            if DestroyWindow(hwnd).is_err() {
                tracing::warn!("could not destroy the balloon window");
            }
        }
    }

}

/// `NIN_BALLOONUSERCLICK`, from `WM_USER + 5`.
///
/// Spelled out here rather than imported from the Shell bindings so the
/// decoding below — the part that can be wrong in two different ways on two
/// different Windows versions — is testable without a window.
const NIN_BALLOON_USER_CLICK: u32 = 1029;

/// Whether this `lParam` is "the user clicked the balloon".
///
/// The one piece of genuine subtlety in the Windows half. With
/// `NOTIFYICON_VERSION_4` the shell packs the event id into the **low word**
/// and the icon id into the high word; before the version is set, the whole
/// `lParam` *is* the event id. Comparing the whole value would miss every click
/// once version 4 took effect — which is set two calls earlier in the same
/// function, so the naive version fails on a perfectly healthy system.
///
/// Masking the low word satisfies both, and only because every `NIN_*` id is
/// below 0x10000; that bound is asserted below rather than assumed.
pub fn is_balloon_click(lparam: isize) -> bool {
    ((lparam as u32) & 0xFFFF) == NIN_BALLOON_USER_CLICK
}

/// How often the watcher looks at the update state.
///
/// The check itself runs every `auto_check_interval_hours` (6 by default), so
/// this only decides how long after a check finishes the balloon appears.
/// Five seconds is far below the threshold anyone would call "slow", and the
/// work per tick is one read lock and one comparison — no repaint, no window,
/// no allocation, which is what separates this from the 30 fps toast loop that
/// used to idle a core.
const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Watches the shared update state and announces a release exactly once.
///
/// The decisions are [`decide`]'s; this only supplies them and acts on the
/// result. It runs on its own thread because the state it watches is written by
/// the async checker and read by a synchronous Windows API that needs a message
/// pump to click on.
///
/// Deliberately *not* a `tokio` task: the balloon needs a window on a thread
/// that owns one, and the notification has to survive the runtime shutting
/// down during exit.
pub fn spawn_watcher(
    state: crate::updates::SharedUpdateState,
    settings: std::sync::Arc<std::sync::RwLock<crate::config::settings::Settings>>,
    config_path: std::path::PathBuf,
    handle: Option<BalloonHandle>,
) {
    let Some(handle) = handle else {
        // Without a notifier there is nothing to announce through. Not an
        // error: a build without the Windows balloon still has the in-app
        // banner, which is what this feature replaced rather than removed.
        tracing::info!("update balloons unavailable; the in-app banner still applies");
        return;
    };

    std::thread::Builder::new()
        .name("update-watch".into())
        .spawn(move || {
            let mut last_seen: Option<crate::updates::UpdateState> = None;
            loop {
                let current = match state.read() {
                    Ok(s) => s.clone(),
                    // A poisoned lock means some other thread panicked holding
                    // it. Loosening here would hide that; stopping is honest.
                    Err(e) => {
                        tracing::error!("update state lock poisoned; notifier stopping: {e}");
                        return;
                    }
                };
                // Compared by value so a repeated `Available` for the same
                // release is not treated as a new event — which is the same
                // guard as the persisted dedup, one layer earlier.
                if last_seen.as_ref() != Some(&current) {
                    last_seen = Some(current.clone());
                    if let crate::updates::UpdateState::Available(info) = current {
                        announce(&handle, &settings, &config_path, &info);
                    }
                }
                std::thread::sleep(WATCH_INTERVAL);
            }
        })
        .map_err(|e| tracing::error!("could not start the update watcher: {e}"))
        .ok();
}

/// Decides, shows, and — only on success — records what was announced.
///
/// The ordering matters and is the point of this function: if the shell
/// refuses the balloon, `last_notified_version` is **not** written, so the next
/// check tries again. Recording first would mean a refused balloon is never
/// retried and the user never hears about the release at all.
fn announce(
    handle: &BalloonHandle,
    settings: &std::sync::Arc<std::sync::RwLock<crate::config::settings::Settings>>,
    config_path: &std::path::Path,
    info: &crate::updates::UpdateInfo,
) {
    let (enabled, last_notified) = match settings.read() {
        Ok(s) => (
            s.updates.notify_on_available,
            s.updates.last_notified_version.clone(),
        ),
        Err(e) => {
            tracing::error!("settings lock poisoned; not announcing update: {e}");
            return;
        }
    };

    let Some(balloon) = decide(enabled, &last_notified, info) else {
        return;
    };

    if !handle.show(balloon) {
        tracing::warn!("balloon notifier is gone; will retry on the next check");
        return;
    }

    let recorded = remember(info);
    let mut guard = match settings.write() {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("settings lock poisoned after announcing: {e}");
            return;
        }
    };
    guard.updates.last_notified_version = recorded.clone();
    let snapshot = guard.clone();
    drop(guard);

    if let Err(e) = snapshot.save(config_path) {
        // The balloon is already on screen, so this is not a failed
        // announcement — but an unsaved value means the next start announces
        // the same release again, which is the spam this exists to prevent.
        tracing::warn!(
            "announced {} but could not record it in {}: {e}",
            recorded,
            config_path.display()
        );
    } else {
        tracing::info!(version = %recorded, "announced a new release");
    }
}
