//! What the UI sees, and how the machine says it.
//!
//! `AppState`/`AppStatus` are the packet; the mutators below are the only
//! sanctioned ways to change it. Keeping them together is what makes the
//! channel's rules reviewable in one place — in particular that a mid-session
//! chunk must never flip the visible state (see `AppStatus::chunk_busy`).

use tokio::sync::watch;

/// Visible application state (also mirrored to the UI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Idle,
    Recording,
    Processing,
    Typing,
    Error(String),
}

/// External status packet emitted by the state machine on every transition.
#[derive(Debug, Clone)]
pub struct AppStatus {
    pub state: AppState,
    pub last_text: Option<String>,
    pub vad_engine: &'static str,
    /// Live partial transcript published by a streaming engine (Antigravity).
    /// Only populated while `state == Processing`; cleared when we leave it.
    pub partial: Option<String>,
    /// The application profile in force for the dictation being recorded, by
    /// name, or `None` when the general rules apply.
    ///
    /// Published once, when the microphone opens, from the *same* resolution the
    /// conversion will use — [`crate::state::coordinator::rules_for`] against the
    /// destination captured at that moment — so the orb can name the rules that
    /// are shaping the text the user is about to get. A profile is a property of
    /// the dictation, not of the instant, which is why this is decided at the
    /// start and carried rather than recomputed while the user talks.
    ///
    /// Cleared when the dictation ends (see [`StatusChannel::set_state`]).
    /// `None` draws nothing at all — a user with no profiles sees no new UI, and
    /// "عمومی" over every window would be noise on the orb.
    pub profile: Option<String>,
    /// True while the recording is latched hands-free (two quick presses of the
    /// record key). Only meaningful while `state == Recording`.
    pub latched: bool,
    /// True while a *mid-session* chunk is being transcribed and typed.
    ///
    /// phase 3.2: this used to be a `AppState::Processing` transition, which
    /// flipped the visible state (and therefore the orb's shape) on every chunk
    /// boundary. The user asked for the opposite: while a session is running the
    /// app should look like it is recording, and only a release/click should end
    /// it. The state now stays `Recording` and this flag carries the (invisible)
    /// "a chunk is in flight" fact for the UI.
    pub chunk_busy: bool,
}

/// The status channel, wrapped so the transition rules cannot be bypassed.
///
/// Every writer goes through a method here rather than reaching for
/// `watch::Sender` directly; that is the whole point of the newtype.
pub(crate) struct StatusChannel {
    tx: watch::Sender<AppStatus>,
}

impl StatusChannel {
    pub(crate) fn new(vad_engine: &'static str) -> Self {
        let (tx, _rx) = watch::channel(AppStatus {
            state: AppState::Idle,
            last_text: None,
            vad_engine,
            partial: None,
            profile: None,
            latched: false,
            chunk_busy: false,
        });
        Self { tx }
    }

    /// Subscribe to status updates (for the overlay/tray).
    pub fn subscribe(&self) -> watch::Receiver<AppStatus> {
        self.tx.subscribe()
    }

