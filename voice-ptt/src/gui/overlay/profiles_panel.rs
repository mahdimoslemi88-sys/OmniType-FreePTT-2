//! The profiles tab: which application runs under which rules.
//!
//! A panel of its own rather than another card in Settings, because it is a
//! *list with an editor* — the shape the dictionary tab has — and not a form of
//! scalar preferences. Mixing the two in one column is how a tab ends up three
//! screens long with the one setting somebody wants at the bottom.
//!
//! # What this panel may and may not decide
//!
//! It edits [`ProfileSet`] values and nothing else. It does not resolve them,
//! does not know which application is in front, and cannot apply a profile to a
//! dictation: [`crate::profiles`] owns the decisions and
//! [`crate::state::coordinator`] owns asking for them. The one place this file
//! touches the outside world is the *bind to the window in front* button, which
//! asks [`crate::output::capture_target`] for the executable of the foreground
//! window so the user does not have to spell `chrome.exe` from memory.
//!
//! # The draft
//!
//! Edits land in the settings **draft** the Settings tab already owns, and are
//! written to `config.toml` by this panel's save button through the same
//! validated path. That is deliberate: two drafts would mean two truths about
//! `config.toml`, and the second one to be saved would silently undo the first.
//!
//! # The live preview
//!
//! The editor shows what the profile being edited does to one sentence, beside
//! what the same application would get without it. Both lines come from
//! [`TextRules`], the value the coordinator applies to a real dictation, and the
//! general line borrows the **live** dictionary — so the difference between the
//! two is exactly what the profile is for, down to a rule that was saved a
//! moment ago. This is not the panel resolving a destination: which application
//! is in front stays [`crate::profiles`]' business.

use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use eframe::egui;
use egui_phosphor::regular as ic;

use super::text::{format_persian_display, persian_text_edit_layouter};
use super::theme::*;
use crate::config::settings::Settings;
use crate::processing::quickfix;
use crate::processing::{Dictionary, Normalizer, TextMode, TextRules};
use crate::profiles::{binding_key, mode_for, AppProfile, Overrides, ProfileSet};

/// The sentence the live preview runs on when the panel first opens.
///
/// Written **without** the half-space between `می` and `کنم`, and with `پاتون`
/// rather than `پایتون`, on purpose: the ordinary modes insert the half-space
/// and the general dictionary corrects that word, so the built-in sample is one
/// the pipeline visibly does something to. A sample both lines left alone would
/// make the feature look broken the first time it was opened.
pub(crate) const DEFAULT_SAMPLE: &str = concat!("من با پاتون کار می", "کنم");

/// The profile panel's own UI state.
///
/// None of it is settings: which row is selected, what is half-typed into the
/// new-correction fields, and the feedback line. Keeping it out of [`Settings`]
/// is what lets the panel be closed and reopened without inventing a config key
/// for "the row the user last clicked".
pub(crate) struct ProfilesPanelState {
    /// The entry the editor below the list is open on, by index into the set.
    ///
    /// An index rather than the binding, because the binding is exactly what
    /// the editor can change — and a selection that changes identity as the
    /// user types would jump to another row mid-edit.
    pub(crate) selected: Option<usize>,
    /// Half-typed correction, in the order the fields are drawn.
    pub(crate) rule_from: String,
    pub(crate) rule_to: String,
    /// The sentence the live preview runs on, as the user last wrote it.
    pub(crate) preview_sample: String,
    /// Transient feedback (saved / refused), auto-expiring like the settings
    /// tab's, so a message cannot outlive the state it describes.
    pub(crate) msg: Option<(String, Instant)>,
    /// Built once with the state rather than per frame: the preview runs the
    /// real pipeline, and rebuilding the normalizer's tables sixty times a
    /// second for a panel that is usually not even open is work nobody asked
    /// for.
    normalizer: Normalizer,
}

impl Default for ProfilesPanelState {
    fn default() -> Self {
        Self {
            selected: None,
            rule_from: String::new(),
            rule_to: String::new(),
            // Seeded, not hinted: a preview that opened empty would look like a
            // panel with nothing to say rather than one waiting for a sentence.
            preview_sample: DEFAULT_SAMPLE.to_string(),
            msg: None,
            normalizer: Normalizer::new(),
        }
    }
}

impl ProfilesPanelState {
    /// Whether a feedback line is still worth drawing.
    fn flash(&mut self, text: impl Into<String>) {
        self.msg = Some((text.into(), Instant::now()));
    }

    /// Keeps the selection inside the list after a removal.
    ///
    /// Called after anything that can shorten the set, so "the third row" can
    /// never point past the end — an editor bound to a missing entry would
    /// either panic or silently edit nothing.
    fn clamp(&mut self, len: usize) {
        self.selected = match self.selected {
            Some(i) if i < len => Some(i),
            Some(_) if len > 0 => Some(len - 1),
            _ => None,
        };
    }
}

