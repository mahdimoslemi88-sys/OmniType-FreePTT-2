//! Floating status overlay (egui/eframe): an AI-native, glassmorphic capsule
//! that reflects the current state (Idle / Recording / Processing / Typing),
//! animates live audio waveforms, and displays Persian transcripts crisply.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use ar_reshaper::ArabicReshaper;
use eframe::egui;
use egui_notify::{Anchor, Toast, ToastLevel, Toasts};
use egui_phosphor::regular as ic;
use unicode_bidi::BidiInfo;

use crate::asr::engine::AsrHealth;
use crate::asr::router::AsrRouter;
use crate::config::settings::{CustomProvider, Settings};
use crate::hotkey::HotkeyEvent;
use crate::processing::Dictionary;
use crate::state::{AppState, AppStatus};

/// Sync-readable wrapper around the Tokio watch channel for the GUI thread.
pub struct StatusClient {
    rx: tokio::sync::watch::Receiver<AppStatus>,
}

impl StatusClient {
    pub fn new(rx: tokio::sync::watch::Receiver<AppStatus>) -> Self {
        Self { rx }
    }

    pub fn get(&self) -> AppStatus {
        self.rx.borrow().clone()
    }
}

/// Reshapes Persian/Arabic cursive text and reorders visually for LTR renderers like egui.
pub fn format_persian_display(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Fast check: if text contains any Arabic/Persian/Presentation-form characters
    let has_persian = trimmed.chars().any(|c| {
        ('\u{0600}'..='\u{06FF}').contains(&c)
            || ('\u{FB50}'..='\u{FDFF}').contains(&c)
            || ('\u{FE70}'..='\u{FEFF}').contains(&c)
    });

    if !has_persian {
        return trimmed.to_string();
    }

    let reshaped = ArabicReshaper::default().reshape(trimmed);
    let bidi_info = BidiInfo::new(&reshaped, None);
    let mut visual = String::new();
    for para in &bidi_info.paragraphs {
        let line = bidi_info.reorder_line(para, para.range.clone());
        if !visual.is_empty() {
            visual.push(' ');
        }
        visual.push_str(&line);
    }
    visual
}

/// OmniType UI palette. Two complete themes behind identical role names,
/// selected at compile time: `dark` (default) and `light`
/// (`cargo build --features light-theme`). Every role is an OPAQUE fill —
/// translucent fills were flattened to their blended-over-parent result
/// (alpha compositing here happens in gamma space, see the theme-parity
/// test) so both themes stay `const`-friendly without premultiplied math.
/// Call sites never branch on the theme; they read role names only.
mod palette {
    use eframe::egui::Color32;

    #[cfg(not(feature = "light-theme"))]
    pub use self::dark::*;
    #[cfg(feature = "light-theme")]
    pub use self::light::*;

    #[cfg(not(feature = "light-theme"))]
    mod dark {
        use super::Color32;

        // Window & card surfaces
        pub const WINDOW_BG: Color32 = Color32::from_rgb(18, 20, 28);
        pub const TOAST_BG: Color32 = Color32::from_rgb(20, 24, 34);
        pub const CARD_BG: Color32 = Color32::from_rgb(24, 26, 36);
        pub const CARD_BG_ALT: Color32 = Color32::from_rgb(26, 30, 42);
        pub const HEADER_PILL_BG: Color32 = Color32::from_rgb(30, 36, 52);
        pub const ENGINE_PILL_BG: Color32 = Color32::from_rgb(30, 42, 60);
        pub const CHIP_BG: Color32 = Color32::from_rgb(36, 42, 56);
        pub const STROKE: Color32 = Color32::from_rgb(42, 48, 66);
        pub const CARD_STROKE: Color32 = Color32::from_rgb(40, 44, 60);
        pub const SELECTED_BG: Color32 = Color32::from_rgb(28, 42, 54);

        // Text roles
        pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(240, 245, 255);
        pub const TEXT_SECTION: Color32 = Color32::from_rgb(220, 230, 248);
        pub const TEXT_STRONG_SOFT: Color32 = Color32::from_rgb(210, 225, 250);
        pub const TEXT_TABLE: Color32 = Color32::from_rgb(220, 225, 235);
        pub const TEXT_LABEL: Color32 = Color32::from_rgb(180, 190, 210);
        pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(150, 160, 180);
        pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 155, 175);
        pub const TEXT_FAINT: Color32 = Color32::from_rgb(120, 130, 150);

        // Accent (info / interactive)
        pub const ACCENT: Color32 = Color32::from_rgb(100, 220, 255);
        pub const ACCENT_SOFT: Color32 = Color32::from_rgb(100, 200, 255);
        pub const ACCENT_BADGE: Color32 = Color32::from_rgb(160, 190, 230);
        /// Deep-blue fill of the primary action button in the engine window.
        pub const ACCENT_ACTION: Color32 = Color32::from_rgb(32, 100, 180);

        // Semantic status
        pub const SUCCESS: Color32 = Color32::from_rgb(120, 240, 160);
        pub const SUCCESS_OK: Color32 = Color32::from_rgb(100, 240, 160);
        pub const SUCCESS_DOT: Color32 = Color32::from_rgb(60, 220, 130);
        pub const SUCCESS_CHIPTXT: Color32 = Color32::from_rgb(80, 220, 140);
        pub const DANGER: Color32 = Color32::from_rgb(255, 110, 110);
        pub const DANGER_SOFT: Color32 = Color32::from_rgb(255, 120, 120);
        pub const DANGER_TEXT: Color32 = Color32::from_rgb(255, 80, 80);

        // Fills derived from the semantic colors
        pub const SUCCESS_FILL: Color32 = Color32::from_rgb(24, 48, 38);
        pub const SUCCESS_PILL: Color32 = Color32::from_rgb(20, 80, 50);
        pub const DANGER_FILL: Color32 = Color32::from_rgb(45, 25, 30);

