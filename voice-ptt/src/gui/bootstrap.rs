//! One-shot GUI setup: everything that happens once, before the event loop.
//!
//! This block used to live inline in `run()` (`lib.rs`), where 145 lines of
//! window geometry, font loading and dependency wiring sat in a function with
//! no test at all. Splitting it buys two different things:
//!
//! * the *arithmetic and configuration* (where the orb lands on a fresh
//!   install, which window flags must never change, which fonts must load)
//!   moves into pure functions that a unit test can pin, so a future edit
//!   cannot silently move the orb or re-enable the title bar;
//! * the *wiring* stays in one `run_gui` call that takes a single named
//!   struct instead of ten positional arguments — the same swap-hazard that
//!   `DashboardFlags` was created to remove, one level up.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::asr::router::AsrRouter;
use crate::config::{GuiSettings, Settings};
use crate::gui::flags::DashboardFlags;
use crate::gui::overlay::OverlayApp;
use crate::hotkey::{HotkeyControl, HotkeyEvent};
use crate::processing::Dictionary;
use crate::state::AppStatus;
use crate::updates::SharedUpdateState;

/// Everything the overlay needs in order to exist.
///
/// Grouped because `OverlayApp::new` takes nine arguments and most of them are
/// opaque types (`Arc<RwLock<Dictionary>>`, `AsrRouter`, `HotkeyControl`): with
/// positional parameters, swapping the dictionary and the router is a perfect
/// compile and a wrong app at runtime. Named fields make that a type error.
pub struct GuiStartup {
    /// Immutable snapshot taken at startup; the overlay reads only startup
    /// preferences (`show_overlay`, orb position) from this.
    pub settings: Arc<Settings>,
    /// The live, user-writable settings the dashboard edits.
    pub settings_rwlock: Arc<RwLock<Settings>>,
    pub config_path: PathBuf,
    pub flags: DashboardFlags,
    pub events_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
    pub update_state: SharedUpdateState,
    pub hotkey_control: HotkeyControl,
    /// The startup diagnosis, or `None` when the configuration was clean. Both
    /// the tray icon and the dashboard callout read the *same* value, so the two
    /// cannot end up describing different problems.
    pub boot_warning: Option<crate::gui::tray_warning::TrayWarning>,
    pub dictionary: Arc<RwLock<Dictionary>>,
    pub router: AsrRouter,
    pub status: tokio::sync::watch::Receiver<AppStatus>,
    /// The microphone gate, built by the state machine that owns the status
    /// channel it reads. Passed in rather than rebuilt here so the dashboard and
    /// the recorder cannot consult two different truths about whether a
    /// dictation is live.
    pub mic_gate: Arc<crate::audio::gate::LiveMicGate>,
    /// The review/recovery wire, built by the state machine that owns the loop
    /// which resolves answers. Passed in for the same reason as `mic_gate`: the
    /// review window and the loop must be looking at the same drafts.
    pub review: Arc<crate::state::ReviewChannel>,
}

/// The orb center the user last dragged to, or `None` on a first run.
///
/// Both coordinates must be present: a half-saved position (x set, y lost to
/// a truncated hand-edited `config.toml`) falls back to the screen center
/// rather than parking the orb at y=0. Zipping both is what makes that rule
/// atomic — matching on the pair, as this used to, hides the same behaviour
/// inside a tuple pattern nobody tests.
pub fn saved_orb_center(gui: &GuiSettings) -> Option<(i32, i32)> {
    gui.orb_position_x.zip(gui.orb_position_y)
}

/// Top-left corner of the square orb window.
///
/// `saved_center` is the orb *center* in physical screen pixels; the window's
/// top-left is half a side up and to the left of it, because the orb is drawn
/// centered in its own canvas.
pub fn orb_window_origin(
    saved_center: Option<(i32, i32)>,
    screen: (f32, f32),
    side: f32,
) -> [f32; 2] {
    let (cx, cy) = saved_center
        .map(|(x, y)| (x as f32, y as f32))
        .unwrap_or((screen.0 * 0.5, screen.1 * 0.5));
    [cx - side * 0.5, cy - side * 0.5]
}

/// Physical size of the primary display. Windows-only in practice; the
/// non-Windows branch exists so the geometry stays testable off-Windows.
fn primary_screen_size() -> (f32, f32) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        unsafe {
            (
                GetSystemMetrics(SM_CXSCREEN) as f32,
                GetSystemMetrics(SM_CYSCREEN) as f32,
            )
        }
    }
    #[cfg(not(windows))]
    {
        (1920.0, 1080.0)
    }
}

