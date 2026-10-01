//! The capture session's decisions: what an event means, when a chunk is cut,
//! and when an utterance ends. No audio, no clock, no keyboard.
//!
//! `state/machine.rs` used to re-derive "what does this event mean" at five
//! call sites, with the stop-and-finalise sequence copy-pasted into three of
//! them, and the VAD window arithmetic written out inline a second time in the
//! stop path. Everything here is a pure function over settings and counters, so
//! the rules can be unit-tested with no microphone, no tick and no network.
//!
//! [`SessionDriver`] is the part that answers for the *whole* session: it owns
//! the hands-free latch and whether a recording is live, and turns
//! (event, time) into a list of [`Effect`]s for the loop to perform. The machine
//! keeps no rules of its own beyond performing them and reporting back whether
//! the microphone really opened ([`SessionDriver::began_recording`]) or died
//! mid-session ([`SessionDriver::note_capture_live`]).

use std::time::{Duration, Instant};

use crate::config::settings::StreamingSettings;
use crate::config::HotkeySettings;

// ── hands-free latch (the record key's down/up edges) ─────────────────────

/// What the record key's down/up edges mean for the running session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatchAction {
    /// Nothing to do for this edge (e.g. the key was released while latched).
    None,
    /// Start a fresh recording.
    Start,
    /// Finalise the running recording (transcribe + inject) and go idle.
    Finish,
    /// Finalise the pending tap and immediately start a new recording.
    FinishAndRestart,
    /// A double-tap just latched the recording: keep running hands-free.
    Latched,
    /// A short tap ended: keep recording, but leave the finalise decision open
    /// for a second tap until [`LatchPolicy::tick`] times the window out.
    AwaitSecondTap,
}

/// Hands-free latch policy for the record key (pure logic, unit-tested).
///
/// * **hold** the key → recording runs while held, finalises on release (the
///   long-standing hold-to-talk behaviour, unchanged),
/// * **tap once** → the recording stays open for `double_tap_window_ms` and then
///   finalises as before (a lone tap carries no speech, so it is discarded by
///   the endpoint's min-speech filter either way),
/// * **tap twice quickly** → the recording is *latched*: it keeps running after
///   the key is released, and the next press ends it.
#[derive(Debug)]
struct LatchPolicy {
    enabled: bool,
    tap_max_ms: u64,
    window_ms: u64,
    press_started_at: Option<Instant>,
    pending_tap_at: Option<Instant>,
    latched: bool,
}

impl LatchPolicy {
    fn new(hotkey: &HotkeySettings) -> Self {
        Self {
            enabled: hotkey.double_tap_latch,
            tap_max_ms: hotkey.tap_max_ms,
            window_ms: hotkey.double_tap_window_ms,
            press_started_at: None,
            pending_tap_at: None,
            latched: false,
        }
    }

    fn is_latched(&self) -> bool {
        self.latched
    }

    /// Drops all latch state (cancel, shutdown, external finalise).
    fn reset(&mut self) {
        self.press_started_at = None;
        self.pending_tap_at = None;
        self.latched = false;
    }

    /// Record key pressed.
    fn press(&mut self, now: Instant, recording: bool) -> LatchAction {
        // While latched, the next press ends the hands-free session.
        if self.latched {
            self.reset();
            return LatchAction::Finish;
        }
        // A pending tap plus a new press is the second tap of a double-tap.
        if let Some(tap_at) = self.pending_tap_at.take() {
            self.press_started_at = Some(now);
            if self.enabled && now.duration_since(tap_at) <= Duration::from_millis(self.window_ms) {
                self.latched = true;
                return LatchAction::Latched;
            }
            // Too slow to be a double-tap: close the first tap and start fresh.
            return LatchAction::FinishAndRestart;
        }
        self.press_started_at = Some(now);
        if recording {
            LatchAction::None
        } else {
            LatchAction::Start
        }
    }

    /// Record key released.
    fn release(&mut self, now: Instant, recording: bool) -> LatchAction {
        if !recording {
            self.press_started_at = None;
            return LatchAction::None;
        }
        // Hands-free: releasing the key must not stop the recording.
        if self.latched {
            return LatchAction::None;
        }
        let held = self
            .press_started_at
            .map(|started| now.duration_since(started));
        self.press_started_at = None;
        let is_tap = self.enabled
            && held
                .map(|held| held <= Duration::from_millis(self.tap_max_ms))
                .unwrap_or(false);
        if is_tap {
            self.pending_tap_at = Some(now);
            LatchAction::AwaitSecondTap
        } else {
            self.pending_tap_at = None;
            LatchAction::Finish
        }
    }

    /// Called from the 20 ms tick: closes a tap whose second press never came.
    fn tick(&mut self, now: Instant, recording: bool) -> LatchAction {
        if !recording {
            // A finalise that came from another path (VAD endpoint, ring-buffer
            // valve, cancel) ends the hands-free session too — otherwise a stale
            // latch would swallow the next press.
            self.reset();
            return LatchAction::None;
        }
        if let Some(tap_at) = self.pending_tap_at {
            if now.duration_since(tap_at) >= Duration::from_millis(self.window_ms) {
                self.pending_tap_at = None;
                return LatchAction::Finish;
            }
        }
        LatchAction::None
    }
}

// ── what the loop should do about it ─────────────────────────────────────

/// One instruction from the session to the machine. Effects are performed in
/// order; the machine never invents its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Discard the buffer and the seam memory, then open the microphone.
    BeginRecording,
    /// Stop capture, transcribe and type, and end the session.
    FinishSession,
    /// Throw the buffer away and end the session *without* transcribing.
    DiscardSession,
    /// Pull newly buffered audio and run the VAD over it.
    ///
    /// The one effect whose outcome is not known when it is emitted: it may end
    /// the session (endpoint, ring-buffer valve, hard cap) or hand a mid-session
    /// chunk to the recogniser. The chunk *policy* it consults is pure
    /// ([`should_flush_chunk`]); only the counters it reads are live.
    PollAudio,
}