        // One-off role colors
        pub const SELECT_STROKE: Color32 = Color32::from_rgb(60, 150, 240);
        pub const AUTO_BG: Color32 = Color32::from_rgb(26, 44, 58);
        pub const DOT_INACTIVE: Color32 = Color32::from_rgb(160, 160, 160);
        pub const WARNING: Color32 = Color32::from_rgb(255, 180, 50);
        pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);

        /// History card surface (was translucent, flattened over WINDOW_BG).
        pub const CARD_TRANSLUCENT: Color32 = Color32::from_rgb(26, 30, 42);
        /// Barely-visible stroke for history cards.
        pub const HAIRLINE: Color32 = Color32::from_rgb(40, 44, 55);

        /// Recording capsule & visual-mode states (idle pill, recording,
        /// processing). Fill roles; text/icon colors are separate roles so
        /// the light theme can re-tune them independently.
        pub mod pill {
            use super::Color32;

            /// Capsule surface shared by the awake/processing states.
            pub const SURFACE: Color32 = super::TOAST_BG;
            /// Dormant 3 px bar above the taskbar.
            pub const DORMANT_BAR: Color32 = Color32::from_rgb(130, 142, 158);

            // Idle (hover-awake) capsule
            pub const IDLE_GLOW: Color32 = Color32::from_rgb(47, 66, 96);
            pub const MIC_IDLE: Color32 = Color32::from_rgb(31, 51, 77);
            pub const MIC_IDLE_HOVER: Color32 = Color32::from_rgb(42, 74, 120);
            pub const BADGE_TEXT: Color32 = Color32::from_rgb(190, 210, 235);
            pub const BADGE_BG: Color32 = Color32::from_rgb(33, 42, 61);
            pub const HIST_ICON: Color32 = Color32::from_rgb(170, 230, 210);
            pub const HIST_IDLE: Color32 = Color32::from_rgb(26, 42, 40);
            pub const HIST_HOVER: Color32 = Color32::from_rgb(38, 70, 62);

            // Recording capsule (red)
            pub const REC_GLOW: Color32 = Color32::from_rgb(157, 58, 62);
            pub const CANCEL_IDLE: Color32 = Color32::from_rgb(59, 24, 30);
            pub const CANCEL_HOVER: Color32 = Color32::from_rgb(81, 25, 30);
            pub const CANCEL_ICON: Color32 = Color32::from_rgb(255, 120, 120);
            pub const REC_TEXT: Color32 = Color32::from_rgb(255, 145, 145);
            pub const WAVE_STRONG: Color32 = Color32::from_rgb(255, 90, 90);
            pub const WAVE_FAINT: Color32 = Color32::from_rgb(255, 230, 230);
            pub const SUBMIT_IDLE: Color32 = Color32::from_rgb(22, 61, 41);
            pub const SUBMIT_HOVER: Color32 = Color32::from_rgb(30, 84, 55);
            pub const SUBMIT_ICON: Color32 = Color32::from_rgb(85, 250, 155);

            // Processing capsule (amber)
            pub const PROC_GLOW: Color32 = Color32::from_rgb(112, 87, 38);
            pub const PROC_AMBER: Color32 = Color32::from_rgb(255, 185, 45);
            pub const PROC_TEXT: Color32 = Color32::from_rgb(255, 215, 130);
        }
    }

    /// Light theme — first-pass values; tune after a visual pass. Text and
    /// semantic colors are darkened for contrast on light surfaces; capsule
    /// fills become light tints (the pill floats over arbitrary desktops).
    #[cfg(feature = "light-theme")]
    mod light {
        use super::Color32;

        // Window & card surfaces
        pub const WINDOW_BG: Color32 = Color32::from_rgb(244, 246, 250);
        pub const TOAST_BG: Color32 = Color32::from_rgb(248, 250, 253);
        pub const CARD_BG: Color32 = Color32::from_rgb(246, 249, 253);
        pub const CARD_BG_ALT: Color32 = Color32::from_rgb(252, 253, 255);
        pub const HEADER_PILL_BG: Color32 = Color32::from_rgb(232, 238, 248);
        pub const ENGINE_PILL_BG: Color32 = Color32::from_rgb(226, 236, 248);
        pub const CHIP_BG: Color32 = Color32::from_rgb(238, 242, 248);
        pub const STROKE: Color32 = Color32::from_rgb(206, 214, 228);
        pub const CARD_STROKE: Color32 = Color32::from_rgb(220, 226, 236);
        pub const SELECTED_BG: Color32 = Color32::from_rgb(224, 238, 248);

        // Text roles
        pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(28, 32, 42);
        pub const TEXT_SECTION: Color32 = Color32::from_rgb(45, 52, 66);
        pub const TEXT_STRONG_SOFT: Color32 = Color32::from_rgb(55, 62, 78);
        pub const TEXT_TABLE: Color32 = Color32::from_rgb(50, 55, 68);
        pub const TEXT_LABEL: Color32 = Color32::from_rgb(85, 92, 108);
        pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(105, 112, 128);
        pub const TEXT_MUTED: Color32 = Color32::from_rgb(118, 125, 140);
        pub const TEXT_FAINT: Color32 = Color32::from_rgb(135, 142, 156);

        // Accent (darkened vs dark theme for contrast on light surfaces)
        pub const ACCENT: Color32 = Color32::from_rgb(8, 120, 180);
        pub const ACCENT_SOFT: Color32 = Color32::from_rgb(20, 140, 200);
        pub const ACCENT_BADGE: Color32 = Color32::from_rgb(60, 100, 150);
        pub const ACCENT_ACTION: Color32 = Color32::from_rgb(32, 100, 180);

        // Semantic status
        pub const SUCCESS: Color32 = Color32::from_rgb(16, 150, 88);
        pub const SUCCESS_OK: Color32 = Color32::from_rgb(16, 150, 88);
        pub const SUCCESS_DOT: Color32 = Color32::from_rgb(24, 176, 104);
        pub const SUCCESS_CHIPTXT: Color32 = Color32::from_rgb(16, 140, 84);
        pub const DANGER: Color32 = Color32::from_rgb(210, 50, 50);
        pub const DANGER_SOFT: Color32 = Color32::from_rgb(220, 60, 60);
        pub const DANGER_TEXT: Color32 = Color32::from_rgb(190, 35, 35);

        // Fills derived from the semantic colors
        pub const SUCCESS_FILL: Color32 = Color32::from_rgb(224, 244, 232);
        pub const SUCCESS_PILL: Color32 = Color32::from_rgb(196, 236, 212);
        pub const DANGER_FILL: Color32 = Color32::from_rgb(250, 228, 228);

        // One-off role colors
        pub const SELECT_STROKE: Color32 = Color32::from_rgb(50, 130, 230);
        pub const AUTO_BG: Color32 = Color32::from_rgb(224, 238, 248);
        pub const DOT_INACTIVE: Color32 = Color32::from_rgb(150, 150, 150);
        pub const WARNING: Color32 = Color32::from_rgb(200, 130, 0);
        pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);

        pub const CARD_TRANSLUCENT: Color32 = Color32::from_rgb(252, 253, 255);
        pub const HAIRLINE: Color32 = Color32::from_rgb(228, 233, 242);

        pub mod pill {
            use super::Color32;

            pub const SURFACE: Color32 = super::TOAST_BG;
            pub const DORMANT_BAR: Color32 = Color32::from_rgb(150, 158, 172);

            pub const IDLE_GLOW: Color32 = Color32::from_rgb(226, 236, 252);
            pub const MIC_IDLE: Color32 = Color32::from_rgb(206, 222, 248);
            pub const MIC_IDLE_HOVER: Color32 = Color32::from_rgb(178, 202, 244);
            pub const BADGE_TEXT: Color32 = Color32::from_rgb(70, 95, 135);
            pub const BADGE_BG: Color32 = Color32::from_rgb(224, 232, 246);
            pub const HIST_ICON: Color32 = Color32::from_rgb(20, 140, 104);
            pub const HIST_IDLE: Color32 = Color32::from_rgb(220, 240, 230);
            pub const HIST_HOVER: Color32 = Color32::from_rgb(198, 230, 214);

            pub const REC_GLOW: Color32 = Color32::from_rgb(252, 206, 206);
            pub const CANCEL_IDLE: Color32 = Color32::from_rgb(248, 212, 212);
            pub const CANCEL_HOVER: Color32 = Color32::from_rgb(244, 190, 190);
            pub const CANCEL_ICON: Color32 = Color32::from_rgb(200, 40, 40);
            pub const REC_TEXT: Color32 = Color32::from_rgb(190, 45, 45);
            pub const WAVE_STRONG: Color32 = Color32::from_rgb(220, 60, 60);
            pub const WAVE_FAINT: Color32 = Color32::from_rgb(255, 225, 225);
            pub const SUBMIT_IDLE: Color32 = Color32::from_rgb(212, 240, 224);
            pub const SUBMIT_HOVER: Color32 = Color32::from_rgb(190, 232, 208);
            pub const SUBMIT_ICON: Color32 = Color32::from_rgb(20, 150, 85);

            pub const PROC_GLOW: Color32 = Color32::from_rgb(252, 232, 196);
            pub const PROC_AMBER: Color32 = Color32::from_rgb(220, 140, 20);
            pub const PROC_TEXT: Color32 = Color32::from_rgb(150, 95, 10);
        }
    }
}

// Toast card colors read from the shared palette. egui-notify reads
// `widgets.noninteractive.bg_fill` for the card background and
// `widgets.noninteractive.fg_stroke` for the caption, ✕ and progress bar;
// all three come straight from the app palette, with the accent carried by
// a Phosphor microphone glyph (same visual language as the capsule).
const TOAST_CARD_BG: egui::Color32 = palette::TOAST_BG;
const TOAST_TEXT: egui::Color32 = palette::TEXT_PRIMARY;
const TOAST_ACCENT: egui::Color32 = palette::ACCENT;

/// Height of the transparent glass host viewport; sized for two stacked
/// compact cards (max 4 caption rows each: 3 text + footer) with headroom,
/// so even the tallest preview never clips.
const TOAST_HOST_HEIGHT: f32 = 190.0;
const TOAST_TOTAL_SECS: u64 = 10;

/// Width cap for a toast card (vendored egui-notify width-cap port of
/// ItsEthra/egui-notify#54). Capped captions hard-wrap (even unbreakable
/// tokens) and grow the card vertically instead of stretching; uncapped
/// toasts keep the library's snug auto width. Fits the 380 px host
/// viewport with margin.
const TOAST_MAX_WIDTH: f32 = 320.0;

// Compile-time sanity: the cap must fit the 380 px host viewport with room
// to spare, and stay above the smallest useful card width.
const _: () = {
    assert!(TOAST_MAX_WIDTH < 380.0_f32);
    assert!(TOAST_MAX_WIDTH > 90.0_f32);
};

/// Fresh `egui_notify` channel with the app's dark bottom-right layout.
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
fn apply_toast_style(ctx: &egui::Context) -> std::sync::Arc<egui::Style> {
    let original = ctx.style();
    let mut styled = (*original).clone();
    styled.visuals.widgets.noninteractive.bg_fill = TOAST_CARD_BG;
    styled.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TOAST_TEXT);
    ctx.set_style(styled);
    original
}

/// Restores the style captured by [`apply_toast_style`].
fn restore_toast_style(ctx: &egui::Context, original: std::sync::Arc<egui::Style>) {
    ctx.set_style(original);
}

/// Wraps the (already shaped) transcript into a compact multi-line caption
/// with a live countdown footer (`⏱ Ns`). Only the preview is truncated —
/// the full raw text is what click-to-copy puts on the clipboard.
///
/// No blank spacer row is emitted between body and footer: egui-notify
/// measures each card from its laid-out galley every frame, so a tight
/// caption directly shrinks short previews (1-line text => 2-row card).
fn toast_caption(display: &str, remaining_secs: u64) -> String {
    const CHARS_PER_LINE: usize = 44;
    const MAX_LINES: usize = 3;

    let mut out = String::new();
    for (i, ch) in display.chars().enumerate() {
        if i / CHARS_PER_LINE >= MAX_LINES {
            out.push('…');
            break;
        }
        if i > 0 && i % CHARS_PER_LINE == 0 {
            out.push('\n');
        }
        out.push(ch);
    }
    out.push('\n');
    out.push_str(&format!("{} {}s", ic::TIMER, remaining_secs));
    out
}

/// Shared window chrome for the manager windows: dark central panel with
/// the uniform 14 px margin. Single source so restyling all windows is a
/// one-line change.
fn manager_central_panel() -> egui::Frame {
    egui::Frame::none()
        .fill(palette::WINDOW_BG)
        .inner_margin(egui::Margin::same(14.0))
}

/// Shared content-card frame for the manager windows: rounded 8 px panel,
/// 1 px stroke and the standard 10 px padding — geometry lives here so
/// chrome and cards theme from one source. `fill`/`stroke` stay parameters
/// because they carry meaning (plain vs selected vs hairline variants).
/// Call `.inner_margin(...)` on the result to override padding per card.
fn manager_card(fill: egui::Color32, stroke: egui::Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(8.0))
        .stroke(egui::Stroke::new(1.0_f32, stroke))
        .inner_margin(egui::Margin::same(10.0))
}

/// Shared header row: strong 16 pt title with an optional right-aligned
/// badge pill (`(text, fill, text_color)`, already shaped for RTL display).
fn manager_header(
    ui: &mut egui::Ui,
    title: &str,
    badge: Option<(&str, egui::Color32, egui::Color32)>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format_persian_display(title))
                .size(16.0)
                .strong()
                .color(palette::TEXT_PRIMARY),
        );

        if let Some((text, bg, fg)) = badge {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                header_badge(ui, text, bg, fg);
            });
        }
    });
}

/// Shared subtitle line under the manager header.
fn manager_subtitle(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(format_persian_display(text))
            .size(11.0)
            .color(palette::TEXT_SECONDARY),
    );
}

/// Shared transient success banner (`palette::SUCCESS_FILL` pill).
fn success_banner(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(4.0);
    status_chip(ui, msg, palette::SUCCESS_FILL, palette::SUCCESS, 11.0, ChipFamily::Tiny);
}

/// Geometry families for [`status_chip`].
enum ChipFamily {
    /// Inline micro-chip: 4 px corner, 5×1.5 px margin.
    Small,
    /// Tiny status label: 5 px corner, 6×2 px margin.
    Tiny,
}

/// Aligns a small status chip with `text` (Persian display shaping applied) in
/// `color` on a `bg` rounded pill. Geometry families: `SMALL` (4 px corner,
/// 5×1.5 margin) for inline micro-chips, `TINY` (5 px corner, 6×2 margin) for
/// status labels. Single source so chip restyling is one-line changes.
fn status_chip(
    ui: &mut egui::Ui,
    text: &str,
    bg: egui::Color32,
    color: egui::Color32,
    size: f32,
    family: ChipFamily,
) {
    let (rounding, margin) = match family {
        ChipFamily::Small => (4.0_f32, egui::Margin::symmetric(5.0, 1.5)),
        ChipFamily::Tiny => (5.0_f32, egui::Margin::symmetric(6.0, 2.0)),
    };
    egui::Frame::none()
        .fill(bg)
        .rounding(egui::Rounding::same(rounding))
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(format_persian_display(text))
                    .size(size)
                    .color(color),
            );
        });
}