/// Zero-flash launch: a generously sized, fully transparent, undecorated,
/// always-on-top window at the orb's last position.
///
/// The side is the orb's *fixed* canvas (the largest it ever draws at), not the
/// idle one. The window is created once at this size and never resized,
/// because resizing a transparent always-on-top window strands the pixels the
/// old rect covered — see `Orb::max_canvas_points` and the ghost-aura report
/// in `docs/GUI-WINDOW-ARTIFACT-REPORT.md`.
pub fn native_options(settings: &Settings) -> eframe::NativeOptions {
    let side = crate::gui::orb::Orb::initial_side_points();
    let origin = orb_window_origin(saved_orb_center(&settings.gui), primary_screen_size(), side);
    let (icon_rgba, icon_w, icon_h) = crate::gui::tray::app_icon_rgba();

    eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_visible(settings.gui.show_overlay)
            .with_position(origin)
            .with_inner_size([side, side])
            .with_title("")
            .with_icon(Arc::new(egui::IconData {
                rgba: icon_rgba,
                width: icon_w,
                height: icon_h,
            })),
        ..Default::default()
    }
}

/// Native Windows fonts with full Persian/Arabic glyph coverage, plus the
/// Phosphor icon font.
///
/// Phosphor is appended to the *same* definitions so it joins the fallback
/// chain after Segoe UI: the PUA icon codepoints resolve to it, while Persian
/// coverage keeps coming from Segoe UI. Loading a missing file is not an error
/// — a machine without Tahoma should still start, just with fewer glyphs.
pub fn ui_fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for (name, path, order) in [
        ("segoe_ui", r"C:\Windows\Fonts\segoeui.ttf", Slot::First),
        ("tahoma", r"C:\Windows\Fonts\tahoma.ttf", Slot::Last),
        (
            "segoe_ui_symbol",
            r"C:\Windows\Fonts\seguisym.ttf",
            Slot::Last,
        ),
    ] {
        if let Ok(data) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.to_owned(), egui::FontData::from_owned(data));
            let family = fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default();
            match order {
                Slot::First => family.insert(0, name.to_owned()),
                Slot::Last => family.push(name.to_owned()),
            };
        } else {
            tracing::debug!(font = name, "system font unavailable; skipping");
        }
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    fonts
}

/// Where a fallback font lands in the proportional chain.
enum Slot {
    First,
    Last,
}

/// Dark theme with every fill forced transparent, so the wgpu surface stays a
/// see-through hole in the desktop instead of painting an opaque panel behind
/// the orb. `extreme_bg_color` matters too: egui uses it for the frame
/// background, which would otherwise flash dark on the first frame.
pub fn ui_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::TRANSPARENT;
    visuals.window_fill = egui::Color32::TRANSPARENT;
    visuals.extreme_bg_color = egui::Color32::TRANSPARENT;
    visuals
}