/// One capture session's identity, unique in the process, handed out from 1.
///
/// It exists because "which dictation does this text belong to" had no answer:
/// the UI guessed a session from the edges of `AppState::Recording`, and a late
/// engine reply had nothing to compare itself against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(pub u64);

/// One piece of a session's audio, numbered from 1 within that session.
///
/// Deliberately meaningless without its session: a bare `ChunkId` cannot be used
/// to order anything, which is why every API that takes one also takes the
/// `SessionId` it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkId(pub u32);

/// How a session was started. This is about the *trigger*, not its shape: a
/// session that streams in chunks is still push-to-talk, and saying otherwise
/// would have made "chunked" a third way of opening something it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    PushToTalk,
    HandsFree,
}

/// Where a session is in its life.
///
/// The distinction this type exists for: **stopping the microphone is not
/// closing the session.** The last chunk's text arrives *after* the key is
/// released — by definition — so a rule that treated "closed" as "no longer
/// recording" would reject the normal result of every dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    /// The microphone is open.
    Recording,
    /// Recording stopped; the last result has not arrived yet. Results that
    /// arrive in this phase are **valid** and must be used.
    AwaitingResult,
    /// Its result arrived (and was injected, or was deliberately not).
    Completed,
    /// The user abandoned it. Nothing of this session may be typed.
    Cancelled,
}

/// How many finished sessions to remember, so a reply that arrives after one was
/// closed can still be answered with *which* ending it hit rather than a guess.
///
/// Eight is far more than the loop can have in flight (one per open session plus
/// the last few), and bounded on purpose: this is the one structure that would
/// otherwise grow for the life of the process.
const REMEMBERED_CLOSURES: usize = 8;

#[derive(Debug, Clone, Copy)]
struct SessionRecord {
    id: SessionId,
    kind: SessionKind,
    phase: SessionPhase,
    /// The chunk id the *next* handed-out piece will carry.
    next_chunk: u32,
}

/// The rules for one capture session, with no hardware attached.
///
/// The machine used to read "am I recording?" back out of the status channel
/// and then re-type the finish sequence at every call site. Owning the flag here
/// instead makes the flag a *rule* — `recording` mirrors whether the microphone
/// is really live — and makes the effect list something a test can assert on.
pub(crate) struct SessionDriver {
    latch: LatchPolicy,
    recording: bool,
    /// Sessions that can still produce a valid result: the one being recorded and
    /// any earlier one still waiting for its last chunk.
    ///
    /// More than one can be here at a time — pressing record again before the
    /// previous result arrives keeps the old session open rather than discarding
    /// text the user already spoke.
    open: Vec<SessionRecord>,
    /// Terminal phase of the sessions that have closed, oldest first.
    closed: Vec<(SessionId, SessionPhase)>,
    next_session: u64,
}

impl SessionDriver {
    pub fn new(hotkey: &HotkeySettings) -> Self {
        Self {
            latch: LatchPolicy::new(hotkey),
            recording: false,
            open: Vec::new(),
            closed: Vec::new(),
            next_session: 0,
        }
    }

    /// Whether a recording session is live.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Whether the hands-free badge should be showing.
    pub fn latched(&self) -> bool {
        self.latch.is_latched()
    }

    /// The session currently being recorded, if any.
    pub fn current_session(&self) -> Option<SessionId> {
        self.open.last().map(|s| s.id)
    }

    /// The phase of `id`, or `None` if this driver never knew it.
    pub fn phase_of(&self, id: SessionId) -> Option<SessionPhase> {
        if let Some(record) = self.open.iter().find(|s| s.id == id) {
            return Some(record.phase);
        }
        self.closed
            .iter()
            .rev()
            .find(|(closed, _)| *closed == id)
            .map(|(_, phase)| *phase)
    }

