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

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use super::text::{format_persian_display, persian_text_edit_layouter};
use super::theme::{
    apply_theme_visuals, manager_card, manager_central_panel, manager_header, manager_subtitle,
    palette,
};
use crate::state::review::{ArchivedDraft, DraftKind, PendingDraft, ReviewCommand};
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
    /// Recovery is opened by the user, rather than interrupting every failed insert.
    recovery_requested: bool,
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
    /// Pending drafts whose window the user closed.
    ///
    /// Closing is **not** an answer: the draft keeps its deadline and its text,
    /// and it is offered again from the archive once it expires. What closing
    /// stops is this window coming back for the same text — the build the user
    /// tested had no way out of it at all, because a closed window was simply
    /// drawn again on the next frame.
    closed_pending: BTreeSet<u64>,
    /// Archived texts the user has been shown and closed the window for.
    ///
    /// Kept apart from [`Self::closed_pending`] on purpose: a pending text the
    /// user closed *is* worth mentioning once it becomes an archived one (the
    /// text is now kept for good), while an archived text the user has already
    /// seen and dismissed is not worth re-opening for.
    closed_archive: BTreeSet<u64>,
    /// An answer waiting for this window to get out of the way, with the moment
    /// it may go out.
    ///
    /// Set by a click on «درج» and by nothing else — see [`Self::defer_insert`].
    pending_answer: Option<DeferredAnswer>,
}

/// How long this window stays out of the way before the insert is asked for.
///
/// The window has to be *gone* when the loop judges the destination: the desktop
/// hands the foreground back to the window that had it before this one, and that
/// hand-back is what the check needs to see. Four or five frames is already
/// enough in practice; a fifth of a second costs the user nothing they can
/// perceive and covers the frames the OS needs to settle.
const STEP_ASIDE: Duration = Duration::from_millis(200);

/// One answer that has to wait for the window to move first.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredAnswer {
    command: ReviewCommand,
    send_at: Instant,
}

/// How many closed ids are remembered before the oldest is forgotten.
///
/// A bound, so a session that closes hundreds of texts cannot grow this window's
/// state without limit. Forgetting the oldest can at worst re-open the window for
/// a text the user closed long ago — which is the safe direction to fail in.
const CLOSED_MEMORY: usize = 128;

impl ReviewPanelState {
    pub fn open_recovery(&mut self) {
        self.recovery_requested = true;
        self.closed_pending.clear();
        self.closed_archive.clear();
    }
    /// The draft id the buttons would act on, if the window has one loaded.
    fn editing_id(&self) -> Option<u64> {
        self.editing
    }

    /// Remembers that the user closed the window for this pending draft.
    fn close_pending(&mut self, id: u64) {
        forget_oldest(&mut self.closed_pending, CLOSED_MEMORY);
        self.closed_pending.insert(id);
    }

    /// A click on «درج»: gets this window out of the way first.
    ///
    /// The command is **not** sent on the click. Pressing a button in the review
    /// window makes that window the one in front, and the loop judges the
    /// destination against the window in front — so an insert asked for from here
    /// was refused every single time, which is the user's "the insert button does
    /// not work, only copy does" (their log: four attempts, four
    /// `validity=Changed`, the same text re-offered each time). Closing the
    /// window for `STEP_ASIDE` avoids competing with activation. The coordinator
    /// explicitly restores the original destination and verifies it; this delay
    /// itself is not evidence that the destination has focus.
    fn defer_insert(&mut self, id: u64, text: String, now: Instant) {
        self.pending_answer = Some(DeferredAnswer {
            command: ReviewCommand::Insert { id, text },
            send_at: now + STEP_ASIDE,
        });
    }

    /// The answer that is due now, if any. `None` means "not yet" — the caller
    /// keeps drawing no window and asking for another frame.
    fn take_due_answer(&mut self, now: Instant) -> Option<ReviewCommand> {
        let pending = self.pending_answer.as_ref()?;
        if now < pending.send_at {
            return None;
        }
        self.pending_answer.take().map(|held| held.command)
    }

