//! Text that is ready but has **not** been typed yet.
//!
//! Two features share this surface because they are the same moment seen from
//! two sides:
//!
//! * **review before insert** — the user asked to see the text before it
//!   reaches the keyboard;
//! * **recovery** — the conversion succeeded and the insert did not, so the
//!   text is finished but undelivered.
//!
//! In both cases exactly one thing is true: the text exists, it is not in the
//! document, and a person has to decide what happens to it. So the decisions
//! live here, as pure functions over plain data, and the coordinator keeps
//! only what it needs to honour them.
//!
//! Three rules this module exists to make unbreakable:
//!
//! 1. **Nothing is typed before a decision.** A draft is inert. There is no
//!    path from [`PendingDraft`] to the keyboard that does not pass a
//!    [`ReviewCommand`].
//! 2. **One decision, one delivery.** [`ReviewCommand`] is consumed once; a
//!    replayed or duplicated command finds nothing left to act on.
//! 3. **Retry repeats the insert, never the conversion.** A draft holds text.
//!    There is no audio behind it and no engine to ask again — see the roadmap
//!    decision that recovery is text-only.

use std::time::{Duration, Instant};

use crate::output::target::TargetIdentity;

/// How long a draft may wait for an answer before the app stops offering it.
///
/// The roadmap asks for expiry on "a testable clock", so the duration is a
/// value every caller passes in rather than a constant read from a thread
/// local. Nothing here reads the wall clock on its own.
pub const DEFAULT_DRAFT_TTL: Duration = Duration::from_secs(120);

/// Why this text is waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftKind {
    /// Review mode is on and this text has not been shown to the user yet.
    Review,
    /// The conversion succeeded; the insert did not. This is the recovery case,
    /// and the text is *finished* — only delivery is outstanding.
    Undelivered,
}

impl DraftKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Undelivered => "undelivered",
        }
    }
}

/// Identity of the preserved recovery record a draft was raised from.
///
/// Lives here, next to the draft that carries it, because it exists for exactly
/// one purpose: removing **that** record when the draft is finally inserted.
/// The alternative — clearing the coordinator's whole kept list on a success —
/// deletes text belonging to other drafts and other dictations, which is the
/// failure this feature is supposed to prevent.
///
/// The triple is the one the record itself was created with, so it identifies an
/// insert rather than a moment in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordIdentity {
    pub session: Option<crate::state::session::SessionId>,
    pub chunk: Option<crate::state::session::ChunkId>,
    pub seq: u64,
}

/// One piece of finished text waiting on a decision.
///
/// `id` is what makes rule 2 above enforceable: the answer names the draft it
/// is about, so an answer that arrives after the draft was resolved, expired or
/// superseded cannot be applied to whatever happens to be pending now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDraft {
    pub id: u64,
    pub kind: DraftKind,
    /// **The base text**, as the dictation produced it.
    ///
    /// Deliberately *not* boundary-adjusted: the separator that belongs between
    /// this text and what is already in the document depends on the document at
    /// the moment of insert, and a space baked in here would be a stale guess by
    /// the time the user presses «درج» — or by the time they have edited the
    /// text, which makes the guess wrong even if nothing else moved.
    pub text: String,
    /// The recovery record this draft was raised from, if any.
    ///
    /// `None` for a review draft: nothing failed, so there is no record to
    /// retire. `Some` for an undelivered draft, so a successful insert can
    /// remove exactly its own record.
    pub source_record: Option<RecordIdentity>,
    /// The window this dictation was addressed to. `None` when the session
    /// captured none, which is why an undelivered draft can have no
    /// destination to retry into.
    pub destination: Option<TargetIdentity>,
    /// The dictation this text came from, so cancelling a recording can drop
    /// exactly its own drafts and leave an older dictation's recovery alone.
    pub session: Option<crate::state::session::SessionId>,
    /// When it was raised.
    pub raised: Instant,
}

impl PendingDraft {
    /// Whether there is anything a decision could act on.
    ///
    /// An empty or whitespace-only draft is not offered: "insert" would send
    /// nothing and "copy" would put a blank string on the clipboard, and the
    /// window would be a decision the user cannot make a useful one about.
    pub fn is_worth_offering(&self) -> bool {
        !self.text.trim().is_empty()
    }
}

/// What the user decided about a draft.
///
/// Carries the id so a stale answer is inert, and carries the text so a
/// **user edit** survives: the review window is editable, and the edited text —
/// not the original — is what gets typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewCommand {
    /// Type it now. `text` is what the user is looking at, edits included.
    Insert { id: u64, text: String },
    /// Put it on the clipboard and type nothing.
    Copy { id: u64 },
    /// Discard it. Type nothing.
    Cancel { id: u64 },
    /// Bring an **expired** draft back for a fresh decision.
    ///
    /// The id names the archived draft, not a pending one. This is the only way
    /// text that expired can become insertable again, and it is deliberately a
    /// *new* decision: the restored draft is re-raised under a fresh id with a
    /// fresh deadline, and approving it re-validates the destination like any
    /// other insert. An answer meant for the expired draft's old id still finds
    /// nothing.
    Restore { id: u64 },
    /// Delete one archived draft on the user's explicit request.
    ///
    /// Separate from expiry on purpose: expiry revokes the *permission to
    /// insert*, it does not throw the text away, and throwing it away is only
    /// ever this command, a new dictation, or the program exiting.
    DiscardArchived { id: u64 },
}

