#!/usr/bin/env bash
# The T2 coordinator batch, on its own.
#
# It exists for the same reason `canary-harness.sh` exists: the judge must not
# have two copies. This script sources that one and lists only the coordinator
# decisions, so a run of this stage can be measured on its own without re-measuring
# S1/O1/T1 — which is what makes "the tests for this stage bite" a claim that
# can be checked in a couple of minutes instead of a quarter of an hour.
#
# Run from v-2/voice-ptt:  bash ../docs/mutation-check-coordinator.sh
set -u

# shellcheck source=docs/canary-harness.sh
source "$(dirname "$0")/canary-harness.sh"

K=src/state/coordinator.rs
U=src/state/session.rs
M=src/state/machine.rs
E=src/state/utterance.rs
I=src/output/injector.rs

canary_init "$K" "$U" "$M" "$E" "$I"

# The batch is longer than one tool budget, so it is run in sets:
#
#   CANARY_SETS=1 bash ../docs/mutation-check-coordinator.sh
#
# A mutation outside the set is reported SKIP **with its id**, never silently
# dropped, because "18 of 26 ran and all 18 caught" and "26 ran and all 26
# caught" must not look the same in a log. `CANARY_SETS=all` (the default) runs
# everything, which is what an unattended run should do.
#
# The id is the first word of the name, so a set is written as a plain list and
# a typo is a visible typo rather than a silently shorter run.
CANARY_SETS="${CANARY_SETS:-all}"
eval "$(declare -f mutate | sed '1s/^mutate /canary_mutate /')"
mutate() {
  local name="$4" id
  id="${name%% *}"
  case " $CANARY_SETS " in
    *" all "*|*" $id "*) canary_mutate "$@" ;;
    *) printf 'SKIP    %-52s not in CANARY_SETS=%s
' "$name" "$CANARY_SETS" ;;
  esac
}

echo "=== T2 canaries: mutate one decision, expect red ==="

# Every anchor is a SINGLE line. A multi-line anchor has broken this harness more
# than once — the shell mangles it, the anchor stops matching, and a skipped
# mutation reports as SKIP rather than as the hole it is.

# Removing the acceptance check. This is the whole stage: a result that arrives
# after its session was cancelled must change nothing.
mutate "$K" \
  '            Some(id) => self.with_session(|s| s.accepts_result(id)),' \
  '            Some(id) => self.with_session(|_| Ok(())),' \
  'C27 the cancel check is gone: a cancelled result reaches the keyboard'

# Waiting for the conversion inline. The loop's `select!` is what keeps Escape
# readable while the engine works; a loop that awaits the in-flight task before
# it reads the next event has no such property.
mutate "$K" \
  '        if !in_flight.handle.is_finished() {' \
  '        if false {' \
  'C28 the loop waits for the conversion inline and cannot read Escape'

# Order of arrival instead of order of hand-out: a result is typed the moment it
# reaches the loop, so a final chunk's answer no longer waits for the
# mid-session chunk it belongs after.
#
# This one was MISSED at first, and the reason is worth keeping. The end-to-end
# scenarios cannot catch it: two conversions are no longer two racing tasks, but
# the *queue* can still be handed a chosen order. It is caught by
# `a_result_that_arrives_early_waits_for_its_turn`, which gives the gate the
# second answer first and requires it to wait.
mutate "$K" \
  '            let Some(next) = self.pending.remove(&self.apply_next) else {' \
  '            let Some(next) = self.pending.pop_first().map(|(_, v)| v) else {' \
  'C29 a result is applied in arrival order, so the final overtakes its chunks'

# The port said there was nothing worth an engine call and the loop asks for one
# anyway: a recogniser handed silence answers "empty transcription" or explodes,
# either way for a tap of the key.
mutate "$K" \
  '        self.enqueue(session, audio, true);' \
  '        self.enqueue(session, Some(audio.unwrap_or_else(|| AudioUtterance { samples: Vec::new(), sample_rate: 16_000 })), true);' \
  'C30 an utterance the VAD called empty is still sent to the engine'

# The ending is no longer an ordered event: the session closes the moment the
# port says there is nothing to convert, so a chunk that is still in the engine
# is refused afterwards as a "late result of a completed session" — the text the
# user really spoke, deleted by the end of the dictation.
mutate "$K" \
  '        self.enqueue(session, audio, true);' \
  '        self.settle(session, true, self.owns_badge(session));' \
  'C35 an ending with no audio closes the session instead of taking its turn'