    /// Remembers that the user closed the archive window.
    ///
    /// Every text on screen at that moment counts as seen, so the window does not
    /// simply re-open on the next frame for the same set. A text archived *after*
    /// this — the pending draft that expires later, or another dictation — is not
    /// in the set and does open the window, which is how "your text was kept"
    /// still reaches the user.
    fn close_archive(&mut self, archived: &[ArchivedDraft]) {
        for item in archived {
            forget_oldest(&mut self.closed_archive, CLOSED_MEMORY);
            self.closed_archive.insert(item.draft.id);
        }
    }
}

/// Drops the smallest id once `set` is at its bound.
///
/// Ids are handed out in increasing order, so the smallest is the oldest and the
/// least likely to be closed on purpose.
fn forget_oldest(set: &mut BTreeSet<u64>, bound: usize) {
    while set.len() >= bound {
        let Some(oldest) = set.iter().next().copied() else {
            return;
        };
        set.remove(&oldest);
    }
}

/// Which window this panel draws, and for what.
///
/// Pure, so the rule the user experiences ("close it and it stays closed, until
/// there is something new to say") is a table that can be read and tested
/// without a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowPlan {
    /// The newest draft, waiting for an answer.
    Pending(u64),
    /// Text that expired uninserted and is kept until the user decides.
    Archive,
    /// Nothing to say — or the user has already closed the window for what is
    /// there.
    Hidden,
}

/// The window this frame draws, which is [`plan_window`] except for one
/// deliberate override: **nothing** is drawn while an answer is waiting for this
/// window to get out of the way. Drawing it would put it back in front — the
/// exact window the loop must not see when it judges the destination.
fn plan_for_frame(
    state: &ReviewPanelState,
    latest: Option<&PendingDraft>,
    archived: &[ArchivedDraft],
) -> WindowPlan {
    if state.pending_answer.is_some() {
        return WindowPlan::Hidden;
    }
    if !state.recovery_requested && !latest.is_some_and(|draft| draft.kind == DraftKind::Review) {
        return WindowPlan::Hidden;
    }
    plan_window(
        latest,
        archived,
        &state.closed_pending,
        &state.closed_archive,
    )
}

fn plan_window(
    latest: Option<&PendingDraft>,
    archived: &[ArchivedDraft],
    closed_pending: &BTreeSet<u64>,
    closed_archive: &BTreeSet<u64>,
) -> WindowPlan {
    if let Some(draft) = latest {
        return if closed_pending.contains(&draft.id) {
            WindowPlan::Hidden
        } else {
            WindowPlan::Pending(draft.id)
        };
    }
    if archived
        .iter()
        .any(|item| !closed_archive.contains(&item.draft.id))
    {
        return WindowPlan::Archive;
    }
    WindowPlan::Hidden
}

/// What the user did to the window itself, as opposed to to a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowClosed {
    /// The × (or Alt+F4) on the text waiting for an answer.
    Pending(u64),
    /// The × (or Alt+F4) on the archive.
    Archive,
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
    let latest = snapshot.latest().cloned();

    // An insert that asked this window to get out of the way first: it goes out
    // as soon as the wait is over. `request_repaint_after` is what makes the wait
    // a delay rather than a stall — an event-driven GUI has no reason to draw
    // another frame while there is nothing on screen, and the answer would then
    // sit in here until the user moved the mouse.
    let now = Instant::now();
    if let Some(command) = state.take_due_answer(now) {
        answer(channel, command);
    } else if state.pending_answer.is_some() {
        ctx.request_repaint_after(Duration::from_millis(25));
    }

    let plan = plan_for_frame(state, latest.as_ref(), &snapshot.archived);
    // Filled in by whichever viewport is drawn, then applied below: the reply to
    // "the user clicked ×" cannot be decided inside the frame that is closing,
    // or the window would be closed again for a draft that has not been seen.
    let mut closed = None;

    match plan {
        WindowPlan::Pending(_) => {
            let draft = latest.expect("a pending plan without a draft is impossible");
            render_pending(ctx, channel, state, &draft, &snapshot, ttl, &mut closed);
        }
        // Nothing on screen. If expired text is still available the window shows
        // **all** of it — never only the newest entry, which would make the rest
        // reachable only by deleting it. It is drawn while there is an entry the
        // user has not closed the window for, so "×" ends it and a newly kept
        // text brings it back once.
        WindowPlan::Archive => render_archive_window(ctx, &snapshot.archived, channel, &mut closed),
        WindowPlan::Hidden => {}
    }

    match closed {
        Some(WindowClosed::Pending(id)) => {
            state.recovery_requested = false;
            tracing::info!(
                id,
                "review window closed by the user; the text keeps its deadline"
            );
            state.close_pending(id);
        }
        Some(WindowClosed::Archive) => {
            state.recovery_requested = false;
            tracing::info!(
                kept = snapshot.archived.len(),
                "archive window closed by the user"
            );
            state.close_archive(&snapshot.archived);
        }
        None => {}
    }

    if matches!(plan, WindowPlan::Hidden) && state.pending_answer.is_none() {
        // The box belongs to the window. With nothing on screen it starts empty,
        // so a text the user closed the window for cannot appear in the box of
        // the next draft without passing through the store again. Not while an
        // answer is in flight, though: that window is out of the way, not done —
        // the text in the box is what the loop is about to type.
        state.editing = None;
        state.text.clear();
        state.history = EditHistory::default();
        state.opened = None;
    }
}

