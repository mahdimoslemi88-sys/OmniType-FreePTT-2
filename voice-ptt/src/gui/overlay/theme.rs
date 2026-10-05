//! Theme layer for the overlay: the two compiled colour palettes plus the
//! shared manager-window widgets built on top of them.
//!
//! Everything here is a pure function of its arguments — no `self`, no app
//! state. That is precisely what makes it safe to lift out of `overlay.rs`:
//! the whole module can be read, reviewed and restyled without opening the
//! app, and no panel can change a colour by accident.

use eframe::egui;
use egui_phosphor::regular as ic;

use super::text::format_persian_display;

/// OmniType UI palette. Two complete themes behind identical role names,
/// selected at compile time: `dark` (default) and `light`
/// (`cargo build --features light-theme`). Every role is an OPAQUE fill —
/// translucent fills were flattened to their blended-over-parent result
/// (alpha compositing here happens in gamma space, see the theme-parity
/// test) so both themes stay `const`-friendly without premultiplied math.
/// Call sites never branch on the theme; they read role names only.
pub(crate) mod palette {
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

        // Callout surfaces (info / warning banners inside cards)
        pub const CALLOUT_INFO_BG: Color32 = Color32::from_rgb(22, 32, 50);
        pub const CALLOUT_INFO_STROKE: Color32 = Color32::from_rgb(42, 62, 94);
        pub const CALLOUT_WARN_BG: Color32 = Color32::from_rgb(44, 36, 22);
        pub const CALLOUT_WARN_STROKE: Color32 = Color32::from_rgb(82, 64, 32);

        // Data-table header / zebra striping
        pub const TABLE_HEADER_BG: Color32 = Color32::from_rgb(30, 34, 46);
        pub const TABLE_ROW_ALT: Color32 = Color32::from_rgb(24, 27, 38);

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
        #[allow(dead_code)]
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

        // Callout surfaces (info / warning banners inside cards)
        pub const CALLOUT_INFO_BG: Color32 = Color32::from_rgb(226, 238, 250);
        pub const CALLOUT_INFO_STROKE: Color32 = Color32::from_rgb(178, 204, 236);
        pub const CALLOUT_WARN_BG: Color32 = Color32::from_rgb(252, 244, 226);
        pub const CALLOUT_WARN_STROKE: Color32 = Color32::from_rgb(232, 205, 150);

        // Data-table header / zebra striping
        pub const TABLE_HEADER_BG: Color32 = Color32::from_rgb(234, 239, 248);
        pub const TABLE_ROW_ALT: Color32 = Color32::from_rgb(248, 250, 253);

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
/// Shared window chrome for the manager windows: dark central panel with
/// the uniform 14 px margin. Single source so restyling all windows is a
/// one-line change.
pub(crate) fn manager_central_panel() -> egui::Frame {
    egui::Frame::none()
        .fill(palette::WINDOW_BG)
        .inner_margin(egui::Margin::same(14.0))
}

/// Shared content-card frame for the manager windows: rounded 8 px panel,
/// 1 px stroke and the standard 10 px padding — geometry lives here so
/// chrome and cards theme from one source. `fill`/`stroke` stay parameters
/// because they carry meaning (plain vs selected vs hairline variants).
/// Call `.inner_margin(...)` on the result to override padding per card.
pub(crate) fn manager_card(fill: egui::Color32, stroke: egui::Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(8.0))
        .stroke(egui::Stroke::new(1.0_f32, stroke))
        .inner_margin(egui::Margin::same(10.0))
}

/// Shared header row: strong 16 pt title with an optional right-aligned
/// badge pill (`(text, fill, text_color)`, already shaped for RTL display).
pub(crate) fn manager_header(
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
pub(crate) fn manager_subtitle(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(format_persian_display(text))
            .size(11.0)
            .color(palette::TEXT_SECONDARY),
    );
}

/// Masks a secret for safe display (`gsk_...3a1f`): keeps the first 4 and
/// last 4 characters, replacing the middle with an ellipsis. Short or empty
/// secrets collapse to a neutral placeholder so no key is ever shown in full.
#[allow(dead_code)]
pub(crate) fn mask_secret(secret: &str) -> String {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        return "••••".to_string();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().rev().take(4).rev().collect();
    format!("{head}…{tail}")
}

