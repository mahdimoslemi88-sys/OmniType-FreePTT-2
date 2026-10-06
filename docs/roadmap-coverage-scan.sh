#!/usr/bin/env bash
# Roadmap coverage scan.
#
# Every row below is one promise made by docs/PRODUCT-DEVELOPMENT-ROADMAP.md, and
# the file/pattern that has to carry it. Text checks only — no build, no device,
# no window — so it can be run as often as wanted and cannot be confused with a
# passing test suite. It answers one question: "does the thing the roadmap asks
# for exist somewhere in the tree?", never "does it work?".
#
# Run from v-2/:  bash docs/roadmap-coverage-scan.sh [-v]
#
# Exit status 1 if any row is missing. A missing row is either a real gap or a
# stale pattern; both are findings, never something to silence by editing the
# pattern until it matches.
set -u

cd "$(dirname "$0")/.." || exit 2
VERBOSE=0
[ "${1:-}" = "-v" ] && VERBOSE=1

checks=0
fails=0
group=""

section() {
  group="$1"
  printf '\n== %s\n' "$1"
}

# need <label> <extended-regex> <file...>
need() {
  local label="$1" pattern="$2"
  shift 2
  checks=$((checks + 1))
  local hit
  hit=$(grep -nE -- "$pattern" "$@" 2>/dev/null | head -1)
  if [ -n "$hit" ]; then
    printf '  ok   %s\n' "$label"
    [ "$VERBOSE" = 1 ] && printf '         %s\n' "$hit"
  else
    fails=$((fails + 1))
    printf '  MISS %s\n' "$label"
    printf '         pattern: %s\n' "$pattern"
    printf '         files:   %s\n' "$*"
  fi
}

# gone <label> <extended-regex> <file...> — must NOT appear anywhere.
gone() {
  local label="$1" pattern="$2"
  shift 2
  checks=$((checks + 1))
  if grep -qE -- "$pattern" "$@" 2>/dev/null; then
    fails=$((fails + 1))
    printf '  MISS %s (found, and it should not exist)\n' "$label"
  else
    printf '  ok   %s\n' "$label"
  fi
}

P=voice-ptt/src

section "§3 baseline: the evidence files exist and carry the samples"
need "B0 orb baseline" 'هاله|halo' docs/execution/B0-orb-baseline.md
need "T0 text baseline: half-space sample" 'نیم.*فاصله' docs/execution/T0-text-baseline.md
need "S0 session baseline: target map" 'مقصد' docs/execution/S0-session-baseline.md
need "K0 contracts exist" '^## ' docs/execution/CONTRACTS.md

section "§4.1 selectable Persian correction (T1)"
need "TextMode carries the three modes" 'pub enum TextMode' $P/processing/mod.rs
need "mode is parsed from the settings string" 'pub fn parse\(value: &str\) -> Self' $P/processing/mod.rs
need "unknown mode falls back to standard, not raw" 'TextMode::Standard' $P/config/settings.rs
need "raw mode still bypasses the rules" 'TextMode::Raw' $P/processing/mod.rs
need "half-space is its own rule" 'fn fix_half_spaces' $P/processing/normalizer.rs
need "a test pins the conservative sample set" 'mod tests' $P/processing/normalizer.rs

section "§4.2 the insert boundary policy (T2)"
need "boundary module" 'pub fn needs_boundary_space' $P/processing/boundary.rs
need "boundary memory survives a target change only while valid" 'pub struct BoundaryTracker' $P/processing/boundary.rs
need "seam repair exists and is bounded" 'pub struct SeamStitcher' $P/processing/seam.rs
need "seam repair can be switched off" 'seam_backspace' $P/config/settings.rs

section "§4.3 orb geometry, size and interaction (O1)"
need "one reference for painted reach" 'fn painted_reach_pt' $P/gui/orb.rs
need "click radius is the interaction radius, not the glow" 'fn interaction_radius_pt' $P/gui/orb.rs
need "click radius has a floor, so switching the glow off cannot shrink it" 'MIN_INTERACTION_RADIUS' $P/gui/orb.rs
need "painting is separated from window management" 'pub mod orb_animation|pub struct OrbAnimation' $P/gui/orb_animation.rs
need "window shape / click region lives on its own" 'pub enum ClickRegion' $P/gui/window_shape.rs
need "the orb is clamped to the work area while dragged" 'clamp_home|keep_out|EDGE_MARGIN_PT' $P/gui/orb.rs
need "user scale is clamped to a usable range" 'MAX_USER_SCALE' $P/gui/orb.rs
need "the saved position is understood in physical pixels" 'orb_position_x' $P/config/settings.rs

section "§5.1 keep the typing target (T2)"
need "target identity is more than an HWND" 'pub struct TargetIdentity' $P/output/target.rs
need "validity is a three-way answer, not a bool" 'pub enum TargetValidity' $P/output/target.rs
need "the target is re-checked before typing" 'validate_target' $P/state/coordinator.rs
need "a late answer for a cancelled session types nothing" 'NotAttempted' $P/state/coordinator.rs