/// Draws the window for the newest draft.
#[allow(clippy::too_many_arguments)]
fn render_pending(
    ctx: &egui::Context,
    channel: &Arc<ReviewChannel>,
    state: &mut ReviewPanelState,
    draft: &PendingDraft,
    snapshot: &crate::state::ReviewSnapshot,
    ttl: Duration,
    closed: &mut Option<WindowClosed>,
) {
    // The box follows the store. When the newest draft is not the one loaded,
    // it is rebuilt from that draft — which is what makes a second dictation
    // replace the first one's text instead of quietly appending to it.
    let needs_reload = match state.editing {
        Some(id) => draft.id != id || snapshot.revision != state.built_from,
        None => true,
    };
    if needs_reload {
        state.editing = Some(draft.id);
        state.text = draft.text.clone();
        // A new draft starts a new history. Without this, undo would be able
        // to bring back a *previous* dictation's text over the one the user is
        // looking at — the "do not destroy new text" rule, as a reset.
        state.history = EditHistory::loaded(&state.text);
        state.built_from = snapshot.revision;
        state.opened = Some(Instant::now());
    }
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

    let id = draft.id;
    ctx.show_viewport_immediate(
        review_viewport_id(),
        egui::ViewportBuilder::default()
            .with_title(title_for(draft))
            .with_position([pos_x, pos_y])
            .with_inner_size([win_w, win_h])
            .with_min_inner_size([380.0, 240.0])
            .with_decorations(true)
            .with_resizable(true)
            .with_transparent(false),
        |win_ctx, _class| {
            if win_ctx.input(|i| i.viewport().close_requested()) {
                // Answering "the window is going away" *here* rather than
                // ignoring it is the whole fix for the window that could not be
                // closed: this viewport is re-created every frame while a draft
                // exists, so a close request nobody acts on is a close button
                // that does nothing.
                if closed.is_none() {
                    *closed = Some(WindowClosed::Pending(id));
                }
            }
            apply_theme_visuals(win_ctx);
            egui::CentralPanel::default()
                .frame(manager_central_panel())
                .show(win_ctx, |ui| {
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                        |ui| {
                            render_body(ui, state, draft, channel, ttl);
                        },
                    );
                });
        },
    );
}

