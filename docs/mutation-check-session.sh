#!/usr/bin/env bash
# Canary check for the phase-three refactor: mutate one decision at a time and
# record which tests notice. A mutation that leaves the suite green is a hole in
# the tests, not a pass.
#
# Run from v-2/voice-ptt:  bash ../docs/mutation-check-session.sh
set -u

# shellcheck source=docs/canary-harness.sh
source "$(dirname "$0")/canary-harness.sh"

S=src/state/session.rs
U=src/state/utterance.rs

canary_init "$S" "$U"

echo "=== canaries: mutate one decision, expect red ==="

mutate "$S" \
  '    pub fn ended_session(&mut self) {
        self.recording = false;
        self.latch.reset();
    }' \
  '    pub fn ended_session(&mut self) {
        self.recording = false;
    }' \
  'C1 finalise no longer clears the hands-free latch'

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

mutate "$S" \
  '        if !self.recording {
            return Vec::new();
        }
        self.recording = false;
        self.latch.reset();
        vec![Effect::DiscardSession]' \
  '        if !self.recording {
            return Vec::new();
        }
        self.recording = false;
        vec![Effect::DiscardSession]' \
  'C7 cancel leaves a half-open tap behind'

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

canary_finish
