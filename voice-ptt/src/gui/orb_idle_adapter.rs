//! Turning the desktop into something the idle policy can be asked about.
//!
//! [`crate::gui::orb_idle_policy`] is pure on purpose: it knows about corners
//! and monitors as *names*, and it never asks the operating system anything.
//! This file is the adapter that does the asking, and it is deliberately the
//! only place where a monitor rectangle, an orb radius and a policy corner meet.
//!
//! Two things it refuses to do:
//!
//! * **It does not pick a spot when the monitor cannot be resolved.** A guess
//!   based on the virtual desktop would put the orb somewhere the user never
//!   asked for and cannot predict — and the policy's contract says a missing
//!   target is `TargetUnavailable`, not "somewhere plausible".
//! * **It does not choose a corner.** `MonitorChoice` and `Corner` are the
//!   user's settings; the adapter reads them and applies them, and decides
//!   nothing on its own.

use eframe::egui::Pos2;

use crate::gui::orb_idle_policy::{Corner, IdleReturnSettings, MonitorChoice, SpotTarget};

/// The waiting spot for the current settings, in physical screen pixels.
///
/// `None` means "there is no spot to ask about" — no resolvable monitor, or a
/// work area too small to hold the orb. Both are reported as
/// [`crate::gui::orb_idle_policy::NoReturnReason::TargetUnavailable`] by the
/// policy, which is the honest answer: the orb stays where the user left it.
pub fn waiting_spot(
    settings: &IdleReturnSettings,
    orb_center_px: Pos2,
    ppp: f32,
) -> Option<SpotTarget> {
    let monitor = resolve_monitor(settings.monitor, orb_center_px)?;
    let (left, top, right, bottom) = monitor.rect;

    // Asked through the orb rather than recomputed: the orb clamps itself to
    // this exact reach, so a spot derived from a smaller one would be pushed
    // away by the clamp every single time, which would present as a return
    // that never works. No extra inset on top — the orb's own
    // `EDGE_MARGIN_PT` is the designed visual margin, and adding a second one
    // here would be a second number that has to be kept in step with it.
    let inset = crate::gui::orb::idle_half_reach_px(ppp);
    let min_x = left as f32 + inset;
    let max_x = right as f32 - inset;
    let min_y = top as f32 + inset;
    let max_y = bottom as f32 - inset;
    if min_x > max_x || min_y > max_y {
        return None;
    }

    let x = match settings.corner {
        Corner::TopLeft | Corner::BottomLeft => min_x,
        Corner::TopRight | Corner::BottomRight => max_x,
    };
    let y = match settings.corner {
        Corner::TopLeft | Corner::TopRight => min_y,
        Corner::BottomLeft | Corner::BottomRight => max_y,
    };

    Some(SpotTarget::new(
        x.round() as i32,
        y.round() as i32,
        geometry_version(&monitor),
    ))
}

/// Picks the monitor the waiting spot is computed on.
///
/// `FollowOrb` follows whichever monitor the orb is currently on, so a user who
/// dragged the orb to a second display gets *that* display's corner rather than
/// being yanked back to the first one.
fn resolve_monitor(
    choice: MonitorChoice,
    orb_center_px: Pos2,
) -> Option<crate::gui::window_shape::MonitorWorkArea> {
    match choice {
        // `MonitorFromPoint` with `MONITOR_DEFAULTTONEAREST` means the orb need
        // not be inside the monitor for this to resolve: the nearest one wins,
        // which is the behaviour a user dragging towards an edge expects.
        MonitorChoice::FollowOrb => crate::gui::window_shape::monitor_work_area(
            orb_center_px.x.round() as i32,
            orb_center_px.y.round() as i32,
        ),
        MonitorChoice::Primary => {
            let found = crate::gui::window_shape::primary_monitor_work_area()?;
            // Asking for the primary and being handed something that is not
            // marked primary means the enumeration and the request disagree;
            // trusting the name would move the orb to the wrong screen.
            found.is_primary.then_some(found)
        }
        MonitorChoice::Fixed(wanted) => {
            crate::gui::window_shape::all_monitor_work_areas()
                .get(wanted as usize)
                .copied()
        }
    }
}

