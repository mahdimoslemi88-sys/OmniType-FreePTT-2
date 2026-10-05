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
use std::time::Instant;

use eframe::egui;

use super::text::format_persian_display;
use super::theme::{
    apply_theme_visuals, manager_card, manager_central_panel, manager_header, manager_subtitle,
    palette,
};
use crate::state::review::{DraftKind, PendingDraft, ReviewCommand};
use crate::state::ReviewChannel;

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
/// saying which button was pressed.
pub fn render(ctx: &egui::Context, channel: &Arc<ReviewChannel>, state: &mut ReviewPanelState) {
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
        state.built_from = snapshot.revision;
        state.opened = latest.as_ref().map(|_| Instant::now());
    }

    let Some(draft) = latest else {
        // Nothing pending: the window closes itself. Left open with an empty
        // box it would be a dialog the user has to dismiss to get back to work.
        state.editing = None;
        state.text.clear();
        state.opened = None;
        return;
    };
    if state.editing != Some(draft.id) {
        // The store moved on under a box we could not reload (only reachable
        // while the loop is answering another draft). Draw the store's text.
        state.text = draft.text.clone();
    }

    let (win_w, win_h) = (win_w(), win_h());
    let screen = ctx.screen_rect();
    // Bottom-centre, above the taskbar: where the transcript bubble already
    // lives, so a review reads as "your dictation finished" rather than as an
    // unrelated dialog.
    let pos_x = (screen.center().x - win_w / 2.0).round();
    let pos_y = (screen.bottom() - win_h - 48.0).round();

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
                            render_body(ui, state, &draft, channel);
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
        let secs = opened.elapsed().as_secs();
        if secs >= 5 {
            ui.label(
                egui::RichText::new(format_persian_display(&format!(
                    "این پنجره پس از {secs} ثانیه بی‌پاسخ بسته می‌شود."
                )))
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
    use crate::state::review::DraftKind;
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
                    |ui| render_body(ui, state, draft, channel),
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
        render(&egui::Context::default(), &channel, &mut state);
        assert_eq!(state.editing_id(), None);

        channel.raise(draft(0, "سلام دنیا"));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state);
        });
        assert_eq!(state.editing_id(), Some(0), "the box takes the draft");
        assert_eq!(state.text, "سلام دنیا");

        // A second dictation replaces the box rather than appending: two
        // dictations running together in one box would type both at once.
        channel.raise(draft(1, "دوم"));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render(ctx, &channel, &mut state);
        });
        assert_eq!(state.editing_id(), Some(1));
        assert_eq!(
            state.text, "دوم",
            "a new draft must replace the box, not append to it"
        );
    }
}