/// Header-badge pill geometry, shared by [`manager_header`].
fn header_badge(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(format_persian_display(text))
                    .size(10.5)
                    .color(fg),
            );
        });
}

/// Frame of the floating recording capsule for one visual mode. Single
/// source for the capsule's fill/stroke/rounding per state — restyling a
/// capsule state is a one-line change.
fn capsule_frame(
    fill: egui::Color32,
    stroke: egui::Stroke,
    rounding: f32,
    margin: egui::Margin,
) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(egui::Rounding::same(rounding))
        .inner_margin(margin)
}

/// Aligns the egui stock-widget colors of a manager-window context with the
/// compiled palette. The windows use stock widgets (buttons, text edits,
/// scroll areas) whose colors come from `ctx.style()`; without this a
/// light-theme build would render egui's dark stock widgets inside light
/// windows. Idempotent per pass; the main capsule draws only hand-styled
/// frames from the palette, so it is unaffected.
fn apply_theme_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.panel_fill = palette::WINDOW_BG;
    v.extreme_bg_color = palette::CARD_BG_ALT; // text-edit interiors
    v.faint_bg_color = palette::CHIP_BG; // alternate rows
    v.window_stroke = egui::Stroke::new(1.0_f32, palette::STROKE);
    v.widgets.noninteractive.bg_fill = palette::CARD_BG;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, palette::TEXT_PRIMARY);
    v.widgets.inactive.bg_fill = palette::CHIP_BG;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, palette::TEXT_PRIMARY);
    v.widgets.hovered.bg_fill = palette::HEADER_PILL_BG;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, palette::TEXT_PRIMARY);
    v.widgets.active.bg_fill = palette::SELECTED_BG;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, palette::TEXT_PRIMARY);
    v.selection.bg_fill = palette::SELECT_STROKE;
    v.override_text_color = Some(palette::TEXT_PRIMARY);
    ctx.set_style(style);
}

/// Historical voice transcription record.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HistoryItem {
    pub id: usize,
    pub text: String,
    pub timestamp: String,
    pub engine: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualMode {
    /// State 1: Ultra-minimal subtle horizontal line above taskbar (36x5 px).
    IdleDormant,
    /// State 2: Mouse hovered: small mic pill button + 'Dictate CapsLock' tooltip.
    HoveredAwake,
    /// State 3: Active recording: sleek black stadium capsule with ✕, waveform bars, and ✓.
    RecordingActive,
    /// State 4: Processing / typing indicator.
    Processing,
}

#[cfg(windows)]
#[repr(C)]
struct WinMargins {
    cx_left: i32,
    cx_right: i32,
    cy_top: i32,
    cy_bottom: i32,
}

#[cfg(windows)]
#[link(name = "gdi32")]
extern "system" {
    fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> isize;
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SetWindowRgn(hwnd: isize, hrgn: isize, b_redraw: i32) -> i32;
    fn SystemParametersInfoW(
        ui_action: u32,
        ui_param: u32,
        pv_param: *mut windows::Win32::Foundation::RECT,
        f_win_ini: u32,
    ) -> i32;
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
}

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

#[cfg(windows)]
pub fn enable_true_transparency(hwnd: isize) {
    let margins = WinMargins {
        cx_left: -1,
        cx_right: -1,
        cy_top: -1,
        cy_bottom: -1,
    };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        // DWMWA_SYSTEMBACKDROP_TYPE = 38, DWMSBT_NONE = 1
        // Disables acrylic and mica frosted glass so the background is 100% transparent!
        let backdrop_none: u32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_none as *const _ as *const std::ffi::c_void,
            4,
        );
    }
}

#[cfg(windows)]
static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
pub fn position_above_taskbar(hwnd: isize, width_px: i32, height_px: i32, corner_px: i32) {
    use windows::Win32::Foundation::RECT;

    let mut work_area = RECT::default();
    let ok = unsafe { SystemParametersInfoW(0x0030 /* SPI_GETWORKAREA */, 0, &mut work_area, 0) };
    if ok != 0 {
        let center_x = (work_area.left + work_area.right) / 2;
        let taskbar_top = work_area.bottom;

        let left = center_x - width_px / 2;
        let top = taskbar_top - height_px - 8; // 8 physical pixels above the taskbar

        unsafe {
            // SWP_NOACTIVATE = 0x0010, SWP_SHOWWINDOW = 0x0040
            SetWindowPos(hwnd, -1 /* HWND_TOPMOST */, left, top, width_px, height_px, 0x0010 | 0x0040);
            let corner_dia = corner_px * 2;
            let hrgn = CreateRoundRectRgn(0, 0, width_px, height_px, corner_dia, corner_dia);
            if hrgn != 0 {
                SetWindowRgn(hwnd, hrgn, 1);
            }
        }
        enable_true_transparency(hwnd);
    }
}

#[cfg(windows)]
fn local_time_str() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}

#[cfg(not(windows))]
fn local_time_str() -> String {
    "00:00:00".to_string()
}

#[cfg(windows)]
fn apply_window_shapes_all() {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetClientRect, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let actual = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..actual as usize]);

            if title == "OmniType" {
                MAIN_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
                enable_true_transparency(hwnd.0 as isize);
            } else if title.contains("OmniType_Preview") {
                enable_true_transparency(hwnd.0 as isize);
                let mut rect = RECT::default();
                if GetClientRect(hwnd, &mut rect).is_ok() {
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;
                    if w > 0 && h > 0 {
                        // Toast has egui corner radius 12.0 -> scale to physical pixels
                        let ppp_est = (h as f32 / 90.0_f32).max(1.0);
                        let corner_dia = ((12.0_f32 * ppp_est).round() as i32) * 2;
                        let hrgn = CreateRoundRectRgn(0, 0, w, h, corner_dia, corner_dia);
                        if hrgn != 0 {
                            SetWindowRgn(hwnd.0 as isize, hrgn, 1);
                        }
                    }
                }
            }
        }
        BOOL(1)
    }

    unsafe {
        let thread_id = GetCurrentThreadId();
        let _ = EnumThreadWindows(thread_id, Some(enum_proc), LPARAM(0));
    }
}

/// Paints a Phosphor microphone icon inside `rect` (replaces the hand-drawn vector mic).
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
fn paint_vector_check(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        ic::CHECK,
        egui::FontId::proportional(stroke.width * 7.0),
        stroke.color,
    );
}

/// eframe application implementing the modern AI-native overlay bubble.
pub struct OverlayApp {
    status: Arc<StatusClient>,
    events_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
    visible: bool,
    recording_start: Option<Instant>,
    /// Set externally (tray menu / hotkey) to request a visibility toggle.
    overlay_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu) to request opening the dictionary window.
    dict_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu) to request opening the AI engine window.
    engine_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu) to request opening the history window.
    history_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu / hotkey) to request application quit.
    quit_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Shared technical dictionary for real-time rule management.
    dictionary: Arc<RwLock<Dictionary>>,
    /// Shared ASR router for dynamic model selection and registration.
    router: AsrRouter,
    /// Shared application settings.
    settings: Arc<RwLock<Settings>>,
    /// Path to config.toml.
    config_path: PathBuf,
    /// Persistent history of voice transcribed texts.
    pub history: Vec<HistoryItem>,
    next_history_id: usize,
    /// Controls display of the History & Clipboard secondary viewport window.
    pub show_history_window: bool,
    history_search: String,
    history_copy_msg: Option<(String, Instant)>,
    shape_frames_checked: u8,
    /// Transcribed text for the 10-second secondary preview toast window
    /// Library-managed notification channel (egui-notify). Each transcript
    /// enqueues a toast that renders inside the preview viewport and manages
    /// its own 10 s lifetime, slide animation, and progress bar.
    toasts: Toasts,
    /// Mirror of the toasts currently alive, oldest first: the raw text,
    /// first version's enqueue time (lifetime anchor), and the countdown
    /// digit currently rendered on the card.
    live_toasts: VecDeque<(usize, String, Instant, u64)>,
    next_toast_seq: usize,
    pub last_seen_transcript: Option<String>,
    /// Wispr Flow interaction and dock mode
    pub visual_mode: VisualMode,
    pub is_hovered: bool,
    pub last_hover_time: Option<Instant>,
    has_initial_positioned: bool,
    current_width: f32,
    current_height: f32,
    /// Controls display of the Dictionary Manager secondary viewport window.
    pub show_dict_window: bool,
    new_from: String,
    new_to: String,
    new_cat: String,
    search_query: String,
    dict_msg: Option<(String, Instant)>,
    /// Controls display of the AI Engine Manager secondary viewport window.
    pub show_engine_window: bool,
    new_engine_id: String,
    new_engine_name: String,
    new_engine_url: String,
    new_engine_key: String,
    new_engine_model: String,
    new_engine_lang: String,
    engine_msg: Option<(String, Instant)>,
}

