#!/usr/bin/env bash
# Canary check for the state refactor (S1) and the orb click target (O1): mutate
# one decision at a time and record which tests notice. A mutation that leaves
# the suite green is a hole in the tests, not a pass.
#
# Run from v-2/voice-ptt:  bash ../docs/mutation-check-session.sh
set -u

# shellcheck source=docs/canary-harness.sh
source "$(dirname "$0")/canary-harness.sh"

S=src/state/session.rs
U=src/state/utterance.rs
M=src/state/machine.rs
O=src/gui/orb.rs
A=src/gui/orb_animation.rs
W=src/gui/window_shape.rs
P=src/processing/mod.rs
N=src/processing/normalizer.rs
C=src/config/settings.rs

canary_init "$S" "$U" "$M" "$O" "$A" "$W" "$P" "$N" "$C"

echo "=== canaries: mutate one decision, expect red ==="

mutate "$S" \
  '    pub fn recording_stopped(&mut self, id: SessionId) {
        if let Some(record) = self.open.iter_mut().find(|s| s.id == id) {
            record.phase = SessionPhase::AwaitingResult;
        }
        self.recording = false;
        self.latch.reset();
    }' \
  '    pub fn recording_stopped(&mut self, id: SessionId) {
        if let Some(record) = self.open.iter_mut().find(|s| s.id == id) {
            record.phase = SessionPhase::Completed;
        }
        self.recording = false;
        self.latch.reset();
    }' \
  'C1 stopping the microphone closes the session (the final chunk is dropped)'

# C12 ("a result arriving after the key was released is refused") and C13 ("a
# cancelled session is treated as a finished one") used to live here. The cancel
# work reshaped `accepts_result` and `cancelled`, and both decisions are now
# mutated as C31 and C32 in `mutation-check-coordinator.sh` — one copy of a
# judge, one place to look.

# C7 used to mutate `on_cancel`'s half-open-tap guard. That guard is gone too;
# the rule it protected is now the `latch.reset()` line below.
mutate "$S" \
  '        self.recording = false;
        self.latch.reset();
        vec![Effect::DiscardSession(target)]' \
  '        self.recording = false;
        vec![Effect::DiscardSession(target)]' \
  'C7 cancel leaves a half-open tap behind'

mutate "$S" \
  '    pub fn open_session(&mut self, capture_open: bool) -> Option<SessionId> {
        if !capture_open {
            return None;
        }' \
  '    pub fn open_session(&mut self, capture_open: bool) -> Option<SessionId> {
        if false {
            return None;
        }' \
  'C14 a microphone that never opened still gets an identity'

mutate "$S" \
  '            if self.closed.len() > REMEMBERED_CLOSURES {
                let drop_to = self.closed.len() - REMEMBERED_CLOSURES;
                self.closed.drain(..drop_to);
            }' \
  '' \
  'C15 the remembered-closure list grows for the life of the process'

mutate "$S" \
  '        let chunk = ChunkId(record.next_chunk);
        record.next_chunk += 1;
        Some(chunk)' \
  '        let chunk = ChunkId(1);
        Some(chunk)' \
  'C16 every chunk of a session claims to be the first one'

mutate "$S" \
  '    if frame == 0 || cursor >= buffer_len {
        return 0;
    }
    (buffer_len - cursor) / frame' \
  '    if frame == 0 {
        return 0;
    }
    buffer_len / frame' \
  'C2 VAD read cursor re-feeds analysed audio'

mutate "$S" \
  'let cursor = cursor.saturating_sub(keep_from).min(remaining);' \
  'let cursor = cursor.saturating_sub(keep_from);' \
  'C3 chunk cut can leave the cursor past the tail'

mutate "$S" \
  '    if !streaming_enabled && buffer_len >= capacity {' \
  '    if buffer_len >= capacity {' \
  'C4 a full ring buffer ends a streaming session (the 30 s bug)'

mutate "$S" \
  'LatchAction::FinishAndRestart => {
                vec![Effect::FinishSession, Effect::BeginRecording]
            }' \
  'LatchAction::FinishAndRestart => vec![Effect::BeginRecording],' \
  'C5 a late second tap restarts without finalising the first'

mutate "$S" \
  '        if effects.is_empty() {
            effects.push(Effect::PollAudio);
        }' \
  '' \
  'C6 the tick stops pumping audio while recording'

# C7's old anchor is gone; the reshaped one is above the T1 section.

mutate "$U" \
  '    if merge.text.is_empty() {
        return TypePlan::Skip(SkipReason::EmptyAfterSeam);
    }' \
  '' \
  'C8 text the seam ate is typed anyway'

mutate "$U" \
  '    if raw.trim().is_empty() {
        return TypePlan::Skip(SkipReason::EmptyTranscript);
    }' \
  '' \
  'C9 an empty transcript counts as something to type'