section "§5.2 preview and optional confirmation (V1)"
need "the review decision layer" 'pub enum ReviewCommand' $P/state/review.rs
need "a draft is inert until a command names it" 'pub fn should_hold' $P/state/review.rs
need "the wire between the loop and the GUI" 'pub ' $P/state/review_channel.rs
need "the window the user acts in" 'pub fn render' $P/gui/overlay/review_panel.rs
need "review is opt-in" 'review_before_insert: false' $P/config/settings.rs
need "the copy path needs no new dependency" 'copy_text' $P/gui/overlay/review_panel.rs

section "§5.3 waiting corner and return after idle (O2)"
need "the idle decision is pure and testable" 'pub enum ReturnBlocker' $P/gui/orb_idle_policy.rs
need "text waiting for a decision suppresses the return" 'pending_text' $P/gui/orb_idle_policy.rs
need "the return can be switched off" 'orb_return_enabled' $P/config/settings.rs
need "waiting time is a setting with the roadmap's 60s default" 'orb_return_after_idle_secs' $P/config/settings.rs
need "the corner is a setting" 'orb_return_corner' $P/config/settings.rs
need "a pinned position and the waiting spot are two values" 'orb_pinned' $P/config/settings.rs
need "a monitor that went away does not strand the window" 'MonitorChoice|unavailable|detached' $P/gui/orb_idle_policy.rs

section "§5.4 recovery of un-inserted text (R1)"
need "recovery is text, never audio" 'Undelivered' $P/state/review.rs
need "the offered text is only the part that did not land" 'fn unaccepted_text' $P/state/coordinator.rs
need "a record becomes something the user can act on" 'fn recover_from' $P/state/coordinator.rs
need "retry repeats the insert, not the conversion" 'Retry repeats the insert, never the conversion' $P/state/review.rs
need "recovery does not depend on the review setting" 'fn should_hold' $P/state/review.rs
need "what is kept is reachable from production code" 'self\.recover_from\(&record\)' $P/state/coordinator.rs
gone "no audio retention setting" 'save_audio|keep_audio|audio_archive' $P/config/settings.rs

section "§6.1 one-action word fix (D1)"
need "the correction model" 'pub struct Correction|pub struct Dictionary' $P/processing/dictionary.rs
need "candidate assessment" 'pub fn assess' $P/processing/quickfix.rs
need "the fix explains itself when it changes nothing" 'pub enum NoEffect' $P/processing/quickfix.rs
need "a preview of the effect exists" 'pub fn preview' $P/processing/quickfix.rs
need "the panel" 'fn render\(' $P/gui/overlay/dict_fix_panel.rs

section "§6.2 per-application profiles (P1)"
need "profiles are stored with the settings" 'pub profiles' $P/config/settings.rs
need "general rules and per-app overrides are separate types" 'pub struct GeneralRules' $P/profiles/mod.rs
need "the effective rules are resolved in one place" 'pub fn effective' $P/profiles/mod.rs
need "the panel" 'MODE_CHOICES' $P/gui/overlay/profiles_panel.rs
need "the profile is pinned at session start, not re-read per chunk" 'rules_for' $P/state/coordinator.rs

section "§6.3 limited spoken commands (C1)"
need "the command parser is pure" 'pub fn parse' $P/processing/commands.rs
need "commands are off by default" 'commands: false' $P/config/settings.rs
need "a phrase that is not a command is left alone" 'Unknown' $P/processing/commands.rs
need "a newline becomes Enter rather than U+000A" 'fn enter_inputs' $P/output/injector.rs
need "both the burst and the paced path agree" 'enter_inputs\(\)' $P/output/injector.rs

section "§7.1 microphone test (M1)"
need "the diagnostic analyser" 'pub struct MicTestAnalyzer' $P/audio/diagnostics.rs
need "device ownership is arbitrated with the recorder" 'pub enum DeviceOwnership' $P/audio/diagnostics.rs
need "a recording press during the test has a defined answer" 'pub enum RecordPressDecision' $P/audio/diagnostics.rs
need "the panel" 'pub fn render' $P/gui/overlay/mic_test_panel.rs

section "§7.2 secure key storage (A1)"
need "the credential contract" 'pub ' $P/credentials/mod.rs
need "the Windows implementation" 'CredWriteW|CredReadW' $P/credentials/windows.rs
need "migration off the settings file" 'pub ' $P/credentials/migration.rs
need "one resolver, so no second reader of the key appears" 'pub ' $P/credentials_resolver.rs
need "the settings no longer print the key" 'skip_serializing|redact|serialize_with' $P/config/settings.rs

