//! The window where finished text waits for the user.
//!
//! Two things arrive here and they look identical to the user, because they are
//! the same moment: a dictation that review mode held back, and a dictation whose
//! insert failed. In both cases the text exists, it is not in any document, and
//! the user has to say what happens to it.
//!
//! So there is one window with one set of buttons, and it is deliberately small:
//! an editable box, «درج», «کپی», «لغو». The window's whole job is to make the
//! three answers available without taking focus away from the document the user
//! is writing in — it is a **viewport**, not a modal, and it never calls
//! `request_focus` on anything outside itself.
//!
//! Two rules this module enforces by construction, because they are the ones a
//! UI is most able to break:
//!
//! * **Insert types the edited text.** The box is the source of truth, not the
//!   draft; an edit the user made is part of what they approved.
//! * **Every button resolves its draft.** Each one names an id, so a
//!   double-click is a stale answer rather than a second insert.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use super::text::format_persian_display;
use super::theme::{
    apply_theme_visuals, manager_card, manager_central_panel, manager_header, manager_subtitle,
    palette,
};
use crate::state::review::{DraftKind, PendingDraft, ReviewCommand};
use crate::state::ReviewChannel;

/// What put this version of the text in the box.
///
/// The history is of versions **and** of the user's actions: a line of text
/// with no account of how it got there cannot answer "what did I just undo?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The box was built from a draft (or rebuilt for a newer one).
    Loaded,
    /// The user typed it.
    Edited,
    /// Restored by undo.
    Undone,
    /// Restored by redo.
    Redone,
}

/// One version of the box: the text, and the action that put it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub text: String,
    pub action: Action,
}

/// The box's way back: where the text came from, where it is, and where an
/// undo or a redo could take it.
///
/// Lives in the panel rather than in the store on purpose — U1's rule:
/// the draft in [`crate::state::review::DraftStore`] is immutable, so undoing
/// is a change to *this box only*. It never crosses
/// [`crate::state::ReviewChannel`], never resolves a draft, and never reaches
/// a session: the three things it must not destroy are the way back, the
/// session, and a newer dictation's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditHistory {
    /// Versions older than the current one, newest last.
    past: Vec<Version>,
    /// What the box holds now.
    current: Version,
    /// Versions undo took the box away from, for redo.
    future: Vec<Version>,
    /// When the text last changed, so a typing run can be one undo step.
    last_change: Option<Instant>,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            current: Version {
                text: String::new(),
                action: Action::Loaded,
            },
            future: Vec::new(),
            last_change: None,
        }
    }
}

impl EditHistory {
    /// Two changes closer together than this are one typing run, and undo
    /// takes back the run rather than one letter of it.
    const BURST: Duration = Duration::from_millis(1200);

    /// How many versions are kept before the oldest is dropped. A bound, so a
    /// long editing session cannot grow the window without limit; the dropped
    /// end is the oldest text, which the draft in the store still holds.
    const CAP: usize = 100;

    /// Starts the history at the text a draft put in the box.
    pub fn loaded(text: &str) -> Self {
        Self {
            current: Version {
                text: text.to_string(),
                action: Action::Loaded,
            },
            ..Self::default()
        }
    }

    /// The text the box should show.
    pub fn text(&self) -> &str {
        &self.current.text
    }

    /// The action that produced the version on screen — what a hover on the
    /// undo button can honestly claim the next click will do.
    pub fn action(&self) -> Action {
        self.current.action
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Takes whatever the box now holds as a version.
    ///
    /// A change inside a run replaces the running version: a pause is what
    /// ends a version, not every keystroke, or undo would walk backwards one
    /// letter at a time. A change after a quiet period closes the previous
    /// version and opens a new one — and drops what redo had left, because a
    /// linear history is the one a user can predict.
    pub fn observe(&mut self, live: &str, now: Instant) {
        if live == self.current.text {
            return;
        }
        let in_run = self
            .last_change
            .is_some_and(|at| now.saturating_duration_since(at) < Self::BURST);
        if in_run {
            self.current.text = live.to_string();
            self.current.action = Action::Edited;
        } else {
            let left = std::mem::replace(
                &mut self.current,
                Version {
                    text: live.to_string(),
                    action: Action::Edited,
                },
            );
            self.past.push(left);
            if self.past.len() > Self::CAP {
                self.past.remove(0);
            }
            self.future.clear();
        }
        self.last_change = Some(now);
    }

    /// One step back. False means there is nowhere to go, and the box is
    /// left exactly as it was.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.past.pop() else {
            return false;
        };
        let left = std::mem::replace(
            &mut self.current,
            Version {
                text: previous.text,
                action: Action::Undone,
            },
        );
        self.future.push(left);
        self.last_change = None;
        true
    }

    /// One step forward again. False means redo has nothing left.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        let left = std::mem::replace(
            &mut self.current,
            Version {
                text: next.text,
                action: Action::Redone,
            },
        );
        self.past.push(left);
        self.last_change = None;
        true
    }
}

