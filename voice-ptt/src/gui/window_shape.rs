//! Win32 window shaping for the floating overlay windows.
//!
//! Everything this app does to make a frameless `wgpu` window actually
//! transparent on Windows lives here, deliberately away from the egui UI code in
//! [`super::overlay`]: the styles, the DWM attributes, the frame-extension
//! strategies, the per-frame drift guard and the handle bookkeeping are one
//! concern, they are the part that has to be reasoned about against the Windows
//! compositor rather than against a layout, and they were ~650 lines in the
//! middle of a 5 000-line UI file.
//!
//! Evidence for the current defaults lives in
//! `docs/GUI-WINDOW-ARTIFACT-REPORT.md`.

#[cfg(windows)]
#[repr(C)]
struct WinMargins {
    cx_left: i32,
    cx_right: i32,
    cy_top: i32,
    cy_bottom: i32,
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    /// Makes the window manager hit-test only inside `h_rgn`. Passing 0 clears
    /// the region and restores "the whole rect".
    fn SetWindowRgn(hwnd: isize, h_rgn: isize, b_redraw: i32) -> i32;
    /// Reads the window region's bounding box back in window-relative
    /// coordinates. Zero when the window carries no region at all (the box is
    /// then emptied). Needs no buffer, so it is cheap enough to ask once a
    /// frame — which is the point: it is the only way to tell whether the
    /// window still has the region this process last gave it.
    fn GetWindowRgnBox(hwnd: isize, lp_rect: *mut windows::Win32::Foundation::RECT) -> i32;
    fn SystemParametersInfoW(
        ui_action: u32,
        ui_param: u32,
        pv_param: *mut windows::Win32::Foundation::RECT,
        f_win_ini: u32,
    ) -> i32;
    fn GetWindowRect(hwnd: isize, lp_rect: *mut windows::Win32::Foundation::RECT) -> i32;
    fn GetClientRect(hwnd: isize, lp_rect: *mut windows::Win32::Foundation::RECT) -> i32;
    /// Zero on pre-1607 Windows, where callers must fall back to 96.
    fn GetDpiForWindow(hwnd: isize) -> u32;
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn RedrawWindow(
        hwnd: isize,
        lprc_update: *const std::ffi::c_void,
        hrgn_update: isize,
        flags: u32,
    ) -> i32;
}

/// `RDW_*` flags for [`RedrawWindow`].
///
/// phase 3.3: `RDW_ERASE` and `RDW_FRAME` are deliberately **not** set: asking
/// DWM to repaint the frame region of a transparent window lands as a light
/// layer over the parts egui never paints.
#[cfg(windows)]
const RDW_FORCE_REPAINT: u32 = 0x0001 /* INVALIDATE */
    | 0x0020 /* ALLCHILDREN */
    | 0x0100 /* UPDATENOW */;

#[cfg(windows)]
#[link(name = "gdi32")]
extern "system" {
    fn CreateEllipticRgn(l: i32, t: i32, r: i32, b: i32, f: i32) -> isize;
    fn CreateRoundRectRgn(l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> isize;
    fn CombineRgn(dst: isize, src1: isize, src2: isize, mode: i32) -> i32;
    fn DeleteObject(ho: *mut std::ffi::c_void) -> i32;
}

/// `RGN_OR` from `wingdi.h`: the union of the two source regions.
#[cfg(windows)]
const RGN_OR: i32 = 2;
/// `ERROR`, the value `wingdi.h` gives `CombineRgn` for "could not build it".
#[cfg(windows)]
const RGN_ERROR: i32 = 0;

#[cfg(windows)]
#[link(name = "dwmapi")]
extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: isize, p_mar_inset: *const WinMargins) -> i32;
    fn DwmSetWindowAttribute(
        hwnd: isize,
        dw_attribute: u32,
        pv_attribute: *const std::ffi::c_void,
        cb_attribute: u32,
    ) -> i32;
}

/// How the frameless overlay windows obtain per-pixel transparency.
///
/// Picked once at startup from the `OMNITYPE_TRANSPARENCY` environment variable
/// so the candidates can be A/B'd on **one** build. See
/// `docs/GUI-WINDOW-ARTIFACT-REPORT.md` §12 for the measurements.
///
/// * `swapchain` (**default**) — frameless `WS_POPUP` +
///   `DWMWA_WINDOW_CORNER_PREFERENCE=DONOTROUND` only. Transparency comes from
///   the swapchain's premultiplied alpha, which egui-wgpu sets from
///   `ViewportBuilder::with_transparent`.
/// * `dwm-extend` — the phase-3 path: `DWMWA_NCRENDERING_POLICY=DISABLED`, no
///   border colour, no system backdrop, `DwmExtendFrameIntoClientArea(-1)`.
/// * `dwm-extend-0` — the same but without extending the frame.
///
/// Measured on this machine: the pale 28 px band at the top of the orb window is
/// **identical in all three**, so none of these attributes is responsible for it.
#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransparencyMode {
    DwmExtendFrame,
    DwmExtendNone,
    SwapchainAlpha,
    /// (**default**) Touch nothing but DWM attributes: no `SetWindowLongW`, no
    /// `SWP_FRAMECHANGED`, no frame extension, no per-frame style repair.
    ///
    /// This is the fix. The style rewrite this app has always done is what made
    /// DWM compose a caption for the window: a light 28 px band across the top
    /// (the Windows 11 small-caption height at 125%), closed by a 1 px
    /// separator. Measured with `artifact-repro.ps1`:
    ///
    /// ```text
    /// nostyle     row4=(20,22,28)     row40=(20,22,28)     strip=no
    /// swapchain   row4=(178,205,235)  row40=(20,22,28)     strip=YES rows=28
    /// dwm-extend  row4=(178,205,235)  row40=(20,22,28)     strip=YES rows=28
    /// ```
    ///
    /// None of the DWM attributes matter — only the style write does. winit's
    /// own `WM_NCCALCSIZE` handling already makes the client area cover the whole
    /// window, so the window is visually frameless without any help from us.
    NoStyleRewrite,
}

#[cfg(windows)]
impl TransparencyMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DwmExtendFrame => "dwm-extend",
            Self::DwmExtendNone => "dwm-extend-0",
            Self::SwapchainAlpha => "swapchain",
            Self::NoStyleRewrite => "nostyle",
        }
    }
}

/// Maps the raw `OMNITYPE_TRANSPARENCY` value to a strategy. Split out from
/// [`transparency_mode`] so it is testable without touching the environment.
#[cfg(windows)]
fn transparency_mode_from(raw: &str) -> TransparencyMode {
    match raw.trim().to_ascii_lowercase().as_str() {
        "dwm-extend-0" | "dwm_extend_0" | "extend0" => TransparencyMode::DwmExtendNone,
        "dwm-extend" | "dwm_extend" | "extend" => TransparencyMode::DwmExtendFrame,
        "nostyle" | "no-style" => TransparencyMode::NoStyleRewrite,
        "swapchain" | "alpha" => TransparencyMode::SwapchainAlpha,
        _ => TransparencyMode::NoStyleRewrite,
    }
}

/// Reads `OMNITYPE_TRANSPARENCY` once. Unset/unknown ⇒ [`TransparencyMode::SwapchainAlpha`].
#[cfg(windows)]
fn transparency_mode() -> &'static TransparencyMode {
    static MODE: std::sync::OnceLock<TransparencyMode> = std::sync::OnceLock::new();
    MODE.get_or_init(|| {
        let raw = std::env::var("OMNITYPE_TRANSPARENCY").unwrap_or_default();
        let mode = transparency_mode_from(&raw);
        tracing::info!(
            mode = mode.as_str(),
            "transparency strategy (OMNITYPE_TRANSPARENCY): swapchain = frameless popup \
             + premultiplied-alpha swapchain only, no DwmExtendFrameIntoClientArea"
        );
        mode
    })
}

/// One-time geometry log, in physical pixels, for the app's own log file: the
/// numbers an artifact screenshot has to be explained by. (Window probes run
/// from PowerShell are not DPI-aware and report these divided by 1.25.)
#[cfg(windows)]
fn log_window_geometry(hwnd: isize) {
    use windows::Win32::Foundation::RECT;
    let mut outer = RECT::default();
    let mut client = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut outer);
        GetClientRect(hwnd, &mut client);
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    tracing::info!(
        hwnd,
        dpi,
        ppp = dpi as f32 / 96.0,
        window_w = outer.right - outer.left,
        window_h = outer.bottom - outer.top,
        client_w = client.right - client.left,
        client_h = client.bottom - client.top,
        mode = transparency_mode().as_str(),
        "overlay window geometry (physical pixels; probes outside this process see these / 1.25)"
    );
}

/// `DWMWA_*` attributes this app sets. Spelled out because the `windows` crate
/// is built without the `Win32_Graphics_Dwm` feature.
#[cfg(windows)]
mod dwm_attr {
    /// `DWMWA_WINDOW_CORNER_PREFERENCE`
    pub const WINDOW_CORNER_PREFERENCE: u32 = 33;
    /// `DWMWA_BORDER_COLOR`
    pub const BORDER_COLOR: u32 = 34;
    /// `DWMWA_SYSTEMBACKDROP_TYPE`
    pub const SYSTEMBACKDROP_TYPE: u32 = 38;
}

/// One `DwmSetWindowAttribute` call with a `u32` payload.
#[cfg(windows)]
fn dwm_set_u32(hwnd: isize, attribute: u32, value: u32) {
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            attribute,
            &value as *const _ as *const std::ffi::c_void,
            4,
        );
    }
}

