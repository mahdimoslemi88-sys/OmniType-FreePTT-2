//! The one loop that decides what reaches the keyboard.
//!
//! Two things have to happen at once and used to fight each other: the engine
//! spends seconds turning audio into text, and the user can press Escape in the
//! middle of that. A loop that awaits the engine cannot read Escape; a loop that
//! spawns the engine and forgets the answer types text the user threw away.
//!
//! So the two are separated by *role*, not by flag:
//!
//! * a **worker** takes audio and gives back text. It touches no VAD, no seam,
//!   no keyboard, no history and no badge — so there is nothing for it to
//!   corrupt on the way to the answer.
//! * the **coordinator** owns the event stream *and* the result stream, asks
//!   whether each result is still wanted, and applies it. Every effect in the
//!   program is applied from [`Coordinator::apply`], which is what makes "a
//!   cancelled result changes nothing" a property rather than a hope.
//!
//! Work is queued and converted **one job at a time, in hand-out order**. That
//! is what an ending can be ordered against: the port saying "this utterance
//! held nothing worth converting" is still a job, so the session closes after
//! the chunks before it have settled instead of refusing their text as late.
//! The event loop is never blocked by a conversion — the engine runs on its own
//! task, and the loop goes straight back to reading input.
//!
//! The hardware sits behind [`Port`], so the same loop runs against the real
//! microphone in the app and against a scripted fake in the tests at the bottom
//! of this file. There is no second implementation: the tests drive *this* loop.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use anyhow::Result;
use futures_util::FutureExt;

use crate::asr::engine::AudioUtterance;
use crate::asr::router::AsrRouter;
use crate::config::settings::Settings;
use crate::hotkey::HotkeyEvent;
use crate::output::target::{TargetIdentity, TargetValidity};
use crate::output::Injection;
use crate::processing::boundary::BoundaryTracker;
use crate::processing::seam::{SeamMerge, SeamOptions, SeamStitcher};
use crate::state::review::{self, DraftKind, PendingDraft, ReviewCommand, ReviewOutcome};
use crate::state::review_channel::ReviewChannel;
use crate::processing::{Dictionary, Normalizer};
use crate::profiles::{effective, EffectiveRules, GeneralRules};
use crate::state::session::{ChunkId, Effect, SessionDriver, SessionId};
use crate::state::status::{AppState, StatusChannel};
use crate::state::utterance::{
    is_transient, judge_insert, plan_typing, InjectOutcome, TypePlan, ERROR_READABLE,
};

/// Where typed text goes.
///
/// A seam so a test can see whether the keyboard was touched: `SendInput` needs
/// a desktop session, and a test that cannot observe that cannot assert silence.
///
/// Both methods **report** what the platform took rather than raising. A send
/// that was only partly accepted is a fact the loop has to react to \u2014 stop,
/// say how much went out, keep the text, do not ask again \u2014 and an error it
/// could rethrow would carry none of that. The verdict those reports make is
/// [`judge_insert`].
pub(crate) trait TextSink: Send + Sync {
    fn type_text(&self, text: &str) -> Injection;
    fn backspace(&self, count: usize) -> Injection;
}

/// The real sink: the keystrokes the app has always sent.
pub(crate) struct KeystrokeSink {
    /// How to deliver text, read from the user's settings at startup.
    ///
    /// `pub(crate)` rather than private because `state::machine` builds this
    /// struct: the pacing decision is made where the settings are in hand, so
    /// the sink does not have to reach back into configuration on every call.
    ///
    /// Held here rather than read at each call because the sink is constructed
    /// once and shared, and the pacing choice is a startup decision: changing it
    /// mid-dictation would mean two different delivery styles inside one
    /// sentence.
    pub(crate) pacing: TypePacing,
}

/// Whether a dictation arrives in one burst or in paced steps.
///
/// Two values rather than a bool because "paced" is only meaningful together
/// with how fast, and a `bool` plus two loose numbers is the shape that lets the
/// two drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TypePacing {
    /// The default, and the reason it is the default: burst delivery is what
    /// this app has always done, so a missing or unknown setting must land on
    /// the behaviour every existing install already has.
    #[default]
    Burst,
    /// `step_chars` characters, then `step_ms` between groups.
    Paced {
        step_chars: usize,
        step_ms: std::time::Duration,
    },
}

impl TypePacing {
    /// Reads the user's choice, clamping both numbers to a usable range.
    pub(crate) fn from_settings(gui: &crate::config::settings::GuiSettings) -> Self {
        if !gui.type_progressively {
            return TypePacing::Burst;
        }
        let (step_chars, step_ms) = gui.type_step();
        TypePacing::Paced {
            step_chars,
            step_ms,
        }
    }
}

impl TextSink for KeystrokeSink {
    fn type_text(&self, text: &str) -> Injection {
        match self.pacing {
            TypePacing::Burst => crate::output::inject_text(text),
            TypePacing::Paced {
                step_chars,
                step_ms,
            } => crate::output::inject_text_paced(text, step_chars, step_ms),
        }
    }
    fn backspace(&self, count: usize) -> Injection {
        crate::output::inject_backspaces(count)
    }
}

/// Which window the text is allowed to go into, asked twice.
///
/// The destination is read **when recording starts** and re-checked
/// **immediately before the first key goes out**, because those are the only
/// two moments that mean anything: `SendInput` has no idea where it is typing,
/// so a slow transcription must be judged against the window the user was in
/// when they started, not against whatever is in front when the text is ready.
///
/// A seam, like [`Port`] and [`TextSink`], because both halves need a desktop
/// session to answer honestly. What the *decision* is does not live here: it is
/// [`crate::output::classify`] in both the app and the tests, so a fake can
/// only lie about what it observed, never about what that means.
pub(crate) trait TargetPort: Send + Sync {
    /// The window in front of the user right now, or `None` when there is none
    /// to name. A dictation with no capturable destination is **not** an error:
    /// the text is still produced, and the insert is refused later with a
    /// reason.
    fn capture(&self) -> Option<TargetIdentity>;
    /// Whether `expected` is still the window in front of the user.
    fn check(&self, expected: &TargetIdentity) -> TargetValidity;
}

/// The real question, asked of Windows: three calls, no decision of its own.
pub(crate) struct WindowTargets;

impl TargetPort for WindowTargets {
    fn capture(&self) -> Option<TargetIdentity> {
        crate::output::capture_target().ok()
    }
    fn check(&self, expected: &TargetIdentity) -> TargetValidity {
        crate::output::validate_target(expected)
    }
}

/// What one poll of the hardware produced.
///
/// The VAD judgement happens on the port, before any audio is handed to a
/// worker: the worker is told what it is looking at, never asked to decide.
#[derive(Debug)]
pub(crate) enum PollOutcome {
    /// Nothing finished yet.
    More,
    /// A mid-session piece. Recording continues.
    Chunk { audio: AudioUtterance },
    /// An endpoint, a safety cap, or the key coming up. Recording stops.
    ///
    /// `audio: None` is the port saying it already judged this utterance not
    /// worth an engine call (a tap, a breath). The session still ends — it just
    /// has nothing to convert.
    Finished { audio: Option<AudioUtterance> },
}

/// The hardware side, as the coordinator needs it.
pub(crate) trait Port: Send {
    /// Opens the microphone. `None` is success; `Some(reason)` is a device that
    /// refused, which is a fact the session rules need to hear rather than an
    /// error to raise.
    async fn begin(&self) -> Option<String>;
    /// Pumps the buffer and runs the VAD over it.
    async fn poll(&self) -> Result<PollOutcome>;
    /// Stops capture and hands back what was accumulated.
    async fn finish(&self) -> Result<Option<AudioUtterance>>;
    /// Stops capture and throws the buffer away.
    async fn discard(&self) -> Result<()>;
    /// Whether the microphone is still live — read after a mid-session chunk so
    /// a device that vanished mid-dictation is noticed here rather than on the
    /// next key press.
    fn capture_live(&self) -> bool;
}

/// One thing asking the coordinator to do something.
pub(crate) enum Input {
    /// A hotkey or tray event.
    Event(HotkeyEvent),
    /// The 20 ms heartbeat, so a held key keeps pumping audio.
    Tick,
}

/// The parts of a conversion that are not the hardware: the engine chain and
/// the text rules.
///
/// Bundled so the coordinator — and every test that builds one — passes them as
/// a single value instead of four that have to agree with each other.
#[derive(Clone)]
pub(crate) struct Speech {
    pub router: Arc<AsrRouter>,
    pub normalizer: Arc<Normalizer>,
    pub dictionary: Arc<RwLock<Dictionary>>,
    pub settings: Arc<Settings>,
}

/// The loop's clock.
///
/// It exists so that "the failure stays readable for three seconds" is a loop
/// decision a test can move by hand, rather than a background timer that writes
/// the status channel on its own. The loop only asks *when*; the answer to "is
/// this error still the active one?" is the loop's own business.
pub(crate) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    /// Resolves once `deadline` has passed; never, when there is none.
    ///
    /// `Option` because most of the time there is no deadline, and a disabled
    /// `select!` branch that must still build its future is a trap: an
    /// always-pending future says "no deadline" without a precondition.
    fn sleep_until(&self, deadline: Option<Instant>) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// The real clock: wall time for the loop, a Tokio timer to wake it.
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep_until(&self, deadline: Option<Instant>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        match deadline {
            Some(at) => Box::pin(tokio::time::sleep_until(tokio::time::Instant::from_std(at))),
            None => Box::pin(std::future::pending()),
        }
    }
}

/// What a worker hands back. Pure data: no effect has been applied to it yet.
#[derive(Debug)]
pub(crate) struct ConversionResult {
    /// Hand-out order. Results are applied in this order, so a final chunk that
    /// comes back first waits for the pieces it belongs after.
    seq: u64,
    session: Option<SessionId>,
    chunk: Option<ChunkId>,
    /// The window **this** session captured when its recording started.
    ///
    /// Carried with the work rather than looked up at effect time: the answer
    /// belongs to the dictation that produced it, and a lookup of "the window
    /// captured last" is the bug this field exists to make impossible.
    target: Option<TargetIdentity>,
    /// The engine's own string, kept because "the recogniser said nothing" and
    /// "the seam ate everything" are different faults and the logs say which.
    raw: String,
    /// The same text after the normalizer and the dictionary.
    processed: String,
    is_final: bool,
    failure: Option<String>,
}

/// One piece of work, waiting for its turn.
///
/// `audio: None` is the port saying the ending utterance held nothing worth a
/// conversion (a tap, a breath). It still takes its place in the order: that is
/// what makes "the session closes only after its earlier chunks settled" true,
/// instead of a final that closes it immediately and leaves a converted
/// mid-chunk with nowhere to go.
struct Job {
    seq: u64,
    session: Option<SessionId>,
    chunk: Option<ChunkId>,
    /// The window captured when this session's recording started. Copied in at
    /// hand-out so the work carries its own destination.
    target: Option<TargetIdentity>,
    audio: Option<AudioUtterance>,
    is_final: bool,
}

/// The one conversion running right now.
///
/// The handle is kept — never dropped on the floor — because a task that died
/// without sending an answer has to be *noticed*, not waited for forever: see
/// [`Coordinator::reap_in_flight`].
struct InFlight {
    seq: u64,
    session: Option<SessionId>,
    chunk: Option<ChunkId>,
    is_final: bool,
    handle: tokio::task::JoinHandle<()>,
}

/// One error's readable window: which error owns the badge, and when it ends.
///
/// A list rather than one slot because every window is its *own* loop event: a
/// window that a newer failure superseded still closes, and must then find that
/// its error is no longer the active one.
#[derive(Debug, Clone)]
struct ErrorWindow {
    /// Errors are numbered in the order they are shown, from 1, so an expiry
    /// can name exactly which error it is about.
    version: u64,
    deadline: Instant,
    /// The exact message the window was armed for. A window may only clear the
    /// error it was armed for — a newer failure that replaced it, or a
    /// recording that started after it, is somebody else's to keep.
    message: String,
}

/// Turns audio into text and nothing else.
///
/// The worker is deliberately blind: no seam, no sink, no status, no session
/// rules. Everything a cancelled result could have damaged lives on the other
/// side of the channel, so "a cancelled result changed nothing" is true by
/// construction rather than by remembering to check at each of the five steps a
/// conversion used to walk through.
/// The rules one dictation runs under, resolved from the destination it was
/// aimed at.
///
/// The destination is the **carried** one: captured when the microphone really
/// opened, and copied into the job when the work was handed out. Never the
/// foreground window at this moment. That is what makes a profile a property of
/// the dictation rather than of the instant — a user who switches windows while
/// a long utterance is being transcribed cannot have its later chunks governed
/// by the rules of the window they moved to.
///
/// A destination whose executable could not be read (`exe_path: None`) is an
/// application this program cannot name, and it gets the general rules. Nothing
/// here consults the window title, which changes as the user types and would
/// therefore stop matching halfway through the document the profile was made
/// for.
///
/// Deliberately **one** decision point: the conversion path and the insert path
/// both call this, so "which rules applied to this text" cannot have two
/// answers that disagree.
pub(crate) fn rules_for(settings: &Settings, target: Option<&TargetIdentity>) -> EffectiveRules {
    effective(
        &settings.profiles,
        target.and_then(|t| t.exe_path.as_deref()),
        GeneralRules::from(settings),
    )
}

/// Runs the text pipeline under `rules`.
///
/// Two stages, in this order: the general pipeline at the profile's mode, then
/// the profile's own correction rules.
///
/// The profile's rules are applied **even in `Raw` mode**. `Raw` silences the
/// built-in pipeline — the normalizer and the general dictionary — because a
/// terminal wants the recogniser's string verbatim. It does not silence a rule
/// the user wrote for that terminal by hand: an explicit instruction outranks a
/// mode chosen to suppress *implicit* rewriting. The other way round, a panel
/// that accepts a correction in a raw profile would never apply it, which reads
/// as the feature being broken.
fn process_with_rules(
    text: &str,
    normalizer: &Normalizer,
    dictionary: &Dictionary,
    rules: &EffectiveRules,
) -> String {
    // Delegated rather than reimplemented: [`crate::processing::TextRules`] is
    // the one place that knows the order, and the dictionary panel's quick-fix
    // preview runs the same value to show the user what a rule will do.
    crate::processing::TextRules {
        mode: rules.mode,
        normalizer,
        dictionary,
        corrections: &rules.corrections,
        commands: rules.commands,
        formal: rules.formal,
    }
    .apply(text)
}

pub(crate) async fn convert(
    speech: Speech,
    seq: u64,
    session: Option<SessionId>,
    chunk: Option<ChunkId>,
    target: Option<TargetIdentity>,
    audio: AudioUtterance,
    is_final: bool,
) -> ConversionResult {
    let mut result = ConversionResult {
        seq,
        session,
        chunk,
        target,
        raw: String::new(),
        processed: String::new(),
        is_final,
        failure: None,
    };
    match speech.router.transcribe(&audio).await {
        Ok(raw) => {
            // Resolved per conversion from the destination this job carries,
            // so every chunk of one dictation is judged against the window the
            // dictation started in. Read back off the result: `target` itself
            // was moved into it above.
            let rules = rules_for(&speech.settings, result.target.as_ref());
            if rules.is_profiled() {
                // Only when a profile matched: a user with no profiles sees no
                // new lines, and a user with one can see which rules ran.
                tracing::info!(
                    seq,
                    profile = rules.profile.as_deref().unwrap_or(""),
                    mode = rules.mode.as_str(),
                    review = rules.review_before_insert,
                    "application profile in force"
                );
            }
            let processed = {
                let dict = speech
                    .dictionary
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                process_with_rules(&raw, &speech.normalizer, &dict, &rules)
            };
            result.raw = raw;
            result.processed = processed;
        }
        Err(e) => result.failure = Some(format!("{e:#}")),
    }
    result
}

/// A preserved record of text that was not completely delivered to its destination.
///
/// Preserved across normal session completion ([`Coordinator::settle`]) so that
/// undelivered chunks or focus changes do not lose user dictation.
/// Only explicit cancellation ([`HotkeyEvent::RecordCancel`]) drops the records
/// belonging to that cancelled session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeptRecord {
    pub session: Option<SessionId>,
    pub chunk: Option<ChunkId>,
    /// Sequential order in which this insert was evaluated.
    pub seq: u64,
    /// The text that was planned to be typed.
    pub planned_text: String,
    /// The target window this dictation was addressed to, if captured.
    pub destination: Option<TargetIdentity>,
    /// The overall outcome of the injection attempt.
    pub outcome: InjectOutcome,
    /// Report from the seam backspace erase step, if backspaces were planned.
    pub backspace: Option<Injection>,
    /// Report from the text typing step, if text typing was attempted.
    pub text: Option<Injection>,
}

#[allow(dead_code)]
impl KeptRecord {
    /// Whether the text was entirely unaccepted by the platform from the standpoint of text delivery.
    ///
    /// For [`InjectOutcome::NotAttempted`] and [`InjectOutcome::Failed`], 0 keystroke
    /// pairs were accepted, so the planned text is wholly undelivered.
    /// For [`InjectOutcome::Partial`], if text delivery was never attempted (e.g. backspace
    /// failed or stopped early) or zero text events were accepted, the planned text is also
    /// wholly undelivered with respect to text delivery. If some text events were accepted,
    /// calling the entire planned text "undelivered" would be inaccurate.
    pub fn is_wholly_undelivered(&self) -> bool {
        match self.outcome {
            InjectOutcome::Complete { .. } => false,
            InjectOutcome::NotAttempted | InjectOutcome::Failed => true,
            InjectOutcome::Partial { .. } => {
                // If text delivery was never attempted (e.g. backspace stopped before text),
                // or if 0 text events were accepted, the planned text is wholly undelivered.
                self.text.as_ref().map(|t| t.accepted == 0).unwrap_or(true)
            }
        }
    }

    /// Returns the unaccepted suffix of the planned text, or `None` if the boundary
    /// is uncertain or indeterminate.
    ///
    /// # Uncertainty Rules
    /// 1. If text injection has an odd `accepted` count (e.g. only the first key-down
    ///    was accepted without a matching key-up, or a torn event pair), the platform/window
    ///    state is indeterminate. Neither the whole text nor any suffix can be reliably known
    ///    or recovered. Returns `None`.
    /// 2. If the accepted UTF-16 code units boundary falls inside a surrogate pair (e.g.
    ///    half of an emoji was accepted), the character boundary is torn and indeterminate.
    ///    Returns `None`.
    /// 3. If text delivery was never attempted (e.g. destination refused, or backspaces
    ///    failed/stopped before text was sent) or 0 text events were accepted, the entire
    ///    planned text is unaccepted with respect to text delivery. Returns `Some(&self.planned_text)`.
    ///    Any separate effect of backspaces is recorded in `self.backspace`.
    /// 4. If an even number of events was accepted and falls on a complete UTF-16 code unit
    ///    (and Unicode character) boundary, returns `Some(unaccepted_suffix)`.
    ///
    /// **Note on guarantees:** This output reflects event acceptance by the platform's
    /// `SendInput`, which is the boundary of our honest knowledge, not proof of characters
    /// sitting inside the target document.
    pub fn unaccepted_text(&self) -> Option<&str> {
        match self.outcome {
            InjectOutcome::NotAttempted | InjectOutcome::Failed => Some(&self.planned_text),
            InjectOutcome::Partial { .. } => {
                let text_report = match &self.text {
                    Some(t) => t,
                    None => {
                        // Text delivery was not attempted (e.g. backspaces failed/stopped early).
                        // From the standpoint of text typing, the entire text was unaccepted.
                        // Any separate effect of backspaces is recorded in `self.backspace`.
                        return Some(&self.planned_text);
                    }
                };
                if text_report.accepted % 2 != 0 {
                    // An odd number of accepted events means an incomplete key-event pair
                    // (e.g. key-down accepted without matching key-up).
                    // The platform/window state is indeterminate; neither the whole text nor
                    // any suffix can be reliably known or recovered.
                    return None;
                }
                let accepted_units = text_report.accepted / 2;
                if accepted_units == 0 {
                    return Some(&self.planned_text);
                }
                let mut accumulated_utf16 = 0;
                for (byte_idx, ch) in self.planned_text.char_indices() {
                    if accumulated_utf16 == accepted_units {
                        // Exactly on a character boundary (and outside any surrogate pair).
                        return Some(&self.planned_text[byte_idx..]);
                    }
                    let ch_len = ch.len_utf16();
                    if accepted_units > accumulated_utf16
                        && accepted_units < accumulated_utf16 + ch_len
                    {
                        // The accepted count lands inside a surrogate pair (e.g. half of an emoji).
                        // The character is torn and indeterminate.
                        return None;
                    }
                    accumulated_utf16 += ch_len;
                }
                if accumulated_utf16 == accepted_units {
                    return Some("");
                }
                None
            }
            InjectOutcome::Complete { .. } => Some(""),
        }
    }
}

/// The coordinator: event stream in, keystrokes out.
pub(crate) struct Coordinator<P: Port> {
    port: P,
    speech: Speech,
    /// The session's own rules — latch, recording flag, effect plan. A plain
    /// mutex: every question asked of it is a handful of integer comparisons,
    /// and nothing here ever holds it across an `await`.
    session: Mutex<SessionDriver>,
    /// Chunk-seam repair state, one stitcher per session.
    ///
    /// Keyed by identity on purpose: a new dictation starting must not reset
    /// the stitcher an older one still needs, and an older dictation's tail
    /// must never be used to delete the new one's words. `Option<SessionId>`
    /// because a sessionless answer (a test handing the queue a result
    /// directly) still gets a seam of its own. Plain mutex for the same reason
    /// as the session lock: `stitch` is pure string work.
    seams: Mutex<HashMap<Option<SessionId>, SeamStitcher>>,
    /// The window each session captured, keyed by identity — one per
    /// session, not one for the process.
    ///
    /// A single shared tracker replaced by the next dictation is not enough, and
    /// the reason is the case this map exists for: an older dictation's answer
    /// can land *after* a newer one has started, and it must be judged against
    /// the window it was dictated into. With one shared destination the older
    /// text is refused for a window it never targeted — or, worse, accepted
    /// because somebody else's capture says the same hwnd.
    targets: Mutex<HashMap<Option<SessionId>, TargetIdentity>>,
    /// Preserved records of undelivered or action-needed text.
    ///
    /// Decoupled from active session lifespan: normal session completion does
    /// not drop these records, so an insert refused or broken late in the turn
    /// is not lost. Explicit cancel drops records for that session only.
    kept: Arc<Mutex<Vec<KeptRecord>>>,
    /// Continuity and spacing policy across chunks and continued sessions.
    boundary: Mutex<BoundaryTracker>,
    status: Arc<StatusChannel>,
    sink: Arc<dyn TextSink>,
    /// Which window may still receive this dictation's text.
    ///
    /// Named `desktop` rather than `windows`, which the error list already owns.
    desktop: Arc<dyn TargetPort>,
    /// Where workers hand their answers back. Separate from the event stream so
    /// a slow engine never delays a key press by even one tick.
    results: tokio::sync::mpsc::UnboundedReceiver<ConversionResult>,
    /// The workers' side of `results`. Held here so a spawn does not have to go
    /// looking for it — and so the receiver above can never see a closed channel
    /// while this loop is still running.
    results_tx: tokio::sync::mpsc::UnboundedSender<ConversionResult>,
    /// Next hand-out number. Assigning it when the work is handed over, not when
    /// it comes back, is what makes "apply in order" mean what it says.
    next_seq: u64,
    /// Answers that arrived early, keyed by the order they belong in.
    pending: BTreeMap<u64, ConversionResult>,
    /// The next order number allowed to touch the keyboard.
    apply_next: u64,
    /// Order numbers of queued jobs that were set aside before they ran.
    ///
    /// They will never produce a result, so the ordering gate has to step over
    /// them; otherwise the next real answer waits forever for a number that is
    /// never coming — the very stall the queue must not have.
    skipped: BTreeSet<u64>,
    /// Work waiting for its turn. One conversion runs at a time, so a cancel
    /// still has unstarted audio it can set aside.
    queue: VecDeque<Job>,
    /// The single conversion running right now, if any.
    in_flight: Option<InFlight>,
    /// Failures whose readable window has not closed yet, oldest first.
    windows: Vec<ErrorWindow>,
    /// Errors are numbered in the order they are shown, from 1; a window
    /// carries the number of the error it was armed for.
    error_version: u64,
    /// The loop's clock; see [`Clock`].
    clock: Arc<dyn Clock>,
    /// Text waiting on the user, and the wire the dashboard answers over.
    ///
    /// Held as an `Arc` and handed to the GUI, so both sides share **one**
    /// store. Two stores would each be able to say "nothing is pending" while
    /// the other was holding a dictation.
    review: Arc<ReviewChannel>,
    /// The answers, in the order the user gave them.
    ///
    /// A separate channel from the hotkey stream rather than an `Input`
    /// variant, so that the "the user is answering about text" path cannot be
    /// confused with the "the user is talking" path: one of them may type and
    /// the other must not.
    answers: tokio::sync::mpsc::UnboundedReceiver<ReviewCommand>,
}