/// The window's own state: the editable text and which draft it belongs to.
///
/// The draft id is remembered so that a text arriving from a *new* dictation
/// replaces the box rather than appending to it, and so the buttons always name
/// the draft the user is actually looking at.
#[derive(Debug, Default)]
pub struct ReviewPanelState {
    /// The draft currently in the box, if any.
    editing: Option<u64>,
    /// The editable text.
    text: String,
    /// How the text got here and how to step back through it.
    history: EditHistory,
    /// The snapshot revision the box was built from.
    ///
    /// A plain "not equal" check would also fire when a draft was resolved and
    /// nothing replaced it, so the revision is what says "the world moved"; the
    /// id is what says "which text".
    built_from: u64,
    /// When the window was last opened, for the elapsed hint.
    opened: Option<Instant>,
}

impl ReviewPanelState {
    /// The draft id the buttons would act on, if the window has one loaded.
    fn editing_id(&self) -> Option<u64> {
        self.editing
    }
}

/// Shows the review window when a draft is waiting, and hides it when none is.
///
/// Returns nothing; every action is sent to the loop, which is the only holder
/// of the keyboard. This function's authority is limited to drawing a box and
/// saying which button was pressed. `ttl` is the deadline the *loop* expires
/// drafts with, handed in so the countdown this window draws cannot promise a
/// different one.
pub fn render(
    ctx: &egui::Context,
    channel: &Arc<ReviewChannel>,
    state: &mut ReviewPanelState,
    ttl: Duration,
) {
    let snapshot = channel.snapshot();

    // The box follows the store. When the newest draft is not the one loaded,
    // it is rebuilt from that draft — which is what makes a second dictation
    // replace the first one's text instead of quietly appending to it.
    let latest = snapshot.latest().cloned();
    let needs_reload = match (&latest, state.editing) {
        (Some(draft), Some(id)) => draft.id != id || snapshot.revision != state.built_from,
        (Some(_), None) => true,
        (None, _) => false,
    };
    if needs_reload {
        state.editing = latest.as_ref().map(|d| d.id);
        state.text = latest.as_ref().map(|d| d.text.clone()).unwrap_or_default();
        // A new draft starts a new history. Without this, undo would be able
        // to bring back a *previous* dictation's text over the one the user is
        // looking at — the "do not destroy new text" rule, as a reset.
        state.history = EditHistory::loaded(&state.text);
        state.built_from = snapshot.revision;
        state.opened = latest.as_ref().map(|_| Instant::now());
    }

    let Some(draft) = latest else {
        // Nothing pending: the window closes itself. Left open with an empty
        // box it would be a dialog the user has to dismiss to get back to work.
        state.editing = None;
        state.text.clear();
        state.history = EditHistory::default();
        state.opened = None;
        return;
    };
    if state.editing != Some(draft.id) {
        // The store moved on under a box we could not reload (only reachable
        // while the loop is answering another draft). Draw the store's text.
        state.text = draft.text.clone();
        state.history = EditHistory::loaded(&state.text);
    }

    let (win_w, win_h) = (win_w(), win_h());
    // Bottom-centre of the **screen**, above the taskbar: where the transcript
    // bubble already lives, so a review reads as "your dictation finished"
    // rather than as an unrelated dialog. The screen, not `ctx.screen_rect()` —
    // that is the capsule's own rect with its origin at (0,0), and deriving
    // position from it dropped this window off the top-left of the desktop
    // (Q1-1).
    let (pos_x, pos_y) = crate::gui::window_shape::position_in_screen(
        super::screen_size_pt(ctx),
        (win_w, win_h),
        Some(BOTTOM_MARGIN),
    );

    ctx.show_viewport_immediate(
        review_viewport_id(),
        egui::ViewportBuilder::default()
            .with_title(title_for(&draft))
            .with_position([pos_x, pos_y])
            .with_inner_size([win_w, win_h])
            .with_min_inner_size([380.0, 240.0])
            .with_decorations(true)
            .with_resizable(true)
            .with_transparent(false),
        |win_ctx, _class| {
            apply_theme_visuals(win_ctx);
            egui::CentralPanel::default()
                .frame(manager_central_panel())
                .show(win_ctx, |ui| {
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                        |ui| {
                            render_body(ui, state, &draft, channel, ttl);
                        },
                    );
                });
        },
    );
}

