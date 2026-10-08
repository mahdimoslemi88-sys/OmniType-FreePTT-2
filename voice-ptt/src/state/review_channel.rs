//! The shared wire between the loop and the dashboard for review and recovery.
//!
//! Two directions, two channels, and the asymmetry is the point:
//!
//! * **loop → GUI**: drafts the user has to answer. This is a `watch` channel
//!   so the GUI can read the current state in its frame loop without a
//!   subscription of its own, and so a missed notification costs a frame
//!   rather than a draft.
//! * **GUI → loop**: the answers. A `mpsc` channel, because each command is
//!   consumed exactly once and the loop must act on it in order.
//!
//! It is deliberately **not** the status channel. `StatusChannel` answers "what
//! is the app doing"; this answers "there is text that is not in your document
//! and only you can decide". Folding the second into the first would mean a
//! text recovery would have to pretend to be a state, and would lose the
//! payload — the state packet is cloned into a `watch`, and a draft's text is
//! not something that belongs in there.

use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::mpsc;

use super::review::{
    ArchivedDraft, DraftStore, ExpiryOutcome, PendingDraft, RaiseOutcome, ReviewCommand,
    ReviewOutcome,
};

/// Everything the dashboard needs to know about pending text, read once a frame.
///
/// `revision` is bumped on every change so a reader can tell "nothing pending"
/// from "something changed and I did not look". Without it a GUI that polls
/// would have to diff the whole list to notice one draft arriving.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewSnapshot {
    /// Oldest first — the order the user dictated in.
    pub drafts: Vec<PendingDraft>,
    /// Expired drafts whose text is still available, oldest first.
    ///
    /// Carried in the same snapshot as the pending drafts because a reader that
    /// saw the two separately could draw a frame in which text the user was told
    /// is safe appears to have vanished.
    pub archived: Vec<ArchivedDraft>,
    pub revision: u64,
}

impl ReviewSnapshot {
    /// The draft a window should open on: the newest.
    pub fn latest(&self) -> Option<&PendingDraft> {
        self.drafts.last()
    }

    pub fn is_empty(&self) -> bool {
        self.drafts.is_empty()
    }

    /// Whether anything expired is still the user's to read, copy or restore.
    ///
    /// "Something", not "the newest one": the archive view lists the whole slice
    /// (`archived`), because an entry that could only be reached by deleting
    /// another one is exactly the trade this archive refuses to make (F3).
    pub fn has_archived(&self) -> bool {
        !self.archived.is_empty()
    }
}

/// The loop's end of the review wire.
///
/// Held as an `Arc` because both sides are handed out separately: the
/// coordinator writes drafts, the dashboard reads them and writes answers.
#[derive(Debug)]
pub struct ReviewChannel {
    drafts: Mutex<DraftStore>,
    /// The revision counter, as a watch so a reader can *wait* on a change
    /// rather than poll for it.
    ///
    /// The drafts themselves stay behind a mutex and are read through
    /// [`Self::snapshot`]. Publishing them down the watch would mean holding the
    /// whole list twice and cloning it on every raise, for a reader that only
    /// needs to know *that* something changed before it looks.
    revision: tokio::sync::watch::Sender<u64>,
    answers_tx: mpsc::UnboundedSender<ReviewCommand>,
    answers_rx: Mutex<Option<mpsc::UnboundedReceiver<ReviewCommand>>>,
}

impl ReviewChannel {
    /// Builds the wire.
    ///
    /// One handle, not two: the answering end comes out of [`Self::take_receiver`]
    /// rather than being handed back here, so that "the loop took the answers"
    /// is a thing that happened and can be observed once, rather than a
    /// possibility that two callers each believe they own.
    pub fn new() -> Arc<Self> {
        let (answers_tx, answers_rx) = mpsc::unbounded_channel();
        let (revision, _) = tokio::sync::watch::channel(0);
        Arc::new(Self {
            drafts: Mutex::new(DraftStore::new()),
            revision,
            answers_tx,
            answers_rx: Mutex::new(Some(answers_rx)),
        })
    }