/// The **one** Win32 correction every viewport window in this app gets, however
/// it was created.
///
/// # Why it is idempotent and gets re-applied
///
/// It is deliberately limited to four calls, none of which asks DWM to
/// recompose the window (no `SetWindowPos(SWP_FRAMECHANGED)`, no style write),
/// so running it again is a no-op rather than a way to conjure a caption — the
/// per-frame `SetWindowLongW` guard used to do exactly that, ~2 300 times in 16
/// seconds. Being safe to repeat is what lets it run on a timer for *every*
/// viewport instead of racing window creation exactly once.
///
/// # What it fixes
///
/// 1. `disable_winit_blur_behind` — undoes the `DwmEnableBlurBehindWindow`
///    that winit performs in `Window::on_create` for every window built with
///    `with_transparent(true)`. See that function's docs for the Win11
///    mechanism; this is the single most important line here.
/// 2. Rounded corners off — without it DWM rounds the *backdrop* corners of a
///    frameless popup, which is the "the corners of the pale box are sometimes
///    round and sometimes square" behaviour.
/// 3. No system backdrop, no border colour — belt and braces for the case where
///    (1) is not enough on a given Windows build.
///
/// (rollback: every call here is independent; delete one without touching the
/// others. Re-enabling `TransparencyMode::SwapchainAlpha` is *not* a rollback
/// of this function, it is the style-rewrite path documented on
/// [`TransparencyMode::NoStyleRewrite`].)
#[cfg(windows)]
pub fn apply_viewport_transparency(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    disable_winit_blur_behind(hwnd);
    dwm_set_u32(hwnd, dwm_attr::WINDOW_CORNER_PREFERENCE, 1); // DWMWCP_DONOTROUND
    dwm_set_u32(hwnd, dwm_attr::BORDER_COLOR, 0xFFFF_FFFE); // DWMWA_COLOR_NONE
    dwm_set_u32(hwnd, dwm_attr::SYSTEMBACKDROP_TYPE, 1); // DWMSBT_NONE
}

/// The last `(hwnd, region)` handed to the window manager, so an unchanged
/// shape costs nothing.
#[cfg(windows)]
static CLICK_REGION_CACHE: std::sync::Mutex<Option<(isize, ClickRegion)>> =
    std::sync::Mutex::new(None);

/// The `HWND` of the transcript card window, or 0 when it has not been created
/// yet. Read by [`crate::gui::preview_window`] to attach a click region to the
/// card without a second window walk.
#[cfg(windows)]
pub fn preview_hwnd() -> isize {
    PREVIEW_HWND.load(std::sync::atomic::Ordering::Relaxed)
}

/// Where a viewport window accepts clicks, in **points** relative to its own
/// top-left corner.
///
/// # Why this and not `WS_EX_TRANSPARENT`
///
/// `WS_EX_TRANSPARENT` is all-or-nothing: with it the window cannot be clicked
/// at all, without it the window swallows every click over its whole rect. For
/// the orb that is unusable — it has to stay draggable — and for the card it
/// costs click-to-dismiss and, because winit pairs the bit with
/// `WS_EX_LAYERED` ([winit window_state.rs:300-301][1]), it puts a layered
/// style on a window whose pixels come from a `wgpu` swapchain.
///
/// A **window region** is the other half of Win32 hit-testing: `WindowFromPoint`
/// only ever offers a point to a window whose region contains it, so a region
/// smaller than the window lets the desktop through everywhere else while the
/// window itself stays fully interactive.
///
/// `WM_NCHITTEST` returning `HTTRANSPARENT` looks like a third option and is
/// not: it only skips windows **in the same thread**, and the windows being
/// protected are other processes'.
///
/// [1]: https://docs.rs/winit/0.30/winit/0.30.13/src/winit/platform_impl/windows/window_state.rs.html#300-301
#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ClickRegion {
    /// No region: the window takes the hit over its whole rect.
    Full,
    /// A circle centred on the window.
    Circle { radius_pt: f32 },
    /// A rounded rectangle. `rect_pt` is `[left, top, right, bottom]`.
    RoundedRect { rect_pt: [f32; 4], radius_pt: f32 },
    /// A circle centred on the window, with a rounded bar hanging below it.
    ///
    /// The orb's shape while it names the application profile in force. Two
    /// shapes OR'd together rather than one tall rounded rectangle, because the
    /// rectangle would also claim the two crescents beside the circle's lower
    /// half — pixels that paint nothing and would swallow clicks meant for the
    /// desktop, which is the same defect the circle itself was introduced to
    /// remove.
    CircleWithPill {
        radius_pt: f32,
        /// The bar, `[left, top, right, bottom]`, window-relative points.
        pill_pt: [f32; 4],
        pill_radius_pt: f32,
    },
}

/// Converts a region to physical pixels against a window of `window_pt`.
///
/// Returns `None` for any shape that would describe an empty or out-of-window
/// region.
///
/// **What the caller does with that is "keep whatever region the window
/// already has", not the `ClickRegion::Full` this note used to promise.**
/// `apply_click_region` returns early instead (see `window_shape.rs:558`), so a
/// window that has never had a region keeps taking clicks over its whole rect,
/// and one that has keeps a region that may no longer match its shape. Whether
/// clearing to `Full` would be better is an open question — clearing means the
/// whole rect swallows desktop clicks, which is the defect the region exists
/// to remove — so the behaviour is left alone until someone can see it happen
/// (Q1-4, unconfirmed: no one has reached this state).
///
/// `SetWindowRgn` fails on an empty region, which is why an empty shape cannot
/// simply be applied.
#[cfg(windows)]
pub fn click_region_px(region: ClickRegion, window_pt: [f32; 2], ppp: f32) -> Option<ClickRegion> {
    /// `NaN` and non-positive both have to be rejected: `NaN` would otherwise
    /// sail through every comparison and reach `SetWindowRgn` as a zero-size
    /// region.
    fn usable(v: f32) -> bool {
        v.is_finite() && v > 0.0
    }

    let (window_w, window_h) = (window_pt[0], window_pt[1]);
    if !usable(window_w) || !usable(window_h) || !usable(ppp) {
        return Some(ClickRegion::Full);
    }
    match region {
        ClickRegion::Full => Some(ClickRegion::Full),
        ClickRegion::Circle { radius_pt } => Some(ClickRegion::Circle {
            radius_pt: clamp_circle_radius_pt(radius_pt, window_pt, ppp),
        }),
        ClickRegion::RoundedRect { rect_pt, radius_pt } => {
            clamp_rect_px(rect_pt, radius_pt, window_pt, ppp)
                .map(|(rect_pt, radius_pt)| ClickRegion::RoundedRect { rect_pt, radius_pt })
        }
        ClickRegion::CircleWithPill {
            radius_pt,
            pill_pt,
            pill_radius_pt,
        } => {
            let radius_pt = clamp_circle_radius_pt(radius_pt, window_pt, ppp);
            match clamp_rect_px(pill_pt, pill_radius_pt, window_pt, ppp) {
                Some((pill_pt, pill_radius_pt)) => Some(ClickRegion::CircleWithPill {
                    radius_pt,
                    pill_pt,
                    pill_radius_pt,
                }),
                // A bar with no area is not a reason to give up the circle that
                // still describes the orb — and `Full` would hand the whole
                // transparent canvas back to the desktop's clicks.
                None => Some(ClickRegion::Circle { radius_pt }),
            }
        }
    }
}

/// The circle's radius after rounding to whole pixels and clamping it to the
/// window, in points.
///
/// One function for the two shapes that carry a circle — the orb alone and the
/// orb with its profile pill — so the "the region has to fit inside the window,
/// or it is clipped to the window anyway and the shape bought nothing" rule
/// cannot be honoured in one place and forgotten in the other.
#[cfg(windows)]
fn clamp_circle_radius_pt(radius_pt: f32, window_pt: [f32; 2], ppp: f32) -> f32 {
    let radius_px = (radius_pt * ppp).round() as i32;
    let max_px = ((window_pt[0].min(window_pt[1]) * ppp) * 0.5).round() as i32;
    radius_px.clamp(1, max_px.max(1)) as f32 / ppp
}

/// A rectangle after rounding to whole pixels and clamping it — corners
/// included — to the window, or `None` when what is left has no area.
///
/// `None` rather than a zero-size region because `SetWindowRgn` fails on an
/// empty region and, handed one that degenerates at runtime, leaves a window
/// nobody can see and everybody can click.
#[cfg(windows)]
fn clamp_rect_px(
    rect_pt: [f32; 4],
    radius_pt: f32,
    window_pt: [f32; 2],
    ppp: f32,
) -> Option<([f32; 4], f32)> {
    let mut left = (rect_pt[0] * ppp).round() as i32;
    let mut top = (rect_pt[1] * ppp).round() as i32;
    let mut right = (rect_pt[2] * ppp).round() as i32;
    let mut bottom = (rect_pt[3] * ppp).round() as i32;
    left = left.clamp(0, (window_pt[0] * ppp).round() as i32);
    top = top.clamp(0, (window_pt[1] * ppp).round() as i32);
    right = right.clamp(0, (window_pt[0] * ppp).round() as i32);
    bottom = bottom.clamp(0, (window_pt[1] * ppp).round() as i32);
    if right - left < 2 || bottom - top < 2 {
        return None;
    }
    let corner = (radius_pt * ppp).round().max(0.0) as i32;
    let max_corner = ((right - left) / 2).min((bottom - top) / 2);
    let corner = corner.min(max_corner.max(0));
    Some((
        [
            left as f32 / ppp,
            top as f32 / ppp,
            right as f32 / ppp,
            bottom as f32 / ppp,
        ],
        corner as f32 / ppp,
    ))
}