section "§7.3 conditional undo (U1)"
need "the review box keeps a history of its text" 'pub struct EditHistory' $P/gui/overlay/review_panel.rs
need "undo restores a version" 'fn undo' $P/gui/overlay/review_panel.rs
need "redo exists too" 'fn redo' $P/gui/overlay/review_panel.rs
need "undo never reaches the keyboard" "authority is limited to drawing a box" $P/gui/overlay/review_panel.rs
need "the outer half is deliberately off, with its reason" 'بازگردانی در برنامهٔ بیرونی' docs/execution/CONTRACTS.md

section "§8.3 one owner for insertion (the shape the code has)"
need "the outcome type separates complete, partial and failed" 'pub enum InjectOutcome|pub struct Injection' $P/output/injector.rs
need "a partial send is never repeated automatically" 'Partial' $P/state/coordinator.rs
need "the seam memory dies with the target" 'forget|invalidat' $P/state/coordinator.rs

section "§8.4 independent processing steps, in a documented order"
need "the order is written down" 'نرمال.?ساز|normalizer' docs/execution/CONTRACTS.md
need "formal rules are their own step" 'pub fn apply' $P/processing/formal.rs
need "each formal group can be turned off" 'formal_punctuation' $P/config/settings.rs
need "the dictionary step is separate" 'pub fn correct\(&self, text: &str\) -> String' $P/processing/dictionary.rs
need "the samples the roadmap asks for are in the tests" 'محاوره|formal|mixed' $P/processing/formal.rs

section "§8.5 capture decision separated from the operation"
need "session identity" 'pub struct SessionId' $P/state/session.rs
need "chunk identity, meaningless without its session" 'pub struct ChunkId' $P/state/session.rs
need "the typing decision is a pure function of the outcome" 'fn judge_insert' $P/state/utterance.rs
need "free recording has a latch policy" 'LatchPolicy' $P/state/session.rs
need "free recording still ends on silence" 'silence' $P/config/settings.rs

section "§11 the product decisions the roadmap fixed"
need "Persian correction: conservative by default" 'mode: "standard"' $P/config/settings.rs
need "insert mode: direct, with review optional" 'review_before_insert: false' $P/config/settings.rs
need "spoken commands off until asked for" 'commands: false' $P/config/settings.rs
need "return is switchable" 'orb_return_enabled' $P/config/settings.rs
gone "audio retention is absent, not switched off" 'save_audio|save_recordings|\[audio_retention\]|keep_audio' $P/config/settings.rs
gone "no audio retention key in the settings file either" 'retain_audio|archive_audio' $P/config/settings.rs

# ── pass 3: every `voice-ptt/src/...rs` a document points at must exist ────────
# The claim "band 11 says X is in normalizer.rs" is only worth anything if that
# file is still there. A dangling path in a report is a stale claim, and stale
# claims are what this whole file exists to catch.
section "pass 3: doc → code paths (a report may not point at a file that moved)"
paths=$(grep -rhoE 'voice-ptt/src/[A-Za-z0-9_/.-]+\.rs' docs/*.md docs/execution/*.md 2>/dev/null | sort -u)

# Named as a *proposal*, not as a claim that the file is there. CONTRACTS band 0's
# table is titled «پیشنهاد محل ثبت» and the execution plan says new file names
# there are proposed structure; three of these were never built under those
# names (the boundary module went to `processing/`, not `output/`). An explicit
# list so the exception is visible, and so the check cannot be quietly weakened.
PROPOSED="voice-ptt/src/output/boundary.rs
voice-ptt/src/processing/options.rs
voice-ptt/src/state/draft.rs
voice-ptt/src/state/recovery.rs"

link_total=0
link_missing=0
link_proposed=0
while IFS= read -r p; do
  [ -n "$p" ] || continue
  if grep -qxF "$p" <<< "$PROPOSED"; then
    link_proposed=$((link_proposed + 1))
    [ "$VERBOSE" = 1 ] && printf '  prop %s (named as a proposal)\n' "$p"
    continue
  fi
  link_total=$((link_total + 1))
  checks=$((checks + 1))
  if [ -f "$p" ]; then
    [ "$VERBOSE" = 1 ] && printf '  ok   %s\n' "$p"
  else
    link_missing=$((link_missing + 1))
    fails=$((fails + 1))
    printf '  MISS %s (no such file)\n' "$p"
  fi
done <<< "$paths"
printf '  -- %d distinct code paths named by the documents, %d missing, %d proposal-only\n' "$link_total" "$link_missing" "$link_proposed"

section "§12 the documents that carry the results"
need "installer audit" 'نصب' docs/INSTALLER-AUDIT.md
need "measured facts" '|' docs/MEASURED-FACTS.md
need "status file" '^## ' docs/execution/STATUS.md
need "index reaches the review report" 'Q1-review' docs/INDEX.md
need "the scanner itself is indexed" 'roadmap-coverage-scan' docs/INDEX.md

printf '\n== summary: %d checks, %d missing\n' "$checks" "$fails"
if [ "$fails" -eq 0 ]; then
  echo "ALL COVERED"
  exit 0
fi
echo "GAPS FOUND"
exit 1