impl ReviewCommand {
    /// The draft this command is about.
    pub fn id(&self) -> u64 {
        match self {
            Self::Insert { id, .. }
            | Self::Copy { id }
            | Self::Cancel { id }
            | Self::Restore { id }
            | Self::DiscardArchived { id } => *id,
        }
    }

    /// Whether this command acts on the archive rather than on the pending set.
    pub fn is_archival(&self) -> bool {
        matches!(self, Self::Restore { .. } | Self::DiscardArchived { .. })
    }
}

/// What the coordinator should do with a command.
///
/// The point of this enum is that "the answer did not match anything" is a
/// **value**, not an early `return` buried in the coordinator: an answer for a
/// draft that expired or was already resolved has to be visibly inert, and the
/// only way to keep that honest is to make it something the tests can match on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewOutcome {
    /// Type this text into this destination. Never re-check the destination:
    /// the caller asked, and the coordinator re-validates it.
    Insert {
        text: String,
        destination: Option<TargetIdentity>,
    },
    /// The user copied it. Type nothing.
    Copied { text: String },
    /// The draft was dropped on purpose.
    Cancelled,
    /// The command named a draft that is not pending: already resolved,
    /// expired, or never existed. **Do nothing.**
    Stale,
    /// An archived draft was brought back under a fresh id, with a fresh
    /// deadline and a fresh decision to make.
    Restored { id: u64, new_id: u64 },
    /// One archived draft was deleted because the user asked for exactly that.
    ArchivedDiscarded { id: u64 },
}

/// Decides whether a ready text should be typed immediately or held.
///
/// One function so the two ways of answering "should this be typed?" cannot
/// drift: a setting the user changed and a destination that refused are both
/// just reasons to hold.
pub fn should_hold(kind: DraftKind, review_enabled: bool) -> bool {
    match kind {
        // A refused or failed insert has nothing to do with review mode. The
        // text is undelivered whether or not the user likes seeing drafts, and
        // this is the case where losing it costs the user their dictation.
        DraftKind::Undelivered => true,
        DraftKind::Review => review_enabled,
    }
}

/// The drafts that are still answerable.
///
/// A small ordered map rather than a queue or a single slot, because the
/// multi-chunk session is a real case: two chunks can both fail to insert, and
/// only the *second* is at the front of the user's attention while the first
/// is still unanswered. Ordering by id keeps the session's chunk order, which
/// is the ordering the user dictated in.
/// One piece of text whose deadline passed without an answer.
///
/// This is the F3 archive: expiry **revokes the permission to insert** by taking
/// the draft out of the answerable set, and the text comes here rather than
/// disappearing. It keeps everything the draft had — its id, what it was for,
/// where it was going, which dictation it came from — plus when it expired, so
/// the user can read it, copy it, or bring it back deliberately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedDraft {
    /// The draft exactly as it was, including the id no answer can reach any
    /// more and the text the user dictated.
    pub draft: PendingDraft,
    /// When the deadline passed. Not a deletion time: nothing is deleted here.
    pub expired_at: Instant,
}

/// What an expiry pass did, so the caller can report it honestly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpiryOutcome {
    /// Every draft whose deadline had passed, now in the archive.
    ///
    /// **All of them, at any capacity.** Expiry moves text between the two
    /// buckets of one budget instead of adding to it, so it never needs room
    /// and there is no capacity at which a due draft stays answerable: the
    /// permission to insert is revoked the moment the deadline passes, and the
    /// text is kept. The two are independent by construction, which is what
    /// makes "expiry revoked the insert, the text is still yours" true at every
    /// archive size rather than only below some ceiling (F3).
    pub archived: Vec<ArchivedDraft>,
}

/// What an attempt to put a text up for a decision did.
///
/// An enum rather than `Option` because the two ways an attempt can fail are
/// different facts with different consequences: one has nothing a decision
/// could act on, and the other has text the store is **not** holding — which in
/// review mode also means text the app is not typing. A caller that could not
/// tell them apart could not say either of them honestly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaiseOutcome {
    /// It is waiting for an answer, under this id.
    Raised(PendingDraft),
    /// There was nothing worth offering: an empty or blank text.
    NothingWorthOffering,
    /// The retention budget is full, so nothing new is held.
    ///
    /// Counts only, never text: whoever reports this says the limit was reached
    /// and what the user can do about it, and nothing it logs contains a
    /// dictation.
    RefusedAtCapacity { held: usize, cap: usize },
}

impl RaiseOutcome {
    /// The draft that is now waiting, if one is.
    pub fn raised(&self) -> Option<&PendingDraft> {
        match self {
            Self::Raised(draft) => Some(draft),
            _ => None,
        }
    }