/// What the pipeline does to one sentence under a profile, against what the same
/// application would get without it.
///
/// Both texts are produced by [`TextRules`] — the value the coordinator applies
/// to a real dictation — so neither line is a paraphrase of the product. The
/// general side is deliberately *not* the profile with its overrides stripped:
/// it is the plain general pipeline, because that is the thing a user is
/// comparing against when they ask what a profile is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffectPreview {
    /// What an application without a profile gets, mode and dictionary alike.
    pub general: String,
    /// What this application gets, with the profile as it is being edited.
    pub profile: String,
    /// The mode the profile runs under. Shown because `raw` is the override that
    /// explains an otherwise puzzling "it ignored my rule".
    pub mode: TextMode,
}

impl EffectPreview {
    /// Whether the profile changes this sentence at all.
    pub(crate) fn changes_anything(&self) -> bool {
        self.general != self.profile
    }
}

/// Runs `sample` through the general pipeline and through `overrides`.
///
/// The dictionary is **borrowed, never rebuilt**: it is the same compiled value
/// the coordinator matches against, so a rule saved a second ago is already in
/// the preview rather than after the next restart. The profile's own rules are
/// compiled per call inside [`TextRules::apply`], which is what that type was
/// built for — a profile holds a handful of rules.
pub(crate) fn preview_effect(
    sample: &str,
    general_mode: TextMode,
    commands: bool,
    formal: crate::processing::formal::FormalOptions,
    normalizer: &Normalizer,
    dictionary: &Dictionary,
    overrides: &Overrides,
) -> EffectPreview {
    let mode = mode_for(overrides, general_mode);
    let seen = quickfix::preview(
        sample,
        TextRules {
            mode: general_mode,
            commands,
            formal,
            normalizer,
            dictionary,
            corrections: &[],
        },
        TextRules {
            mode,
            commands,
            formal,
            normalizer,
            dictionary,
            corrections: &overrides.corrections,
        },
    );
    EffectPreview {
        general: seen.before,
        profile: seen.after,
        mode,
    }
}

/// The drop-down entries for a text mode, `None` meaning "the general value".
///
/// `None` first, because it is the default state of a new profile and the one
/// a user who only wants a dictionary rule for one application leaves alone.
const MODE_CHOICES: [(&str, Option<&str>); 5] = [
    ("مقدار عمومی", None),
    ("خام (بدون پردازش)", Some("raw")),
    ("محافظه‌کارانه", Some("conservative")),
    ("استاندارد", Some("standard")),
    ("رسمی (نگارش رسمی)", Some("formal")),
];

/// A one-line description of what a profile changes.
///
/// Written for the list, where the question is "what does this row do?" — so it
/// names the things that differ from the general settings and says nothing about
/// the ones that do not. An inert entry says so, rather than showing an empty
/// line that reads like a rendering bug.
pub(crate) fn summary_parts(profile: &AppProfile) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(mode) = profile.overrides.text_mode.as_deref() {
        let trimmed = mode.trim();
        let label = MODE_CHOICES
            .iter()
            .find(|(_, value)| value.is_some_and(|v| v.eq_ignore_ascii_case(trimmed)))
            .map(|(label, _)| *label);
        parts.push(match label {
            Some(label) => label.to_string(),
            // A hand-edited string this panel cannot name is still worth
            // showing: the user typed it, and hiding it would look like the
            // panel dropped it.
            None => format!("حالت «{trimmed}»"),
        });
    }
    if let Some(review) = profile.overrides.review_before_insert {
        parts.push(
            if review {
                "بازبینی پیش از درج"
            } else {
                "درج مستقیم"
            }
            .to_string(),
        );
    }
    if !profile.overrides.corrections.is_empty() {
        parts.push(format!(
            "{} قاعدهٔ اختصاصی",
            profile.overrides.corrections.len()
        ));
    }
    if parts.is_empty() {
        // An inert entry says so rather than showing an empty line, which
        // would read as a rendering fault and send the user looking for the
        // part that is missing.
        parts.push("بدون تغییر (مقدارهای عمومی)".to_string());
    }
    parts
}

