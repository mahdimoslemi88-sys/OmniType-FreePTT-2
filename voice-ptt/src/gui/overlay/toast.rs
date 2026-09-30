//! The transcript card: the 10-second bubble that appears after a dictation.
//!
//! # Status: currently disabled, and deliberately kept whole
//!
//! [`SHOW_TRANSCRIPT_CARD`] is `false`. The card was an `egui` child viewport,
//! and an `egui` child viewport is a real top-level `HWND`: on Windows 11
//! build 22621+ winit's `on_create` turns on a *system backdrop* for any
//! `with_transparent` window, that window swallows mouse input over its whole
//! rect, and it survives a few frames after egui stops reporting it — which is
//! the pale box that used to sit over the desktop and block clicks.
//!
//! The transcript itself is not lost: it is typed into the focused field, and
//! the dashboard history keeps the session text. Only the floating duplicate
//! went away.
//!
//! Nothing here was deleted, because turning the card back on is a one-line
//! change. That is the whole reason the feature flag lives in *this* module
//! rather than in the app: the flag and the code it gates are now in the same
//! file, so a reader cannot see one without the other.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_notify::{Anchor, Toast, ToastLevel, Toasts};
use egui_phosphor::regular as ic;

use super::preview_window;
use super::text::{format_persian_display, toast_caption};
use super::theme::palette;

/// Everything the transcript card owns.
///
/// Was three separate fields on `OverlayApp` (`toasts`, `live_toasts`,
/// `next_toast_seq`); grouping them means the card's bookkeeping can be read —
/// and tested — without an `OverlayApp`.
pub(crate) struct ToastState {
    /// Library-managed notification channel (egui-notify). Each transcript
    /// enqueues a toast that renders inside the preview viewport and manages
    /// its own lifetime, slide animation, and progress bar.
    toasts: Toasts,
    /// Mirror of the toasts currently alive, oldest first: the raw text,
    /// first version's enqueue time (lifetime anchor), and the countdown
    /// digit currently rendered on the card.
    live_toasts: VecDeque<(usize, String, Instant, u64)>,
    /// Monotonic id, so a chunked dictation can be told apart from a repeat.
    next_toast_seq: usize,
}

impl Default for ToastState {
    fn default() -> Self {
        Self {
            toasts: new_toast_channel(),
            live_toasts: VecDeque::new(),
            next_toast_seq: 0,
        }
    }
}

impl ToastState {
    /// Whether any card is on screen, from either bookkeeping list.
    ///
    /// Takes `&mut self` because `egui_notify` exposes its list mutably; the
    /// frame-pacing decision in the app needs the answer, not the list.
    pub(crate) fn has_visible_cards(&mut self) -> bool {
        !self.live_toasts.is_empty() || !self.toasts.toasts_mut().is_empty()
    }

    /// Drops every pending card without touching the channel's style.
    pub(crate) fn clear(&mut self) {
        self.live_toasts.clear();
    }

    /// Throws the notification channel away and builds a fresh one.
    ///
    /// Used when a new recording session starts: the previous session's
    /// bubbles must not survive into the next dictation.
    pub(crate) fn reset_channel(&mut self) {
        self.toasts = new_toast_channel();
    }

    /// Queues `text` as a card that lives for [`TOAST_TOTAL_SECS`].
    pub(crate) fn enqueue(&mut self, text: &str, now: Instant) {
        let seq = self.next_toast_seq;
        self.next_toast_seq += 1;
        self.live_toasts
            .push_back((seq, text.to_string(), now, TOAST_TOTAL_SECS));
    }
}

/// Whether the transcript card is ever built.
///
/// The single source of truth for the feature flag: the app checks this before
/// reading the user's preference, so the disabled path never touches settings.
pub(crate) fn enabled() -> bool {
    SHOW_TRANSCRIPT_CARD
}

// Toast card colors read from the shared palette. egui-notify reads
// `widgets.noninteractive.bg_fill` for the card background and
// `widgets.noninteractive.fg_stroke` for the caption, ✕ and progress bar;
// all three come straight from the app palette, with the accent carried by
// a Phosphor microphone glyph (same visual language as the capsule).
#[allow(dead_code)]
const TOAST_CARD_BG: egui::Color32 = palette::TOAST_BG;
#[allow(dead_code)]
const TOAST_TEXT: egui::Color32 = palette::TEXT_PRIMARY;
#[allow(dead_code)]
const TOAST_ACCENT: egui::Color32 = palette::ACCENT;

/// Height of the transparent glass host viewport; sized for two stacked
/// compact cards (max 4 caption rows each: 3 text + footer) with headroom,
/// so even the tallest preview never clips.
const TOAST_TOTAL_SECS: u64 = 10;