/// The box, in window-relative pixels, that giving `px` to a window of
/// `side_px` produces — or `None` when it means "this window carries no region
/// at all".
///
/// Derived from the very coordinates handed to `CreateEllipticRgn` /
/// `CreateRoundRectRgn` below, because that is the only place the two can be
/// kept honest: [`window_region_box`] reads back exactly this shape's bounding
/// box, so comparing them is a real check rather than a comparison of two
/// separately-derived guesses.
#[cfg(windows)]
fn expected_region_box(px: ClickRegion, side_px: [i32; 2], ppp: f32) -> Option<[i32; 4]> {
    let (side_w, side_h) = (side_px[0].max(0), side_px[1].max(0));
    match px {
        // `SetWindowRgn(hwnd, 0, 1)`, and `GetWindowRgnBox` reports nothing.
        ClickRegion::Full => None,
        // `right`/`bottom` are exclusive in both GDI region calls, which is why
        // the `+ 1` here matches the `+ 1` handed to `CreateEllipticRgn`.
        ClickRegion::Circle { radius_pt } => {
            let r = (radius_pt * ppp).round() as i32;
            let (cx, cy) = (side_w / 2, side_h / 2);
            Some([cx - r, cy - r, cx + r + 1, cy + r + 1])
        }
        ClickRegion::RoundedRect { rect_pt, .. } => Some([
            (rect_pt[0] * ppp).round() as i32,
            (rect_pt[1] * ppp).round() as i32,
            (rect_pt[2] * ppp).round() as i32,
            (rect_pt[3] * ppp).round() as i32,
        ]),
        // The union's bounding box is the union of the two bounding boxes, and
        // each half is derived from the very coordinates its own GDI call gets.
        ClickRegion::CircleWithPill {
            radius_pt,
            pill_pt,
            ..
        } => {
            let r = (radius_pt * ppp).round() as i32;
            let (cx, cy) = (side_w / 2, side_h / 2);
            Some([
                (cx - r).min((pill_pt[0] * ppp).round() as i32),
                (cy - r).min((pill_pt[1] * ppp).round() as i32),
                (cx + r + 1).max((pill_pt[2] * ppp).round() as i32),
                (cy + r + 1).max((pill_pt[3] * ppp).round() as i32),
            ])
        }
    }
}

/// What the window says it is carrying: its region's bounding box in
/// window-relative pixels, or `None` when it carries no region.
///
/// One `user32` call, no allocation, no buffer — cheap enough to be the thing
/// that makes [`apply_click_region`]'s cache trustworthy instead of merely
/// self-consistent.
#[cfg(windows)]
fn window_region_box(hwnd: isize) -> Option<[i32; 4]> {
    use windows::Win32::Foundation::RECT;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // Fails for a window with no region, which is exactly the case that has to
    // read as `None`.
    if unsafe { GetWindowRgnBox(hwnd, &mut rect) } == 0 {
        return None;
    }
    Some([rect.left, rect.top, rect.right, rect.bottom])
}

/// Whether [`apply_click_region`] may skip the Win32 call this frame.
///
/// Two independent things have to agree, and the second one is the point:
///
/// * the cache says this process already asked for this handle and shape, and
/// * the window itself reports that shape.
///
/// Either alone is insufficient. The cache alone went stale when Windows rebuilt
/// the OS window behind the same `HWND` value — the handle comparison still
/// matched, so `SetWindowRgn` was never called again and the rebuilt window
/// claimed clicks over its whole square forever. Asking the window is the only
/// check that survives that, because it reads the state that actually matters.
#[cfg(windows)]
fn region_is_current(
    cached: Option<(isize, ClickRegion)>,
    hwnd: isize,
    px: ClickRegion,
    side_px: [i32; 2],
    ppp: f32,
    observed: Option<[i32; 4]>,
) -> bool {
    cached == Some((hwnd, px)) && observed == expected_region_box(px, side_px, ppp)
}

/// Records the shape the window is now carrying, after `SetWindowRgn` succeeded.
///
/// Only ever called on the success path: a cache that stores an *intent* the
/// window manager refused is exactly how a cache starts lying.
#[cfg(windows)]
fn cache_click_region(hwnd: isize, px: ClickRegion) {
    if let Ok(mut last) = CLICK_REGION_CACHE.lock() {
        *last = Some((hwnd, px));
    }
}

/// Applies a click region, skipping the call when it has not changed.
///
/// `SetWindowRgn` makes the window manager send `WM_WINDOWPOSCHANGING` and
/// `WM_WINDOWPOSCHANGED`, so calling it every frame would put the window
/// through a spurious move/activate cycle 60 times a second for a shape that
/// only changes when the orb animates.
///
/// The cache is a *hint*, not the truth: [`region_is_current`] also asks the
/// window what it is actually carrying. That is what covers the one case the
/// cache alone cannot — the OS window being rebuilt behind the same `HWND`
/// value, which [`super::window_shape::register_main_hwnd`] deliberately reports
/// as "unchanged" precisely because the value did not change.
#[cfg(windows)]
pub fn apply_click_region(hwnd: isize, region: ClickRegion, window_pt: [f32; 2], ppp: f32) {
    if hwnd == 0 {
        return;
    }
    let Some(px) = click_region_px(region, window_pt, ppp) else {
        return;
    };

    // Half-pixel inset: a region that exactly touches the window edge can leave
    // a hairline of unowned pixels along the far sides.
    let side_w = (window_pt[0] * ppp).round() as i32;
    let side_h = (window_pt[1] * ppp).round() as i32;
    let side_px = [side_w, side_h];
    {
        let Ok(last) = CLICK_REGION_CACHE.lock() else {
            return;
        };
        if region_is_current(*last, hwnd, px, side_px, ppp, window_region_box(hwnd)) {
            return;
        }
    }

    let rgn: isize = match px {
        ClickRegion::Full => {
            let ok = unsafe { SetWindowRgn(hwnd, 0, 1) };
            if ok == 0 {
                tracing::debug!(hwnd, "clearing the window region failed");
            } else {
                cache_click_region(hwnd, px);
                force_repaint(hwnd);
            }
            return;
        }
        ClickRegion::Circle { radius_pt } => {
            let r = (radius_pt * ppp).round() as i32;
            let cx = side_w / 2;
            let cy = side_h / 2;
            unsafe { CreateEllipticRgn(cx - r, cy - r, cx + r + 1, cy + r + 1, 1) }
        }
        ClickRegion::RoundedRect { rect_pt, radius_pt } => {
            let l = (rect_pt[0] * ppp).round() as i32;
            let t = (rect_pt[1] * ppp).round() as i32;
            let r = (rect_pt[2] * ppp).round() as i32;
            let b = (rect_pt[3] * ppp).round() as i32;
            let c = (radius_pt * ppp).round() as i32;
            unsafe { CreateRoundRectRgn(l, t, r, b, c * 2, c * 2) }
        }
        ClickRegion::CircleWithPill {
            radius_pt,
            pill_pt,
            pill_radius_pt,
        } => {
            let r = (radius_pt * ppp).round() as i32;
            let (cx, cy) = (side_w / 2, side_h / 2);
            let circle = unsafe { CreateEllipticRgn(cx - r, cy - r, cx + r + 1, cy + r + 1, 1) };
            let l = (pill_pt[0] * ppp).round() as i32;
            let t = (pill_pt[1] * ppp).round() as i32;
            let rr = (pill_pt[2] * ppp).round() as i32;
            let b = (pill_pt[3] * ppp).round() as i32;
            let c = (pill_radius_pt * ppp).round() as i32;
            let pill = unsafe { CreateRoundRectRgn(l, t, rr, b, c * 2, c * 2) };
            if circle == 0 || pill == 0 {
                // Neither handle may be leaked, and neither is safe to hand to
                // the window: a half-built region would carve the orb's own
                // clicks away.
                unsafe {
                    if circle != 0 {
                        DeleteObject(circle as *mut std::ffi::c_void);
                    }
                    if pill != 0 {
                        DeleteObject(pill as *mut std::ffi::c_void);
                    }
                }
                tracing::debug!(hwnd, "could not build one half of the orb region");
                return;
            }
            // The union is written into the destination, so the circle is the
            // handle that survives this and the bar is handed back — `SetWindowRgn`
            // takes ownership of exactly one region and the system deletes it.
            let kind = unsafe { CombineRgn(circle, circle, pill, RGN_OR) };
            unsafe { DeleteObject(pill as *mut std::ffi::c_void) };
            if kind == RGN_ERROR {
                unsafe { DeleteObject(circle as *mut std::ffi::c_void) };
                tracing::debug!(hwnd, "could not union the orb region");
                return;
            }
            circle
        }
    };

    if rgn == 0 {
        tracing::debug!(hwnd, ?px, "could not build the window region");
        return;
    }
    let ok = unsafe { SetWindowRgn(hwnd, rgn, 1) };
    if ok == 0 {
        // The call failed, so ownership never transferred and the region is
        // still ours to free.
        unsafe { DeleteObject(rgn as *mut std::ffi::c_void) };
        tracing::debug!(hwnd, ?px, "SetWindowRgn failed");
        return;
    }
    // On success the system owns the region and must not be told to delete it.
    cache_click_region(hwnd, px);

    // Repaint, or the old boundary stays on screen.
    //
    // `SetWindowRgn(..., TRUE)` asks for a redraw, but all it delivers is
    // `WM_WINDOWPOSCHANGED` — and DWM composes a per-pixel-alpha window from
    // its *cached* redirection surface, which that message does not invalidate.
    // So when the region shrinks (the orb returning to idle drops it from the
    // `Recording` radius of 96 pt to 58 pt), DWM keeps showing the last frame
    // that was composed with the larger region. The symptom is a hard-edged
    // arc of the old circle, left floating above the orb where nothing is
    // painted.
    //
    // `RDW_ERASE`/`RDW_FRAME` are deliberately not set — asking DWM to repaint
    // the frame region of a transparent window lands as a light layer over the
    // parts egui never paints, which is the pale box this module exists to
    // prevent. INVALIDATE + UPDATENOW only forces the next presented frame to
    // be composited, which is what actually needs to happen.
    force_repaint(hwnd);
}

/// Drops the cached region so the next [`apply_click_region`] call always
/// reaches the window manager.
///
/// Deliberately **not** wired into production any more, and that is the outcome
/// rather than an omission. It used to be the only answer to "the window may not
/// be carrying the region this process last gave it", and it needed a caller who
/// could *detect* that — which nobody can, because the failure it was written
/// for (Windows rebuilding the OS window behind the same `HWND` value) is by
/// definition invisible to a handle comparison. The read-back in
/// [`window_region_box`] detects it without a caller, so this is left as the
/// hook tests use to prove the cache is consulted at all.
#[cfg(all(windows, test))]
fn cached_click_region() -> Option<(isize, ClickRegion)> {
    CLICK_REGION_CACHE.lock().ok().and_then(|c| *c)
}