/// Builds the native window and runs the eframe event loop on the main thread.
/// Blocks until the overlay closes.
pub fn run_gui(startup: GuiStartup) -> Result<()> {
    let GuiStartup {
        settings,
        settings_rwlock,
        config_path,
        flags,
        events_tx,
        update_state,
        hotkey_control,
        dictionary,
        router,
        status,
        boot_warning,
        mic_gate,
        review,
    } = startup;

    let native_options = native_options(&settings);
    let status = Arc::new(crate::gui::overlay::StatusClient::new(status));

    eframe::run_native(
        "voice-ptt",
        native_options,
        Box::new(move |cc| {
            cc.egui_ctx.set_fonts(ui_fonts());
            cc.egui_ctx.set_visuals(ui_visuals());
            Ok(Box::new(OverlayApp::new(
                status.clone(),
                events_tx,
                flags,
                dictionary,
                router,
                settings_rwlock,
                config_path,
                update_state,
                Some(hotkey_control),
                boot_warning,
                mic_gate,
                review,
            )) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow!("GUI failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui_with_position(x: Option<i32>, y: Option<i32>) -> GuiSettings {
        GuiSettings {
            orb_position_x: x,
            orb_position_y: y,
            ..Default::default()
        }
    }

    #[test]
    fn a_first_run_orb_lands_in_the_middle_of_the_screen() {
        let origin = orb_window_origin(None, (1920.0, 1080.0), 200.0);
        assert_eq!(origin, [960.0 - 100.0, 540.0 - 100.0]);
    }

    /// The stored value is the orb *center*, so the window must be pulled back
    /// by half a side. Getting this backwards parks the orb off-screen by a
    /// full side and was invisible to tests while it lived in `run()`.
    #[test]
    fn a_saved_center_becomes_the_windows_top_left_corner() {
        let origin = orb_window_origin(Some((1600, 400)), (1920.0, 1080.0), 200.0);
        assert_eq!(origin, [1500.0, 300.0]);
    }

    /// A half-written position is not a position. Parking the orb at y=0
    /// (off the top of the screen, unreachable because only Idle/Error are
    /// draggable) would be a silent trap.
    #[test]
    fn a_half_saved_position_is_ignored() {
        let screen = (1920.0, 1080.0);
        let side = 200.0;
        assert_eq!(saved_orb_center(&gui_with_position(Some(1600), None)), None);
        assert_eq!(
            orb_window_origin(
                saved_orb_center(&gui_with_position(Some(1600), None)),
                screen,
                side
            ),
            orb_window_origin(None, screen, side)
        );
    }

    #[test]
    fn a_whole_saved_position_survives_both_coordinates() {
        assert_eq!(
            saved_orb_center(&gui_with_position(Some(1600), Some(400))),
            Some((1600, 400))
        );
    }

    /// The zero-flash contract, in one test: square, fixed at the orb's fixed
    /// canvas, transparent, undecorated, unresizable and always on top. Each of
    /// these was a deliberate fix for a visible artifact; all of them are
    /// invisible to the type checker.
    #[test]
    fn the_window_is_a_fixed_transparent_square() {
        let settings = Settings::default();
        let side = crate::gui::orb::Orb::initial_side_points();
        let vp = native_options(&settings).viewport;

        assert_eq!(vp.inner_size, Some(egui::vec2(side, side)));
        assert_eq!(vp.transparent, Some(true));
        assert_eq!(vp.decorations, Some(false));
        assert_eq!(vp.resizable, Some(false));
        assert_eq!(vp.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(vp.title.as_deref(), Some(""));
    }

    /// Closes the loop between the geometry helper and the window: the origin
    /// has to actually reach the viewport. Found by mutation — flipping the
    /// half-side offset failed the geometry tests but left the window tests
    /// green, because nothing asserted that `orb_window_origin` is the thing
    /// being handed to `with_position`.
    #[test]
    fn the_window_opens_where_the_orb_was_left() {
        let settings = Settings {
            gui: gui_with_position(Some(1600), Some(400)),
            ..Default::default()
        };
        let side = crate::gui::orb::Orb::initial_side_points();
        let expected = orb_window_origin(Some((1600, 400)), primary_screen_size(), side);
        assert_eq!(
            native_options(&settings).viewport.position,
            Some(egui::pos2(expected[0], expected[1])),
            "the viewport must be positioned by orb_window_origin, not by a second copy of the math"
        );
    }

    #[test]
    fn a_hidden_overlay_starts_invisible() {
        let settings = Settings {
            gui: GuiSettings {
                show_overlay: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(native_options(&settings).viewport.visible, Some(false));
    }

    #[test]
    fn a_visible_overlay_starts_visible() {
        let settings = Settings::default();
        assert_eq!(native_options(&settings).viewport.visible, Some(true));
    }

    /// Segoe UI must come first or Persian shaping is picked from Tahoma's
    /// fallback instead, which changes the glyph advance widths and jitters
    /// the orb label on every redraw.
    #[test]
    fn persian_and_icon_glyphs_share_one_fallback_chain() {
        let fonts = ui_fonts();
        let proportional = &fonts.families[&egui::FontFamily::Proportional];
        assert_eq!(proportional.first().map(String::as_str), Some("segoe_ui"));

        // egui_phosphor appends its family; on Windows every entry above
        // resolved, so the chain ends with the icon font.
        if cfg!(windows) {
            for expected in ["tahoma", "segoe_ui_symbol"] {
                assert!(
                    proportional.iter().any(|f| f == expected),
                    "{expected} missing from {proportional:?}"
                );
            }
            assert!(
                proportional.last().is_some_and(|f| f != "tahoma"),
                "phosphor icons must be appended last, got {proportional:?}"
            );
        }
    }

    /// A missing system font must not stop the app from starting; the chain
    /// simply gets shorter.
    #[test]
    fn a_missing_font_file_leaves_the_chain_shorter_not_broken() {
        let fonts = ui_fonts();
        let proportional = &fonts.families[&egui::FontFamily::Proportional];
        assert!(!proportional.is_empty());
        for name in proportional {
            assert!(!name.is_empty(), "an unnamed font in {proportional:?}");
        }
    }

    /// The transparency is the whole reason the orb has no dark box around it.
    #[test]
    fn the_overlay_paints_nothing_behind_itself() {
        let visuals = ui_visuals();
        assert_eq!(visuals.panel_fill, egui::Color32::TRANSPARENT);
        assert_eq!(visuals.window_fill, egui::Color32::TRANSPARENT);
        assert_eq!(visuals.extreme_bg_color, egui::Color32::TRANSPARENT);
        // Transparent, but still a dark *theme*: egui derives text contrast
        // from dark_mode, so flipping it would silently grey out every label.
        assert!(visuals.dark_mode);
    }
}