/// Whether the transcript card window is ever created.
///
/// # Off, and the whole card path is unreachable
///
/// The card is an `egui` child viewport, and an `egui` child viewport is a real
/// top-level `HWND`. Making it correct is not a matter of drawing it correctly:
/// on Windows 11 build 22621+ winit's `on_create` turns on a *system backdrop*
/// for any `with_transparent` window, that window swallows mouse input over its
/// whole rect, and it survives a few frames after egui stops reporting it —
/// which is the pale box that used to sit over the desktop and block clicks.
///
/// The transcript is still fully available: it is typed into the focused
/// field, and the dashboard history keeps the session text. Only the floating
/// duplicate is gone.
///
/// Set to `true` to bring the card back. Nothing else has to change —
/// [`OverlayApp::render_preview_toast_window`], [`crate::gui::preview_window`]
/// and [`crate::gui::window_shape::ensure_preview_window_shaped`] are all
/// still there and still wired up.
const SHOW_TRANSCRIPT_CARD: bool = false;

/// Width cap for a toast card (vendored egui-notify width-cap port of
/// ItsEthra/egui-notify#54). Capped captions hard-wrap (even unbreakable
/// tokens) and grow the card vertically instead of stretching; uncapped
/// toasts keep the library's snug auto width. Fits the 380 px host
/// viewport with margin.
#[allow(dead_code)]
const TOAST_MAX_WIDTH: f32 = 320.0;

// Compile-time sanity: the cap must fit the 380 px host viewport with room
// to spare, and stay above the smallest useful card width.
const _: () = {
    assert!(TOAST_MAX_WIDTH < 380.0_f32);
    assert!(TOAST_MAX_WIDTH > 90.0_f32);
};

/// Fresh `egui_notify` channel with the app's dark bottom-right layout.
#[allow(dead_code)]
fn new_toast_channel() -> Toasts {
    Toasts::new()
        .with_anchor(Anchor::BottomRight)
        .with_margin(egui::vec2(12.0, 10.0))
        .with_spacing(6.0)
        .with_default_font(egui::FontId::proportional(12.5))
}

/// Applies the OmniType dark palette to `ctx` for the duration of one pass,
/// returning the previous style for [`restore_toast_style`]. Scoped so the
/// main capsule and the other manager windows are never re-styled.
#[allow(dead_code)]
fn apply_toast_style(ctx: &egui::Context) -> std::sync::Arc<egui::Style> {
    let original = ctx.style();
    let mut styled = (*original).clone();
    styled.visuals.widgets.noninteractive.bg_fill = TOAST_CARD_BG;
    styled.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TOAST_TEXT);
    ctx.set_style(styled);
    original
}

/// Restores the style captured by [`apply_toast_style`].
#[allow(dead_code)]
fn restore_toast_style(ctx: &egui::Context, original: std::sync::Arc<egui::Style>) {
    ctx.set_style(original);
}

/// Paints a Phosphor microphone icon inside `rect` (replaces the hand-drawn vector mic).
#[allow(dead_code)]
fn paint_vector_mic(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        ic::MICROPHONE,
        egui::FontId::proportional(rect.height() * 0.7),
        color,
    );
}

/// Paints a Phosphor cross (✕) icon inside `rect`.
#[allow(dead_code)]
fn paint_vector_cross(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        ic::X,
        egui::FontId::proportional(stroke.width * 7.0),
        stroke.color,
    );
}

/// Paints a Phosphor checkmark (✓) icon inside `rect`.
#[allow(dead_code)]
fn paint_vector_check(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        ic::CHECK,
        egui::FontId::proportional(stroke.width * 7.0),
        stroke.color,
    );
}

impl ToastState {
    /// Builds a dark OmniType notification card for a transcript.
    ///
    /// phase 2: no longer called (the egui-notify channel is never rendered —
    /// see the note at its former call sites). Kept for rollback.
    #[allow(dead_code)]
    pub(crate) fn make_toast(raw: String, remaining_secs: u64, lifetime: Duration) -> Toast {
        let mut toast = Toast::basic(toast_caption(&raw, remaining_secs));
        toast.set_duration(Some(lifetime));
        toast.set_closable(true);
        toast.set_show_progress_bar(true);
        toast.set_level(ToastLevel::Custom(ic::MICROPHONE.to_string(), TOAST_ACCENT));
        toast.set_max_width(Some(TOAST_MAX_WIDTH));
        toast
    }