/// The window for **expired** text: everything the app is still holding that it
/// will no longer insert on its own.
///
/// It lists the **whole** archive, newest first, rather than only the newest
/// entry. Showing one at a time made the older texts reachable only by deleting
/// the newer ones — deleting the user's text in order to reach their text, which
/// is the transaction this archive exists to avoid (F3). Here every kept text can
/// be read, copied, restored or deleted on its own, and none of those requires
/// touching another one.
///
/// Four things it deliberately is not:
///
/// * it is not editable — a text that is not answerable must not look like one
///   click away from being typed;
/// * «کپی» does **not** send a command: copying archived text is a pure GUI
///   action and must not retire the draft. That is the difference between the
///   archive and the pending window, where copying is an answer;
/// * «حذف از آرشیو» is the only control that deletes anything, and it names the
///   one text it deletes, so no misclick can take another;
/// * nothing is deleted **for** the user. Reaching the cap costs new text, never
///   old text: the store enforces that, and this window is where the user sees it
///   and can act on it.
fn render_archive_window(
    ctx: &egui::Context,
    archived: &[ArchivedDraft],
    channel: &Arc<ReviewChannel>,
    closed: &mut Option<WindowClosed>,
) {
    let (win_w, win_h) = (win_w(), win_h());
    let (pos_x, pos_y) = crate::gui::window_shape::position_in_screen(
        super::screen_size_pt(ctx),
        (win_w, win_h),
        Some(BOTTOM_MARGIN),
    );
    ctx.show_viewport_immediate(
        review_viewport_id(),
        egui::ViewportBuilder::default()
            .with_title(ARCHIVE_TITLE)
            .with_position([pos_x, pos_y])
            .with_inner_size([win_w, win_h])
            .with_min_inner_size([380.0, 240.0])
            .with_decorations(true)
            .with_resizable(true)
            .with_transparent(false),
        |win_ctx, _class| {
            if win_ctx.input(|i| i.viewport().close_requested()) && closed.is_none() {
                // Closing the archive is a real answer too, and it must stick:
                // "every kept text keeps this window open forever" was the other
                // half of the window that could not be closed. Nothing is deleted
                // and nothing is answered — the texts stay kept, and the next one
                // that expires brings the window back once.
                *closed = Some(WindowClosed::Archive);
            }
            apply_theme_visuals(win_ctx);
            egui::CentralPanel::default()
                .frame(manager_central_panel())
                .show(win_ctx, |ui| {
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                        |ui| render_archive_body(ui, archived, channel),
                    );
                });
        },
    );
}

/// The archive window's title, in one place: a window renamed here and asserted
/// somewhere else is a window nobody proved was opened.
const ARCHIVE_TITLE: &str = "متن‌های درج‌نشدهٔ بازمانده — OmniType";

fn render_archive_body(
    ui: &mut egui::Ui,
    archived: &[ArchivedDraft],
    channel: &Arc<ReviewChannel>,
) {
    manager_header(ui, "متن‌های درج‌نشدهٔ بازمانده", None);
    manager_subtitle(
        ui,
        &format!(
            "مهلت درج این متن‌ها تمام شده و مستقیم درج نمی‌شوند؛ {} متن نگه داشته شده و هیچ‌یک حذف نخواهد شد.",
            archived.len()
        ),
    );
    ui.add_space(6.0);

    // The list scrolls, so a full budget is browsable in one window: reaching an
    // older text never requires giving up a newer one.
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Newest first: the text the user has just lost is the one they are
            // most likely looking for.
            for item in archived.iter().rev() {
                render_archived_item(ui, item, channel);
                ui.add_space(8.0);
            }
        });
}

