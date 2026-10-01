//! The tray's warning badge, as pure decisions.
//!
//! Why this module exists: the doctor already knows, at startup, that the
//! configuration is not what the user asked for — `doctor::diagnose` returns a
//! `Verdict` and a list of `problems`. That knowledge used to go exactly one
//! place, a `tracing::warn!` nobody opens. The tray icon is the one surface the
//! app *always* shows and the user *always* sees, so a non-clean verdict has to
//! be visible there, and the fix (reading the report) has to be one click away.
//!
//! Everything here is a value in and a value out: no `TrayIcon`, no menu, no
//! Windows. The runtime half (build the menu item, paint the icon, spawn
//! `cmd /C start`) lives in [`crate::gui::tray`]. That split is what makes the
//! badge testable — a "does the badge actually get drawn?" test cannot touch a
//! real notification area, but it can check the returned RGBA buffer.
//!
//! Two rules the tests below pin down, because both are invisible in a code
//! reading and both would ship as a silent no-op:
//!
//! 1. **Clean means *absent*, not "present and green".** A tray badge that
//!    shows up on every healthy startup teaches the eye to ignore it, which
//!    destroys the only property a warning badge has. So `TrayWarning::new`
//!    returns `None` for [`Verdict::Clean`] and the caller appends nothing.
//! 2. **The badge must survive Windows down-scaling.** The icon is authored at
//!    32×32 and Explorer renders it at 20 or 16 logical pixels. Geometry is
//!    therefore a fraction of the icon's shorter side, not a fixed pixel count.

use std::path::{Path, PathBuf};

use crate::doctor::{Diagnosis, Verdict};

/// Base tooltip shown when nothing is wrong. Kept here so the warning variant
/// can be defined as a suffix of it rather than a second string that drifts.
pub const BASE_TOOLTIP: &str = "OmniType — AI Voice Typing & Industrial Speech Routing";

/// Badge radius as a fraction of the icon's shorter side.
///
/// 0.24 of 32 is 7.7px, which lands at ~3.8px once Explorer scales the icon to
/// 16 — still a legible dot. A fixed 4px radius would vanish entirely.
const BADGE_RADIUS_FRACTION: f32 = 0.24;

/// The dark ring drawn around the coloured dot.
///
/// Without it the badge disappears against a light taskbar (amber on white) and
/// against a dark one (dark red on black) — the two cases that matter. One pixel
/// is enough at 32 and stays visible at 16.
const BADGE_OUTLINE_PX: f32 = 1.0;

/// Colour for a verdict that is merely not what the user asked.
const SUBSTITUTED_COLOR: [u8; 4] = [232, 168, 32, 255];
/// Colour for a verdict where something the user selected will not run at all.
const BROKEN_COLOR: [u8; 4] = [220, 60, 50, 255];
/// The ring colour, near-black and opaque.
const OUTLINE_COLOR: [u8; 4] = [24, 24, 24, 255];

/// Where a click on the tray warning should take the user.
///
/// One target per warning, chosen by the *fact* that is wrong, not by how bad it
/// is. A missing ASR engine is fixable on the Engines tab in two clicks;
/// sending that user to a text file instead is a worse fix for the same badge.
/// Everything else has no page to go to, so the report is the target — and it
/// is the only artefact that survives the app not starting, which is exactly
/// when a missing key or a bad hotkey has to be explained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenTarget {
    /// Raise `Toggle::Engine`; the overlay shows its Engines tab.
    EnginePanel,
    /// Open the diagnostic report with the system's file association.
    Report,
}

/// A startup diagnosis that the tray must surface, plus where to send the user.
///
/// `None` (from [`TrayWarning::new`]) *is* the healthy state — there is no
/// "warning with zero severity" to represent, because the caller's behaviour in
/// that case is "add no menu item, draw no badge, use the base tooltip".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayWarning {
    verdict: Verdict,
    problems: usize,
    active_engine_missing: bool,
    active_engine: String,
    report: PathBuf,
}

impl TrayWarning {
    /// Builds a warning from a diagnosis, or `None` when the diagnosis is clean.
    ///
    /// Takes the whole `Diagnosis` rather than loose arguments because the
    /// fields it reads are easy to transpose: `problems.len()` and `verdict` are
    /// both small and both integers-or-smaller, and a swapped pair compiles
    /// happily and reports the wrong number to the user.
    pub fn new(d: &Diagnosis, report: PathBuf) -> Option<Self> {
        if d.verdict == Verdict::Clean {
            return None;
        }
        Some(Self {
            verdict: d.verdict,
            problems: d.problems.len(),
            active_engine_missing: d.active_engine_missing,
            active_engine: d.active_engine.clone(),
            report,
        })
    }

    /// The verdict that caused this warning.
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// How many individual problems the report lists.
    pub fn problems(&self) -> usize {
        self.problems
    }