#[cfg(windows)]
#[cfg_attr(not(test), allow(dead_code))]
pub fn invalidate_click_region() {
    if let Ok(mut last) = CLICK_REGION_CACHE.lock() {
        *last = None;
    }
}

/// Cancels the blur-behind layer that winit puts on every window created with
/// `WindowBuilder::with_transparent(true)`.
///
/// # Why this exists
///
/// On Windows, `with_transparent` does **not** mean "give me an alpha
/// channel". winit 0.30.13 implements it as
/// `DwmEnableBlurBehindWindow(hwnd, { DWM_BB_ENABLE | DWM_BB_BLURREGION, hRgnBlur:
/// CreateRectRgn(0, 0, -1, -1) })` — an *empty* blur region
/// (`winit-0.30.13/src/platform_impl/windows/window.rs:1231-1246`), which on
/// Windows 11 leaves the light-theme backdrop behind the client area. Actual
/// per-pixel transparency on this platform comes from the swapchain's
/// `CompositeAlphaMode` instead, which egui-wgpu already sets to
/// `PreMultiplied` because the app asks for a transparent viewport.
///
/// The two are independent, so `with_transparent` only ever *added* the pale
/// layer. The measurements in `docs/GUI-WINDOW-ARTIFACT-REPORT.md` §13 are the
/// proof: the pixels immediately inside the transcript card's window are
/// `rgb(248,248,248)` while the pixels immediately outside it are
/// `rgb(3,8,10)`, and the same window with identical styles and identical
/// DWM attributes as the orb — which does not set `with_transparent` — has no
/// such band.
///
/// Calling `DwmEnableBlurBehindWindow` again with `DWM_BB_ENABLE` cleared and a
/// null region is the documented way to remove the effect; it is also exactly
/// what winit would have done had `transparent` been false.
///
/// This is a *correction*, not a deletion: the caller still passes
/// `with_transparent(true)`, which is what still gets egui-wgpu the
/// `PreMultiplied` alpha mode this app's transparency depends on.
/// (rollback: remove the call in `apply_dwm_attributes_only`.)
#[cfg(windows)]
fn disable_winit_blur_behind(hwnd: isize) {
    use windows::Win32::Foundation::{BOOL, HWND};
    use windows::Win32::Graphics::Dwm::{DwmEnableBlurBehindWindow, DWM_BLURBEHIND};
    use windows::Win32::Graphics::Gdi::HRGN;

    // All-zero is the documented "turn it off" form: no DWM_BB_ENABLE flag, no
    // region. winit set it on with an empty region; this puts it back.
    let behind = DWM_BLURBEHIND {
        dwFlags: 0,
        fEnable: BOOL(0),
        hRgnBlur: HRGN(std::ptr::null_mut()),
        fTransitionOnMaximized: BOOL(0),
    };
    let result = unsafe { DwmEnableBlurBehindWindow(HWND(hwnd as *mut std::ffi::c_void), &behind) };
    if let Err(err) = result {
        tracing::debug!(hwnd, ?err, "could not clear the winit blur-behind layer");
    }
}

#[cfg(windows)]
pub fn enable_true_transparency(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, GWL_STYLE, WS_BORDER, WS_CAPTION,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    let mode = *transparency_mode();
    let win_hwnd = HWND(hwnd as *mut std::ffi::c_void);
    unsafe {
        // 1. Strip non-client styles (caption, thickframe, min/max buttons, sysmenu, borders) FIRST
        let cur_style = GetWindowLongW(win_hwnd, GWL_STYLE);
        if mode == TransparencyMode::NoStyleRewrite {
            apply_viewport_transparency(hwnd);
            return;
        }
        let stripped = (cur_style as u32
            & !(WS_CAPTION.0
                | WS_THICKFRAME.0
                | WS_MINIMIZEBOX.0
                | WS_MAXIMIZEBOX.0
                | WS_SYSMENU.0
                | WS_BORDER.0
                | 0x0080_0000/* WS_DLGFRAME */))
            | WS_POPUP.0;
        let style_changed = stripped as i32 != cur_style;
        let _ = SetWindowLongW(win_hwnd, GWL_STYLE, stripped as i32);

        // 2. Strip extended styles (sunken/raised edges, static edges, dialog frame)
        let cur_ex = GetWindowLongW(win_hwnd, GWL_EXSTYLE);
        let stripped_ex = cur_ex as u32
            & !(0x0000_0100 /* WS_EX_WINDOWEDGE */
                | 0x0000_0200 /* WS_EX_CLIENTEDGE */
                | 0x0002_0000 /* WS_EX_STATICEDGE */
                | 0x0000_0001/* WS_EX_DLGMODALFRAME */);
        let ex_changed = stripped_ex as i32 != cur_ex;
        let _ = SetWindowLongW(win_hwnd, GWL_EXSTYLE, stripped_ex as i32);

        // 3. Clear window title string from OS window so Windows never renders "OmniType"
        // phase 1: the window title is never cleared any more. Clearing it made
        // this window indistinguishable from winit's internal event-target
        // window and the tray-icon message window (all three ended up
        // title-less), which is how the wrong window got captured above.
        // let _ = SetWindowTextW(win_hwnd, windows::core::w!(""));

        // 4-6. Corners off, no border colour, no system backdrop, and the
        // blur-behind layer winit adds for `with_transparent` cancelled.
        // Shared with [`apply_viewport_transparency`], which is the same set of
        // calls minus the style rewriting below.
        apply_viewport_transparency(hwnd);

        if mode == TransparencyMode::SwapchainAlpha {
            // Steps 7-8 of the phase-3 path (NC rendering disabled, frame
            // extended over the whole client) are deliberately skipped: on a GPU
            // swapchain neither is needed for alpha —
            // `ViewportBuilder::with_transparent` already gives the surface a
            // premultiplied alpha mode — and together they are the only thing in
            // the app that asks DWM to treat the *entire* window as a frame it
            // owns. Leaving NC rendering at its Windows default also keeps DWM
            // compositing the window the ordinary way.
            // (rollback: OMNITYPE_TRANSPARENCY=dwm-extend restores the old path.)
        } else {
            // 7. DWMWA_NCRENDERING_POLICY = 2, DWMNCRP_DISABLED = 1
            // Disables non-client area rendering and window drop shadow
            let ncrp_disabled: u32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                2,
                &ncrp_disabled as *const _ as *const std::ffi::c_void,
                4,
            );

            // 8. Extend the DWM frame into the client area. With -1 margins the
            // *whole window* is non-client as far as DWM is concerned, so every
            // pixel egui does not paint is a pixel DWM may fill with a light
            // backdrop layer. `dwm-extend-0` turns this one call into the A/B
            // that proves or kills it without touching anything else.
            let extend_by = if mode == TransparencyMode::DwmExtendFrame {
                -1
            } else {
                0
            };
            let margins = WinMargins {
                cx_left: extend_by,
                cx_right: extend_by,
                cy_top: extend_by,
                cy_bottom: extend_by,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }

        // 9. Recalculate the frame, but only when a style really changed.
        //
        // DISABLED: shaping the transcript window here made DWM rebuild its
        // frame, and a rebuilt frame on these popups comes back with a caption —
        // the window title became visible above the transcript card, and the
        // same 28 px band sits at the top of the orb window. Since the caption
        // is the artifact, do not ask DWM to rebuild frames at all.
        // (rollback: `if style_changed || ex_changed { SetWindowPos(..., SWP_FRAMECHANGED) }`)
        let _ = (style_changed, ex_changed);

        // phase 3.3: no manual erase here. `RDW_ERASE`/`RDW_FRAME` made DWM paint
        // the extended-frame region (i.e. the whole window) in a light colour,
        // which showed up as pale bars/boxes over the orb and the transcript
        // card. The `SWP_FRAMECHANGED` above already recalculates the frame when a
        // style really changed, and egui repaints the window every frame anyway.
        // (rollback: RedrawWindow(hwnd, std::ptr::null(), 0, RDW_INVALIDATE | RDW_ERASE | RDW_FRAME))
    }
}

/// True when the OS window already has the shaper's target style: a frameless
/// popup with no raised/sunken/dialog edge.
#[cfg(windows)]
fn window_style_is_shaped(style: i32, ex: i32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_BORDER, WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };
    const WS_DLGFRAME: u32 = 0x0080_0000;
    const WS_EX_WINDOWEDGE: u32 = 0x0000_0100;
    const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
    const WS_EX_STATICEDGE: u32 = 0x0002_0000;
    const WS_EX_DLGMODALFRAME: u32 = 0x0000_0001;

    let s = style as u32;
    let e = ex as u32;
    let framed = s
        & (WS_CAPTION.0
            | WS_THICKFRAME.0
            | WS_MINIMIZEBOX.0
            | WS_MAXIMIZEBOX.0
            | WS_SYSMENU.0
            | WS_BORDER.0
            | WS_DLGFRAME)
        != 0;
    let not_popup = s & WS_POPUP.0 == 0;
    let edged =
        e & (WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_STATICEDGE | WS_EX_DLGMODALFRAME) != 0;
    !(framed || not_popup || edged)
}

