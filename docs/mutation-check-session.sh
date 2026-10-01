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
M=src/state/machine.rs

canary_init "$S" "$U" "$M"

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

mutate "$S" \
  '            Some(SessionPhase::Recording) | Some(SessionPhase::AwaitingResult) => Ok(()),' \
  '            Some(SessionPhase::Recording) => Ok(()),
            Some(SessionPhase::AwaitingResult) => Err("late result of a cancelled session"),' \
  'C12 a result arriving after the key was released is refused'

mutate "$S" \
  '    pub fn cancelled(&mut self) {
        if let Some(id) = self.open.last().map(|s| s.id) {
            self.close(id, SessionPhase::Cancelled);
        }' \
  '    pub fn cancelled(&mut self) {
        if let Some(id) = self.open.last().map(|s| s.id) {
            self.close(id, SessionPhase::Completed);
        }' \
  'C13 a cancelled session is treated as a finished one'

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

mutate "$M" \
  '        EmitOutcome::Typed { .. } | EmitOutcome::NotAttempted { .. } | EmitOutcome::Skipped(_) => {
            AppState::Idle
        }' \
  '        EmitOutcome::Typed { .. } | EmitOutcome::Skipped(_) => AppState::Idle,
        EmitOutcome::NotAttempted { .. } => AppState::Error("refused".to_string()),' \
  'C17 a refused result wears the error badge the user never asked for'

# C18 (the insertion boundary in `machine::emit`) is deliberately NOT mutated
# here: that check only runs with real audio and a live engine, so no automated
# test notices either way. Recording the gap beats inventing a mutation that
# would have to report MISSED for the wrong reason. It stays "unverified" until
# a test-session turn can exercise a cancelled session end to end.

canary_finish