    /// Whether text belonging to `id` may still be typed, and if not, why.
    ///
    /// This is the boundary check, not a pre-flight one: it is asked *again*
    /// immediately before injection, because the answer can change while a
    /// result is being transcribed. `Recording` and `AwaitingResult` both accept
    /// — the second is the ordinary case for the final chunk.
    ///
    /// One method rather than a predicate plus a reason, because the reason is
    /// needed at exactly the one place the predicate is used, and a second
    /// lookup could disagree with the first if the session changed in between.
    pub fn accepts_result(&self, id: SessionId) -> Result<(), &'static str> {
        match self.phase_of(id) {
            Some(SessionPhase::Recording) | Some(SessionPhase::AwaitingResult) => Ok(()),
            Some(SessionPhase::Completed) => Err("late result of a completed session"),
            Some(SessionPhase::Cancelled) => Err("late result of a cancelled session"),
            None => Err("unknown session"),
        }
    }

    /// Hands out the next chunk id of `id`, or `None` if it cannot take one.
    pub fn next_chunk(&mut self, id: SessionId) -> Option<ChunkId> {
        let record = self.open.iter_mut().find(|s| s.id == id)?;
        if !matches!(
            record.phase,
            SessionPhase::Recording | SessionPhase::AwaitingResult
        ) {
            return None;
        }
        let chunk = ChunkId(record.next_chunk);
        record.next_chunk += 1;
        Some(chunk)
    }

    /// Opens a session, or reports why one could not be opened.
    ///
    /// A microphone that failed to open gets **no** id: there is nothing to
    /// transcribe, and an id that can never produce a result is a trap for
    /// whoever answers the "which session?" question later.
    pub fn open_session(&mut self, capture_open: bool) -> Option<SessionId> {
        if !capture_open {
            return None;
        }
        self.next_session += 1;
        let id = SessionId(self.next_session);
        let kind = if self.latch.is_latched() {
            SessionKind::HandsFree
        } else {
            SessionKind::PushToTalk
        };
        self.open.push(SessionRecord {
            id,
            kind,
            phase: SessionPhase::Recording,
            next_chunk: 1,
        });
        self.recording = true;
        Some(id)
    }

    /// The microphone stopped. The session is **not** closed: it is waiting for
    /// the last result, which is what almost every dictation does.
    pub fn recording_stopped(&mut self, id: SessionId) {
        if let Some(record) = self.open.iter_mut().find(|s| s.id == id) {
            record.phase = SessionPhase::AwaitingResult;
        }
        self.recording = false;
        self.latch.reset();
    }

    /// The session's final result arrived.
    pub fn result_arrived(&mut self, id: SessionId) {
        self.close(id, SessionPhase::Completed);
    }

    /// The user abandoned the session.
    pub fn cancelled(&mut self) {
        if let Some(id) = self.open.last().map(|s| s.id) {
            self.close(id, SessionPhase::Cancelled);
        }
        self.recording = false;
        self.latch.reset();
    }

    fn close(&mut self, id: SessionId, phase: SessionPhase) {
        let before = self.open.len();
        self.open.retain(|s| s.id != id);
        if self.open.len() != before {
            self.closed.push((id, phase));
            if self.closed.len() > REMEMBERED_CLOSURES {
                let drop_to = self.closed.len() - REMEMBERED_CLOSURES;
                self.closed.drain(..drop_to);
            }
        }
    }

    /// How a session was opened, for the report and for future policy.
    pub fn kind_of(&self, id: SessionId) -> Option<SessionKind> {
        self.open.iter().find(|s| s.id == id).map(|s| s.kind)
    }

    /// The record key went down.
    pub fn on_record_down(&mut self, now: Instant) -> Vec<Effect> {
        let action = self.latch.press(now, self.recording);
        self.decide(action)
    }

    /// The record key came up.
    pub fn on_record_up(&mut self, now: Instant) -> Vec<Effect> {
        let action = self.latch.release(now, self.recording);
        self.decide(action)
    }

    /// The user asked to abandon the utterance (Escape / tray).
    pub fn on_cancel(&mut self) -> Vec<Effect> {
        if !self.recording {
            return Vec::new();
        }
        self.recording = false;
        self.latch.reset();
        vec![Effect::DiscardSession]
    }

    /// The 20 ms tick.
    ///
    /// While recording this either closes an open tap or pumps the audio path.
    /// While idle it only ages the latch out, so a session that ended on some
    /// other path (VAD endpoint, cap, cancel) leaves no stale latch behind.
    pub fn on_tick(&mut self, now: Instant) -> Vec<Effect> {
        if !self.recording {
            self.latch.tick(now, false);
            return Vec::new();
        }
        let action = self.latch.tick(now, true);
        let mut effects = self.decide(action);
        if effects.is_empty() {
            effects.push(Effect::PollAudio);
        }
        effects
    }

    /// The machine opened (or failed to open) the microphone.
    ///
    /// Kept for callers that do not care about the identity; the id is returned
    /// here so the machine can thread it through the whole utterance.
    pub fn began_recording(&mut self, capture_open: bool) -> Option<SessionId> {
        self.open_session(capture_open)
    }

    /// The session ended on a path of the machine's choosing.
    ///
    /// Clearing the latch here rather than trusting the next idle tick is what
    /// makes "no stale latch survives a finalise" structural: a press arriving
    /// in the 20 ms before that tick used to be swallowed.
    pub fn ended_session(&mut self, id: SessionId) {
        self.recording_stopped(id);
    }

    /// The machine noticed the microphone is no longer live.
    ///
    /// Without this the session flag would lie after a capture device is closed
    /// mid-dictation, and the next press would find a "recording" already open.
    ///
    /// This is *not* a close: a device that died mid-dictation still ends with
    /// an empty buffer and nothing to type, so the session moves to
    /// [`SessionPhase::AwaitingResult`] like any other stop and its last result
    /// — if any — stays valid.
    pub fn note_capture_live(&mut self, live: bool) {
        self.recording = live;
        if !live {
            if let Some(id) = self.current_session() {
                self.recording_stopped(id);
            }
        }
    }

    /// Maps one latch decision onto the effects the loop performs.
    fn decide(&mut self, action: LatchAction) -> Vec<Effect> {
        let effects = match action {
            // A double-tap only raises the hands-free badge, and the machine
            // reads that from `latched()` after the batch — no effect needed.
            LatchAction::None | LatchAction::Latched | LatchAction::AwaitSecondTap => Vec::new(),
            LatchAction::Start => vec![Effect::BeginRecording],
            LatchAction::Finish => vec![Effect::FinishSession],
            LatchAction::FinishAndRestart => {
                vec![Effect::FinishSession, Effect::BeginRecording]
            }
        };
        // `FinishAndRestart` closes and re-opens, so the re-open is folded last.
        // These only move the *flags*; the id is handed out by
        // `open_session`/`recording_stopped` once the machine reports what
        // actually happened to the hardware.
        if effects.contains(&Effect::FinishSession) {
            self.recording = false;
        }
        effects
    }
}

/// The audio accumulated for the current utterance, plus how far the VAD has
/// read into it.
///
/// Bundled so the effect executor threads one argument through instead of
/// three that must be kept in step.
#[derive(Debug, Default)]
pub(crate) struct SessionBuffers {
    pub audio: Vec<f32>,
    pub vad_cursor: usize,
}

// ── the VAD read cursor ──────────────────────────────────────────────────

/// How many complete `frame`-sample windows sit between the read cursor and the
/// end of the buffer.
///
/// Each window is analysed exactly once: the cursor moves forward by what was
/// consumed, so the unconsumed tail is re-examined on the next poll and no
/// sample is ever fed twice (which would corrupt the Silero recurrent state and
/// scale the endpoint's sample count quadratically).
///
/// A zero `frame` yields 0 rather than looping forever on the caller's
/// `while` — `vad.chunk_size = 0` in a hand-edited config must not wedge the
/// tick at 100 % CPU.
pub(crate) fn frames_to_feed(cursor: usize, buffer_len: usize, frame: usize) -> usize {
    if frame == 0 || cursor >= buffer_len {
        return 0;
    }
    (buffer_len - cursor) / frame
}

// ── chunked streaming: is a mid-session chunk due? ───────────────────────