/// Cheap per-frame guard: keeps a window frameless for as long as it lives.
///
/// Two `GetWindowLongW` reads per call, and the expensive path (styles + DWM
/// attributes + full repaint) only runs when the OS has actually drifted. Drift
/// is real and reproducible: winit re-applies its window attributes on a few
/// paths — minimize/restore is the one the user hit — which puts `WS_CAPTION`
/// and the frame right back, and the orb then shows a normal Windows title bar.
///
/// Returns `true` when a repair happened, so the caller can log it.
#[cfg(windows)]
/// (see the note on `OMNITYPE_DWM_CAPTION` in `apply_dwm_attributes_only`)
#[cfg(windows)]
pub fn enforce_frameless_window(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongW, GWL_EXSTYLE, GWL_STYLE};
    if hwnd == 0 {
        return false;
    }
    let win_hwnd = HWND(hwnd as *mut std::ffi::c_void);
    if *transparency_mode() == TransparencyMode::NoStyleRewrite {
        // This mode never writes styles, so "the style drifted" is not an error
        // to repair: repairing it is exactly what we are testing against.
        return false;
    }
    let style = unsafe { GetWindowLongW(win_hwnd, GWL_STYLE) };
    let ex = unsafe { GetWindowLongW(win_hwnd, GWL_EXSTYLE) };
    if window_style_is_shaped(style, ex) {
        return false;
    }
    enable_true_transparency(hwnd);
    true
}

/// True screen dimensions in physical pixels. Used to center the dashboard
/// viewport: inside a child viewport, `ctx.screen_rect()` returns the *parent*
/// viewport's rect (the tiny capsule), not the monitor, so positioning from it
/// pins the dashboard to the capsule's corner.
#[cfg(windows)]
pub fn true_screen_size_px() -> Option<(i32, i32)> {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { Some((GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))) }
}

#[cfg(not(windows))]
pub fn true_screen_size_px() -> Option<(i32, i32)> {
    None
}

/// A secondary window's top-left corner, in logical screen points.
///
/// Centred horizontally; vertically either centred or hung `bottom_margin`
/// above the bottom edge. Never negative: a screen smaller than the window
/// pins it to the corner rather than throwing it off the desktop.
///
/// **The screen, not `ctx.screen_rect()`.** That rect is the calling window's
/// own, with its origin at `(0,0)` — `egui-winit-0.28.1/src/lib.rs:38-41` and
/// `:239-241` set it from `window.inner_size()` — and the calling window here
/// is the small capsule. Deriving a position from it put the review and the
/// consent windows at the top-left of the desktop, partly off-screen, wherever
/// the orb happened to be. The dashboard and the transcript bubble were already
/// fixed with `true_screen_size_px()`; this is that answer, in one place, for
/// every window that needs it (found by Q1-1).
pub fn position_in_screen(
    screen_pt: (f32, f32),
    win_pt: (f32, f32),
    bottom_margin: Option<f32>,
) -> (f32, f32) {
    let x = ((screen_pt.0 - win_pt.0) / 2.0).round().max(0.0);
    let y = match bottom_margin {
        Some(margin) => (screen_pt.1 - win_pt.1 - margin).round().max(0.0),
        None => ((screen_pt.1 - win_pt.1) / 2.0).round().max(0.0),
    };
    (x, y)
}

// ── monitor enumeration ─────────────────────────────────────────────────────
// One query, three shapes, so the idle policy never has to know about HMONITOR.
//
// `MonitorFromWindow`-style lookup is not enough on its own: the policy needs to
// tell "the orb is on the second monitor" from "the second monitor was unplugged",
// and only an enumeration with a stable index can do that. The index is the
// enumeration order, which Windows does not guarantee across reboots — so it is
// used as a *within-session* handle and never written to `config.toml`.

/// One monitor's usable desktop, plus how to recognise it.
///
/// Named rather than a tuple because it travels through three signatures and
/// `(i32, i32, i32, i32), u8, bool` reads as three unrelated things rather than
/// one monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorWorkArea {
    /// Usable rectangle, physical pixels: `left, top, right, bottom`.
    pub rect: (i32, i32, i32, i32),
    /// Position in this session's enumeration order. **Not** stable across
    /// reboots, so it is never written to `config.toml`.
    pub index: u8,
    pub is_primary: bool,
}

/// `MONITORINFOF_PRIMARY` from the Win32 headers.
///
/// Spelled out here because the `windows` 0.58 bindings do not export it as a
/// named constant, and an unnamed magic number in a bit test is exactly the kind
/// of thing that survives a dependency bump as a wrong answer.
#[cfg(windows)]
const MONITORINFOF_PRIMARY: u32 = 0x0000_0001;

/// Work area of the monitor containing `(x, y)`, plus its index and whether it
/// is the primary. Physical pixels.
#[cfg(windows)]
pub fn monitor_work_area(x: i32, y: i32) -> Option<MonitorWorkArea> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let pt = POINT { x, y };
    let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    if mon.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(mon, &mut info) }.as_bool() {
        return None;
    }
    let rc = info.rcWork;
    if rc.right <= rc.left || rc.bottom <= rc.top {
        return None;
    }
    let rect = (rc.left, rc.top, rc.right, rc.bottom);
    let all = all_monitor_work_areas();
    // Matching on the rectangle rather than on the handle: the handle cannot be
    // compared with a usable `PartialEq`, and two monitors cannot share a work
    // rect, so the match is unambiguous.
    let index = all
        .iter()
        .find(|m| m.rect == rect)
        .map(|m| m.index)
        .unwrap_or(0);
    Some(MonitorWorkArea {
        rect,
        index,
        is_primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

#[cfg(not(windows))]
pub fn monitor_work_area(_x: i32, _y: i32) -> Option<MonitorWorkArea> {
    None
}

/// Work area of the primary monitor, if there is one.
#[cfg(windows)]
pub fn primary_monitor_work_area() -> Option<MonitorWorkArea> {
    all_monitor_work_areas().into_iter().find(|m| m.is_primary)
}

#[cfg(not(windows))]
pub fn primary_monitor_work_area() -> Option<MonitorWorkArea> {
    None
}

/// Every monitor's work area, in enumeration order.
#[cfg(windows)]
pub fn all_monitor_work_areas() -> Vec<MonitorWorkArea> {
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    // The callback cannot borrow, so the rectangles land in a raw pointer the
    // caller owns. `EnumDisplayMonitors` is synchronous and returns only after
    // the callback has finished for every monitor, so nothing can touch the
    // vector afterwards.
    unsafe extern "system" fn collect(mon: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
        let out = &mut *(data.0 as *mut Vec<MonitorWorkArea>);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut info).as_bool() {
            let rc: RECT = info.rcWork;
            if rc.right > rc.left && rc.bottom > rc.top {
                out.push(MonitorWorkArea {
                    rect: (rc.left, rc.top, rc.right, rc.bottom),
                    index: 0,
                    is_primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
                });
            }
        }
        BOOL(1)
    }

    let mut out: Vec<MonitorWorkArea> = Vec::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut out as *mut _ as isize),
        )
    };
    // A failed enumeration means "no answer", not "no monitors" — but the two
    // lead to the same place for the caller, and returning the partial list
    // would be the dangerous one: a monitor that was not enumerated would look
    // like a monitor that has been unplugged.
    if !ok.as_bool() {
        return Vec::new();
    }
    for (i, entry) in out.iter_mut().enumerate() {
        entry.index = i as u8;
    }
    out
}

#[cfg(not(windows))]
pub fn all_monitor_work_areas() -> Vec<MonitorWorkArea> {
    Vec::new()
}

/// How many monitors are attached right now. Part of the idle policy's
/// geometry stamp: unplugging a display must invalidate an in-flight request.
pub fn monitor_count() -> usize {
    all_monitor_work_areas().len()
}

#[cfg(windows)]
pub static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Bumped every time a new transcript bubble appears.
///
/// phase 3.2: no longer read — the preview window is now a single window for the
/// whole process and is corrected on a timer ([`ensure_preview_window_shaped`])
/// rather than once per generation. Kept for the rollback path documented there.
#[cfg(windows)]
#[allow(dead_code)]
pub static PREVIEW_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// HWND of the transcript card window the last time this pass found it, and
/// whether this pass's thread walk found it at all.
///
/// The window now lives for the process lifetime, so this separates "winit
/// (re)created the window, correct it immediately" from "same window, it is
/// already correct" — see [`ensure_preview_window_shaped`].
#[cfg(windows)]
static PREVIEW_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
#[cfg(windows)]
static PREVIEW_FOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Non-Windows: there is no HWND to correct.
#[cfg(not(windows))]
pub fn ensure_preview_window_shaped() {}

/// Erases and repaints a window right now.
///
/// Called after moving a transparent window: without it the pixels of the old
/// position stay on screen and successive moves pile up as the "nested window
/// frames" the user saw.
#[cfg(windows)]
pub fn force_repaint(hwnd: isize) {
    if hwnd != 0 {
        let _ = unsafe { RedrawWindow(hwnd, std::ptr::null(), 0, RDW_FORCE_REPAINT) };
    }
}

#[cfg(windows)]
pub fn position_above_taskbar(hwnd: isize, width_px: i32, height_px: i32, _corner_px: i32) {
    use windows::Win32::Foundation::RECT;

    let mut work_area = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(0x0030 /* SPI_GETWORKAREA */, 0, &mut work_area, 0)
    };
    if ok != 0 {
        let center_x = (work_area.left + work_area.right) / 2;
        let taskbar_top = work_area.bottom;

        let left = center_x - width_px / 2;
        let top = taskbar_top - height_px - 8; // 8 physical pixels above the taskbar

        unsafe {
            // SWP_NOACTIVATE = 0x0010, SWP_SHOWWINDOW = 0x0040
            SetWindowPos(
                hwnd,
                -1, /* HWND_TOPMOST */
                left,
                top,
                width_px,
                height_px,
                0x0010 | 0x0040,
            );
        }
        enable_true_transparency(hwnd);
    }
}

#[cfg(windows)]
pub fn taskbar_bottom_center_pt(win_w_pt: f32, win_h_pt: f32, ppp: f32) -> (f32, f32) {
    use windows::Win32::Foundation::RECT;

    let mut work_area = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(0x0030 /* SPI_GETWORKAREA */, 0, &mut work_area, 0)
    };
    if ok != 0 {
        let center_x = (work_area.left + work_area.right) / 2;
        let taskbar_top = work_area.bottom;

        let px_w = (win_w_pt * ppp).round() as i32;
        let px_h = (win_h_pt * ppp).round() as i32;

        let left = center_x - px_w / 2;
        let top = taskbar_top - px_h - 16; // 16 physical pixels above taskbar
        (left as f32 / ppp, top as f32 / ppp)
    } else {
        let (sw, sh) = true_screen_size_px().unwrap_or((1920, 1080));
        let sw_pt = sw as f32 / ppp;
        let sh_pt = sh as f32 / ppp;
        ((sw_pt - win_w_pt) * 0.5, sh_pt - win_h_pt - 48.0)
    }
}

