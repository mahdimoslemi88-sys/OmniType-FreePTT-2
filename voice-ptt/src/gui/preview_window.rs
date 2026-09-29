//! Lifecycle owner for the transcript card's OS window.
//!
//! # Status: dormant
//!
//! [`report_window`] is not currently called. [`crate::gui::overlay`]'s
//! `SHOW_TRANSCRIPT_CARD` is `false`, so no viewport is ever reported and this
//! module creates no `HWND` at all — which is the only way to be certain the
//! box cannot come back, on a hotkey, mid-dictation, or after a resume. The
//! code below is kept intact and tested; flipping that one constant restores
//! the card.
//!
//! # What the pale box actually was
//!
//! A light, opaque, always-on-top rectangle that appeared around the card,
//! swallowed clicks meant for the app behind it, jumped left when the
//! transcript grew a line, shrank away with the fade-out, and had corners DWM
//! rounded sometimes and not others.
//!
//! It was not a rendering artifact. A rectangle drawn by `egui` cannot eat a
//! click destined for another process, and it cannot survive a window that has
//! already been torn down. It was an OS window, and the reason it was *light*
//! is a three-hop chain that is worth writing down because two of the hops are
//! counter-intuitive:
//!
//! 1. `ViewportBuilder::with_transparent(true)` — which this module used to set
//!    — becomes `winit::WindowBuilder::with_transparent(true)`
//!    (`egui-winit-0.28.1/src/lib.rs:1595`).
//! 2. In `Window::on_create`, winit turns that into
//!    `DwmEnableBlurBehindWindow(hwnd, { DWM_BB_ENABLE | DWM_BB_BLURREGION,
//!    hRgnBlur: CreateRectRgn(0, 0, -1, -1) })` — a deliberately **empty**
//!    blur region (`winit-0.30.13/src/platform_impl/windows/window.rs:1231-1246`).
//! 3. On Windows 11 build 22621 and later, `DWM_BB_ENABLE` no longer means
//!    "blur this region"; it turns on the **system backdrop** for the window.
//!    An empty region then means "backdrop over everything". This machine runs
//!    build 26200, so the card window — always-on-top, never painted there —
//!    was a light backdrop panel the size of the whole client area. That is
//!    the box, and because it is a window it also intercepts clicks.
//!
//! # Why the flag cannot simply be left on and corrected later
//!
//! It can be, and [`crate::gui::window_shape::ensure_preview_window_shaped`] does
//! exactly that as a second line of defence. But a correction that has to win a
//! race against winit's `on_create`, on every window creation, forever, is a
//! bug waiting for the one path that forgets to correct — which is exactly the
//! state this module was in: the preview window's `HWND` was never passed to
//! any of the Win32 code (only the root window was), so nothing corrected it
//! at all.
//!
//! So the flag is gone. What replaces it is nothing, and that is the point:
//!
//! * **`with_transparent` never gave this window its alpha in the first place.**
//!   The swapchain's `CompositeAlphaMode` comes from `egui_wgpu::winit::Painter`,
//!   which eframe constructs **once**, at startup, from the *root* viewport's
//!   `native_options.viewport.transparent` (`eframe-0.28.1/src/native/
//!   wgpu_integration.rs:196`). `Painter::add_surface` consults that single
//!   field for **every** viewport, child included. The root already asks for
//!   transparency (`lib.rs`), so the card's surface was already
//!   `PreMultiplied`. The flag on the child was pure side effect, no benefit.
//! * The four-line Win32 correction now runs for the card too, because
//!   [`report_window`] calls it.
//!
//! # The window's own lifetime
//!
//! One window, created once, never destroyed: [`report_window`] is called on
//! **every** frame for the lifetime of the process and `with_visible` carries
//! the show/hide state. The earlier design reported the viewport only on frames
//! that had a bubble, which made eframe prune it
//! (`remove_viewports_not_in`) and winit post `WM_DESTROY` asynchronously
//! (`winit-0.30.13/.../windows/window.rs:1107-1116`) — leaving, for a few
//! frames, a window that was still alive, still on top, still hit-testable and
//! no longer being drawn into. That window was a second explanation for the box,
//! and it is gone for good.
//!
//! # The window's size
//!
//! Fixed for the process lifetime (`WINDOW_W` × `WINDOW_MAX_H`). It used to be
//! `card + 2 * shadow_pad`, so a growing transcript resized it and a resized
//! `wgpu` surface exposes fresh, never-presented memory until the next present
//! — which is why the box used to move to the left as the text grew.
//!
//! # The window's hit-testing
//!
//! A fixed window is a fixed rectangle of desktop that is always on top, so
//! even a perfectly transparent one would keep swallowing clicks across
//! `WINDOW_W` × `WINDOW_MAX_H` — much larger than the card. `with_mouse_passthrough`
//! sets `WS_EX_TRANSPARENT` on it, so the desktop underneath stays usable and
//! the cost is that click-to-dismiss on the card stops working. See
//! [`CARD_CLICKS_PASS_THROUGH`] if that trade is ever worth revisiting; region
//! based pass-through is not available in egui 0.28 (it is all-or-nothing,
//! `egui-winit-0.28.1/src/lib.rs:1731`).