    /// The file a click on the menu item opens.
    pub fn report(&self) -> &Path {
        &self.report
    }

    /// Where clicking this warning should go.
    pub fn target(&self) -> OpenTarget {
        if self.active_engine_missing {
            OpenTarget::EnginePanel
        } else {
            OpenTarget::Report
        }
    }

    /// Menu item text, top of the menu, so the fix is the first thing offered.
    ///
    /// The engine case names the engine: "an engine" is not actionable, but
    /// "`groq` is not registered" tells the user which row to look at.
    pub fn menu_label(&self) -> String {
        match self.target() {
            OpenTarget::EnginePanel => format!(
                "⚠ Active Engine '{}' is not registered — every dictation fails (مدیریت موتورها)",
                self.active_engine
            ),
            OpenTarget::Report => format!(
                "⚠ Diagnostic Report — {} {} (گزارش تشخیصی)",
                self.problems,
                self.verdict.label()
            ),
        }
    }

    /// Tooltip for the icon itself. The base text is kept so the tray still
    /// identifies the app; the warning rides at the end where it is read first.
    pub fn tooltip(&self) -> String {
        match self.target() {
            OpenTarget::EnginePanel => format!(
                "{BASE_TOOLTIP} — ⚠ engine '{}' is not registered: every dictation will fail",
                self.active_engine
            ),
            OpenTarget::Report => format!(
                "{BASE_TOOLTIP} — ⚠ {} problem(s): open Diagnostic Report from the menu",
                self.problems
            ),
        }
    }

    /// The dashboard callout, so a user who never looks at the tray still finds
    /// out at boot.
    ///
    /// Plain text, one paragraph, English with the Persian in brackets — the
    /// same shape as the tray labels, so both surfaces read alike. It repeats
    /// the menu item rather than adding new information: the badge says
    /// *something* is wrong, this says *what*, and the two must not disagree.
    pub fn banner_body(&self) -> String {
        match self.target() {
            OpenTarget::EnginePanel => format!(
                "Active engine '{}' is selected but not registered, so every dictation will \
                 fail. Open the Engines tab to fix it. (موتور «{}» ثبت نشده است؛ هر دیکته‌ای \
                 شکست می‌خورد. تب Engines را باز کنید.)",
                self.active_engine, self.active_engine
            ),
            OpenTarget::Report => format!(
                "{} configuration problem(s) were found and some settings were replaced. \
                 Open the diagnostic report from the tray menu. (مشکل پیکربندی و \
                 جایگزینی تنظیمات — گزارش از منوی ترای باز می‌شود.)",
                self.problems
            ),
        }
    }

    /// Paints the badge onto an RGBA buffer, returning it unchanged in length.
    ///
    /// A disc, not a drawn glyph: at 16 logical pixels a "!" would be a grey
    /// smudge, whereas a filled dot is still a dot. Broken is red and
    /// substituted is amber, so severity is readable without hovering.
    pub fn icon(&self, rgba: Vec<u8>, w: u32, h: u32) -> Vec<u8> {
        let mut rgba = rgba;
        apply_badge(&mut rgba, w, h, self.verdict);
        rgba
    }
}

/// Where the badge sits, in pixels, for an icon of `w`×`h`.
///
/// The disc is *fully inside* the buffer (centre + radius == size - 1). An
/// earlier instinct was to bleed it off the corner for a modern look; that
/// costs the bottom-right arc exactly where the alpha edge is hardest to see
/// against a taskbar, and buys nothing at this size.
fn badge_geometry(w: u32, h: u32) -> (f32, f32, f32) {
    let shorter = w.min(h) as f32;
    let radius = (shorter * BADGE_RADIUS_FRACTION).max(2.0);
    let cx = (w as f32 - 1.0) - radius;
    let cy = (h as f32 - 1.0) - radius;
    (cx, cy, radius)
}