#[cfg(not(windows))]
pub fn taskbar_bottom_center_pt(win_w_pt: f32, win_h_pt: f32, _ppp: f32) -> (f32, f32) {
    (500.0, 800.0)
}

#[cfg(windows)]
pub fn local_time_str() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}

#[cfg(not(windows))]
pub fn local_time_str() -> String {
    "00:00:00".to_string()
}

#[cfg(windows)]
/// LEGACY (phase 1) — kept in the tree, uncalled, as a rollback path.
///
/// It resolved the app window by *title* across every window of the UI thread.
/// winit's internal "Winit Thread Event Target" window and the tray-icon
/// message window are *also* title-less, so they were treated as the app window:
/// `MAIN_HWND` was overwritten with whichever window matched last (enumeration
/// order is z-order, so "last" is effectively arbitrary) and `OrbWindow::place()`
/// then moved/resized *that* window onto the orb on every frame, leaving the
/// frozen "ghost" rectangles that accumulated with each dictation.
///
/// Evidence and the verification probe: `docs/GUI-BUGFIX-PLAN.md` §1-1 and
/// `docs/reaserch/gui/probes/window-probe.ps1`.
#[allow(dead_code)]
fn apply_window_shapes_all_legacy() {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; (len + 1) as usize];
        let actual = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..actual as usize]);

        if title.is_empty() || title == "OmniType" {
            MAIN_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
            enable_true_transparency(hwnd.0 as isize);
        } else if title.contains("OmniType_Preview") {
            enable_true_transparency(hwnd.0 as isize);
        }
        BOOL(1)
    }

    unsafe {
        let thread_id = GetCurrentThreadId();
        let _ = EnumThreadWindows(thread_id, Some(enum_proc), LPARAM(0));
    }
}

/// Resolves and registers the **real** main window handle exactly once, straight
/// from eframe's own raw platform window handle ([`eframe::Frame`] implements
/// `raw_window_handle::HasWindowHandle`), then applies the alpha shaping to it.
///
/// Returns `true` when a handle was registered on this call.
///
/// This replaces title-based picking entirely: no thread-wide enumeration, no
/// window ever has its title cleared or its styles stripped unless it is the
/// window eframe hands us for the main viewport.
#[cfg(windows)]
pub fn register_main_hwnd(frame: &eframe::Frame) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = frame.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd: isize = win32.hwnd.get();
    if hwnd == 0 {
        return false;
    }
    // phase 3.2: compare instead of "only when empty". winit can recreate the OS
    // window (minimize/restore, DPI change); the cached handle then pointed at a
    // dead window, the *new* one was never shaped, and it came up as a normal
    // captioned window — one more "frame" on screen every time.
    let previous = MAIN_HWND.swap(hwnd, std::sync::atomic::Ordering::Relaxed);
    if previous == hwnd {
        return false;
    }
    enable_true_transparency(hwnd);
    log_window_geometry(hwnd);
    if previous == 0 {
        tracing::info!(
            hwnd,
            "main window handle registered from eframe's raw window handle"
        );
    } else {
        tracing::warn!(
            hwnd,
            previous,
            "main window handle changed; re-shaped the new OS window"
        );
    }
    true
}

/// Drops the cached main-window handle so [`register_main_hwnd`] resolves it
/// again. Called when a `SetWindowPos` on it fails, i.e. the OS window is gone
/// (it can be recreated when the main viewport is reopened).
#[cfg(windows)]
pub fn invalidate_main_hwnd() {
    MAIN_HWND.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Finds the transcript card's OS window and gives it the same Win32
/// correction as the main window.
///
/// # Why the handle has to be hunted for
///
/// egui 0.28 gives no way to reach a child viewport's `HWND`: `RawInput` and
/// [`eframe::Frame`] expose the **root** handle only, and `ViewportCommand` has
/// no getter. So the handle is found by the one thing that is unique to this
/// window and to nothing else in the process — its own title,
/// [`crate::gui::preview_window::PREVIEW_WINDOW_TITLE`]. That is strictly safer
/// than the old `apply_window_shapes_all_legacy`, which adopted *any* title-less
/// window on the UI thread and thereby captured winit's own internal windows.
///
/// # Why this runs every frame and is still cheap
///
/// `EnumThreadWindows` over the UI thread sees a handful of windows, the title
/// compare rejects all but one, and [`apply_viewport_transparency`] is four
/// idempotent DWM calls. The full treatment re-runs at most every
/// [`PREVIEW_RESHAPE_INTERVAL`] frames, so even if something re-enables the
/// backdrop the window cannot stay wrong for more than a fraction of a second.
///
/// (rollback: drop the call in `report_window`. The previous per-bubble
/// "shape once on creation" behaviour is the `PREVIEW_HWND.swap` branch below,
/// which is kept exactly as it was.)
#[cfg(windows)]
pub fn ensure_preview_window_shaped() {
    use std::sync::atomic::Ordering::Relaxed;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetWindowTextLengthW, GetWindowTextW, IsWindow,
    };

    static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        use std::sync::atomic::Ordering::Relaxed;
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; (len + 1) as usize];
        let actual = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..actual as usize]);
        if !title.contains(crate::gui::preview_window::PREVIEW_WINDOW_TITLE) {
            return BOOL(1);
        }
        PREVIEW_FOUND.store(true, Relaxed);
        let raw = hwnd.0 as isize;

        // A brand new OS window must be corrected the moment it appears, and
        // then corrected again now and then. winit's `on_create` is what turns
        // the backdrop on, and it runs on a different code path from ours, so a
        // single one-shot pass is a race rather than a guarantee.
        let is_new = PREVIEW_HWND.swap(raw, Relaxed) != raw;
        let due = FRAMES
            .load(Relaxed)
            .is_multiple_of(PREVIEW_RESHAPE_INTERVAL);
        if is_new || due {
            apply_viewport_transparency(raw);
            FRAMES.store(0, Relaxed);
            if is_new {
                tracing::debug!(hwnd = raw, "preview window transparency applied");
            }
        }
        BOOL(1)
    }

    unsafe {
        FRAMES.fetch_add(1, Relaxed);
        PREVIEW_FOUND.store(false, Relaxed);
        let thread_id = GetCurrentThreadId();
        let _ = EnumThreadWindows(thread_id, Some(enum_proc), LPARAM(0));
        // The window is gone (it now lives for the process lifetime, but winit
        // can still recreate it): forget the handle so the next one is treated
        // as new even if Windows recycles the same HWND value.
        if PREVIEW_FOUND.load(Relaxed) {
            // Keep the cached handle only while it is still a live window.
            if IsWindow(HWND(PREVIEW_HWND.load(Relaxed) as *mut std::ffi::c_void)).as_bool() {
                return;
            }
        }
        PREVIEW_HWND.store(0, Relaxed);
    }
}

/// How many frames between two unconditional [`apply_viewport_transparency`]
/// passes over the preview window. At the card's own 60 fps request this is
/// about four times a second.
#[cfg(windows)]
const PREVIEW_RESHAPE_INTERVAL: u32 = 15;

#[cfg(all(test, windows))]
mod tests {
    use super::{
        click_region_px, expected_region_box, invalidate_click_region, position_in_screen,
        region_is_current, transparency_mode_from, ClickRegion, TransparencyMode,
    };
    use super::super::orb::{self, Orb};
    use super::super::orb_animation::OrbMode;

    /// Q1-1, pinned as arithmetic: every window this places has to end up
    /// **inside** the screen it was given, for both anchors and for screens
    /// far larger than the window. The defect this replaces produced negative
    /// coordinates — a window whose top-left is off the desktop is a window
    /// the user never sees.
    #[test]
    fn a_placed_window_stays_inside_the_screen_it_was_given() {
        for screen in [(1920.0, 1080.0), (1280.0, 720.0), (2560.0, 1440.0), (3840.0, 2160.0)] {
            for win in [(460.0, 300.0), (440.0, 260.0), (720.0, 640.0)] {
                for bottom in [Some(48.0), None] {
                    let (x, y) = position_in_screen(screen, win, bottom);
                    assert!(x >= 0.0 && x + win.0 <= screen.0, "{screen:?} {win:?} {bottom:?} → x={x}");
                    assert!(y >= 0.0 && y + win.1 <= screen.1, "{screen:?} {win:?} {bottom:?} → y={y}");
                    if bottom.is_some() {
                        assert!(
                            (screen.1 - (y + win.1) - 48.0).abs() < 1.0,
                            "the bottom-anchored window keeps its margin"
                        );
                    }
                }
            }
        }
    }

    /// A screen smaller than the window pins it to the corner instead of
    /// sending it to negative coordinates.
    #[test]
    fn a_screen_smaller_than_the_window_does_not_go_negative() {
        assert_eq!(position_in_screen((300.0, 200.0), (460.0, 300.0), None), (0.0, 0.0));
        assert_eq!(position_in_screen((300.0, 200.0), (460.0, 300.0), Some(48.0)), (0.0, 0.0));
    }

    /// The transparency default is the one thing here a user can only verify by
    /// looking at the screen, so pin the mapping: an unset/unknown env var must
    /// never silently fall back to a mode that rewrites the window style, because
    /// that is what paints the pale 28 px band over the orb
    /// (docs/GUI-WINDOW-ARTIFACT-REPORT.md §13).
    #[test]
    fn transparency_defaults_to_no_style_rewrite() {
        assert_eq!(transparency_mode_from(""), TransparencyMode::NoStyleRewrite);
        assert_eq!(
            transparency_mode_from("   "),
            TransparencyMode::NoStyleRewrite
        );
        assert_eq!(
            transparency_mode_from("nonsense"),
            TransparencyMode::NoStyleRewrite
        );
        assert_eq!(
            transparency_mode_from("NOSTYLE"),
            TransparencyMode::NoStyleRewrite
        );
    }