# One seam stitcher for the whole process instead of one per session: the older
# dictation's tail is still remembered when the newer one's first chunk arrives,
# and the newer chunk's opening word is deleted as overlap.
mutate "$K" \
  '            .entry(session)' \
  '            .entry(None)' \
  'C34 one seam stitcher for every session: an old tail eats a new word'

# Every result owns the visible badge, so an older dictation's ending (or
# failure) overwrites the recording that started after it.
mutate "$K" \
  '        let owns = self.owns_badge(session);' \
  '        let owns = true;' \
  'C36 an older result overwrites the live recording'\''s badge'

# Every closed error window clears the badge, whoever it belongs to: an expiry
# armed before a recording starts takes the recording down with it, and a window
# superseded by a newer error clears that newer error.
mutate "$K" \
  '        if still_active {' \
  '        if true {' \
  'C37 an expired window clears whatever is on the badge now'

# Dropped queued work leaves its order number in the sequence: the next real
# answer waits forever for a result that is never coming. This is the stall the
# stage is about, reintroduced through the cancel path.
mutate "$K" \
  '            while self.skipped.remove(&self.apply_next) {' \
  '            while false {' \
  'C38 a dropped job leaves a hole in the order numbers'

# A conversion that died is treated as an empty success: nothing is reported,
# and the failure the user cannot see is exactly the one they cannot explain.
mutate "$K" \
  '                                failure: Some(format!("conversion task panicked: {why}")),' \
  '                                failure: None,' \
  'C39 a panicked conversion is reported as an empty success'

# The version half of an error window's identity, with two failures that say the
# same thing: the message check cannot refuse the stale window, so only the
# number can.
mutate "$K" \
  '        let still_active = window.version == self.error_version' \
  '        let still_active = true' \
  'C43 an older error window clears a newer error that reads the same'

# The hands-free badge written from the loop's guess instead of from the rules:
# every turn ends by clearing it. A double-tap produces no effect at all — there
# is nothing for the loop to perform — so a badge that only exists as a
# consequence of performing something never lights, and hands-free dictation
# looks like an ordinary one.
mutate "$K" \
  '        self.publish_latched();' \
  '        self.status.set_latched(false);' \
  'C40 the hands-free badge is guessed by the loop instead of asked for'

# The two producers were separate tasks, and the heartbeat's clone of the sender
# is what made a closed event source invisible: the loop's only shutdown is that
# channel closing, and a beat that goes on forever keeps it open. The producer
# that still answers a dead source never drops the last sender.
mutate "$M" \
  '                None => return,' \
  '                None => {}' \
  'C41 a closed event source does not stop the producer, so the loop never hears it'

# The loop's own half of the same shutdown: a closed input is the end of the
# program, not a turn it does not care about.
mutate "$K" \
  '                    None => break,' \
  '                    None => {}' \
  'C42 a closed event channel no longer stops the loop'

# The heartbeat's schedule moves when the *wait is built* instead of when a beat
# happens. Every event rebuilds that wait and throws it away, so the beat would be
# pushed one period later for every keystroke the user pressed first — the
# heartbeat slowed down by the user typing, which is the opposite of its job.
mutate "$M"   '            _ = tokio::time::sleep_until(due) => {'   '            _ = { due += period; tokio::time::sleep_until(due - period) } => {'   'C44 building the wait pushes the next beat one period later'

# The `Skip` re-base lands on the moment of the stall rather than a full period
# after it: one beat arrives as a burst of two, one period apart and then nothing.
mutate "$M"   '    std::cmp::max(at + period, now + period)'   '    std::cmp::max(at + period, now)'   'C45 a beat after a stall is followed immediately by another one'

# Refusing a result whose session has stopped recording. This is `session.rs`'s
# decision, but it is kept here because it is the same stage: the final chunk's
# text arrives *by definition* after the key came up, so refusing it drops the
# ordinary result of every dictation.
mutate "$U" \
  '            Some(SessionPhase::Recording) | Some(SessionPhase::AwaitingResult) => Ok(()),' \
  '            Some(SessionPhase::Recording) | Some(SessionPhase::AwaitingResult) => Err("cancelled"),' \
  'C31 a result arriving after the key was released is refused'

# Cancel no longer reaches a session that is waiting for its result — the rule
# this stage was written to remove.
mutate "$U" \
  '        let target = self.open.last().map(|s| s.id);' \
  '        let target = if self.recording { self.open.last().map(|s| s.id) } else { None };' \
  'C32 cancel only fires while the microphone is still open'