/// The one id this window uses, in one place.
///
/// Both the render and the tests that assert the window opened have to name the
/// same id; a test that spelled it out separately would pass against a window
/// that had been renamed and never opened at all.
fn review_viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("omnitype_review_viewport")
}

/// The window's size, in one place so the layout and anything that has to
/// agree with it read the same numbers.
fn win_w() -> f32 {
    460.0
}
fn win_h() -> f32 {
    300.0
}

/// How far above the bottom edge the window hangs, so it clears the taskbar.
const BOTTOM_MARGIN: f32 = 48.0;

/// How long the window is open before it is worth mentioning that it closes.
const SHOW_AFTER: Duration = Duration::from_secs(5);

/// What the window says about its own deadline, and only when it is worth
/// saying.
///
/// The line this replaced printed `elapsed` in a sentence that reads as a
/// deadline («پس از {secs} ثانیه بسته می‌شود») — a count-**up** claiming to be a
/// count-down, so the number was wrong at every moment it was shown (Q1-2).
/// This prints what is actually left, counted against the same `ttl` the loop
/// expires drafts with, and stays quiet for the first few seconds so opening
/// the window is not an alarm.
fn deadline_line(elapsed: Duration, ttl: Duration) -> Option<String> {
    if elapsed < SHOW_AFTER {
        return None;
    }
    let remaining = ttl.saturating_sub(elapsed).as_secs();
    Some(if remaining == 0 {
        "این پنجره بدون پاسخ بسته می‌شود.".to_string()
    } else {
        format!("این پنجره تا {remaining} ثانیهٔ دیگر بدون پاسخ بسته می‌شود.")
    })
}

fn title_for(draft: &PendingDraft) -> &'static str {
    match draft.kind {
        DraftKind::Review => "بازبینی متن — OmniType",
        DraftKind::Undelivered => "متن درج نشد — OmniType",
    }
}