/// The line the list shows for one entry.
///
/// The parts are formatted here and joined, and **only** here — the shaping is
/// a rendering detail, which is why the decisions above live in
/// [`summary_parts`] where a test can read them as the plain strings the user
/// would type.
pub(crate) fn summary(profile: &AppProfile) -> String {
    summary_parts(profile)
        .iter()
        .map(|part| format_persian_display(part))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Whether `binding` may be saved for the entry at `index`.
///
/// Two rules, and both exist because the resolver refuses what they would
/// produce: an empty binding would match nothing (so the row would look broken
/// to the user and be inert in fact), and a second entry for one executable
/// makes [`ProfileSet::resolve`] ambiguous — which the coordinator answers with
/// the general rules. Catching it here is the difference between "the panel
/// says why" and "my profile silently does nothing".
pub(crate) fn validate_binding(
    binding: &str,
    set: &ProfileSet,
    index: Option<usize>,
) -> Result<(), String> {
    let trimmed = binding.trim();
    if trimmed.is_empty() {
        return Err(format_persian_display(
            "نام برنامه خالی است؛ مثل code.exe یا مسیر کامل آن را بنویسید",
        ));
    }
    let key = binding_key(trimmed);
    let clash = set
        .profiles()
        .iter()
        .enumerate()
        .find(|(i, p)| Some(*i) != index && binding_key(&p.exe) == key);
    if let Some((_, other)) = clash {
        let name = if other.name.trim().is_empty() {
            other.exe.trim()
        } else {
            other.name.trim()
        };
        return Err(format_persian_display(&format!(
            "این برنامه همین حالا به پروفایل «{name}» بسته شده است"
        )));
    }
    Ok(())
}

/// The first problem in the whole set, if any.
///
/// Checked before a save so a set that the resolver would refuse is never
/// written to disk: an ambiguity in the file is invisible until a dictation
/// quietly uses the general rules, which is the failure this whole feature
/// exists to remove.
pub(crate) fn validate_set(set: &ProfileSet) -> Result<(), String> {
    for (i, profile) in set.profiles().iter().enumerate() {
        validate_binding(&profile.exe, set, Some(i))
            .map_err(|e| format_persian_display(&format!("پروفایل {i} → {e}")))?;
    }
    Ok(())
}

/// The executable of the window in front of the user, as a binding.
///
/// The only thing in this module that reads the desktop, and it reads it for
/// exactly one purpose: filling in a field the user would otherwise have to
/// spell. It resolves nothing — the value goes into the draft and is saved by
/// the save button like any typed binding.
fn foreground_binding() -> Option<String> {
    let target = crate::output::capture_target().ok()?;
    let exe = target.exe_path?;
    exe.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
}

/// Renders the profiles tab into the dashboard's current panel.
pub(crate) fn render(
    ui: &mut egui::Ui,
    state: &mut ProfilesPanelState,
    draft: &mut Settings,
    settings: &Arc<RwLock<Settings>>,
    config_path: &Path,
    dictionary: &Arc<RwLock<Dictionary>>,
) {
    state.clamp(draft.profiles.len());

    // The sample box holds edited Persian, so it goes through the same shaper the
    // quick fix uses: without it, a sentence typed here would draw as
    // disconnected letters in the wrong order.
    let mut persian_layouter =
        |ui: &egui::Ui, text: &str, w: f32| persian_text_edit_layouter(ui, text, w);

    manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            egui::RichText::new(format!(
                "{}  {}",
                format_persian_display("پروفایل برنامه‌ها"),
                ic::APP_WINDOW
            ))
            .size(12.5)
            .strong()
            .color(palette::TEXT_SECTION),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format_persian_display(
                "هر پنجره با قواعد خودش اجرا می‌شود؛ پنجره‌ای که پروفایل ندارد، \
                 مقدارهای عمومی همین تب تنظیمات را می‌گیرد.",
            ))
            .size(10.0)
            .color(palette::TEXT_MUTED),
        );
        ui.add_space(6.0);

        ui.columns(2, |cols| {
            // ── the list ──────────────────────────────────────────────────
            cols[0].push_id("profiles_list", |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                    |ui| {
                        manager_card(palette::CARD_BG, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format_persian_display("فهرست"))
                                    .size(11.0)
                                    .color(palette::TEXT_LABEL),
                            );
                            ui.add_space(4.0);

                            if draft.profiles.is_empty() {
                                ui.label(
                                    egui::RichText::new(format_persian_display(
                                        "هنوز پروفایلی ساخته نشده است",
                                    ))
                                    .size(10.0)
                                    .color(palette::TEXT_FAINT),
                                );
                            }

                            let mut remove: Option<usize> = None;
                            for (i, profile) in draft.profiles.profiles().iter().enumerate() {
                                let selected = state.selected == Some(i);
                                let title = if profile.name.trim().is_empty() {
                                    profile.exe.trim().to_string()
                                } else {
                                    profile.name.trim().to_string()
                                };
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(
                                            selected,
                                            egui::RichText::new(title).size(11.0),
                                        )
                                        .clicked()
                                    {
                                        state.selected = Some(i);
                                    }
                                    if ui
                                        .button(egui::RichText::new(ic::TRASH).size(10.0))
                                        .on_hover_text("Remove this profile")
                                        .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                });
                                ui.label(
                                    egui::RichText::new(summary(profile))
                                        .size(9.5)
                                        .color(palette::TEXT_MUTED),
                                );
                                ui.add_space(3.0);
                            }
                            if let Some(i) = remove {
                                draft.profiles.remove(i);
                                state.selected = None;
                                state.flash("پروفایل حذف شد (برای ثبت، ذخیره کنید)");
                            }

                            ui.add_space(6.0);
                            if ui
                                .button(
                                    egui::RichText::new(format!(
                                        "{}  {}",
                                        format_persian_display("پروفایل تازه"),
                                        ic::PLUS
                                    ))
                                    .size(10.5),
                                )
                                .clicked()
                            {
                                // Bound to nothing yet: an entry with an empty
                                // binding is editable and inert until the user
                                // says which application it is for. Saving it in
                                // that state is refused by `validate_set`.
                                draft
                                    .profiles
                                    .push(AppProfile::new("", "", Overrides::default()));
                                state.selected = Some(draft.profiles.len().saturating_sub(1));
                                state.rule_from.clear();
                                state.rule_to.clear();
                            }
                        });

                        ui.add_space(8.0);

                        // ── save ─────────────────────────────────────────────
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            if let Some((text, at)) = state.msg.clone() {
                                if at.elapsed().as_secs() < 6 {
                                    ui.label(
                                        egui::RichText::new(text)
                                            .size(10.0)
                                            .color(palette::SUCCESS),
                                    );
                                    ui.add_space(4.0);
                                } else {
                                    state.msg = None;
                                }
                            }

                            // The same validator the Settings tab uses: this
                            // panel writes the *same* draft, so it must not be
                            // the one place that can put an invalid one on disk.
                            let validation = super::validate_settings(draft).err();
                            let set_error = validate_set(&draft.profiles).err();
                            if let Some(err) = validation.clone().or_else(|| set_error.clone()) {
                                callout(ui, CalloutKind::Warning, &err);
                                ui.add_space(4.0);
                            }

                            let can_save = validation.is_none() && set_error.is_none();
                            ui.horizontal(|ui| {
                                let save = ui.add_enabled(
                                    can_save,
                                    egui::Button::new(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display("ذخیره پروفایل‌ها"),
                                            ic::FLOPPY_DISK
                                        ))
                                        .size(11.0)
                                        .strong()
                                        .color(palette::WHITE),
                                    )
                                    .fill(if can_save {
                                        palette::ACCENT_ACTION
                                    } else {
                                        palette::CHIP_BG
                                    })
                                    .rounding(egui::Rounding::same(6.0)),
                                );
                                if save.clicked() {
                                    if let Ok(mut s) = settings.write() {
                                        *s = draft.clone();
                                        match s.save(config_path) {
                                            Ok(()) => {
                                                state.flash("پروفایل‌ها ذخیره شد");
                                            }
                                            Err(e) => {
                                                state.flash(format!(
                                                    "ذخیره نشد: {e}"
                                                ));
                                            }
                                        }
                                    }
                                }
                                if ui
                                    .button(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display("بارگذاری مجدد"),
                                            ic::ARROWS_CLOCKWISE
                                        ))
                                        .size(10.5),
                                    )
                                    .clicked()
                                {
                                    if let Ok(loaded) = Settings::load_or_create(config_path) {
                                        let profiles = loaded.profiles.clone();
                                        if let Ok(mut s) = settings.write() {
                                            *s = loaded;
                                        }
                                        // Only the profiles are taken back: the
                                        // Settings tab's own draft is its own
                                        // business, and reloading this tab must
                                        // not silently discard an unrelated
                                        // half-typed field over there.
                                        draft.profiles = profiles;
                                        state.selected = None;
                                        state.msg = None;
                                    }
                                }
                            });
                        });
                    },
                );
            });

            // ── the editor ────────────────────────────────────────────────────
            cols[1].push_id("profiles_editor", |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                    |ui| {
                        manager_card(palette::CARD_BG, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format_persian_display("ویرایش پروفایل"))
                                    .size(11.0)
                                    .color(palette::TEXT_LABEL),
                            );
                            ui.add_space(4.0);

                            let Some(index) = state.selected else {
                                ui.label(
                                    egui::RichText::new(format_persian_display(
                                        "یک پروفایل را از فهرست انتخاب کنید",
                                    ))
                                    .size(10.0)
                                    .color(palette::TEXT_FAINT),
                                );
                                return;
                            };
                            let Some(profile) = draft.profiles.profiles().get(index).cloned()
                            else {
                                state.selected = None;
                                return;
                            };
                            let mut edited = profile.clone();

                            let label_w = 96.0;
                            rtl_form_row(ui, "نام:", label_w, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut edited.name)
                                        .hint_text("Editor")
                                        .desired_width(150.0),
                                );
                            });
                            ui.add_space(4.0);

                            rtl_form_row(ui, "برنامه:", label_w, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut edited.exe)
                                        .hint_text("code.exe")
                                        .desired_width(150.0),
                                );
                            });
                            ui.add_space(2.0);
                            if ui
                                .button(
                                    egui::RichText::new(format!(
                                        "{}  {}",
                                        format_persian_display("از پنجرهٔ فعال"),
                                        ic::CROSSHAIR
                                    ))
                                    .size(10.0),
                                )
                                .on_hover_text(
                                    "Bind to the executable of the window that is in front \
                                     right now, instead of typing its name.",
                                )
                                .clicked()
                            {
                                match foreground_binding() {
                                    Some(exe) => edited.exe = exe,
                                    None => state.flash(
                                        "پنجرهٔ فعال خوانده نشد؛ نام برنامه را دستی بنویسید",
                                    ),
                                }
                            }
                            if let Err(problem) = validate_binding(&edited.exe, &draft.profiles, Some(index))
                            {
                                ui.add_space(3.0);
                                callout(ui, CalloutKind::Warning, &problem);
                            }
                            ui.add_space(6.0);

                            // ── what this profile changes ─────────────────────
                            rtl_form_row(ui, "متن:", label_w, |ui| {
                                let current = edited
                                    .overrides
                                    .text_mode
                                    .as_deref()
                                    .map(|m| m.trim().to_ascii_lowercase());
                                let selected_label = MODE_CHOICES
                                    .iter()
                                    .find(|(_, value)| {
                                        value.map(|v| {
                                            current.as_deref() == Some(v.to_ascii_lowercase().as_str())
                                        })
                                        .unwrap_or(current.is_none())
                                    })
                                    .map(|(label, _)| *label)
                                    .unwrap_or("استاندارد");
                                egui::ComboBox::from_id_source("profile_text_mode")
                                    .selected_text(format_persian_display(selected_label))
                                    .width(150.0)
                                    .show_ui(ui, |ui| {
                                        for (label, value) in MODE_CHOICES {
                                            let chosen = match value {
                                                None => current.is_none(),
                                                Some(v) => {
                                                    current.as_deref()
                                                        == Some(v.to_ascii_lowercase().as_str())
                                                }
                                            };
                                            if ui
                                                .selectable_label(
                                                    chosen,
                                                    format_persian_display(label),
                                                )
                                                .clicked()
                                            {
                                                edited.overrides.text_mode =
                                                    value.map(str::to_string);
                                            }
                                        }
                                    });
                            });
                            ui.add_space(4.0);

                            // The checkbox shows what is *in force* here — the
                            // override if there is one, the general value if
                            // not — so it never displays a state this
                            // application is not actually in.
                            let general_review = draft.gui.review_before_insert;
                            let mut review = edited
                                .overrides
                                .review_before_insert
                                .unwrap_or(general_review);
                            ui.horizontal(|ui| {
                                if ui
                                    .checkbox(
                                        &mut review,
                                        format_persian_display("نمایش متن پیش از درج"),
                                    )
                                    .on_hover_text(
                                        "Hold this application's text for approval. Unticking \
                                         it inserts directly even when the general switch asks \
                                         for review.",
                                    )
                                    .changed()
                                {
                                    edited.overrides.review_before_insert = Some(review);
                                }
                                // The way *back* to the general value. Without
                                // it an override could be set from here and never
                                // removed, and "my profile cannot be undone" is
                                // how a panel loses a user's trust.
                                if edited.overrides.review_before_insert.is_some()
                                    && ui
                                        .button(
                                            egui::RichText::new(format_persian_display(
                                                "پیروی از عمومی",
                                            ))
                                            .size(9.5),
                                        )
                                        .on_hover_text(
                                            "Drop this override: the general setting applies \
                                             again in this application.",
                                        )
                                        .clicked()
                                {
                                    edited.overrides.review_before_insert = None;
                                }
                            });
                            if edited.overrides.review_before_insert.is_none() {
                                ui.label(
                                    egui::RichText::new(format_persian_display(&format!(
                                        "از تنظیمات عمومی ({})",
                                        if general_review { "روشن" } else { "خاموش" }
                                    )))
                                    .size(9.5)
                                    .color(palette::TEXT_FAINT),
                                );
                            }
                            ui.add_space(6.0);

                            // ── the profile's own dictionary rules ────────────
                            ui.label(
                                egui::RichText::new(format_persian_display(
                                    "قاعده‌های اختصاصی این برنامه",
                                ))
                                .size(10.5)
                                .color(palette::TEXT_LABEL),
                            );
                            if edited.overrides.corrections.is_empty() {
                                ui.label(
                                    egui::RichText::new(format_persian_display(
                                        "قاعده‌ای نیست",
                                    ))
                                    .size(9.5)
                                    .color(palette::TEXT_FAINT),
                                );
                            }
                            let mut drop: Option<usize> = None;
                            for (i, rule) in edited.overrides.corrections.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(egui::RichText::new(ic::TRASH).size(9.0))
                                        .clicked()
                                    {
                                        drop = Some(i);
                                    }
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} → {}",
                                            rule.from, rule.to
                                        ))
                                        .size(10.0),
                                    );
                                });
                            }
                            if let Some(i) = drop {
                                edited.overrides.corrections.remove(i);
                            }
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut state.rule_to)
                                        .hint_text("پایتون")
                                        .desired_width(78.0),
                                );
                                ui.label(
                                    egui::RichText::new(format_persian_display("جایگزین")).size(10.0),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut state.rule_from)
                                        .hint_text("پاتون")
                                        .desired_width(78.0),
                                );
                                if ui
                                    .button(egui::RichText::new(ic::PLUS).size(10.0))
                                    .clicked()
                                {
                                    let (from, to) = (
                                        state.rule_from.trim().to_string(),
                                        state.rule_to.trim().to_string(),
                                    );
                                    // Both halves or neither: a rule with one
                                    // empty side does nothing in the dictionary
                                    // (`compile_rules` drops it), so accepting it
                                    // would put a row in this list that changes no
                                    // text at all.
                                    if from.is_empty() || to.is_empty() || from == to {
                                        state.flash(
                                            "قاعده باید دو طرف داشته باشد و دو طرف یکسان نباشد",
                                        );
                                    } else {
                                        edited.overrides.corrections.push(
                                            crate::processing::dictionary::Correction {
                                                from,
                                                to,
                                                category: Some("profile".into()),
                                            },
                                        );
                                        state.rule_from.clear();
                                        state.rule_to.clear();
                                    }
                                }
                            });
                            if let Some(from) = edited.overrides.text_mode.as_deref() {
                                if from.trim().eq_ignore_ascii_case(TextMode::Raw.as_str())
                                    && !edited.overrides.corrections.is_empty()
                                {
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(format_persian_display(
                                            "در حالت خام، این قاعده‌ها هنوز اجرا می‌شوند؛ \
                                             خام فقط پردازش خودکار را خاموش می‌کند",
                                        ))
                                        .size(9.5)
                                        .color(palette::TEXT_MUTED),
                                    );
                                }
                            }
                            // ── what this profile does to a sentence ──────────
                            ui.add_space(6.0);
                            ui.separator();
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format_persian_display(
                                    "اثر این پروفایل روی یک جمله",
                                ))
                                .size(10.5)
                                .color(palette::TEXT_LABEL),
                            );
                            ui.label(
                                egui::RichText::new(format_persian_display(
                                    "هر دو خط با همان مسیر واقعی پردازش ساخته می‌شوند؛ تفاوتشان همان چیزی است که این برنامه از پروفایل می‌گیرد.",
                                ))
                                .size(9.5)
                                .color(palette::TEXT_MUTED),
                            );
                            ui.add_space(3.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut state.preview_sample)
                                    .hint_text(format_persian_display(DEFAULT_SAMPLE))
                                    .desired_width(f32::INFINITY)
                                    .layouter(&mut persian_layouter),
                            );
                            ui.add_space(4.0);

                            // Computed from the values on screen right now,
                            // including the edits not yet applied — which is the
                            // whole point of a live preview.
                            let effect = {
                                let general_mode = draft.text.mode();
                                match dictionary.read() {
                                    Ok(dict) => preview_effect(
                                        &state.preview_sample,
                                        general_mode,
                                        draft.text.commands,
                                        draft.text.formal_options(),
                                        &state.normalizer,
                                        &dict,
                                        &edited.overrides,
                                    ),
                                    // A poisoned lock is not a reason to draw a
                                    // wrong answer: two identical lines are the
                                    // honest thing to show when nothing can be
                                    // computed at all.
                                    Err(_) => EffectPreview {
                                        general: state.preview_sample.clone(),
                                        profile: state.preview_sample.clone(),
                                        mode: general_mode,
                                    },
                                }
                            };
                            // Reversed on purpose, like the quick fix's: Persian
                            // reads right to left, so the line without the
                            // profile is the one on the right.
                            preview_line(
                                ui,
                                "بدون پروفایل",
                                &effect.general,
                                palette::TEXT_SECONDARY,
                            );
                            preview_line(
                                ui,
                                "با این پروفایل",
                                &effect.profile,
                                if effect.changes_anything() {
                                    palette::SUCCESS
                                } else {
                                    palette::TEXT_MUTED
                                },
                            );
                            ui.add_space(3.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}: {}",
                                    format_persian_display("حالت این برنامه"),
                                    format_persian_display(mode_label(effect.mode)),
                                ))
                                .size(9.5)
                                .color(palette::TEXT_FAINT),
                            );
                            if !effect.changes_anything() {
                                ui.add_space(3.0);
                                callout(
                                    ui,
                                    CalloutKind::Info,
                                    &format_persian_display(
                                        "این پروفایل این جمله را تغییر نمی‌دهد؛ یا قاعده‌ای نگرفته و حالتش هم مثل مقدار عمومی است، یا واژه‌های این جمله در آن نیستند.",
                                    ),
                                );
                            }

                            ui.add_space(6.0);

                            ui.horizontal(|ui| {
                                if ui
                                    .button(
                                        egui::RichText::new(format_persian_display("اعمال"))
                                            .size(10.5),
                                    )
                                    .clicked()
                                {
                                    draft.profiles.upsert(edited.clone());
                                    // `upsert` is keyed on the binding and can
                                    // move the entry, so the selection is
                                    // re-found rather than assumed.
                                    state.selected = draft
                                        .profiles
                                        .profiles()
                                        .iter()
                                        .position(|p| {
                                            binding_key(&p.exe) == binding_key(&edited.exe)
                                        })
                                        .or(state.selected);
                                }
                            });
                        });
                    },
                );
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::dictionary::Correction;

    fn profile(name: &str, exe: &str, overrides: Overrides) -> AppProfile {
        AppProfile::new(name, exe, overrides)
    }

    fn raw() -> Overrides {
        Overrides {
            text_mode: Some("raw".into()),
            ..Default::default()
        }
    }

    // ── the list's one-line description ───────────────────────────────────

    // The decisions are asserted on `summary_parts` rather than on the rendered
    // line: `format_persian_display` reshapes Persian into presentation forms,
    // so a substring written the way a human types it is not a substring of
    // what is drawn. The shaping is asserted once, in its own test below.

    /// An entry that changes nothing says so. A blank line would read as a
    /// rendering fault, and the user would go looking for the missing part.
    #[test]
    fn an_inert_profile_says_it_changes_nothing() {
        assert_eq!(
            summary_parts(&profile("Empty", "code.exe", Overrides::default())),
            vec!["بدون تغییر (مقدارهای عمومی)".to_string()]
        );
        assert!(!summary(&profile("Empty", "code.exe", Overrides::default())).is_empty());
    }

    /// Only the fields that *are* overridden are named — that is the whole
    /// question the list answers.
    #[test]
    fn the_summary_names_only_what_the_profile_overrides() {
        let mode_only = summary_parts(&profile("T", "wt.exe", raw()));
        assert_eq!(mode_only.len(), 1, "one override, one part: {mode_only:?}");
        assert!(mode_only[0].contains("خام"), "{mode_only:?}");

        let review_only = summary_parts(&profile(
            "C",
            "slack.exe",
            Overrides {
                review_before_insert: Some(false),
                ..Default::default()
            },
        ));
        assert_eq!(
            review_only,
            vec!["درج مستقیم".to_string()],
            "an explicit `false` is worth showing, not hiding"
        );
    }

    /// A hand-edited mode string this panel cannot name is still shown, so the
    /// user can see what is in the file rather than an empty override.
    #[test]
    fn an_unrecognised_mode_is_shown_rather_than_dropped() {
        let parts = summary_parts(&profile(
            "Typo",
            "code.exe",
            Overrides {
                text_mode: Some("standart".into()),
                ..Default::default()
            },
        ));
        assert_eq!(parts.len(), 1);
        assert!(parts[0].contains("standart"), "{parts:?}");
    }

    #[test]
    fn corrections_are_counted_in_the_summary() {
        let parts = summary_parts(&profile(
            "P",
            "code.exe",
            Overrides {
                corrections: vec![
                    Correction {
                        from: "a".into(),
                        to: "b".into(),
                        category: None,
                    },
                    Correction {
                        from: "c".into(),
                        to: "d".into(),
                        category: None,
                    },
                ],
                ..Default::default()
            },
        ));
        assert_eq!(parts.len(), 1);
        assert!(parts[0].contains('2'), "the rule count must appear: {parts:?}");
    }

    /// What [`summary`] itself decides: the parts, in order, joined by one
    /// separator — the only thing left that can be wrong once the parts are
    /// right.
    #[test]
    fn the_rendered_summary_joins_its_parts() {
        let text = summary(&profile(
            "T",
            "wt.exe",
            Overrides {
                text_mode: Some("raw".into()),
                review_before_insert: Some(true),
                ..Default::default()
            },
        ));
        assert_eq!(text.matches('·').count(), 1, "two parts, one separator: {text:?}");

        // A single part must not trail the separator.
        let one = summary(&profile("T", "wt.exe", raw()));
        assert_eq!(one.matches('·').count(), 0, "{one:?}");
    }

    // ── what may be saved ─────────────────────────────────────────────────

    /// An empty binding matches nothing, so saving one would produce a row that
    /// looks configured and never applies.
    #[test]
    fn an_empty_binding_is_refused() {
        let set = ProfileSet::default();
        assert!(validate_binding("", &set, None).is_err());
        assert!(validate_binding("   ", &set, None).is_err());
    }

    /// Two entries for one executable is an ambiguity the resolver answers with
    /// the general rules — so it is refused where the user can be told why.
    #[test]
    fn a_second_entry_for_one_executable_is_refused() {
        let set = ProfileSet::new(vec![profile("Editor", "code.exe", raw())]);
        assert!(validate_binding("code.exe", &set, Some(1)).is_err());
        assert!(
            validate_binding("CODE.EXE", &set, Some(1)).is_err(),
            "the same application, spelled differently, is still the same application"
        );
        assert!(
            validate_binding("wt.exe", &set, Some(1)).is_ok(),
            "a different application is not a clash"
        );
    }

    /// Editing an entry must not clash with itself: the row being edited is
    /// excluded from the search.
    #[test]
    fn an_entry_may_keep_its_own_binding() {
        let set = ProfileSet::new(vec![profile("Editor", "code.exe", raw())]);
        assert!(validate_binding("code.exe", &set, Some(0)).is_ok());
    }

    /// The whole-set check the save button runs: the first problem is reported,
    /// and a clean set passes.
    #[test]
    fn a_set_with_a_duplicate_or_an_empty_binding_is_refused() {
        let clean = ProfileSet::new(vec![
            profile("Editor", "code.exe", raw()),
            profile("Terminal", "wt.exe", raw()),
        ]);
        assert!(validate_set(&clean).is_ok());

        let duplicate = ProfileSet::new(vec![
            profile("One", "code.exe", raw()),
            profile("Two", "Code.exe", Overrides::default()),
        ]);
        assert!(validate_set(&duplicate).is_err());

        let empty = ProfileSet::new(vec![profile("Nowhere", "", raw())]);
        assert!(validate_set(&empty).is_err());

        // …and an empty set is the ordinary case, not a problem.
        assert!(validate_set(&ProfileSet::default()).is_ok());
    }

    // ── state ─────────────────────────────────────────────────────────────

    /// The editor's index must not survive the entry it points at.
    #[test]
    fn the_selection_follows_a_shortened_list() {
        let mut state = ProfilesPanelState {
            selected: Some(3),
            ..Default::default()
        };
        state.clamp(2);
        assert_eq!(state.selected, Some(1), "the last row, not a row that is gone");

        state.clamp(0);
        assert_eq!(state.selected, None, "nothing left to edit");
    }

    /// A selection that is still inside the list is left alone — clicking a row
    /// and having the panel move it would be worse than useless.
    #[test]
    fn a_valid_selection_is_untouched() {
        let mut state = ProfilesPanelState {
            selected: Some(1),
            ..Default::default()
        };
        state.clamp(3);
        assert_eq!(state.selected, Some(1));
    }

    // ── the live preview ──────────────────────────────────────────────────

    // The lines the panel draws are two calls to `preview_line`, so what is
    // worth testing is the text they are handed: `preview_effect`.

    /// The built-in sample is one the pipeline actually touches: the ordinary
    /// modes insert the half-space, so the very first open shows the machine
    /// doing something rather than two identical lines that read as a fault.
    #[test]
    fn the_built_in_sample_is_one_the_pipeline_changes() {
        let effect = preview_effect(
            DEFAULT_SAMPLE,
            TextMode::Standard,
            false,
            crate::processing::formal::FormalOptions::default(),
            &Normalizer::new(),
            &Dictionary::with_defaults(),
            &Overrides::default(),
        );
        assert!(
            effect.general.contains("می\u{200c}کنم"),
            "the ordinary modes insert the half-space: {effect:?}"
        );
        assert_ne!(
            effect.general, DEFAULT_SAMPLE,
            "a sample nothing happens to would leave both lines identical"
        );
    }

    /// A profile that overrides nothing produces the same sentence on both
    /// lines, and the preview says so instead of inventing a difference. This is
    /// also the case that keeps the mode from being shown as anything but the
    /// general value.
    #[test]
    fn a_profile_that_overrides_nothing_changes_nothing() {
        let effect = preview_effect(
            DEFAULT_SAMPLE,
            TextMode::Standard,
            false,
            crate::processing::formal::FormalOptions::default(),
            &Normalizer::new(),
            &Dictionary::with_defaults(),
            &Overrides::default(),
        );
        assert!(!effect.changes_anything(), "{effect:?}");
        assert_eq!(effect.mode, TextMode::Standard);
        assert_eq!(effect.general, effect.profile);
    }

    /// `raw` is the override users meet by surprise — the one that makes a
    /// correct-looking rule do nothing — so the preview has to make it
    /// impossible to miss.
    #[test]
    fn a_raw_profile_leaves_the_sample_untouched() {
        let effect = preview_effect(
            DEFAULT_SAMPLE,
            TextMode::Standard,
            false,
            crate::processing::formal::FormalOptions::default(),
            &Normalizer::new(),
            &Dictionary::with_defaults(),
            &Overrides {
                text_mode: Some("raw".into()),
                ..Default::default()
            },
        );
        assert_eq!(effect.mode, TextMode::Raw);
        assert_eq!(
            effect.profile, DEFAULT_SAMPLE,
            "raw types the sentence exactly as it was heard"
        );
        assert!(effect.changes_anything(), "{effect:?}");
    }

    /// A profile's own rules run **on top of** the general pipeline rather than
    /// instead of it. A preview that showed only the profile's rule would teach
    /// the user that giving an application a profile costs it the shared
    /// dictionary.
    #[test]
    fn a_profiles_own_rule_runs_on_top_of_the_general_rules() {
        // Both rule sets spelled out, so this asserts the *order* rather than
        // whatever the shipped dictionary happens to contain today.
        let dictionary = Dictionary::new(vec![Correction {
            from: "پاتون".into(),
            to: "PY".into(),
            category: None,
        }]);
        let effect = preview_effect(
            DEFAULT_SAMPLE,
            TextMode::Standard,
            false,
            crate::processing::formal::FormalOptions::default(),
            &Normalizer::new(),
            &dictionary,
            &Overrides {
                corrections: vec![Correction {
                    from: "کار".into(),
                    to: "Job".into(),
                    category: None,
                }],
                ..Default::default()
            },
        );
        assert!(effect.profile.contains("Job"), "{effect:?}");
        assert!(
            effect.profile.contains("PY"),
            "the general dictionary must still run under a profile: {effect:?}"
        );
        assert!(effect.general.contains("PY"), "{effect:?}");
        assert!(
            !effect.general.contains("Job"),
            "the profile's own rule must not appear without the profile: {effect:?}"
        );
        assert!(effect.changes_anything());
    }

    /// The dictionary is the **live** one, not a fresh read of the file: a rule
    /// that is matching dictations right now is already in both lines.
    #[test]
    fn the_preview_reads_the_dictionary_it_is_given() {
        let dictionary = Dictionary::new(vec![Correction {
            from: "پاتون".into(),
            to: "PY".into(),
            category: None,
        }]);
        let effect = preview_effect(
            DEFAULT_SAMPLE,
            TextMode::Standard,
            false,
            crate::processing::formal::FormalOptions::default(),
            &Normalizer::new(),
            &dictionary,
            &Overrides::default(),
        );
        assert!(effect.general.contains("PY"), "{effect:?}");
        assert_eq!(effect.general, effect.profile);
    }
}