/// One kept text: readable, copyable, restorable and deletable — on its own.
fn render_archived_item(ui: &mut egui::Ui, archived: &ArchivedDraft, channel: &Arc<ReviewChannel>) {
    let id = archived.draft.id;
    manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
        match &archived.draft.destination {
            Some(destination) => {
                let title = destination.title_at_capture.trim();
                let line = if title.is_empty() {
                    "مقصد: نامشخص".to_string()
                } else {
                    format!("مقصد: {title}")
                };
                ui.label(
                    egui::RichText::new(format_persian_display(&line))
                        .size(10.5)
                        .color(palette::TEXT_SECONDARY),
                );
            }
            None => {
                ui.label(
                    egui::RichText::new(format_persian_display(
                        "مقصدی ثبت نشده است؛ برای درج باید مقصد دوباره بررسی شود.",
                    ))
                    .size(10.5)
                    .color(palette::WARNING),
                );
            }
        }

        // Read-only. `interactive(false)` rather than a disabled `TextEdit` so
        // the text is still selectable for the user's own copy, and so it cannot
        // be mistaken for an editable box. Laid out through the Persian shaper
        // for the same reason the pending box is: an unshaped `TextEdit` draws
        // Persian unconnected and in reading order left-to-right.
        let mut shown = archived.draft.text.clone();
        let mut layouter =
            |ui: &egui::Ui, text: &str, wrap: f32| persian_text_edit_layouter(ui, text, wrap);
        ui.add(
            egui::TextEdit::multiline(&mut shown)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .layouter(&mut layouter)
                .interactive(false),
        );

        ui.horizontal(|ui| {
            if ui
                .button(format_persian_display("بازگردانی برای بررسی"))
                .on_hover_text(
                    "bring this text back as a new draft. It gets a fresh deadline,\n\
                     and inserting it re-checks the destination first.",
                )
                .clicked()
            {
                answer(channel, ReviewCommand::Restore { id });
            }

            if ui
                .button(format_persian_display("کپی"))
                .on_hover_text("copy only: nothing is typed and nothing is deleted.")
                .clicked()
            {
                // Pure GUI action: the archive keeps the text.
                ui.ctx().copy_text(archived.draft.text.clone());
            }

            if ui
                .button(format_persian_display("حذف از آرشیو"))
                .on_hover_text("delete exactly this text, at your request.")
                .clicked()
            {
                answer(channel, ReviewCommand::DiscardArchived { id });
            }
        });

        let mut footer = format!("شناسهٔ متن: {id}");
        if let Some(session) = archived.draft.session {
            footer.push_str(&format!(" — از دیکتهٔ شمارهٔ {}", session.0));
        }
        ui.label(
            egui::RichText::new(format_persian_display(&footer))
                .size(9.5)
                .color(palette::TEXT_FAINT),
        );
    });
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
                egui::RichText::new(format_persian_display(&format!("مقصد: {title}")))
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
        // **The box is laid out through the Persian shaper.** It used to be a
        // plain `TextEdit`, which egui draws with no shaping and no direction:
        // the letters came out unconnected and in reading order left-to-right,
        // which for Persian is exactly the "completely reversed and unreadable"
        // the user reported. The buffer itself still holds the keystrokes — only
        // the drawing is shaped.
        let mut layouter =
            |ui: &egui::Ui, text: &str, wrap: f32| persian_text_edit_layouter(ui, text, wrap);
        ui.add(
            egui::TextEdit::multiline(&mut state.text)
                .desired_width(f32::INFINITY)
                .desired_rows(5)
                .layouter(&mut layouter)
                .hint_text(format_persian_display("متن خالی است")),
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
            .add_enabled(insertable, egui::Button::new(format_persian_display("درج")))
            .on_hover_text(
                "type this text into the window it was dictated into.\n\
                 This window steps out of the way first, so the text goes to your\n\
                 document instead of here.",
            )
            .clicked()
        {
            if let Some(id) = id {
                // Deferred, never sent from here: see `defer_insert`. This window
                // is the one in front while the button is being clicked, and it is
                // the one window that can never be the destination.
                state.defer_insert(id, state.text.clone(), Instant::now());
            }
        }
        if ui.button(format_persian_display("کپی")).clicked() {
            if let Some(id) = id {
                // The clipboard is a GUI-thread resource, so the copy happens
                // here and the loop is only told the draft is finished. Its
                // `Copied` arm logs and types nothing.
                ui.ctx().copy_text(state.text.clone());
                answer(channel, ReviewCommand::Copy { id });
            }
        }
        // The cancel button, named as the user asked for it: «لغو» throws the text
        // away (nothing is typed, nothing is kept), which is also the only way to
        // delete a draft. Its hover text says so, because a button whose effect is
        // "this text is gone" should not have to be guessed at.
        if ui
            .button(format_persian_display("لغو"))
            .on_hover_text(
                "throw this text away: nothing is typed, nothing is kept, and the\n\
                 window closes. Use کپی first if you want a copy of it.",
            )
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

    fn recovery_state() -> ReviewPanelState {
        let mut state = ReviewPanelState::default();
        state.open_recovery();
        state
    }

    #[test]
    fn undelivered_text_waits_for_explicit_recovery_but_review_still_opens() {
        let mut state = ReviewPanelState::default();
        let mut pending = draft(7, "متن");
        assert_eq!(plan_for_frame(&state, Some(&pending), &[]), WindowPlan::Hidden);
        state.open_recovery();
        assert_eq!(plan_for_frame(&state, Some(&pending), &[]), WindowPlan::Pending(7));
        pending.kind = DraftKind::Review;
        assert_eq!(plan_for_frame(&ReviewPanelState::default(), Some(&pending), &[]), WindowPlan::Pending(7));
    }

    fn draft(id: u64, text: &str) -> PendingDraft {
        PendingDraft {
            id,
            kind: DraftKind::Undelivered,
            text: text.into(),
            destination: None,
            session: None,
            raised: Instant::now(),
            source_record: None,
        }
    }

    /// The rule that keeps two dictations from running together in one box.
    #[test]
    fn a_new_draft_replaces_the_text_in_the_box() {
        let mut state = recovery_state();
        let snapshot = ReviewSnapshot {
            drafts: vec![draft(0, "اول")],
            revision: 1,
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
        let state = recovery_state();
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
        let mut state = recovery_state();

        channel.raise(draft(0, "اول"));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL)
        });
        state.text = "اول ویرایش‌شده".into();
        state.history.observe(&state.text, Instant::now());
        assert!(state.history.can_undo());

        // A second dictation arrives while the first is still in the box.
        channel.raise(draft(1, "دوم"));
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL)
        });
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
        let mut state = recovery_state();
        channel.raise(draft(0, "سلام"));

        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL)
        });
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
        let mut state = recovery_state();
        let destination = crate::output::target::TargetIdentity {
            hwnd: 5,
            pid: 9,
            exe_path: None,
            title_at_capture: "سند".into(),
            focus_hwnd: None,
            focus_element: None,
        };
        let mut d = draft(0, "سلام");
        d.destination = Some(destination.clone());
        channel.raise(d);

        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL)
        });

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
            let mut state = recovery_state();
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
        let mut state = recovery_state();
        let mut d = draft(0, "متن");
        d.destination = None;
        let output = draw_body(&channel, &mut state, &d);
        assert!(output.shapes.len() > 5);
    }

    /// The box is drawn **shaped**, which is what the user's "completely reversed
    /// and unreadable" was about.
    ///
    /// Asserted on the shapes egui was actually given, not on the widget's
    /// arguments: the bug was a `TextEdit` with no layouter, and a test that only
    /// read `state.text` would have passed all the way through it. Every text
    /// shape this window draws is checked, so the shaped line has to be in there.
    #[test]
    fn the_text_box_is_drawn_shaped_not_raw() {
        let channel = ReviewChannel::new();
        let mut state = recovery_state();
        let mut d = draft(0, "سلام دنیا از پاتون");
        d.destination = None;
        // What the loader would have put in the box (see `render_pending`):
        // `draw_body` draws the box, it does not fill it.
        state.text = d.text.clone();
        state.editing = Some(d.id);
        let output = draw_body(&channel, &mut state, &d);

        let shaped = format_persian_display(&d.text);
        assert_ne!(
            shaped, d.text,
            "the sample has to be text that shaping moves"
        );
        let mut drawn: Vec<String> = Vec::new();
        for clipped in &output.shapes {
            collect_drawn_text(&clipped.shape, &mut drawn);
        }
        assert!(
            drawn.iter().any(|line| line == &shaped),
            "the box must be laid out through the Persian shaper; drawn: {drawn:?}"
        );
    }

    /// Every string a shape will actually draw, including the ones inside
    /// [`egui::epaint::Shape::Vec`] — a `TextEdit` draws its text inside one.
    fn collect_drawn_text(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::epaint::Shape::Vec(shapes) => {
                for inner in shapes {
                    collect_drawn_text(inner, out);
                }
            }
            _ => {}
        }
    }

    /// A destination **with** a title must draw too: that arm formats and
    /// reshapes the title, which is the most likely place for a Persian string
    /// to go wrong.
    #[test]
    fn a_named_destination_draws() {
        let channel = ReviewChannel::new();
        let mut state = recovery_state();
        let mut d = draft(0, "متن");
        d.destination = Some(crate::output::target::TargetIdentity {
            hwnd: 1,
            pid: 1,
            exe_path: None,
            title_at_capture: "سندImportant".into(),
            focus_hwnd: None,
            focus_element: None,
        });
        let output = draw_body(&channel, &mut state, &d);
        assert!(output.shapes.len() > 5);
    }

    // ── the window's own life: open, close, and stay closed ──────────────

    /// The table the user experiences. Nothing pending and nothing kept: no
    /// window. A draft: the pending window. Kept text the user has not closed the
    /// window for: the archive.
    #[test]
    fn the_window_shows_what_is_worth_showing() {
        let pending = draft(4, "متن");
        let archived = vec![ArchivedDraft {
            draft: draft(3, "کهنه"),
            expired_at: Instant::now(),
        }];
        let none = BTreeSet::new();

        assert_eq!(plan_window(None, &[], &none, &none), WindowPlan::Hidden);
        assert_eq!(
            plan_window(Some(&pending), &[], &none, &none),
            WindowPlan::Pending(4)
        );
        assert_eq!(
            plan_window(Some(&pending), &archived, &none, &none),
            WindowPlan::Pending(4),
            "a live draft outranks the archive"
        );
        assert_eq!(
            plan_window(None, &archived, &none, &none),
            WindowPlan::Archive
        );
    }

    /// The defect the user hit: closing the window did nothing, because the
    /// window was drawn again on the very next frame. Closing it must hold for
    /// the text it was closed for.
    #[test]
    fn a_closed_window_stays_closed_for_what_it_was_closed_for() {
        let pending = draft(4, "متن");
        let archived = vec![ArchivedDraft {
            draft: draft(3, "کهنه"),
            expired_at: Instant::now(),
        }];

        let mut closed_pending = BTreeSet::new();
        closed_pending.insert(4);
        assert_eq!(
            plan_window(Some(&pending), &archived, &closed_pending, &BTreeSet::new()),
            WindowPlan::Hidden,
            "the user closed the window for this draft"
        );

        let mut closed_archive = BTreeSet::new();
        closed_archive.insert(3);
        assert_eq!(
            plan_window(None, &archived, &BTreeSet::new(), &closed_archive),
            WindowPlan::Hidden
        );
    }

    /// Closing is not a lock-out: a *new* text re-opens the window, which is how
    /// "your text was kept" still reaches the user after they closed the window.
    #[test]
    fn a_new_text_reopens_the_window_the_user_closed() {
        let mut closed_archive = BTreeSet::new();
        closed_archive.insert(3);
        let fresh = vec![
            ArchivedDraft {
                draft: draft(3, "کهنه"),
                expired_at: Instant::now(),
            },
            ArchivedDraft {
                draft: draft(9, "تازه"),
                expired_at: Instant::now(),
            },
        ];
        assert_eq!(
            plan_window(None, &fresh, &BTreeSet::new(), &closed_archive),
            WindowPlan::Archive,
            "an entry the user has not seen must be offered"
        );

        let mut closed_pending = BTreeSet::new();
        closed_pending.insert(3);
        let next = draft(9, "دیکتهٔ بعدی");
        assert_eq!(
            plan_window(Some(&next), &[], &closed_pending, &BTreeSet::new()),
            WindowPlan::Pending(9),
            "a different draft is a different decision"
        );
    }

    /// Closing the archive marks every text on screen as seen — otherwise the
    /// very next frame would compute the same plan and re-open the window.
    #[test]
    fn closing_the_archive_marks_everything_on_screen_as_seen() {
        let channel = ReviewChannel::new();
        let mut state = recovery_state();
        let archived = vec![
            ArchivedDraft {
                draft: draft(2, "اول"),
                expired_at: Instant::now(),
            },
            ArchivedDraft {
                draft: draft(5, "دوم"),
                expired_at: Instant::now(),
            },
        ];
        assert!(!archived.is_empty() && archived.len() == 2);
        state.close_archive(&archived);
        assert_eq!(
            plan_window(None, &archived, &BTreeSet::new(), &state.closed_archive),
            WindowPlan::Hidden
        );
        let _ = channel;
    }

    /// The bound on that memory: ids are handed out in increasing order, so the
    /// smallest is the oldest, and it is the one forgotten first.
    #[test]
    fn the_closed_memory_forgets_the_oldest_first() {
        let mut set = BTreeSet::new();
        for id in 0..(CLOSED_MEMORY as u64 + 5) {
            forget_oldest(&mut set, CLOSED_MEMORY);
            set.insert(id);
        }
        assert!(set.len() < CLOSED_MEMORY + 1, "the set is bounded");
        assert!(!set.contains(&0), "the oldest id was forgotten");
        assert!(
            set.contains(&(CLOSED_MEMORY as u64 + 4)),
            "the newest is kept"
        );
    }

    // ── the click that had to wait: «درج» ────────────────────────────────

    /// A click on «درج» hides the window and holds the command back.
    ///
    /// Both halves matter, and both are what the user's build was missing. The
    /// window has to go: a destination is judged against the window in front, and
    /// the window in front at the moment of the click is this one — which is how
    /// four presses of the button turned into four `validity=Changed` refusals.
    /// And the command has to wait, because sending it now would be judged *while*
    /// this window is still up.
    #[test]
    fn an_insert_gets_the_window_out_of_the_way_and_waits() {
        let mut state = recovery_state();
        let clicked = Instant::now();
        state.defer_insert(7, "سلام".into(), clicked);

        assert!(
            state.take_due_answer(clicked).is_none(),
            "the loop must not be asked while this window is still in front"
        );
        let draft = draft(7, "سلام");
        assert_eq!(
            plan_for_frame(&state, Some(&draft), &[]),
            WindowPlan::Hidden,
            "the window steps out of the way"
        );

        let due = clicked + STEP_ASIDE;
        assert_eq!(
            state.take_due_answer(due),
            Some(ReviewCommand::Insert {
                id: 7,
                text: "سلام".into()
            })
        );
        assert!(
            state.take_due_answer(due + STEP_ASIDE).is_none(),
            "an answer is sent exactly once"
        );
        assert_eq!(
            plan_for_frame(&state, Some(&draft), &[]),
            WindowPlan::Pending(7),
            "and afterwards the ordinary rule decides again"
        );
    }

    /// The text the loop is about to type must survive the moment the window is
    /// out of the way.
    ///
    /// "Nothing on screen" normally clears the box — a new draft must not inherit
    /// an old one's text — but an answer in flight is not that case: the box holds
    /// what the insert is about to type, and clearing it here would send the
    /// *empty* text the loop would then refuse.
    #[test]
    fn the_box_keeps_the_text_while_the_window_is_out_of_the_way() {
        let channel = ReviewChannel::new();
        let mut state = recovery_state();
        channel.raise(draft(0, "سلام دنیا"));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL)
        });
        assert_eq!(state.text, "سلام دنیا");
        // The store owns numbering, so the id the buttons must name is the one
        // the box was loaded with.
        let id = state.editing_id().expect("a draft is loaded");

        state.text = "سلام دنیا!".into();
        state.defer_insert(id, state.text.clone(), Instant::now());
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state, DEFAULT_DRAFT_TTL)
        });
        assert_eq!(
            state.editing_id(),
            Some(id),
            "the draft is still the one in hand"
        );
        assert_eq!(
            state.text, "سلام دنیا!",
            "what the loop was asked to type must still be in the box"
        );
    }

    /// Closing the pending window is not an answer: the draft is still there for
    /// the store, and it is still offered once it becomes archived text — which
    /// is the difference between "stop showing me this" and "throw this away".
    #[test]
    fn closing_the_window_answers_nothing_about_the_text() {
        let channel = ReviewChannel::new();
        let mut state = recovery_state();
        channel.raise(draft(0, "سلام"));
        let before = channel.snapshot();

        state.close_pending(0);

        let after = channel.snapshot();
        assert_eq!(after.drafts.len(), before.drafts.len());
        assert_eq!(after.drafts[0].text, "سلام");
        assert_eq!(after.revision, before.revision, "closing is not an answer");
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
        let mut state = recovery_state();

        // Nothing pending: nothing is loaded.
        render(
            &egui::Context::default(),
            &channel,
            &mut state,
            DEFAULT_DRAFT_TTL,
        );
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