    /// A receiver that fires when the pending set changes.
    ///
    /// Subscribe **before** reading the snapshot if the caller intends to wait:
    /// a change raised between the read and the wait is exactly the one a
    /// subscribe-then-read order cannot miss and the reverse order can.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.revision.subscribe()
    }

    /// The loop's own receiver. Taken once, by the coordinator's constructor.
    ///
    /// `None` the second time is the point: a draft's fate is a decision about
    /// the keyboard, and the keyboard has exactly one owner.
    pub fn take_receiver(&self) -> Option<mpsc::UnboundedReceiver<ReviewCommand>> {
        self.answers_rx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Raises a draft for a decision and reports what happened to it.
    ///
    /// The outcome is passed through rather than flattened to an `Option`
    /// because "there was nothing to offer" and "the store is full and is
    /// holding nothing new" are different facts: only the second one is a
    /// problem the user has to be told about (F3).
    pub fn raise(&self, draft: PendingDraft) -> RaiseOutcome {
        let outcome = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .raise_from(draft);
        if outcome.raised().is_some() {
            self.bump();
        }
        outcome
    }

    /// The preserved record a pending draft was raised from, if any.
    ///
    /// Read **before** the answer is resolved, because resolving removes the
    /// draft. It is what lets a successful insert retire exactly its own record
    /// instead of clearing the coordinator's whole kept list.
    pub fn source_record(&self, id: u64) -> Option<super::review::RecordIdentity> {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending()
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| d.source_record)
    }

    /// Resolves an answer and says what the loop should do about it.
    pub fn resolve(&self, command: ReviewCommand) -> ReviewOutcome {
        let outcome = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .resolve(command);
        if outcome != ReviewOutcome::Stale {
            self.bump();
        }
        outcome
    }

    /// Moves every draft whose deadline has passed into the archive.
    ///
    /// "Moves", not "drops": the returned outcome names what is now archived
    /// and every due draft is in it, at any capacity, because revoking the
    /// permission to insert and keeping the text are independent.
    pub fn expire(&self, now: std::time::Instant, ttl: std::time::Duration) -> ExpiryOutcome {
        let outcome = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .expire(now, ttl);
        if !outcome.archived.is_empty() {
            self.bump();
        }
        outcome
    }

    /// Brings one expired draft back as a fresh, answerable draft.
    pub fn restore(&self, id: u64, now: std::time::Instant) -> Option<PendingDraft> {
        let restored = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .restore(id, now);
        if restored.is_some() {
            self.bump();
        }
        restored
    }

    /// Deletes one archived draft because the user asked for that one.
    pub fn discard_archived(&self, id: u64) -> bool {
        let discarded = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .discard_archived(id);
        if discarded {
            self.bump();
        }
        discarded
    }

    /// How many expired drafts are currently available to the user.
    pub fn archive_len(&self) -> usize {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .archive_len()
    }

    /// Every text being held: waiting and expired together.
    pub fn retention_len(&self) -> usize {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retention_len()
    }

    /// Whether the retention budget is full, so no new text would be held.
    ///
    /// Asked by the loop so the state can be reported **when it is reached**
    /// rather than on every turn it lasts: a warning that repeats every frame
    /// stops being a warning.
    pub fn retention_is_full(&self) -> bool {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retention_is_full()
    }

    /// How many more texts could be held right now, promises included.
    ///
    /// Zero is what stops a recording from opening its microphone: work must not
    /// start when the text it produces would have nowhere to go.
    pub fn retention_room(&self) -> usize {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retention_room()
    }

    /// How many slots are promised to accepted work that has not produced its
    /// text yet. Exposed for the loop's own bookkeeping and for the tests that
    /// prove a promise is released exactly once.
    pub fn reserved(&self) -> usize {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reserved()
    }

    /// Claims a slot for work about to be accepted, or says there is none.
    ///
    /// One lock: two dictations starting at the same moment cannot both be told
    /// yes for the same last slot.
    pub fn reserve(&self) -> bool {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reserve()
    }

    /// Gives a claimed slot back, for work that produced nothing to keep.
    pub fn release_reservation(&self) {
        self.drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .release_reservation();
    }

    /// Stores the text of work that already holds a slot, converting that slot
    /// into a held draft instead of asking for a new one.
    pub fn raise_reserved(&self, draft: PendingDraft) -> RaiseOutcome {
        let outcome = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .raise_reserved(draft);
        if outcome.raised().is_some() {
            self.bump();
        }
        outcome
    }

    /// Empties the archive on an explicit request. Returns how many were dropped.
    pub fn clear_archive(&self) -> usize {
        let cleared = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear_archive();
        if cleared > 0 {
            self.bump();
        }
        cleared
    }

    /// Drops one dictation's drafts when the user cancels that recording.
    pub fn forget_session(&self, session: crate::state::session::SessionId) -> usize {
        let dropped = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .forget_session(session);
        if dropped > 0 {
            self.bump();
        }
        dropped
    }

    /// What the dashboard reads each frame.
    pub fn snapshot(&self) -> ReviewSnapshot {
        let store = self.drafts.lock().unwrap_or_else(PoisonError::into_inner);
        let drafts = store.pending().to_vec();
        let archived = store.archived().to_vec();
        let revision = *self.revision.borrow();
        ReviewSnapshot {
            drafts,
            archived,
            revision,
        }
    }

    /// Sends an answer to the loop. Called from the GUI thread.
    ///
    /// A failed send means the loop is gone; the caller has nothing useful to
    /// do about it, and pretending otherwise would mean the dashboard sits on
    /// a window offering to insert text into a program that has exited.
    pub fn answer(&self, command: ReviewCommand) -> bool {
        self.answers_tx.send(command).is_ok()
    }

    fn bump(&self) {
        // `send_modify` rather than `send`: the receiver's *value* is not read
        // for the number, only for "something changed", and `send` refuses once
        // every receiver is gone — which would silently stop the review window
        // noticing drafts after a dashboard reload.
        self.revision.send_modify(|revision| {
            *revision = revision.wrapping_add(1);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::review::{DraftKind, ReviewOutcome};

    fn draft(text: &str, id: u64) -> PendingDraft {
        PendingDraft {
            id,
            kind: DraftKind::Undelivered,
            text: text.into(),
            destination: None,
            session: None,
            raised: std::time::Instant::now(),
            source_record: None,
        }
    }

    /// Raises through the channel, for scenarios about something else.
    fn must_raise(channel: &ReviewChannel, draft: PendingDraft) -> PendingDraft {
        match channel.raise(draft) {
            RaiseOutcome::Raised(draft) => draft,
            other => panic!("expected a raised draft, got {other:?}"),
        }
    }

    #[test]
    fn a_raised_draft_reaches_the_reader_with_a_bumped_revision() {
        let channel = ReviewChannel::new();
        let before = channel.snapshot();
        assert!(before.is_empty());

        must_raise(&channel, draft("سلام", 0));
        let after = channel.snapshot();
        assert_eq!(after.drafts.len(), 1);
        assert_eq!(after.latest().unwrap().text, "سلام");
        assert_ne!(
            before.revision, after.revision,
            "a reader that only watches the revision must still notice"
        );
    }

    #[test]
    fn an_answer_resolves_the_draft_it_names_and_bumps_the_revision() {
        let channel = ReviewChannel::new();
        let raised = must_raise(&channel, draft("سلام", 0));
        let before = channel.snapshot().revision;

        assert_eq!(
            channel.resolve(ReviewCommand::Insert {
                id: raised.id,
                text: "سلام دنیا".into()
            }),
            ReviewOutcome::Insert {
                text: "سلام دنیا".into(),
                destination: None,
            }
        );
        assert!(channel.snapshot().is_empty());
        assert_ne!(channel.snapshot().revision, before);
    }

    /// A stale answer changes nothing, and — importantly — does not pretend
    /// something happened by bumping the revision.
    #[test]
    fn a_stale_answer_changes_nothing() {
        let channel = ReviewChannel::new();
        let raised = must_raise(&channel, draft("سلام", 0));
        assert_eq!(
            channel.resolve(ReviewCommand::Cancel { id: raised.id }),
            ReviewOutcome::Cancelled
        );
        let after_first = channel.snapshot();

        assert_eq!(
            channel.resolve(ReviewCommand::Cancel { id: raised.id }),
            ReviewOutcome::Stale
        );
        assert_eq!(channel.snapshot(), after_first);
    }

    #[test]
    fn an_answer_reaches_the_loop_over_the_channel() {
        let channel = ReviewChannel::new();
        let mut rx = channel.take_receiver().expect("the loop takes the answers");
        assert!(channel.answer(ReviewCommand::Cancel { id: 7 }));
        assert_eq!(
            rx.try_recv().expect("the answer was sent"),
            ReviewCommand::Cancel { id: 7 }
        );
    }

    /// The receiver is taken exactly once: two loops resolving answers would be
    /// two owners of the keyboard.
    #[test]
    fn the_answers_receiver_can_only_be_taken_once() {
        let channel = ReviewChannel::new();
        assert!(channel.take_receiver().is_some());
        assert!(
            channel.take_receiver().is_none(),
            "a second loop must not be able to claim the answers"
        );
    }

    #[test]
    fn expiry_reaches_the_reader_with_the_text_intact() {
        let channel = ReviewChannel::new();
        let now = std::time::Instant::now();
        let mut d = draft("سلام", 0);
        d.raised = now;
        must_raise(&channel, d);

        let ttl = std::time::Duration::from_secs(10);
        assert!(
            channel
                .expire(now + ttl - std::time::Duration::from_secs(1), ttl)
                .archived
                .is_empty(),
            "a draft that has not reached its deadline must still be there"
        );
        let outcome = channel.expire(now + ttl, ttl);
        assert_eq!(outcome.archived.len(), 1);
        assert_eq!(outcome.archived[0].draft.text, "سلام");

        // The pending set is empty — the permission to insert is gone — but the
        // reader is handed the text in the same snapshot that says so.
        let snapshot = channel.snapshot();
        assert!(snapshot.is_empty());
        assert!(snapshot.has_archived(), "the text must not vanish");
        assert_eq!(snapshot.archived.last().unwrap().draft.text, "سلام");
        assert_eq!(snapshot.archived[0].draft.destination, None);
    }

    /// Recovery is a fresh decision, and the reader sees the new draft instead
    /// of the archived one. An answer to the old id is inert.
    #[test]
    fn a_restored_draft_becomes_answerable_under_a_new_id() {
        let channel = ReviewChannel::new();
        let now = std::time::Instant::now();
        let mut d = draft("سلام", 0);
        d.raised = now;
        let raised = must_raise(&channel, d);
        let ttl = std::time::Duration::from_secs(10);
        channel.expire(now + ttl, ttl);

        let restored = channel
            .restore(raised.id, now + ttl + std::time::Duration::from_secs(1))
            .expect("the draft was archived");
        assert_ne!(restored.id, raised.id);
        let snapshot = channel.snapshot();
        assert_eq!(snapshot.drafts.len(), 1);
        assert_eq!(snapshot.drafts[0].text, "سلام");
        assert!(!snapshot.has_archived(), "it is not in two places at once");

        // The old answer still reaches nothing.
        assert_eq!(
            channel.resolve(ReviewCommand::Cancel { id: raised.id }),
            ReviewOutcome::Stale
        );
        // And the restored one works normally.
        assert_eq!(
            channel.resolve(ReviewCommand::Cancel { id: restored.id }),
            ReviewOutcome::Cancelled
        );
    }

    /// An explicit request is the only thing that deletes archived text.
    #[test]
    fn archived_text_is_deleted_only_when_the_user_names_it() {
        let channel = ReviewChannel::new();
        let now = std::time::Instant::now();
        let mut d = draft("سلام", 0);
        d.raised = now;
        must_raise(&channel, d);
        let ttl = std::time::Duration::from_secs(10);
        let archived = channel.expire(now + ttl, ttl).archived;

        assert!(!channel.discard_archived(999), "not that one");
        assert!(channel.snapshot().has_archived());
        assert!(channel.discard_archived(archived[0].draft.id));
        assert!(!channel.snapshot().has_archived());
    }

    #[test]
    fn an_empty_draft_is_never_raised() {
        let channel = ReviewChannel::new();
        assert_eq!(
            channel.raise(draft("   ", 0)),
            RaiseOutcome::NothingWorthOffering
        );
        assert!(channel.snapshot().is_empty());
    }

    /// At the budget the refusal is passed through **as a value**, so the loop
    /// can tell "there was nothing to decide about" from "the app is holding as
    /// much as it is willing to hold" — and the revision is not bumped, because
    /// nothing changed.
    #[test]
    fn a_full_store_refuses_new_text_without_pretending_anything_changed() {
        let channel = ReviewChannel::new();
        let now = std::time::Instant::now();
        for i in 0..DraftStore::RETENTION_CAP {
            let mut d = draft("متن", i as u64);
            d.raised = now;
            must_raise(&channel, d);
        }
        let before = channel.snapshot();
        assert_eq!(before.drafts.len(), DraftStore::RETENTION_CAP);

        let mut extra = draft("تازه", 9_999);
        extra.raised = now;
        assert_eq!(
            channel.raise(extra),
            RaiseOutcome::RefusedAtCapacity {
                held: DraftStore::RETENTION_CAP,
                cap: DraftStore::RETENTION_CAP,
            }
        );
        assert_eq!(
            channel.snapshot(),
            before,
            "a refused raise changes nothing at all"
        );
        assert!(channel.retention_is_full());
        assert_eq!(channel.retention_len(), DraftStore::RETENTION_CAP);
    }
}