    #[test]
    fn swapchain_mode_is_still_reachable() {
        assert_eq!(
            transparency_mode_from("swapchain"),
            TransparencyMode::SwapchainAlpha
        );
        assert_eq!(
            transparency_mode_from("Swapchain"),
            TransparencyMode::SwapchainAlpha
        );
    }

    #[test]
    fn transparency_rollback_paths_remain_reachable() {
        assert_eq!(
            transparency_mode_from("dwm-extend"),
            TransparencyMode::DwmExtendFrame
        );
        assert_eq!(
            transparency_mode_from(" Dwm_Extend "),
            TransparencyMode::DwmExtendFrame
        );
        assert_eq!(
            transparency_mode_from("dwm-extend-0"),
            TransparencyMode::DwmExtendNone
        );
        // The two legacy paths must stay distinguishable, otherwise the
        // single-variable A/B in the report would prove nothing.
        assert_ne!(
            TransparencyMode::DwmExtendFrame,
            TransparencyMode::DwmExtendNone
        );
    }
    // ── click-through ───────────────────────────────────────────────────
    //
    // The transparent part of the orb window must not take clicks from whatever
    // is underneath. That is a property of the *region*, not of the window: a
    // smaller window would achieve it by accident, and a bigger one would lose
    // it. The tests below hold the window size fixed and move the region, so
    // they describe the mechanism rather than one number.

    /// The region the orb asks for is a circle of exactly its painted reach —
    /// never the whole rect, never a different size.
    ///
    /// That single number is the whole of the click-through guarantee:
    /// `WindowFromPoint` only offers a point to a window whose region contains
    /// it, so every pixel past the circle reaches the application below. The
    /// region is not allowed to grow either, or the orb would steal clicks from
    /// the desktop over pixels nothing was ever painted on.
    ///
    /// The window is swept rather than fixed, because the guarantee has to be a
    /// property of the *mechanism*: a smaller window would satisfy it by
    /// accident and a bigger one would lose it. Every window here is at least
    /// `Orb::max_canvas_points()`, which is the only size the orb ever uses.
    #[test]
    fn the_orb_region_is_a_circle_whatever_the_window_is() {
        let reach_pt = super::super::orb::interaction_radius_pt_for_test(1.0, true);
        let canvas = super::super::orb::Orb::max_canvas_points();
        assert!(
            canvas >= reach_pt * 2.0 - 0.001,
            "the orb's own canvas ({canvas} pt) is smaller than the reach it has to hold"
        );
        for side_pt in [canvas, canvas + 1.0, 300.0, 400.0, 900.0, 2_000.0] {
            for ppp in [1.0f32, 1.25, 1.5, 2.0] {
                let got = click_region_px(
                    ClickRegion::Circle {
                        radius_pt: reach_pt,
                    },
                    [side_pt, side_pt],
                    ppp,
                )
                .expect("a circle inside a square window is always usable");
                match got {
                    ClickRegion::Circle { radius_pt } => {
                        // Rounding to whole pixels is the only slack allowed.
                        assert!(
                            (radius_pt - reach_pt).abs() <= 0.5 / ppp,
                            "side {side_pt} @ {ppp}x: region {radius_pt} pt !=                              painted reach {reach_pt} pt"
                        );
                    }
                    other => panic!("side {side_pt} @ {ppp}x: region became {other:?}"),
                }
            }
        }
    }

    /// A window smaller than the orb cannot happen, but a window *larger* than
    /// the region must not have the region grown to fill it: the clamp in
    /// `click_region_px` is what stops a future oversized canvas from quietly
    /// taking the clicks back.
    #[test]
    fn a_generous_window_does_not_grow_the_region() {
        let reach_pt = super::super::orb::interaction_radius_pt_for_test(1.0, true);
        let small = click_region_px(
            ClickRegion::Circle {
                radius_pt: reach_pt,
            },
            [240.0, 240.0],
            1.0,
        );
        let huge = click_region_px(
            ClickRegion::Circle {
                radius_pt: reach_pt,
            },
            [900.0, 900.0],
            1.0,
        );
        assert_eq!(small, huge, "the region must not depend on the window size");
    }

    /// The region may never be degenerate. `SetWindowRgn` fails on an empty
    /// region and, given one that degenerates at runtime, leaves a window nobody
    /// can see and everybody can click — so these are the inputs that must fall
    /// back to `Full` rather than produce a zero-size circle.
    #[test]
    fn an_unusable_window_or_scale_falls_back_to_the_full_rect() {
        for window_pt in [[0.0f32, 0.0], [-10.0, 240.0], [f32::NAN, 240.0]] {
            for ppp in [0.0f32, -1.0, f32::NAN] {
                assert_eq!(
                    click_region_px(ClickRegion::Circle { radius_pt: 50.0 }, window_pt, ppp),
                    Some(ClickRegion::Full),
                    "window {window_pt:?} @ {ppp}x"
                );
            }
        }
    }

    /// The cache is a hint; the window is the authority.
    ///
    /// B0 §6-3: `invalidate_click_region` had no production caller, because the
    /// one event that invalidates the cache — Windows rebuilding the OS window
    /// behind the same `HWND` value — is invisible to a handle comparison by
    /// definition. So the decision now reads the window's own region back, and
    /// these are the four answers it has to give.
    #[test]
    fn the_region_is_reapplied_whenever_the_window_disagrees() {
        let hwnd = 0x1234;
        let region = ClickRegion::Circle { radius_pt: 60.0 };
        let side_px = [300, 300];
        let ppp = 1.25;
        let expected = expected_region_box(region, side_px, ppp);
        assert!(expected.is_some(), "a circle has a bounding box");

        // Cache and window agree: nothing to do.
        assert!(region_is_current(
            Some((hwnd, region)),
            hwnd,
            region,
            side_px,
            ppp,
            expected
        ));
        // Cache says yes, window says it is carrying nothing: this is the
        // rebuilt-window case, and it has to reach `SetWindowRgn`.
        assert!(!region_is_current(
            Some((hwnd, region)),
            hwnd,
            region,
            side_px,
            ppp,
            None
        ));
        // Cache says yes, window is carrying a different box: also reapply.
        assert!(!region_is_current(
            Some((hwnd, region)),
            hwnd,
            region,
            side_px,
            ppp,
            Some([0, 0, 1, 1])
        ));
        // Nothing cached at all, even with the window already correct: the first
        // frame after startup has to establish the cache.
        assert!(!region_is_current(
            None, hwnd, region, side_px, ppp, expected
        ));
        // A different handle is never "current", whatever the window reports.
        assert!(!region_is_current(
            Some((0x999, region)),
            hwnd,
            region,
            side_px,
            ppp,
            expected
        ));
    }

    /// The box the read-back is compared against must be the box GDI was
    /// actually given, or the comparison above would flag a correct window as
    /// wrong forever. The `+ 1` on right/bottom is GDI's exclusive bound.
    #[test]
    fn the_expected_box_is_the_coordinates_gdi_receives() {
        // 60 pt at 125% = 75 px, centred in a 300 px window.
        let box_px = expected_region_box(ClickRegion::Circle { radius_pt: 60.0 }, [300, 300], 1.25);
        assert_eq!(box_px, Some([75, 75, 226, 226]));
        // The card's rounded rect is compared against its own bounds.
        assert_eq!(
            expected_region_box(
                ClickRegion::RoundedRect {
                    rect_pt: [10.0, 20.0, 110.0, 60.0],
                    radius_pt: 8.0
                },
                [200, 200],
                2.0,
            ),
            Some([20, 40, 220, 120])
        );
        // `Full` means "no region at all", which is exactly what the read-back
        // reports for a window that has none.
        assert_eq!(
            expected_region_box(ClickRegion::Full, [200, 200], 1.0),
            None
        );
    }

    /// `invalidate_click_region` now has no production caller on purpose (see
    /// its doc), so this is where it has to keep working — and where the cache
    /// it clears is proved to be the thing being consulted.
    #[test]
    fn the_region_cache_can_be_forced_back_to_empty() {
        let hwnd = 0x1234;
        let region = ClickRegion::Circle { radius_pt: 60.0 };
        let side_px = [300, 300];
        let ppp = 1.25;
        let box_px = expected_region_box(region, side_px, ppp);
        super::cache_click_region(hwnd, region);
        assert!(region_is_current(
            super::cached_click_region(),
            hwnd,
            region,
            side_px,
            ppp,
            box_px
        ));
        invalidate_click_region();
        assert!(
            !region_is_current(
                super::cached_click_region(),
                hwnd,
                region,
                side_px,
                ppp,
                box_px
            ),
            "invalidating must make the cache miss even though the window agrees"
        );
        invalidate_click_region();
    }

    /// A circle is clipped to the window when it does not fit, because
    /// `SetWindowRgn` intersects the region with the window rect anyway. The orb
    /// never hits this — its canvas is derived from the same number — but a
    /// clamped region must still be a sane circle rather than a zero one.
    #[test]
    fn an_oversized_circle_is_clipped_not_rejected() {
        let got = click_region_px(
            ClickRegion::Circle {
                radius_pt: 10_000.0,
            },
            [200.0, 200.0],
            1.0,
        )
        .expect("clipping is not rejection");
        match got {
            ClickRegion::Circle { radius_pt } => {
                assert!((radius_pt - 100.0).abs() < 0.001, "{radius_pt}");
            }
            other => panic!("{other:?}"),
        }
    }
    // ── the application-profile bar ─────────────────────────────────────
    //
    // While a dictation runs under a named profile the orb paints a bar below
    // itself naming that profile, and the click region has to grow to cover it.
    // Getting that wrong is visible in both directions: a region that misses the
    // bar clips the very label the user is meant to read, and a region wider than
    // the bar is a strip of nothing that swallows clicks meant for the desktop —
    // the defect the circle itself exists to remove.