/// Chunked streaming: should the audio accumulated so far be flushed as a
/// mid-session chunk?
///
/// Pure (no state, no hardware) so the policy is unit-tested:
/// * never flush before `min_chunk_seconds`,
/// * always flush once the chunk reaches `chunk_seconds` — mid-phrase if the
///   speaker never pauses (a free cloud endpoint must never see a long upload),
/// * otherwise, in the `silence` strategy, flush at a real pause: enough trailing
///   silence **and** some actual speech inside this chunk.
pub(crate) fn should_flush_chunk(
    cfg: &StreamingSettings,
    sample_rate: u32,
    chunk_samples: usize,
    trailing_silence_samples: usize,
    speech_samples_in_chunk: usize,
) -> bool {
    if !cfg.enabled || sample_rate == 0 {
        return false;
    }
    let rate = f64::from(sample_rate);
    let chunk_secs = chunk_samples as f64 / rate;
    if chunk_secs < cfg.min_chunk_seconds as f64 {
        return false;
    }
    if chunk_secs >= cfg.chunk_seconds as f64 {
        return true;
    }
    if cfg.strategy.trim().eq_ignore_ascii_case("fixed") {
        return false;
    }
    let silence_secs = trailing_silence_samples as f64 / rate;
    // ≥ 250 ms of speech: a chunk of pure pause is not worth a round trip.
    let has_speech = speech_samples_in_chunk >= sample_rate as usize / 4;
    silence_secs * 1000.0 >= cfg.silence_ms as f64 && has_speech
}

/// The cut a completed chunk makes in the accumulation buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkSplit {
    /// Samples handed to the recogniser (`buffer[..keep_from]`).
    pub emit_len: usize,
    /// How many samples are dropped off the front.
    pub keep_from: usize,
    /// Where the VAD read cursor lands in the shortened buffer.
    pub cursor: usize,
}

/// Splits a finished chunk off the front of the buffer, keeping the configured
/// overlap tail so a seam cannot clip a word.
///
/// `cursor` moves back by the same amount as the buffer, so the retained tail
/// keeps its already-analysed position: the configured overlap is far larger
/// than a VAD frame, so those samples stay analysed and nothing is fed twice.
/// The clamp at the end is the invariant that makes it safe when the overlap is
/// misconfigured (`overlap_ms = 0`) or the chunk is shorter than the cursor —
/// the cursor may never point past the end of what is left, or those samples
/// would be skipped and never reach the VAD at all.
pub(crate) fn split_chunk(buffer_len: usize, cursor: usize, overlap: usize) -> ChunkSplit {
    let keep_from = buffer_len.saturating_sub(overlap);
    let remaining = buffer_len - keep_from;
    let cursor = cursor.saturating_sub(keep_from).min(remaining);
    ChunkSplit {
        emit_len: keep_from,
        keep_from,
        cursor,
    }
}

// ── does the utterance end here? ─────────────────────────────────────────

/// Why an utterance is being ended.
///
/// Named because "the speaker paused" and "the buffer filled" need different
/// fixes, and a bare `bool` in the middle of a 20-line expression hid which one
/// had fired from both the logs and the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalizeReason {
    /// Trailing silence ≥ `silence_timeout_ms` after speech (cutoff on hold).
    Silence,
    /// The ring buffer approached capacity — only while streaming is off.
    RingBufferFull,
    /// `max_utterance_seconds` reached: the hard safety net, always applies.
    UtteranceCap,
}

impl FinalizeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Silence => "silence",
            Self::RingBufferFull => "ring_buffer_full",
            Self::UtteranceCap => "utterance_cap",
        }
    }
}

/// The session's answer to "are we done yet?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointDecision {
    KeepRecording,
    Finalize(FinalizeReason),
}