# The destination is asked and the answer is ignored: the insert goes to
# whatever window happens to be in front, which is the one outcome the check
# exists to prevent. Catches three scenarios, and the one that matters is the
# refusal scenario's zero `Op`.
mutate "$K"   '        let steps = if validity.allows_insert() {'   '        let steps = if true {'   'C46 a refused destination is typed into anyway'

# "No destination" read as "any destination". `C-T2-5` was this mistake inside
# the tracker; here it is the same mistake one layer up, and the neutral value
# of a failed capture is exactly where it hides.
mutate "$K"   '            None => TargetValidity::Unknown,'   '            None => TargetValidity::Valid,'   'C47 a dictation with no destination is typed into whatever is in front'

# One destination for the whole process instead of one per session: the second
# recording's capture replaces the first's, and an older answer that lands after
# it is judged against a window it was never dictated into. This is the mutation
# the two-session scenario exists for, and it is the exact bug a shared
# `TargetTracker` would have had.
mutate "$K"   '            .insert(session, target);'   '            .insert(None, target);'   'C48 one destination for every session: an old answer is judged against a new window'

# The captured window is thrown away instead of kept, so every dictation is
# refused for want of a destination. The opposite of the bug above, and the
# reason the capture line is worth a canary of its own: a stage that only ever
# refuses would pass a suite that never checked anything was sent.
mutate "$K"   '                            Some(id) => self.remember_target(session, id.clone()),'   '                            Some(_) => {},'   'C49 a captured window is never kept, so nothing is ever typed'

# A seam erase that did not go out whole no longer stops the insert, so the
# complete word is typed on top of the fragment the erase was meant to remove
# and the user is left with both.
mutate "$K"   '            if !whole {'   '            if false {'   'C50 a torn seam erase is followed by the text anyway'

# The seam memory survives an insert that did not land, so the next chunk is
# stitched against text that is not in the document — and the stitcher's
# backspace rule can then delete the user's own words.
mutate "$K"   '        if !outcome.is_whole() {'   '        if false {'   'C51 a failed insert leaves the seam memory behind'

# A send the platform only partly accepted is called a success. The verdict is
# pure, so this is caught by the table test rather than by a scenario.
mutate "$E"   '    if steps.iter().all(|step| step.whole_pairs()) {'   '    if true {'   'C52 a partly accepted send is reported as Complete'

# A torn down/up pair counts as a whole send: the "success" that is not one, and
# the case a blocking `SendInput` would produce if it ever returned a partial
# count.
mutate "$I"   '        self.whole() && self.accepted.is_multiple_of(2)'   '        self.whole()'   'C53 a torn key pair is taken for a whole send'

# Three decisions are recorded as **gaps** rather than as canaries, because they
# were measured rather than assumed:
#
# * C33 was "delete `biased;`". With the keyword removed,
#   `nothing_is_typed_after_quit` stayed green for 240 rounds (12 runs × 20): the
#   loop parks at `select!` long before a worker thread gets far enough to send,
#   so the two branches are essentially never ready at the same instant. The line
#   stays, justified by the policy it states (an event the user already sent is
#   handled before an answer that is already waiting), not by a test.
# * The `Err` arm of `reap_in_flight` (a task killed from outside its own body)
#   is **not tested in this delivery**: a panic inside the conversion is caught
#   by the worker itself, before it can kill the task, and no scenario here
#   builds a task that dies from outside. The arm is kept as defence, and
#   `a_conversion_that_dies_is_reported_and_does_not_stall_the_queue` pins the
#   behaviour that matters — the failure is tied to its own job and the queue
#   moves on — through the path a scenario does reach.
# * `producer.abort()` in `machine.rs` is **not tested in this delivery** either:
#   a task that is left running dies with the runtime, and no scenario can see
#   it from here.
# * `Coordinator::kept` — the text a refused destination left in memory — has
#   **no reader yet**, so nothing here can canary it: removing the write would
#   keep every scenario green. It is memory, not a feature: the recovery surface
#   is a later stage's decision, and the gap is written down rather than hidden.
#   It is deliberately *not* written into `AppStatus::last_text`, which means
#   "text that was typed" and feeds the orb's history.
#
# And one gap is now **closed**: the version half of an `ErrorWindow`'s identity.
# With the two windows' messages made different (which is what made the
# newer-error test deterministic), the message check alone refused the stale
# window, so deleting the version comparison stayed green. C43 closes it: two
# failures that say the same thing cannot be told apart by their text, so the
# number is all that is left.

canary_finish