    /// The shape the orb asks for while a profile is in force, in points.
    ///
    /// Assembled from the orb's own accessors rather than re-derived here,
    /// because a second copy of that geometry would only be asserting against
    /// itself: what these tests are about is the *region*, not the bar.
    fn orb_profile_region(canvas_pt: f32, user_scale: f32) -> ClickRegion {
        let width_pt = orb::pill_max_width_pt_for_test(canvas_pt, user_scale);
        let bar = orb::profile_pill_rect_for_test(canvas_pt, width_pt, user_scale);
        ClickRegion::CircleWithPill {
            radius_pt: orb::interaction_radius_pt_for_test(
                OrbMode::Recording.target_scale() * user_scale,
                false,
            ),
            pill_pt: [bar.min.x, bar.min.y, bar.max.x, bar.max.y],
            pill_radius_pt: orb::pill_corner_pt_for_test(),
        }
    }

    /// A region's bar as window-relative pixels, so the assertions below read in
    /// the same units the region is applied in.
    fn bar_px(region: ClickRegion, ppp: f32) -> [f32; 4] {
        match region {
            ClickRegion::CircleWithPill { pill_pt, .. } => [
                pill_pt[0] * ppp,
                pill_pt[1] * ppp,
                pill_pt[2] * ppp,
                pill_pt[3] * ppp,
            ],
            other => panic!("expected a bar, got {other:?}"),
        }
    }

    /// The region is the orb's circle **plus** the bar, and it grows in exactly
    /// one direction: down. Sideways and upwards the bar lives inside the orb's
    /// own reach, so the region must not widen or rise there — those pixels are
    /// transparent and belong to the desktop underneath.
    #[test]
    fn the_profile_bar_extends_the_region_below_the_orb() {
        let canvas = Orb::max_canvas_points();
        let ppp = 1.0;
        let side_px = [(canvas * ppp).round() as i32; 2];

        let px = click_region_px(orb_profile_region(canvas, 1.0), [canvas, canvas], ppp)
            .expect("a bar inside the orb's own canvas is always usable");
        let ClickRegion::CircleWithPill { radius_pt, .. } = px else {
            panic!("a profile region must not collapse to {px:?}");
        };
        let union = expected_region_box(px, side_px, ppp).expect("the union has a box");
        let circle = expected_region_box(ClickRegion::Circle { radius_pt }, side_px, ppp)
            .expect("the circle has a box");

        assert!(
            union[3] > circle[3],
            "the bar must reach below the orb: {union:?} vs {circle:?}"
        );
        assert_eq!(union[0], circle[0], "the bar must not widen the region");
        assert_eq!(union[1], circle[1], "the bar must not raise the region");
        assert_eq!(union[2], circle[2], "the bar must not widen the region");

        // And every pixel of the bar is inside what the region claims, so no row
        // or column of the label can be cut away by the window's shape.
        let bar = bar_px(px, ppp);
        for (edge, value) in [
            ("left", bar[0] >= union[0] as f32),
            ("top", bar[1] >= union[1] as f32),
            ("right", bar[2] <= union[2] as f32),
            ("bottom", bar[3] <= union[3] as f32),
        ] {
            assert!(value, "the bar's {edge} ({bar:?}) is outside the region {union:?}");
        }
    }

    /// The box handed to the read-back check has to be the union of the two
    /// shapes' boxes, element by element. It is computed by one piece of code and
    /// compared against what GDI built from two calls, so this is the comparison
    /// that keeps those from drifting: a box too small would have the region
    /// reapplied every frame, and one too large would hide a window that left the
    /// bar unclipped when it should not have.
    #[test]
    fn the_expected_box_is_the_union_of_the_circle_and_the_bar() {
        let side_px = [300, 300];
        let ppp = 1.25;
        let circle = expected_region_box(ClickRegion::Circle { radius_pt: 60.0 }, side_px, ppp)
            .expect("a circle has a box");
        let bar = expected_region_box(
            ClickRegion::RoundedRect {
                rect_pt: [90.0, 96.0, 210.0, 240.0],
                radius_pt: 8.5,
            },
            side_px,
            ppp,
        )
        .expect("a rounded rect has a box");
        let union = expected_region_box(
            ClickRegion::CircleWithPill {
                radius_pt: 60.0,
                pill_pt: [90.0, 96.0, 210.0, 240.0],
                pill_radius_pt: 8.5,
            },
            side_px,
            ppp,
        )
        .expect("the union has a box");

        assert_eq!(
            union,
            [
                circle[0].min(bar[0]),
                circle[1].min(bar[1]),
                circle[2].max(bar[2]),
                circle[3].max(bar[3]),
            ],
            "the union must be the two boxes' union: circle {circle:?}, bar {bar:?}"
        );
        // The bar's far side is the one that has to widen the box, otherwise this
        // test would pass against a box that ignored the bar entirely.
        assert!(bar[2] > circle[2] && bar[3] > circle[3], "{bar:?} vs {circle:?}");
    }

    /// The bar is laid out inside the canvas at every size the user can pick, so
    /// `click_region_px` never has to move it. A bar the clamp had to trim would
    /// be a label with both ends cut off — and the clamp would be hiding layout
    /// that no longer fits.
    #[test]
    fn the_bar_fits_its_window_at_every_size_the_user_can_choose() {
        let canvas = Orb::max_canvas_points();
        for user_scale in [0.6f32, 0.8, 1.0, 1.25, 1.5, 2.0] {
            for ppp in [1.0f32, 1.25, 1.5, 2.0] {
                let asked = orb_profile_region(canvas, user_scale);
                let px = click_region_px(asked, [canvas, canvas], ppp)
                    .expect("the orb's own canvas is always usable");
                assert!(
                    matches!(px, ClickRegion::CircleWithPill { .. }),
                    "{user_scale} @ {ppp}x: the profile region collapsed to {px:?}"
                );
                let want = bar_px(asked, ppp);
                let got = bar_px(px, ppp);
                for i in 0..4 {
                    assert!(
                        (got[i] - want[i]).abs() <= 0.5,
                        "{user_scale} @ {ppp}x: the window moved the bar \
                         ({got:?} against the painted {want:?})"
                    );
                }
                let side = (canvas * ppp).round();
                assert!(
                    got[0] >= 0.0 && got[1] >= 0.0 && got[2] <= side && got[3] <= side,
                    "{user_scale} @ {ppp}x: the bar {got:?} is outside its {side} px window"
                );
            }
        }
    }

    /// A bar with no area is not a reason to lose the orb. The circle that still
    /// describes the window survives it; `Full`, which would hand the whole
    /// transparent square back to the desktop's clicks, is what the fallback must
    /// not become.
    #[test]
    fn a_bar_with_no_area_leaves_the_orb_its_own_circle() {
        let window_pt = [300.0, 300.0];
        let ppp = 1.25;
        let plain = click_region_px(ClickRegion::Circle { radius_pt: 60.0 }, window_pt, ppp);
        // Zero width, zero height, entirely right of the window, and entirely
        // below it: each of these clamps down to nothing.
        for pill in [
            [100.0f32, 150.0, 100.0, 167.0],
            [100.0, 150.0, 200.0, 150.0],
            [400.0, 150.0, 500.0, 167.0],
            [100.0, 400.0, 200.0, 420.0],
        ] {
            let got = click_region_px(
                ClickRegion::CircleWithPill {
                    radius_pt: 60.0,
                    pill_pt: pill,
                    pill_radius_pt: 8.5,
                },
                window_pt,
                ppp,
            );
            assert_eq!(
                got, plain,
                "a bar {pill:?} with no area must leave the orb its own circle"
            );
        }
    }

    /// The bar does not have to be the widest thing in the union for the region
    /// to be honest: the pixels beside the bar's ends — inside the union's own
    /// bounding box, outside both shapes — must stay click-through.
    ///
    /// That is the whole reason the orb asks for a circle OR'd with a bar instead
    /// of one rounded rectangle over the union: such a rectangle's corners would
    /// claim exactly these pixels, and they paint nothing.
    #[test]
    fn the_crescents_beside_the_bar_are_not_part_of_the_region() {
        let canvas = Orb::max_canvas_points();
        let ppp = 1.0;
        let side_px = [(canvas * ppp).round() as i32; 2];
        let px = click_region_px(orb_profile_region(canvas, 1.0), [canvas, canvas], ppp)
            .expect("usable");
        let ClickRegion::CircleWithPill { radius_pt, .. } = px else {
            panic!("{px:?}");
        };
        let union = expected_region_box(px, side_px, ppp).expect("the union has a box");
        let bar = bar_px(px, ppp);

        // A pixel just past the bar's left end and just above its top: inside
        // the union's box, below the circle's lower edge, outside the bar.
        let probe = [bar[0] - 3.0, bar[1] - 4.0];
        assert!(
            probe[0] > union[0] as f32
                && probe[0] < union[2] as f32
                && probe[1] > union[1] as f32
                && probe[1] < union[3] as f32,
            "the probe {probe:?} has to sit inside the union's box to mean anything"
        );
        let (cx, cy) = (side_px[0] as f32 * 0.5, side_px[1] as f32 * 0.5);
        let r = radius_pt * ppp;
        let in_circle = (probe[0] - cx).powi(2) + (probe[1] - cy).powi(2) <= r * r;
        let in_bar = probe[0] >= bar[0] && probe[0] <= bar[2] && probe[1] >= bar[1] && probe[1] <= bar[3];
        assert!(
            !in_circle && !in_bar,
            "the probe {probe:?} is inside the region after all (circle {in_circle}, bar {in_bar})"
        );
        // ...and the bar is the shape the region grew for, so this is not a test
        // that would pass with no bar at all.
        assert!(bar[3] > cy + r, "the bar has to hang below the circle: {bar:?}");
    }
}
