#!/usr/bin/env bash
# Canary check for the tray warning badge and its click destination: mutate one
# decision at a time and record which tests notice. A mutation that leaves the
# suite green is a hole in the tests, not a pass.
#
# Run from v-2/voice-ptt:  bash ../docs/mutation-check-tray-warning.sh
set -u

# shellcheck source=docs/canary-harness.sh
source "$(dirname "$0")/canary-harness.sh"

W=src/gui/tray_warning.rs
U=src/updates.rs
D=src/doctor.rs
H=src/hotkey/diagnostics.rs

canary_init "$W" "$U" "$D" "$H"

echo "=== canaries: mutate one decision, expect red ==="

mutate "$W" \
  '        if d.verdict == Verdict::Clean {
            return None;
        }' \
  '' \
  'C1 a clean diagnosis still raises a warning badge'

mutate "$W" \
  '    let radius = (shorter * BADGE_RADIUS_FRACTION).max(2.0);' \
  '    let radius = 2.0;' \
  'C2 the badge uses a fixed radius instead of a fraction'

mutate "$W" \
  '    let cx = (w as f32 - 1.0) - radius;
    let cy = (h as f32 - 1.0) - radius;' \
  '    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;' \
  'C3 the badge sits in the centre instead of the corner'

mutate "$W" \
  '    let (cx, cy, radius) = badge_geometry(w, h);
    let fill_radius' \
  '    let (cx, cy, radius) = badge_geometry(w, h);
    if rgba.len() > 0 { return; }
    let fill_radius' \
  'C4 the badge is never actually painted'

mutate "$W" \
  '        Verdict::Broken => BROKEN_COLOR,' \
  '        Verdict::Broken => SUBSTITUTED_COLOR,' \
  'C5 a broken configuration looks identical to a substituted one'

mutate "$W" \
  '    let cx = (w as f32 - 1.0) - radius;
    let cy = (h as f32 - 1.0) - radius;' \
  '    let cx = (w as f32 - 1.0) - radius * 0.4;
    let cy = (h as f32 - 1.0) - radius * 0.4;' \
  'C6 the badge is clipped by the icon edge'

mutate "$W" \
  '                "⚠ Diagnostic Report — {} {} (گزارش تشخیصی)",
                self.problems,
                self.verdict.label()' \
  '                "⚠ Diagnostic Report — {} (گزارش تشخیصی)",
                self.verdict.label()' \
  'C7 the menu item never says how many problems there are'

mutate "$W" \
  '                "{BASE_TOOLTIP} — ⚠ {} problem(s): open Diagnostic Report from the menu",
                self.problems' \
  '                "⚠ {} problem(s)",
                self.problems' \
  'C8 the tooltip stops naming the app'

mutate "$U" \
  '        String::new(), // window title — deliberately empty' \
  '        String::from("title"),' \
  'C9 start eats the target as its window title'

mutate "$U" \
  '        format!("\"{target}\""),' \
  '        target.to_string(),' \
  'C10 a report path with spaces is split by cmd'

# ---- the engine-failure destination -----------------------------------------

mutate "$W" \
  '        if self.active_engine_missing {
            OpenTarget::EnginePanel
        } else {
            OpenTarget::Report
        }' \
  '        let _ = self.active_engine_missing;
        OpenTarget::Report' \
  'C11 a dead engine sends the user to a text file instead of the Engines tab'

mutate "$W" \
  '        if self.active_engine_missing {' \
  '        if self.verdict == Verdict::Broken {' \
  'C12 the destination is read off the severity label, not the fact'

mutate "$W" \
  '                "⚠ Active Engine '"'"'{}'"'"' is not registered — every dictation fails (مدیریت موتورها)",
                self.active_engine' \
  '                "⚠ Active Engine '"'"'{}'"'"' is not registered — every dictation fails (مدیریت موتورها)",
                String::new()' \
  'C13 the menu item does not say which engine is dead'

mutate "$W" \
  '            OpenTarget::EnginePanel => format!' \
  '            OpenTarget::EnginePanel | OpenTarget::Report => format!' \
  'C14 the engine item promises the file it no longer opens'

mutate "$W" \
  '            OpenTarget::EnginePanel => format!(
                "{BASE_TOOLTIP} — ⚠ engine '"'"'{}'"'"' is not registered: every dictation will fail",
                self.active_engine
            ),' \
  '            OpenTarget::EnginePanel => BASE_TOOLTIP.to_string(),' \
  'C15 the tooltip hides the dead engine on a red badge'

mutate "$D" \
  '    let active_engine_missing = plan.selection == ActiveSelection::Missing;' \
  '    let active_engine_missing = false;' \
  'C16 the diagnosis never records that the selected engine is missing'

# ---- the completed report ----------------------------------------------------

mutate "$D" \
  '            HotkeyLine::new(
                HotkeyRole::Record,
                &settings.hotkey.record,
                hotkeys.problems(),
            ),' \
  '            HotkeyLine::new(
                HotkeyRole::Record,
                &settings.hotkey.record,
                &[],
            ),' \
  'C17 the record row never notices it was replaced'

mutate "$D" \
  '    let effective = if substituted {
            FALLBACK_SPEC.to_string()
        } else {
            requested.to_string()
        };' \
  '    let effective = requested.to_string();' \
  'C18 the report names the requested key, not the working one'

mutate "$D" \
  '    let _ = writeln!(out, "hotkeys");
    for line in &d.hotkeys {
        let _ = writeln!(out, "{}", line.row());
    }' \
  '    let _ = writeln!(out, "hotkeys ({} found)", d.problems.len());' \
  'C19 the hotkeys section is dropped from the file'

mutate "$D" \
  '            "  active    {}   ** NOT REGISTERED — not in the chain below, so dictation fails **",' \
  '            "  active    {}   (all good)",' \
  'C20 an unregistered engine is never marked'

mutate "$H" \
  'pub const FALLBACK_SPEC: &str = "CapsLock";' \
  'pub const FALLBACK_SPEC: &str = "F12";' \
  'C21 the report promises a key the resolver does not use'

mutate "$W" \
  '            OpenTarget::EnginePanel => format!(
                "Active engine' \
  '            OpenTarget::EnginePanel | OpenTarget::Report => format!(
                "Active engine' \
  'C22 the dashboard callout stops naming the dead engine'

canary_finish