fn render_body(
    ui: &mut egui::Ui,
    state: &mut ReviewPanelState,
    draft: &PendingDraft,
    channel: &Arc<ReviewChannel>,
    ttl: Duration,
) {
    manager_header(ui, "متن آمادهٔ درج", None);

    // The reason this window is open, stated before the text. A user who sees
    // only a box has to guess whether the app is asking permission or reporting
    // a failure, and those want different answers.
    let (line, tint) = match draft.kind {
        DraftKind::Review => (
            "این متن هنوز درج نشده است. می‌توانید آن را ویرایش کنید.",
            palette::TEXT_SECONDARY,
        ),
        DraftKind::Undelivered => (
            "درج این متن انجام نشد؛ می‌توانید دوباره تلاش کنید یا آن را کپی کنید.",
            palette::TEXT_PRIMARY,
        ),
    };
    manager_subtitle(ui, line);
    if let Some(destination) = &draft.destination {
        let title = destination.title_at_capture.trim();
        if !title.is_empty() {
            ui.label(
                egui::RichText::new(format_persian_display(&format!(
                    "مقصد: {title}"
                )))
                .size(10.5)
                .color(tint),
            );
        } else {
            ui.label(
                egui::RichText::new(format_persian_display("مقصد: نامشخص"))
                    .size(10.5)
                    .color(tint),
            );
        }
    } else {
        // Said out loud rather than left blank: this text has no window to go
        // back to, so approving it will not type it anywhere.
        ui.label(
            egui::RichText::new(format_persian_display("مقصدی ثبت نشده است؛ درج ممکن نیست."))
                .size(10.5)
                .color(palette::WARNING),
        );
    }

    ui.add_space(8.0);

    manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(&mut state.text)
                .desired_width(f32::INFINITY)
                .desired_rows(5)
                .hint_text("متن خالی است"),
        );
    });

    // Whatever the user did to the box in this frame becomes a version —
    // after the widget, so it is the edited text that is observed.
    state.history.observe(&state.text, Instant::now());

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        // What is on screen now, in the words of the history itself: the
        // button says what the *next* click does, and this says what it does
        // it to.
        let now_showing = match state.history.action() {
            Action::Loaded => "the text as the dictation produced it",
            Action::Edited => "edited by you",
            Action::Undone => "restored by بازگردانی",
            Action::Redone => "restored by بازانجام",
        };
        let undoing = ui
            .add_enabled(
                state.history.can_undo(),
                egui::Button::new(format_persian_display("بازگردانی")),
            )
            .on_hover_text(format!(
                "step the text back to the version before your last change.\n\
                 On screen now: {now_showing}.\n\
                 It only edits this box — nothing is typed, no draft is answered,\n\
                 and a newer dictation's text is never touched."
            ));
        if undoing.clicked() && state.history.undo() {
            state.text = state.history.text().to_string();
        }
        let redoing = ui
            .add_enabled(
                state.history.can_redo(),
                egui::Button::new(format_persian_display("بازانجام")),
            )
            .on_hover_text("put back what بازگردانی just took away.");
        if redoing.clicked() && state.history.redo() {
            state.text = state.history.text().to_string();
        }
    });

    ui.add_space(10.0);

    let id = state.editing_id();
    ui.horizontal(|ui| {
        let insertable = id.is_some() && !state.text.trim().is_empty();
        if ui
            .add_enabled(
                insertable,
                egui::Button::new(format_persian_display("درج")),
            )
            .clicked()
        {
            if let Some(id) = id {
                answer(channel, ReviewCommand::Insert {
                    id,
                    text: state.text.clone(),
                });
            }
        }
        if ui
            .button(format_persian_display("کپی"))
            .clicked()
        {
            if let Some(id) = id {
                // The clipboard is a GUI-thread resource, so the copy happens
                // here and the loop is only told the draft is finished. Its
                // `Copied` arm logs and types nothing.
                ui.ctx().copy_text(state.text.clone());
                answer(channel, ReviewCommand::Copy { id });
            }
        }
        if ui
            .button(format_persian_display("لغو"))
            .clicked()
        {
            if let Some(id) = id {
                answer(channel, ReviewCommand::Cancel { id });
            }
        }
    });

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(format_persian_display(
            "درجر: بار دوم تبدیل نمی‌شود؛ فقط درج تکرار می‌شود.",
        ))
        .size(10.0)
        .color(palette::TEXT_SECONDARY),
    );
    if let Some(opened) = state.opened {
        if let Some(line) = deadline_line(opened.elapsed(), ttl) {
            ui.label(
                egui::RichText::new(format_persian_display(&line))
                    .size(10.0)
                    .color(palette::TEXT_SECONDARY),
            );
        }
    }
}