use eframe::egui;
use egui::{Context, Ui, ViewportBuilder, ViewportId};

use crate::gui::window_shape;

/// Stable id: the same window is reused for every bubble in the process.
pub fn preview_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("preview_toast_viewport")
}

/// Window title, used by [`window_shape::ensure_preview_window_shaped`] and by
/// the window probes to find the card on screen.
pub const PREVIEW_WINDOW_TITLE: &str = "OmniType_Preview";

/// Padding around the card for its drop shadow, in points.
pub const SHADOW_PAD: f32 = 16.0;

/// The card's own width range, in points.
pub const CARD_MIN_W: f32 = 240.0;
pub const CARD_MAX_W: f32 = 420.0;

/// Fixed window geometry, in points.
///
/// Width is the widest the card can ever be plus its shadow padding; height
/// leaves room for a tall multi-line transcript. Neither depends on the current
/// text, which is the point: see the module docs.
const WINDOW_W: f32 = CARD_MAX_W + SHADOW_PAD * 2.0;
const WINDOW_MAX_H: f32 = 260.0;

/// Corner radius of the card, used both for painting and for the click region,
/// so the area that swallows clicks is exactly the area that is drawn.
pub const CARD_RADIUS: f32 = 16.0;

/// Geometry of the card inside the fixed-size window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardLayout {
    /// Width of the card itself (without shadow padding).
    pub card_w: f32,
    /// Height of the card itself.
    pub card_h: f32,
    /// Size of the OS window, constant for the process lifetime.
    pub window_size: [f32; 2],
    /// Top-left of the card inside the window.
    pub card_origin: [f32; 2],
}

impl CardLayout {
    /// Places a card of the given size, centred, inside the fixed window.
    ///
    /// A card taller than the window is clamped rather than allowed to overflow:
    /// overflow would mean the text is drawn outside the window, which on a
    /// transparent window looks like the text floating over the desktop.
    fn new(card_w: f32, card_h: f32) -> Self {
        let card_w = card_w.clamp(CARD_MIN_W, WINDOW_W - SHADOW_PAD * 2.0);
        // Both bounds are compile-time constants with `min < max`, so `clamp`
        // cannot panic; `tests::card_never_overflows_the_window` covers the
        // degenerate inputs.
        let card_h = card_h.clamp(1.0, WINDOW_MAX_H - SHADOW_PAD * 2.0);
        Self {
            card_w,
            card_h,
            window_size: [WINDOW_W, WINDOW_MAX_H],
            card_origin: [
                ((WINDOW_W - card_w) * 0.5).max(SHADOW_PAD),
                ((WINDOW_MAX_H - card_h) * 0.5).max(SHADOW_PAD),
            ],
        }
    }

    /// The card's rectangle in viewport-local points.
    pub fn card_rect(self) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(self.card_origin[0], self.card_origin[1]),
            egui::vec2(self.card_w, self.card_h),
        )
    }
}

