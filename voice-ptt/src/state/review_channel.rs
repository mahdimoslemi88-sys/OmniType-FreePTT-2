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

use super::review::{DraftStore, PendingDraft, ReviewCommand, ReviewOutcome};

/// Everything the dashboard needs to know about pending text, read once a frame.
///
/// `revision` is bumped on every change so a reader can tell "nothing pending"
/// from "something changed and I did not look". Without it a GUI that polls
/// would have to diff the whole list to notice one draft arriving.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewSnapshot {
    /// Oldest first — the order the user dictated in.
    pub drafts: Vec<PendingDraft>,
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

    /// Raises a draft for a decision and returns it, or `None` when there was
    /// nothing worth offering.
    pub fn raise(
        &self,
        draft: PendingDraft,
    ) -> Option<PendingDraft> {
        let raised = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .raise_from(draft);
        if raised.is_some() {
            self.bump();
        }
        raised
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

    /// Drops drafts whose deadline has passed, returning what was dropped.
    pub fn expire(&self, now: std::time::Instant, ttl: std::time::Duration) -> Vec<PendingDraft> {
        let expired = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .expire(now, ttl);
        if !expired.is_empty() {
            self.bump();
        }
        expired
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
        let drafts = self
            .drafts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending()
            .to_vec();
        let revision = *self.revision.borrow();
        ReviewSnapshot { drafts, revision }
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
        }
    }

    #[test]
    fn a_raised_draft_reaches_the_reader_with_a_bumped_revision() {
        let channel = ReviewChannel::new();
        let before = channel.snapshot();
        assert!(before.is_empty());

        assert!(channel.raise(draft("سلام", 0)).is_some());
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
        let raised = channel.raise(draft("سلام", 0)).unwrap();
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
        let raised = channel.raise(draft("سلام", 0)).unwrap();
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
        channel.raise(d);

        let ttl = std::time::Duration::from_secs(10);
        assert!(
            channel.expire(now + ttl - std::time::Duration::from_secs(1), ttl).is_empty(),
            "a draft that has not reached its deadline must still be there"
        );
        let expired = channel.expire(now + ttl, ttl);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].text, "سلام");
        assert!(channel.snapshot().is_empty());
    }

    #[test]
    fn an_empty_draft_is_never_raised() {
        let channel = ReviewChannel::new();
        assert!(channel.raise(draft("   ", 0)).is_none());
        assert!(channel.snapshot().is_empty());
    }
}