mutate "$U" \
  '        let rms = if samples.is_empty() {
            0.0
        } else {' \
  '        let rms = if true {
            0.0
        } else {' \
  'C10 loudness reports 0 dBFS for every utterance'

mutate "$U" \
  'pub(crate) fn is_transient(state: &AppState) -> bool {
    matches!(state, AppState::Error(_))
}' \
  'pub(crate) fn is_transient(_state: &AppState) -> bool {
    false
}' \
  'C11 a failure never clears itself (push-to-talk stays frozen)'

# The T2 coordinator decisions live in their own script,
# `mutation-check-coordinator.sh`, which sources this same harness. Two copies of
# a judge is how one of them goes stale — see the note at the top of
# `canary-harness.sh`.

# C17 used to mutate `machine::emit`'s refusal branch. That code is gone — the
# T2 coordinator stage moved every effect into one `apply` — and the gap note
# at the bottom now covers what replaced it.

# ── O1: the orb's click target ──────────────────────────────────────────
#
# The dead ring (B0 §6-1) existed because the pointer rectangle and the Win32
# region were derived from two different numbers. These four mutations each put
# a second, wrong derivation back, and each has to be caught by name.

mutate "$O" \
  '    painted_reach_pt(scale, with_shake)
        .max(MIN_INTERACTION_RADIUS)
        .min(Orb::max_canvas_points() * 0.5)' \
  '    let _ = with_shake;
    (BASE_DIAMETER * 0.5 * scale * 1.1).max(MIN_INTERACTION_RADIUS)' \
  'C19 the click target goes back to guessing "radius * 1.1" (the dead ring)'

mutate "$A" \
  '    pub fn shakes(self) -> bool {
        matches!(self, OrbMode::Error)
    }' \
  '    pub fn shakes(self) -> bool {
        let _ = self;
        true
    }' \
  'C20 every mode is charged for a shake only Error can perform'

mutate "$W" \
  '    cached == Some((hwnd, px)) && observed == expected_region_box(px, side_px, ppp)' \
  '    let _ = (observed, side_px, ppp);
    cached == Some((hwnd, px))' \
  'C21 the click-region cache is trusted without asking the window (B0 §6-3)'

mutate "$W" \
  '            Some([cx - r, cy - r, cx + r + 1, cy + r + 1])' \
  '            Some([cx - r, cy - r, cx + r, cy + r])' \
  'C22 the region read-back is compared against a box GDI never produced'

# ── T1: the text mode and the half-space rule ───────────────────────────
#
# T0 measured four healthy Persian words being broken by one suffix list
# (T0-001…004). Each mutation below puts the old behaviour back.

mutate "$N" \
  'const HALF_SPACE_SUFFIXES: &[&str] = &["ها", "های", "هایی", "تر", "ترین"];' \
  'const HALF_SPACE_SUFFIXES: &[&str] = &["ها", "های", "هایی", "تر", "ترین", "ام", "ات", "اش"];' \
  'C23 the non-productive suffixes -ات/-ام are back (کلمات → کلم‌ات)'

mutate "$P" \
  '        TextMode::Conservative => dictionary.correct(&normalizer.normalize_conservative(text)),' \
  '        TextMode::Conservative => dictionary.correct(&normalizer.normalize(text)),' \
  'C24 conservative mode quietly runs the full normaliser again'

mutate "$P" \
  '        TextMode::Raw => text.to_string(),' \
  '        TextMode::Raw => dictionary.correct(&normalizer.normalize(text)),' \
  'C25 raw mode is no longer raw'

mutate "$P" \
  '            "raw" => TextMode::Raw,' \
  '            "raw" => TextMode::Raw,
            "nonsense" => TextMode::Raw,' \
  'C26 an unknown mode name drops to raw, typing verbatim text into a document'

# T1 changed `machine.rs` to ask the settings for the mode, but that call site
# only runs with real audio and a live injection target, so no mutation of it
# can be judged here either way. The *policy* it depends on is covered by C26
# and by `the_text_mode_survives_a_round_trip_through_the_config_file`; what is
# unverified is that the live path reads it at all. Recorded as a gap rather
# than given a mutation that would have to report MISSED.

# C18 (the insertion boundary in `machine::emit`) is deliberately NOT mutated
# here: that check only runs with real audio and a live engine, so no automated
# test notices either way. Recording the gap beats inventing a mutation that
# would have to report MISSED for the wrong reason. It stays "unverified" until
# a test-session turn can exercise a cancelled session end to end.

# C17 has been reshaped by the T2 coordinator stage. `machine.rs` no longer owns
# a loop: `StateMachine::run` forwards hotkeys onto `Input` and beats every
# 20 ms. The forwarding, the heartbeat and the real `Port` all need a live
# capture device, so no mutation of them can be judged automatically. What IS
# covered is that the loop those two producers feed behaves: the heartbeat is
# `Input::Tick` in the ten scenarios, and the pure endpoint/chunk policy is
# still unit-tested here (`endpoint_policy_finalizes_on_timeout_or_release`,
# plus C2/C3/C4). The wiring itself stays unverified until a test-session turn
# runs the app.

canary_finish