/// The pure end-of-utterance test, in the order the reasons are checked.
///
/// Reaching the ring-buffer capacity no longer ends a *streaming* session: that
/// valve used to cut every dictation at ~30 s (``audio_secs = 30.01`` in the
/// logs). Long sessions are flushed as ordered chunks instead
/// ([`should_flush_chunk`]), so the valve only applies when streaming is
/// disabled, and the absolute `max_utterance_seconds` cap stays as a hard
/// safety net either way.
pub(crate) fn endpoint_decision(
    buffer_len: usize,
    endpoint_finalized: bool,
    cutoff_on_hold: bool,
    streaming_enabled: bool,
    capacity: usize,
    max_utterance_samples: u64,
) -> EndpointDecision {
    if cutoff_on_hold && endpoint_finalized {
        return EndpointDecision::Finalize(FinalizeReason::Silence);
    }
    if !streaming_enabled && buffer_len >= capacity {
        return EndpointDecision::Finalize(FinalizeReason::RingBufferFull);
    }
    if buffer_len as u64 >= max_utterance_samples {
        return EndpointDecision::Finalize(FinalizeReason::UtteranceCap);
    }
    EndpointDecision::KeepRecording
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::seam::{SeamOptions, SeamStitcher};

    fn hotkey() -> HotkeySettings {
        HotkeySettings::default()
    }

    /// Presses the record key and then reports that the microphone opened — what
    /// the machine does for one `Effect::BeginRecording`.
    ///
    /// The two are deliberately separate: the flag mirrors *hardware*, so the
    /// rules below that talk about "a recording is open" have to say so rather
    /// than lean on the key press having happened.
    fn press_and_open(d: &mut SessionDriver, at: Instant) -> SessionId {
        d.on_record_down(at);
        d.open_session(true).expect("the microphone opens")
    }

    /// Asserts the session is idle and hands-free-free, the state every
    /// "nothing to do" branch must leave behind.
    fn assert_quiet(d: &SessionDriver) {
        assert!(!d.is_recording());
        assert!(!d.latched());
    }

    // ── identity and lifecycle (S1) ──────────────────────────────────────

    /// The regression this whole section exists for. The final chunk's text
    /// arrives *after* the key is released, so a session that closes when the
    /// microphone stops rejects the normal result of every dictation. Both
    /// halves are asserted: the stop must keep the result acceptable, and the
    /// real close must not.
    #[test]
    fn stopping_the_microphone_does_not_close_the_session() {
        let mut d = SessionDriver::new(&hotkey());
        let id = d.open_session(true).expect("a session");
        assert_eq!(d.phase_of(id), Some(SessionPhase::Recording));

        d.next_chunk(id);
        d.recording_stopped(id);

        assert_eq!(
            d.phase_of(id),
            Some(SessionPhase::AwaitingResult),
            "the last chunk is still coming; the session is not finished"
        );
        assert!(
            d.accepts_result(id).is_ok(),
            "a result arriving after the key was released is the ordinary case"
        );

        d.result_arrived(id);
        assert_eq!(d.phase_of(id), Some(SessionPhase::Completed));
        assert!(
            d.accepts_result(id).is_err(),
            "a completed session takes no more text"
        );
        assert_eq!(
            d.accepts_result(id),
            Err("late result of a completed session")
        );
    }

    /// A cancelled session must stay silent even though its audio was real and
    /// its transcription may already be in flight.
    #[test]
    fn a_cancelled_session_refuses_its_own_late_result() {
        let mut d = SessionDriver::new(&hotkey());
        let id = d.open_session(true).unwrap();
        d.next_chunk(id);
        d.cancelled();

        assert_eq!(d.phase_of(id), Some(SessionPhase::Cancelled));
        assert_eq!(
            d.accepts_result(id),
            Err("late result of a cancelled session")
        );
        assert_quiet(&d);
    }

    /// Pressing record again before the first result arrives must not throw the
    /// first session away: the user spoke it.
    #[test]
    fn starting_a_new_session_keeps_the_previous_one_usable() {
        let mut d = SessionDriver::new(&hotkey());
        let first = d.open_session(true).unwrap();
        d.next_chunk(first);
        d.recording_stopped(first);

        let second = d.open_session(true).unwrap();
        assert_ne!(first, second, "ids are unique");
        assert_eq!(d.current_session(), Some(second));
        assert!(
            d.accepts_result(first).is_ok(),
            "the older session is waiting for its result, not cancelled"
        );
        assert_eq!(d.phase_of(first), Some(SessionPhase::AwaitingResult));
    }

    #[test]
    fn chunk_ids_run_from_one_within_a_session_and_reset_in_the_next() {
        let mut d = SessionDriver::new(&hotkey());
        let first = d.open_session(true).unwrap();
        assert_eq!(d.next_chunk(first), Some(ChunkId(1)));
        assert_eq!(d.next_chunk(first), Some(ChunkId(2)));
        assert_eq!(d.next_chunk(first), Some(ChunkId(3)));
        d.result_arrived(first);

        // A closed session cannot hand out more chunks.
        assert_eq!(d.next_chunk(first), None);

        let second = d.open_session(true).unwrap();
        assert_eq!(d.next_chunk(second), Some(ChunkId(1)));
    }

    /// A microphone that never opened must not produce an id: there is nothing
    /// to transcribe, and an id that can never answer is worse than none.
    #[test]
    fn a_failed_microphone_gets_no_identity() {
        let mut d = SessionDriver::new(&hotkey());
        assert_eq!(d.open_session(false), None);
        assert_eq!(d.current_session(), None);
        assert_eq!(d.phase_of(SessionId(1)), None);
        assert_eq!(d.accepts_result(SessionId(1)), Err("unknown session"));
    }

    /// The remembered-closure list is bounded: it must not grow for the life of
    /// the process, or a long-running app leaks a `Vec` entry per dictation.
    #[test]
    fn closed_sessions_are_remembered_only_recently() {
        let mut d = SessionDriver::new(&hotkey());
        let mut ids = Vec::new();
        for _ in 0..(REMEMBERED_CLOSURES + 4) {
            let id = d.open_session(true).unwrap();
            d.result_arrived(id);
            ids.push(id);
        }
        assert!(
            d.closed.len() <= REMEMBERED_CLOSURES,
            "kept {} closures",
            d.closed.len()
        );
        assert!(
            d.phase_of(ids[0]).is_none(),
            "the oldest closure has aged out and must read as unknown"
        );
        let newest = *ids.last().unwrap();
        assert_eq!(d.phase_of(newest), Some(SessionPhase::Completed));
    }

    #[test]
    fn a_latched_session_records_how_it_was_opened() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        // Two quick taps latch the recording hands-free. The first press has to
        // have a live session for the release to count as a tap at all — the
        // rules read the hardware flag, not "a key went down".
        press_and_open(&mut d, t0);
        assert_eq!(d.on_record_up(t0 + Duration::from_millis(30)), Vec::new());
        let second_down = t0 + Duration::from_millis(80);
        assert_eq!(d.on_record_down(second_down), Vec::new());
        assert!(d.latched());

        let id = d.open_session(true).unwrap();
        assert_eq!(d.kind_of(id), Some(SessionKind::HandsFree));
    }

    // ── the driver's own rules ───────────────────────────────────────────

    #[test]
    fn a_first_press_opens_the_microphone_and_a_release_closes_the_session() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let id = press_and_open(&mut d, t0);
        assert!(d.is_recording());

        // Hold-to-talk: the release ends the session.
        assert_eq!(
            d.on_record_up(t0 + Duration::from_millis(900)),
            vec![Effect::FinishSession]
        );
        d.ended_session(id);
        assert_quiet(&d);
    }

    /// The bug this pins: a press arriving in the 20 ms between "the VAD ended
    /// the session" and "the next idle tick" used to be eaten, because only the
    /// badge was cleared, never the latch itself.
    #[test]
    fn a_press_right_after_a_finalise_still_opens_a_fresh_recording() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        d.on_record_up(t0 + Duration::from_millis(90)); // short tap
        let _ = press_and_open(&mut d, t0 + Duration::from_millis(200)); // second tap
        assert!(d.latched(), "double-tap must go hands-free");

        // The VAD endpoint ends the session — no key involved.
        let id = d.open_session(true).unwrap();
        d.ended_session(id);
        assert!(!d.is_recording());
        // No tick has run yet. The very next press must still start a recording.
        assert_eq!(
            d.on_record_down(t0 + Duration::from_millis(500)),
            vec![Effect::BeginRecording]
        );
    }

    /// A second tap that lands too late to be a double-tap must still finalise
    /// the utterance already on the microphone before starting the next one.
    ///
    /// The finish step here is the one a copy-paste edit can silently drop, and
    /// dropping it loses everything the user had already said.
    #[test]
    fn a_late_second_tap_finalises_the_first_utterance_before_restarting() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        d.on_record_up(t0 + Duration::from_millis(90)); // short tap, window open
                                                        // 2 s later: far past the 600 ms double-tap window.
        let effects = d.on_record_down(t0 + Duration::from_millis(2_090));
        assert_eq!(effects, vec![Effect::FinishSession, Effect::BeginRecording]);
        // The restart is only a recording once the machine confirms the
        // microphone opened — between the effect and that confirmation the flag
        // is down, which is the honest answer.
        assert!(!d.is_recording());
        d.open_session(true);
        assert!(d.is_recording());
        assert!(!d.latched());
    }

    /// A second tap inside the window goes hands-free instead: the recording
    /// keeps running, so there is no finalise to schedule.
    #[test]
    fn a_double_tap_schedules_no_effects_and_raises_the_badge() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        d.on_record_up(t0 + Duration::from_millis(90));
        assert_eq!(d.on_record_down(t0 + Duration::from_millis(290)), vec![]);
        assert!(d.is_recording());
        assert!(d.latched());
    }

    /// The next press after a hands-free session ends it exactly once.
    #[test]
    fn a_latched_session_ends_on_the_next_press() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        d.on_record_up(t0 + Duration::from_millis(90));
        let _ = press_and_open(&mut d, t0 + Duration::from_millis(290)); // latched
                                                                         // Releasing must not stop it, and the tick must not time it out.
        assert_eq!(d.on_record_up(t0 + Duration::from_millis(380)), vec![]);
        assert_eq!(
            d.on_tick(t0 + Duration::from_secs(5)),
            vec![Effect::PollAudio]
        );
        // The next press is the one that ends it.
        assert_eq!(
            d.on_record_down(t0 + Duration::from_secs(6)),
            vec![Effect::FinishSession]
        );
        assert_quiet(&d);
    }

    #[test]
    fn a_press_while_a_recording_is_open_does_not_restart_it() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        assert_eq!(d.on_record_down(t0 + Duration::from_millis(400)), vec![]);
        assert!(d.is_recording());
    }

    #[test]
    fn a_held_session_keeps_pumping_audio_on_every_tick() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        for i in 1..=50 {
            assert_eq!(
                d.on_tick(t0 + Duration::from_millis(20 * i)),
                vec![Effect::PollAudio],
                "tick {i}"
            );
        }
        assert!(d.is_recording());
    }

    /// A middle chunk must never look like the end of the session, or the
    /// user loses the rest of a long dictation.
    #[test]
    fn an_idle_tick_never_touches_the_microphone() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        for i in 1..=10 {
            assert_eq!(d.on_tick(t0 + Duration::from_millis(20 * i)), vec![]);
        }
        assert_quiet(&d);
    }

    #[test]
    fn cancel_discards_a_live_session_but_is_ignored_while_idle() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        assert_eq!(d.on_cancel(), vec![]);

        let _ = press_and_open(&mut d, t0);
        d.on_record_up(t0 + Duration::from_millis(90));
        let _ = press_and_open(&mut d, t0 + Duration::from_millis(200));
        assert!(d.latched());
        assert_eq!(d.on_cancel(), vec![Effect::DiscardSession]);
        assert_quiet(&d);
    }

    /// Cancel must also drop a half-open tap, or the window would still time
    /// out and finalise a session the user already threw away.
    #[test]
    fn cancel_drops_a_pending_tap_so_no_tick_finalises_afterwards() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        d.on_record_up(t0 + Duration::from_millis(90)); // pending tap
        assert_eq!(d.on_cancel(), vec![Effect::DiscardSession]);
        for i in 1..=100 {
            assert_eq!(d.on_tick(t0 + Duration::from_millis(20 * i)), vec![]);
        }
    }

    /// A capture device that disappears mid-dictation must not leave the
    /// session flag claiming we are still recording.
    #[test]
    fn a_capture_that_dies_mid_session_reopens_on_the_next_press() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        let _ = press_and_open(&mut d, t0);
        d.note_capture_live(false);
        assert!(!d.is_recording());
        assert_eq!(
            d.on_record_down(t0 + Duration::from_millis(300)),
            vec![Effect::BeginRecording]
        );
    }

    #[test]
    fn a_failed_microphone_open_leaves_the_session_idle() {
        let mut d = SessionDriver::new(&hotkey());
        let t0 = Instant::now();
        assert_eq!(d.on_record_down(t0), vec![Effect::BeginRecording]);
        assert!(!d.is_recording(), "a press alone is not a recording");
        // The machine tried and the device refused: no id is handed out, so
        // nothing can later ask "which session was this?" about a session that
        // never existed.
        assert_eq!(d.began_recording(false), None);
        assert_quiet(&d);
    }

    // ── the VAD read cursor ──────────────────────────────────────────────

    #[test]
    fn the_vad_reads_every_window_exactly_once() {
        // 10 polls of 320 samples at a 512-sample window: 3200 samples in,
        // 6 complete windows, 128 samples of tail left for the next poll.
        let mut buf_len = 0usize;
        let mut cursor = 0usize;
        let mut fed = 0usize;
        for _ in 0..10 {
            buf_len += 320; // 20 ms of audio per simulated poll
            let frames = frames_to_feed(cursor, buf_len, 512);
            cursor += frames * 512;
            fed += frames;
        }
        assert_eq!(fed, 6, "1+2+3… re-feeding would give 21");
        assert_eq!(cursor, 3_072);
        assert_eq!(3_200 - cursor, 128, "unconsumed tail");
    }

    #[test]
    fn a_window_size_of_zero_reads_nothing_instead_of_looping_forever() {
        assert_eq!(frames_to_feed(0, 1_000, 0), 0);
    }

    #[test]
    fn a_cursor_at_the_buffer_end_reads_nothing() {
        assert_eq!(frames_to_feed(1_000, 1_000, 128), 0);
        assert_eq!(frames_to_feed(0, 0, 128), 0);
    }

    // ── cutting a chunk off the buffer ───────────────────────────────────

    #[test]
    fn a_chunk_cut_keeps_the_overlap_tail_for_the_next_one() {
        let s = split_chunk(16_000, 16_000, 4_800);
        assert_eq!(s.keep_from, 11_200);
        assert_eq!(s.emit_len, 11_200);
        // The tail was already analysed, so the cursor lands at its end.
        assert_eq!(s.cursor, 4_800);
    }

    /// The invariant that makes the cut safe: whatever the overlap, the cursor
    /// may never point past the samples that are left, or they would never be
    /// analysed at all.
    #[test]
    fn the_cursor_never_ends_up_past_the_retained_tail() {
        for len in [0usize, 1, 100, 512, 16_000] {
            for overlap in [0usize, 1, 480, 4_800, 60_000] {
                for cursor in [0usize, 1, 100, 512, 16_000] {
                    let s = split_chunk(len, cursor, overlap);
                    let remaining = len - s.keep_from;
                    assert!(s.cursor <= remaining, "len={len} ov={overlap} cur={cursor}");
                    assert!(s.emit_len == s.keep_from);
                    assert_eq!(s.keep_from, len.saturating_sub(overlap));
                }
            }
        }
    }

    /// With the overlap misconfigured to zero, the cut must not rewind the
    /// cursor past the start of the buffer and re-feed analysed audio.
    #[test]
    fn a_zero_overlap_does_not_rewind_the_cursor_below_zero() {
        let s = split_chunk(16_000, 0, 0);
        assert_eq!(s.emit_len, 16_000);
        assert_eq!(s.cursor, 0);
    }

    // ── end-of-utterance policy ──────────────────────────────────────────

    #[test]
    fn a_paused_speaker_ends_the_utterance_only_when_cutoff_is_on() {
        assert_eq!(
            endpoint_decision(1_000, true, true, true, 480_000, 9_600_000),
            EndpointDecision::Finalize(FinalizeReason::Silence)
        );
        assert_eq!(
            endpoint_decision(1_000, true, false, true, 480_000, 9_600_000),
            EndpointDecision::KeepRecording,
            "hold-to-talk must ignore the silence timeout"
        );
    }

    /// The regression proof for the old 30 s ceiling: with chunked streaming on,
    /// a full ring buffer is just another chunk, not the end of the dictation.
    #[test]
    fn a_full_ring_buffer_ends_a_session_only_when_streaming_is_off() {
        assert_eq!(
            endpoint_decision(480_000, false, true, false, 480_000, 9_600_000),
            EndpointDecision::Finalize(FinalizeReason::RingBufferFull)
        );
        assert_eq!(
            endpoint_decision(480_000, false, true, true, 480_000, 9_600_000),
            EndpointDecision::KeepRecording
        );
    }

    #[test]
    fn the_absolute_utterance_cap_ends_a_session_either_way() {
        let cap = 9_600_000u64; // 600 s at 16 kHz
                                // A ring buffer that cannot fill before the cap, so the cap is the
                                // backstop that fires rather than a competitor for it.
        let roomy = usize::MAX;
        for streaming in [true, false] {
            assert_eq!(
                endpoint_decision(cap as usize, false, true, streaming, roomy, cap),
                EndpointDecision::Finalize(FinalizeReason::UtteranceCap),
                "streaming={streaming}"
            );
            assert_eq!(
                endpoint_decision(cap as usize - 1, false, true, streaming, roomy, cap),
                EndpointDecision::KeepRecording,
                "streaming={streaming}"
            );
        }
    }

    #[test]
    fn every_finalize_reason_names_itself() {
        assert_eq!(FinalizeReason::Silence.as_str(), "silence");
        assert_eq!(FinalizeReason::RingBufferFull.as_str(), "ring_buffer_full");
        assert_eq!(FinalizeReason::UtteranceCap.as_str(), "utterance_cap");
    }

    // ── chunked streaming policy ─────────────────────────────────────────

    #[test]
    fn streaming_never_flushes_below_the_minimum_chunk() {
        let cfg = StreamingSettings::default();
        // 3 s of audio with a long pause: still too short to cut.
        assert!(!should_flush_chunk(&cfg, 16_000, 48_000, 16_000, 32_000));
    }

    #[test]
    fn streaming_always_flushes_at_the_hard_chunk_cap() {
        let cfg = StreamingSettings::default();
        let at_cap = (16_000 * cfg.chunk_seconds) as usize;
        // Never paused, never a word: the cap alone must cut, or a free cloud
        // endpoint would be sent the whole dictation.
        assert!(should_flush_chunk(&cfg, 16_000, at_cap, 0, 0));
        assert!(!should_flush_chunk(&cfg, 16_000, at_cap - 1, 0, 0));
    }

    #[test]
    fn streaming_cuts_at_a_pause_in_silence_strategy() {
        let cfg = StreamingSettings::default();
        let long_enough = (16_000 * cfg.min_chunk_seconds) as usize;
        let pause = (16_000 * cfg.silence_ms as usize) / 1000;
        let speech = 16_000usize; // 1 s of real speech in the chunk
        assert!(should_flush_chunk(&cfg, 16_000, long_enough, pause, speech));
        // Enough silence but no speech: a chunk of pure pause is not worth a
        // round trip.
        assert!(!should_flush_chunk(&cfg, 16_000, long_enough, pause, 0));
        // Speech but no pause yet.
        assert!(!should_flush_chunk(
            &cfg,
            16_000,
            long_enough,
            pause - 1,
            speech
        ));
    }

    #[test]
    fn streaming_fixed_strategy_ignores_pauses_but_keeps_the_cap() {
        let cfg = StreamingSettings {
            strategy: "fixed".into(),
            ..StreamingSettings::default()
        };
        let long_enough = (16_000 * cfg.min_chunk_seconds) as usize;
        let pause = (16_000 * cfg.silence_ms as usize) / 1000;
        assert!(!should_flush_chunk(
            &cfg,
            16_000,
            long_enough,
            pause,
            16_000
        ));
        let at_cap = (16_000 * cfg.chunk_seconds) as usize;
        assert!(should_flush_chunk(&cfg, 16_000, at_cap, 0, 0));
    }

    #[test]
    fn streaming_disabled_never_flushes() {
        let cfg = StreamingSettings {
            enabled: false,
            ..StreamingSettings::default()
        };
        assert!(!should_flush_chunk(&cfg, 16_000, 9_999_999, 16_000, 16_000));
        // A zero sample rate is not a division by zero waiting to happen.
        assert!(!should_flush_chunk(
            &StreamingSettings::default(),
            0,
            48_000,
            0,
            0
        ));
    }

    /// Long-dictation shape without hardware or network: feed the session the
    /// same policy the loop uses, apply the same cut rule, and stitch each
    /// chunk like the loop does.
    ///
    /// This is the regression proof for the old 30 s ceiling: the session is not
    /// ended, it is *cut into ordered chunks* — and they read as one text.
    #[test]
    fn a_sixty_second_session_is_chunked_and_stitched_in_order() {
        let cfg = StreamingSettings::default();
        let rate: u32 = 16_000;
        let frame = 240usize; // 15 ms of audio per simulated tick
        let overlap = rate as usize * cfg.overlap_ms as usize / 1000;

        let mut buffer = SessionBuffers::default();
        let mut elapsed_ms = 0u64;
        let mut cuts: Vec<u64> = Vec::new();
        let mut stitcher = SeamStitcher::new(SeamOptions::default());
        let mut typed = String::new();

        for _ in 0..4_000 {
            buffer.audio.resize(buffer.audio.len() + frame, 0.0);
            elapsed_ms += 15;
            // Unbroken speech: no pause is ever available, so only the hard cap
            // can cut — exactly the case that used to run into the valve.
            if !should_flush_chunk(&cfg, rate, buffer.audio.len(), 0, buffer.audio.len()) {
                continue;
            }
            cuts.push(elapsed_ms);
            let split = split_chunk(buffer.audio.len(), buffer.vad_cursor, overlap);
            let chunk: Vec<f32> = buffer.audio.drain(..split.keep_from).collect();
            buffer.vad_cursor = split.cursor;
            assert_eq!(chunk.len(), split.emit_len);

            let transcript = format!("قسمت {} ادامه دارد", cuts.len());
            let merge = stitcher.stitch(&transcript);
            if !merge.text.is_empty() {
                if !typed.is_empty() {
                    typed.push(' ');
                }
                typed.push_str(&merge.text);
            }
        }

        // Three cuts inside 60 s, one per chunk cap; the old safety valve would
        // have ended the whole session at 30 s, after the first one.
        assert_eq!(cuts.len(), 3, "cuts: {cuts:?}");
        // Frame-quantised, so allow the tick that crosses the cap.
        assert!(
            (20_000..20_100).contains(&cuts[0]),
            "first cut: {}",
            cuts[0]
        );
        assert!(
            (39_000..=40_000).contains(&cuts[1]),
            "second cut: {}",
            cuts[1]
        );
        assert!(cuts[2] >= 58_000, "third cut: {}", cuts[2]);
        // Past the old ceiling the microphone is still live and audio is still
        // accumulating (the overlap tail plus everything since the last cut).
        assert!(buffer.audio.len() > overlap);

        // Ordered, complete text — and no false dedupe between chunks that
        // happen to start with the same word.
        assert_eq!(
            typed,
            "قسمت 1 ادامه دارد قسمت 2 ادامه دارد قسمت 3 ادامه دارد"
        );
    }

    // ── hands-free latch policy ──────────────────────────────────────────

    #[test]
    fn latch_policy_hold_to_talk_is_unchanged() {
        let mut p = LatchPolicy::new(&hotkey());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        assert_eq!(
            p.release(t0 + Duration::from_millis(900), true),
            LatchAction::Finish
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_single_tap_waits_for_the_window_then_finalises() {
        let mut p = LatchPolicy::new(&hotkey());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        let tap_end = t0 + Duration::from_millis(80);
        assert_eq!(p.release(tap_end, true), LatchAction::AwaitSecondTap);
        // Still inside the double-tap window: keep holding the decision open.
        assert_eq!(
            p.tick(tap_end + Duration::from_millis(300), true),
            LatchAction::None
        );
        // Window expired with no second tap: behave like a plain tap did before.
        assert_eq!(
            p.tick(tap_end + Duration::from_millis(650), true),
            LatchAction::Finish
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_double_tap_goes_hands_free_until_the_next_press() {
        let mut p = LatchPolicy::new(&hotkey());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        let first_up = t0 + Duration::from_millis(90);
        assert_eq!(p.release(first_up, true), LatchAction::AwaitSecondTap);

        let second_down = first_up + Duration::from_millis(200);
        assert_eq!(p.press(second_down, true), LatchAction::Latched);
        assert!(p.is_latched());

        // Releasing the key must not stop a latched session…
        assert_eq!(
            p.release(second_down + Duration::from_millis(90), true),
            LatchAction::None
        );
        assert!(p.is_latched());
        // …and the tick keeps it alive (no pending tap to time out).
        assert_eq!(
            p.tick(second_down + Duration::from_secs(5), true),
            LatchAction::None
        );

        // The next press ends it.
        assert_eq!(
            p.press(second_down + Duration::from_secs(6), true),
            LatchAction::Finish
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_second_tap_too_late_restarts() {
        let mut p = LatchPolicy::new(&hotkey());
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        let first_up = t0 + Duration::from_millis(90);
        assert_eq!(p.release(first_up, true), LatchAction::AwaitSecondTap);
        // 2 s later is way past the 600 ms window.
        assert_eq!(
            p.press(first_up + Duration::from_millis(2_000), true),
            LatchAction::FinishAndRestart
        );
        assert!(!p.is_latched());
    }

    #[test]
    fn latch_policy_disabled_keeps_plain_hold_to_talk() {
        let hotkey = HotkeySettings {
            double_tap_latch: false,
            ..Default::default()
        };
        let mut p = LatchPolicy::new(&hotkey);
        let t0 = Instant::now();
        assert_eq!(p.press(t0, false), LatchAction::Start);
        // Even a very short tap finalises immediately when the latch is off.
        assert_eq!(
            p.release(t0 + Duration::from_millis(50), true),
            LatchAction::Finish
        );
    }
}