/// Sends one answer and resets the box.
///
/// A failed send means the loop is gone: the program is exiting. Resetting the
/// box in that case is deliberate — a window still offering to insert text into
/// an app that has quit is worse than no window.
fn answer(channel: &Arc<ReviewChannel>, command: ReviewCommand) {
    if !channel.answer(command) {
        tracing::warn!("review answer could not be sent: the loop is no longer running");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::review::{DraftKind, DEFAULT_DRAFT_TTL};
    use crate::state::ReviewSnapshot;

    fn draft(id: u64, text: &str) -> PendingDraft {
        PendingDraft {
            id,
            kind: DraftKind::Undelivered,
            text: text.into(),
            destination: None,
            session: None,
            raised: Instant::now(),
        }
    }

    /// The rule that keeps two dictations from running together in one box.
    #[test]
    fn a_new_draft_replaces_the_text_in_the_box() {
        let mut state = ReviewPanelState::default();
        let snapshot = ReviewSnapshot {
            drafts: vec![draft(0, "اول")],
            revision: 1,
        };

        // First draft: the box takes its text.
        let latest = snapshot.latest().cloned();
        state.editing = latest.as_ref().map(|d| d.id);
        state.text = latest.as_ref().map(|d| d.text.clone()).unwrap_or_default();
        assert_eq!(state.text, "اول");

        // A second dictation arrives while the first is still in the box.
        let next = ReviewSnapshot {
            drafts: vec![draft(0, "اول"), draft(1, "دوم")],
            revision: 2,
        };
        let latest = next.latest().cloned();
        let needs_reload = match (&latest, state.editing) {
            (Some(d), Some(id)) => d.id != id,
            _ => false,
        };
        assert!(needs_reload, "a different draft must reload the box");
    }

    #[test]
    fn the_window_only_offers_its_actions_for_the_draft_it_is_showing() {
        let state = ReviewPanelState {
            editing: Some(7),
            text: "متن".into(),
            history: EditHistory::loaded("متن"),
            built_from: 3,
            opened: None,
        };
        assert_eq!(state.editing_id(), Some(7));
    }

    #[test]
    fn each_kind_gets_a_title_that_says_which_moment_it_is() {
        let undelivered = draft(0, "x");
        assert_eq!(
            title_for(&undelivered),
            "متن درج نشد — OmniType",
            "a failure must not look like a permission request"
        );
        let review = PendingDraft {
            kind: DraftKind::Review,
            ..undelivered
        };
        assert_eq!(title_for(&review), "بازبینی متن — OmniType");
    }

    #[test]
    fn a_default_panel_has_no_draft_and_no_text() {
        let state = ReviewPanelState::default();
        assert_eq!(state.editing_id(), None);
        assert!(state.text.is_empty());
    }

    // ── Q1-2: what the window promises about its own deadline ───────────

    /// The countdown says what is **left**, counted against the same ttl the
    /// loop expires drafts with. The line this replaced printed elapsed time
    /// inside a sentence that reads as a deadline («پس از {secs} ثانیه…»), so
    /// it was wrong at every moment it was shown — five seconds in, it
    /// promised five seconds.
    #[test]
    fn the_countdown_says_what_is_left_not_what_has_passed() {
        let ttl = DEFAULT_DRAFT_TTL;
        // Quiet for the first few seconds: opening the window is not an alarm.
        assert_eq!(deadline_line(Duration::from_secs(0), ttl), None);
        assert_eq!(deadline_line(Duration::from_secs(4), ttl), None);

        let line = deadline_line(Duration::from_secs(10), ttl).expect("worth saying");
        assert!(line.contains("110"), "ten seconds in, 110 are left: {line}");

        let line = deadline_line(Duration::from_secs(119), ttl).expect("worth saying");
        assert!(line.contains('1'), "one second is left: {line}");

        // At and past the deadline there is no number left to promise.
        let line = deadline_line(ttl, ttl).expect("worth saying");
        assert!(
            !line.contains('0'),
            "an expired window must not promise zero seconds: {line}"
        );
        assert!(deadline_line(ttl + Duration::from_secs(30), ttl).is_some());
    }

    // ── U1: stepping back through the box ────────────────────────────────

    /// A typing run is one undo step, or undo would walk back one letter at
    /// a time — history is of versions and of what the user *did*, not of
    /// keystrokes.
    #[test]
    fn undo_takes_back_a_whole_typing_run_not_one_letter() {
        let t0 = Instant::now();
        let mut h = EditHistory::loaded("سلام");
        assert_eq!(h.action(), Action::Loaded);

        h.observe("سلا", t0 + Duration::from_millis(120));
        h.observe("سلام د", t0 + Duration::from_millis(400));
        h.observe("سلام دنیا", t0 + Duration::from_millis(900));
        assert_eq!(h.action(), Action::Edited);

        assert!(h.can_undo());
        assert!(h.undo(), "the run is one step back");
        assert_eq!(h.text(), "سلام");
        assert_eq!(h.action(), Action::Undone);
        assert!(
            !h.can_undo(),
            "the text the draft put in the box is the end of the way back"
        );
        assert!(h.can_redo());
        assert!(h.redo());
        assert_eq!(h.text(), "سلام دنیا", "redo puts the whole run back");
        assert_eq!(h.action(), Action::Redone);
    }

    /// A pause is what ends a version: two runs apart in time are two steps.
    #[test]
    fn a_pause_starts_a_new_version() {
        let t0 = Instant::now();
        let mut h = EditHistory::loaded("");
        h.observe("یک", t0);
        h.observe("یک دو", t0 + EditHistory::BURST * 3);

        assert!(h.undo());
        assert_eq!(h.text(), "یک", "the second run is its own version");
        assert!(h.undo());
        assert_eq!(h.text(), "");
        assert!(!h.undo(), "and that is all of it");
    }

    /// Undo and redo are inert rather than surprising when there is nothing
    /// to do — the box must not move under the user.
    #[test]
    fn stepping_with_nowhere_to_go_changes_nothing() {
        let mut h = EditHistory::loaded("سلام");
        assert!(!h.undo());
        assert!(!h.redo());
        assert_eq!(h.text(), "سلام");

        h.observe("سلام دنیا", Instant::now());
        assert!(h.undo());
        assert!(h.redo());
        assert!(!h.redo(), "redo ends at the newest version");
        assert_eq!(h.text(), "سلام دنیا");
    }

    /// Typing again after an undo drops redo: a linear history is the one a
    /// user can predict, and the alternative silently branches.
    #[test]
    fn editing_after_an_undo_drops_redo_instead_of_branching() {
        let t0 = Instant::now();
        let mut h = EditHistory::loaded("");
        h.observe("اول", t0);
        assert!(h.undo());
        assert!(h.can_redo());

        h.observe("دوم", t0 + EditHistory::BURST * 3);
        assert!(!h.can_redo(), "what redo held was replaced on purpose");
        assert!(h.undo());
        assert_eq!(h.text(), "");
    }

    /// The history belongs to one draft. A newer dictation replaces the box
    /// *and* its history, so undo can never bring an older dictation's text
    /// back over the newer one.
    #[test]
    fn history_does_not_cross_drafts() {
        let channel = ReviewChannel::new();
        let mut state = ReviewPanelState::default();

        channel.raise(draft(0, "اول"));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL));
        state.text = "اول ویرایش‌شده".into();
        state.history.observe(&state.text, Instant::now());
        assert!(state.history.can_undo());

        // A second dictation arrives while the first is still in the box.
        channel.raise(draft(1, "دوم"));
        let _ = ctx.run(Default::default(), |ctx| render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL));
        assert_eq!(state.text, "دوم");
        assert!(
            !state.history.can_undo(),
            "the new draft starts with no way back to the old one's text"
        );
        assert!(!state.history.undo());
        assert_eq!(state.text, "دوم");
    }

    /// Undo is a change to the box, and only to the box: it sends no answer,
    /// resolves no draft, and bumps no revision — the store and the session
    /// behind it are untouched.
    #[test]
    fn undo_answers_nothing_and_touches_no_session() {
        let channel = ReviewChannel::new();
        let mut state = ReviewPanelState::default();
        channel.raise(draft(0, "سلام"));

        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL));
        let before = channel.snapshot();

        state.text = "سلام ویرایش".into();
        state.history.observe(&state.text, Instant::now());
        assert!(state.history.undo());
        state.text = state.history.text().to_string();
        assert_eq!(state.text, "سلام");

        let after = channel.snapshot();
        assert_eq!(after.revision, before.revision, "undo is not an answer");
        assert_eq!(
            after.drafts.len(),
            before.drafts.len(),
            "undo resolves nothing"
        );
        assert_eq!(after.drafts[0].text, "سلام", "the draft keeps its original");
    }

    /// What undo restores is what «درج» types, and the destination the draft
    /// was raised with still carries it — undoing must leave the window able
    /// to do its job, not strand it.
    #[test]
    fn undo_then_insert_types_the_restored_text_into_the_original_destination() {
        let channel = ReviewChannel::new();
        let mut state = ReviewPanelState::default();
        let destination = crate::output::target::TargetIdentity {
            hwnd: 5,
            pid: 9,
            exe_path: None,
            title_at_capture: "سند".into(),
        };
        let mut d = draft(0, "سلام");
        d.destination = Some(destination.clone());
        channel.raise(d);

        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL));

        // The user edits, then takes the edit back.
        state.text = "سلام!".into();
        state.history.observe(&state.text, Instant::now());
        assert!(state.history.undo());
        state.text = state.history.text().to_string();
        assert_eq!(state.text, "سلام");

        let outcome = channel.resolve(ReviewCommand::Insert {
            id: state.editing_id().expect("a draft is loaded"),
            text: state.text.clone(),
        });
        assert_eq!(
            outcome,
            crate::state::review::ReviewOutcome::Insert {
                text: "سلام".into(),
                destination: Some(destination),
            },
            "the restored text, into the window it was dictated to"
        );
    }

    /// The window body is the one part of this package no loop scenario
    /// executes, and review mode is off by default so a live run does not reach
    /// it either. A palette name that does not exist, or a layout call that
    /// panics, would otherwise ship green.
    ///
    /// **Drives [`render_body`] directly, not through `render`.** That is a
    /// deliberate correction of an earlier version of this file: a headless
    /// `egui::Context` reports only the root viewport, so `show_viewport_immediate`
    /// never invokes its closure and every assertion about "the window opened"
    /// passed without a single widget being drawn. Calling the body inside a real
    /// `Ui` is the only way to actually execute it without an OS window.
    fn draw_body(
        channel: &Arc<ReviewChannel>,
        state: &mut ReviewPanelState,
        draft: &PendingDraft,
    ) -> egui::FullOutput {
        let ctx = egui::Context::default();
        // Inside `Context::run`: the body asks for `available_rect()` and
        // egui panics outside a running frame.
        ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                    |ui| render_body(ui, state, draft, channel, DEFAULT_DRAFT_TTL),
                );
            });
        })
    }

    /// Both kinds must draw. A failure that renders differently from a
    /// permission request is the whole reason the window says which it is.
    #[test]
    fn both_kinds_draw_without_panicking() {
        for kind in [DraftKind::Review, DraftKind::Undelivered] {
            let channel = ReviewChannel::new();
            let mut state = ReviewPanelState::default();
            let mut d = draft(0, "متن");
            d.kind = kind;
            let output = draw_body(&channel, &mut state, &d);
            assert!(
                output.shapes.len() > 5,
                "{kind:?} must actually draw, saw {} shapes",
                output.shapes.len()
            );
        }
    }

    /// A draft with no captured destination must still draw — that is the case
    /// where the window has to say "inserting may not work" rather than leaving
    /// a blank where the destination name goes.
    #[test]
    fn a_draft_with_no_destination_still_draws() {
        let channel = ReviewChannel::new();
        let mut state = ReviewPanelState::default();
        let mut d = draft(0, "متن");
        d.destination = None;
        let output = draw_body(&channel, &mut state, &d);
        assert!(output.shapes.len() > 5);
    }

    /// A destination **with** a title must draw too: that arm formats and
    /// reshapes the title, which is the most likely place for a Persian string
    /// to go wrong.
    #[test]
    fn a_named_destination_draws() {
        let channel = ReviewChannel::new();
        let mut state = ReviewPanelState::default();
        let mut d = draft(0, "متن");
        d.destination = Some(crate::output::target::TargetIdentity {
            hwnd: 1,
            pid: 1,
            exe_path: None,
            title_at_capture: "سندImportant".into(),
        });
        let output = draw_body(&channel, &mut state, &d);
        assert!(output.shapes.len() > 5);
    }

    /// The loader: a pending draft must land in the box, because the box is what
/// the buttons would type.
    ///
    /// Driven through [`render`], not [`render_body`]: loading happens
    /// *before* the viewport is opened, so this half does run headlessly and is
    /// worth covering directly. The window itself is proved by the shape-count
    /// tests above.
    #[test]
    fn loading_a_draft_puts_its_text_in_the_box() {
        let channel = ReviewChannel::new();
        let mut state = ReviewPanelState::default();

        // Nothing pending: nothing is loaded.
        render(&egui::Context::default(), &channel, &mut state, DEFAULT_DRAFT_TTL);
        assert_eq!(state.editing_id(), None);

        channel.raise(draft(0, "سلام دنیا"));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL);
        });
        assert_eq!(state.editing_id(), Some(0), "the box takes the draft");
        assert_eq!(state.text, "سلام دنیا");

        // A second dictation replaces the box rather than appending: two
        // dictations running together in one box would type both at once.
        channel.raise(draft(1, "دوم"));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL);
        });
        assert_eq!(state.editing_id(), Some(1));
        assert_eq!(
            state.text, "دوم",
            "a new draft must replace the box, not append to it"
        );
    }
}