/// Shared transient success banner (`palette::SUCCESS_FILL` pill).
pub(crate) fn success_banner(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(4.0);
    status_chip(
        ui,
        msg,
        palette::SUCCESS_FILL,
        palette::SUCCESS,
        11.0,
        ChipFamily::Tiny,
    );
}

/// Callout severity. Maps to a palette surface/stroke pair and a phosphor
/// glyph, so the three variants stay visually distinct in both themes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CalloutKind {
    Info,
    Warning,
    /// A measurement that came out well. Added for the mic test, where "no
    /// action needed" is a real answer and deserves the same visual weight as
    /// the two unhappy ones — otherwise every good result has to be spelled
    /// out in the body text to avoid being mistaken for a warning.
    Success,
}

impl CalloutKind {
    fn icon(self) -> &'static str {
        match self {
            Self::Info => ic::INFO,
            Self::Warning => ic::WARNING,
            Self::Success => ic::CHECK_CIRCLE,
        }
    }

    fn colors(self) -> (egui::Color32, egui::Color32, egui::Color32) {
        match self {
            Self::Info => (
                palette::CALLOUT_INFO_BG,
                palette::CALLOUT_INFO_STROKE,
                palette::ACCENT_SOFT,
            ),
            Self::Warning => (
                palette::CALLOUT_WARN_BG,
                palette::CALLOUT_WARN_STROKE,
                palette::WARNING,
            ),
            Self::Success => (
                palette::CALLOUT_INFO_BG,
                palette::CALLOUT_INFO_STROKE,
                palette::SUCCESS,
            ),
        }
    }
}

/// Inline callout: an icon + body text inside a tinted, stroked frame.
/// Uses `ui.horizontal` with explicit `.wrap()` on the label instead of
/// `horizontal_wrapped`, because `horizontal_wrapped` in egui 0.28 sets
/// `cursor.max.x = f32::NAN` when wrapping in a `RightToLeft` layout.
pub(crate) fn callout(ui: &mut egui::Ui, kind: CalloutKind, body: &str) {
    let (bg, stroke, icon_color) = kind.colors();
    let icon = kind.icon();
    egui::Frame::none()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0_f32, stroke))
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icon).size(13.0).color(icon_color));
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format_persian_display(body))
                            .size(11.0)
                            .color(palette::TEXT_PRIMARY),
                    )
                    .wrap(),
                );
            });
        });
}

/// Renders a single right-to-left form row without `egui::Grid` (because `Grid`
/// in egui 0.28 sets `cursor.min.x = -INFINITY` on row 0 inside RTL parent
/// layouts, corrupting `max_rect` and panicking during hit-testing).
/// Places the Persian label on the visual right in a fixed-width slot and runs
/// `add_control` immediately to its left so controls align vertically.
pub(crate) fn rtl_form_row(
    ui: &mut egui::Ui,
    label: &str,
    label_width: f32,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, 22.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(label_width);
                ui.label(
                    egui::RichText::new(format_persian_display(label))
                        .size(11.0)
                        .color(palette::TEXT_LABEL),
                );
            },
        );
        add_control(ui);
    });
}

/// Renders a fixed-width cell inside an RTL horizontal table row, enforcing
/// `min_width` so subsequent cells in the row start at a deterministic X offset.
pub(crate) fn rtl_table_cell(
    ui: &mut egui::Ui,
    width: f32,
    layout: egui::Layout,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.allocate_ui_with_layout(egui::vec2(width, 22.0), layout, |ui| {
        ui.set_min_width(width);
        add_contents(ui);
    });
}

/// Geometry families for [`status_chip`].
pub(crate) enum ChipFamily {
    /// Inline micro-chip: 4 px corner, 5×1.5 px margin.
    Small,
    /// Tiny status label: 5 px corner, 6×2 px margin.
    Tiny,
}

/// Aligns a small status chip with `text` (Persian display shaping applied) in
/// `color` on a `bg` rounded pill. Geometry families: `SMALL` (4 px corner,
/// 5×1.5 margin) for inline micro-chips, `TINY` (5 px corner, 6×2 margin) for
/// status labels. Single source so chip restyling is one-line changes.
pub(crate) fn status_chip(
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
pub(crate) fn header_badge(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) {
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
#[allow(dead_code)]
pub(crate) fn capsule_frame(
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
pub(crate) fn apply_theme_visuals(ctx: &egui::Context) {
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