/// Where a galley will actually land on screen when it is painted at `anchor`.
///
/// `epaint` anchors a `RIGHT`-aligned galley at its right edge, so the drawn
/// rectangle runs leftwards from `anchor.x` rather than rightwards. Getting this
/// wrong is invisible in code review and obvious on screen.
pub fn text_rect(anchor: egui::Pos2, galley: &egui::text::Galley) -> egui::Rect {
    let size = galley.size();
    egui::Rect::from_min_max(
        egui::pos2(anchor.x - size.x, anchor.y),
        egui::pos2(anchor.x, anchor.y + size.y),
    )
}

/// The bubble the card should currently show, or `None` to hide the window.
pub struct CardContent {
    /// egui 0.28 hands out galleys as `Arc`s; `Painter::galley` wants one back.
    pub text: std::sync::Arc<egui::text::Galley>,
    pub text_size: [f32; 2],
    /// 0..=1, multiplies every alpha so the card can fade in and out.
    pub fade_alpha: f32,
    /// Seconds since the bubble was shown, and its total lifetime.
    pub elapsed_secs: f32,
    pub total_secs: f32,
    /// Set when the user clicks the card, to dismiss the bubble.
    pub dismissed: bool,
}

/// The OS window description, split out from [`report_window`] so the flags that
/// matter can be asserted in tests instead of being eyeballed on screen.
fn card_window_builder(pos: [f32; 2], visible: bool) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(PREVIEW_WINDOW_TITLE)
        .with_position(pos)
        .with_inner_size([WINDOW_W, WINDOW_MAX_H])
        // No title bar: the card draws its own rounded glass body, and a
        // caption here is a light band the user can see.
        .with_decorations(false)
        .with_always_on_top()
        .with_resizable(false)
        // Visibility, not existence, is what changes between bubbles.
        .with_visible(visible)
        .with_active(false)
        // The window is larger than the card and sits on top of the desktop,
        // so it must not take clicks over its whole rect. A window **region**
        // sized to the card does that and leaves the card itself interactive;
        // `with_mouse_passthrough` would do it at the cost of the whole window
        // (no click-to-dismiss) plus `WS_EX_LAYERED`, which winit sets
        // alongside `WS_EX_TRANSPARENT` and which does not belong on a window
        // whose pixels come from a `wgpu` swapchain.
        // (rollback: `.with_mouse_passthrough(true)` and delete the
        // `apply_click_region` call in `report_window`.)
        .with_mouse_passthrough(false)
        // NOTE: deliberately **not** `with_transparent(true)`.
        // It cannot give this window alpha — `egui_wgpu`'s `Painter` is built
        // once from the *root* viewport's setting and applies that one
        // `CompositeAlphaMode` to every surface — and on Windows 11 >= 22621 it
        // makes winit enable the system backdrop over the entire client area,
        // which is the pale box this module was written to remove.
        // `tests::card_window_does_not_ask_winit_for_transparency` pins it.
}

/// Reports the card window every frame, visible or not.
///
/// # Why every frame
///
/// The viewport has to appear in `egui`'s viewport output on every frame. If a
/// frame omits it, eframe treats the viewport as gone and starts tearing it
/// down — which is precisely the bug this module replaces. Hiding is done with
/// `ViewportBuilder::with_visible(false)`, which keeps the window alive but off
/// screen.
pub fn report_window(ctx: &Context, content: Option<&mut CardContent>, ppp: f32) {
    // egui 0.28 hands no way to reach a child viewport's HWND, so the Win32
    // correction for it lives in `window_shape` and finds the window by its own
    // title. Cheap, idempotent, and it re-applies on a timer so a window that
    // winit recreates cannot stay wrong.
    window_shape::ensure_preview_window_shaped();

    let pos = window_position(ppp);
    // The layout is computed here rather than inside the paint closure because
    // the OS window's click region has to be the card, and that needs the same
    // rectangle the painter will use.
    let layout = content.as_ref().map(|c| {
        CardLayout::new(
            c.text_size[0] + TEXT_PAD_X + DOT_MARGIN,
            c.text_size[1] + TEXT_PAD_Y * 2.0 + 6.0,
        )
    });
    apply_card_click_region(layout, ppp);

    let builder = card_window_builder(pos, layout.is_some());

    ctx.show_viewport_immediate(preview_viewport_id(), builder, |toast_ctx, _class| {
        let (Some(content), Some(layout)) = (content, layout) else {
            return;
        };

        // The child viewport's `egui::Context` starts on the *default* (light)
        // theme, whose `panel_fill` is `rgb(248,248,248)` — the exact colour
        // measured inside the card window. Nothing else in the app paints that
        // colour, and no Windows system colour is 248 (they are 240), so the
        // visuals have to be replaced before anything is drawn, not after.
        let mut transparent = egui::Visuals::dark();
        transparent.panel_fill = egui::Color32::TRANSPARENT;
        transparent.window_fill = egui::Color32::TRANSPARENT;
        transparent.extreme_bg_color = egui::Color32::TRANSPARENT;
        toast_ctx.set_visuals(transparent);
        toast_ctx.request_repaint_after(std::time::Duration::from_millis(16));

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(toast_ctx, |ui| {
                paint_card(ui, &layout, content);
            });
    });
}