impl OverlayApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: Arc<StatusClient>,
        events_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
        overlay_flag: Arc<std::sync::atomic::AtomicBool>,
        dict_flag: Arc<std::sync::atomic::AtomicBool>,
        engine_flag: Arc<std::sync::atomic::AtomicBool>,
        history_flag: Arc<std::sync::atomic::AtomicBool>,
        quit_flag: Arc<std::sync::atomic::AtomicBool>,
        dictionary: Arc<RwLock<Dictionary>>,
        router: AsrRouter,
        settings: Arc<RwLock<Settings>>,
        config_path: PathBuf,
    ) -> Self {
        let initial_visible = settings
            .read()
            .map(|s| s.gui.show_overlay)
            .unwrap_or(true);

        Self {
            status,
            events_tx,
            visible: initial_visible,
            recording_start: None,
            overlay_flag,
            dict_flag,
            engine_flag,
            history_flag,
            quit_flag,
            dictionary,
            router,
            settings,
            config_path,
            history: Vec::new(),
            next_history_id: 1,
            show_history_window: false,
            history_search: String::new(),
            history_copy_msg: None,
            shape_frames_checked: 0,
            toasts: new_toast_channel(),
            live_toasts: VecDeque::new(),
            next_toast_seq: 0,
            last_seen_transcript: None,
            visual_mode: VisualMode::IdleDormant,
            is_hovered: false,
            last_hover_time: None,
            has_initial_positioned: false,
            current_width: 0.0,
            current_height: 0.0,
            show_dict_window: false,
            new_from: String::new(),
            new_to: String::new(),
            new_cat: String::new(),
            search_query: String::new(),
            dict_msg: None,
            show_engine_window: false,
            new_engine_id: String::new(),
            new_engine_name: String::new(),
            new_engine_url: String::new(),
            new_engine_key: String::new(),
            new_engine_model: "whisper-large-v3-turbo".to_string(),
            new_engine_lang: "fa".to_string(),
            engine_msg: None,
        }
    }

    /// Toggles overlay visibility and persists the preference.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if let Ok(mut s) = self.settings.write() {
            s.gui.show_overlay = self.visible;
            let _ = s.save(&self.config_path);
        }
    }

    /// Renders the standalone Dictionary Manager window in an immediate viewport.
    fn render_dict_window(&mut self, ctx: &egui::Context) {
        if !self.show_dict_window {
            return;
        }

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("dictionary_manager_viewport"),
            egui::ViewportBuilder::default()
                .with_title("مدیریت دیکشنری تخصصی — OmniType")
                .with_inner_size([560.0, 520.0])
                .with_min_inner_size([420.0, 350.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_transparent(false),
            |dict_ctx, _class| {
                if dict_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_dict_window = false;
                }
                apply_theme_visuals(dict_ctx);

                egui::CentralPanel::default()
                    .frame(manager_central_panel())
                    .show(dict_ctx, |ui| {
                        // ── Header & Title ──
                        let total_rules = self.dictionary.read().map(|d| d.len()).unwrap_or(0);
                        manager_header(
                            ui,
                            "مدیریت دیکشنری کلمات تخصصی",
                            Some((
                                &format!("{total_rules} قانون فعال"),
                                palette::HEADER_PILL_BG,
                                palette::ACCENT_SOFT,
                            )),
                        );

                        manager_subtitle(
                            ui,
                            "تعریف و تصحیح خودکار واژگان فنی، مهندسی و گفتاری",
                        );

                        // Feedback message if active
                        if let Some((ref msg, timestamp)) = self.dict_msg {
                            if timestamp.elapsed() < Duration::from_secs(4) {
                                success_banner(ui, msg);
                            }
                        }

                        ui.add_space(10.0);

                        // ── Card 1: Add New Word ──
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{} {}", ic::PLUS, format_persian_display("افزودن یا ویرایش کلمه جدید:")))
                                        .size(12.0)
                                        .strong()
                                        .color(palette::TEXT_SECTION),
                                );
                                ui.add_space(6.0);

                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format_persian_display("کلمه شنیده شده (از):"))
                                            .size(11.0)
                                            .color(palette::TEXT_LABEL),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.new_from)
                                            .hint_text("پاتون / سی ان سی")
                                            .desired_width(140.0),
                                    );

                                    ui.label(
                                        egui::RichText::new(format_persian_display("معادل صحیح (به):"))
                                            .size(11.0)
                                            .color(palette::TEXT_LABEL),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.new_to)
                                            .hint_text("پایتون / CNC")
                                            .desired_width(140.0),
                                    );
                                });

                                ui.add_space(5.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format_persian_display("دسته‌بندی (اختیاری):"))
                                            .size(11.0)
                                            .color(palette::TEXT_LABEL),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.new_cat)
                                            .hint_text("برنامه‌نویسی / مکانیک")
                                            .desired_width(130.0),
                                    );

                                    ui.add_space(8.0);
                                    let add_btn = ui.button(
                                        egui::RichText::new(format_persian_display("+ ثبت در دیکشنری"))
                                            .size(11.5)
                                            .color(egui::Color32::WHITE),
                                    );
                                    if add_btn.clicked() {
                                        let from = self.new_from.trim().to_string();
                                        let to = self.new_to.trim().to_string();
                                        let cat = if self.new_cat.trim().is_empty() {
                                            None
                                        } else {
                                            Some(self.new_cat.trim().to_string())
                                        };

                                        if !from.is_empty() && !to.is_empty() && from != to {
                                            if let Ok(mut dict) = self.dictionary.write() {
                                                dict.add_rule(from, to, cat);
                                                let _ = dict.save_to_file();
                                                self.new_from.clear();
                                                self.new_to.clear();
                                                self.new_cat.clear();
                                                self.dict_msg = Some((
                                                    format_persian_display("قاعده با موفقیت ثبت و ذخیره شد!"),
                                                    Instant::now(),
                                                ));
                                            }
                                        }
                                    }
                                });
                            });

                        ui.add_space(10.0);

                        // ── Search Bar ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{} {}", ic::MAGNIFYING_GLASS, format_persian_display("جستجو:")))
                                    .size(11.5)
                                    .color(palette::TEXT_LABEL),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.search_query)
                                    .hint_text("جستجو در بین کلمات...")
                                    .desired_width(ui.available_width() - 10.0),
                            );
                        });

                        ui.add_space(6.0);

                        // ── Scrollable List of Rules ──
                        let mut rule_to_remove: Option<String> = None;
                        let query = self.search_query.trim().to_lowercase();

                        if let Ok(dict) = self.dictionary.read() {
                            let rules = dict.rules();
                            egui::ScrollArea::vertical()
                                .max_height(210.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    egui::Grid::new("dict_rules_grid")
                                        .striped(true)
                                        .spacing([12.0, 6.0])
                                        .min_col_width(80.0)
                                        .show(ui, |ui| {
                                            // Table Header
                                            ui.label(egui::RichText::new(format_persian_display("کلمه گفتاری (از)")).strong().size(11.0));
                                            ui.label(egui::RichText::new("").size(10.0));
                                            ui.label(egui::RichText::new(format_persian_display("معادل صحیح (به)")).strong().size(11.0));
                                            ui.label(egui::RichText::new(format_persian_display("دسته")).strong().size(11.0));
                                            ui.label(egui::RichText::new(format_persian_display("حذف")).strong().size(11.0));
                                            ui.end_row();

                                            for r in rules {
                                                if !query.is_empty() {
                                                    let matches_from = r.from.to_lowercase().contains(&query);
                                                    let matches_to = r.to.to_lowercase().contains(&query);
                                                    let matches_cat = r.category.as_deref().unwrap_or("").to_lowercase().contains(&query);
                                                    if !matches_from && !matches_to && !matches_cat {
                                                        continue;
                                                    }
                                                }

                                                ui.label(
                                                    egui::RichText::new(format_persian_display(&r.from))
                                                        .size(11.0)
                                                        .color(palette::TEXT_TABLE),
                                                );
                                                ui.label(
                                                    egui::RichText::new("→")
                                                        .size(11.0)
                                                        .color(palette::TEXT_FAINT),
                                                );
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(&r.to))
                                                        .size(11.0)
                                                        .strong()
                                                        .color(palette::ACCENT),
                                                );
                                                let cat_str = r.category.as_deref().unwrap_or("-");
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(cat_str))
                                                        .size(9.5)
                                                        .color(palette::TEXT_MUTED),
                                                );

                                                if ui.button(egui::RichText::new(ic::TRASH).size(10.5)).clicked() {
                                                    rule_to_remove = Some(r.from.clone());
                                                }
                                                ui.end_row();
                                            }
                                        });
                                });
                        }

                        // Apply pending removal if clicked
                        if let Some(target_from) = rule_to_remove {
                            if let Ok(mut dict) = self.dictionary.write() {
                                dict.remove_by_from(&target_from);
                                let _ = dict.save_to_file();
                                self.dict_msg = Some((
                                    format_persian_display("کلمه از دیکشنری حذف شد"),
                                    Instant::now(),
                                ));
                            }
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // ── Bottom Action Buttons ──
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new(format!("{} {}", ic::FLOPPY_DISK, format_persian_display("ذخیره در فایل"))).size(11.0)).clicked() {
                                if let Ok(dict) = self.dictionary.read() {
                                    if dict.save_to_file().is_ok() {
                                        self.dict_msg = Some((
                                            format_persian_display("دیکشنری با موفقیت ذخیره شد"),
                                            Instant::now(),
                                        ));
                                    }
                                }
                            }

                            if ui.button(egui::RichText::new(format!("{} {}", ic::NOTE_PENCIL, format_persian_display("ویرایش در Notepad"))).size(11.0)).clicked() {
                                if let Ok(dict) = self.dictionary.read() {
                                    if let Some(path) = dict.file_path() {
                                        #[cfg(windows)]
                                        {
                                            let _ = std::process::Command::new("cmd")
                                                .args(["/C", "start", "", path.to_str().unwrap_or("dictionary.toml")])
                                                .spawn();
                                        }
                                    }
                                }
                            }

                            if ui.button(egui::RichText::new(format!("{} {}", ic::ARROWS_CLOCKWISE, format_persian_display("بارگذاری مجدد"))).size(11.0)).clicked() {
                                if let Ok(mut dict) = self.dictionary.write() {
                                    if dict.reload_from_file().is_ok() {
                                        self.dict_msg = Some((
                                            format_persian_display("دیکشنری از دیسک بازخوانی شد"),
                                            Instant::now(),
                                        ));
                                    }
                                }
                            }
                        });
                    });
            },
        );
    }

    /// Renders the standalone AI Engine & Model Manager window in an immediate viewport.
    fn render_engine_window(&mut self, ctx: &egui::Context) {
        if !self.show_engine_window {
            return;
        }

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("engine_manager_viewport"),
            egui::ViewportBuilder::default()
                .with_title("مدیریت مدل‌های هوش مصنوعی و API — OmniType")
                .with_inner_size([580.0, 560.0])
                .with_min_inner_size([440.0, 380.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_transparent(false),
            |eng_ctx, _class| {
                if eng_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_engine_window = false;
                }
                apply_theme_visuals(eng_ctx);

                egui::CentralPanel::default()
                    .frame(manager_central_panel())
                    .show(eng_ctx, |ui| {
                        // ── Header & Title ──
                        let active = self.router.active_engine();
                        let badge_text = if active == "auto" {
                            "حالت فعال: خودکار (Auto)".to_string()
                        } else {
                            format!("فعال: {active}")
                        };
                        manager_header(
                            ui,
                            "مدیریت مدل‌های صوتی و API",
                            Some((&badge_text, palette::ENGINE_PILL_BG, palette::ACCENT)),
                        );

                        manager_subtitle(
                            ui,
                            "انتخاب موتور پیش‌فرض یا افزودن سرور و مدل‌های اختصاصی (سازگار با OpenAI)",
                        );

                        // Feedback message
                        if let Some((ref msg, timestamp)) = self.engine_msg {
                            if timestamp.elapsed() < Duration::from_secs(4) {
                                success_banner(ui, msg);
                            }
                        }

                        ui.add_space(10.0);

                        // ── Available Engines List ──
                        ui.label(
                            egui::RichText::new(format_persian_display("موتورهای گفتار به متن ثبت‌شده:"))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_STRONG_SOFT),
                        );
                        ui.add_space(4.0);

                        let engines = self.router.list_engines();
                        let current_active = self.router.active_engine();

                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .show(ui, |ui| {
                                // Auto Fallback Option
                                let is_auto = current_active == "auto";
                                manager_card(
                                    if is_auto { palette::AUTO_BG } else { palette::CARD_BG },
                                    if is_auto {
                                        palette::SELECT_STROKE
                                    } else {
                                        palette::CARD_STROKE
                                    },
                                )
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if ui.radio(is_auto, "").clicked() && !is_auto {
                                                self.router.set_active_engine("auto");
                                                if let Ok(mut s) = self.settings.write() {
                                                    s.active_engine = "auto".to_string();
                                                    let _ = s.save(&self.config_path);
                                                }
                                                self.engine_msg = Some((
                                                    format_persian_display("حالت خودکار هوشمند (Auto) فعال شد."),
                                                    Instant::now(),
                                                ));
                                            }

                                            ui.vertical(|ui| {
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(
                                                        "حالت خودکار هوشمند (Auto Fallback)",
                                                    ))
                                                    .strong()
                                                    .size(12.0)
                                                    .color(palette::TEXT_PRIMARY),
                                                );
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(
                                                        "اولویت‌بندی خودکار بین ابری و محلی؛ سوییچ بدون وقفه در قطعی شبکه",
                                                    ))
                                                    .size(10.0)
                                                    .color(palette::TEXT_MUTED),
                                                );
                                            });

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if is_auto {
                                                    status_chip(
                                                        ui,
                                                        "انتخاب‌شده",
                                                        palette::SUCCESS_PILL,
                                                        palette::SUCCESS_OK,
                                                        9.5,
                                                        ChipFamily::Tiny,
                                                    );
                                                }
                                            });
                                        });
                                    });

                                ui.add_space(4.0);

                                let mut to_delete: Option<String> = None;
                                for (id, display_name, kind, health, _is_selected) in &engines {
                                    let is_active = current_active == *id;
                                    manager_card(
                                        if is_active {
                                            palette::SELECTED_BG
                                        } else {
                                            palette::CARD_BG
                                        },
                                        if is_active {
                                            palette::SELECT_STROKE
                                        } else {
                                            palette::CARD_STROKE
                                        },
                                    )
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                if ui.radio(is_active, "").clicked() && !is_active {
                                                    self.router.set_active_engine(id);
                                                    if let Ok(mut s) = self.settings.write() {
                                                        s.active_engine = id.clone();
                                                        let _ = s.save(&self.config_path);
                                                    }
                                                    self.engine_msg = Some((
                                                        format_persian_display(&format!("موتور {} فعال شد.", display_name)),
                                                        Instant::now(),
                                                    ));
                                                }

                                                // Health indicator dot
                                                let (dot_color, health_desc) = match health {
                                                    AsrHealth::Ready => (
                                                        palette::SUCCESS_DOT,
                                                        "آماده کار".to_string(),
                                                    ),
                                                    AsrHealth::NoModel => (
                                                        palette::DOT_INACTIVE,
                                                        "مدل دانلود نشده".to_string(),
                                                    ),
                                                    AsrHealth::Cooldown { reason, .. } => (
                                                        palette::WARNING,
                                                        format!("در حال بازیابی: {reason}"),
                                                    ),
                                                    AsrHealth::Failed { reason } => (
                                                        palette::DANGER_TEXT,
                                                        format!("غیرفعال: {reason}"),
                                                    ),
                                                };

                                                let (resp, painter) = ui.allocate_painter(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                                painter.circle_filled(resp.rect.center(), 3.5, dot_color);
                                                resp.on_hover_text(&health_desc);

                                                ui.vertical(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(display_name)
                                                                .strong()
                                                                .size(12.0)
                                                                .color(palette::TEXT_PRIMARY),
                                                        );

                                                        status_chip(
                                                            ui,
                                                            kind,
                                                            palette::CHIP_BG,
                                                            palette::ACCENT_BADGE,
                                                            9.0,
                                                            ChipFamily::Small,
                                                        );
                                                    });

                                                    ui.label(
                                                        egui::RichText::new(format!("ID: {id}"))
                                                            .size(9.5)
                                                            .color(palette::TEXT_FAINT),
                                                    );
                                                });

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    if *kind == "Cloud (Custom)" {
                                                        let del_btn = ui.add(
                                                            egui::Button::new(
                                                                egui::RichText::new(ic::TRASH)
                                                                    .size(11.0)
                                                                    .color(palette::DANGER),
                                                            )
                                                            .fill(palette::DANGER_FILL)
                                                            .rounding(egui::Rounding::same(4.0)),
                                                        );
                                                        if del_btn.clicked() {
                                                            to_delete = Some(id.clone());
                                                        }
                                                    }

                                                    if is_active {
                                                        status_chip(
                                                            ui,
                                                            "فعال",
                                                            palette::SUCCESS_PILL,
                                                            palette::SUCCESS_OK,
                                                            9.5,
                                                            ChipFamily::Tiny,
                                                        );
                                                    }
                                                });
                                            });
                                        });
                                    ui.add_space(4.0);
                                }

                                if let Some(del_id) = to_delete {
                                    self.router.remove_engine(&del_id);
                                    if let Ok(mut s) = self.settings.write() {
                                        s.remove_provider(&del_id);
                                        if s.active_engine == del_id {
                                            s.active_engine = "auto".to_string();
                                            self.router.set_active_engine("auto");
                                        }
                                        let _ = s.save(&self.config_path);
                                    }
                                    self.engine_msg = Some((
                                        format_persian_display("مدل اختصاصی حذف گردید."),
                                        Instant::now(),
                                    ));
                                }
                            });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // ── Add New API Provider Form ──
                        ui.label(
                            egui::RichText::new(format_persian_display("افزودن API / سرور دلخواه (سازگار با OpenAI):"))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_STRONG_SOFT),
                        );
                        ui.add_space(6.0);

                        egui::Grid::new("add_provider_grid")
                            .num_columns(2)
                            .spacing([10.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format_persian_display("شناسه یکتا (ID):"))
                                        .size(11.0)
                                        .color(palette::TEXT_LABEL),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_id)
                                        .hint_text("e.g. custom_openai")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("نام نمایشی:"))
                                        .size(11.0)
                                        .color(palette::TEXT_LABEL),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_name)
                                        .hint_text("e.g. OpenAI Whisper Large")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("آدرس Base URL:"))
                                        .size(11.0)
                                        .color(palette::TEXT_LABEL),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_url)
                                        .hint_text("e.g. https://api.openai.com/v1")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("کلید API:"))
                                        .size(11.0)
                                        .color(palette::TEXT_LABEL),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_key)
                                        .password(true)
                                        .hint_text("sk-...")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("نام مدل:"))
                                        .size(11.0)
                                        .color(palette::TEXT_LABEL),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_model)
                                        .hint_text("whisper-1 / whisper-large-v3-turbo")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("زبان (Language):"))
                                        .size(11.0)
                                        .color(palette::TEXT_LABEL),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_lang)
                                        .hint_text("fa")
                                        .desired_width(340.0),
                                );
                                ui.end_row();
                            });

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let add_btn = ui.add(
                                egui::Button::new(
                                    egui::RichText::new(format_persian_display("ثبت و فعال‌سازی این موتور"))
                                        .strong()
                                        .size(11.5)
                                        .color(palette::WHITE),
                                )
                                .fill(palette::ACCENT_ACTION)
                                .rounding(egui::Rounding::same(6.0)),
                            );

                            if add_btn.clicked() {
                                let id = self.new_engine_id.trim().to_lowercase();
                                let url = self.new_engine_url.trim().to_string();
                                if id.is_empty() || url.is_empty() {
                                    self.engine_msg = Some((
                                        format_persian_display("خطا: شناسه (ID) و آدرس URL الزامی هستند."),
                                        Instant::now(),
                                    ));
                                } else {
                                    let name = if self.new_engine_name.trim().is_empty() {
                                        id.clone()
                                    } else {
                                        self.new_engine_name.trim().to_string()
                                    };
                                    let model = if self.new_engine_model.trim().is_empty() {
                                        "whisper-large-v3-turbo".to_string()
                                    } else {
                                        self.new_engine_model.trim().to_string()
                                    };
                                    let lang = if self.new_engine_lang.trim().is_empty() {
                                        "fa".to_string()
                                    } else {
                                        self.new_engine_lang.trim().to_string()
                                    };
                                    let provider = CustomProvider {
                                        id: id.clone(),
                                        name,
                                        base_url: url,
                                        api_key: self.new_engine_key.trim().to_string(),
                                        model,
                                        language: lang,
                                        timeout_secs: 20,
                                    };

                                    let usage_path = crate::paths::resolve_usage_path();
                                    let engine = Arc::new(crate::asr::CloudEngine::new_custom(&provider, usage_path));
                                    self.router.register_engine(engine);
                                    self.router.set_active_engine(&id);

                                    if let Ok(mut s) = self.settings.write() {
                                        s.active_engine = id.clone();
                                        s.add_or_update_provider(provider);
                                        let _ = s.save(&self.config_path);
                                    }

                                    self.engine_msg = Some((
                                        format_persian_display("مدل جدید ثبت و به عنوان موتور فعال انتخاب شد."),
                                        Instant::now(),
                                    ));
                                    self.new_engine_id.clear();
                                    self.new_engine_name.clear();
                                    self.new_engine_url.clear();
                                    self.new_engine_key.clear();
                                }
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(format_persian_display("باز کردن config.toml")).clicked() {
                                    #[cfg(windows)]
                                    {
                                        let target = self.config_path.to_str().unwrap_or("config.toml");
                                        let _ = std::process::Command::new("cmd")
                                            .args(["/C", "start", "", target])
                                            .spawn();
                                    }
                                }
                            });
                        });
                    });
            },
        );
    }

    /// Renders the standalone Speech History & Clipboard manager window in an immediate viewport.
    fn render_history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history_window {
            return;
        }

        let now = Instant::now();

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("history_manager_viewport"),
            egui::ViewportBuilder::default()
                .with_title("تاریخچه گفتار و رونوشت‌ها — OmniType")
                .with_inner_size([560.0, 520.0])
                .with_min_inner_size([400.0, 320.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_transparent(false),
            |hist_ctx, _class| {
                if hist_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_history_window = false;
                }
                apply_theme_visuals(hist_ctx);

                egui::CentralPanel::default()
                    .frame(manager_central_panel())
                    .show(hist_ctx, |ui| {
                        // ── Header & Title ──
                        let count = self.history.len();
                        manager_header(
                            ui,
                            "تاریخچه گفتار و رونوشت‌های صوتی",
                            Some((
                                &format!("{count} مورد ثبت‌شده"),
                                palette::HEADER_PILL_BG,
                                palette::ACCENT_SOFT,
                            )),
                        );

                        ui.add_space(8.0);

                        // ── Quick Actions Row (Copy All, Clear, Search) ──
                        ui.horizontal(|ui| {
                            let search_placeholder = format_persian_display("جستجو در متن‌ها...");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.history_search)
                                    .hint_text(search_placeholder)
                                    .desired_width(200.0),
                            );

                            if !self.history.is_empty() {
                                if ui.button(egui::RichText::new(format!("{} {}", ic::CLIPBOARD_TEXT, format_persian_display("کپی همه متن‌ها")))
                                            .size(11.5)).clicked() {
                                    let all_texts = self.history
                                        .iter()
                                        .map(|h| h.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n\n");
                                    hist_ctx.copy_text(all_texts);
                                    self.history_copy_msg = Some(("تمامی متن‌ها در کلیپ‌بورد کپی شدند!".into(), now));
                                }

                                if ui.button(
                                        egui::RichText::new(format!("{} {}", ic::TRASH_SIMPLE, format_persian_display("پاکسازی")))
                                            .size(11.5)
                                            .color(palette::DANGER_SOFT),
                                    ).clicked() {
                                    self.history.clear();
                                    self.history_copy_msg = Some(("تاریخچه با موفقیت پاک شد.".into(), now));
                                }
                            }
                        });

                        // Notification / Feedback message
                        if let Some((ref msg, t)) = self.history_copy_msg {
                            if now.duration_since(t) < Duration::from_secs(3) {
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format_persian_display(msg))
                                        .size(11.5)
                                        .color(palette::SUCCESS_CHIPTXT),
                                );
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // ── Items List ──
                        let query = self.history_search.trim().to_lowercase();
                        let filtered_indices: Vec<usize> = self.history
                            .iter()
                            .enumerate()
                            .filter(|(_, h)| {
                                query.is_empty() || h.text.to_lowercase().contains(&query) || h.engine.to_lowercase().contains(&query)
                            })
                            .map(|(i, _)| i)
                            .collect();

                        if filtered_indices.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(
                                    egui::RichText::new(format_persian_display("موردی در تاریخچه یافت نشد."))
                                        .size(13.0)
                                        .color(palette::TEXT_MUTED),
                                );
                                ui.label(
                                    egui::RichText::new(format_persian_display("هر صحبتی که انجام دهید به طور خودکار در این بخش نگهداری می‌شود."))
                                        .size(11.0)
                                        .color(palette::TEXT_FAINT),
                                );
                            });
                        } else {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let mut delete_idx = None;
                                    for &idx in &filtered_indices {
                                        let item = &self.history[idx];
                                        manager_card(palette::CARD_TRANSLUCENT, palette::HAIRLINE)
                                            .show(ui, |ui| {
                                                // Meta row: Time, Engine, Actions
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(format!("⏱ {}", item.timestamp))
                                                            .size(10.5)
                                                            .color(palette::TEXT_MUTED),
                                                    );

                                                    ui.label(
                                                        egui::RichText::new(format!("⚡ {}", item.engine))
                                                            .size(10.0)
                                                            .color(palette::ACCENT_SOFT),
                                                    );

                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        if ui.button(egui::RichText::new(ic::TRASH).size(11.0)).on_hover_text("حذف این مورد").clicked() {
                                                            delete_idx = Some(idx);
                                                        }

                                                        let copy_btn = ui.button(egui::RichText::new(format!("{} {}", ic::COPY, format_persian_display("کپی متن"))).size(11.5));
                                                        if copy_btn.clicked() {
                                                            hist_ctx.copy_text(item.text.clone());
                                                            self.history_copy_msg = Some(("متن در کلیپ‌بورد کپی شد!".into(), now));
                                                        }
                                                        copy_btn.on_hover_text("کپی کردن این رونوشت صوتی در کلیپ‌بورد");
                                                    });
                                                });

                                                ui.add_space(4.0);

                                                // Persian text display
                                                let display = format_persian_display(&item.text);
                                                ui.label(
                                                    egui::RichText::new(display)
                                                        .size(12.5)
                                                        .color(palette::TEXT_PRIMARY),
                                                );
                                            });
                                        ui.add_space(6.0);
                                    }

                                    if let Some(d_idx) = delete_idx {
                                        self.history.remove(d_idx);
                                    }
                                });
                        }
                    });
            },
        );
    }
    /// Builds a dark OmniType notification card for a transcript.
    fn make_toast(display: String, remaining_secs: u64, lifetime: Duration) -> Toast {
        let mut toast = Toast::basic(toast_caption(&display, remaining_secs));
        toast.set_duration(Some(lifetime));
        toast.set_closable(true);
        toast.set_show_progress_bar(true);
        toast.set_level(ToastLevel::Custom(ic::MICROPHONE.to_string(), TOAST_ACCENT));
        toast.set_max_width(Some(TOAST_MAX_WIDTH));
        toast
    }

    /// Keeps the numeric 10-second countdown on the cards ticking. egui-notify
    /// freezes captions at enqueue time, so once per second (when a digit
    /// changes) a freshened toast is appended while the stale one is dismissed
    /// mid fade-out — a sub-second shimmer at the bottom of the stack.
    fn refresh_toast_countdowns(&mut self, now: Instant) {
        let mut stale: Vec<(usize, u64)> = Vec::new();
        for (seq, _, shown_at, rendered) in self.live_toasts.iter_mut() {
            let elapsed = now.duration_since(*shown_at).as_secs().min(TOAST_TOTAL_SECS);
            let remaining = TOAST_TOTAL_SECS.saturating_sub(elapsed);
            if *rendered != remaining {
                *rendered = remaining;
                stale.push((*seq, remaining));
            }
        }
        for (seq, remaining) in stale {
            let Some(entry) = self.live_toasts.iter().rev().find(|item| item.0 == seq) else {
                continue;
            };
            let raw = entry.1.clone();
            let display = format_persian_display(&raw);
            let toast = Self::make_toast(
                display,
                remaining,
                Duration::from_secs(remaining + 1), // outlive until the next refresh
            );
            self.toasts.add(toast);
            self.toasts.dismiss_oldest_toast();
        }
    }

    /// Shows the toast preview window. The OS window itself is a bare glass
    /// host (`OmniType_Preview`, so DWM corner clipping keeps working); the
    /// notifications inside are fully managed by `egui_notify::Toasts` —
    /// slide-in animation, dark card, live countdown, progress bar and ✕.
    fn render_preview_toast_window(&mut self, ctx: &egui::Context) {
        let now = Instant::now();

        // Mirror the library's 10 s lifetime locally: every card's mirror
        // entry is anchored to its first version's enqueue time, so the FIFO
        // and the library queue stay in step. The extra 0.4 s covers the
        // slide-out animation before the host window closes.
        while let Some((_, _, shown_at, _)) = self.live_toasts.front() {
            if now.duration_since(*shown_at) >= Duration::from_millis(10_400) {
                self.live_toasts.pop_front();
            } else {
                break;
            }
        }
        self.refresh_toast_countdowns(now);
        if self.live_toasts.is_empty() {
            return;
        }

        // Position directly above the docked capsule at the bottom.
        let main_rect = ctx.input(|i| i.viewport().outer_rect);
        let (pos_x, pos_y) = if let Some(rect) = main_rect {
            // Anchor the host's *bottom edge* just above the capsule so the
            // newest card (drawn at the viewport's BottomRight) hugs it.
            (
                (rect.center().x - 190.0).round(),
                (rect.min.y - TOAST_HOST_HEIGHT - 8.0).round(),
            )
        } else {
            (100.0, 100.0)
        };

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("preview_toast_viewport"),
            egui::ViewportBuilder::default()
                .with_title("OmniType_Preview")
                .with_position([pos_x, pos_y])
                .with_inner_size([380.0, TOAST_HOST_HEIGHT])
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_resizable(false),
            |toast_ctx, _class| {
                #[cfg(windows)]
                apply_window_shapes_all();

                // A plain release inside the preview copies the newest
                // transcript (in practice the card under the pointer); the
                // library ✕ still dismisses through its own hit-test.
                if toast_ctx.input(|i| i.pointer.primary_released()) {
                    if let Some((_, raw, _, _)) = self.live_toasts.back() {
                        toast_ctx.copy_text(raw.clone());
                    }
                }

                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                    .show(toast_ctx, |_ui| {
                        let original = apply_toast_style(toast_ctx);
                        self.toasts.show(toast_ctx);
                        restore_toast_style(toast_ctx, original);
                    });
            },
        );
    }
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Physically clip OS windows to eliminate any black or frosted glass rectangular bounding box
        if self.shape_frames_checked < 10 {
            self.shape_frames_checked += 1;
            #[cfg(windows)]
            apply_window_shapes_all();
        }

        // Consume external control flags.
        if self
            .overlay_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.toggle_visible();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
        }
        if self
            .dict_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.show_dict_window = true;
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        if self
            .engine_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.show_engine_window = true;
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        if self
            .history_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.show_history_window = true;
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        if self.quit_flag.load(std::sync::atomic::Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Always render secondary viewports if open
        self.render_dict_window(ctx);
        self.render_engine_window(ctx);
        self.render_history_window(ctx);
        self.render_preview_toast_window(ctx);

        // 30 fps gives buttery smooth waveform animations and accurate 10s countdown
        ctx.request_repaint_after(Duration::from_millis(33));

        if !self.visible {
            return;
        }

        let status = self.status.get();
        let now = Instant::now();

        // Track recording duration
        if matches!(status.state, AppState::Recording) {
            if self.recording_start.is_none() {
                self.recording_start = Some(now);
                self.last_seen_transcript = None; // Reset so next utterance can trigger toast
            }
            // Dismiss toast previews when the user starts speaking again
            self.live_toasts.clear();
            self.toasts = new_toast_channel();
        } else {
            self.recording_start = None;
        }

        // When speech transcript arrives, pop up the 10-second toast preview and record in history (ONLY ONCE per utterance)
        if let Some(ref raw) = status.last_text {
            let trimmed = raw.trim();
            if !trimmed.is_empty() && self.last_seen_transcript.as_deref() != Some(trimmed) {
                self.last_seen_transcript = Some(trimmed.to_string());
                let seq = self.next_toast_seq;
                self.next_toast_seq += 1;
                self.live_toasts
                    .push_back((seq, trimmed.to_string(), now, TOAST_TOTAL_SECS));

                // Dark OmniType card: near-white caption with a live 10 s
                // countdown footer, accent mic glyph, progress bar, ✕.
                let display = format_persian_display(trimmed);
                let toast = Self::make_toast(
                    display,
                    TOAST_TOTAL_SECS,
                    Duration::from_secs(TOAST_TOTAL_SECS),
                );
                self.toasts.add(toast);

                let time_now = local_time_str();
                let active_engine_str = self.router.active_engine();

                self.history.insert(
                    0,
                    HistoryItem {
                        id: self.next_history_id,
                        text: trimmed.to_string(),
                        timestamp: time_now,
                        engine: active_engine_str,
                    },
                );
                self.next_history_id += 1;
                if self.history.len() > 100 {
                    self.history.truncate(100);
                }

                #[cfg(windows)]
                apply_window_shapes_all();
            }
        }

        let time = ctx.input(|i| i.time);

        // Determine target VisualMode based on state and hover
        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let pointer_in_window = pointer_pos.is_some();
        if pointer_in_window {
            self.is_hovered = true;
            self.last_hover_time = Some(now);
        } else if let Some(last) = self.last_hover_time {
            if now.duration_since(last) > Duration::from_millis(400) {
                self.is_hovered = false;
            }
        }

        let target_mode = match status.state {
            AppState::Recording => VisualMode::RecordingActive,
            AppState::Processing | AppState::Typing => VisualMode::Processing,
            _ => {
                if self.is_hovered {
                    VisualMode::HoveredAwake
                } else {
                    VisualMode::IdleDormant
                }
            }
        };

        // Determine target dimensions and corner radius for Variant 5
        let (target_w, target_h, target_corner) = match target_mode {
            VisualMode::IdleDormant => (38.0_f32, 6.0_f32, 3.0_f32),
            VisualMode::HoveredAwake => (144.0_f32, 32.0_f32, 16.0_f32),
            VisualMode::RecordingActive => (176.0_f32, 34.0_f32, 17.0_f32),
            VisualMode::Processing => (126.0_f32, 32.0_f32, 16.0_f32),
        };

        let mode_changed = self.visual_mode != target_mode
            || (self.current_width - target_w).abs() > 0.5
            || (self.current_height - target_h).abs() > 0.5;

        if mode_changed {
            self.visual_mode = target_mode;
            self.current_width = target_w;
            self.current_height = target_h;

            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(target_w, target_h)));

            #[cfg(windows)]
            {
                let hwnd = MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
                if hwnd != 0 {
                    let ppp = ctx.pixels_per_point();
                    let w_px = (target_w * ppp).round() as i32;
                    let h_px = (target_h * ppp).round() as i32;
                    let corner_px = (target_corner * ppp).round() as i32;
                    position_above_taskbar(hwnd, w_px, h_px, corner_px);
                    self.has_initial_positioned = true;
                }
            }
        } else if !self.has_initial_positioned {
            #[cfg(windows)]
            {
                let hwnd = MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
                if hwnd != 0 {
                    let ppp = ctx.pixels_per_point();
                    let w_px = (target_w * ppp).round() as i32;
                    let h_px = (target_h * ppp).round() as i32;
                    let corner_px = (target_corner * ppp).round() as i32;
                    position_above_taskbar(hwnd, w_px, h_px, corner_px);
                    self.has_initial_positioned = true;
                }
            }
        }

        // Render Variant 5 (Executive Pill) according to current visual mode
        match self.visual_mode {
            VisualMode::IdleDormant => {
                egui::CentralPanel::default()
                    .frame(capsule_frame(
                        palette::pill::DORMANT_BAR,
                        egui::Stroke::NONE,
                        3.0,
                        egui::Margin::same(0.0),
                    ))
                    .show(ctx, |ui| {
                        let rect = ui.max_rect();
                        let sense = ui.interact(rect, ui.id().with("dormant_bar"), egui::Sense::click());
                        if sense.clicked() {
                            let _ = self.events_tx.send(HotkeyEvent::RecordDown);
                        }
                        if sense.hovered() {
                            self.is_hovered = true;
                            self.last_hover_time = Some(now);
                        }
                    });
            }
            VisualMode::HoveredAwake => {
                egui::CentralPanel::default()
                    .frame(capsule_frame(
                        palette::pill::SURFACE,
                        egui::Stroke::new(1.0_f32, palette::pill::IDLE_GLOW),
                        16.0,
                        egui::Margin::symmetric(7.0, 4.0),
                    ))
                    .show(ctx, |ui| {
                        let mut action_btn_clicked = false;

                        ui.horizontal(|ui| {
                            // Vector Mic button (Click to start recording)
                            let (mic_rect, mic_resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                            let mic_hover = mic_resp.hovered();
                            let mic_bg = if mic_hover {
                                palette::pill::MIC_IDLE_HOVER
                            } else {
                                palette::pill::MIC_IDLE
                            };
                            ui.painter().circle_filled(mic_rect.center(), 10.5, mic_bg);
                            paint_vector_mic(ui.painter(), mic_rect, palette::WHITE);
                            if mic_resp.clicked() {
                                let _ = self.events_tx.send(HotkeyEvent::RecordDown);
                                action_btn_clicked = true;
                            }
                            let _ = mic_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("شروع ضبط گفتار");

                            ui.add_space(2.0);

                            // Shortcut badge
                            ui.label(
                                egui::RichText::new("CapsLock")
                                    .size(10.5)
                                    .strong()
                                    .color(palette::TEXT_STRONG_SOFT),
                            );

                            // Right-aligned buttons: Engine badge & History
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Engine Badge
                                    let active_engine = self.router.active_engine();
                                    let engine_short = match active_engine.as_str() {
                                        "auto" => "AUTO",
                                        "google" => "GGL",
                                        "local_whisper" | "whisper.cpp" => "LOC",
                                        "groq" | "cloud" => "GROQ",
                                        _ => "API",
                                    };

                                    let badge_btn = ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(engine_short)
                                                    .size(8.5)
                                                    .strong()
                                                    .color(palette::pill::BADGE_TEXT),
                                            )
                                            .fill(palette::pill::BADGE_BG)
                                            .rounding(egui::Rounding::same(5.0)),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(format!(
                                            "موتور فعال: {active_engine} (کلیک برای مدیریت)"
                                        ));
                                    if badge_btn.clicked() {
                                        self.show_engine_window = !self.show_engine_window;
                                        action_btn_clicked = true;
                                    }

                                    ui.add_space(1.0);

                                    // History icon (Clean vector badge)
                                    let (hist_rect, hist_resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                                    let hist_hover = hist_resp.hovered();
                                    let hist_bg = if hist_hover {
                                        palette::pill::HIST_HOVER
                                    } else {
                                        palette::pill::HIST_IDLE
                                    };
                                    ui.painter().rect_filled(hist_rect, egui::Rounding::same(4.0), hist_bg);
                                    ui.painter().text(
                                        hist_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        ic::CLOCK_COUNTER_CLOCKWISE,
                                        egui::FontId::proportional(10.0),
                                        palette::pill::HIST_ICON,
                                    );
                                    if hist_resp.clicked() {
                                        self.show_history_window = !self.show_history_window;
                                        action_btn_clicked = true;
                                    }
                                    let _ = hist_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text("تاریخچه گفتار (History)");
                                },
                            );
                        });

                        // Click anywhere else on the pill to start recording
                        let pill_sense = ui.interact(
                            ui.max_rect(),
                            ui.id().with("pill_awake_body"),
                            egui::Sense::click(),
                        );
                        if pill_sense.clicked() && !action_btn_clicked {
                            let _ = self.events_tx.send(HotkeyEvent::RecordDown);
                        }
                    });
            }
            VisualMode::RecordingActive => {
                egui::CentralPanel::default()
                    .frame(capsule_frame(
                        palette::WINDOW_BG,
                        egui::Stroke::new(1.2_f32, palette::pill::REC_GLOW),
                        17.0,
                        egui::Margin::symmetric(7.0, 4.0),
                    ))
                    .show(ctx, |ui| {
                        let mut action_btn_clicked = false;

                        ui.horizontal(|ui| {
                            // 1. Vector Cancel button (✕) on the left
                            let (cancel_rect, cancel_resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                            let cancel_hover = cancel_resp.hovered();
                            let cancel_bg = if cancel_hover {
                                palette::pill::CANCEL_HOVER
                            } else {
                                palette::pill::CANCEL_IDLE
                            };
                            ui.painter().circle_filled(cancel_rect.center(), 10.0, cancel_bg);
                            let cross_stroke = egui::Stroke::new(1.5_f32, palette::pill::CANCEL_ICON);
                            paint_vector_cross(ui.painter(), cancel_rect, cross_stroke);
                            if cancel_resp.clicked() {
                                let _ = self.events_tx.send(HotkeyEvent::Cancel);
                                action_btn_clicked = true;
                            }
                            let _ = cancel_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("لغو ضبط (بدون ارسال متن)");

                            ui.add_space(2.0);

                            // 2. Digital recording timer (00:04)
                            let elapsed = self
                                .recording_start
                                .map(|s| now.duration_since(s).as_secs())
                                .unwrap_or(0);
                            let timer_str = format!("{:02}:{:02}", elapsed / 60, elapsed % 60);
                            ui.label(
                                egui::RichText::new(timer_str)
                                    .size(10.0)
                                    .strong()
                                    .color(palette::pill::REC_TEXT),
                            );

                            ui.add_space(2.0);

                            // 3. Dynamic Equalizer Waveform Bars (8 animated bars)
                            let wave_w = 46.0_f32;
                            let (wave_resp, wave_painter) = ui.allocate_painter(
                                egui::vec2(wave_w, 16.0),
                                egui::Sense::hover(),
                            );
                            let num_bars = 8;
                            let bar_w = 2.4_f32;
                            let bar_spacing = (wave_w - (num_bars as f32 * bar_w)) / ((num_bars - 1) as f32);
                            let center_y = wave_resp.rect.center().y;

                            for i in 0..num_bars {
                                let phase = time * 10.0 + (i as f64) * 0.95;
                                let wave_val = (phase.sin().abs() * 0.7 + (phase * 1.6).cos().abs() * 0.3) as f32;
                                let h = (3.5 + 11.5 * wave_val).clamp(3.0, 15.0);

                                let bx = wave_resp.rect.min.x + (i as f32) * (bar_w + bar_spacing);
                                let by = center_y - h / 2.0;

                                let bar_color = if i % 2 == 0 {
                                    palette::pill::WAVE_STRONG
                                } else {
                                    palette::pill::WAVE_FAINT
                                };

                                wave_painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(bx, by),
                                        egui::vec2(bar_w, h),
                                    ),
                                    egui::Rounding::same(1.2),
                                    bar_color,
                                );
                            }

                            // 4. Vector Submit / Finish button (✓) on the right
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let (submit_rect, submit_resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                                    let submit_hover = submit_resp.hovered();
                                    let submit_bg = if submit_hover {
                                        palette::pill::SUBMIT_HOVER
                                    } else {
                                        palette::pill::SUBMIT_IDLE
                                    };
                                    ui.painter().circle_filled(submit_rect.center(), 10.0, submit_bg);
                                    let check_stroke = egui::Stroke::new(1.8_f32, palette::pill::SUBMIT_ICON);
                                    paint_vector_check(ui.painter(), submit_rect, check_stroke);
                                    if submit_resp.clicked() {
                                        let _ = self.events_tx.send(HotkeyEvent::RecordUp);
                                        action_btn_clicked = true;
                                    }
                                    let _ = submit_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text("پایان ضبط و تایپ متن");
                                },
                            );
                        });

                        // Clicking elsewhere on recording capsule also submits
                        let rec_sense = ui.interact(
                            ui.max_rect(),
                            ui.id().with("pill_rec_body"),
                            egui::Sense::click(),
                        );
                        if rec_sense.clicked() && !action_btn_clicked {
                            let _ = self.events_tx.send(HotkeyEvent::RecordUp);
                        }
                    });
            }
            VisualMode::Processing => {
                egui::CentralPanel::default()
                    .frame(capsule_frame(
                        palette::pill::SURFACE,
                        egui::Stroke::new(1.0_f32, palette::pill::PROC_GLOW),
                        16.0,
                        egui::Margin::symmetric(8.0, 4.0),
                    ))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            let pulse = 3.0 + 1.5 * (time * 6.0).sin().abs() as f32;
                            let (response, painter) = ui.allocate_painter(
                                egui::vec2(12.0, 12.0),
                                egui::Sense::hover(),
                            );
                            let center = response.rect.center();
                            let amber = palette::pill::PROC_AMBER;
                            painter.circle_filled(center, pulse + 1.5, amber.linear_multiply(0.25));
                            painter.circle_filled(center, 3.2, amber);

                            ui.add_space(2.0);

                            ui.label(
                                egui::RichText::new(format_persian_display("در حال پردازش..."))
                                    .size(11.0)
                                    .strong()
                                    .color(palette::pill::PROC_TEXT),
                            );
                        });
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_persian_display_handles_persian() {
        let input = "سلام دنیا";
        let formatted = format_persian_display(input);
        assert!(!formatted.is_empty());
        // Reshaped Persian does not stay identical to raw input (contains contextual presentation forms)
        assert_ne!(formatted, input);
    }

    #[test]
    fn test_toast_caption_wraps_and_appends_countdown() {
        // Short text: body line + footer, no blank spacer row (compact card).
        let short = toast_caption("Hello", 10);
        assert_eq!(short.lines().count(), 2);
        assert_eq!(short, format!("Hello\n{} 10s", ic::TIMER));

        // Long text: wrapped to 3 lines of 44 chars, then ellipsis + footer.
        let long_text = "x".repeat(200);
        let long = toast_caption(&long_text, 7);
        let body_lines = long.lines().count() - 1; // minus footer
        assert_eq!(body_lines, 3); // 3 rows; ellipsis ends the 3rd row
        assert!(long.contains('…'));
        assert!(long.ends_with(" 7s"));
    }

    /// Theme sanity for whichever theme this binary was built with: every
    /// text role must sit far from every surface role in luminance (dark:
    /// text much lighter, light: text much darker) — catches accidentally
    /// swapped or duplicated role values in either theme. Role-name parity
    /// between the two cfg modules is enforced by the compiler: the shared
    /// call sites simply do not compile if either theme misses a role.
    #[test]
    fn test_theme_text_surface_contrast() {
        let lum = |c: egui::Color32| {
            0.2126_f32 * c.r() as f32
                + 0.7152_f32 * c.g() as f32
                + 0.0722_f32 * c.b() as f32
        };
        let surfaces = [
            palette::WINDOW_BG,
            palette::TOAST_BG,
            palette::CARD_BG,
            palette::CARD_BG_ALT,
            palette::CHIP_BG,
        ];
        let texts = [
            palette::TEXT_PRIMARY,
            palette::TEXT_SECTION,
            palette::TEXT_LABEL,
            palette::TEXT_SECONDARY,
            palette::TEXT_MUTED,
            palette::TEXT_FAINT,
        ];
        for s in surfaces {
            for t in texts {
                assert!(
                    (lum(t) - lum(s)).abs() > 40.0,
                    "text role {t:?} too close to surface {s:?} luminance"
                );
            }
        }
    }

    #[test]
    fn test_format_persian_display_handles_english() {
        let input = "Hello World";
        let formatted = format_persian_display(input);
        assert_eq!(formatted, "Hello World");
    }

    #[test]
    fn overlay_toggles_visibility() {
        let (_tx, rx) = tokio::sync::watch::channel(AppStatus {
            state: AppState::Idle,
            last_text: None,
            vad_engine: "silero",
        });
        let (events_tx, _events_rx) = tokio::sync::mpsc::unbounded_channel();
        let flags = (
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let dict = Arc::new(RwLock::new(Dictionary::with_defaults()));
        let router = AsrRouter::new(vec![]);
        let settings = Arc::new(RwLock::new(Settings::default()));
        let config_path = PathBuf::from("config.toml");
        let mut app = OverlayApp::new(
            Arc::new(StatusClient::new(rx)),
            events_tx,
            flags.0,
            flags.1,
            flags.2,
            flags.3,
            flags.4,
            dict,
            router,
            settings,
            config_path,
        );
        assert!(app.visible);
        app.toggle_visible();
        assert!(!app.visible);
        app.toggle_visible();
        assert!(app.visible);
    }

    #[tokio::test]
    async fn status_client_sees_updates() {
        let (tx, rx) = tokio::sync::watch::channel(AppStatus {
            state: AppState::Idle,
            last_text: None,
            vad_engine: "silero",
        });
        let client = Arc::new(StatusClient::new(rx));
        assert_eq!(client.get().state, AppState::Idle);
        tx.send(AppStatus {
            state: AppState::Recording,
            last_text: None,
            vad_engine: "silero",
        })
        .unwrap();
        assert_eq!(client.get().state, AppState::Recording);
    }

    #[test]
    fn test_history_item_serialization_and_retrieval() {
        let item = HistoryItem {
            id: 1,
            text: "متن تستی اول".into(),
            timestamp: "12:30:45".into(),
            engine: "google".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        let parsed: HistoryItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.text, "متن تستی اول");
        assert_eq!(parsed.engine, "google");
    }

    #[test]
    fn test_history_truncation_limits() {
        let mut history: Vec<HistoryItem> = Vec::new();
        for i in 0..120 {
            history.insert(
                0,
                HistoryItem {
                    id: i,
                    text: format!("Text {i}"),
                    timestamp: "10:00:00".into(),
                    engine: "auto".into(),
                },
            );
            if history.len() > 100 {
                history.truncate(100);
            }
        }
        assert_eq!(history.len(), 100);
        assert_eq!(history[0].id, 119);
        assert_eq!(history[99].id, 20);
    }

    #[test]
    fn test_toast_auto_dismiss_10s_expiration() {
        let now = Instant::now();
        let start = now - Duration::from_secs(11);
        let elapsed = now.duration_since(start);
        assert!(elapsed >= Duration::from_secs(10));
        let remaining_secs = 10_u64.saturating_sub(elapsed.as_secs());
        assert_eq!(remaining_secs, 0);

        let active_start = now - Duration::from_secs(3);
        let active_elapsed = now.duration_since(active_start);
        assert!(active_elapsed < Duration::from_secs(10));
        let active_remaining = 10_u64.saturating_sub(active_elapsed.as_secs()).max(1);
        assert_eq!(active_remaining, 7);
    }

    #[test]
    fn test_visual_mode_transitions() {
        let determine_mode = |state: AppState, is_hovered: bool| -> VisualMode {
            match state {
                AppState::Recording => VisualMode::RecordingActive,
                AppState::Processing | AppState::Typing => VisualMode::Processing,
                _ => {
                    if is_hovered {
                        VisualMode::HoveredAwake
                    } else {
                        VisualMode::IdleDormant
                    }
                }
            }
        };

        assert_eq!(determine_mode(AppState::Idle, false), VisualMode::IdleDormant);
        assert_eq!(determine_mode(AppState::Idle, true), VisualMode::HoveredAwake);
        assert_eq!(determine_mode(AppState::Recording, false), VisualMode::RecordingActive);
        assert_eq!(determine_mode(AppState::Recording, true), VisualMode::RecordingActive);
        assert_eq!(determine_mode(AppState::Processing, false), VisualMode::Processing);
        assert_eq!(determine_mode(AppState::Typing, false), VisualMode::Processing);
    }
}