    /// Keeps the numeric 10-second countdown on the cards ticking. egui-notify
    /// freezes captions at enqueue time, so once per second (when a digit
    /// changes) the live toast's caption is rewritten **in place** — no
    /// toast is added or dismissed, so no appear/disappear animation runs
    /// and the host window never rebuilds (fixes the sub-second flicker).
    #[allow(dead_code)]
    pub(crate) fn refresh_countdowns(&mut self, now: Instant) {
        // Collect (raw text, old remaining, new remaining) for entries whose
        // countdown digit just changed.
        let mut changed: Vec<(String, u64, u64)> = Vec::new();
        for (_, raw, shown_at, rendered) in self.live_toasts.iter_mut() {
            let elapsed = now
                .duration_since(*shown_at)
                .as_secs()
                .min(TOAST_TOTAL_SECS);
            let remaining = TOAST_TOTAL_SECS.saturating_sub(elapsed);
            if *rendered != remaining {
                let old = *rendered;
                *rendered = remaining;
                changed.push((raw.clone(), old, remaining));
            }
        }
        // Rewrite the caption of the matching library toast in place. The old
        // caption is reconstructed exactly, so the right card is found even
        // when several transcripts share the same text.
        for (raw, old, new) in changed {
            let old_caption = toast_caption(&raw, old);
            let new_caption = toast_caption(&raw, new);
            for toast in self.toasts.toasts_mut() {
                if toast.caption() == old_caption {
                    toast.set_caption(new_caption);
                    break;
                }
            }
        }
    }
}

/// Lays out one bubble and computes its fade.
///
/// Fades in over 0.35 s and out over the last 0.8 s, so a bubble that is
/// replaced by a newer one does not pop.
fn build_card_content(
    ctx: &egui::Context,
    text: &str,
    shown_at: Instant,
    total_secs: u64,
    now: Instant,
) -> preview_window::CardContent {
    let elapsed = now.duration_since(shown_at).as_secs_f32();
    let total = total_secs as f32;
    let fade_in = (elapsed / 0.35).clamp(0.0, 1.0);
    let fade_out = ((total - elapsed) / 0.8).clamp(0.0, 1.0);
    let fade_alpha = (fade_in * fade_out).clamp(0.0, 1.0);

    let formatted = format_persian_display(text);
    let mut layout_job = egui::text::LayoutJob::single_section(
        formatted,
        egui::text::TextFormat {
            font_id: egui::FontId::proportional(13.5),
            color: egui::Color32::from_rgba_premultiplied(
                245,
                248,
                255,
                (235.0 * fade_alpha) as u8,
            ),
            ..Default::default()
        },
    );
    // Wrap at the card's inner width so a long transcript breaks into lines
    // instead of being clipped at the window edge.
    layout_job.wrap =
        egui::text::TextWrapping::wrap_at_width(preview_window::card_text_wrap_width());
    layout_job.halign = egui::Align::RIGHT;

    let galley = ctx.fonts(|f| f.layout_job(layout_job));
    let size = galley.size();

    preview_window::CardContent {
        text: galley,
        text_size: [size.x, size.y],
        fade_alpha,
        elapsed_secs: elapsed,
        total_secs: total,
        dismissed: false,
    }
}

/// Draws the newest live bubble into the preview window.
///
/// Assumes the caller has already checked [`enabled`] and read the
/// user's `show_transcript_bubble` preference: both are app policy, and
/// keeping them at the call site means the disabled path is visible
/// where a reader looks for it.
pub(crate) fn render(ctx: &egui::Context, state: &mut ToastState, bubble_enabled: bool) {
    let now = Instant::now();

    // Expire finished transcripts, oldest first.
    while let Some((_, _, shown_at, total_secs)) = state.live_toasts.front() {
        if now.duration_since(*shown_at) >= Duration::from_secs(*total_secs) {
            state.live_toasts.pop_front();
        } else {
            break;
        }
    }
    if !bubble_enabled {
        // Disabled: drop queued bubbles instead of leaving them pending.
        state.live_toasts.clear();
    }

    let ppp = ctx.pixels_per_point();
    let mut content = state
        .live_toasts
        .back()
        .map(|(_, text, shown_at, total_secs)| {
            build_card_content(ctx, text, *shown_at, *total_secs, now)
        });

    // Reported unconditionally, including when there is no bubble: the
    // viewport has to stay in egui's output every frame or eframe will tear
    // the window down, and a torn-down-but-not-yet-destroyed window is the
    // pale, click-stealing box this replaces.
    preview_window::report_window(ctx, content.as_mut(), ppp);

    if content.as_ref().is_some_and(|c| c.dismissed) {
        state.live_toasts.clear();
    }
}