/// Clips the card window's click region to the card.
///
/// The window is [`WINDOW_W`] × [`WINDOW_MAX_H`] — fixed, so a growing
/// transcript never resizes it — which means it always covers far more desktop
/// than the card does. Before this, `WS_EX_TRANSPARENT` made the *whole* window
/// click-through, which fixed the desktop but also killed click-to-dismiss and
/// put `WS_EX_LAYERED` on a swapchain window.
fn apply_card_click_region(layout: Option<CardLayout>, ppp: f32) {
    let hwnd = window_shape::preview_hwnd();
    if hwnd == 0 {
        return;
    }
    let region = match layout {
        Some(layout) => {
            let card = layout.card_rect();
            window_shape::ClickRegion::RoundedRect {
                rect_pt: [card.min.x, card.min.y, card.max.x, card.max.y],
                radius_pt: CARD_RADIUS,
            }
        }
        // Nothing drawn: the window is hidden, and a full rect would leave an
        // invisible click blocker behind if it were ever shown again without a
        // fresh layout.
        None => window_shape::ClickRegion::Full,
    };
    window_shape::apply_click_region(
        hwnd,
        region,
        [WINDOW_W, WINDOW_MAX_H],
        ppp,
    );
}

const TEXT_PAD_X: f32 = 20.0;
const TEXT_PAD_Y: f32 = 12.0;
const DOT_MARGIN: f32 = 28.0;

/// How wide the transcript may be before it wraps, in points.
///
/// Deliberately narrower than the card's own inner width: the card is sized
/// from the galley's actual width, so this is what keeps a long transcript
/// from ever growing the card past [`CARD_MAX_W`] and being clipped by the
/// window. `tests::transcript_is_drawn_inside_the_card` relies on it.
pub fn card_text_wrap_width() -> f32 {
    CARD_MAX_W - SHADOW_PAD * 2.0 - TEXT_PAD_X - DOT_MARGIN - 8.0
}

/// Anchors the window to the bottom centre of the screen, just above the
/// Windows taskbar.
///
/// This is the same work-area arithmetic the orb and the dashboard already use
/// ([`window_shape::taskbar_bottom_center_pt`]), which matters on machines where
/// the taskbar is not at the bottom or is auto-hidden: measuring the screen
/// instead of the work area is how a window ends up under the taskbar.
fn window_position(ppp: f32) -> [f32; 2] {
    let (x, y) = window_shape::taskbar_bottom_center_pt(WINDOW_W, WINDOW_MAX_H, ppp);
    [x.round(), y.round()]
}

