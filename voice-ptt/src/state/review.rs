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

/// One piece of finished text waiting on a decision.
///
/// `id` is what makes rule 2 above enforceable: the answer names the draft it
/// is about, so an answer that arrives after the draft was resolved, expired or
/// superseded cannot be applied to whatever happens to be pending now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDraft {
    pub id: u64,
    pub kind: DraftKind,
    /// The text as it would be typed. Immutable: what the user edits is a
    /// *command*, not a mutation of the draft, so the original stays available
    /// for the log and for "copy the original".
    pub text: String,
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
}

impl ReviewCommand {
    /// The draft this command is about.
    pub fn id(&self) -> u64 {
        match self {
            Self::Insert { id, .. } | Self::Copy { id } | Self::Cancel { id } => *id,
        }
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
#[derive(Debug, Default)]
pub struct DraftStore {
    next_id: u64,
    drafts: Vec<PendingDraft>,
}

impl DraftStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a draft up for a decision and returns it with the id it was given.
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
    ) -> Option<PendingDraft> {
        self.raise_from(PendingDraft {
            // Overwritten by `raise_from`, which owns numbering.
            id: 0,
            kind,
            text,
            destination,
            session,
            raised: now,
        })
    }

    /// Numbers and stores an already-built draft.
    ///
    /// Separate from [`Self::raise`] so a caller that has the whole draft in
    /// hand — the channel, which the coordinator fills in — does not have to
    /// take it apart and hand the pieces back. The id on `draft` is ignored:
    /// numbering belongs to the store alone, or two holders of it could both
    /// believe they minted the same id.
    pub fn raise_from(&mut self, mut draft: PendingDraft) -> Option<PendingDraft> {
        // A text nobody can act on is not raised at all. Handing back an id for
        // it would let the coordinator believe it had recorded something it
        // never offered the user.
        if !draft.is_worth_offering() {
            return None;
        }
        draft.id = self.next_id;
        self.next_id += 1;
        self.drafts.push(draft.clone());
        Some(draft)
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
        let Some(index) = self.drafts.iter().position(|d| d.id == command.id()) else {
            return ReviewOutcome::Stale;
        };
        let draft = self.drafts.remove(index);
        match command {
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

    /// Drops every draft older than `ttl`, returning what was dropped.
    ///
    /// Returns the texts so the caller can say what expired rather than letting
    /// them vanish: the roadmap is explicit that lost text is the failure this
    /// whole feature exists to prevent, and "it quietly expired" is the same
    /// outcome with none of the honesty.
    pub fn expire(&mut self, now: Instant, ttl: Duration) -> Vec<PendingDraft> {
        let mut expired = Vec::new();
        let mut kept = Vec::with_capacity(self.drafts.len());
        for draft in self.drafts.drain(..) {
            if now.saturating_duration_since(draft.raised) >= ttl {
                expired.push(draft);
            } else {
                kept.push(draft);
            }
        }
        self.drafts = kept;
        expired
    }

    /// Drops the drafts belonging to `session`, used when the user cancels a
    /// recording: text from a dictation they threw away must not sit in a
    /// window offering to be inserted later.
    pub fn forget_session(&mut self, session: crate::state::session::SessionId) -> usize {
        let before = self.drafts.len();
        self.drafts.retain(|d| d.session != Some(session));
        before - self.drafts.len()
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
        }
    }

    fn store_with_one(now: Instant) -> DraftStore {
        let mut s = DraftStore::new();
        s.raise(
            DraftKind::Undelivered,
            "سلام".into(),
            Some(target(7)),
            None,
            now,
        )
        .expect("a non-empty draft is raised");
        s
    }

    // ── offering ─────────────────────────────────────────────────────────

    #[test]
    fn an_empty_text_is_never_offered() {
        let mut s = DraftStore::new();
        for blank in ["", "   ", "\n\t "] {
            assert!(
                s.raise(DraftKind::Undelivered, blank.into(), None, None, Instant::now())
                    .is_none(),
                "{blank:?} cannot be inserted or usefully copied"
            );
        }
        assert!(s.is_empty(), "an unraised draft must leave nothing pending");
    }

    #[test]
    fn ids_are_unique_so_an_answer_cannot_hit_the_wrong_draft() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        let a = s
            .raise(DraftKind::Undelivered, "یک".into(), None, None, now)
            .unwrap();
        let b = s
            .raise(DraftKind::Undelivered, "دو".into(), None, None, now)
            .unwrap();
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
        assert_eq!(s.resolve(ReviewCommand::Cancel { id }), ReviewOutcome::Cancelled);
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
        let first = s
            .raise(DraftKind::Undelivered, "اول".into(), Some(target(1)), None, now)
            .unwrap();
        let second = s
            .raise(DraftKind::Undelivered, "دوم".into(), Some(target(1)), None, now)
            .unwrap();

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

        let expired = s.expire(now + DEFAULT_DRAFT_TTL - Duration::from_secs(1), DEFAULT_DRAFT_TTL);
        assert!(expired.is_empty(), "not due yet");

        let expired = s.expire(now + DEFAULT_DRAFT_TTL, DEFAULT_DRAFT_TTL);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].text, "سلام", "expiry must not lose the text");
        assert!(s.is_empty());

        // And an answer arriving after the deadline finds nothing.
        assert_eq!(
            s.resolve(ReviewCommand::Cancel {
                id: expired[0].id
            }),
            ReviewOutcome::Stale
        );
    }

    #[test]
    fn expiry_only_takes_what_is_due() {
        let now = Instant::now();
        let mut s = DraftStore::new();
        let old = s
            .raise(DraftKind::Undelivered, "قدیمی".into(), None, None, now)
            .unwrap();
        let new = s
            .raise(
                DraftKind::Undelivered,
                "جدید".into(),
                None,
                None,
                now + Duration::from_secs(30),
            )
            .unwrap();

        let expired = s.expire(now + Duration::from_secs(60), Duration::from_secs(45));
        assert_eq!(
            expired.iter().map(|d| d.id).collect::<Vec<_>>(),
            vec![old.id],
            "the fresh draft is not due and must survive"
        );
        assert_eq!(s.latest().unwrap().id, new.id);
    }

    /// Cancelling a recording must drop **its** drafts and only its: an older
    /// dictation's undelivered text is still the user's, and a cancel cannot be
    /// allowed to reach back and take it.
    #[test]
    fn cancelling_a_dictation_drops_only_its_own_drafts() {
        let now = Instant::now();
        let mine = crate::state::session::SessionId(4);
        let older = crate::state::session::SessionId(3);
        let mut s = DraftStore::new();
        s.raise(
            DraftKind::Undelivered,
            "قدیمی".into(),
            None,
            Some(older),
            now,
        )
        .unwrap();
        let doomed = s
            .raise(
                DraftKind::Undelivered,
                "لغو شده".into(),
                None,
                Some(mine),
                now,
            )
            .unwrap();

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