    /// The capacity refusal as `(held, cap)`.
    pub fn refusal(&self) -> Option<(usize, usize)> {
        match self {
            Self::RefusedAtCapacity { held, cap } => Some((*held, *cap)),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct DraftStore {
    next_id: u64,
    drafts: Vec<PendingDraft>,
    archive: Vec<ArchivedDraft>,
    /// Slots promised to work that has been **accepted** but has not produced
    /// its text yet — a recording in progress, a chunk queued, a conversion
    /// running. A promise is not text, so it is not in `drafts`; it is still
    /// spent budget, so it is subtracted from the room. Without this, the app
    /// could accept a dictation, transcribe it, and only then discover there is
    /// nowhere to put the text — dropping a sentence the user really spoke.
    reserved: usize,
}

impl DraftStore {
    /// How many texts may be **held at once**: waiting and expired together.
    ///
    /// One budget over `pending + archive` rather than a cap on the archive
    /// alone. An archive-only ceiling bounds nothing — the answerable set would
    /// be free to grow without limit once the archive was full, which is exactly
    /// the unbounded case a capacity exists to prevent.
    ///
    /// What reaching it does — the capacity-pressure policy, in full:
    ///
    /// 1. **Expiry is unaffected.** Archiving a due draft moves text between the
    ///    two buckets of the same budget, so the total is unchanged and no draft
    ///    is ever left answerable because the archive looks full. Revoking the
    ///    permission to insert and keeping the text are independent, and always
    ///    were.
    /// 2. **Nothing already held is ever deleted to make room** — not the oldest
    ///    archived text, not the newly expired one. Freeing space is the user's
    ///    decision: answer a draft, restore one and answer it, or delete one
    ///    archived text explicitly.
    /// 3. **New text is refused, out loud.** [`Self::raise_from`] answers
    ///    [`RaiseOutcome::RefusedAtCapacity`] instead of holding more, and the
    ///    caller reports it as a user-visible problem. The refused text is not
    ///    stored; in review mode it is not typed either, because a hold that was
    ///    refused is not a decision anyone made. Producing new text stops here,
    ///    at the moment the app would otherwise have kept more of it.
    /// 4. **Work is admitted only if its text can be kept.** A refusal at raise
    ///    time is too late: the dictation has already been spoken and converted,
    ///    so refusing there *drops* text rather than preventing the loss. The
    ///    budget therefore also counts **slots promised to work that has been
    ///    accepted but has not produced its text yet** ([`Self::reserve`] — a
    ///    recording in progress, a queued chunk, a conversion still running),
    ///    and those promises are subtracted from the room. The caller refuses the
    ///    work *before* it starts when no slot can be claimed, and the text of
    ///    work that did claim one is stored with [`Self::raise_reserved`] — the
    ///    conversion of its own promise, never a fresh check against a budget
    ///    its promise already spent.
    ///
    /// In memory by design (the roadmap's decision: retention is text in RAM,
    /// never audio, never a file), which is why the budget can be a constant at
    /// all.
    pub const RETENTION_CAP: usize = 64;

    pub fn new() -> Self {
        Self::default()
    }

    /// Every expired draft whose text is still available, oldest first.
    ///
    /// The **whole** list, never only the newest one: the reader has to be able
    /// to view, copy, restore or delete any of them, and reaching the older ones
    /// must not require deleting the newer ones first.
    pub fn archived(&self) -> &[ArchivedDraft] {
        &self.archive
    }

    pub fn archive_len(&self) -> usize {
        self.archive.len()
    }

    /// Every text being held: waiting and expired together.
    pub fn retention_len(&self) -> usize {
        self.drafts.len() + self.archive.len()
    }

    /// How many slots are promised to accepted work that has not produced its
    /// text yet. Counted against the budget by [`Self::retention_room`], so a
    /// promise made to one dictation cannot be spent twice.
    pub fn reserved(&self) -> usize {
        self.reserved
    }

    /// Claims one slot for work that is about to be accepted.
    ///
    /// Answers `false` when there is no room, and claims nothing in that case:
    /// the caller must then refuse the work **before** it starts — before the
    /// microphone opens, before an engine is asked — rather than let it run and
    /// drop the text it produces. Refusing early is the whole point; the app
    /// must not accept work whose text it has nowhere to keep.
    pub fn reserve(&mut self) -> bool {
        if self.retention_room() == 0 {
            return false;
        }
        self.reserved += 1;
        true
    }

    /// Gives a claimed slot back: the work it was claimed for produced no text
    /// that has to be kept (it was cancelled, it failed, its text was typed, or
    /// its record already holds the text).
    pub fn release_reservation(&mut self) {
        self.reserved = self.reserved.saturating_sub(1);
    }

    /// How many more texts may be held before the budget is full.
    ///
    /// Promised slots are subtracted: a slot handed out to a recording that has
    /// not produced its text yet is already spent, and offering it again is how
    /// the total would exceed the cap.
    pub fn retention_room(&self) -> usize {
        Self::RETENTION_CAP.saturating_sub(self.retention_len() + self.reserved)
    }

    /// Whether the retention budget is full.
    pub fn retention_is_full(&self) -> bool {
        self.retention_room() == 0
    }

    /// Brings one expired draft back as a **new** draft.
    ///
    /// Fresh id, fresh deadline: the returned draft is a new decision, not a
    /// revival of the old one, which is what makes "recovering expired text
    /// requires fresh confirmation" true by construction. Answers addressed to
    /// the old id are still [`ReviewOutcome::Stale`].
    ///
    /// Returns `None` when the id is not in the archive.
    pub fn restore(&mut self, id: u64, now: Instant) -> Option<PendingDraft> {
        let index = self.archive.iter().position(|a| a.draft.id == id)?;
        let archived = self.archive.remove(index);
        // A brand-new answerable draft: the id is minted here, the deadline is
        // restarted from the moment the user asked to recover it, and the
        // dictation it came from is kept so a cancel can still reach it.
        let mut draft = archived.draft.clone();
        draft.raised = now;
        match self.raise_from(draft) {
            RaiseOutcome::Raised(draft) => Some(draft),
            // Unreachable — the text was worth offering when it was raised, and
            // removing it above just freed a slot in a budget that is never
            // per-bucket — but if it ever happened, dropping the text would be
            // the one outcome an archive exists to prevent. So the draft goes
            // back where it was and the caller is told nothing happened.
            _ => {
                self.archive.insert(index, archived);
                None
            }
        }
    }

    /// Deletes one archived draft. True when something was deleted.
    ///
    /// The only path that throws archived text away on a user's behalf, and it
    /// requires the user to name the draft.
    pub fn discard_archived(&mut self, id: u64) -> bool {
        let before = self.archive.len();
        self.archive.retain(|a| a.draft.id != id);
        self.archive.len() != before
    }

    /// Empty the archive on an explicit request. Returns how many were dropped.
    pub fn clear_archive(&mut self) -> usize {
        let count = self.archive.len();
        self.archive.clear();
        count
    }

    /// Drops archived drafts belonging to one dictation.
    ///
    /// Used when the user cancels a recording: text from a dictation they threw
    /// away must not sit in an archive offering to be recovered. This is an
    /// explicit user action, not expiry.
    pub fn forget_archived_session(&mut self, session: crate::state::session::SessionId) -> usize {
        let before = self.archive.len();
        self.archive.retain(|a| a.draft.session != Some(session));
        before - self.archive.len()
    }

    /// Puts a draft up for a decision and reports what happened to it.
    ///
    /// A thin wrapper over [`Self::raise_from`] for the common case; the parts
    /// are passed one at a time so a caller cannot forget one.
    #[cfg(test)]
    pub fn raise(
        &mut self,
        kind: DraftKind,
        text: String,
        destination: Option<TargetIdentity>,
        session: Option<crate::state::session::SessionId>,
        now: Instant,
    ) -> RaiseOutcome {
        self.raise_from(PendingDraft {
            // Overwritten by `raise_from`, which owns numbering.
            id: 0,
            kind,
            text,
            destination,
            session,
            raised: now,
            source_record: None,
        })
    }

    /// Numbers and stores an already-built draft, or says why it did not.
    ///
    /// Separate from [`Self::raise`] so a caller that has the whole draft in
    /// hand — the channel, which the coordinator fills in — does not have to
    /// take it apart and hand the pieces back. The id on `draft` is ignored:
    /// numbering belongs to the store alone, or two holders of it could both
    /// believe they minted the same id.
    pub fn raise_from(&mut self, mut draft: PendingDraft) -> RaiseOutcome {
        // A text nobody can act on is not raised at all. Handing back an id for
        // it would let the coordinator believe it had recorded something it
        // never offered the user.
        if !draft.is_worth_offering() {
            return RaiseOutcome::NothingWorthOffering;
        }
        // Asked **before** the id is minted, so a refusal leaves no trace at
        // all: no id is spent, no draft is stored, and the caller is told which
        // of the two refusals it got rather than a bare "no".
        if self.retention_is_full() {
            return RaiseOutcome::RefusedAtCapacity {
                held: self.retention_len(),
                cap: Self::RETENTION_CAP,
            };
        }
        draft.id = self.next_id;
        self.next_id += 1;
        self.drafts.push(draft.clone());
        RaiseOutcome::Raised(draft)
    }

    /// Stores the text of work that **already holds a slot**.
    ///
    /// The difference from [`Self::raise_from`] is the whole point of the
    /// reservation: a slot claimed when the work was accepted is this caller's
    /// own, so converting it into a held draft is not growth — it is the other
    /// half of a move inside the same budget. Checking the room here instead
    /// would refuse text the budget had already promised to keep, which is the
    /// loss this exists to prevent.
    ///
    /// A caller with nothing reserved falls back to the ordinary rule, so a
    /// bookkeeping slip can still never grow the budget past its cap. Reaching
    /// that fallback is a bug in the caller, not in the store, and the debug
    /// build says so.
    pub fn raise_reserved(&mut self, draft: PendingDraft) -> RaiseOutcome {
        if !draft.is_worth_offering() {
            return RaiseOutcome::NothingWorthOffering;
        }
        if self.reserved == 0 {
            debug_assert!(
                false,
                "raise_reserved without a reservation: the caller has no slot to convert"
            );
            return self.raise_from(draft);
        }
        let mut draft = draft;
        self.reserved -= 1;
        draft.id = self.next_id;
        self.next_id += 1;
        self.drafts.push(draft.clone());
        debug_assert!(
            self.retention_len() <= Self::RETENTION_CAP,
            "a converted reservation must never push the budget past its cap"
        );
        RaiseOutcome::Raised(draft)
    }

    /// Every draft still waiting, oldest first.
    pub fn pending(&self) -> &[PendingDraft] {
        &self.drafts
    }

    /// The newest draft, which is the one a window should open on.
    #[cfg(test)]
    pub fn latest(&self) -> Option<&PendingDraft> {
        self.drafts.last()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.drafts.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.drafts.len()
    }

    /// Resolves one command and removes the draft it named.
    ///
    /// A command for an unknown id is [`ReviewOutcome::Stale`] and changes
    /// nothing — which is what makes a duplicated click, or an answer that
    /// arrives after expiry, harmless rather than a second insert.
    pub fn resolve(&mut self, command: ReviewCommand) -> ReviewOutcome {
        // Restore and discard act on the archive, which needs the caller's clock
        // to restart a deadline. They are handled by the coordinator through
        // `restore`/`discard_archived` before this method is reached; reaching
        // here means the answer was misrouted, and the honest response is
        // "nothing happened".
        if command.is_archival() {
            return ReviewOutcome::Stale;
        }
        let Some(index) = self.drafts.iter().position(|d| d.id == command.id()) else {
            return ReviewOutcome::Stale;
        };
        let draft = self.drafts.remove(index);
        match command {
            // Unreachable: `is_archival` returned above. Present so the match
            // stays exhaustive without a wildcard that could hide a future
            // variant.
            ReviewCommand::Restore { .. } | ReviewCommand::DiscardArchived { .. } => {
                ReviewOutcome::Stale
            }
            ReviewCommand::Insert { text, .. } => {
                if text.trim().is_empty() {
                    // "Insert" with nothing left in the box is a cancel the
                    // user did not mean to make. Typing an empty string is a
                    // no-op, but it would also mark the draft as delivered and
                    // clear it, so it is treated as dropping the text.
                    ReviewOutcome::Cancelled
                } else {
                    ReviewOutcome::Insert {
                        text,
                        destination: draft.destination,
                    }
                }
            }
            ReviewCommand::Copy { .. } => ReviewOutcome::Copied { text: draft.text },
            ReviewCommand::Cancel { .. } => ReviewOutcome::Cancelled,
        }
    }

    /// Moves every draft older than `ttl` into the archive, and reports what
    /// moved.
    ///
    /// **Revokes, never deletes.** A due draft leaves the answerable set — so a
    /// late answer or a second click finds nothing — and its text comes back in
    /// the outcome instead of vanishing. That happens at every capacity, because
    /// archiving is a move inside one budget rather than growth: the roadmap is
    /// explicit that lost text is the failure this whole feature exists to
    /// prevent, and "it quietly expired" would be the same outcome with none of
    /// the honesty.
    pub fn expire(&mut self, now: Instant, ttl: Duration) -> ExpiryOutcome {
        let mut outcome = ExpiryOutcome::default();
        let mut kept = Vec::with_capacity(self.drafts.len());
        for draft in self.drafts.drain(..) {
            if now.saturating_duration_since(draft.raised) < ttl {
                kept.push(draft);
                continue;
            }
            let archived = ArchivedDraft {
                draft,
                expired_at: now,
            };
            self.archive.push(archived.clone());
            outcome.archived.push(archived);
        }
        self.drafts = kept;
        // The invariants this pass has to preserve, checked where they could be
        // broken: every due draft is now archived (none is silently dropped),
        // none is left answerable, and the budget is still respected because
        // moving text adds none of it. The middle one is what makes the loop's
        // wake-up deadline a **future** moment after a pass — and therefore what
        // keeps a processed deadline from waking it again.
        debug_assert!(
            self.drafts
                .iter()
                .all(|d| now.saturating_duration_since(d.raised) < ttl),
            "a pass must not leave a due draft answerable"
        );
        debug_assert!(
            self.retention_len() <= Self::RETENTION_CAP,
            "expiry must move text, not grow the budget"
        );
        outcome
    }

    /// Drops the drafts belonging to `session`, used when the user cancels a
    /// recording: text from a dictation they threw away must not sit in a
    /// window offering to be inserted later.
    pub fn forget_session(&mut self, session: crate::state::session::SessionId) -> usize {
        let before = self.drafts.len();
        self.drafts.retain(|d| d.session != Some(session));
        let pending = before - self.drafts.len();
        pending + self.forget_archived_session(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(pid: u32) -> TargetIdentity {
        TargetIdentity {
            hwnd: 100,
            pid,
            exe_path: None,
            title_at_capture: "doc".into(),
            focus_hwnd: None,
            focus_element: None,
        }
    }

    /// Raises a draft that must be accepted — the setup step of scenarios about
    /// something else. A refusal here is a bug in the test, so it panics loudly
    /// rather than returning an `Option` the scenario would then unwrap anyway.
    fn must_raise(
        s: &mut DraftStore,
        kind: DraftKind,
        text: &str,
        destination: Option<TargetIdentity>,
        session: Option<crate::state::session::SessionId>,
        now: Instant,
    ) -> PendingDraft {
        match s.raise(kind, text.into(), destination, session, now) {
            RaiseOutcome::Raised(draft) => draft,
            other => panic!("expected a raised draft, got {other:?}"),
        }
    }

    fn store_with_one(now: Instant) -> DraftStore {
        let mut s = DraftStore::new();
        must_raise(
            &mut s,
            DraftKind::Undelivered,
            "سلام",
            Some(target(7)),
            None,
            now,
        );
        s
    }

    // ── offering ─────────────────────────────────────────────────────────

    #[test]
    fn an_empty_text_is_never_offered() {
        let mut s = DraftStore::new();
        for blank in ["", "   ", "\n\t "] {
            assert_eq!(
                s.raise(
                    DraftKind::Undelivered,
                    blank.into(),
                    None,
                    None,
                    Instant::now()
                ),
                RaiseOutcome::NothingWorthOffering,
                "{blank:?} cannot be inserted or usefully copied"
            );
        }
        assert!(s.is_empty(), "an unraised draft must leave nothing pending");
    }

    #[test]
    fn ids_are_unique_so_an_answer_cannot_hit_the_wrong_draft() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        let a = must_raise(&mut s, DraftKind::Undelivered, "یک", None, None, now);
        let b = must_raise(&mut s, DraftKind::Undelivered, "دو", None, None, now);
        assert_ne!(a.id, b.id);
    }

    // ── deciding ─────────────────────────────────────────────────────────

    #[test]
    fn insert_carries_the_edited_text_not_the_original() {
        let now = Instant::now();
        let mut s = store_with_one(now);
        let id = s.latest().unwrap().id;
        assert_eq!(
            s.resolve(ReviewCommand::Insert {
                id,
                text: "سلام دنیا".into()
            }),
            ReviewOutcome::Insert {
                text: "سلام دنیا".into(),
                destination: Some(target(7)),
            },
            "the user's edit is what gets typed"
        );
        assert!(s.is_empty(), "a resolved draft is no longer pending");
    }

    #[test]
    fn copy_and_cancel_never_type_anything() {
        let now = Instant::now();
        let mut s = store_with_one(now);
        let id = s.latest().unwrap().id;
        assert_eq!(
            s.resolve(ReviewCommand::Copy { id }),
            ReviewOutcome::Copied {
                text: "سلام".into()
            }
        );

        let mut s = store_with_one(now);
        let id = s.latest().unwrap().id;
        assert_eq!(
            s.resolve(ReviewCommand::Cancel { id }),
            ReviewOutcome::Cancelled
        );
    }

    /// The rule that stops one click from typing a dictation twice.
    #[test]
    fn one_decision_inserts_once_and_a_replay_is_inert() {
        let now = Instant::now();
        let mut s = store_with_one(now);
        let id = s.latest().unwrap().id;
        let command = ReviewCommand::Insert {
            id,
            text: "سلام".into(),
        };

        assert!(matches!(
            s.resolve(command.clone()),
            ReviewOutcome::Insert { .. }
        ));
        assert_eq!(
            s.resolve(command),
            ReviewOutcome::Stale,
            "the same answer a second time must type nothing"
        );
    }

    #[test]
    fn an_answer_for_a_draft_that_never_existed_is_inert() {
        let mut s = DraftStore::new();
        assert_eq!(
            s.resolve(ReviewCommand::Cancel { id: 999 }),
            ReviewOutcome::Stale
        );
    }

    /// Inserting an emptied box is a cancel the user did not mean to make.
    #[test]
    fn inserting_nothing_left_is_a_cancel_not_an_empty_insert() {
        let now = Instant::now();
        let mut s = store_with_one(now);
        let id = s.latest().unwrap().id;
        assert_eq!(
            s.resolve(ReviewCommand::Insert {
                id,
                text: "   ".into()
            }),
            ReviewOutcome::Cancelled
        );
    }

    /// Several chunks from one dictation can both fail; answering must be able
    /// to reach each of them by its own id, in any order.
    #[test]
    fn several_drafts_are_answered_independently_and_keep_their_order() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        let first = must_raise(
            &mut s,
            DraftKind::Undelivered,
            "اول",
            Some(target(1)),
            None,
            now,
        );
        let second = must_raise(
            &mut s,
            DraftKind::Undelivered,
            "دوم",
            Some(target(1)),
            None,
            now,
        );

        assert_eq!(
            s.pending().iter().map(|d| d.id).collect::<Vec<_>>(),
            vec![first.id, second.id],
            "the session's chunk order is the order the user dictated in"
        );

        // Answering the older one first leaves the newer pending.
        assert!(matches!(
            s.resolve(ReviewCommand::Insert {
                id: first.id,
                text: "اول".into()
            }),
            ReviewOutcome::Insert { .. }
        ));
        assert_eq!(s.len(), 1);
        assert_eq!(s.latest().unwrap().id, second.id);
    }

    // ── expiry ───────────────────────────────────────────────────────────

    /// Expiry is driven by an injected clock so it can be tested, and it hands
    /// the text **back** rather than dropping it silently.
    #[test]
    fn an_unanswered_draft_expires_with_its_text_intact() {
        let now = Instant::now();
        let mut s = store_with_one(now);

        let not_yet = s.expire(
            now + DEFAULT_DRAFT_TTL - Duration::from_secs(1),
            DEFAULT_DRAFT_TTL,
        );
        assert!(not_yet.archived.is_empty(), "not due yet");

        let outcome = s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);
        assert_eq!(outcome.archived.len(), 1);
        assert_eq!(
            outcome.archived[0].draft.text, "سلام",
            "expiry must not lose the text"
        );
        assert!(s.is_empty(), "expiry revokes the permission to insert");
        assert_eq!(s.archived().len(), 1, "and the text is still reachable");

        // An answer arriving after the deadline finds nothing — the old id is
        // dead, which is the whole point of revoking on expiry.
        assert_eq!(
            s.resolve(ReviewCommand::Cancel {
                id: outcome.archived[0].draft.id
            }),
            ReviewOutcome::Stale
        );
    }

    #[test]
    fn expiry_only_takes_what_is_due() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        let old = must_raise(&mut s, DraftKind::Undelivered, "قدیمی", None, None, now);
        let new = must_raise(
            &mut s,
            DraftKind::Undelivered,
            "جدید",
            None,
            None,
            now + Duration::from_secs(30),
        );

        let outcome = s.expire(now + Duration::from_secs(60), Duration::from_secs(45));
        assert_eq!(
            outcome
                .archived
                .iter()
                .map(|a| a.draft.id)
                .collect::<Vec<_>>(),
            vec![old.id],
            "the fresh draft is not due and must survive"
        );
        assert_eq!(s.latest().unwrap().id, new.id);
    }

    // ── F3: the archive that expiry moves text into ──────────────────────

    /// The finding in one test: expiry must not be the end of the text.
    #[test]
    fn an_expired_draft_keeps_its_destination_origin_and_identity() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        let session = crate::state::session::SessionId(9);
        must_raise(
            &mut s,
            DraftKind::Undelivered,
            "متن گم‌شده",
            Some(target(42)),
            Some(session),
            now,
        );

        let outcome = s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);
        let archived = &outcome.archived[0];
        assert_eq!(archived.draft.id, 0, "the id is kept, not recycled");
        assert_eq!(archived.draft.kind, DraftKind::Undelivered);
        assert_eq!(archived.draft.text, "متن گم‌شده");
        assert_eq!(archived.draft.destination, Some(target(42)));
        assert_eq!(archived.draft.session, Some(session));
        assert_eq!(archived.expired_at, now + DEFAULT_DRAFT_TTL);
    }

    /// Recovery is a **fresh** decision: new id, restarted deadline. This is
    /// what makes a late answer to the old id unable to insert anything.
    #[test]
    fn restoring_an_expired_draft_mints_a_new_id_and_a_new_deadline() {
        let now = Instant::now();
        let mut s = store_with_one(now);
        let old_id = s.pending()[0].id;
        s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);

        let later = now + Duration::from_secs(600);
        let restored = s.restore(old_id, later).expect("it was archived");
        assert_ne!(restored.id, old_id, "recovery is a new decision");
        assert_eq!(restored.raised, later, "and it gets a fresh deadline");
        assert_eq!(restored.text, "سلام");

        // The old id stays dead: it can neither insert nor cancel.
        assert_eq!(
            s.resolve(ReviewCommand::Cancel { id: old_id }),
            ReviewOutcome::Stale
        );
        assert_eq!(s.pending().len(), 1, "the restored draft is answerable");
        assert!(s.archived().is_empty(), "and it is no longer archived");
    }

    /// Restoring something that is not archived changes nothing.
    #[test]
    fn restoring_an_unknown_id_changes_nothing() {
        let mut s = store_with_one(Instant::now());
        assert!(s.restore(999, Instant::now()).is_none());
        assert_eq!(s.pending().len(), 1);
    }

    /// An archived draft survives until the user deletes it: nothing on the
    /// expiry path throws text away.
    #[test]
    fn archived_text_is_only_removed_by_an_explicit_request() {
        let now = Instant::now();
        let mut s = store_with_one(now);
        s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);
        assert_eq!(s.archived().len(), 1);

        // Expiring again does not touch what is already archived.
        s.expire(now + DEFAULT_DRAFT_TTL * 4, DEFAULT_DRAFT_TTL);
        assert_eq!(s.archived().len(), 1);

        assert!(s.discard_archived(0), "the user named it");
        assert!(s.archived().is_empty());
        assert!(!s.discard_archived(0), "and it is gone exactly once");
    }

    /// F3, the finding: **expiry revokes the permission to insert whatever the
    /// archive holds.** A full budget must not leave a stale draft answerable,
    /// because "your text is safe" and "this id can still type" are two
    /// different promises, and only the second one expired.
    #[test]
    fn expiry_revokes_the_permission_to_insert_at_any_capacity() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        // Fill the whole budget with drafts that are already due, so the store
        // is at its cap before anything is expired.
        for i in 0..DraftStore::RETENTION_CAP {
            must_raise(
                &mut s,
                DraftKind::Undelivered,
                &format!("متن {i}"),
                None,
                None,
                now,
            );
        }
        assert!(s.retention_is_full());
        assert_eq!(s.retention_room(), 0);

        let outcome = s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);
        assert_eq!(
            outcome.archived.len(),
            DraftStore::RETENTION_CAP,
            "every due draft leaves the answerable set, cap or no cap"
        );
        assert!(
            s.pending().is_empty(),
            "no draft may stay answerable past its deadline"
        );
        assert_eq!(
            s.archived().len(),
            DraftStore::RETENTION_CAP,
            "and every text is kept"
        );

        // The old ids are dead: an answer that arrives late types nothing.
        for archived in s.archived().to_vec() {
            assert_eq!(
                s.resolve(ReviewCommand::Insert {
                    id: archived.draft.id,
                    text: archived.draft.text.clone(),
                }),
                ReviewOutcome::Stale,
                "an expired id must not be insertable, whatever the archive holds"
            );
        }
    }

    /// The capacity-pressure policy in one test: at the budget new text is
    /// refused with the numbers that explain why, nothing already held is
    /// deleted, and freeing room is a user's decision that one text is enough
    /// for.
    #[test]
    fn at_the_budget_new_text_is_refused_and_nothing_already_held_is_deleted() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        for i in 0..DraftStore::RETENTION_CAP {
            must_raise(
                &mut s,
                DraftKind::Undelivered,
                &format!("متن {i}"),
                None,
                None,
                now,
            );
        }
        s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);

        let refused = s.raise(DraftKind::Undelivered, "تازه".into(), None, None, now);
        assert_eq!(
            refused,
            RaiseOutcome::RefusedAtCapacity {
                held: DraftStore::RETENTION_CAP,
                cap: DraftStore::RETENTION_CAP,
            },
            "at the budget the store refuses rather than holding more"
        );
        assert_eq!(
            s.retention_len(),
            DraftStore::RETENTION_CAP,
            "nothing was kept"
        );
        assert_eq!(s.pending().len(), 0, "and no new id was minted");
        assert_eq!(
            s.archived()[0].draft.text,
            "متن 0",
            "the oldest text is still the user's"
        );

        // Freeing room is the user's decision, and naming one text is enough.
        assert!(s.discard_archived(0), "the user named the oldest one");
        assert!(!s.retention_is_full());
        let raised = s.raise(DraftKind::Undelivered, "تازه".into(), None, None, now);
        assert!(
            matches!(raised, RaiseOutcome::Raised(_)),
            "with room, new text is held again: {raised:?}"
        );
    }

    /// Restoring is a move **inside** the budget, so it works even when the
    /// budget is full: the text is already the user's and cannot be refused.
    #[test]
    fn restoring_works_even_when_the_budget_is_full() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        must_raise(&mut s, DraftKind::Undelivered, "تنها", None, None, now);
        s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);
        // Fill the remaining room so the store is exactly at its cap.
        for i in 0..DraftStore::RETENTION_CAP - 1 {
            must_raise(
                &mut s,
                DraftKind::Undelivered,
                &format!("پر {i}"),
                None,
                None,
                now,
            );
        }
        assert!(s.retention_is_full());

        let restored = s.restore(0, now).expect("the archived text came back");
        assert_eq!(restored.text, "تنها");
        assert_eq!(
            s.retention_len(),
            DraftStore::RETENTION_CAP,
            "restoring is a move, not growth"
        );
        assert!(s.archived().is_empty(), "it left the archive");
        assert_eq!(
            s.pending().len(),
            DraftStore::RETENTION_CAP,
            "and it is answerable again"
        );
    }

    /// Cancelling a recording must drop **its** drafts and only its (and, by
    /// the same explicit-action rule, its archived text): an older
    /// dictation's undelivered text is still the user's, and a cancel cannot be
    /// allowed to reach back and take it.
    #[test]
    fn cancelling_a_dictation_drops_only_its_own_drafts() {
        let now = Instant::now();
        let mine = crate::state::session::SessionId(4);
        let older = crate::state::session::SessionId(3);
        let mut s = DraftStore::new();
        must_raise(
            &mut s,
            DraftKind::Undelivered,
            "قدیمی",
            None,
            Some(older),
            now,
        );
        let doomed = must_raise(
            &mut s,
            DraftKind::Undelivered,
            "لغو شده",
            None,
            Some(mine),
            now,
        );

        assert_eq!(s.forget_session(mine), 1);
        assert_eq!(
            s.pending().iter().map(|d| d.id).collect::<Vec<_>>(),
            vec![doomed.id - 1],
            "the other dictation's text must still be offered"
        );
    }

    // ── the hold policy ──────────────────────────────────────────────────

    /// Recovery does not depend on the review setting. Someone who never asked
    /// to see drafts still gets told their text did not land.
    #[test]
    fn undelivered_text_is_always_held_even_with_review_off() {
        assert!(should_hold(DraftKind::Undelivered, false));
        assert!(should_hold(DraftKind::Undelivered, true));
    }

    #[test]
    fn review_mode_is_the_only_thing_that_holds_a_fresh_draft() {
        assert!(should_hold(DraftKind::Review, true));
        assert!(
            !should_hold(DraftKind::Review, false),
            "direct mode must stay direct"
        );
    }

    #[test]
    fn every_kind_names_itself_for_the_log() {
        assert_eq!(DraftKind::Review.as_str(), "review");
        assert_eq!(DraftKind::Undelivered.as_str(), "undelivered");
    }

    #[test]
    fn a_command_reports_the_draft_it_is_about() {
        assert_eq!(ReviewCommand::Cancel { id: 3 }.id(), 3);
        assert_eq!(ReviewCommand::Copy { id: 4 }.id(), 4);
        assert_eq!(
            ReviewCommand::Insert {
                id: 5,
                text: "x".into()
            }
            .id(),
            5
        );
    }
}