/// Draws the glass card: shadow, body, border, sheen, status dot, text, timer.
fn paint_card(ui: &mut Ui, layout: &CardLayout, content: &mut CardContent) {
    let painter = ui.painter();
    let fade = content.fade_alpha;
    let card = layout.card_rect();

    // 1. Soft ambient shadow, four layers of decreasing alpha.
    for i in 1..=4 {
        let spread = i as f32 * 3.5;
        let alpha = ((20.0 / (i as f32 * 1.5)) * fade) as u8;
        painter.rect_filled(
            card.expand(spread),
            CARD_RADIUS + spread * 0.4,
            egui::Color32::from_black_alpha(alpha),
        );
    }

    // 2. Glass body.
    painter.rect_filled(
        card,
        CARD_RADIUS,
        egui::Color32::from_rgba_premultiplied(18, 22, 34, (215.0 * fade) as u8),
    );

    // 3. Hairline border.
    painter.rect_stroke(
        card,
        CARD_RADIUS,
        egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgba_premultiplied(255, 255, 255, (38.0 * fade) as u8),
        ),
    );

    // 4. Specular highlight along the top inner rim.
    let sheen_y = card.min.y + 1.2;
    painter.line_segment(
        [
            egui::pos2(card.min.x + 24.0, sheen_y),
            egui::pos2(card.max.x - 24.0, sheen_y),
        ],
        egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgba_premultiplied(255, 255, 255, (48.0 * fade) as u8),
        ),
    );

    // 5. Status dot.
    let dot = egui::pos2(card.max.x - 18.0, card.min.y + 18.0);
    painter.circle_filled(
        dot,
        5.5,
        egui::Color32::from_rgba_premultiplied(52, 211, 153, (55.0 * fade) as u8),
    );
    painter.circle_filled(
        dot,
        2.5,
        egui::Color32::from_rgba_premultiplied(52, 211, 153, (230.0 * fade) as u8),
    );

    // 6. Text, right-aligned for Persian, kept inside the card.
    //
    // `pos.x` is the galley's **anchor**, not its left edge. epaint anchors a
    // `RIGHT`-aligned galley at its right edge, so `galley.rect.right() == 0.0`
    // and every glyph position is negative
    // (`epaint-0.28.1/src/text/text_layout_types.rs`, "Bounding rect"), which
    // `Shape::galley` then translates by `pos` verbatim. Passing a left edge
    // here therefore draws the whole transcript one galley-width to the left of
    // the card, which is what "text outside the box" was.
    //
    // The width is safe by construction: the card is sized as the galley's own
    // width plus `TEXT_PAD_X + DOT_MARGIN`, so the text starts exactly
    // `TEXT_PAD_X` inside it. `card_text_rect` is what makes that an assertion
    // rather than a hope.
    let text = content.text.clone();
    let text_pos = egui::pos2(
        card.max.x - DOT_MARGIN,
        card.min.y + TEXT_PAD_Y,
    );
    painter.galley(
        text_pos,
        text.clone(),
        egui::Color32::from_rgba_premultiplied(245, 248, 255, (235.0 * fade) as u8),
    );
    debug_assert!(
        card.contains_rect(text_rect(text_pos, &text)),
        "transcript drawn outside the card: {:?} not inside {:?}",
        text_rect(text_pos, &text),
        card
    );

    // 7. Countdown bar along the bottom edge.
    let remaining = if content.total_secs > 0.0 {
        ((content.total_secs - content.elapsed_secs) / content.total_secs).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let bar_y = card.max.y - 1.5;
    let bar_w = (card.width() - 32.0) * remaining;
    painter.line_segment(
        [
            egui::pos2(card.min.x + 16.0, bar_y),
            egui::pos2(card.min.x + 16.0 + bar_w, bar_y),
        ],
        egui::Stroke::new(
            1.5_f32,
            egui::Color32::from_rgba_premultiplied(52, 211, 153, (130.0 * fade) as u8),
        ),
    );

    // 8. Click anywhere on the card to dismiss.
    //
    // Only reachable while `CARD_CLICKS_PASS_THROUGH` is `false`: with it on,
    // the window carries `WS_EX_TRANSPARENT` and never sees a click. Kept, not
    // deleted, so the rollback is a single constant.
    let response = ui.interact(
        card,
        egui::Id::new("transcript_bubble_hit"),
        egui::Sense::click(),
    );
    if response.clicked() {
        content.dismissed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context with fonts installed.
    ///
    /// `Context::fonts` panics with "No fonts available until first call to
    /// `Context::run`" — the font set is built on the first frame, not on
    /// construction — so any test that lays out a galley has to burn one empty
    /// frame first.
    fn test_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        ctx
    }

    #[test]
    fn card_window_does_not_ask_winit_for_transparency() {
        // This is the single line that produced the pale always-on-top box:
        // `with_transparent(true)` makes winit call
        // `DwmEnableBlurBehindWindow(DWM_BB_ENABLE | DWM_BB_BLURREGION)` in
        // `Window::on_create`, and on Windows 11 >= 22621 that turns the system
        // backdrop on over the whole client area. The card's alpha comes from
        // the root viewport's setting instead (`eframe` builds one
        // `egui_wgpu::Painter` from `native_options.viewport.transparent` and
        // reuses it for every surface), so nothing is lost by leaving it out.
        let builder = card_window_builder([0.0, 0.0], true);
        assert_ne!(
            builder.transparent,
            Some(true),
            "with_transparent(true) re-enables the Windows 11 system backdrop"
        );
    }

    #[test]
    fn card_window_does_not_block_the_desktop() {
        let builder = card_window_builder([0.0, 0.0], true);
        // `false`, not `true`: click-through is handled by a window **region**
        // sized to the card, which leaves the card itself interactive. See
        // `apply_card_click_region`.
        assert_eq!(builder.mouse_passthrough, Some(false));
        assert!(
            !builder.decorations.unwrap_or(true),
            "a caption is a light band above the card"
        );
        assert_eq!(builder.resizable, Some(false));
    }

    /// The contract the whole text placement rests on: epaint anchors a
    /// `RIGHT`-aligned galley at its **right** edge.
    ///
    /// If this ever stops holding, `paint_card` must start passing a left edge
    /// — and until someone notices, the transcript renders outside the card
    /// again. It is the single cheapest thing in the file to check and the
    /// single easiest to get wrong.
    #[test]
    fn right_aligned_galley_is_anchored_at_its_right_edge() {
        let ctx = test_ctx();
        let mut job = egui::text::LayoutJob::default();
        job.append(
            "گزینه کارت آزمایشی",
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(13.5),
                ..Default::default()
            },
        );
        job.halign = egui::Align::RIGHT;
        job.wrap = egui::text::TextWrapping::wrap_at_width(CARD_MAX_W);
        let galley = ctx.fonts(|f| f.layout_job(job));

        assert!(
            galley.rect.right().abs() < 0.01,
            "expected a right-anchored galley (rect.right() == 0), got {:?}",
            galley.rect
        );
        assert!(galley.rect.min.x < -1.0, "a right-anchored galley extends leftwards");

        // Which means painting at `anchor` draws it leftwards from there.
        let anchor = egui::pos2(400.0, 10.0);
        let drawn = text_rect(anchor, &galley);
        assert!((drawn.max.x - 400.0).abs() < 0.01);
        assert!(drawn.min.x < 400.0 - 1.0);
    }

    #[test]
    fn transcript_is_drawn_inside_the_card() {
        let ctx = test_ctx();
        for body in [
            "کارت",
            "گزینه کارت آزمایشی را باز کن",
            "یک متن بلند فارسی که حتما از حداکثر عرض کارت بیشتر می شود و باید بشکند و به چند خط تقسیم شود تا داخل کارت جا شود",
        ] {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                body,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::proportional(13.5),
                    ..Default::default()
                },
            );
            job.halign = egui::Align::RIGHT;
            job.wrap = egui::text::TextWrapping::wrap_at_width(card_text_wrap_width());
            let galley = ctx.fonts(|f| f.layout_job(job));
            let size = galley.size();

            let layout = CardLayout::new(size.x + TEXT_PAD_X + DOT_MARGIN, size.y + 30.0);
            let card = layout.card_rect();
            let anchor = egui::pos2(card.max.x - DOT_MARGIN, card.min.y + TEXT_PAD_Y);

            let drawn = text_rect(anchor, &galley);
            assert!(
                card.contains_rect(drawn),
                "{body:?}: text {:?} escaped the card {:?}",
                drawn,
                card
            );
            // The right edge is the edge that matters: the galley is
            // `Align::RIGHT`, so it is anchored at `anchor` and grows leftwards.
            // Asserting a constant left inset instead would only hold for text
            // that happens to fill the card exactly — which is the bug, not the
            // contract.
            assert!(
                (drawn.max.x - anchor.x).abs() < 0.01,
                "{body:?}: text ends at {:?} but the anchor is {anchor:?}",
                drawn.max.x
            );
            assert!(
                card.max.x - drawn.max.x - DOT_MARGIN < 0.51,
                "{body:?}: text does not end DOT_MARGIN inside the card"
            );
        }
    }

    #[test]
    fn card_click_region_is_the_card_not_the_window() {
        let layout = CardLayout::new(300.0, 60.0);
        let card = layout.card_rect();
        let region = window_shape::click_region_px(
            window_shape::ClickRegion::RoundedRect {
                rect_pt: [card.min.x, card.min.y, card.max.x, card.max.y],
                radius_pt: CARD_RADIUS,
            },
            [WINDOW_W, WINDOW_MAX_H],
            1.25,
        )
        .expect("a laid-out card always describes a real region");

        let window_shape::ClickRegion::RoundedRect { rect_pt, .. } = region else {
            panic!("expected a rounded rect, got {region:?}");
        };
        assert!(rect_pt[0] >= card.min.x - 0.51);
        assert!(rect_pt[2] <= card.max.x + 0.51);
        assert!(rect_pt[2] < WINDOW_W, "region must not cover the window");
        assert!(rect_pt[3] < WINDOW_MAX_H, "region must not cover the window");
    }

    #[test]
    fn show_and_hide_differ_only_in_visibility() {
        // If any other field differed between the two calls, a hidden window
        // and a shown one would be two different windows as far as eframe is
        // concerned, which is how the old per-bubble create/destroy cycle
        // (and its ghost windows) started.
        let shown = card_window_builder([10.0, 20.0], true);
        let hidden = card_window_builder([10.0, 20.0], false);
        assert_eq!(shown.visible, Some(true));
        assert_eq!(hidden.visible, Some(false));
        assert_eq!(shown.position, hidden.position);
        assert_eq!(shown.inner_size, hidden.inner_size);
        assert_eq!(shown.transparent, hidden.transparent);
    }

    #[test]
    fn window_size_is_independent_of_the_card() {
        // Two very different bubbles must produce the same OS window, or the
        // window is resized and a fresh, unpresented swapchain area is exposed.
        let short = CardLayout::new(240.0, 48.0);
        let tall = CardLayout::new(420.0, 200.0);
        assert_eq!(short.window_size, tall.window_size);
    }

    #[test]
    fn card_is_centred_in_the_window() {
        let layout = CardLayout::new(300.0, 60.0);
        let left_gap = layout.card_origin[0];
        let right_gap = layout.window_size[0] - layout.card_origin[0] - layout.card_w;
        assert!(
            (left_gap - right_gap).abs() < 0.01,
            "card off-centre: left {left_gap}, right {right_gap}"
        );
    }

    #[test]
    fn card_never_overflows_the_window() {
        for (w, h) in [(1.0, 1.0), (240.0, 48.0), (420.0, 260.0), (5000.0, 5000.0)] {
            let layout = CardLayout::new(w, h);
            assert!(layout.card_w <= layout.window_size[0] - SHADOW_PAD);
            assert!(layout.card_h <= layout.window_size[1] - SHADOW_PAD);
            assert!(layout.card_origin[0] + layout.card_w <= layout.window_size[0]);
            assert!(layout.card_origin[1] + layout.card_h <= layout.window_size[1]);
        }
    }

    #[test]
    fn window_stays_above_the_taskbar_at_every_dpi() {
        for ppp in [1.0_f32, 1.25, 1.5, 1.75, 2.0] {
            let [x, y] = window_position(ppp);
            assert!(x.is_finite() && y.is_finite(), "bad position at ppp={ppp}");
            assert!(y > 0.0, "window top off the screen at ppp={ppp}: {y}");
            if let Some((screen_w_px, screen_h_px)) = window_shape::true_screen_size_px() {
                let screen_w = screen_w_px as f32 / ppp;
                let screen_h = screen_h_px as f32 / ppp;
                assert!(WINDOW_W <= screen_w, "window wider than the screen");
                assert!(
                    y + WINDOW_MAX_H <= screen_h + 1.0,
                    "window below the screen bottom at ppp={ppp}: bottom {}",
                    y + WINDOW_MAX_H
                );
            }
        }
    }

    #[test]
    fn window_position_does_not_depend_on_the_card() {
        // A fixed window that moved with the text was the clearest symptom of
        // the pale box "jumping left" as the transcript grew.
        assert_eq!(window_position(1.25), window_position(1.25));
    }
}