    /// Zero-copy read of the current status.
    ///
    /// Returns a borrow rather than an `AppStatus` on purpose: `AppState::Error`
    /// owns a `String`, so a caller that only wants to compare the state would
    /// allocate on every poll if we returned by value.
    pub(crate) fn snapshot(&self) -> watch::Ref<'_, AppStatus> {
        self.tx.borrow()
    }

    pub(crate) fn set_state(&self, state: AppState) {
        let _ = self.tx.send_if_modified(|s| {
            let mut changed = false;
            if s.state != state {
                s.state = state.clone();
                changed = true;
            }
            // Live partials belong to the processing phase only: dropping them
            // on the way out keeps a stale fragment from outliving its session.
            if s.partial.is_some() && !matches!(state, AppState::Processing) {
                s.partial = None;
                changed = true;
            }
            // The profile badge belongs to the dictation, not to the app: it
            // goes up when the microphone opens and comes down when that
            // dictation ends, so the next one cannot inherit the name of a
            // window the user has since left.
            if s.profile.is_some()
                && !matches!(
                    state,
                    AppState::Recording | AppState::Processing | AppState::Typing
                )
            {
                s.profile = None;
                changed = true;
            }
            changed
        });
        tracing::debug!(?state, "state");
    }

    /// Publishes a live partial transcript from a streaming engine
    /// (`asr::progress`). Ignored unless we are actually processing audio.
    pub fn publish_partial(&self, text: &str) {
        let _ = self.tx.send_if_modified(|s| {
            if s.state != AppState::Processing || s.partial.as_deref() == Some(text) {
                return false;
            }
            s.partial = Some(text.to_string());
            true
        });
    }

    /// Publishes the application profile in force for the recording that is
    /// starting.
    ///
    /// `None` is how a window with no profile announces itself, and it is not a
    /// label: the orb draws nothing rather than drawing "general".
    pub(crate) fn set_profile(&self, profile: Option<String>) {
        let _ = self.tx.send_if_modified(|s| {
            if s.profile == profile {
                return false;
            }
            s.profile = profile;
            true
        });
    }

    /// Publishes the hands-free latch state (double-tap mode) to the UI.
    pub(crate) fn set_latched(&self, latched: bool) {
        let _ = self.tx.send_if_modified(|s| {
            if s.latched == latched {
                return false;
            }
            s.latched = latched;
            true
        });
    }

    /// Marks a mid-session chunk as in flight. Publishes on the same watch
    /// channel as everything else, but never changes `state`: the orb must keep
    /// its recording shape while the microphone is live (see `AppStatus`).
    pub(crate) fn set_chunk_busy(&self, busy: bool) {
        let _ = self.tx.send_if_modified(|s| {
            if s.chunk_busy == busy {
                return false;
            }
            s.chunk_busy = busy;
            true
        });
    }

    pub(crate) fn set_last_text(&self, text: String) {
        let _ = self.tx.send_if_modified(|s| {
            s.last_text = Some(text);
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True when the receiver was notified since the last time it was updated.
    ///
    /// Every mutator is written to notify *only on change*
    /// (`send_if_modified` returning false), so "was it notified" is part of the
    /// contract, not an implementation detail: a redundant notify wakes the GUI
    /// for nothing.
    fn notified(rx: &mut watch::Receiver<AppStatus>) -> bool {
        if rx.has_changed().unwrap_or(false) {
            let _ = rx.borrow_and_update();
            true
        } else {
            false
        }
    }

    fn pair() -> (StatusChannel, watch::Receiver<AppStatus>) {
        let ch = StatusChannel::new("silero");
        let mut rx = ch.subscribe();
        // Mark the starting value as seen, so the first `notified` call answers
        // "did the first operation notify?" and not "has anything ever happened?".
        let _ = rx.borrow_and_update();
        (ch, rx)
    }

    #[test]
    fn a_new_channel_starts_idle_and_empty() {
        let (ch, _rx) = pair();
        let s = ch.snapshot();
        assert_eq!(s.state, AppState::Idle);
        assert_eq!(s.vad_engine, "silero");
        assert!(s.last_text.is_none() && s.partial.is_none());
        assert!(!s.latched && !s.chunk_busy);
    }

    #[test]
    fn a_repeated_state_does_not_notify() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Recording);
        assert!(notified(&mut rx), "the first transition must notify");
        ch.set_state(AppState::Recording);
        assert!(!notified(&mut rx), "the same state again must stay quiet");
    }

    /// Live partials belong to the processing phase only — a stale fragment
    /// must not outlive its session.
    #[test]
    fn a_partial_survives_processing_and_is_cleared_on_the_way_out() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Processing);
        assert!(notified(&mut rx));

        ch.publish_partial("سلام");
        assert!(notified(&mut rx));
        assert_eq!(ch.snapshot().partial.as_deref(), Some("سلام"));

        // Staying in Processing must not drop it.
        ch.set_state(AppState::Processing);
        assert!(!notified(&mut rx));
        assert_eq!(ch.snapshot().partial.as_deref(), Some("سلام"));

        ch.set_state(AppState::Typing);
        assert!(notified(&mut rx));
        assert_eq!(ch.snapshot().partial, None);
    }

    #[test]
    fn publish_partial_is_ignored_unless_processing() {
        let (ch, mut rx) = pair();
        ch.publish_partial("nonsense");
        assert_eq!(ch.snapshot().partial, None);
        assert!(
            !notified(&mut rx),
            "a partial with no Processing state must be dropped"
        );

        ch.set_state(AppState::Processing);
        let _ = rx.borrow_and_update();
        ch.publish_partial("real");
        assert_eq!(ch.snapshot().partial.as_deref(), Some("real"));
    }

    #[test]
    fn an_unchanged_partial_does_not_notify() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Processing);
        ch.publish_partial("same");
        assert!(notified(&mut rx));
        ch.publish_partial("same");
        assert!(
            !notified(&mut rx),
            "engines republish every chunk; only changes may notify"
        );
        ch.publish_partial("different");
        assert!(notified(&mut rx));
    }

    /// phase 3.2, asked for explicitly by the user: while a session runs the app
    /// must keep looking like it is recording, so a mid-session chunk may set
    /// `chunk_busy` but must never move `state`.
    #[test]
    fn chunk_busy_never_moves_the_visible_state() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Recording);
        let _ = rx.borrow_and_update();

        ch.set_chunk_busy(true);
        assert!(notified(&mut rx));
        assert_eq!(
            ch.snapshot().state,
            AppState::Recording,
            "the orb must keep its recording shape"
        );
        assert!(ch.snapshot().chunk_busy);

        ch.set_chunk_busy(true);
        assert!(!notified(&mut rx));
        ch.set_chunk_busy(false);
        assert!(notified(&mut rx));
        assert!(!ch.snapshot().chunk_busy);
    }

    /// The badge belongs to one dictation: it must survive the
    /// recording → processing → typing walk and be gone once the text is in.
    /// Clearing it any earlier would blink the label off mid-dictation; leaving
    /// it up would name the previous window during the next one.
    #[test]
    fn a_profile_survives_its_dictation_and_is_cleared_afterwards() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Recording);
        ch.set_profile(Some("کروم".into()));
        assert!(notified(&mut rx));
        assert_eq!(ch.snapshot().profile.as_deref(), Some("کروم"));

        // A mid-session chunk is the same dictation: the label must not
        // flicker on every chunk boundary.
        ch.set_chunk_busy(true);
        let _ = rx.borrow_and_update();
        assert_eq!(ch.snapshot().profile.as_deref(), Some("کروم"));

        ch.set_state(AppState::Processing);
        assert_eq!(ch.snapshot().profile.as_deref(), Some("کروم"));
        ch.set_state(AppState::Typing);
        assert_eq!(ch.snapshot().profile.as_deref(), Some("کروم"));

        ch.set_state(AppState::Idle);
        assert!(notified(&mut rx));
        assert_eq!(ch.snapshot().profile, None);
    }

    #[test]
    fn an_unchanged_profile_does_not_notify_and_an_error_clears_it() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Recording);
        ch.set_profile(Some("vim".into()));
        assert!(notified(&mut rx));
        ch.set_profile(Some("vim".into()));
        assert!(
            !notified(&mut rx),
            "the same profile again must stay quiet"
        );

        ch.set_state(AppState::Error("capture start failed".into()));
        assert!(notified(&mut rx));
        assert_eq!(
            ch.snapshot().profile,
            None,
            "a failed dictation must not leave a stale label on the orb"
        );
    }

    /// A window with no profile publishes `None`, which is not a badge: it must
    /// not wake the UI on its own.
    #[test]
    fn no_profile_is_silence_rather_than_a_label() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Recording);
        let _ = rx.borrow_and_update();
        ch.set_profile(None);
        assert!(!notified(&mut rx));
        assert_eq!(ch.snapshot().profile, None);
    }

    #[test]
    fn latched_and_last_text_round_trip() {
        let (ch, mut rx) = pair();
        ch.set_latched(true);
        assert!(notified(&mut rx));
        assert!(ch.snapshot().latched);
        ch.set_latched(true);
        assert!(!notified(&mut rx));

        ch.set_last_text("متن".into());
        assert!(notified(&mut rx));
        assert_eq!(ch.snapshot().last_text.as_deref(), Some("متن"));
    }

    #[test]
    fn a_subscriber_sees_later_transitions() {
        let (ch, mut rx) = pair();
        ch.set_state(AppState::Recording);
        assert!(notified(&mut rx));
        assert_eq!(rx.borrow().state, AppState::Recording);
    }
}