impl<P: Port> Coordinator<P> {
    /// Builds the loop from its collaborators.
    ///
    /// `#[allow(clippy::too_many_arguments)]` rather than another bundle: the
    /// collaborators already fall into three honest groups (hardware in `port`,
    /// the text pipeline in `speech`, and the loop's own seams), and a fourth
    /// wrapper around `status`/`sink`/`desktop`/`clock` would only hide that
    /// they are four different things. Same call `OverlayApp::new` makes.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        port: P,
        speech: Speech,
        session: SessionDriver,
        status: Arc<StatusChannel>,
        sink: Arc<dyn TextSink>,
        desktop: Arc<dyn TargetPort>,
        clock: Arc<dyn Clock>,
        review: Arc<ReviewChannel>,
    ) -> Self {
        let (results_tx, results) = tokio::sync::mpsc::unbounded_channel();
        // Taken here, once. A second loop that could resolve answers would be a
        // second owner of the keyboard, which is the one thing this program
        // insists on having exactly one of.
        let answers = review
            .take_receiver()
            .expect("the review wire hands its answers to exactly one loop");
        Self {
            port,
            speech,
            session: Mutex::new(session),
            seams: Mutex::new(HashMap::new()),
            targets: Mutex::new(HashMap::new()),
            kept: Arc::new(Mutex::new(Vec::new())),
            boundary: Mutex::new(BoundaryTracker::new()),
            status,
            sink,
            desktop,
            results,
            results_tx,
            next_seq: 0,
            pending: BTreeMap::new(),
            apply_next: 0,
            skipped: BTreeSet::new(),
            queue: VecDeque::new(),
            in_flight: None,
            windows: Vec::new(),
            error_version: 0,
            clock,
            review,
            answers,
        }
    }

    /// Whether a finished text should be shown before it is typed.
    ///
    /// Read from the live settings rather than captured at startup, because the
    /// dashboard offers this as a switch and a user who flips it should not
    /// have to restart to feel it.
    ///
    /// The matched profile wins over the general setting, and the match is made
    /// against the destination this dictation carries: a profile that asks for
    /// review in one application holds its text there even when the general
    /// switch is off, and a profile that turns it off inserts straight into the
    /// windows it names.
    fn review_enabled(&self, target: Option<&TargetIdentity>) -> bool {
        rules_for(&self.speech.settings, target).review_before_insert
    }

    /// How long a draft may wait before the app stops offering it.
    fn draft_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.speech.settings.gui.draft_ttl_secs())
    }

    /// Provides a clone of the shared handle to kept action-needed records.
    #[allow(dead_code)]
    pub fn kept_handle(&self) -> Arc<Mutex<Vec<KeptRecord>>> {
        self.kept.clone()
    }

    /// Returns a snapshot of the records held for undelivered / action-needed text.
    #[allow(dead_code)]
    pub fn kept_records(&self) -> Vec<KeptRecord> {
        self.kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Records successful text injection in boundary memory.
    fn record_boundary(
        &self,
        text: &str,
        session: Option<SessionId>,
        target: Option<TargetIdentity>,
    ) {
        let mut boundary = self
            .boundary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        boundary.record_success(text, session, target);
    }

    /// Invalidates boundary memory.
    fn forget_boundary(&self) {
        let mut boundary = self
            .boundary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        boundary.invalidate();
    }

    /// Runs until Quit, or until the event channel closes.
    ///
    /// The heartbeat is an [`Input`] like any other message, and the callers
    /// produce it (`state::machine` beats every 20 ms; a test beats by hand).
    /// Nothing inside this loop knows where a tick came from, which is what lets
    /// a test drive the real loop without guessing at durations.
    pub async fn run(
        mut self,
        mut input: tokio::sync::mpsc::UnboundedReceiver<Input>,
    ) -> Result<()> {
        self.status.set_state(AppState::Idle);

        // Why the loop stopped, so the exit log can say it: a source that died
        // and a user who quit are different bugs to whoever reads the log next.
        let mut stop = "event channel closed";

        loop {
            let clock = self.clock.clone();
            // A draft's deadline joins the error windows' as another way this
            // loop can be woken. One `sleep_until` rather than two: the earliest
            // deadline is the only one that matters, and two timers would mean
            // two places where the loop could be late.
            let deadline = self
                .windows
                .iter()
                .map(|w| w.deadline)
                .chain(self.next_draft_deadline())
                .min();
            tokio::select! {
                // `biased`, with the event stream first: the policy is that the
                // user decides what happens next, and an answer already waiting
                // in the channel takes one turn. Without it, `select!` picks
                // pseudo-randomly between two ready branches, so that policy
                // would hold only by luck.
                //
                // A caveat worth keeping, because it is the kind of thing this
                // repo has been wrong about: **this line cannot be measured from
                // outside.** Deleting it left `nothing_is_typed_after_quit` green
                // for 240 rounds, because the loop parks here long before a
                // worker gets far enough to send. So the line is justified by the
                // policy it states, not by a test that would fail without it.
                //
                // A closed channel is the shutdown signal: there is no other way
                // to say "no more events will arrive", and inventing an explicit
                // one would only give the loop a second way to be wrong.
                biased;
                message = input.recv() => match message {
                    None => break,
                    Some(Input::Event(HotkeyEvent::Quit)) => {
                        stop = "quit requested";
                        break;
                    }
                    Some(Input::Event(ev)) => self.on_event(ev).await?,
                    Some(Input::Tick) => self.on_tick().await?,
                },
                // The result branch can never be disabled: `results_tx` is held
                // here, so the channel stays open for the life of the loop.
                Some(result) = self.results.recv() => self.drain_in_order(result).await,
                // The user answering a draft. Below the event stream on purpose:
                // a key press outranks a click on a window, and an answer that
                // waits one turn behind a keystroke is not an answer the user
                // can see was lost.
                //
                // `Some(...)` rather than a bare pattern because this channel
                // **does** close: the dashboard is the only holder of the
                // sender, and a closed window means no more answers. That is
                // not a shutdown signal — the loop keeps dictating — so this
                // arm has to be disabled rather than break the loop.
                answer = self.answers.recv(), if !self.answers.is_closed() => {
                    if let Some(command) = answer {
                        self.on_review(command).await;
                    }
                }
                // An error's readable window closing is a **loop event**: the
                // clock only says *when*; this loop decides whether the error it
                // was armed for is still the one on the badge. No timer writes
                // the status channel on its own. The branch is last because it
                // is the only one that can wait forever, and input still wins.
                _ = clock.sleep_until(deadline) => {
                    self.expire_windows_due();
                    self.expire_drafts_due();
                },
            }
            // A finished conversion is retired — and a dead one is reported —
            // before the next is started: one at a time, in hand-out order.
            self.reap_in_flight().await;
            self.pump().await;
        }

        // Exit policy, stated once: a quit stops *reading*. Work already handed
        // to an engine is awaited — not abandoned, because a JoinHandle dropped
        // on the floor is a panic nobody will ever see — and its text is dropped
        // rather than typed, because the user has already left the window. Work
        // still waiting in the queue never starts at all.
        tracing::info!(
            reason = stop,
            in_flight = usize::from(self.in_flight.is_some()),
            queued = self.queue.len(),
            "coordinator stopping: dropping queued audio; awaiting the in-flight conversion, which will not be typed"
        );
        self.queue.clear();
        if let Some(in_flight) = self.in_flight.take() {
            if let Err(e) = in_flight.handle.await {
                tracing::error!(seq = in_flight.seq, error = %e, "task PANICKED");
            }
        }
        Ok(())
    }

    /// One event, one turn.
    ///
    /// The time an event is stamped with comes from the loop's clock, not from
    /// the wall: the rules that read it are timing rules (is this the second
    /// tap of a double-tap?), and a test can only move a value the loop was
    /// given. `SystemClock` reads the wall, so nothing changes for the app.
    async fn on_event(&mut self, ev: HotkeyEvent) -> Result<()> {
        let now = self.clock.now();
        let effects = match ev {
            HotkeyEvent::RecordDown => self.with_session(|s| s.on_record_down(now)),
            HotkeyEvent::RecordUp => self.with_session(|s| s.on_record_up(now)),
            HotkeyEvent::Cancel => {
                self.forget_boundary();
                self.with_session(|s| s.on_cancel())
            }
            // GUI concerns; the loop stays idle.
            HotkeyEvent::ToggleOverlay | HotkeyEvent::Quit => Vec::new(),
        };
        self.perform(effects).await
    }

    async fn on_tick(&mut self) -> Result<()> {
        let now = self.clock.now();
        let effects = self.with_session(|s| s.on_tick(now));
        self.perform(effects).await
    }

    /// Performs the effects the session decided on, in order.
    ///
    /// Every way out of a session — key release, VAD endpoint, safety cap,
    /// cancel — goes through this one function, so none of them can forget the
    /// hands-free badge or the bookkeeping that tells the rules whether the
    /// microphone is still live.
    ///
    /// The badge is published **once, at the end of the turn**, and not from
    /// inside an effect arm. A latch decision can be a real event that has no
    /// effect at all: tapping twice raises the hands-free badge and nothing
    /// else, so a publish that only happened while performing an effect would
    /// never see it and the badge would stay dark for a recording that really
    /// is running hands-free. Publishing after the batch also means one
    /// write per turn, with the rules' final answer rather than a mid-batch
    /// guess.
    async fn perform(&mut self, effects: Vec<Effect>) -> Result<()> {
        for effect in effects {
            match effect {
                Effect::BeginRecording => match self.port.begin().await {
                    Some(reason) => self
                        .status
                        .set_state(AppState::Error(format!("capture start failed: {reason}"))),
                    None => {
                        // Nothing to reset here: the seam lives per session, so
                        // a fresh dictation starts from an empty stitcher by
                        // construction — and starting it cannot touch the seam
                        // an older dictation still needs.
                        //
                        // The id is handed out here, once the microphone really
                        // opened. A dictation with no id could not be asked
                        // "is your text still wanted?" later — which is exactly
                        // what a cancel does.
                        let session = self.with_session(|s| s.began_recording(true));
                        // The window the user is dictating into is read here,
                        // at the moment the microphone really opened, and kept
                        // **by session id**. A capture that failed is not an
                        // error: the dictation is real, and its insert will be
                        // refused later with a reason.
                        let target = self.desktop.capture();
                        match &target {
                            Some(id) => self.remember_target(session, id.clone()),
                            None => tracing::warn!(
                                session = session.map(|id| id.0),
                                "no foreground window to capture; the insert will be refused"
                            ),
                        }
                        // The kind is in the log because it is the difference
                        // between "held the key" and "tapped twice and walked
                        // away", and a bug report about the wrong one is
                        // otherwise unreadable.
                        let kind = session.and_then(|id| self.with_session(|s| s.kind_of(id)));
                        tracing::info!(
                            ?session,
                            ?kind,
                            target = target.as_ref().map(|id| id.hwnd),
                            "recording started"
                        );
                        // The profile this dictation runs under, resolved by the
                        // same function the conversion uses and against the very
                        // destination captured above — so the orb's label cannot
                        // name a profile other than the one that will shape the
                        // text. Published before the state change, so the first
                        // Recording frame already carries it.
                        let profile = rules_for(&self.speech.settings, target.as_ref()).profile;
                        self.status.set_profile(profile);
                        self.status.set_state(AppState::Recording);
                    }
                },
                Effect::FinishSession => {
                    let audio = self.port.finish().await?;
                    let session = self.end_session();
                    self.end_conversion(session, audio);
                }
                Effect::DiscardSession(id) => {
                    // Ownership is read *before* the session closes: the cancel
                    // must take its own badge down, but must not take down a
                    // newer recording's.
                    let owned = self.owns_badge(id);
                    self.port.discard().await?;
                    tracing::info!(
                        session = id.map(|id| id.0),
                        "speech recording cancelled by user"
                    );
                    self.with_session(|s| s.cancelled(id));
                    self.forget_seam(id);
                    self.forget_target(id);
                    self.forget_kept(id);
                    // Drafts from the recording the user just threw away must
                    // not stay on offer: a cancelled dictation that a window
                    // still offers to insert is text the user deliberately
                    // discarded.
                    if let Some(id) = id {
                        self.review.forget_session(id);
                    }
                    // Work of this session that never started is set aside, and
                    // its audio is dropped rather than kept for a recovery that
                    // will never happen.
                    self.drop_queued(id);
                    // The badge comes down **on the cancel**, not on the result
                    // that follows it: a cancelled result changes nothing, so
                    // it cannot be the thing that tidies up after one.
                    if owned {
                        self.status.set_chunk_busy(false);
                        self.status.set_state(AppState::Idle);
                    }
                }
                Effect::PollAudio => {
                    // The driver only ever asks for this while it believes a
                    // session is live, so pumping a buffer nobody owns would
                    // mean the two had drifted apart.
                    debug_assert!(self.with_session(|s| s.is_recording()));
                    match self.port.poll().await? {
                        PollOutcome::More => {}
                        PollOutcome::Chunk { audio } => {
                            let session = self.with_session(|s| s.current_session());
                            // phase 3.2: a chunk boundary must not look like
                            // the recording stopped and restarted.
                            self.status.set_chunk_busy(true);
                            self.enqueue(session, Some(audio), false);
                        }
                        PollOutcome::Finished { audio } => {
                            let session = self.end_session();
                            self.end_conversion(session, audio);
                        }
                    }
                }
            }
        }
        // After the batch, always — including when the batch was empty, which is
        // the double-tap's own case.
        self.publish_latched();
        Ok(())
    }

    /// Closes the recording half and returns the id, leaving the session open
    /// for its result — the badge has to come down now, before the engine runs.
    fn end_session(&self) -> Option<SessionId> {
        let session = self.with_session(|s| s.current_session());
        if let Some(id) = session {
            self.with_session(|s| s.ended_session(id));
        }
        session
    }

    /// The last piece of audio: one ordered final event, sound or no sound.
    ///
    /// The `None` branch matters as much as the other. A port that judged the
    /// utterance empty still gets a job — the same order as the chunks before
    /// it — so the session closes only after they have settled, and a chunk
    /// that is still being converted cannot be refused afterwards as a "late
    /// result of a completed session". Refusing it was deleting text the user
    /// really spoke.
    fn end_conversion(&mut self, session: Option<SessionId>, audio: Option<AudioUtterance>) {
        if audio.is_none() {
            tracing::info!(
                session = session.map(|id| id.0),
                "utterance discarded: nothing worth transcribing"
            );
        }
        self.enqueue(session, audio, true);
    }

    /// Puts one piece of work at the end of the queue and gives it its number.
    fn enqueue(
        &mut self,
        session: Option<SessionId>,
        audio: Option<AudioUtterance>,
        is_final: bool,
    ) {
        let seq = self.next_seq;
        self.next_seq += 1;
        // The last piece of a streamed session sits on the same seam as the
        // mid-session ones, so it takes the next chunk id of the same session:
        // one dictation reads 1..n and then this.
        let chunk = session.and_then(|id| self.with_session(|s| s.next_chunk(id)));
        // The destination is copied in **here**, while the session that captured
        // it is still the one being handed work. From this point on the job
        // knows where its text may go, whatever any later dictation captures.
        let target = self.target_of(session);
        // The badge says `Processing` only for the session that owns it: an
        // older dictation's ending must not overwrite a recording that started
        // after it — and there is nothing to process for an empty utterance.
        if is_final && audio.is_some() && self.owns_badge(session) {
            self.status.set_state(AppState::Processing);
        }
        self.queue.push_back(Job {
            seq,
            session,
            chunk,
            target,
            audio,
            is_final,
        });
    }

    /// Starts the next queued conversion, once the previous one is done.
    ///
    /// One at a time, in hand-out order: a chunk boundary late in a long
    /// dictation cannot overtake the piece before it, and a cancel still has
    /// unstarted work it can set aside. The loop itself is never blocked by
    /// this — the engine runs on its own task, which is how Escape is still
    /// read while it works.
    async fn pump(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        let Some(job) = self.queue.pop_front() else {
            return;
        };
        let Job {
            seq,
            session,
            chunk,
            target,
            audio,
            is_final,
        } = job;
        match audio {
            // Nothing to convert: the ordered final event just takes its turn.
            // It goes through the same ordering gate as a real answer, so it
            // cannot close the session ahead of a chunk that is in flight.
            None => {
                self.drain_in_order(ConversionResult {
                    seq,
                    session,
                    chunk,
                    target,
                    raw: String::new(),
                    processed: String::new(),
                    is_final,
                    failure: None,
                })
                .await;
            }
            Some(audio) => {
                let speech = self.speech.clone();
                let sender = self.results_tx.clone();
                let handle = tokio::spawn(async move {
                    // A conversion that panics must still answer. Dropping the
                    // answer would leave this order number empty forever, and
                    // every later result — ordered behind it — would wait for
                    // it in vain. The panic is caught here so the failure is
                    // tied to the job it came from and the queue moves on.
                    let result = match AssertUnwindSafe(convert(
                        speech,
                        seq,
                        session,
                        chunk,
                        target.clone(),
                        audio,
                        is_final,
                    ))
                    .catch_unwind()
                    .await
                    {
                        Ok(result) => result,
                        Err(panic) => {
                            let why = panic_message(panic.as_ref());
                            tracing::error!(
                                seq,
                                %why,
                                "conversion PANICKED; reported for this job only"
                            );
                            ConversionResult {
                                seq,
                                session,
                                chunk,
                                target,
                                raw: String::new(),
                                processed: String::new(),
                                is_final,
                                failure: Some(format!("conversion task panicked: {why}")),
                            }
                        }
                    };
                    if sender.send(result).is_err() {
                        tracing::debug!(seq, "coordinator is gone; result not applied");
                    }
                });
                self.in_flight = Some(InFlight {
                    seq,
                    session,
                    chunk,
                    is_final,
                    handle,
                });
            }
        }
    }

    /// Holds a result until every earlier one has been applied, then applies
    /// this one and anything that was waiting behind it.
    ///
    /// A final chunk whose engine happens to answer first waits here for the
    /// mid-session chunk it belongs after, instead of typing itself ahead of it.
    async fn drain_in_order(&mut self, result: ConversionResult) {
        self.pending.insert(result.seq, result);
        loop {
            // Numbers that were set aside before they ran never come back: step
            // over them, or everything behind a ghost waits forever.
            while self.skipped.remove(&self.apply_next) {
                self.apply_next += 1;
            }
            let Some(next) = self.pending.remove(&self.apply_next) else {
                return;
            };
            self.apply_next += 1;
            self.apply(next).await;
        }
    }

    /// The only place in the program that turns a conversion into an effect.
    async fn apply(&mut self, result: ConversionResult) {
        let ConversionResult {
            seq,
            session,
            chunk,
            target,
            raw,
            processed,
            is_final,
            failure,
        } = result;

        // The question is asked **here**, at the moment of effect, not when the
        // work was handed out. A cancel that lands while the engine runs is
        // exactly the case this exists for, and a flag set at cancel time would
        // only prove the two were ordered — not that the event was read while
        // the engine was still working.
        if let Err(reason) = self.wanted_by(session) {
            tracing::info!(
                seq,
                session = session.map(|id| id.0),
                reason,
                "result dropped"
            );
            return;
        }

        // Who owns the visible badge. An older dictation's result is still
        // applied — its text was wanted — but it may not overwrite what the
        // user is looking at: the recording that started after it, its chunk
        // badge and its state all belong to the newer session.
        let owns = self.owns_badge(session);

        if let Some(error) = failure {
            // Reaching here means a live session lost its dictation to a real
            // fault. An engine that failed after a cancel did nothing wrong,
            // and it already returned above, so an error badge can never blame
            // the app for text the user threw away on purpose.
            tracing::error!(seq, %error, "conversion failed");
            if owns {
                self.show_error(format!("ASR failed: {error}"));
            }
            self.settle(session, is_final, owns);
            return;
        }

        let merge = self.stitch(session, &processed);
        let plan = plan_typing(&raw, &merge);
        let (to_type_raw, backspaces) = match &plan {
            TypePlan::Skip(reason) => {
                tracing::info!(seq, reason = reason.as_str(), "nothing to type");
                self.settle(session, is_final, owns);
                return;
            }
            TypePlan::Type { text, backspaces } => (text.as_str(), *backspaces),
        };

        // ── boundary spacing policy ──────────────────────────────────────
        // Evaluated after seam overlap resolution and before delivery.
        // If backspaces > 0, this is word repair (e.g. truncated tail); no space is added.
        // If backspaces == 0 and boundary memory is valid, a single space separator is
        // prepended if needed.
        //
        // Note on Cross-Session Continuation (تخمین بر اساس همان پنجره):
        // Preserving boundary memory across sessions is merely a heuristic estimate
        // based on the same foreground window (matching hwnd and pid), NOT proof of
        // caret/insertion-point continuity within the target document. Detection of
        // manual cursor movement or edits by the user between sessions is currently
        // unimplemented (documented desktop platform limitation); therefore, uncertain
        // continuations are NEVER repaired with automatic Backspace or text deletion.
        let to_type_adjusted = {
            let boundary = self
                .boundary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            boundary.apply(to_type_raw, backspaces, &target)
        };
        let to_type = to_type_adjusted.as_str();
        tracing::info!(
            seq,
            session = session.map(|id| id.0),
            chunk = chunk.map(|c| c.0),
            kind = if is_final { "final" } else { "chunk" },
            raw = %raw,
            typed = %to_type,
            backspaces,
            "text ready"
        );

        // ── review mode: hold the text, type nothing ───────────────────────
        //
        // Placed before the destination is even asked, because a held draft is
        // not an insert and has no destination to be wrong about yet: the
        // destination is re-checked when the user approves, which may be
        // minutes later.
        //
        // `backspaces` is deliberately **not** replayed on approval. A seam
        // repair erases a fragment this app itself typed a moment ago; if the
        // user edited the text in between, the erase would delete their words.
        // So an approved insert is a plain type of whatever the box says.
        if review::should_hold(DraftKind::Review, self.review_enabled(target.as_ref())) {
            self.raise_draft(
                DraftKind::Review,
                to_type.to_string(),
                target.clone(),
                session,
            );
            // `stitch` above has already advanced this session's seam memory,
            // and that memory is only true if the document took the text. It
            // did not — nothing was typed. Leaving it behind would make the
            // *next* dictation's seam repair erase words to match a tail that
            // is not there, which is the one way this stage eats the user's
            // text.
            self.forget_seam(session);
            self.forget_boundary();
            // The session closes normally; `settle` also puts the badge back
            // to Idle, which is right here — `Typing` would be a lie, since
            // nothing is being typed.
            self.settle(session, is_final, owns);
            return;
        }

        // ── the destination, asked once, at the moment of effect ──────────
        //
        // The window this session captured is re-checked here, immediately
        // before the first key, and asked **once**: re-asking between the erase
        // and the text would only widen the gap it is trying to close.
        //
        // Two limits are stated rather than solved, because the code cannot
        // close either of them:
        //
        // * the answer is about the **window**, not the text box inside it — one
        //   window holds several typeable fields, and moving between two of them
        //   is invisible here;
        // * the gap between this answer and `SendInput` is real time, and the
        //   user can spend it moving focus. That is the price of not stealing
        //   focus back, which would be worse.
        let validity = match &target {
            Some(id) => self.desktop.check(id),
            // A dictation that captured no window has no destination, and "no
            // destination" is **not** "any destination": typing into whatever
            // happens to be in front is the one outcome this check exists to
            // prevent.
            None => TargetValidity::Unknown,
        };

        // The orb shows the typing: the badge moves before the keystrokes go
        // out. A mid-session chunk deliberately does not — a chunk boundary is
        // neither a stop nor a start. And it only moves for the session that
        // owns the badge: an older dictation's text may land while a new one is
        // recording, and the orb must keep showing that recording.
        if is_final && owns {
            self.status.set_state(AppState::Typing);
        }
        // ── the sends, or the reason there are none ──────────────────────
        let steps = if validity.allows_insert() {
            self.deliver(to_type, backspaces)
        } else {
            // Refused. Not one key — no characters, and no backspace either,
            // because a backspace into a window the user did not dictate into
            // deletes *their* text.
            Vec::new()
        };

        let (backspace_report, text_report) = if !validity.allows_insert() {
            (None, None)
        } else if backspaces > 0 {
            let erase = steps.first().cloned();
            let typed = if steps.len() > 1 {
                steps.get(1).cloned()
            } else {
                None
            };
            (erase, typed)
        } else {
            (None, steps.first().cloned())
        };

        let outcome = judge_insert(&steps);
        // The stitch above has already advanced this session's seam memory, and
        // that memory is only true if the document really took the text. A
        // refusal, a torn send and a blocked send all leave it wrong, so it dies
        // here — the next chunk starts with an empty tail, which can drop
        // nothing and delete nothing. Leaving it behind would be the one way to
        // make this stage *eat the user's words*.
        if !outcome.is_whole() {
            self.forget_seam(session);
            self.forget_boundary();
        }

        match outcome {
            InjectOutcome::Complete { accepted_pairs } => {
                tracing::info!(
                    seq,
                    accepted_pairs,
                    "input accepted whole — keystroke pairs the platform took, not characters proven to be in a document"
                );
                self.status.set_last_text(to_type.to_string());
                self.record_boundary(to_type, session, target.clone());
            }
            InjectOutcome::Partial { accepted_pairs } => {
                // Reported, and **never re-sent**: the accepted half is already
                // in the document, so sending the whole text again would type it
                // twice. What the user is offered instead is the **suffix** —
                // see `unaccepted_text`, which refuses to guess when the
                // accepted count lands mid-character.
                tracing::error!(
                    seq,
                    accepted_pairs,
                    "input accepted in part only; NOT sent again"
                );
                let record = KeptRecord {
                    session,
                    chunk,
                    seq,
                    planned_text: to_type.to_string(),
                    destination: target.clone(),
                    outcome,
                    backspace: backspace_report,
                    text: text_report,
                };
                self.recover_from(&record);
                if owns {
                    self.show_error(format!(
                        "injection incomplete: {accepted_pairs} keystroke pair(s) accepted; not re-sent"
                    ));
                }
            }
            InjectOutcome::Failed => {
                tracing::error!(seq, "injection failed: the platform accepted nothing");
                let record = KeptRecord {
                    session,
                    chunk,
                    seq,
                    planned_text: to_type.to_string(),
                    destination: target.clone(),
                    outcome,
                    backspace: backspace_report,
                    text: text_report,
                };
                self.recover_from(&record);
                if owns {
                    self.show_error("injection failed: nothing was typed".to_string());
                }
            }
            InjectOutcome::NotAttempted => {
                tracing::warn!(
                    seq,
                    session = session.map(|id| id.0),
                    ?validity,
                    hwnd = target.as_ref().map(|id| id.hwnd),
                    "destination refused: not one key was sent"
                );
                // The text stays in memory for the next action, and no audio is
                // kept for a recovery: this job's recording was already given up
                // by the engine, and sound nobody can re-insert is not a safety
                // net.
                let record = KeptRecord {
                    session,
                    chunk,
                    seq,
                    planned_text: to_type.to_string(),
                    destination: target.clone(),
                    outcome,
                    backspace: backspace_report,
                    text: text_report,
                };
                self.recover_from(&record);
                if owns {
                    self.show_error(match validity {
                        TargetValidity::Changed => {
                            "text kept, nothing typed: focus moved".to_string()
                        }
                        // `Valid` cannot reach this arm — it is the refusal — and
                        // `Unknown` is the other half of it: a window we cannot
                        // even ask about.
                        _ => "text kept, nothing typed: destination unknown".to_string(),
                    });
                }
            }
        }
        self.settle(session, is_final, owns);
    }

    /// Acts on the user's answer to a draft.
    ///
    /// The one place in the program where text the user has already seen becomes
    /// keystrokes. That is why the destination is **re-validated here** rather
    /// than trusted from when the draft was raised: minutes may have passed, and
    /// the user may have clicked into another window precisely because the
    /// draft was sitting there. Re-checking is the same rule the direct path
    /// follows, and skipping it here would make review mode the *less* careful
    /// of the two modes.
    ///
    /// The edited text is typed, not the original: the review window is
    /// editable, and an edit the user made is a decision about what to type.
    async fn on_review(&mut self, command: ReviewCommand) {
        let command_id = command.id();
        match self.review.resolve(command) {
            // Nothing to do, and that is the whole handling. A duplicate click,
            // or an answer that arrived after the draft expired, must type
            // nothing at all — not "the same text again".
            ReviewOutcome::Stale => {
                tracing::info!(id = command_id, "review answer for an unknown draft; ignored");
            }
            ReviewOutcome::Cancelled => {
                tracing::info!(id = command_id, "draft discarded by the user");
                // A discarded text is a boundary the app never established, so
                // the next dictation must not add a space as if it had.
                self.forget_boundary();
            }
            ReviewOutcome::Copied { text } => {
                tracing::info!(
                    id = command_id,
                    chars = text.chars().count(),
                    "draft copied; nothing typed"
                );
                self.forget_boundary();
            }
            ReviewOutcome::Insert { text, destination } => {
                self.insert_reviewed(command_id, &text, destination).await;
            }
        }
    }

    /// Types text the user has approved, under the same destination rule as
    /// every other insert.
    async fn insert_reviewed(
        &mut self,
        id: u64,
        text: &str,
        destination: Option<TargetIdentity>,
    ) {
        let validity = match &destination {
            Some(target) => self.desktop.check(target),
            // The same refusal as the direct path: "no destination" is not
            // "any destination".
            None => TargetValidity::Unknown,
        };
        if !validity.allows_insert() {
            tracing::warn!(
                id,
                ?validity,
                hwnd = destination.as_ref().map(|t| t.hwnd),
                "approved text not inserted: the destination is no longer the one it was dictated into"
            );
            // Raised again rather than dropped: the user asked for this text to
            // be typed, and losing it because they moved a window is exactly the
            // failure this feature exists to prevent. It comes back as
            // `Undelivered` so it is offered regardless of the review setting.
            self.raise_draft(DraftKind::Undelivered, text.to_string(), destination, None);
            return;
        }

        let steps = self.deliver(text, 0);
        let outcome = judge_insert(&steps);
        match outcome {
            InjectOutcome::Complete { accepted_pairs } => {
                tracing::info!(
                    id,
                    accepted_pairs,
                    "approved text accepted whole"
                );
                self.status.set_last_text(text.to_string());
                self.record_boundary(text, None, destination);
                self.clear_kept();
            }
            // Reported, never re-sent: the accepted half is already in the
            // document.
            InjectOutcome::Partial { accepted_pairs } => {
                tracing::error!(
                    id,
                    accepted_pairs,
                    "approved text accepted in part only; NOT sent again"
                );
                let record = KeptRecord {
                    session: None,
                    chunk: None,
                    seq: 0,
                    planned_text: text.to_string(),
                    destination,
                    outcome,
                    backspace: None,
                    text: steps.first().cloned(),
                };
                self.record_kept(record);
                self.show_error(format!(
                    "injection incomplete: {accepted_pairs} keystroke pair(s) accepted; not re-sent"
                ));
            }
            InjectOutcome::Failed | InjectOutcome::NotAttempted => {
                tracing::error!(id, "approved text could not be typed; kept for another try");
                let record = KeptRecord {
                    session: None,
                    chunk: None,
                    seq: 0,
                    planned_text: text.to_string(),
                    destination: destination.clone(),
                    outcome,
                    backspace: None,
                    text: steps.first().cloned(),
                };
                self.record_kept(record);
                // Offered again immediately: the user is looking at a window
                // with this text in it, and a silent failure would read as the
                // app having ignored them.
                self.raise_draft(DraftKind::Undelivered, text.to_string(), destination, None);
                self.show_error("injection failed: nothing was typed".to_string());
            }
        }
    }

    /// Puts a draft up for a decision and says so in the log.
    ///
    /// Returns nothing: the caller does not act on the draft, and the only
    /// honest response to "there is now text waiting" is a log line naming it.
    /// A caller that needed to know *whether* it was raised would have to
    /// duplicate the store's own emptiness rule.
    fn raise_draft(
        &self,
        kind: DraftKind,
        text: String,
        destination: Option<TargetIdentity>,
        session: Option<SessionId>,
    ) -> Option<PendingDraft> {
        let raised = self.review.raise(PendingDraft {
            id: 0,
            kind,
            text,
            destination,
            session,
            raised: self.clock.now(),
        });
        if let Some(draft) = &raised {
            tracing::info!(
                id = draft.id,
                kind = draft.kind.as_str(),
                chars = draft.text.chars().count(),
                destination = draft.destination.as_ref().map(|t| t.hwnd),
                "text held for the user; nothing has been typed"
            );
        } else {
            tracing::info!(
                kind = kind.as_str(),
                "nothing worth holding: there was no text to decide about"
            );
        }
        raised
    }

    /// The sends of one insert, in the order they are made, and **nothing** when
    /// the destination refused.
    ///
    /// An erase that did not go out whole **stops the insert**. The fragment it
    /// meant to delete is then either still in the document or half-deleted, and
    /// typing the complete word on top of it leaves the user with both — so the
    /// smaller wrong is chosen: warn, keep the text, send nothing further.
    fn deliver(&self, text: &str, backspaces: usize) -> Vec<Injection> {
        let mut steps = Vec::with_capacity(2);
        if backspaces > 0 {
            let erase = self.sink.backspace(backspaces);
            let whole = erase.whole_pairs();
            steps.push(erase);
            if !whole {
                return steps;
            }
        }
        steps.push(self.sink.type_text(text));
        steps
    }

    /// Finishes the bookkeeping for a **wanted** result.
    ///
    /// Called on every path a result can take out of [`Self::apply`] — typed,
    /// refused as empty, failed — because a session left open would make every
    /// later question asked about it meaningless. A mid-session chunk never
    /// closes its session here: the dictation continues after it.
    ///
    /// `owns` is the badge question, asked once before any of this: an older
    /// dictation's result is still applied, but it may not overwrite the
    /// recording that started after it, and it may not report on that
    /// recording's microphone either.
    fn settle(&self, session: Option<SessionId>, is_final: bool, owns: bool) {
        if is_final {
            if let Some(id) = session {
                self.with_session(|s| s.result_arrived(id));
            }
            // The session is settled: its seam memory and its destination die
            // with it, and no other session's are touched.
            // Undelivered text records are NOT dropped here: they are decoupled
            // from the active session lifecycle and preserved for future action/recovery.
            self.forget_seam(session);
            self.forget_target(session);
            // A refusal is **not** a failure: a cancelled or empty result must
            // not leave an error badge behind, or the next press-to-talk is
            // swallowed behind a red orb the user cannot explain.
            if owns {
                let failed = is_transient(&self.status.snapshot().state);
                if !failed {
                    self.status.set_state(AppState::Idle);
                }
            }
        } else {
            // A chunk moves no visible state (phase 3.2) — it only says the
            // boundary is over. A capture device that died mid-dictation is
            // noticed here rather than on the next key press, and only for the
            // session that owns the microphone now.
            if owns {
                self.status.set_chunk_busy(false);
                let live = self.port.capture_live();
                self.with_session(|s| s.note_capture_live(live));
                if !live {
                    self.status.set_state(AppState::Idle);
                }
            }
        }
    }

    /// Retires the conversion that finished — by awaiting it, never by dropping
    /// the handle.
    ///
    /// The ordinary ending leaves its answer in the result channel, and the
    /// loop's result branch picks it up. An ending *without* an answer — a task
    /// that panicked past the worker's own guard, or was aborted — would
    /// otherwise hold the queue forever: its order number would never be
    /// applied and every later result would wait behind it in vain. So an
    /// abnormal end is turned into a failure **for this job**, which the
    /// ordinary apply path then reports and settles.
    ///
    /// A panic *inside* the conversion does not arrive here: the worker catches
    /// its own ([`Coordinator::pump`]) so that the loop has a deterministic
    /// answer to wake up for. What is left here is a task killed from outside
    /// its own body — **not tested in this delivery**: no scenario here builds
    /// one, so the arm is kept as defence and recorded as a gap rather than
    /// claimed as covered.
    async fn reap_in_flight(&mut self) {
        let Some(in_flight) = self.in_flight.take() else {
            return;
        };
        if !in_flight.handle.is_finished() {
            self.in_flight = Some(in_flight);
            return;
        }
        if let Err(e) = in_flight.handle.await {
            tracing::error!(
                seq = in_flight.seq,
                error = %e,
                "conversion task ended abnormally"
            );
            self.drain_in_order(ConversionResult {
                seq: in_flight.seq,
                session: in_flight.session,
                chunk: in_flight.chunk,
                // A task that died has no text, so there is nothing to deliver
                // and no destination to judge.
                target: None,
                raw: String::new(),
                processed: String::new(),
                is_final: in_flight.is_final,
                failure: Some(format!("conversion task ended abnormally: {e}")),
            })
            .await;
        }
    }

    /// Whether `session` currently owns the visible badge.
    ///
    /// The newest open session does. An older one's result is still applied —
    /// its text was wanted — but `Recording`, the chunk badge and the current
    /// session's state all belong to the new session, so an old result may not
    /// write any of them.
    fn owns_badge(&self, session: Option<SessionId>) -> bool {
        match session {
            None => true,
            Some(id) => self.with_session(|s| s.current_session()) == Some(id),
        }
    }

    /// Drops one session's seam memory. Completion and cancellation both go
    /// through here, and nothing else touches another session's.
    fn forget_seam(&self, session: Option<SessionId>) {
        self.seams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session);
    }

    /// Records the window a session is dictating into.
    ///
    /// Keyed by session id and never replaced by a later dictation: an older
    /// answer can land after a newer recording has started, and it belongs to
    /// the window it was spoken into.
    fn remember_target(&self, session: Option<SessionId>, target: TargetIdentity) {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session, target);
    }

    /// The window `session` captured, if it captured one.
    fn target_of(&self, session: Option<SessionId>) -> Option<TargetIdentity> {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&session)
            .cloned()
    }

    /// Drops one session's destination, with the rest of its bookkeeping.
    fn forget_target(&self, session: Option<SessionId>) {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session);
    }

    /// Closes every draft window whose deadline has passed.
    ///
    /// A loop event for the same reason the error windows are one: the clock
    /// says *when*, and only this loop may decide whether the draft it was armed
    /// for is still the one on offer. No background timer touches the store.
    fn expire_drafts_due(&mut self) {
        let now = self.clock.now();
        let expired = self.review.expire(now, self.draft_ttl());
        for draft in &expired {
            tracing::info!(
                id = draft.id,
                kind = draft.kind.as_str(),
                chars = draft.text.chars().count(),
                "draft expired unanswered; its text is in the log and in history"
            );
        }
    }

    /// The soonest moment this loop has to wake up for a draft.
    fn next_draft_deadline(&self) -> Option<Instant> {
        let ttl = self.draft_ttl();
        self.review
            .snapshot()
            .drafts
            .iter()
            .map(|d| d.raised + ttl)
            .min()
    }

    /// Records an undelivered text and offers it back to the user.
    ///
    /// Both halves together, because they are the same decision: a record nobody
    /// can see is an audit log, and a draft nobody can retry is a label. The
    /// roadmap asks for retry on **insert** only — no audio is kept, no engine
    /// is asked again — and this is the only place that turns a record into
    /// something the user can act on.
    ///
    /// The offered text is `unaccepted_text`, never the whole planned text. On a
    /// partial send, the accepted half is already in the document, so offering
    /// the whole thing back would invite the user to type it twice. When the
    /// accepted boundary cannot be located the answer is `None` and **nothing
    /// is offered**: guessing there would produce duplicate text, which is
    /// worse than saying nothing.
    fn recover_from(&self, record: &KeptRecord) {
        self.record_kept(record.clone());
        let Some(text) = record.unaccepted_text() else {
            tracing::warn!(
                seq = record.seq,
                "text could not be located precisely; not offering a retry, because guessing would duplicate it"
            );
            return;
        };
        if text.is_empty() {
            return;
        }
        self.raise_draft(
            DraftKind::Undelivered,
            text.to_string(),
            record.destination.clone(),
            record.session,
        );
    }

    /// Keeps an undelivered or action-needed text record in memory.
    fn record_kept(&self, record: KeptRecord) {
        tracing::info!(
            session = record.session.map(|id| id.0),
            chunk = record.chunk.map(|c| c.0),
            seq = record.seq,
            outcome = ?record.outcome,
            "action-needed text record preserved in memory"
        );
        self.kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record);
    }

    /// Drops every undelivered record.
    ///
    /// Called after a text the user had been shown is finally typed: the record
    /// described a problem that no longer exists, and leaving it would mean the
    /// next recovery offers text that is already in the document.
    fn clear_kept(&self) {
        self.kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// Drops one session's undelivered records on explicit cancellation.
    fn forget_kept(&self, session: Option<SessionId>) {
        self.kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|rec| rec.session != session);
    }

    /// Sets aside the work of `session` that has not started yet.
    ///
    /// The audio is **dropped**, not kept for a recovery: the user threw the
    /// dictation away, and a chunk converted afterwards would have nowhere to
    /// go except the keyboard they were trying to protect.
    fn drop_queued(&mut self, session: Option<SessionId>) {
        let dropped: Vec<u64> = self
            .queue
            .iter()
            .filter(|job| job.session == session)
            .map(|job| job.seq)
            .collect();
        self.queue.retain(|job| job.session != session);
        self.skipped.extend(dropped.iter().copied());
        if !dropped.is_empty() {
            tracing::info!(
                session = session.map(|id| id.0),
                dropped = dropped.len(),
                "queued audio discarded"
            );
        }
    }

    /// Puts a failure the user can read on the badge, and arms its window.
    ///
    /// Every error the coordinator shows goes through here, so every one has a
    /// numbered window that closes as a loop event ([`Self::expire_windows_due`]).
    fn show_error(&mut self, message: String) {
        self.error_version += 1;
        self.windows.push(ErrorWindow {
            version: self.error_version,
            deadline: self.clock.now() + ERROR_READABLE,
            message: message.clone(),
        });
        tracing::info!(
            version = self.error_version,
            %message,
            "error shown; its window is armed"
        );
        self.status.set_state(AppState::Error(message));
    }

    /// Closes every error window whose deadline has passed, oldest first.
    ///
    /// A superseded window still closes — and then finds that its error is no
    /// longer the active one, which is the whole point of numbering them. The
    /// loop runs this; no timer touches the status channel.
    fn expire_windows_due(&mut self) {
        let now = self.clock.now();
        let mut due = Vec::new();
        self.windows.retain(|window| {
            if window.deadline <= now {
                due.push(window.clone());
                false
            } else {
                true
            }
        });
        for window in &due {
            self.on_error_expired(window);
        }
    }

    /// The expiry of one window.
    ///
    /// It clears the badge only when it is still the error this window was
    /// armed for: a newer failure that replaced it, or a recording that started
    /// after it, owns the badge now and must not be touched.
    fn on_error_expired(&self, window: &ErrorWindow) {
        tracing::debug!(version = window.version, "error window closed");
        let still_active = window.version == self.error_version
            && matches!(
                &self.status.snapshot().state,
                AppState::Error(message) if *message == window.message
            );
        if still_active {
            self.status.set_state(AppState::Idle);
        }
    }

    /// Stitches one transcript into its **own** session's running text.
    ///
    /// Keyed by session, not global: a fresh dictation must not reset the
    /// stitcher an older one still needs, and an older one's tail must never be
    /// used to delete the new one's first words.
    fn stitch(&self, session: Option<SessionId>, text: &str) -> SeamMerge {
        let opts = SeamOptions::from_streaming(&self.speech.settings.streaming);
        let mut seams = self
            .seams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let merge = seams
            .entry(session)
            .or_insert_with(|| SeamStitcher::new(opts))
            .stitch(text);
        if merge.dropped_words > 0 || merge.backspaces > 0 {
            tracing::info!(%merge, ?session, "chunk seam repaired");
        }
        merge
    }

    /// Whether `session` still wants text, and if not, why.
    ///
    /// `Recording` and `AwaitingResult` both accept: the second is the ordinary
    /// case for a final chunk, whose text arrives by definition after the key
    /// was released.
    fn wanted_by(&self, session: Option<SessionId>) -> Result<(), &'static str> {
        match session {
            None => Ok(()),
            Some(id) => self.with_session(|s| s.accepts_result(id)),
        }
    }

    fn with_session<R>(&self, f: impl FnOnce(&mut SessionDriver) -> R) -> R {
        f(&mut self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    /// Publishes whatever the rules currently say about the hands-free badge.
    ///
    /// `StatusChannel::set_latched` ignores a repeat, so calling this
    /// unconditionally is free — the loop does not have to ask whether anything
    /// changed, and a decision that changes nothing costs one lock.
    fn publish_latched(&self) {
        let latched = self.with_session(|s| s.latched());
        self.status.set_latched(latched);
    }
}

/// The readable part of a caught panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    //! The scenarios, driven through the production loop.
    //!
    //! Nothing here re-implements coordination. Each test builds a
    //! [`Coordinator`] — the same struct [`crate::state::machine`] starts in the
    //! app — with a scripted microphone, a scripted engine and a sink that
    //! writes down keystrokes instead of sending them. What differs is only what
    //! the hardware and the engine *say*, and *when*: a [`Gate`] that the test
    //! opens by hand is how "the cancel landed while the engine was still
    //! working" becomes a fact instead of a duration somebody hoped was long
    //! enough. There is no `sleep` in this file.
    //!
    //! Every scenario ends with the loop stopped and the sink asserted, so a
    //! test cannot pass by having quietly never run.

    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Condvar;
    use tokio::sync::{mpsc, watch, Notify};

    use crate::asr::engine::{AsrEngine, AsrHealth};
    use crate::output::target::Observation;
    use crate::state::status::AppStatus;

    // ── a latch the test opens by hand ────────────────────────────────────

    /// A two-sided latch: the engine announces it has started and is waiting,
    /// and the test opens it when it wants the conversion to finish.
    ///
    /// A `Condvar` rather than a `Notify` because the engine runs on a blocking
    /// thread, where awaiting is not an option. This is what replaces every
    /// `sleep` these tests would otherwise need: each ordering claim is a
    /// happens-before the test sets up itself.
    ///
    /// The waits carry a patience deadline, and that deadline decides nothing.
    /// It exists so that a *failing* assertion — which abandons the test while
    /// an engine is still parked here — fails the run instead of hanging it.
    struct Gate {
        state: Mutex<GateState>,
        changed: Condvar,
    }

    /// Every wait in this file is an event, not a duration, so a legitimate run
    /// never gets near this. It is short on purpose: when a mutation breaks a
    /// barrier, the canary pays this once per broken test, and a 60 s guard made
    /// a canary run exceed its own budget.
    const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

    /// Fails the test instead of waiting forever.
    ///
    /// Every wait in this file is an event, not a duration — nothing here is
    /// expected to take [`PATIENCE`]. The bound exists so that a *broken* loop
    /// reports which thing never happened, rather than hanging a whole canary
    /// run until someone notices. A scenario that fails this way has still told
    /// the truth: whatever it was waiting for, did not arrive.
    async fn within<T>(
        limit: std::time::Duration,
        what: &str,
        fut: impl std::future::Future<Output = T>,
    ) -> T {
        match tokio::time::timeout(limit, fut).await {
            Ok(value) => value,
            Err(_) => panic!("waited {limit:?} for {what}"),
        }
    }

    #[derive(Default)]
    struct GateState {
        arrived: bool,
        open: bool,
    }

    impl Gate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(GateState::default()),
                changed: Condvar::new(),
            })
        }

        /// Engine side: "I am here and I am waiting."
        fn arrive(&self) {
            let mut s = self.state.lock().unwrap();
            s.arrived = true;
            self.changed.notify_all();
        }

        /// Engine side: block until the test opens it.
        fn wait_open(&self) {
            let mut s = self.state.lock().unwrap();
            let deadline = std::time::Instant::now() + PATIENCE;
            while !s.open {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                assert!(!left.is_zero(), "the test never opened the gate");
                let (guard, _) = self.changed.wait_timeout(s, left).unwrap();
                s = guard;
            }
        }

        /// Test side: block until the engine is waiting.
        fn wait_arrived(&self) {
            let mut s = self.state.lock().unwrap();
            let deadline = std::time::Instant::now() + PATIENCE;
            while !s.arrived {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                assert!(!left.is_zero(), "the engine never reached the gate");
                let (guard, _) = self.changed.wait_timeout(s, left).unwrap();
                s = guard;
            }
        }

        /// Test side: let the conversion finish.
        fn open(&self) {
            let mut s = self.state.lock().unwrap();
            s.open = true;
            self.changed.notify_all();
        }
    }

    // ── a scripted engine ─────────────────────────────────────────────────

    /// One conversion's worth of behaviour, written down before the test runs.
    ///
    /// Every step is keyed by the **length of the audio it answers**: the answer
    /// belongs to the audio, so the test never has to know the order in which
    /// the port hands chunks over — it only has to make each audio uniquely
    /// identifiable.
    enum Step {
        /// Answer now with this text.
        Text { samples: usize, text: &'static str },
        /// Fail now, with a real error (and a message the test can recognise).
        Fail { samples: usize, why: &'static str },
        /// Announce the start, wait for the test, then answer with this text.
        Gated {
            samples: usize,
            gate: Arc<Gate>,
            text: &'static str,
        },
        /// The same, but the answer is a failure.
        GatedFail { samples: usize, gate: Arc<Gate> },
        /// Panic inside the engine, the way a native recogniser can.
        Panics { samples: usize },
    }

    impl Step {
        /// The audio length this step answers.
        fn samples(&self) -> usize {
            match self {
                Step::Text { samples, .. }
                | Step::Fail { samples, .. }
                | Step::Gated { samples, .. }
                | Step::GatedFail { samples, .. }
                | Step::Panics { samples } => *samples,
            }
        }
    }

    /// An engine whose every call is decided by the test, in advance.
    struct ScriptedEngine {
        steps: Mutex<Vec<Step>>,
        calls: std::sync::atomic::AtomicUsize,
        /// Explode in `health()`, once.
        ///
        /// `transcribe` asks how the engine is *before* it asks it to
        /// transcribe, and that call runs on the conversion task itself — the
        /// router puts only the transcription on the blocking pool. So this is
        /// how a test makes a conversion task die from the inside, the way a
        /// broken native engine can.
        broken_health: std::sync::atomic::AtomicBool,
    }

    fn engine(steps: Vec<Step>) -> Arc<ScriptedEngine> {
        Arc::new(ScriptedEngine {
            steps: Mutex::new(steps),
            calls: std::sync::atomic::AtomicUsize::new(0),
            broken_health: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// The same engine, whose `health()` check explodes once.
    fn engine_with_broken_health(steps: Vec<Step>) -> Arc<ScriptedEngine> {
        let engine = engine(steps);
        engine
            .broken_health
            .store(true, std::sync::atomic::Ordering::SeqCst);
        engine
    }

    impl ScriptedEngine {
        /// How many times the engine was actually asked. A conversion the loop
        /// had no business running is visible here and nowhere else.
        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn router(self: &Arc<Self>) -> Arc<AsrRouter> {
            let router = AsrRouter::new(vec![self.clone()]);
            // A router cools a failing engine down for 30 s. A test that wrote
            // down two failures would then be shown "no engine available"
            // instead of the failure it scripted, so the cooldown is off here.
            router.set_cooldown(0);
            Arc::new(router)
        }
    }

    impl AsrEngine for ScriptedEngine {
        fn name(&self) -> &'static str {
            "scripted"
        }
        fn health(&self) -> AsrHealth {
            if self
                .broken_health
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                panic!("scripted engine: health check exploded");
            }
            AsrHealth::Ready
        }
        fn transcribe(&self, audio: &AudioUtterance) -> Result<String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let wanted = audio.samples.len();
            let step = {
                let mut steps = self.steps.lock().unwrap();
                let at = steps
                    .iter()
                    .position(|s| s.samples() == wanted)
                    .unwrap_or_else(|| panic!("the test scripted no answer for {wanted} samples"));
                steps.remove(at)
            };
            match step {
                Step::Text { text, .. } => Ok(text.to_string()),
                Step::Gated { gate, text, .. } => {
                    gate.arrive();
                    gate.wait_open();
                    Ok(text.to_string())
                }
                Step::GatedFail { gate, .. } => {
                    gate.arrive();
                    gate.wait_open();
                    anyhow::bail!("scripted gated failure")
                }
                Step::Fail { why, .. } => anyhow::bail!("{why}"),
                Step::Panics { .. } => panic!("scripted engine panic"),
            }
        }
    }

    // ── a scripted microphone ─────────────────────────────────────────────

    /// A microphone that reports whatever the test wrote down.
    struct ScriptedPort {
        polls: Mutex<VecDeque<PollOutcome>>,
        finishes: Mutex<VecDeque<Option<AudioUtterance>>>,
        live: AtomicBool,
        /// Fires when a cancel reaches the hardware.
        ///
        /// This is the barrier the "cancel while converting" scenarios need: the
        /// loop calls `discard` and marks the session cancelled with nothing in
        /// between, so once the test has seen this it knows the cancellation has
        /// been decided before it lets the engine answer.
        discarded: Arc<Notify>,
    }

    fn port(
        polls: Vec<PollOutcome>,
        finishes: Vec<Option<AudioUtterance>>,
    ) -> (ScriptedPort, Arc<Notify>) {
        let discarded = Arc::new(Notify::new());
        (
            ScriptedPort {
                polls: Mutex::new(polls.into()),
                finishes: Mutex::new(finishes.into()),
                live: AtomicBool::new(false),
                discarded: discarded.clone(),
            },
            discarded,
        )
    }

    impl Port for ScriptedPort {
        async fn begin(&self) -> Option<String> {
            self.live.store(true, Ordering::SeqCst);
            None
        }
        async fn poll(&self) -> Result<PollOutcome> {
            Ok(self
                .polls
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(PollOutcome::More))
        }
        async fn finish(&self) -> Result<Option<AudioUtterance>> {
            self.live.store(false, Ordering::SeqCst);
            Ok(self.finishes.lock().unwrap().pop_front().flatten())
        }
        async fn discard(&self) -> Result<()> {
            self.live.store(false, Ordering::SeqCst);
            self.discarded.notify_one();
            Ok(())
        }
        fn capture_live(&self) -> bool {
            self.live.load(Ordering::SeqCst)
        }
    }

    // ── a sink that writes down keystrokes ────────────────────────────────

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Op {
        Backspace(usize),
        Type(String),
    }

    /// What the fake platform takes from one send.
    ///
    /// `SendInput` fails outright when a window of higher privilege is in front
    /// (`Then` is the rare, defensive middle), and these are the three shapes
    /// the loop has to tell apart.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Accept {
        /// Every event the send needed.
        All,
        /// The first `n` events, then the platform stops taking them.
        Then(usize),
        /// Nothing at all.
        None,
    }

    /// The keyboard's answers, in the order the sends will be made.
    ///
    /// An empty list means "every send is accepted whole", so the scenarios that
    /// never cared about this say exactly what they said before.
    #[derive(Debug, Clone, Default)]
    struct Platform {
        types: Vec<Accept>,
        backspaces: Vec<Accept>,
    }

    struct RecordingSink {
        ops: watch::Sender<Vec<Op>>,
        types: Mutex<VecDeque<Accept>>,
        backspaces: Mutex<VecDeque<Accept>>,
    }

    impl RecordingSink {
        fn pair() -> (Arc<Self>, watch::Receiver<Vec<Op>>) {
            Self::scripted(Platform::default())
        }

        /// A sink that answers the sends the scenario wrote down.
        fn scripted(platform: Platform) -> (Arc<Self>, watch::Receiver<Vec<Op>>) {
            let (ops, seen) = watch::channel(Vec::new());
            (
                Arc::new(Self {
                    ops,
                    types: Mutex::new(platform.types.into()),
                    backspaces: Mutex::new(platform.backspaces.into()),
                }),
                seen,
            )
        }
        fn push(&self, op: Op) {
            self.ops.send_modify(|ops| ops.push(op));
        }

        /// What the platform did with one send of `attempted` events.
        ///
        /// Written the way the real injector writes it \u2014 accepted, attempted,
        /// and a reason when the two differ \u2014 so a scenario's scripted
        /// shortfall is the same value the loop would see from `SendInput`.
        fn take(&self, script: &Mutex<VecDeque<Accept>>, attempted: usize) -> Injection {
            let accept = script.lock().unwrap().pop_front().unwrap_or(Accept::All);
            let accepted = match accept {
                Accept::All => attempted,
                Accept::Then(n) => n.min(attempted),
                Accept::None => 0,
            };
            Injection {
                total_events: attempted,
                attempted,
                accepted,
                stopped: (accepted != attempted)
                    .then(|| format!("scripted: {accepted} of {attempted} events accepted")),
            }
        }
    }

    impl TextSink for RecordingSink {
        fn type_text(&self, text: &str) -> Injection {
            // The op log records what was **asked for**, not what was accepted:
            // a refused send is still a send that was attempted, and \u201cwas it
            // tried again?\u201d is exactly the question the log has to answer.
            self.push(Op::Type(text.to_string()));
            // One down/up pair per UTF-16 unit, like the real injector.
            self.take(&self.types, text.encode_utf16().count() * 2)
        }
        fn backspace(&self, count: usize) -> Injection {
            self.push(Op::Backspace(count));
            self.take(&self.backspaces, count * 2)
        }
    }

    // ── a desktop the test decides ────────────────────────────────────────

    /// The scripted desktop: one window in front, which is **both** what the
    /// next capture names and what every check compares against.
    ///
    /// One field, not two, because that is how a desktop behaves — and because
    /// a fake with a separate "last captured" and "currently in front" could let
    /// a scenario invent a world the app can never be in. A scenario that wants
    /// two windows moves the user to the second one and then back, which is two
    /// real events rather than one impossible one.
    ///
    /// The **decision** is still production code: `check` calls
    /// `output::classify` with a scripted observation, exactly as the real
    /// `validate_target` calls it with a real one. Only the observation is fake.
    struct ScriptedDesktop {
        /// The window the user is looking at. `None` is no foreground window at
        /// all: a locked desktop, or another session holding it.
        now: Mutex<Option<isize>>,
        /// Explicit window title. If None, defaults to `scripted window {hwnd:#x}`.
        title: Mutex<Option<String>>,
        /// False makes the window look closed.
        alive: AtomicBool,
        /// The executable the window in front belongs to, as a profile resolves
        /// it. `None` is the ordinary case — a process whose image could not be
        /// read — and it means an unknown application, which takes the general
        /// rules. Every scenario written before profiles existed gets `None`
        /// and therefore sees exactly the behaviour it asserted before.
        exe: Mutex<Option<std::path::PathBuf>>,
        /// Every capture, in order, so a scenario can see which window each
        /// dictation was given.
        seen: watch::Sender<Vec<TargetIdentity>>,
    }

    impl ScriptedDesktop {
        /// A desktop with one window in front, and nobody leaving it.
        fn new() -> Arc<Self> {
            let (seen, _rx) = watch::channel(Vec::new());
            Arc::new(Self {
                now: Mutex::new(Some(0x1000)),
                title: Mutex::new(None),
                alive: AtomicBool::new(true),
                exe: Mutex::new(None),
                seen,
            })
        }

        /// Test side: how many windows the loop has captured so far.
        ///
        /// Only read once the loop has stopped: before that, "none yet" and
        /// "never" are the same observation.
        fn captured(&self) -> usize {
            self.seen.borrow().len()
        }

        /// Test side: the user clicked into another window.
        fn moved_to(&self, hwnd: isize) {
            *self.now.lock().unwrap() = Some(hwnd);
        }

        /// Test side: dynamic window title update without changing handle or process.
        fn set_title(&self, title: impl Into<String>) {
            *self.title.lock().unwrap() = Some(title.into());
        }

        /// Test side: the window in front belongs to this executable.
        ///
        /// The handle and the pid are unchanged, which is the point — a profile
        /// binds to *which application* a window is, and that identity has to be
        /// readable without the window being a different one.
        fn belongs_to(&self, path: impl Into<std::path::PathBuf>) {
            *self.exe.lock().unwrap() = Some(path.into());
        }

        /// Test side: no window in front at all.
        fn went_dark(&self) {
            *self.now.lock().unwrap() = None;
        }

        /// Test side: the window that was in front is gone.
        fn closed(&self) {
            self.alive.store(false, Ordering::SeqCst);
        }

        /// Waits until the loop has captured `n` windows, and hands them back.
        ///
        /// The barrier a scenario needs before it may say anything about focus:
        /// a capture happens inside the loop's own turn, so reading the count
        /// right after sending the key press would be reading a hope.
        async fn wait_captured(&self, n: usize) -> Vec<TargetIdentity> {
            let mut seen = self.seen.subscribe();
            within(
                PATIENCE,
                "the loop to capture the destination window",
                async {
                    loop {
                        let here = seen.borrow_and_update().clone();
                        if here.len() >= n {
                            return here;
                        }
                        seen.changed().await.expect("the loop outlives the capture");
                    }
                },
            )
            .await
        }
    }

    impl TargetPort for ScriptedDesktop {
        fn capture(&self) -> Option<TargetIdentity> {
            // One pid per handle: this fake has no process table, and the
            // production rule compares the handle *and* the process.
            let hwnd = (*self.now.lock().unwrap())?;
            let title = self
                .title
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| format!("scripted window {hwnd:#x}"));
            let id = TargetIdentity {
                hwnd,
                pid: hwnd as u32,
                exe_path: self.exe.lock().unwrap().clone(),
                title_at_capture: title,
            };
            self.seen.send_modify(|seen| seen.push(id.clone()));
            Some(id)
        }

        fn check(&self, expected: &TargetIdentity) -> TargetValidity {
            let foreground = (*self.now.lock().unwrap()).map(|hwnd| (hwnd, hwnd as u32));
            crate::output::classify(
                expected,
                Observation {
                    window_alive: self.alive.load(Ordering::SeqCst),
                    foreground,
                },
            )
        }
    }

    /// The two fake platforms a scenario scripts, handed to the loop together.
    ///
    /// One value, because they are one rig: a loop driven against a scripted
    /// desktop and a real keyboard would not be a test of anything.
    struct Rig {
        desktop: Arc<ScriptedDesktop>,
        platform: Platform,
    }

    impl Rig {
        /// The ordinary rig: a desktop nobody leaves, and a keyboard that takes
        /// everything. Every scenario that is not about the destination or the
        /// insert uses this and says nothing about either.
        fn ordinary() -> Self {
            Self {
                desktop: ScriptedDesktop::new(),
                platform: Platform::default(),
            }
        }

        /// A rig whose desktop and keyboard the scenario chose.
        fn with(desktop: Arc<ScriptedDesktop>, platform: Platform) -> Self {
            Self { desktop, platform }
        }
    }
    // ── a clock the test moves by hand ────────────────────────────────────

    /// A clock the test controls, and can watch.
    ///
    /// The loop reads it for deadlines and waits on it for the wake-up; the
    /// test wants two more things from it: to move time without sleeping, and
    /// to see **when the loop asked for its next deadline** — because that
    /// moment is evidence that the previous loop turn finished, an expiry
    /// included. "Three seconds" is a value here, not a wait.
    struct ManualClock {
        inner: Arc<ManualInner>,
    }

    struct ManualInner {
        state: Mutex<ManualState>,
        /// Bumped whenever the state changes, so the loop's sleep future and
        /// the test's waiter both see it. A `watch` rather than a `Notify`: a
        /// change that arrives while a waiter is between checks is not lost,
        /// which is exactly the race a test tool must not have.
        changed: watch::Sender<u64>,
    }

    struct ManualState {
        now: Instant,
        /// Every deadline the loop asked to be woken at, in order. `None` is
        /// "no deadline" — the loop saying it has nothing to wait for.
        asked: Vec<Option<Instant>>,
        bump: u64,
    }

    impl ManualClock {
        fn new() -> Self {
            Self {
                inner: Arc::new(ManualInner {
                    state: Mutex::new(ManualState {
                        now: Instant::now(),
                        asked: Vec::new(),
                        bump: 0,
                    }),
                    changed: watch::channel(0).0,
                }),
            }
        }

        fn advance(&self, by: std::time::Duration) {
            let mut state = self.inner.state.lock().unwrap();
            state.now += by;
            state.bump += 1;
            let bump = state.bump;
            drop(state);
            let _ = self.inner.changed.send(bump);
        }

        fn asked(&self) -> Vec<Option<Instant>> {
            self.inner.state.lock().unwrap().asked.clone()
        }

        /// Waits until the loop's record of asked-for deadlines satisfies
        /// `want` — the barrier that turns "the expiry must have been handled"
        /// into an observation instead of a hope.
        async fn wait_asked(&self, want: impl Fn(&[Option<Instant>]) -> bool) {
            let mut changed = self.inner.changed.subscribe();
            within(
                PATIENCE,
                "the loop to ask the clock for its next deadline",
                async {
                    loop {
                        if want(&self.asked()) {
                            return;
                        }
                        if changed.changed().await.is_err() {
                            return;
                        }
                    }
                },
            )
            .await;
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            self.inner.state.lock().unwrap().now
        }
        fn sleep_until(
            &self,
            deadline: Option<Instant>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            let inner = self.inner.clone();
            {
                let mut state = inner.state.lock().unwrap();
                state.asked.push(deadline);
                state.bump += 1;
                let bump = state.bump;
                drop(state);
                let _ = inner.changed.send(bump);
            }
            let mut changed = inner.changed.subscribe();
            Box::pin(async move {
                let Some(at) = deadline else {
                    // No deadline: the loop has nothing to wait for. A branch
                    // that never resolves says that without a precondition.
                    return std::future::pending::<()>().await;
                };
                loop {
                    if inner.state.lock().unwrap().now >= at {
                        return;
                    }
                    if changed.changed().await.is_err() {
                        return;
                    }
                }
            })
        }
    }

    // ── the loop, wired ───────────────────────────────────────────────────

    /// The production coordinator, running, with the test holding its two ends.
    struct Harness {
        input: mpsc::UnboundedSender<Input>,
        discarded: Arc<Notify>,
        seen: watch::Receiver<Vec<Op>>,
        status: watch::Receiver<AppStatus>,
        loop_task: tokio::task::JoinHandle<Result<()>>,
        kept: Arc<Mutex<Vec<KeptRecord>>>,
        /// The review wire, exactly as the dashboard holds it: reading drafts
        /// from here is reading what the user would see, not a private copy.
        review: Arc<ReviewChannel>,
    }

    impl Harness {
        /// The drafts a user would currently be looking at.
        fn drafts(&self) -> Vec<review::PendingDraft> {
            self.review.snapshot().drafts
        }

        /// Waits for a draft to be raised, and returns it.
        async fn wait_draft(&self) -> review::PendingDraft {
            let review = self.review.clone();
            within(PATIENCE, "the loop to hold a text for the user", async {
                loop {
                    // Subscribe *before* looking, so a draft raised between the
                    // look and the wait is not missed: this is the whole race a
                    // polling test has to get right.
                    let mut changed = review.subscribe();
                    if let Some(draft) = review.snapshot().latest().cloned() {
                        return draft;
                    }
                    changed.changed().await.expect("the wire outlives the loop");
                }
            })
            .await
        }

        /// Waits until nothing is recorded as undelivered.
        ///
        /// The counterpart to [`Self::wait_ops`]: a keystroke and the bookkeeping
        /// that follows it are two moments, and a scenario that asserts on the
        /// second must wait for it rather than hope the first arrived first.
        async fn wait_nothing_kept(&self) {
            within(PATIENCE, "the undelivered records to clear", async {
                loop {
                    if self.kept_records().is_empty() {
                        return;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await;
        }

        /// Waits until `id` is no longer on offer.
        ///
        /// Needed because an answer is a *message*: reading the store in the
        /// same breath as sending one observes the world before the loop had a
        /// turn. Every "it must be gone by now" claim goes through here.
        async fn wait_resolved(&self, id: u64) {
            within(PATIENCE, "the draft to be resolved", async {
                loop {
                    if self.drafts().iter().all(|d| d.id != id) {
                        return;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await;
        }

        /// Answers the given draft, the way the review window does.
        fn answer(&self, command: review::ReviewCommand) {
            assert!(
                self.review.answer(command),
                "the loop was not reading its review wire"
            );
        }

        /// Waits until the keyboard has been asked to do at least `n` things.
        ///
        /// The negative assertions elsewhere cannot use this — "nothing was
        /// typed" is proved by looking after the loop has had turns, which is
        /// why they send a couple of events first. This is the positive half:
        /// the thing happened, and the wait says so rather than racing it.
        async fn wait_ops(&mut self, n: usize) -> Vec<Op> {
            within(PATIENCE, "the keyboard to be used", async {
                loop {
                    let here = self.seen.borrow_and_update().clone();
                    if here.len() >= n {
                        return here;
                    }
                    self.seen
                        .changed()
                        .await
                        .expect("the loop outlives the capture");
                }
            })
            .await
        }
    }

    impl Harness {
        fn start(port: ScriptedPort, discarded: Arc<Notify>, engine: &Arc<ScriptedEngine>) -> Self {
            Self::start_with(port, discarded, engine, Arc::new(SystemClock))
        }

        /// The same, with a clock the test moves by hand.
        fn start_with(
            port: ScriptedPort,
            discarded: Arc<Notify>,
            engine: &Arc<ScriptedEngine>,
            clock: Arc<dyn Clock>,
        ) -> Self {
            Self::start_scripted(port, discarded, engine, clock, false, Rig::ordinary(), false)
        }

        /// The same, with the rig the destination and insert scenarios need.
        ///
        /// The handles stay with the test, which is the point: the loop is the
        /// production one, and the desktop and the keyboard are the two things
        /// it asks questions of.
        fn start_rig(
            port: ScriptedPort,
            discarded: Arc<Notify>,
            engine: &Arc<ScriptedEngine>,
            rig: Rig,
        ) -> Self {
            Self::start_scripted(port, discarded, engine, Arc::new(SystemClock), false, rig, false)
        }

        /// A loop whose *policy* the scenario wrote.
        ///
        /// The clock is real and the port is the ordinary scripted one, because
        /// a profile is not a timing question: what is asserted is which rules
        /// one dictation ran under, and that is decided from the settings and
        /// the captured destination alone. The dictionary is the real seed
        /// list, so a scenario can tell "the dictionary was applied" from "it
        /// was not" — which is the only way a profile's `raw` mode is
        /// observable from out here.
        fn start_profiled(
            port: ScriptedPort,
            discarded: Arc<Notify>,
            engine: &Arc<ScriptedEngine>,
            desktop: Arc<ScriptedDesktop>,
            settings: Settings,
        ) -> Self {
            let review = ReviewChannel::new();
            let session = SessionDriver::new(&settings.hotkey);
            let (sink, seen) = RecordingSink::scripted(Platform::default());
            let status = Arc::new(StatusChannel::new("scripted"));
            let mut status_rx = status.subscribe();
            let _ = status_rx.borrow_and_update();

            let coordinator = Coordinator::new(
                port,
                Speech {
                    router: engine.router(),
                    normalizer: Arc::new(Normalizer::new()),
                    dictionary: Arc::new(RwLock::new(Dictionary::with_defaults())),
                    settings: Arc::new(settings),
                },
                session,
                status,
                sink,
                desktop,
                Arc::new(SystemClock),
                review.clone(),
            );
            let kept = coordinator.kept_handle();
            let (input, input_rx) = mpsc::unbounded_channel();
            let loop_task = tokio::spawn(coordinator.run(input_rx));
            Self {
                input,
                discarded,
                seen,
                status: status_rx,
                loop_task,
                kept,
                review,
            }
        }

        /// The same, with the hands-free double-tap latch **on**.
        ///
        /// Off everywhere else, so that releasing the record key ends the
        /// recording: the latch's timing rules are covered in `session.rs`, and
        /// leaving it on would make every other scenario wait out a real-time
        /// window for a second tap. The one scenario that needs the latch needs
        /// it on *and* the test's clock, which is what makes its two taps a pair
        /// by arithmetic instead of by timing.
        fn start_latched_with(
            port: ScriptedPort,
            discarded: Arc<Notify>,
            engine: &Arc<ScriptedEngine>,
            clock: Arc<dyn Clock>,
        ) -> Self {
            Self::start_scripted(port, discarded, engine, clock, true, Rig::ordinary(), false)
        }

        fn start_mode(
            port: ScriptedPort,
            discarded: Arc<Notify>,
            engine: &Arc<ScriptedEngine>,
            desktop: Arc<ScriptedDesktop>,
            text_mode: &str,
        ) -> Self {
            let mut settings = Settings::default();
            settings.text.mode = text_mode.into();
            settings.hotkey.double_tap_latch = false;
            let session = SessionDriver::new(&settings.hotkey);

            let (sink, seen) = RecordingSink::scripted(Platform::default());
            let status = Arc::new(StatusChannel::new("scripted"));
            let mut status_rx = status.subscribe();
            let _ = status_rx.borrow_and_update();

            let dict = if text_mode == "raw" {
                Dictionary::new(Vec::new())
            } else {
                Dictionary::with_defaults()
            };

            let coordinator = Coordinator::new(
                port,
                Speech {
                    router: engine.router(),
                    normalizer: Arc::new(Normalizer::new()),
                    dictionary: Arc::new(RwLock::new(dict)),
                    settings: Arc::new(settings),
                },
                session,
                status,
                sink,
                desktop,
                Arc::new(SystemClock),
                crate::state::ReviewChannel::new(),
            );
            let kept = coordinator.kept_handle();
            let (input, input_rx) = mpsc::unbounded_channel();
            let loop_task = tokio::spawn(coordinator.run(input_rx));
            Self {
                input,
                discarded,
                seen,
                status: status_rx,
                loop_task,
                kept,
                review: crate::state::ReviewChannel::new(),
            }
        }

        fn start_scripted(
            port: ScriptedPort,
            discarded: Arc<Notify>,
            engine: &Arc<ScriptedEngine>,
            clock: Arc<dyn Clock>,
            double_tap_latch: bool,
            rig: Rig,
            review_before_insert: bool,
        ) -> Self {
            let review = ReviewChannel::new();
            let mut settings = Settings::default();
            // `raw` keeps the engine's own string, so what a scenario asserts
            // about the typed text is about coordination and not about Persian
            // text rules (those have their own tests).
            settings.text.mode = "raw".into();
            settings.hotkey.double_tap_latch = double_tap_latch;
            settings.gui.review_before_insert = review_before_insert;
            let session = SessionDriver::new(&settings.hotkey);

            let (sink, seen) = RecordingSink::scripted(rig.platform);
            let status = Arc::new(StatusChannel::new("scripted"));
            let mut status_rx = status.subscribe();
            let _ = status_rx.borrow_and_update();

            let coordinator = Coordinator::new(
                port,
                Speech {
                    router: engine.router(),
                    normalizer: Arc::new(Normalizer::new()),
                    dictionary: Arc::new(RwLock::new(Dictionary::new(Vec::new()))),
                    settings: Arc::new(settings),
                },
                session,
                status,
                sink,
                rig.desktop,
                clock,
                review.clone(),
            );
            let kept = coordinator.kept_handle();
            let (input, input_rx) = mpsc::unbounded_channel();
            let loop_task = tokio::spawn(coordinator.run(input_rx));
            Self {
                input,
                discarded,
                seen,
                status: status_rx,
                loop_task,
                kept,
                review,
            }
        }

        fn kept_records(&self) -> Vec<KeptRecord> {
            self.kept
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        /// Waits until at least `n` undelivered records have been kept.
        ///
        /// The kept list sits behind a plain mutex with no change notification,
        /// so this polls — deliberately, and not as a sleep-and-hope. The
        /// subtlety it exists for: a record is pushed *before* that chunk's
        /// error is shown, but a **later** chunk's record is a later moment
        /// still. Waiting for the badge to read `Error` therefore proves only
        /// that the *first* refusal finished, and a scenario with two refused
        /// chunks that asserts immediately after samples the loop mid-flight —
        /// seeing one record and calling it a lost record.
        async fn kept_until(&mut self, n: usize) -> Vec<KeptRecord> {
            within(
                PATIENCE,
                &format!("{n} refused-text record(s) to be kept"),
                async {
                    loop {
                        let records = self.kept_records();
                        if records.len() >= n {
                            return records;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                },
            )
            .await
        }

        fn event(&self, ev: HotkeyEvent) {
            self.input
                .send(Input::Event(ev))
                .expect("the loop is still running");
        }
        fn tick(&self) {
            self.input
                .send(Input::Tick)
                .expect("the loop is still running");
        }

        /// Closes the event source, the way a hotkey thread that gave up does.
        ///
        /// The sender the test holds is dropped, and the loop's next `recv` is
        /// the shutdown it was written to hear. The replacement sender keeps
        /// [`Harness::event`] honest: a send after this fails loudly instead of
        /// pretending a loop is still reading.
        fn close_source(&mut self) {
            self.input = mpsc::unbounded_channel::<Input>().0;
        }
        fn state(&self) -> AppState {
            self.status.borrow().state.clone()
        }
        async fn discarded(&self) {
            within(PATIENCE, "the cancel to reach the hardware", async {
                self.discarded.notified().await;
            })
            .await;
        }

        /// Waits until the sink has logged at least `n` operations.
        ///
        /// This is the positive barrier every scenario ends on. It is also what
        /// makes "nothing was typed" a real observation: results are applied in
        /// hand-out order, so seeing the *later* one typed proves every earlier
        /// one has already been applied — and was therefore dropped, not typed.
        async fn ops_until(&mut self, n: usize) -> Vec<Op> {
            within(
                PATIENCE,
                "the sink to record the expected keystrokes",
                async {
                    loop {
                        let seen = self.seen.borrow_and_update().clone();
                        if seen.len() >= n {
                            return seen;
                        }
                        self.seen
                            .changed()
                            .await
                            .expect("the sink outlives the loop");
                    }
                },
            )
            .await
        }

        /// Stops the loop and hands back every keystroke it managed to send.
        ///
        /// Reading the sink *after* the loop is gone is the only way to say
        /// "nothing was typed from here on": before that, absence is just not
        /// having happened yet.
        async fn quit(self) -> Vec<Op> {
            self.event(HotkeyEvent::Quit);
            self.loop_task
                .await
                .expect("the loop task itself")
                .expect("the loop returns Ok");
            self.seen.borrow().clone()
        }

        /// Waits until the badge satisfies `want`.
        async fn wait_for_state(&mut self, want: impl Fn(&AppState) -> bool) -> bool {
            within(PATIENCE, "the badge to reach the expected state", async {
                loop {
                    let here = {
                        let snapshot = self.status.borrow();
                        want(&snapshot.state)
                    };
                    if here {
                        return true;
                    }
                    if self.status.changed().await.is_err() {
                        return false;
                    }
                }
            })
            .await
        }

        /// Waits until the hands-free badge reads `want`.
        ///
        /// Only ever called when the badge currently reads the other thing, so
        /// the first check cannot satisfy it: this waits for a *change*, not for
        /// a value that was already there.
        async fn wait_for_latched(&mut self, want: bool) -> bool {
            within(PATIENCE, "the hands-free badge to change", async {
                loop {
                    if self.status.borrow().latched == want {
                        return true;
                    }
                    if self.status.changed().await.is_err() {
                        return false;
                    }
                }
            })
            .await
        }
    }

    /// Sample count that names one scripted conversion.
    fn n(tag: u32) -> usize {
        16_000 * tag as usize
    }

    /// Audio that names its conversion by its length.
    fn speech(tag: u32) -> AudioUtterance {
        AudioUtterance {
            samples: vec![0.1; n(tag)],
            sample_rate: 16_000,
        }
    }
    fn chunk(tag: u32) -> PollOutcome {
        PollOutcome::Chunk { audio: speech(tag) }
    }
    fn finished(tag: u32) -> PollOutcome {
        PollOutcome::Finished {
            audio: Some(speech(tag)),
        }
    }
    fn ends(tag: u32) -> Option<AudioUtterance> {
        Some(speech(tag))
    }

    fn typed(text: &str) -> Vec<Op> {
        vec![Op::Type(text.to_string())]
    }

    // ── 1. nothing broke ──────────────────────────────────────────────────

    /// The reason the cancel gate is allowed to exist at all: an ordinary
    /// dictation must still reach the window, and the app must end up ready for
    /// the next one.
    ///
    /// The second dictation is what makes the badge assertion deterministic: its
    /// piece is a mid-session chunk, which by rule writes no visible state — so
    /// once its text has been typed, the badge can only be what the *first*
    /// dictation failed to write. It is `Recording`, because that dictation's
    /// ending arrived after a new recording had started, and an older result
    /// has no business moving a badge that is no longer its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_normal_dictation_still_reaches_the_keyboard() {
        let (h_port, discarded) = port(vec![chunk(2)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام دنیا",
            },
            Step::Text {
                samples: n(2),
                text: "ادامه",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        h.event(HotkeyEvent::RecordDown);
        h.tick();

        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type("سلام دنیا".to_string()),
                Op::Type(" ادامه".to_string()),
            ]
        );
        assert_eq!(
            h.state(),
            AppState::Recording,
            "the first dictation's ending overwrote the new recording's badge"
        );
        h.quit().await;
    }

    // ── 2. cancel while still recording ───────────────────────────────────

    /// Escape during a live recording: the session is discarded and nothing is
    /// typed. The second dictation is the barrier — by the time its text lands,
    /// the cancel has certainly been applied.
    ///
    /// No badge assertion here on purpose: reading the badge straight after the
    /// cancel would be reading it in the middle of the loop's own work. The
    /// badge claim is asserted where it is deterministic, in
    /// `a_failed_cancelled_chunk_leaves_a_new_recording_alone`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelling_a_live_recording_types_nothing() {
        let (h_port, discarded) = port(vec![], vec![ends(2)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(2),
            text: "بعدی",
        }]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::Cancel);
        h.discarded().await;

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            typed("بعدی"),
            "the cancelled dictation must not type anything at all"
        );
        h.quit().await;
    }

    // ── 3. cancel while the engine is working (the gap this stage is about) ─

    /// The scenario the whole stage exists for. Escape arrives while the
    /// recogniser is still turning audio into text; the answer lands afterwards
    /// and must be dropped whole — no typing, no backspace, no badge, no seam
    /// memory.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelling_while_the_engine_works_drops_the_late_text() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "دیررس",
            },
            Step::Text {
                samples: n(2),
                text: "بعدی",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        // The conversion is now provably inside the engine and stuck there.
        gate.wait_arrived();

        h.event(HotkeyEvent::Cancel);
        h.discarded().await;
        gate.open();

        // The barrier: a second dictation, applied after this one by order.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            typed("بعدی"),
            "the late answer of a cancelled session reached the keyboard"
        );
        h.quit().await;
    }

    // ── 4. the cancel is aimed, not global ────────────────────────────────

    /// A cancel must take the session the user was looking at and leave an
    /// older dictation alone: this one's text was still wanted, and a blanket
    /// "cancelled something, drop everything" rule would throw away speech the
    /// user already dictated.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancel_takes_the_newest_session_and_leaves_the_older_one_alone() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "قدیمی",
            },
            Step::Text {
                samples: n(2),
                text: "جدید",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        gate.wait_arrived();

        // A second dictation starts while the first is still converting.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::Cancel);
        h.discarded().await;
        gate.open();

        // The second dictation is opened but never released, so it has nothing
        // to say. Its very existence is the assertion: had the press not opened
        // it, Escape would have cancelled the *first* session and its text below
        // would never have been typed at all.
        assert_eq!(
            h.ops_until(1).await,
            typed("قدیمی"),
            "the older session's answer was still wanted when the newer one was cancelled"
        );
        h.quit().await;
    }

    // ── 5. the loop stays open while the engine works ─────────────────────

    /// The architecture's reason to exist. The first conversion is stuck in the
    /// engine the whole time, and a whole second dictation is pressed, released
    /// and queued anyway — the loop reads it because it never awaited the
    /// conversion to begin with. The second *answer* then takes its turn behind
    /// the first: conversions run one at a time, in hand-out order.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_loop_still_reads_events_while_the_engine_works() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "اول",
            },
            Step::Text {
                samples: n(2),
                text: "دوم",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        gate.wait_arrived();

        // A whole second dictation, start to finish, while the first one waits.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        gate.open();
        assert_eq!(
            h.ops_until(2).await,
            vec![Op::Type("اول".to_string()), Op::Type(" دوم".to_string()),],
            "answers must be applied in the order the work was handed out"
        );
        h.quit().await;
    }

    // ── 6. ended by silence, then cancelled before the answer ─────────────

    /// The same gap on the other ending. Nothing the user pressed ends the
    /// dictation — the VAD does — so the cancel lands with no key edge to go
    /// with it. A rule that only fires while the microphone is open answers
    /// nothing here and lets the late text in.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn silence_then_cancel_before_the_answer_types_nothing() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![finished(1)], vec![ends(2)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "دیررس",
            },
            Step::Text {
                samples: n(2),
                text: "بعدی",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the port reports the endpoint: recording stops on its own
        gate.wait_arrived();

        h.event(HotkeyEvent::Cancel);
        h.discarded().await;
        gate.open();

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            typed("بعدی"),
            "a session ended by silence was not cancellable"
        );
        h.quit().await;
    }

    // ── 7. a stalled mid-session chunk and the end of the recording ────────

    /// A long dictation cuts into chunks. Here the first chunk is still inside
    /// the engine when the key comes up and the final piece is handed over — so
    /// both answers exist at once, and neither may be lost.
    ///
    /// What this proves is that nothing is dropped and both arrive in hand-out
    /// order. The ordering rule itself is pinned directly by
    /// [`a_result_that_arrives_early_waits_for_its_turn`], which can hand the
    /// queue a chosen order.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_stalled_mid_chunk_and_the_end_of_recording_lose_nothing() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(2)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "قطعه",
            },
            Step::Text {
                samples: n(2),
                text: "پایان",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the port flushes a mid-session chunk
        gate.wait_arrived();

        // The recording ends while the chunk is still inside the engine. The
        // final conversion cannot even start until the chunk finishes, and must
        // still be applied second.
        h.event(HotkeyEvent::RecordUp);
        gate.open();
        assert_eq!(
            h.ops_until(2).await,
            vec![Op::Type("قطعه".to_string()), Op::Type(" پایان".to_string()),],
            "a chunk or the final piece was lost"
        );
        h.quit().await;
    }

    // ── 7b. the ordering rule, asked directly ────────────────────────────

    /// The queue, on its own.
    ///
    /// Every scenario above drives the real loop, and that is the right place to
    /// check behaviour. It is the wrong place to check *this*, for a reason worth
    /// stating: two conversions are two tasks, and no amount of gating in the
    /// engine lets a test decide which of them reaches the channel first. So the
    /// order those scenarios observe is whatever the runtime produced — which is
    /// exactly how `C29` slipped through a suite that otherwise bites.
    ///
    /// So the rule is asked here, of the same production code the loop calls:
    /// hand [`Coordinator::drain_in_order`] the *second* answer first, and it has
    /// to hold it until the first turns up. No session is open, so
    /// `wanted_by` accepts both — this is about order, not about cancellation.
    /// Both answers carry a destination the scripted desktop still agrees with,
    /// because a result whose window is not in front is refused before it is
    /// ever ordered: the insert rules must hold, or this test would be measuring
    /// two decisions at once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_result_that_arrives_early_waits_for_its_turn() {
        let (h_port, _discarded) = port(vec![], vec![]);
        let h_engine = engine(vec![]);
        let mut settings = Settings::default();
        settings.text.mode = "raw".into();
        settings.hotkey.double_tap_latch = false;
        let session = SessionDriver::new(&settings.hotkey);
        let (sink, mut seen) = RecordingSink::pair();
        let _ = seen.borrow_and_update();
        let status = Arc::new(StatusChannel::new("scripted"));
        let targets = ScriptedDesktop::new();
        let target = targets
            .capture()
            .expect("the scripted desktop has a window");
        let mut coordinator = Coordinator::new(
            h_port,
            Speech {
                router: h_engine.router(),
                normalizer: Arc::new(Normalizer::new()),
                dictionary: Arc::new(RwLock::new(Dictionary::new(Vec::new()))),
                settings: Arc::new(settings),
            },
            session,
            status,
            sink,
            targets.clone(),
            Arc::new(SystemClock),
            crate::state::ReviewChannel::new(),
        );

        let answer = |seq: u64, text: &str| ConversionResult {
            seq,
            session: None,
            chunk: None,
            target: Some(target.clone()),
            raw: text.to_string(),
            processed: text.to_string(),
            is_final: true,
            failure: None,
        };

        coordinator
            .results_tx
            .send(answer(1, "دوم"))
            .expect("the coordinator is holding its own sender");
        let arrived = coordinator
            .results
            .recv()
            .await
            .expect("the second answer is queued");
        coordinator.drain_in_order(arrived).await;
        assert!(
            seen.borrow().is_empty(),
            "the second answer was typed before the first one arrived"
        );

        coordinator
            .results_tx
            .send(answer(0, "اول"))
            .expect("the coordinator is holding its own sender");
        let arrived = coordinator
            .results
            .recv()
            .await
            .expect("the first answer is queued");
        coordinator.drain_in_order(arrived).await;

        assert_eq!(
            seen.borrow().clone(),
            vec![Op::Type("اول".to_string()), Op::Type(" دوم".to_string())],
            "both answers must land, and in the order the work was handed out"
        );
    }

    // ── 8. a cancelled chunk's failure must not reach the new recording ───

    /// The engine fails on a chunk the user had already cancelled, and the
    /// answer arrives after a new dictation has started. A failure badge here
    /// would blame the app for text nobody asked for and would sit on top of the
    /// live recording — so the badge must not move at all.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failed_cancelled_chunk_leaves_a_new_recording_alone() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(1), chunk(2)], vec![]);
        let h_engine = engine(vec![
            Step::GatedFail {
                samples: n(1),
                gate: gate.clone(),
            },
            Step::Text {
                samples: n(2),
                text: "تازه",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // a chunk is handed to the engine
        gate.wait_arrived();

        h.event(HotkeyEvent::Cancel);
        h.discarded().await;
        gate.open(); // the failure is produced now

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the new dictation's own chunk; this is the barrier
        assert_eq!(h.ops_until(1).await, typed("تازه"));

        assert_eq!(
            h.state(),
            AppState::Recording,
            "a cancelled chunk's failure reached the badge of a live recording"
        );
        h.quit().await;
    }

    // ── 9. quitting with work in flight ───────────────────────────────────

    /// The exit policy, stated as a test: a quit stops the loop reading, waits
    /// for the conversion that was already handed to an engine, and types none
    /// of it. The user has left the window; a dictation arriving afterwards
    /// would land somewhere they are no longer looking.
    ///
    /// Read the comment on `biased;` before trusting this as a *guard* on that
    /// keyword: it is a test of the behaviour, and behaviour tests can stop
    /// being able to see the thing they were written for. This one did — see
    /// `mutation-check-coordinator.sh`, where the removal is recorded as a gap
    /// rather than as a canary.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn nothing_is_typed_after_quit() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Gated {
            samples: n(1),
            gate: gate.clone(),
            text: "بعد از خروج",
        }]);
        let h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        gate.wait_arrived();

        h.event(HotkeyEvent::Quit);
        gate.open(); // the answer arrives after the quit was requested

        assert_eq!(
            h.quit().await,
            Vec::new(),
            "text reached the keyboard after the user had already quit"
        );
    }

    // ── 10. an engine that dies is reported ───────────────────────────────

    /// A recogniser that crashes must surface as a failure the user can read,
    /// not as silence. The router turns the panic into an error; the coordinator
    /// has to put it on the badge and type nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_engine_that_panics_is_reported_rather_than_swallowed() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Panics { samples: n(1) }]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(_))).await,
            "a crashed engine was swallowed instead of reported"
        );
        assert_eq!(h.quit().await, Vec::new());
    }

    // ── 11. an utterance with nothing in it ───────────────────────────────

    /// Every ending has to reach the same place. Here the VAD says the
    /// recording held no speech — a tap, a cough — so no engine is asked, and
    /// the session still closes. Handing that silence to a recogniser instead
    /// would come back as an error badge for a dictation nobody made.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_empty_utterance_closes_its_session_without_an_engine_call() {
        let (h_port, discarded) = port(
            vec![PollOutcome::Finished { audio: None }, chunk(2)],
            vec![],
        );
        // Only one answer is scripted: if the empty utterance reached the engine
        // it would ask for audio nobody wrote an answer for.
        let h_engine = engine(vec![Step::Text {
            samples: n(2),
            text: "تازه",
        }]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the port reports an endpoint and no speech to convert
        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the next dictation's own chunk: the barrier

        assert_eq!(h.ops_until(1).await, typed("تازه"));
        assert_eq!(
            h_engine.calls(),
            1,
            "an utterance the VAD called empty still reached the engine"
        );
        assert_eq!(
            h.state(),
            AppState::Recording,
            "the live recording was disturbed by an empty utterance"
        );
        h.quit().await;
    }

    // ── 12. an ending with no audio, ordered behind a stalled chunk ────────

    /// The port ends the dictation with nothing to convert — a tap, a breath —
    /// while a chunk from the same dictation is still inside the engine.
    ///
    /// The ending is still an *ordered* event: it takes its turn behind the
    /// chunk. Closing the session immediately instead completed it while its
    /// chunk was in flight, and the chunk's text was refused afterwards as a
    /// late result — text the user really spoke, deleted by a dictation that
    /// had no last word.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_ending_with_no_audio_waits_for_the_chunk_before_it() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(1)], vec![None, ends(2)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "قطعه",
            },
            Step::Text {
                samples: n(2),
                text: "بعدی",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the port hands over a mid-session chunk
        gate.wait_arrived();

        // The recording ends with nothing worth converting.
        h.event(HotkeyEvent::RecordUp);
        gate.open();

        // The chunk's text lands first — the ending waits for it.
        assert_eq!(h.ops_until(1).await, typed("قطعه"));

        // A second dictation is the barrier: its answer can only be applied
        // after the first session settled, so seeing it proves the queue moved.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(2).await,
            vec![Op::Type("قطعه".to_string()), Op::Type(" بعدی".to_string())],
            "an ending with no audio either ate the chunk before it or blocked the queue"
        );
        assert_eq!(
            h.quit().await,
            vec![Op::Type("قطعه".to_string()), Op::Type(" بعدی".to_string())],
            "the chunk's text was typed more than once, or something else was"
        );
    }

    // ── 13. a seam belongs to one session only ────────────────────────────

    /// Two dictations that share a phrase. The first one's answer is still in
    /// the engine when the second starts recording, and its text lands while the
    /// second is live.
    ///
    /// The seam must not carry across: an older dictation's tail must never be
    /// used to delete the newer one's words. With one stitcher for the whole
    /// process, the older answer's tail («دنیا») was still remembered when the
    /// newer chunk arrived, and the newer chunk's opening word was dropped as
    /// overlap — the user's word, eaten by somebody else's dictation.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_seam_never_belongs_to_another_session() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(2)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "سلام دنیا",
            },
            Step::Text {
                samples: n(2),
                text: "دنیا ادامه",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        gate.wait_arrived();

        // The second dictation starts while the first is still converting.
        h.event(HotkeyEvent::RecordDown);
        h.tick(); // its chunk, queued behind the first answer

        gate.open();

        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type("سلام دنیا".to_string()),
                Op::Type(" دنیا ادامه".to_string()),
            ],
            "the first session's seam memory deleted the second session's words"
        );
        assert_eq!(
            h.state(),
            AppState::Recording,
            "the older session's answer overwrote the recording that started after it"
        );
        h.quit().await;
    }

    // ── 14. an older failure, a live recording ─────────────────────────────

    /// The same, with a failure: the previous dictation's engine died, and its
    /// answer arrives after a new recording started. The failure belongs to the
    /// old session — it must not turn the live recording's badge red.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_older_failure_leaves_a_live_recording_alone() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(2)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::GatedFail {
                samples: n(1),
                gate: gate.clone(),
            },
            Step::Text {
                samples: n(2),
                text: "تازه",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        gate.wait_arrived();

        h.event(HotkeyEvent::RecordDown);
        gate.open(); // the old session's failure lands now
        h.tick(); // the new recording's own chunk: the barrier

        assert_eq!(h.ops_until(1).await, typed("تازه"));
        assert_eq!(
            h.state(),
            AppState::Recording,
            "an older session's failure reached the badge of a live recording"
        );
        h.quit().await;
    }

    // ── 15. an error window is a loop event ────────────────────────────────

    /// The failure badge must not be a dead end for three *real* seconds: its
    /// window closes as a loop event. The clock is the test's, so "three
    /// seconds" is a value it moves — and the evidence that the loop handled
    /// the closure is the loop asking the clock for its next deadline, which
    /// only happens on a later loop turn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_error_window_closes_as_a_loop_event() {
        let clock = Arc::new(ManualClock::new());
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Fail {
            samples: n(1),
            why: "scripted failure",
        }]);
        let mut h = Harness::start_with(h_port, discarded, &h_engine, clock.clone());

        let t0 = clock.now();
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(_))).await,
            "the failure never reached the badge"
        );
        let deadline = Some(t0 + ERROR_READABLE);
        clock
            .wait_asked(|asked| asked.last() == Some(&deadline))
            .await;

        clock.advance(ERROR_READABLE + std::time::Duration::from_secs(1));
        clock
            .wait_asked(|asked| asked.last() != Some(&deadline))
            .await;

        assert_eq!(
            h.state(),
            AppState::Idle,
            "the failure was still on the badge after its window closed"
        );
        h.quit().await;
    }

    /// The window that was armed *before* a new recording started must not
    /// take that recording's badge down when it closes.
    ///
    /// The barrier is the same one: after the expiry, the loop asks the clock
    /// for its next deadline, and only then does the test read the badge.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_expired_window_does_not_take_a_new_recording_down() {
        let clock = Arc::new(ManualClock::new());
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Fail {
                samples: n(1),
                why: "scripted failure",
            },
            Step::Fail {
                samples: n(2),
                why: "scripted failure",
            },
        ]);
        let mut h = Harness::start_with(h_port, discarded, &h_engine, clock.clone());

        let t0 = clock.now();
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(_))).await,
            "the failure never reached the badge"
        );

        // A new dictation starts while the failure is still on the badge.
        h.event(HotkeyEvent::RecordDown);
        assert!(
            h.wait_for_state(|s| *s == AppState::Recording).await,
            "the new dictation never started"
        );

        let first_at = t0 + ERROR_READABLE;
        let deadline = Some(first_at);
        clock
            .wait_asked(|asked| asked.last() == Some(&deadline))
            .await;
        clock.advance(ERROR_READABLE + std::time::Duration::from_secs(1));
        clock
            .wait_asked(|asked| asked.last() != Some(&deadline))
            .await;

        assert_eq!(
            h.state(),
            AppState::Recording,
            "a window armed before the recording took its badge down"
        );

        // And the mechanism still closes the *active* error's window: this
        // recording's own failure clears like any other. Its window is the
        // first deadline later than the one that just closed — waiting for it
        // to appear is also the barrier that proves the failure landed.
        h.event(HotkeyEvent::RecordUp);
        clock
            .wait_asked(|asked| asked.iter().flatten().any(|d| *d > first_at))
            .await;
        let second = clock
            .asked()
            .into_iter()
            .flatten()
            .find(|d| *d > first_at)
            .expect("the second window was armed");
        clock
            .wait_asked(|asked| asked.last() == Some(&Some(second)))
            .await;
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(_))).await,
            "the recording's own failure never reached the badge"
        );

        clock.advance(ERROR_READABLE + std::time::Duration::from_secs(1));
        clock.wait_asked(|asked| asked.last() == Some(&None)).await;
        assert_eq!(h.state(), AppState::Idle, "the active error did not clear");
        h.quit().await;
    }

    /// Two failures, two windows: the older window must not clear the newer
    /// error. This is what the version is for — the badge still shows *an*
    /// error, and only the window armed for that exact one may take it down.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_expired_window_does_not_clear_a_newer_error() {
        let clock = Arc::new(ManualClock::new());
        let (h_port, discarded) = port(vec![chunk(2)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Fail {
                samples: n(1),
                why: "the first failure",
            },
            Step::Fail {
                samples: n(2),
                why: "the second failure",
            },
        ]);
        let mut h = Harness::start_with(h_port, discarded, &h_engine, clock.clone());

        let t0 = clock.now();
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("first")))
                .await,
            "the first failure never reached the badge"
        );
        let first = Some(t0 + ERROR_READABLE);

        // Half-way through the first window, a new recording starts and its own
        // chunk fails: a second error, with a window of its own. Its message is
        // different, so the badge itself says that failure landed — which is the
        // barrier a bare "is it an error?" check can never give, because the
        // first error would satisfy it forever.
        clock.advance(ERROR_READABLE / 2);
        h.event(HotkeyEvent::RecordDown);
        h.tick();
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("second")))
                .await,
            "the second failure never reached the badge"
        );

        // Both windows are open, and the earlier one still owns the next
        // wake-up: nothing has passed it yet.
        clock.wait_asked(|asked| asked.last() == Some(&first)).await;

        // The first window closes now. Its error is no longer the active one, so
        // the badge must survive it — and the next deadline the loop asks for is
        // the second window's, which is proof the first one closed.
        clock.advance(ERROR_READABLE / 2 + std::time::Duration::from_secs(1));
        clock
            .wait_asked(|asked| asked.last().is_some_and(|d| *d != first))
            .await;
        assert!(
            matches!(h.state(), AppState::Error(m) if m.contains("second")),
            "the older window cleared a newer error: {:?}",
            h.state()
        );

        // The second window closes in its own turn, and *that* one may clear.
        clock.advance(ERROR_READABLE + std::time::Duration::from_secs(1));
        clock.wait_asked(|asked| asked.last() == Some(&None)).await;
        assert_eq!(h.state(), AppState::Idle);
        h.quit().await;
    }

    // ── 16. a cancel sets aside the chunks that never started ─────────────

    /// A cancel with work still waiting its turn. The in-flight chunk cannot be
    /// un-run, but the queued ones must never reach the engine: they are set
    /// aside with the session, and their audio is dropped rather than kept for
    /// a recovery nobody asked for.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancel_sets_aside_the_chunks_that_never_started() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(1), chunk(2), chunk(3), chunk(4)], vec![]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "قدیمی",
            },
            Step::Text {
                samples: n(4),
                text: "تازه",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // chunk 1: handed to the engine, and stuck there
        gate.wait_arrived();
        h.tick(); // chunks 2 and 3: queued behind it
        h.tick();

        h.event(HotkeyEvent::Cancel);
        h.discarded().await;
        gate.open(); // the in-flight chunk answers after the cancel

        // A new recording: its own chunk is the barrier.
        h.event(HotkeyEvent::RecordDown);
        h.tick();

        assert_eq!(h.ops_until(1).await, typed("تازه"));
        assert_eq!(
            h.quit().await,
            typed("تازه"),
            "the cancelled session's queued audio ran, or was typed"
        );
        assert_eq!(
            h_engine.calls(),
            2,
            "the queued chunks of the cancelled session still reached the engine"
        );
    }

    // ── 17. a conversion that dies does not leave a hole ──────────────────

    /// A conversion task that dies from the inside: the engine's health check —
    /// which runs on the conversion task, not on the blocking pool — explodes.
    ///
    /// The failure has to be tied to *that* job, reported once, and the queue
    /// has to move on. Dropping the answer instead left the order number empty
    /// and every later result waited behind it forever: transcription was dead
    /// for the rest of the process, silently.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_conversion_that_dies_is_reported_and_does_not_stall_the_queue() {
        let (h_port, discarded) = port(vec![chunk(1), chunk(2)], vec![]);
        let h_engine = engine_with_broken_health(vec![Step::Text {
            samples: n(2),
            text: "تازه",
        }]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the engine's health check explodes here

        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(_))).await,
            "a conversion task that died was swallowed"
        );

        // The next dictation still runs: the queue did not stop at the hole.
        h.event(HotkeyEvent::RecordUp);
        h.event(HotkeyEvent::RecordDown);
        h.tick();
        assert_eq!(h.ops_until(1).await, typed("تازه"));
        assert_eq!(h.state(), AppState::Recording);
        h.quit().await;
    }

    // ── 18. the hands-free badge follows the rules, not the effects ──────────

    /// Two taps inside the window and the recording keeps running after the key
    /// is gone, so the badge has to say so — and the badge is the *only* thing
    /// that says it, because this decision has no effect at all for the loop to
    /// perform. A badge published from inside an effect arm stayed dark while
    /// the microphone was genuinely open: hands-free dictation with no sign that
    /// it was hands-free.
    ///
    /// Deterministic by construction, which is why the loop stamps events with
    /// its own clock: "two taps inside `double_tap_window_ms`" is arithmetic the
    /// test writes down (100 ms of a 600 ms window), not a race with how fast the
    /// machine happens to be. No `sleep` anywhere in this test, and nothing in
    /// it reads the wall clock.
    ///
    /// Both ways out are checked, because both reset the rules: the next press
    /// ends a hands-free session, and a cancel ends it too.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_double_tap_lights_the_hands_free_badge_and_every_way_out_darkens_it() {
        let clock = Arc::new(ManualClock::new());
        let (h_port, discarded) = port(vec![], vec![None, None]);
        let h_engine = engine(vec![]);
        let mut h = Harness::start_latched_with(h_port, discarded, &h_engine, clock.clone());

        h.event(HotkeyEvent::RecordDown);
        assert!(
            h.wait_for_state(|s| *s == AppState::Recording).await,
            "the tap never opened the microphone"
        );
        h.event(HotkeyEvent::RecordUp);
        clock.advance(std::time::Duration::from_millis(100));
        h.event(HotkeyEvent::RecordDown);

        assert!(
            h.wait_for_latched(true).await,
            "two taps inside the window did not light the hands-free badge"
        );

        h.event(HotkeyEvent::RecordDown);
        assert!(
            h.wait_for_latched(false).await,
            "the next press ended the session but left the badge lit"
        );

        // The same pair again, this time ended the other way.
        h.event(HotkeyEvent::RecordDown);
        assert!(h.wait_for_state(|s| *s == AppState::Recording).await);
        h.event(HotkeyEvent::RecordUp);
        clock.advance(std::time::Duration::from_millis(100));
        h.event(HotkeyEvent::RecordDown);
        assert!(
            h.wait_for_latched(true).await,
            "the second double-tap did not latch"
        );

        h.event(HotkeyEvent::Cancel);
        h.discarded().await;
        assert!(
            h.wait_for_latched(false).await,
            "a cancel left the hands-free badge lit"
        );

        assert_eq!(
            h.quit().await,
            Vec::<Op>::new(),
            "a hands-free badge scenario typed something"
        );
    }

    // ── 19. a closed event source stops the loop ───────────────────────────

    /// The event source can die without a Quit: a hotkey thread that gave up, a
    /// tray that closed. The loop's only shutdown is that channel closing, so
    /// that is what this drives — and a conversion that answers *afterwards* is
    /// the sharp half of the claim, because the user is already gone from the
    /// window the text would have been typed into.
    ///
    /// The producer that feeds that channel is covered in `machine.rs` with a
    /// bare channel and no microphone; what is tested here is the other end of
    /// the same fact.
    ///
    /// What this does **not** pin: the loop's `biased;`. Measured — with the
    /// keyword deleted the scenario stayed green for 40 runs, because the loop
    /// parks at `select!` long before a worker thread is near enough to send, so
    /// the closure and the answer are essentially never ready at the same
    /// instant. Same family as C33, registered as a gap rather than claimed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_closed_event_source_stops_the_loop_and_types_nothing() {
        let clock = Arc::new(ManualClock::new());
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(1)], vec![]);
        let h_engine = engine(vec![Step::Gated {
            samples: n(1),
            gate: gate.clone(),
            text: "دیررس",
        }]);
        let mut h = Harness::start_with(h_port, discarded, &h_engine, clock.clone());

        h.event(HotkeyEvent::RecordDown);
        h.tick();
        gate.wait_arrived(); // the conversion is in flight, and stuck there
        h.close_source(); // the source is gone
        gate.open(); // the engine answers, after the stop

        within(
            PATIENCE,
            "the loop to stop when its event source closed",
            h.loop_task,
        )
        .await
        .expect("the loop task itself")
        .expect("the loop returns Ok");

        // Reading the sink after the loop is gone is the only way to say
        // "nothing was typed from here on": before that, absence is just not
        // having happened yet.
        assert_eq!(
            h.seen.borrow().clone(),
            Vec::<Op>::new(),
            "a result that arrived after the stop was typed"
        );
    }

    // ── 20. two identical errors keep their own windows ────────────────────

    /// The same failure twice, with both windows open at once.
    ///
    /// Message comparison alone cannot tell these two apart: they are the same
    /// string, so the badge reads the same before and after the second one, and
    /// no observer watching the state can say which failure it is looking at.
    /// [`ErrorWindow::version`] is what says "this window was armed for the
    /// error on the badge *now*", and this is the test that fails if the number
    /// stops being read — the older window would find its own message sitting
    /// on the badge and clear the newer error.
    ///
    /// The barrier for "the second error landed" is the badge, but only because
    /// the badge goes somewhere and comes back: a new dictation is started in
    /// between, so the second failure is a `Recording → Error` **change**, which
    /// notifies. Waiting for `Error` alone would have been satisfied by the first
    /// failure forever — which is the whole reason this scenario exists.
    ///
    /// The barrier for "the first window closed and a second one is still open"
    /// is the clock: the loop asks for the earliest deadline it has, so the only
    /// deadline it can name that is later than the first is the second window's,
    /// and a `None` — the shape a race would leave behind — cannot satisfy it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_errors_with_the_same_message_keep_their_own_windows() {
        let clock = Arc::new(ManualClock::new());
        let (h_port, discarded) = port(vec![chunk(2)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Fail {
                samples: n(1),
                why: "scripted failure",
            },
            Step::Fail {
                samples: n(2),
                why: "scripted failure",
            },
        ]);
        let mut h = Harness::start_with(h_port, discarded, &h_engine, clock.clone());

        let t0 = clock.now();
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("scripted")))
                .await,
            "the first failure never reached the badge"
        );
        let first = t0 + ERROR_READABLE;
        clock
            .wait_asked(|asked| asked.last() == Some(&Some(first)))
            .await;

        // Half-way through the first window, a new dictation starts and its own
        // chunk fails with **the same message**. The badge leaves `Error` for
        // `Recording` and comes back to the very same error, and that change is
        // the proof that the second window is armed while the first is still
        // open.
        clock.advance(ERROR_READABLE / 2);
        h.event(HotkeyEvent::RecordDown);
        assert!(
            h.wait_for_state(|s| *s == AppState::Recording).await,
            "the new dictation never started"
        );
        h.tick();
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(_))).await,
            "the second, identical failure never reached the badge"
        );
        assert_eq!(
            h_engine.calls(),
            2,
            "the second failure was never asked of the engine"
        );

        // Now to just after the first window closes. It is the older of the two,
        // and it must not take the newer error with it: the badge is the
        // evidence, and `Idle` is what a wrongly cleared badge looks like — so
        // the identical message is no obstacle, because the two outcomes are
        // still different states.
        clock.advance(ERROR_READABLE / 2 + std::time::Duration::from_millis(1));
        clock
            .wait_asked(|asked| {
                asked
                    .last()
                    .copied()
                    .flatten()
                    .is_some_and(|deadline| deadline > first)
            })
            .await;
        assert!(
            matches!(h.state(), AppState::Error(_)),
            "the older window cleared the newer error: {:?}",
            h.state()
        );

        // The second window closes in its own turn, and that one may clear.
        clock.advance(ERROR_READABLE + std::time::Duration::from_secs(1));
        clock.wait_asked(|asked| asked.last() == Some(&None)).await;
        assert_eq!(h.state(), AppState::Idle, "the active error did not clear");
        h.quit().await;
    }

    // ── 21. the destination is asked, and obeyed ──────────────────────────

    /// The ordinary case, which is the one that must not change: a dictation
    /// whose window is still in front is typed — and typed **once**.
    ///
    /// The single `Type` is the assertion, not the string: a destination check
    /// that ran twice, or a send retried after being told it was only partly
    /// accepted, would both show up here as a second `Type` of the same text.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_valid_destination_gets_the_text_exactly_once() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "سلام دنیا",
        }]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(h.ops_until(1).await, typed("سلام دنیا"));
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Idle)).await,
            "the dictation never settled: {:?}",
            h.state()
        );
        assert_eq!(
            desktop.wait_captured(1).await.len(),
            1,
            "the destination was not read when recording started"
        );
        assert_eq!(
            h.quit().await,
            typed("سلام دنیا"),
            "the text was sent more than once"
        );
    }

    /// The refusal, in both of its forms: the user went somewhere else, and the
    /// window is simply not there any more.
    ///
    /// Zero `Op` of any kind is the claim — not "the text is missing from the
    /// window", but **nothing at all was sent**: no characters, and no
    /// backspace either, because a backspace into a window the user did not
    /// dictate into deletes *their* text.
    ///
    /// The badge is the barrier that makes "nothing" observable: only the
    /// refusal branch writes it, so once it is on screen the loop has already
    /// been asked and has already declined.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_moved_or_unknown_destination_sends_nothing_at_all() {
        let first_gate = Gate::new();
        let second_gate = Gate::new();
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: first_gate.clone(),
                text: "متن جلسهٔ اول",
            },
            Step::Gated {
                samples: n(2),
                gate: second_gate.clone(),
                text: "متن جلسهٔ دوم",
            },
        ]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        // First dictation: the user clicked into another window while the
        // engine was still working. The gate is what makes that ordering a fact
        // instead of a race.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        desktop.wait_captured(1).await;
        first_gate.wait_arrived();
        desktop.moved_to(0x9999);
        first_gate.open();
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("focus moved")))
                .await,
            "a destination that moved away was not refused: {:?}",
            h.state()
        );
        // Kept, and not invented: the text is *not* in `last_text`, which means
        // "what was typed" and feeds the orb's history.
        assert!(
            h.status.borrow().last_text.is_none(),
            "a refused destination still published text as typed"
        );

        // Second dictation: the window that was captured is gone.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        second_gate.wait_arrived();
        desktop.closed();
        second_gate.open();
        assert!(
            h.wait_for_state(
                |s| matches!(s, AppState::Error(m) if m.contains("destination unknown"))
            )
            .await,
            "a destination we cannot even ask about was not refused: {:?}",
            h.state()
        );

        assert_eq!(
            h.quit().await,
            Vec::new(),
            "a refused destination still sent keystrokes"
        );
    }

    /// Two dictations, two windows, and the first one's answer lands while the
    /// second is still recording.
    ///
    /// This is the scenario a single shared tracker cannot pass. A tracker
    /// replaced by the second dictation holds the *new* window, so the older
    /// text is refused for a window it was never dictated into — or, the other
    /// way round, the newer dictation's text is judged against somebody else's
    /// capture. Here the user is back in the **first** window when both answers
    /// are applied, so the first text goes in and the second does not.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_sessions_never_share_a_destination() {
        let gate = Gate::new();
        let (h_port, discarded) = port(vec![chunk(2)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Gated {
                samples: n(1),
                gate: gate.clone(),
                text: "متن جلسهٔ اول",
            },
            Step::Text {
                samples: n(2),
                text: "متن جلسهٔ دوم",
            },
        ]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        // The first dictation captures the window that is in front.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        let first_hwnd = desktop.wait_captured(1).await[0].hwnd;

        // The user clicks away and dictates again: the second dictation is
        // given a window of its own.
        desktop.moved_to(0x2000);
        h.event(HotkeyEvent::RecordDown);
        let second = desktop.wait_captured(2).await;
        assert_eq!(
            second[1].hwnd, 0x2000,
            "the second dictation was not given the window the user was in"
        );
        assert_ne!(
            first_hwnd, second[1].hwnd,
            "the two dictations share a window"
        );

        // And back again, before the first answer is even released.
        desktop.moved_to(first_hwnd);
        h.tick(); // the second dictation's chunk, queued behind the first answer
        gate.open();

        assert_eq!(
            h.ops_until(1).await,
            typed("متن جلسهٔ اول"),
            "the older dictation's text was judged against somebody else's window"
        );
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(_))).await,
            "the newer dictation's text was typed into the window the user had left: {:?}",
            h.state()
        );
        assert_eq!(
            h.quit().await,
            typed("متن جلسهٔ اول"),
            "a dictation was typed into a window the user had left"
        );
    }

    // ── 22. what the platform did with the send ───────────────────────────

    /// Nothing was accepted: an elevated window in front makes `SendInput`
    /// return zero. The verdict is `Failed`, the badge says so, and the send is
    /// **not** tried again.
    ///
    /// Honest about what this pins: there is no retry anywhere in the loop, so
    /// this test cannot prove one was removed. It pins that one attempt is what
    /// happens, and it pins the badge — a silent failure here would look exactly
    /// like a dictation the app swallowed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_send_the_platform_refuses_entirely_is_failed() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "سلام دنیا",
        }]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(
                desktop.clone(),
                Platform {
                    types: vec![Accept::None],
                    ..Platform::default()
                },
            ),
        );

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            typed("سلام دنیا"),
            "the send was not even attempted"
        );
        assert!(
            h.wait_for_state(
                |s| matches!(s, AppState::Error(m) if m.contains("nothing was typed"))
            )
            .await,
            "a send the platform refused was reported as a success: {:?}",
            h.state()
        );
        assert_eq!(
            h.quit().await,
            typed("سلام دنیا"),
            "a refused send was attempted again"
        );
    }

    /// Part of it went out and then delivery stopped.
    ///
    /// Two claims, and they are different ones. The verdict is `Partial` — never
    /// `Complete`, because `SendInput` did not accept every pair we asked for —
    /// and the send is **not repeated**: the accepted characters are already in
    /// the document, so asking again would type the whole text a second time.
    ///
    /// The scripted shortfall is three events out of the twenty-two a ten-letter
    /// Persian phrase needs, which is an odd number: a key-down that went out
    /// without its key-up. The verdict must not count that half.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_torn_send_is_partial_and_is_never_sent_again() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "سلام دنیا",
        }]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(
                desktop.clone(),
                Platform {
                    types: vec![Accept::Then(3)],
                    ..Platform::default()
                },
            ),
        );

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(h.ops_until(1).await, typed("سلام دنیا"));
        assert!(
            h.wait_for_state(
                |s| matches!(s, AppState::Error(m) if m.contains("injection incomplete"))
            )
            .await,
            "a partly accepted send was not reported as incomplete: {:?}",
            h.state()
        );
        assert_eq!(
            h.quit().await,
            typed("سلام دنیا"),
            "a partly accepted send was sent again"
        );
    }

    /// The erase at the seam did not go out whole, so the rest of the text is
    /// **not** sent.
    ///
    /// This is the case that changed behaviour: a failed backspace used to be a
    /// warning and the text went in anyway. It cannot any more — the fragment it
    /// meant to delete is either still in the document or half-deleted, and
    /// typing the complete word on top of it leaves the user with both words.
    ///
    /// The seam's own rule builds the situation: a chunk that ends inside a word
    /// («… دنیا») and the next one that spells it out («دنیامان بزرگ») make the
    /// stitcher ask for five backspaces before typing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_torn_seam_erase_stops_the_text() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(2)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "او رفت دنیا",
            },
            Step::Text {
                samples: n(2),
                text: "دنیامان بزرگ",
            },
        ]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(
                desktop.clone(),
                Platform {
                    types: vec![Accept::All],
                    backspaces: vec![Accept::Then(3)],
                },
            ),
        );

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the first chunk, handed over mid-dictation
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(2).await,
            vec![Op::Type("او رفت دنیا".to_string()), Op::Backspace(4)],
            "the chunk and the seam erase were not sent in that order"
        );
        assert!(
            h.wait_for_state(
                |s| matches!(s, AppState::Error(m) if m.contains("injection incomplete"))
            )
            .await,
            "a torn erase was not reported: {:?}",
            h.state()
        );
        assert_eq!(
            h.quit().await,
            vec![Op::Type("او رفت دنیا".to_string()), Op::Backspace(4)],
            "the text was typed after the erase it needed had failed"
        );
    }

    /// An insert that did not land leaves **no** seam memory behind.
    ///
    /// The seam stitcher remembers the tail of what it believes it typed, and
    /// the next chunk is stitched against that tail: a repeated word is dropped
    /// as overlap, and a word the cut truncated is deleted with backspaces. Both
    /// are safe only while the tail is true.
    ///
    /// So this is the test for the worst thing this stage could do — eat the
    /// user's own words. The first chunk is refused by the platform, so nothing
    /// of it is in the document; the second chunk repeats its last word, which
    /// the stitcher would otherwise drop as overlap. With the memory dropped,
    /// the whole phrase is typed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failed_insert_leaves_no_seam_memory() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(2)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام دنیا",
            },
            Step::Text {
                samples: n(2),
                text: "دنیا ادامه",
            },
        ]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(
                desktop.clone(),
                Platform {
                    types: vec![Accept::None, Accept::All],
                    ..Platform::default()
                },
            ),
        );

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // the first chunk, refused by the platform
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(h.ops_until(1).await, typed("سلام دنیا"));
        assert_eq!(
            h.ops_until(2).await,
            [typed("سلام دنیا"), typed("دنیا ادامه")].concat(),
            "the next chunk was stitched against text that was never typed"
        );
        h.quit().await;
    }

    // ── 23. a destination that was never captured ────────────────────────

    /// No window in front when the recording started means **no destination**,
    /// not "any destination".
    ///
    /// This is the shape of mistake `C-T2-5` was written for, one layer up: a
    /// neutral value that a careless mapping would read as permission. The
    /// dictation is still real — the user spoke, the words were recognised — so
    /// the refusal is the badge's job, not an error about the microphone.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_dictation_with_no_captured_window_sends_nothing() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "سلام دنیا",
        }]);
        let desktop = ScriptedDesktop::new();
        desktop.went_dark(); // nothing in front: a locked desktop, another session
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert!(
            h.wait_for_state(
                |s| matches!(s, AppState::Error(m) if m.contains("destination unknown"))
            )
            .await,
            "a dictation with no destination was not refused: {:?}",
            h.state()
        );
        assert_eq!(
            h.quit().await,
            Vec::new(),
            "text was typed into whatever happened to be in front"
        );
        assert_eq!(
            desktop.captured(),
            0,
            "the desktop claims a window while there is none"
        );
    }

    // ── 24. action-needed records decoupled from session completion ──────

    /// When a destination is refused on the final chunk, the record must persist
    /// in memory after the session completes (settles).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn destination_refused_on_final_chunk_leaves_record_after_session_completed() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "متن پایانی",
        }]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        // User moves focus to another window before the final result is applied:
        desktop.moved_to(0x9999);
        h.event(HotkeyEvent::RecordUp);

        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("focus moved")))
                .await,
            "refusal on moved focus was not reported: {:?}",
            h.state()
        );

        let records = h.kept_records();
        h.quit().await;

        assert_eq!(
            records.len(),
            1,
            "the refused text record must survive session completion"
        );
        let rec = &records[0];
        assert_eq!(rec.planned_text, "متن پایانی");
        assert_eq!(rec.outcome, InjectOutcome::NotAttempted);
        assert!(rec.is_wholly_undelivered());
        assert_eq!(rec.unaccepted_text(), Some("متن پایانی"));
        assert!(rec.destination.is_some());
    }

    /// Multiple refused chunks must append separate records without overwriting each other.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multiple_refused_chunks_do_not_overwrite_any_record() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(2)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "قطعه اول",
            },
            Step::Text {
                samples: n(2),
                text: "قطعه دوم",
            },
        ]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        // Focus moves away so both chunk 1 and chunk 2 are refused:
        desktop.moved_to(0xdead);
        h.tick(); // chunk 1
        h.event(HotkeyEvent::RecordUp); // chunk 2

        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("focus moved")))
                .await
        );

        // Both records, not just the first one's error. The badge reads `Error`
        // as soon as chunk 1 is refused, while chunk 2's conversion is still
        // running — asserting on the list there would read one record and
        // report a lost record that was never lost.
        let records = h.kept_until(2).await;
        h.quit().await;

        assert_eq!(
            records.len(),
            2,
            "neither refused chunk record may be overwritten"
        );
        assert_eq!(records[0].planned_text, "قطعه اول");
        assert_eq!(records[0].outcome, InjectOutcome::NotAttempted);
        assert_eq!(records[0].chunk, Some(ChunkId(1)));

        assert_eq!(records[1].planned_text, "قطعه دوم");
        assert_eq!(records[1].outcome, InjectOutcome::NotAttempted);
        assert_eq!(records[1].chunk, Some(ChunkId(2)));
    }

    /// Failed and Partial injections are recorded with their exact reports without automatic retry.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn failed_and_partial_injections_are_recorded_accurately_without_automatic_retry() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(2)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "شکست کامل",
            },
            Step::Text {
                samples: n(2),
                text: "ارسال ناقص",
            },
        ]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(
                desktop.clone(),
                Platform {
                    // Chunk 1 completely fails (0 accepted events).
                    // Chunk 2 partially succeeds: accepts 2 events (1 pair) out of 20 events.
                    types: vec![Accept::None, Accept::Then(2)],
                    ..Platform::default()
                },
            ),
        );

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // Chunk 1
        h.event(HotkeyEvent::RecordUp); // Chunk 2

        assert!(
            h.wait_for_state(
                |s| matches!(s, AppState::Error(m) if m.contains("injection incomplete"))
            )
            .await
        );

        let records = h.kept_records();
        let ops = h.quit().await;
        // Verify exactly one send per chunk was attempted (no automatic replay loops):
        assert_eq!(
            ops,
            vec![
                Op::Type("شکست کامل".to_string()),
                Op::Type("ارسال ناقص".to_string())
            ]
        );

        assert_eq!(records.len(), 2);

        // Record 0: Failed
        assert_eq!(records[0].outcome, InjectOutcome::Failed);
        assert!(records[0].is_wholly_undelivered());
        assert_eq!(records[0].planned_text, "شکست کامل");

        // Record 1: Partial
        assert_eq!(
            records[1].outcome,
            InjectOutcome::Partial { accepted_pairs: 1 }
        );
        assert!(
            !records[1].is_wholly_undelivered(),
            "partial delivery must not be called wholly undelivered"
        );
        assert_eq!(records[1].planned_text, "ارسال ناقص");
        let remaining = records[1].unaccepted_text();
        assert!(remaining.is_some());
        assert_ne!(
            remaining,
            Some("ارسال ناقص"),
            "unaccepted text must not be the full text in partial send"
        );
    }

    /// Explicit cancellation drops only the records of the cancelled session.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancellation_only_drops_records_of_the_cancelled_session() {
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "جلسه اول رد شده",
            },
            Step::Text {
                samples: n(2),
                text: "جلسه دوم لغو شده",
            },
        ]);
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        // Session 1: Destination refused -> preserved in kept records
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        desktop.moved_to(0x1111);
        h.event(HotkeyEvent::RecordUp);
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("focus moved")))
                .await
        );

        assert_eq!(h.kept_records().len(), 1);
        let session_1_id = h.kept_records()[0].session;

        // Session 2: Explicitly cancelled
        desktop.moved_to(0x2222); // Reset to valid window for session 2
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(2).await;
        h.event(HotkeyEvent::Cancel);

        // Session 1's record must still be intact:
        let records = h.kept_records();
        assert_eq!(
            records.len(),
            1,
            "session 1's record must remain after session 2 is cancelled"
        );
        assert_eq!(records[0].session, session_1_id);
        assert_eq!(records[0].planned_text, "جلسه اول رد شده");

        h.quit().await;
    }

    /// Direct semantic assertions on KeptRecord unaccepted_text & is_wholly_undelivered.
    #[test]
    fn kept_record_direct_unaccepted_text_semantics() {
        // 1. پذیرفته‌شدن فقط key-down نخست ⇒ None
        let rec_first_down = KeptRecord {
            session: None,
            chunk: None,
            seq: 1,
            planned_text: "hello".to_string(),
            destination: None,
            outcome: InjectOutcome::Partial { accepted_pairs: 0 },
            backspace: None,
            text: Some(Injection {
                total_events: 10,
                attempted: 10,
                accepted: 1, // Only first key-down accepted (odd)
                stopped: Some("stopped after 1".to_string()),
            }),
        };
        assert_eq!(
            rec_first_down.unaccepted_text(),
            None,
            "first key-down accepted without key-up is indeterminate"
        );
        assert!(!rec_first_down.is_wholly_undelivered());

        // 2. تعداد فرد پس از چند جفت کامل ⇒ None
        let rec_odd_after_pairs = KeptRecord {
            session: None,
            chunk: None,
            seq: 2,
            planned_text: "hello".to_string(),
            destination: None,
            outcome: InjectOutcome::Partial { accepted_pairs: 2 },
            backspace: None,
            text: Some(Injection {
                total_events: 10,
                attempted: 10,
                accepted: 5, // 2 complete pairs (4 events) + 1 key-down (1 event) = 5 (odd)
                stopped: Some("stopped after 5".to_string()),
            }),
        };
        assert_eq!(
            rec_odd_after_pairs.unaccepted_text(),
            None,
            "odd acceptance after complete pairs is indeterminate"
        );
        assert!(!rec_odd_after_pairs.is_wholly_undelivered());

        // 3. یک واحد از emoji پذیرفته شده ⇒ None
        // "🦀" requires 1 surrogate pair = 2 UTF-16 code units = 4 events.
        let rec_half_emoji = KeptRecord {
            session: None,
            chunk: None,
            seq: 3,
            planned_text: "🦀hello".to_string(),
            destination: None,
            outcome: InjectOutcome::Partial { accepted_pairs: 1 },
            backspace: None,
            text: Some(Injection {
                total_events: 14,
                attempted: 14,
                accepted: 2, // 2 events = 1 UTF-16 code unit = high surrogate of 🦀 only
                stopped: Some("stopped after 2".to_string()),
            }),
        };
        assert_eq!(
            rec_half_emoji.unaccepted_text(),
            None,
            "accepting only one surrogate unit of an emoji leaves torn character"
        );
        assert!(!rec_half_emoji.is_wholly_undelivered());

        // 4. emoji کامل پذیرفته شده ⇒ پسوند درست
        // 🦀 accepted completely (2 UTF-16 code units = 4 events).
        let rec_full_emoji = KeptRecord {
            session: None,
            chunk: None,
            seq: 4,
            planned_text: "🦀hello".to_string(),
            destination: None,
            outcome: InjectOutcome::Partial { accepted_pairs: 2 },
            backspace: None,
            text: Some(Injection {
                total_events: 14,
                attempted: 14,
                accepted: 4, // 4 events = 2 UTF-16 units = entire 🦀
                stopped: Some("stopped after 4".to_string()),
            }),
        };
        assert_eq!(
            rec_full_emoji.unaccepted_text(),
            Some("hello"),
            "full emoji accepted yields clean unaccepted suffix"
        );
        assert!(!rec_full_emoji.is_wholly_undelivered());

        // 5. شکست Backspace بدون ارسال متن ⇒ کل متن از نظر ارسال متن پذیرفته‌نشده،
        // همراه با گزارش جداگانهٔ اثر نامطمئن Backspace.
        let rec_backspace_failed = KeptRecord {
            session: None,
            chunk: None,
            seq: 5,
            planned_text: "سلام دنیا".to_string(),
            destination: None,
            outcome: InjectOutcome::Partial { accepted_pairs: 1 },
            backspace: Some(Injection {
                total_events: 4,
                attempted: 4,
                accepted: 2, // 1 backspace accepted out of 2 requested
                stopped: Some("stopped after 1 bs".to_string()),
            }),
            text: None, // No text was ever sent
        };
        assert!(
            rec_backspace_failed.is_wholly_undelivered(),
            "from the standpoint of text delivery, zero text was sent/accepted"
        );
        assert_eq!(
            rec_backspace_failed.unaccepted_text(),
            Some("سلام دنیا"),
            "entire planned text is unaccepted with respect to text delivery"
        );
        // Separate report of backspace's uncertain / partial effect:
        let bs = rec_backspace_failed
            .backspace
            .as_ref()
            .expect("backspace report must be present");
        assert_eq!(bs.accepted, 2);
        assert_eq!(bs.total_events, 4);
        assert!(!bs.whole_pairs());

        // Also verify complete failure of backspace (0 accepted):
        let rec_backspace_zero = KeptRecord {
            session: None,
            chunk: None,
            seq: 6,
            planned_text: "متن کامل".to_string(),
            destination: None,
            outcome: InjectOutcome::Failed,
            backspace: Some(Injection {
                total_events: 2,
                attempted: 2,
                accepted: 0,
                stopped: Some("zero accepted".to_string()),
            }),
            text: None,
        };
        assert!(rec_backspace_zero.is_wholly_undelivered());
        assert_eq!(rec_backspace_zero.unaccepted_text(), Some("متن کامل"));
        assert_eq!(rec_backspace_zero.backspace.unwrap().accepted, 0);
    }

    // ── Boundary policy production path tests ─────────────────────────────

    /// ۱. دو قطعهٔ فارسی ⇒ یک فاصله در مرز
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_two_consecutive_persian_chunks_get_single_space() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام",
            },
            Step::Text {
                samples: n(1),
                text: "دنیا",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // mid-session chunk 1
        h.event(HotkeyEvent::RecordUp); // final chunk

        assert_eq!(
            h.ops_until(2).await,
            vec![Op::Type("سلام".to_string()), Op::Type(" دنیا".to_string()),],
            "two consecutive Persian chunks must receive exactly one space at boundary"
        );
        h.quit().await;
    }

    /// ۲. فاصلهٔ موجود و شکست خط ⇒ بدون جداکنندهٔ اضافی
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_existing_whitespace_or_newline_gets_no_extra_space() {
        // Case A: First chunk ends with space
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام ",
            },
            Step::Text {
                samples: n(1),
                text: "دوست",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick();
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(2).await,
            vec![Op::Type("سلام ".to_string()), Op::Type("دوست".to_string()),],
            "chunk 1 ending with space must not add another separator"
        );
        h.quit().await;

        // Case B: First chunk ends with newline
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "خط اول\n",
            },
            Step::Text {
                samples: n(1),
                text: "خط دوم",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick();
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type("خط اول\n".to_string()),
                Op::Type("خط دوم".to_string()),
            ],
            "chunk 1 ending with newline must not add space before chunk 2"
        );
        h.quit().await;

        // Case C: Second chunk starts with whitespace
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام",
            },
            Step::Text {
                samples: n(1),
                text: " دوست",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick();
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(2).await,
            vec![Op::Type("سلام".to_string()), Op::Type(" دوست".to_string()),],
            "chunk 2 starting with space must not get double spaces"
        );
        h.quit().await;
    }

    /// ۳. نشانه‌گذاری ⇒ بدون فاصلهٔ نادرست
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_attached_punctuation_gets_no_space() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام",
            },
            Step::Text {
                samples: n(1),
                text: ". چطوری؟",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick();
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type("سلام".to_string()),
                Op::Type(". چطوری؟".to_string()),
            ],
            "attached punctuation must not receive leading space"
        );
        h.quit().await;
    }

    /// ۴. ترمیم کلمهٔ ناقص ⇒ کلمهٔ پیوسته (عدم فاصله‌گذاری هنگام backspaces > 0)
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_word_repair_with_backspace_remains_continuous() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "می‌خوا",
            },
            Step::Text {
                samples: n(1),
                text: "می‌خواهم",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick();
        h.event(HotkeyEvent::RecordUp);

        let ops = h.ops_until(3).await;
        assert_eq!(
            ops,
            vec![
                Op::Type("می‌خوا".to_string()),
                Op::Backspace(6),
                Op::Type("می‌خواهم".to_string()),
            ],
            "word repair with backspaces must not prepend space"
        );
        h.quit().await;
    }

    /// ۵. قطعهٔ خالی ⇒ صفر ارسال
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_empty_chunk_advances_no_memory_and_sends_zero_ops() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "",
            },
            Step::Text {
                samples: n(1),
                text: "سلام",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // empty chunk -> skipped, zero ops
        h.event(HotkeyEvent::RecordUp); // final chunk "سلام"

        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type("سلام".to_string())],
            "empty chunk must send zero ops and not leave boundary memory that prefixes following chunk"
        );
        h.quit().await;
    }

    /// ۶. شکست یا رد مقصد ⇒ حافظهٔ معتبر باقی نماند
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_refused_or_failed_destination_invalidates_memory() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![chunk(1), chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "اول",
            },
            Step::Text {
                samples: n(1),
                text: "دوم",
            },
            Step::Text {
                samples: n(1),
                text: "سوم",
            },
        ]);
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.tick(); // chunk 1 ("اول") typed successfully
        assert_eq!(h.ops_until(1).await, vec![Op::Type("اول".to_string())]);

        // Destination refused on chunk 2:
        desktop.moved_to(0x9999);
        h.tick(); // chunk 2 ("دوم") is refused; invalidates boundary memory
        assert!(
            h.wait_for_state(|s| matches!(s, AppState::Error(m) if m.contains("focus moved")))
                .await
        );

        // Move back to captured window and send final chunk:
        desktop.moved_to(0x1000);
        h.event(HotkeyEvent::RecordUp); // chunk 3 ("سوم")

        let ops = h.ops_until(2).await;
        assert_eq!(
            ops,
            vec![
                Op::Type("اول".to_string()),
                Op::Type("سوم".to_string()),
            ],
            "refused destination must invalidate boundary memory so subsequent chunk gets no boundary space"
        );
        h.quit().await;
    }

    /// ۷. Raw با فاصله‌های متعدد و شکست خط ⇒ محتوای اصلی حفظ شود
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_raw_mode_preserves_internal_spaces_and_newlines() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let raw_chunk1 = "متن اول   با فاصله زیاد\nخط جدید\tتب";
        let raw_chunk2 = "متن دوم  فاصله دوتا";
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: raw_chunk1,
            },
            Step::Text {
                samples: n(1),
                text: raw_chunk2,
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick();
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type(raw_chunk1.to_string()),
                Op::Type(format!(" {raw_chunk2}")),
            ],
            "raw mode must preserve all multiple internal spaces, newlines, and tabs without collapsing"
        );
        h.quit().await;
    }

    /// ۸. ادامهٔ جلسهٔ جدید و تغییر مقصد ⇒ رفتار مطابق اعتبار حافظه
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_continued_session_and_destination_change_behavior() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2), ends(3)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "جلسه اول",
            },
            Step::Text {
                samples: n(2),
                text: "ادامه جلسه دوم",
            },
            Step::Text {
                samples: n(3),
                text: "جلسه سوم پنجره جدید",
            },
        ]);
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        // Session 1: in window 0x1000
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        assert_eq!(h.ops_until(1).await, vec![Op::Type("جلسه اول".to_string())]);

        // Session 2: in SAME window 0x1000 -> valid boundary memory -> gets single leading space
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(2).await;
        h.event(HotkeyEvent::RecordUp);
        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type("جلسه اول".to_string()),
                Op::Type(" ادامه جلسه دوم".to_string()),
            ],
            "continued session in same target window must get boundary space"
        );

        // Session 3: in window 0x5555 (different window) -> memory for 0x1000 is invalid -> no leading space
        desktop.moved_to(0x5555);
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(3).await;
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(3).await,
            vec![
                Op::Type("جلسه اول".to_string()),
                Op::Type(" ادامه جلسه دوم".to_string()),
                Op::Type("جلسه سوم پنجره جدید".to_string()),
            ],
            "new session in changed window must not inherit boundary space"
        );
        h.quit().await;
    }

    // ── Additional production path boundary tests (Clause 4) ─────────────

    /// ۱. «میخوا» + دو فاصله، سپس «میخواهم» ⇒ صفر Backspace
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_truncated_word_with_trailing_spaces_does_not_backspace() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "می‌خوا  ",
            },
            Step::Text {
                samples: n(1),
                text: "می‌خواهم",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // chunk 1 ("می‌خوا  ")
        h.event(HotkeyEvent::RecordUp); // chunk 2 ("می‌خواهم")

        // Because chunk 1 had trailing whitespace after "می‌خوا", it was not cut mid-word.
        // Seam repair MUST NOT emit speculative backspaces.
        let ops = h.ops_until(2).await;
        assert_eq!(
            ops,
            vec![
                Op::Type("می‌خوا  ".to_string()),
                Op::Type("می‌خواهم".to_string()),
            ],
            "chunk with trailing spaces must not trigger Backspace word repair"
        );
        h.quit().await;
    }

    /// ۲. همپوشانی همراه newline ⇒ newline باقی بماند
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_overlap_with_newline_preserves_newline() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام دنیا",
            },
            Step::Text {
                samples: n(1),
                text: "دنیا\nخط دوم",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // chunk 1 ("سلام دنیا")
        h.event(HotkeyEvent::RecordUp); // chunk 2 ("دنیا\nخط دوم")

        // "دنیا" is dropped as overlap; the newline following it MUST be preserved.
        let ops = h.ops_until(2).await;
        assert_eq!(
            ops,
            vec![
                Op::Type("سلام دنیا".to_string()),
                Op::Type("\nخط دوم".to_string()),
            ],
            "newline separator after dropped overlap word must be preserved"
        );
        h.quit().await;
    }

    /// ۳. همپوشانی همراه tab و چند فاصله ⇒ جداکننده حفظ شود
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_overlap_with_tab_and_spaces_preserves_separator() {
        let (h_port, discarded) = port(vec![chunk(1)], vec![ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "سلام دنیا",
            },
            Step::Text {
                samples: n(1),
                text: "دنیا\t   بخش جدید",
            },
        ]);
        let mut h = Harness::start(h_port, discarded, &h_engine);

        h.event(HotkeyEvent::RecordDown);
        h.tick(); // chunk 1 ("سلام دنیا")
        h.event(HotkeyEvent::RecordUp); // chunk 2 ("دنیا\t   بخش جدید")

        // "دنیا" is dropped as overlap; tab and spaces following it MUST be preserved.
        let ops = h.ops_until(2).await;
        assert_eq!(
            ops,
            vec![
                Op::Type("سلام دنیا".to_string()),
                Op::Type("\t   بخش جدید".to_string()),
            ],
            "tab and multiple spaces after dropped overlap word must be preserved"
        );
        h.quit().await;
    }

    /// ۴. عنوان جدید با hwnd/pid یکسان ⇒ مرز معتبر بماند
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_title_change_with_same_hwnd_pid_remains_valid() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "جلسه اول",
            },
            Step::Text {
                samples: n(2),
                text: "جلسه دوم",
            },
        ]);
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        // Session 1: in window 0x1000 with initial title
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        assert_eq!(h.ops_until(1).await, vec![Op::Type("جلسه اول".to_string())]);

        // Session 2: same hwnd (0x1000) and pid (0x1000), but title changed dynamically
        desktop.set_title("document title [modified] - App");
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(2).await;
        h.event(HotkeyEvent::RecordUp);

        // Continuation heuristic based on same window (hwnd + pid) considers boundary valid:
        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type("جلسه اول".to_string()),
                Op::Type(" جلسه دوم".to_string()),
            ],
            "same hwnd and pid must maintain valid boundary spacing even if window title changes"
        );
        h.quit().await;
    }

    /// ۵. مقصد متفاوت ⇒ حافظهٔ قبلی استفاده نشود
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn boundary_different_destination_does_not_use_memory() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(2)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: "پنجره اول",
            },
            Step::Text {
                samples: n(2),
                text: "پنجره دوم",
            },
        ]);
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        // Session 1 in window 0x1000
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type("پنجره اول".to_string())]
        );

        // Destination moves to different window 0x7777 for Session 2
        desktop.moved_to(0x7777);
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(2).await;
        h.event(HotkeyEvent::RecordUp);

        // Session 2 in different window must NOT use boundary memory from window 0x1000
        assert_eq!(
            h.ops_until(2).await,
            vec![
                Op::Type("پنجره اول".to_string()),
                Op::Type("پنجره دوم".to_string()),
            ],
            "different destination window must not inherit boundary memory or prepend space"
        );
        h.quit().await;
    }

    // ── آزمون‌های مسیر تولیدی پردازش متن فارسی (Requirement 5) ─────────────

    /// ۱. حفظ همزه در محافظه‌کارانه (مسأله و تأیید و مؤمن)
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn production_pipeline_preserves_hamza_in_conservative_mode() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "مسأله و تأیید و مؤمن",
        }]);
        let mut h = Harness::start_mode(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            "conservative",
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type("مسأله و تأیید و مؤمن".to_string())],
            "conservative mode must preserve hamza through coordinator pipeline"
        );
        h.quit().await;
    }

    /// ۲. حفظ کلمات سالم در حالت پیش‌فرض (کلمات، تمام، اتمام، میکروفون، کبوتر، اژدها)
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn production_pipeline_preserves_healthy_words_in_default_standard_mode() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "کلمات تمام اتمام میکروفون کبوتر اژدها",
        }]);
        let mut h = Harness::start_mode(h_port, discarded, &h_engine, desktop.clone(), "standard");

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type(
                "کلمات تمام اتمام میکروفون کبوتر اژدها".to_string()
            )],
            "standard mode must preserve healthy Persian words without corrupting them"
        );
        h.quit().await;
    }

    /// ۳. اصلاح نمونه‌های معتبر بدون شکستن کلمات مشابه
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn production_pipeline_corrects_valid_samples_without_breaking_similar_words() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "کتابها میروم اژدها کبوتر",
        }]);
        let mut h = Harness::start_mode(h_port, discarded, &h_engine, desktop.clone(), "standard");

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type("کتاب‌ها می‌روم اژدها کبوتر".to_string())],
            "valid samples must be corrected while similar words are preserved"
        );
        h.quit().await;
    }

    /// ۴. حفظ فاصلهٔ صریح «می روم»
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn production_pipeline_preserves_explicit_space_in_mi_rom() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "می روم به خانه",
        }]);
        let mut h = Harness::start_mode(h_port, discarded, &h_engine, desktop.clone(), "standard");

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type("می روم به خانه".to_string())],
            "explicit space between می and روم must not be turned into ZWNJ"
        );
        h.quit().await;
    }

    /// ۵. حفظ ارقام لاتین
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn production_pipeline_preserves_latin_digits() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "شماره 123 و پورت 8080",
        }]);
        let mut h = Harness::start_mode(h_port, discarded, &h_engine, desktop.clone(), "standard");

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type("شماره 123 و پورت 8080".to_string())],
            "Latin digits must remain Latin digits without automatic conversion"
        );
        h.quit().await;
    }

    /// ۶. Raw بدون تغییر متن توسط پردازش فارسی
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn production_pipeline_raw_mode_bypasses_persian_processing() {
        let desktop = ScriptedDesktop::new();
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        // Raw text with Arabic kaf (كتابهاي) and technical mishearing (پاتون):
        let h_engine = engine(vec![Step::Text {
            samples: n(1),
            text: "كتابهاي من با پاتون",
        }]);
        let mut h = Harness::start_mode(h_port, discarded, &h_engine, desktop.clone(), "raw");

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.ops_until(1).await,
            vec![Op::Type("كتابهاي من با پاتون".to_string())],
            "Raw mode must bypass Persian normalization and dictionary"
        );
        h.quit().await;
    }
    // ── review and recovery, driven through the real loop ────────────────
    //
    // `review.rs` is tested on its own; these scenarios cover the parts only the
    // loop can answer: does a held text really leave the keyboard alone, does an
    // approved one really type, and does a refused insert really come back as
    // something the user can act on.

    /// A reviewing harness over a rig the scenario chose.
    fn reviewing(
        port: ScriptedPort,
        discarded: Arc<Notify>,
        engine: &Arc<ScriptedEngine>,
        rig: Rig,
    ) -> Harness {
        Harness::start_scripted(port, discarded, engine, Arc::new(SystemClock), false, rig, true)
    }

    /// One dictation, spoken and released, against `desktop`.
    async fn dictate_once(h: &Harness, desktop: &Arc<ScriptedDesktop>) -> review::PendingDraft {
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        h.wait_draft().await
    }

    /// An engine that answers every utterance with the same words.
    fn one_utterance(text: &'static str) -> Arc<ScriptedEngine> {
        engine(vec![Step::Text { samples: n(1), text }])
    }

    /// The headline rule: a finished dictation types **nothing** until the user
    /// says so.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn review_mode_types_nothing_before_the_user_answers() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("سلام دنیا");
        let desktop = ScriptedDesktop::new();
        let h = reviewing(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        let draft = dictate_once(&h, &desktop).await;

        assert_eq!(draft.kind, DraftKind::Review);
        assert_eq!(draft.text, "سلام دنیا");
        assert!(
            h.seen.borrow().is_empty(),
            "nothing may reach the keyboard before the answer: {:?}",
            h.seen.borrow()
        );

        h.quit().await;
    }

    /// Approving types the text the user is looking at, edits included.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn approving_a_draft_types_the_edited_text() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("اشتباه");
        let desktop = ScriptedDesktop::new();
        let mut h = reviewing(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        let draft = dictate_once(&h, &desktop).await;
        h.answer(review::ReviewCommand::Insert {
            id: draft.id,
            text: "درست شد".into(),
        });

        within(PATIENCE, "the approved text to be typed", h.wait_ops(1)).await;
        assert_eq!(
            h.seen.borrow().clone(),
            typed("درست شد"),
            "the edit is what gets typed, not the original"
        );
        assert!(h.drafts().is_empty(), "an answered draft is done");

        h.quit().await;
    }

    /// Copy and cancel both leave the keyboard untouched.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn copying_a_draft_types_nothing() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("متن");
        let desktop = ScriptedDesktop::new();
        let h = reviewing(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        let draft = dictate_once(&h, &desktop).await;
        h.answer(review::ReviewCommand::Copy { id: draft.id });

        // Give the loop turns to do the wrong thing, then look.
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);
        assert!(
            h.seen.borrow().is_empty(),
            "copy must type nothing, saw {:?}",
            h.seen.borrow()
        );
        h.wait_resolved(draft.id).await;

        h.quit().await;
    }

    /// The rule that stops a double-click from typing a dictation twice.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_approval_types_once_and_a_replay_types_nothing_more() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("سلام");
        let desktop = ScriptedDesktop::new();
        let mut h = reviewing(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        let draft = dictate_once(&h, &desktop).await;
        let command = review::ReviewCommand::Insert {
            id: draft.id,
            text: "سلام".into(),
        };
        h.answer(command.clone());
        within(PATIENCE, "the first approval to type", h.wait_ops(1)).await;

        h.answer(command);
        h.event(HotkeyEvent::RecordDown);
        h.event(HotkeyEvent::RecordUp);

        assert_eq!(
            h.seen.borrow().clone(),
            typed("سلام"),
            "the replayed answer must type nothing: {:?}",
            h.seen.borrow()
        );

        h.quit().await;
    }

    /// Recovery, part one: a destination that refused leaves the text in the
    /// user's hands rather than in a log nobody reads.
    ///
    /// `start_rig` keeps review **off** on purpose: this text is offered because
    /// its insert failed, not because the user asked to see drafts.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn text_the_destination_refused_is_offered_back() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("مهم");
        let desktop = ScriptedDesktop::new();
        let h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        // The user looked away while the engine worked.
        desktop.moved_to(0x9999);
        h.event(HotkeyEvent::RecordUp);

        let draft = h.wait_draft().await;
        assert_eq!(draft.kind, DraftKind::Undelivered);
        assert_eq!(
            draft.text, "مهم",
            "recovery repeats the insert, never the conversion"
        );
        assert!(
            h.seen.borrow().is_empty(),
            "a refused destination must not have received a key"
        );

        h.quit().await;
    }

    /// Recovery, part two: with the window back in front, approving types the
    /// text once. This is the point of the feature — the user does not have to
    /// speak again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn recovering_undelivered_text_types_it_once_the_window_is_back() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("مهم");
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        desktop.moved_to(0x9999);
        h.event(HotkeyEvent::RecordUp);
        let draft = h.wait_draft().await;

        // The user came back to the original window, then approved.
        desktop.moved_to(0x1000);
        h.answer(review::ReviewCommand::Insert {
            id: draft.id,
            text: draft.text.clone(),
        });
        within(PATIENCE, "the recovered text to be typed", h.wait_ops(1)).await;

        assert_eq!(h.seen.borrow().clone(), typed("مهم"));
        h.wait_nothing_kept().await;

        h.quit().await;
    }

    /// Approving while the user is looking at a *different* window must not
    /// type there. Review mode has to be at least as careful as direct mode.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn approving_a_draft_whose_window_moved_types_nothing_and_offers_it_again() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("متن");
        let desktop = ScriptedDesktop::new();
        let h = reviewing(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        let draft = dictate_once(&h, &desktop).await;
        // The user clicked into another app while the window was open.
        desktop.moved_to(0x9999);
        h.answer(review::ReviewCommand::Insert {
            id: draft.id,
            text: draft.text.clone(),
        });

        // It comes back as an undelivered draft rather than disappearing. The
        // wait is on a **different** id: reading "the newest draft" straight
        // after answering would race the loop and find the old one, which is
        // still pending until the answer is handled.
        let again = within(PATIENCE, "the draft to be offered again", async {
            loop {
                if let Some(d) = h.drafts().into_iter().find(|d| d.id != draft.id) {
                    return d;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;

        assert_eq!(again.kind, DraftKind::Undelivered);
        assert_ne!(again.id, draft.id, "a new offer is a new draft");
        assert!(
            h.seen.borrow().is_empty(),
            "focus moved; nothing may be typed into the new window"
        );

        h.quit().await;
    }

    /// Cancelling a recording must take its held text with it. A window that
    /// still offers to insert a dictation the user threw away is offering the
    /// wrong thing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelling_a_dictation_drops_the_text_it_was_holding() {
        let (h_port, discarded) = port(vec![chunk(0)], vec![ends(1), ends(2)]);
        let h_engine = one_utterance("متن اول");
        let desktop = ScriptedDesktop::new();
        let h = reviewing(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        let first = dictate_once(&h, &desktop).await;

        // A second dictation, started and then thrown away. It produces no
        // text — the cancel discards its audio — so its absence from the drafts
        // is what the assertions are really about.
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(2).await;
        h.event(HotkeyEvent::Cancel);

        assert_eq!(
            h.drafts().len(),
            1,
            "only the first dictation's text should be on offer: {:?}",
            h.drafts()
        );
        assert_eq!(
            h.drafts()[0].id, first.id,
            "and it must be that first dictation's, untouched by the cancel"
        );

        h.quit().await;
    }

    /// Direct mode must be untouched by any of this: the default is still a
    /// dictation that types itself, and asks nobody.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn direct_mode_still_types_without_asking_anyone() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance("مستقیم");
        let desktop = ScriptedDesktop::new();
        let mut h = Harness::start_rig(
            h_port,
            discarded,
            &h_engine,
            Rig::with(desktop.clone(), Platform::default()),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        assert_eq!(h.seen.borrow().clone(), typed("مستقیم"));
        assert!(
            h.drafts().is_empty(),
            "direct mode has nothing to ask the user about"
        );

        h.quit().await;
    }
    // ── application profiles (P1) ─────────────────────────────────────────
    //
    // The *decisions* — general fallback, an override merging field by field, an
    // unknown application, an ambiguous binding, a binding that stops matching —
    // are values, and they are decided and tested in `crate::profiles`. What
    // only this loop can answer is the wiring: that the rules a dictation ran
    // under came from the destination it **started** in, that the conversion
    // path and the insert path agree about which those are, and that a profile
    // can hold its text for review while the general switch says otherwise.

    /// The mishearing the seed dictionary fixes. Standard mode types `پایتون`;
    /// `raw` types exactly what the engine said.
    const MISHEARD: &str = "من با پاتون کار میکنم";
    /// What the seed dictionary makes of it.
    const CORRECTED_WORD: &str = "پایتون";
    /// The word RAW mode must leave untouched, corrector or no corrector.
    const MISHEARD_WORD: &str = "پاتون";

    /// The general rules plus the profiles the scenario wrote.
    ///
    /// The general mode is `Settings`' own default — `standard`, which corrects
    /// — so a profile asking for `raw` has something to be a change *from*. The
    /// general review switch is off, so a profile turning it on is visible.
    fn profiled_settings(profiles: Vec<crate::profiles::AppProfile>) -> Settings {
        let mut settings = Settings::default();
        settings.hotkey.double_tap_latch = false;
        settings.gui.review_before_insert = false;
        settings.profiles = crate::profiles::ProfileSet::new(profiles);
        settings
    }

    /// Everything the sink was told to type, in order, as one string.
    ///
    /// Joined rather than compared as a list because the seam and the boundary
    /// policy legitimately add a separator between two dictations, and a test
    /// about *which rules ran* should not fail over a space.
    fn typed_text(ops: &[Op]) -> String {
        ops.iter()
            .filter_map(|op| match op {
                Op::Type(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A profile that asks for the recogniser's string verbatim.
    fn raw_overrides() -> crate::profiles::Overrides {
        crate::profiles::Overrides {
            text_mode: Some("raw".into()),
            ..Default::default()
        }
    }

    /// A profile for `code.exe` that says `raw`.
    fn raw_profile() -> crate::profiles::AppProfile {
        crate::profiles::AppProfile::new("Editor", "code.exe", raw_overrides())
    }

    /// A profile decides the text rules of the application it names.
    ///
    /// The general mode would have corrected the mishearing; this window's
    /// profile says `raw`, so what reaches the keyboard is the engine's own
    /// string. Nothing else about the rig changed, which is what makes this
    /// about the profile.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_profile_for_the_destination_decides_its_text_rules() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance(MISHEARD);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\Code.exe");
        let mut h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![raw_profile()]),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        let typed = typed_text(&h.seen.borrow().clone());
        assert_eq!(
            typed, MISHEARD,
            "the profile asked for raw, so the engine's string must be typed untouched"
        );
        h.quit().await;
    }

    /// An application nobody wrote a profile for keeps the general rules.
    ///
    /// The negative half of the test above, and the one that matters most: a
    /// user who installed profiles for one application must not have the rest of
    /// their machine governed by them.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_application_without_a_profile_keeps_the_general_rules() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance(MISHEARD);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\Chrome.exe");
        let mut h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![raw_profile()]),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        let typed = typed_text(&h.seen.borrow().clone());
        assert!(
            typed.contains(CORRECTED_WORD) && !typed.contains(MISHEARD_WORD),
            "a window with no profile must run the general rules: {typed:?}"
        );
        h.quit().await;
    }

    /// The window the dictation **started** in decides — even when the user has
    /// moved to another application before the engine answers.
    ///
    /// This is the session-stability criterion, and it is deliberately built out
    /// of a real ordering rather than a hope: the capture happens inside the
    /// loop's own turn, so waiting for it proves the destination was read while
    /// `code.exe` was in front. The switch happens *after* that, and the text
    /// must still be `raw`.
    ///
    /// If the rules were resolved from the foreground window at the moment the
    /// text was ready, the correction below would appear — so this test fails on
    /// exactly the bug it is named for.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn switching_application_mid_dictation_does_not_change_its_rules() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance(MISHEARD);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\Code.exe");
        let mut h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![raw_profile()]),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        // The user clicks into another application while still dictating.
        desktop.belongs_to("C:\\Apps\\Chrome.exe");
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        let typed = typed_text(&h.seen.borrow().clone());
        assert_eq!(
            typed, MISHEARD,
            "the rules followed the window that happened to be in front at the end"
        );
        h.quit().await;
    }

    /// …and the symmetry: a dictation that started in a window with no profile
    /// keeps the general rules even if the user moves *into* an application that
    /// has one.
    ///
    /// Without this half, the test above would pass just as well on an
    /// implementation that always used the general rules.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn moving_into_a_profiled_application_mid_dictation_changes_nothing() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance(MISHEARD);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\Chrome.exe");
        let mut h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![raw_profile()]),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        desktop.belongs_to("C:\\Apps\\Code.exe");
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        let typed = typed_text(&h.seen.borrow().clone());
        assert!(
            typed.contains(CORRECTED_WORD),
            "a dictation does not acquire a profile it never started under: {typed:?}"
        );
        h.quit().await;
    }

    /// A window whose executable could not be read is an unknown application,
    /// and unknown means general — never a guess from the title.
    ///
    /// The rig's default destination has no executable at all, which is the
    /// production case for a process this program is not allowed to open.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_unreadable_executable_gets_the_general_rules() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance(MISHEARD);
        let desktop = ScriptedDesktop::new();
        // A title that *looks* like an executable name, which must not be read
        // as the application's identity.
        desktop.set_title("code.exe");
        let mut h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![raw_profile()]),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        let typed = typed_text(&h.seen.borrow().clone());
        assert!(
            typed.contains(CORRECTED_WORD),
            "the window title must not stand in for the application: {typed:?}"
        );
        h.quit().await;
    }

    /// A profile's own correction rules run in its application — including under
    /// `raw`, where the built-in dictionary was told to stand down.
    ///
    /// `raw` suppresses *implicit* rewriting (a terminal wants the recogniser's
    /// string). A rule the user wrote into that profile by hand is not implicit,
    /// and a panel that accepted it would otherwise never apply it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_profiles_own_rules_run_even_under_raw() {
        // A pair the seed dictionary has never heard of, so the word in the
        // output can only have come from the profile. The first version of this
        // test used a mishearing the built-in list already fixes, and the
        // negative control showed it passing with profiles ignored entirely.
        const SPOKEN: &str = "این تستواره است";
        const PROFILE_FROM: &str = "تستواره";
        const PROFILE_TO: &str = "تستآوره";
        const EXPECTED: &str = "این تستآوره است";

        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance(SPOKEN);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\wt.exe");
        let profile = crate::profiles::AppProfile::new(
            "Terminal",
            "wt.exe",
            crate::profiles::Overrides {
                text_mode: Some("raw".into()),
                corrections: vec![crate::processing::dictionary::Correction {
                    from: PROFILE_FROM.into(),
                    to: PROFILE_TO.into(),
                    category: Some("profile".into()),
                }],
                ..Default::default()
            },
        );
        let mut h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![profile]),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        let typed = typed_text(&h.seen.borrow().clone());
        assert_eq!(
            typed, EXPECTED,
            "raw, plus the one rule this profile owns, is the whole pipeline here"
        );
        h.quit().await;
    }

    /// A profile can hold its text for review in one application while the
    /// general switch stays off.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_profile_can_hold_its_text_for_review() {
        let (h_port, discarded) = port(vec![], vec![ends(1)]);
        let h_engine = one_utterance(MISHEARD);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\Code.exe");
        let profile = crate::profiles::AppProfile::new(
            "Editor",
            "code.exe",
            crate::profiles::Overrides {
                review_before_insert: Some(true),
                ..Default::default()
            },
        );
        let h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![profile]),
        );

        let draft = dictate_once(&h, &desktop).await;

        assert_eq!(draft.kind, DraftKind::Review);
        assert!(
            draft.text.contains(CORRECTED_WORD),
            "the preview must show the text the rules produced: {:?}",
            draft.text
        );
        assert!(
            h.seen.borrow().is_empty(),
            "nothing may reach the keyboard while a profile holds the text: {:?}",
            h.seen.borrow()
        );

        h.quit().await;
    }

    /// …and the other way round: a profile that inserts directly keeps typing
    /// straight away in its own windows, while the general switch holds everyone
    /// else's text.
    ///
    /// This is the override-removal criterion at the loop level. Read on its own
    /// it would pass on an implementation that ignored profiles entirely, which
    /// is why the assertion after it checks that a window *without* a profile is
    /// still held.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_profile_can_insert_directly_while_the_general_switch_holds_text() {
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: MISHEARD,
            },
            Step::Text {
                samples: n(1),
                text: MISHEARD,
            },
        ]);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\Code.exe");
        let profile = crate::profiles::AppProfile::new(
            "Editor",
            "code.exe",
            crate::profiles::Overrides {
                review_before_insert: Some(false),
                ..Default::default()
            },
        );
        let mut settings = profiled_settings(vec![profile]);
        settings.gui.review_before_insert = true;
        let mut h = Harness::start_profiled(h_port, discarded, &h_engine, desktop.clone(), settings);

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the dictation to type itself", h.wait_ops(1)).await;

        assert_eq!(
            h.seen.borrow().len(),
            1,
            "the profile asked for a direct insert: {:?}",
            h.seen.borrow()
        );
        assert!(
            h.drafts().is_empty(),
            "the profile turned review off for this application"
        );

        // The control: the same harness, another window, the general switch.
        desktop.belongs_to("C:\\Apps\\Chrome.exe");
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(2).await;
        h.event(HotkeyEvent::RecordUp);
        let draft = h.wait_draft().await;
        assert_eq!(draft.kind, DraftKind::Review);
        assert_eq!(
            h.seen.borrow().len(),
            1,
            "nothing more may be typed while the general switch holds the text"
        );

        h.quit().await;
    }

    /// Editing the profiles while a dictation is in flight does not re-govern
    /// the work already handed out.
    ///
    /// The rules are read from the **destination the job carries**, so the
    /// profile set is consulted where the work is converted. A user who edits
    /// `[[profiles]]` between two dictations gets the new rules — but the
    /// dictation that is already running is not retroactively rewritten.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_destination_that_stops_matching_loses_its_profile_for_the_next_dictation() {
        let (h_port, discarded) = port(vec![], vec![ends(1), ends(1)]);
        let h_engine = engine(vec![
            Step::Text {
                samples: n(1),
                text: MISHEARD,
            },
            Step::Text {
                samples: n(1),
                text: MISHEARD,
            },
        ]);
        let desktop = ScriptedDesktop::new();
        desktop.belongs_to("C:\\Apps\\Code.exe");
        let mut h = Harness::start_profiled(
            h_port,
            discarded,
            &h_engine,
            desktop.clone(),
            profiled_settings(vec![raw_profile()]),
        );

        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(1).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the first dictation to type itself", h.wait_ops(1)).await;
        assert_eq!(
            typed_text(&h.seen.borrow().clone()),
            MISHEARD,
            "the profiled window types the engine's string"
        );

        // The user renames the executable (an update that moved the app).
        desktop.belongs_to("C:\\Apps\\code2.exe");
        h.event(HotkeyEvent::RecordDown);
        desktop.wait_captured(2).await;
        h.event(HotkeyEvent::RecordUp);
        within(PATIENCE, "the second dictation to type itself", h.wait_ops(2)).await;

        let typed = typed_text(&h.seen.borrow().clone());
        assert!(
            typed.contains(CORRECTED_WORD),
            "the next dictation must run the general rules again: {typed:?}"
        );
        h.quit().await;
    }
}