/// Composites the warning dot onto a top-to-bottom RGBA buffer.
///
/// Writes only inside the badge disc and never changes the buffer length, so
/// `Icon::from_rgba` keeps receiving the geometry it was given.
pub fn apply_badge(rgba: &mut [u8], w: u32, h: u32, verdict: Verdict) {
    let (cx, cy, radius) = badge_geometry(w, h);
    let fill_radius = (radius - BADGE_OUTLINE_PX).max(1.0);
    let fill = match verdict {
        Verdict::Clean => return, // nothing to warn about; leave the icon alone
        Verdict::Substituted => SUBSTITUTED_COLOR,
        Verdict::Broken => BROKEN_COLOR,
    };
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let colour = if dist <= fill_radius {
                fill
            } else if dist <= radius {
                OUTLINE_COLOR
            } else {
                continue;
            };
            let idx = ((y * w + x) * 4) as usize;
            if idx + 4 <= rgba.len() {
                rgba[idx..idx + 4].copy_from_slice(&colour);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::KeySource;

    fn report() -> PathBuf {
        PathBuf::from("doctor-report.txt")
    }

    /// A diagnosis built by hand rather than through `diagnose`.
    ///
    /// The point is deliberate: it can produce combinations `diagnose` never
    /// would (`Broken` without a missing engine, a missing engine without
    /// `Broken`). Those are exactly the combinations that expose a decision
    /// quietly reading the wrong field — a test that can only build consistent
    /// input cannot tell "reads the fact" from "reads the label".
    fn diag(verdict: Verdict, problems: usize, engine_missing: bool) -> Diagnosis {
        Diagnosis {
            verdict,
            version: "0.0.0-test",
            config_path: "C:/x/config.toml".into(),
            models_dir: "C:/x/models".into(),
            assets_dir: "C:/x/assets".into(),
            problems: (0..problems).map(|i| format!("problem {i}")).collect(),
            hotkeys: Vec::new(),
            engines: vec!["local_whisper".into()],
            active_engine: "groq".into(),
            active_engine_missing: engine_missing,
            cloud_key_source: KeySource::ConfigFile,
        }
    }

    fn warning(verdict: Verdict, problems: usize, engine_missing: bool) -> TrayWarning {
        TrayWarning::new(&diag(verdict, problems, engine_missing), report())
            .expect("a non-clean diagnosis is a warning")
    }

    fn transparent(size: u32) -> Vec<u8> {
        vec![0u8; (size * size * 4) as usize]
    }

    #[test]
    fn a_clean_diagnosis_produces_no_warning() {
        assert!(TrayWarning::new(&diag(Verdict::Clean, 0, false), report()).is_none());
        // Even a clean verdict that somehow carries problems stays silent:
        // the verdict is the authority, not the count.
        assert!(TrayWarning::new(&diag(Verdict::Clean, 3, false), report()).is_none());
    }

    #[test]
    fn a_non_clean_diagnosis_produces_one_warning() {
        let w = warning(Verdict::Substituted, 2, false);
        assert_eq!(w.verdict(), Verdict::Substituted);
        assert_eq!(w.problems(), 2);
        assert_eq!(w.report(), report());
        assert!(TrayWarning::new(&diag(Verdict::Broken, 1, true), report()).is_some());
    }

    #[test]
    fn a_missing_engine_sends_the_user_to_the_engine_panel() {
        let w = warning(Verdict::Broken, 1, true);
        assert_eq!(w.target(), OpenTarget::EnginePanel);
    }

    #[test]
    fn every_other_problem_sends_the_user_to_the_report() {
        assert_eq!(
            warning(Verdict::Substituted, 1, false).target(),
            OpenTarget::Report
        );
    }

    /// The decision must follow the *fact*, not the severity label. If a later
    /// change makes `Broken` mean something else, the tray must keep sending
    /// non-engine problems to the report rather than to a page that cannot fix
    /// them — this test fails on the day that happens instead of on the day a
    /// user cannot find their API key.
    #[test]
    fn the_target_ignores_the_severity_label() {
        // Broken without a missing engine: still the report.
        assert_eq!(
            warning(Verdict::Broken, 1, false).target(),
            OpenTarget::Report
        );
        // A missing engine without the Broken label: still the engine panel.
        assert_eq!(
            warning(Verdict::Substituted, 1, true).target(),
            OpenTarget::EnginePanel
        );
    }

    #[test]
    fn the_engine_menu_item_names_the_engine_and_the_consequence() {
        let label = warning(Verdict::Broken, 1, true).menu_label();
        assert!(
            label.contains("groq"),
            "user cannot tell which engine: {label}"
        );
        assert!(
            label.contains("not registered"),
            "user cannot tell what is wrong: {label}"
        );
        assert!(
            !label.contains("Diagnostic Report"),
            "the item promises a file it no longer opens: {label}"
        );
    }

    #[test]
    fn the_report_menu_item_carries_the_count_and_the_severity() {
        let label = warning(Verdict::Broken, 4, false).menu_label();
        assert!(label.contains('4'), "count missing from {label}");
        assert!(label.contains("BROKEN"), "severity missing from {label}");
        assert!(
            label.contains("Diagnostic Report"),
            "user cannot tell what clicking does: {label}"
        );
    }

    #[test]
    fn the_tooltip_says_which_fix_the_click_offers() {
        let engine = warning(Verdict::Broken, 1, true).tooltip();
        assert!(
            engine.starts_with(BASE_TOOLTIP),
            "app identity lost: {engine}"
        );
        assert!(
            engine.contains("groq"),
            "tooltip does not name the engine: {engine}"
        );
        assert!(
            !engine.contains('\n'),
            "tooltip must stay one line: {engine}"
        );

        let text = warning(Verdict::Substituted, 2, false).tooltip();
        assert!(
            text.starts_with(BASE_TOOLTIP),
            "app identity lost on the report path too: {text}"
        );
        assert!(text.contains('2'), "count missing from {text}");
        assert!(!text.contains('\n'), "tooltip must stay one line: {text}");
    }

    #[test]
    fn the_banner_says_what_is_wrong_and_where_to_go() {
        let engine = warning(Verdict::Broken, 1, true).banner_body();
        assert!(
            engine.contains("groq"),
            "banner does not name the engine: {engine}"
        );
        assert!(
            engine.contains("Engines"),
            "the engine problem must point at the Engines tab: {engine}"
        );

        let report = warning(Verdict::Substituted, 2, false).banner_body();
        assert!(report.contains('2'), "banner does not count: {report}");
        assert!(
            report.contains("tray"),
            "the report problem must say where the report is: {report}"
        );
    }

    /// The badge, the menu item and the callout all describe the same state. If
    /// the banner said something different, a user would act on the wrong one.
    #[test]
    fn the_banner_agrees_with_the_menu_item() {
        let w = warning(Verdict::Broken, 1, true);
        let banner = w.banner_body();
        assert!(
            banner.contains("not registered") && w.menu_label().contains("not registered"),
            "the two surfaces describe different problems:\n{}\n{}",
            w.menu_label(),
            banner
        );
    }

    #[test]
    fn broken_is_red_and_substituted_is_amber() {
        let broken = warning(Verdict::Broken, 1, true).icon(transparent(32), 32, 32);
        let substituted = warning(Verdict::Substituted, 1, false).icon(transparent(32), 32, 32);
        assert_ne!(
            broken, substituted,
            "severity is not readable from the icon"
        );
        let (cx, cy, _) = badge_geometry(32, 32);
        let idx = ((cy as u32 * 32 + cx as u32) * 4) as usize;
        assert_eq!(&broken[idx..idx + 4], &BROKEN_COLOR);
        assert_eq!(&substituted[idx..idx + 4], &SUBSTITUTED_COLOR);
    }

    #[test]
    fn a_clean_verdict_paints_nothing_at_all() {
        let before = transparent(32);
        let mut after = before.clone();
        apply_badge(&mut after, 32, 32, Verdict::Clean);
        assert_eq!(before, after, "a healthy startup must not touch the icon");
    }

    #[test]
    fn the_badge_is_fully_inside_the_icon() {
        let (cx, cy, radius) = badge_geometry(32, 32);
        assert!(cx - radius >= 0.0, "badge is clipped on the left");
        assert!(cy - radius >= 0.0, "badge is clipped on the top");
        assert!(
            cx + radius <= 31.0 && cy + radius <= 31.0,
            "badge is clipped on the bottom-right: {} {}",
            cx + radius,
            cy + radius
        );
    }

    #[test]
    fn the_badge_lands_in_the_bottom_right_and_spares_the_rest() {
        let before = transparent(32);
        let after = warning(Verdict::Broken, 1, true).icon(before.clone(), 32, 32);
        assert_eq!(after.len(), before.len(), "icon geometry must not change");

        let mut painted = 0usize;
        for y in 0..32u32 {
            for x in 0..32u32 {
                let idx = ((y * 32 + x) * 4) as usize;
                if after[idx..idx + 4] == before[idx..idx + 4] {
                    continue;
                }
                painted += 1;
                assert!(
                    x > 15 && y > 15,
                    "pixel ({x},{y}) changed outside the bottom-right quadrant"
                );
            }
        }
        assert!(painted > 20, "badge too small to see: {painted} px");
        assert!(
            painted < 32 * 32 / 2,
            "badge swallowed the icon: {painted} px"
        );
    }

    #[test]
    fn the_badge_survives_explorer_downscaling_to_16px() {
        // Explorer renders the 32px icon at 16 logical pixels on small taskbars.
        // A fixed-radius dot would land at ~1.5px there; a fraction survives.
        let (_, _, r16) = badge_geometry(16, 16);
        assert!(
            r16 >= 3.0,
            "badge shrinks to {r16}px radius at 16px — it would vanish"
        );
        let after = warning(Verdict::Broken, 1, true).icon(transparent(16), 16, 16);
        let opaque = after.chunks(4).filter(|p| p[3] > 0).count();
        assert!(opaque >= 6, "only {opaque} px visible at 16px");
    }

    #[test]
    fn a_short_buffer_is_not_overrun() {
        // `Icon::from_rgba` would reject a mismatched buffer, but a future
        // caller must not be able to panic the tray thread with a slice bug.
        let mut tiny = vec![0u8; 10];
        apply_badge(&mut tiny, 32, 32, Verdict::Broken);
        assert_eq!(tiny.len(), 10);
    }
}