/// A number that changes when, and only when, the geometry the spot was
/// computed from changed.
///
/// This is the policy's "the world moved under me" signal. Folding the work
/// area, the monitor count and the primary flag into it means a resolution
/// change, a monitor unplug or a taskbar move all invalidate an outstanding
/// request — while a mouse moving across the screen, which changes nothing
/// about where the spot is, does not.
fn geometry_version(monitor: &crate::gui::window_shape::MonitorWorkArea) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x100_0000_01b3;

    let mut hash = FNV_OFFSET;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(FNV_PRIME);
    };
    for edge in [
        monitor.rect.0,
        monitor.rect.1,
        monitor.rect.2,
        monitor.rect.3,
    ] {
        mix(edge as u64);
    }
    mix(monitor.index as u64);
    mix(monitor.is_primary as u64);
    mix(crate::gui::window_shape::monitor_count() as u64);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn settings(corner: Corner, monitor: MonitorChoice) -> IdleReturnSettings {
        IdleReturnSettings {
            enabled: true,
            timeout: Duration::from_secs(60),
            retry_delay: Duration::from_secs(5),
            pinned: false,
            corner,
            monitor,
        }
    }

    /// Whatever the machine has attached, a spot either resolves or it does
    /// not — and asking twice for the same answer must give the same answer.
    /// Without a second monitor this is the honest "no spot" path, which the
    /// policy has a reason for.
    #[test]
    fn a_missing_or_unresolvable_monitor_yields_no_spot() {
        let s = settings(Corner::TopRight, MonitorChoice::Fixed(200));
        assert!(
            waiting_spot(&s, Pos2::new(100.0, 100.0), 1.0).is_none(),
            "a monitor index that cannot exist must produce no spot"
        );
    }

    /// The stamp has to be stable within one geometry and change when the
    /// geometry changes; that is the whole invalidation contract.
    #[test]
    fn the_geometry_stamp_is_stable_and_geometry_sensitive() {
        use crate::gui::window_shape::MonitorWorkArea;
        let monitor = |rect, index, is_primary| MonitorWorkArea {
            rect,
            index,
            is_primary,
        };
        let a = geometry_version(&monitor((0, 0, 1920, 1080), 0, true));
        let b = geometry_version(&monitor((0, 0, 1920, 1080), 0, true));
        assert_eq!(a, b, "the same geometry must stamp the same");

        let moved_taskbar = geometry_version(&monitor((0, 0, 1920, 1040), 0, true));
        assert_ne!(a, moved_taskbar, "a taskbar move must change the stamp");

        let second_monitor = geometry_version(&monitor((0, 0, 1920, 1080), 1, false));
        assert_ne!(a, second_monitor, "a different monitor must change it too");
    }

    /// The user's corner must actually decide where the orb goes.
    ///
    /// The corner used to be hardcoded to top-right at the one call site, so the
    /// orb always walked *up* — including on a machine whose taskbar is at the
    /// bottom, where the top corner is the one that is not out of the way. All
    /// four are checked against the real monitor this machine actually has, at
    /// the same points-per-pixel the app uses.
    ///
    /// It skips honestly when there is no primary monitor rather than asserting
    /// against invented geometry: a test that passes because it compared made-up
    /// numbers proves nothing about the code.
    #[test]
    fn each_corner_picks_its_own_edge_of_the_work_area() {
        let ppp = 1.25f32;
        let Some(monitor) = crate::gui::window_shape::primary_monitor_work_area() else {
            return;
        };
        let (left, top, right, bottom) = monitor.rect;
        let inset = crate::gui::orb::idle_half_reach_px(ppp);
        let min_x = left as f32 + inset;
        let max_x = right as f32 - inset;
        let min_y = top as f32 + inset;
        let max_y = bottom as f32 - inset;
        assert!(
            min_x <= max_x && min_y <= max_y,
            "the primary monitor's work area is too small for the orb"
        );

        // The orb starts in the middle, so `FollowOrb` and `Primary` agree and
        // the corner is the only thing that can move the answer.
        let centre = Pos2::new((left + right) as f32 / 2.0, (top + bottom) as f32 / 2.0);
        let near = |a: f32, b: f32| (a - b).abs() < 1.0;
        for (corner, want_x, want_y) in [
            (Corner::TopLeft, min_x, min_y),
            (Corner::TopRight, max_x, min_y),
            (Corner::BottomLeft, min_x, max_y),
            (Corner::BottomRight, max_x, max_y),
        ] {
            let spot = waiting_spot(&settings(corner, MonitorChoice::Primary), centre, ppp)
                .unwrap_or_else(|| panic!("{corner:?} produced no spot"));
            assert!(
                near(spot.point.x as f32, want_x) && near(spot.point.y as f32, want_y),
                "{corner:?} landed at ({}, {}), expected ({want_x}, {want_y})",
                spot.point.x,
                spot.point.y
            );
        }
    }

    /// The setting the user picks must survive the whole trip: config value →
    /// `IdleReturnSettings` → the spot that comes out.
    ///
    /// This is the seam that broke. The corner was read correctly out of the
    /// config and then overwritten with a constant at the one call site, so the
    /// setting existed, was saved, and did nothing at all.
    #[test]
    fn the_configured_corner_reaches_the_idle_return_settings() {
        let mut gui = crate::config::settings::Settings::default().gui;
        for (text, expected) in [
            ("top_left", Corner::TopLeft),
            ("bottom_left", Corner::BottomLeft),
            ("bottom_right", Corner::BottomRight),
            ("top_right", Corner::TopRight),
        ] {
            gui.orb_return_corner = text.to_string();
            let settings = crate::gui::overlay::idle_settings_from(&gui);
            assert_eq!(
                settings.corner, expected,
                "{text} must survive into the idle-return settings"
            );
        }
    }

    /// Two lookups in a row must not be able to disagree, or the policy would
    /// see a phantom "target changed" every other frame and never return.
    #[test]
    fn two_lookups_in_a_row_agree() {
        let s = settings(Corner::BottomLeft, MonitorChoice::Primary);
        let first = waiting_spot(&s, Pos2::new(400.0, 300.0), 1.0);
        let second = waiting_spot(&s, Pos2::new(400.0, 300.0), 1.0);
        match (first, second) {
            (Some(a), Some(b)) => assert_eq!(a, b, "the spot must be stable"),
            (None, None) => {}
            _ => panic!("one lookup found a spot and the other did not"),
        }
    }
}
