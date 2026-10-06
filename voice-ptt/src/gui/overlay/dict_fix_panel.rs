//! The quick dictionary fix: correct one misheard word, see what it will do,
//! then save it — into the general dictionary or into one application's profile.
//!
//! It sits at the top of the Dictionary tab rather than in a tab of its own. A
//! user who has just watched a word come out wrong goes to the dictionary, and
//! the long rule list below is what they are adding to; putting the two on
//! separate screens would mean the fix and the evidence for it could not be seen
//! together.
//!
//! # Where the word comes from
//!
//! From the user's own sentence. The sample box is the *selection* mechanism:
//! it is seeded with the line they picked in History, they select the word the
//! recogniser got wrong inside it, and one button lifts that selection into the
//! rule. Typing the word by hand is still possible, and the two are the same
//! field — a misheard word is usually one the user can see rather than spell.
//!
//! # What this panel may decide
//!
//! Nothing. Every judgement — is the rule savable, what does it change, what
//! should the user be told — is a value from [`crate::processing::quickfix`],
//! and the preview runs [`crate::processing::TextRules`], the same value the
//! coordinator applies to a real dictation. This module draws those values and
//! performs the two writes the user asked for.

use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_phosphor::regular as ic;

use super::text::{format_persian_display, persian_text_edit_layouter};
use super::theme::*;
use crate::config::settings::Settings;
use crate::processing::dictionary::Correction;
use crate::processing::quickfix::{self, Assessment, Candidate, NoEffect, Objection, Scope, Warning};
use crate::processing::{Dictionary, Normalizer, TextMode, TextRules};
use crate::profiles;

/// The quick-fix panel's private UI state.
///
/// `Default` is the empty form: every field's zero value is also its idle
/// value, so the panel opens with nothing to say rather than with a guess.
#[derive(Default)]
pub(crate) struct DictFixState {
    /// The word to correct.
    pub from: String,
    /// What should be typed instead.
    pub to: String,
    /// The profile the fix is aimed at; `None` is the general dictionary.
    pub profile: Option<usize>,
    /// The sentence the preview runs on.
    pub sample: String,
    /// Transient feedback, auto-expiring like the other panels'.
    msg: Option<(String, Instant)>,
    normalizer: Normalizer,
    cache: Option<Cached>,
}


impl DictFixState {
    /// Seeds the form with a sentence to fix, and optionally the word in it.
    ///
    /// Takes plain strings rather than another panel's request type: History's
    /// output is History's business, and the overlay — the one place that knows
    /// both panels — does the translating. Neither panel imports the other.
    ///
    /// The word is *offered*, not fixed: the user can edit it, and the sentence
    /// stays because it is the preview's subject — a rule shown against the
    /// sentence that went wrong is evidence, while the same rule shown against
    /// an invented example is a slogan.
    pub(crate) fn seed(&mut self, word: &str, sentence: &str) {
        self.from = word.trim().to_string();
        self.to.clear();
        self.sample = sentence.to_string();
        // Not this panel's business who called, but the cache is: the inputs it
        // was computed from have just changed.
        self.cache = None;
    }

    fn flash(&mut self, text: impl Into<String>) {
        self.msg = Some((text.into(), Instant::now()));
    }
}

/// What the preview depends on, hashed, so a panel redrawing at 60 Hz does not
/// rebuild an Aho-Corasick matcher 60 times a second.
#[derive(PartialEq, Eq)]
struct CacheKey(u64);

/// The computed answers for one set of inputs.
struct Cached {
    key: CacheKey,
    assessment: Assessment,
    preview: quickfix::Preview,
    no_effect: Option<NoEffect>,
    /// The mode the destination runs under — shown to the user, and the reason
    /// a general rule can do nothing.
    mode: TextMode,
}

/// The phrase the user selected inside the sample box, if any.
///
/// Char indices, in either order, trimmed, and `None` when the selection is
/// empty or blank — a selection of spaces is not a word.
fn selected_phrase(text: &str, primary: usize, secondary: usize) -> Option<String> {
    let (lo, hi) = if primary <= secondary {
        (primary, secondary)
    } else {
        (secondary, primary)
    };
    let phrase: String = text.chars().skip(lo).take(hi.saturating_sub(lo)).collect();
    let phrase = phrase.trim().to_string();
    (!phrase.is_empty()).then_some(phrase)
}

/// Everything the preview depends on, as one number.
///
/// Includes the dictionary and the profile rules by *content*, not by length: a
/// rule whose replacement is edited keeps the same count, and a preview that
/// then showed a stale result would be worse than a slow one.
fn cache_key(
    state: &DictFixState,
    draft: &Settings,
    dictionary: &Dictionary,
    corpus: &[String],
) -> CacheKey {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    state.from.hash(&mut hasher);
    state.to.hash(&mut hasher);
    state.profile.hash(&mut hasher);
    state.sample.hash(&mut hasher);
    draft.text.mode.hash(&mut hasher);
    dictionary.rules().len().hash(&mut hasher);
    for rule in dictionary.rules() {
        rule.from.hash(&mut hasher);
        rule.to.hash(&mut hasher);
    }
    for profile in draft.profiles.profiles() {
        profile.exe.hash(&mut hasher);
        profile.overrides.text_mode.hash(&mut hasher);
        for rule in &profile.overrides.corrections {
            rule.from.hash(&mut hasher);
            rule.to.hash(&mut hasher);
        }
    }
    // Not the whole corpus: the panel only needs its *words*, and a new
    // dictation adds a line, which is one more thing to hash.
    corpus.len().hash(&mut hasher);
    for line in corpus {
        line.hash(&mut hasher);
    }
    CacheKey(hasher.finish())
}

/// Recomputes the assessment and the preview for the current inputs.
fn compute(
    state: &DictFixState,
    draft: &Settings,
    dictionary: &Dictionary,
    corpus: &[String],
) -> Cached {
    let candidate = Candidate::new(&state.from, &state.to);
    let general_mode = draft.text.mode();

    let (mode, own_rules): (TextMode, &[Correction]) = match state.profile {
        Some(index) => match draft.profiles.profiles().get(index) {
            Some(profile) => (
                profiles::mode_for(&profile.overrides, general_mode),
                &profile.overrides.corrections,
            ),
            // A profile that vanished under the selection behaves like no
            // profile: the general rules, which is what the file would give.
            None => (general_mode, &[]),
        },
        None => (general_mode, &[]),
    };

    let siblings: &[Correction] = match state.profile {
        Some(index) => draft
            .profiles
            .profiles()
            .get(index)
            .map(|p| p.overrides.corrections.as_slice())
            .unwrap_or(&[]),
        None => dictionary.rules(),
    };
    let assessment = quickfix::assess(&candidate, siblings, corpus);

    // Skipped while the rule cannot exist: compiling a matcher for an empty
    // pattern set would produce a preview of nothing, which reads like a bug.
    if !assessment.is_savable() {
        return Cached {
            key: cache_key(state, draft, dictionary, corpus),
            assessment,
            preview: quickfix::Preview {
                before: state.sample.clone(),
                after: state.sample.clone(),
            },
            no_effect: None,
            mode,
        };
    }

    let before = TextRules {
        mode,
        normalizer: &state.normalizer,
        dictionary,
        corrections: own_rules,
    };

    let widened_general;
    let widened_own;
    let after = match state.profile {
        None => {
            widened_general = Dictionary::new(quickfix::with_candidate(dictionary.rules(), &candidate));
            TextRules {
                mode,
                normalizer: &state.normalizer,
                dictionary: &widened_general,
                corrections: own_rules,
            }
        }
        Some(_) => {
            widened_own = quickfix::with_candidate(own_rules, &candidate);
            TextRules {
                mode,
                normalizer: &state.normalizer,
                dictionary,
                corrections: &widened_own,
            }
        }
    };

    let preview = quickfix::preview(&state.sample, before, after);
    let scope = if state.profile.is_some() {
        Scope::Profile
    } else {
        Scope::General
    };
    let no_effect = quickfix::explain_no_effect(&preview, &state.sample, &candidate, scope, mode);

    Cached {
        key: cache_key(state, draft, dictionary, corpus),
        assessment,
        preview,
        no_effect,
        mode,
    }
}

/// Persian text for one warning.
fn describe(warning: &Warning) -> String {
    match warning {
        Warning::ReplacesRule { previous_to } => format!(
            "برای این واژه از قبل قاعده‌ای با معادل «{previous_to}» هست؛ ذخیره کردن، آن را جایگزین می‌کند."
        ),
        Warning::InsideWords { words } => format!(
            "این واژه جزئی از این واژه‌های متن شماست: {}. تطبیق، مرزِ واژه را رعایت می‌کند، پس آن‌ها دست‌نخورده می‌مانند.",
            words.join("، ")
        ),
        Warning::OverlapsRule { other_from } => format!(
            "قاعدهٔ «{other_from}» این واژه را در بر می‌گیرد؛ چون بلندتر است، در آن متن برنده می‌شود."
        ),
    }
}

/// Persian text for one objection.
fn refuse(objection: &Objection) -> String {
    match objection {
        Objection::EmptySide => "هر دو طرف قاعده لازم است: واژهٔ شنیده‌شده و معادل درست.".to_string(),
        Objection::IdenticalSides => "دو طرف قاعده یکسان است، پس قاعده هیچ کاری نمی‌کند.".to_string(),
    }
}

/// Renders the quick-fix card into the Dictionary tab.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    ui: &mut egui::Ui,
    state: &mut DictFixState,
    draft: &mut Settings,
    settings: &Arc<RwLock<Settings>>,
    config_path: &Path,
    dictionary: &Arc<RwLock<Dictionary>>,
    corpus: &[String],
) {
    let mut persian_layouter =
        |ui: &egui::Ui, text: &str, w: f32| persian_text_edit_layouter(ui, text, w);

    // One read for the whole card: the rules are needed for the assessment, the
    // preview and the "a rule for this word already exists" answer below.
    let rules_snapshot = dictionary
        .read()
        .map(|d| d.rules().to_vec())
        .unwrap_or_default();
    let probe = Dictionary::new(rules_snapshot.clone());

    let key = cache_key(state, draft, &probe, corpus);
    let fresh = state.cache.as_ref().map(|c| &c.key) != Some(&key);
    if fresh {
        state.cache = Some(compute(state, draft, &probe, corpus));
    }
    // Cloned out of the cache before the closure below, which needs `&mut state`
    // for its own buttons and feedback: the two borrows cannot coexist. What is
    // copied is small — on the ordinary path the warning list is empty and the
    // preview is two short strings.
    let (assessment, preview, no_effect, mode) = {
        let cached = state.cache.as_ref().expect("just computed");
        (
            cached.assessment.clone(),
            cached.preview.clone(),
            cached.no_effect,
            cached.mode,
        )
    };

    manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            egui::RichText::new(format!(
                "{}  {}",
                format_persian_display("اصلاح سریع واژه"),
                ic::MAGIC_WAND
            ))
            .size(12.5)
            .strong()
            .color(palette::TEXT_SECTION),
        );
        ui.add_space(4.0);

        if let Some((text, at)) = state.msg.clone() {
            if at.elapsed() < Duration::from_secs(6) {
                callout(ui, CalloutKind::Success, &text);
                ui.add_space(4.0);
            } else {
                state.msg = None;
            }
        }

        // ── the sentence, which is also the selection mechanism ───────────
        ui.label(
            egui::RichText::new(format_persian_display(
                "جمله‌ای که واژه در آن اشتباه شد را اینجا بگذارید، واژهٔ غلط را در آن انتخاب کنید:",
            ))
            .size(10.5)
            .color(palette::TEXT_LABEL),
        );
        ui.add_space(3.0);
        // `show` rather than `add`: the selection is the *input* here, and only
        // the full output carries the cursor range. A response alone would leave
        // the user with a sentence they cannot point at.
        let output = egui::TextEdit::singleline(&mut state.sample)
            .hint_text(format_persian_display("من با پاتون کار می‌کنم"))
            .desired_width(f32::INFINITY)
            .layouter(&mut persian_layouter)
            .show(ui);
        let selected = output.cursor_range.and_then(|range| {
            selected_phrase(
                &state.sample,
                range.primary.ccursor.index,
                range.secondary.ccursor.index,
            )
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let can_lift = selected.is_some();
            if ui
                .add_enabled(
                    can_lift,
                    egui::Button::new(
                        egui::RichText::new(format!(
                            "{}  {}",
                            format_persian_display("واژهٔ انتخاب‌شده"),
                            ic::ARROW_UP
                        ))
                        .size(10.5),
                    )
                    .fill(if can_lift {
                        palette::ACCENT_ACTION
                    } else {
                        palette::CHIP_BG
                    })
                    .rounding(egui::Rounding::same(6.0)),
                )
                .on_hover_text("Lift the word you selected in the sentence into the rule.")
                .clicked()
            {
                if let Some(phrase) = selected.clone() {
                    state.from = phrase;
                }
            }
            ui.label(
                egui::RichText::new(format_persian_display(
                    "یا واژه را دستی در کادر «از» بنویسید",
                ))
                .size(9.5)
                .color(palette::TEXT_FAINT),
            );
        });

        ui.add_space(6.0);

        // ── the rule itself ───────────────────────────────────────────────
        let label_w = 92.0;
        rtl_form_row(ui, "از (شنیده‌شده):", label_w, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.from)
                    .hint_text("پاتون")
                    .desired_width(150.0)
                    .layouter(&mut persian_layouter),
            );
        });
        ui.add_space(4.0);
        rtl_form_row(ui, "به (درست):", label_w, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.to)
                    .hint_text("پایتون")
                    .desired_width(150.0)
                    .layouter(&mut persian_layouter),
            );
        });
        ui.add_space(4.0);

        // ── where it is saved ─────────────────────────────────────────────
        let scope_text = match state.profile {
            None => format_persian_display("عمومی — همهٔ برنامه‌ها"),
            Some(_) => format_persian_display("یک برنامه (پروفایل)"),
        };
        rtl_form_row(ui, "محدوده:", label_w, |ui| {
            egui::ComboBox::from_id_source("dict_fix_scope")
                .selected_text(scope_text)
                .width(190.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(state.profile.is_none(), format_persian_display("عمومی — همهٔ برنامه‌ها"))
                        .clicked()
                    {
                        state.profile = None;
                    }
                    for (index, profile) in draft.profiles.profiles().iter().enumerate() {
                        let name = if profile.name.trim().is_empty() {
                            profile.exe.trim().to_string()
                        } else {
                            profile.name.trim().to_string()
                        };
                        if ui
                            .selectable_label(
                                state.profile == Some(index),
                                format_persian_display(&format!("فقط {name}")),
                            )
                            .clicked()
                        {
                            state.profile = Some(index);
                        }
                    }
                });
        });
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(format_persian_display(&format!(
                "این مقصد در حالت «{}» اجرا می‌شود",
                mode_label(mode)
            )))
            .size(9.5)
            .color(palette::TEXT_FAINT),
        );

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        // ── what it will do ───────────────────────────────────────────────
        ui.label(
            egui::RichText::new(format_persian_display("اثر پیش از ذخیره"))
                .size(10.5)
                .color(palette::TEXT_LABEL),
        );
        ui.add_space(3.0);
        // The two lines are printed in the reverse order on purpose: Persian
        // reads right to left, so the *before* line is the one on the right.
        preview_line(ui, "پیش از اصلاح", &preview.before, palette::TEXT_SECONDARY);
        preview_line(
            ui,
            "پس از اصلاح",
            &preview.after,
            if preview.changes_anything() {
                palette::SUCCESS
            } else {
                palette::TEXT_MUTED
            },
        );

        if let Some(reason) = no_effect {
            ui.add_space(4.0);
            let text = match reason {
                NoEffect::SampleDoesNotContainTheWord => format_persian_display(&format!(
                    "واژهٔ «{}» در جملهٔ نمونه نیست، پس این پیش‌نمایش دربارهٔ قاعده چیزی نمی‌گوید. جملهٔ دیگری را بگذارید یا واژه را در همین جمله درست بنویسید.",
                    state.from.trim()
                )),
                NoEffect::RawDestinationIgnoresGeneral => format_persian_display(
                    "این برنامه در حالت خام است و واژه‌نامهٔ عمومی روی آن اجرا نمی‌شود. \
                     برای اثر کردن، محدوده را روی همان پروفایل بگذارید.",
                ),
                NoEffect::AlreadyCorrect => format_persian_display(
                    "متن نمونه از قبل همین را می‌دهد؛ قاعده لازم نیست.",
                ),
            };
            callout(ui, CalloutKind::Info, &text);
        }

        for warning in &assessment.warnings {
            ui.add_space(4.0);
            callout(ui, CalloutKind::Warning, &format_persian_display(&describe(warning)));
        }
        for objection in &assessment.objections {
            ui.add_space(4.0);
            callout(ui, CalloutKind::Warning, &format_persian_display(&refuse(objection)));
        }

        ui.add_space(6.0);

        // ── save / delete ─────────────────────────────────────────────────
        let exists = match state.profile {
            None => rules_snapshot
                .iter()
                .any(|r| r.from.trim() == state.from.trim()),
            Some(index) => draft
                .profiles
                .profiles()
                .get(index)
                .is_some_and(|p| {
                    p.overrides
                        .corrections
                        .iter()
                        .any(|r| r.from.trim() == state.from.trim())
                }),
        };

        ui.horizontal(|ui| {
            let can_save = assessment.is_savable();
            let save = ui.add_enabled(
                can_save,
                egui::Button::new(
                    egui::RichText::new(format!(
                        "{}  {}",
                        format_persian_display("ذخیره اصلاح"),
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
                let (from, to) = Candidate::new(&state.from, &state.to).trimmed();
                match state.profile {
                    None => {
                        if let Ok(mut dict) = dictionary.write() {
                            dict.add_rule(from.clone(), to.clone(), Some("quickfix".into()));
                            match dict.save_to_file() {
                                Ok(()) => {
                                    state.flash(format!("«{from}» → «{to}» در واژه‌نامهٔ عمومی ذخیره شد"))
                                }
                                Err(e) => state.flash(format!("ذخیره نشد: {e}")),
                            }
                        }
                    }
                    Some(index) => {
                        if let Some(profile) = draft.profiles.get_mut(index) {
                            // Same rule as the general dictionary: an existing
                            // rule for this word is replaced, never duplicated.
                            profile
                                .overrides
                                .corrections
                                .retain(|r| r.from.trim() != from);
                            profile.overrides.corrections.push(Correction {
                                from: from.clone(),
                                to: to.clone(),
                                category: Some("quickfix".into()),
                            });
                        }
                        match save_profile_fix(draft, settings, config_path) {
                            Ok(()) => state.flash(format!(
                                "«{from}» → «{to}» برای این برنامه ذخیره شد"
                            )),
                            Err(e) => state.flash(format!("ذخیره نشد: {e}")),
                        }
                    }
                }
                // The dictionary just changed under the cache.
                state.cache = None;
            }

            if exists
                && ui
                    .button(
                        egui::RichText::new(format!(
                            "{}  {}",
                            format_persian_display("حذف قاعده"),
                            ic::TRASH
                        ))
                        .size(10.5),
                    )
                    .clicked()
            {
                let from = state.from.trim().to_string();
                match state.profile {
                    None => {
                        if let Ok(mut dict) = dictionary.write() {
                            dict.remove_by_from(&from);
                            match dict.save_to_file() {
                                Ok(()) => state.flash(format!("قاعدهٔ «{from}» حذف شد")),
                                Err(e) => state.flash(format!("ذخیره نشد: {e}")),
                            }
                        }
                    }
                    Some(index) => {
                        if let Some(profile) = draft.profiles.get_mut(index) {
                            profile
                                .overrides
                                .corrections
                                .retain(|r| r.from.trim() != from);
                        }
                        match save_profile_fix(draft, settings, config_path) {
                            Ok(()) => state.flash(format!("قاعدهٔ «{from}» حذف شد")),
                            Err(e) => state.flash(format!("ذخیره نشد: {e}")),
                        }
                    }
                }
                state.cache = None;
            }

            if ui
                .button(
                    egui::RichText::new(format!(
                        "{}  {}",
                        format_persian_display("پاک کردن فرم"),
                        ic::BROOM
                    ))
                    .size(10.5),
                )
                .clicked()
            {
                state.from.clear();
                state.to.clear();
                state.msg = None;
            }
        });
    });
}

/// Writes the profile-scoped fix, through the same validated path the profiles
/// panel uses.
///
/// The whole draft is written, not just the one rule: the draft is the only
/// copy of the user's unsaved edits, and writing a rule out of it while leaving
/// the rest behind would mean saving the fix silently discarded whatever else
/// they had changed.
fn save_profile_fix(
    draft: &Settings,
    settings: &Arc<RwLock<Settings>>,
    config_path: &Path,
) -> Result<(), String> {
    super::validate_settings(draft)?;
    super::profiles_panel::validate_set(&draft.profiles)?;
    let mut live = settings.write().map_err(|_| "قفل تنظیمات شکست".to_string())?;
    *live = draft.clone();
    live.save(config_path).map_err(|e| e.to_string())
}


#[cfg(test)]
mod tests {
    use super::*;

    // ── lifting the selection out of the sentence ─────────────────────────

    #[test]
    fn the_selected_phrase_is_the_text_between_the_cursors() {
        // `من با پاتون کار می‌کنم` — `پاتون` is char indices 6..11, which is
        // what egui reports for a mouse selection over the word.
        let text = "من با پاتون کار می‌کنم";
        assert_eq!(
            selected_phrase(text, 6, 11).as_deref(),
            Some("پاتون"),
            "the selection is what the user pointed at"
        );
    }

    #[test]
    fn a_backwards_selection_gives_the_same_phrase() {
        let text = "من با پاتون کار می‌کنم";
        assert_eq!(selected_phrase(text, 11, 6), selected_phrase(text, 6, 11));
    }

    #[test]
    fn a_collapsed_or_blank_selection_is_not_a_word() {
        assert_eq!(selected_phrase("من با پاتون", 4, 4), None, "a caret is not a selection");
        // Char 2 of that sentence is the space between `من` and `با`.
        assert_eq!(selected_phrase("من با پاتون", 2, 3), None, "a space is not a word");
        assert_eq!(selected_phrase("من با پاتون", 0, 0), None);
    }

    #[test]
    fn a_selection_running_past_the_end_is_clamped_not_panicking() {
        let text = "کوتاه";
        assert_eq!(selected_phrase(text, 2, 99).as_deref(), Some("تاه"));
    }

    // ── the cache key ─────────────────────────────────────────────────────

    /// A state whose sample *contains* the word being fixed — otherwise every
    /// preview below would legitimately show no change and prove nothing.
    fn state_from(from: &str, to: &str) -> DictFixState {
        DictFixState {
            from: from.into(),
            to: to.into(),
            sample: format!("این {from} کند است"),
            ..DictFixState::default()
        }
    }

    /// Editing a replacement keeps the rule *count* the same, so a key built
    /// from the count alone would show a stale preview of the previous text.
    #[test]
    fn the_key_notices_an_edited_replacement() {
        let draft = Settings::default();
        let state = state_from("پاتون", "پایتون");
        let before = Dictionary::new(vec![Correction {
            from: "کوئری".into(),
            to: "Query".into(),
            category: None,
        }]);
        let after = Dictionary::new(vec![Correction {
            from: "کوئری".into(),
            to: "Request".into(),
            category: None,
        }]);
        assert_ne!(
            cache_key(&state, &draft, &before, &[]).0,
            cache_key(&state, &draft, &after, &[]).0
        );
    }

    #[test]
    fn the_key_notices_a_new_sentence_in_the_corpus() {
        let draft = Settings::default();
        let state = state_from("پاتون", "پایتون");
        let dictionary = Dictionary::new(vec![]);
        assert_ne!(
            cache_key(&state, &draft, &dictionary, &[]).0,
            cache_key(&state, &draft, &dictionary, &["یک خط".to_string()]).0
        );
    }

    /// …and is stable when nothing the preview depends on has changed: a key
    /// that changed every frame would recompute the matcher every frame, which
    /// is the whole reason it exists.
    #[test]
    fn the_key_is_stable_for_unchanged_inputs() {
        let draft = Settings::default();
        let state = state_from("پاتون", "پایتون");
        let dictionary = Dictionary::new(vec![Correction {
            from: "کوئری".into(),
            to: "Query".into(),
            category: None,
        }]);
        let corpus = vec!["این نیست".to_string()];
        assert_eq!(
            cache_key(&state, &draft, &dictionary, &corpus).0,
            cache_key(&state, &draft, &dictionary, &corpus).0
        );
    }

    // ── what the panel computes for a scope ───────────────────────────────

    /// A general fix rides the automatic pipeline; the preview says so, and the
    /// general mode is what decides it.
    #[test]
    fn a_general_fix_previews_against_the_general_mode() {
        let draft = Settings::default();
        let state = state_from("کوئری", "Query");
        let dictionary = Dictionary::new(vec![]);
        let cached = compute(&state, &draft, &dictionary, &[]);
        assert_eq!(cached.mode, draft.text.mode());
        assert!(cached.preview.changes_anything());
        assert_eq!(cached.no_effect, None);
    }

    /// The destination's profile decides the mode, so the preview runs the mode
    /// the application actually uses rather than the general one.
    #[test]
    fn a_profile_fix_previews_against_that_profiles_mode() {
        let mut draft = Settings::default();
        draft.text.mode = "standard".into();
        draft.profiles = profiles::ProfileSet::new(vec![profiles::AppProfile::new(
            "Terminal",
            "wt.exe",
            profiles::Overrides {
                text_mode: Some("raw".into()),
                ..Default::default()
            },
        )]);

        let mut state = state_from("کوئری", "Query");
        state.profile = Some(0);
        let dictionary = Dictionary::new(vec![]);
        let cached = compute(&state, &draft, &dictionary, &[]);

        assert_eq!(cached.mode, TextMode::Raw, "the profile's own mode must win");
        // The profile's own rule still applies under raw, so the fix works.
        assert!(cached.preview.changes_anything(), "{:?}", cached.preview);
        assert_eq!(cached.no_effect, None);
    }

    /// A profile that vanished under the selection falls back to the general
    /// rules instead of panicking or previewing nothing.
    #[test]
    fn a_vanished_profile_falls_back_to_the_general_rules() {
        let draft = Settings::default();
        let mut state = state_from("کوئری", "Query");
        state.profile = Some(7);
        let dictionary = Dictionary::new(vec![]);
        let cached = compute(&state, &draft, &dictionary, &[]);
        assert_eq!(cached.mode, draft.text.mode());
        assert!(cached.preview.changes_anything());
    }

    /// A rule that cannot be saved shows the sample unchanged rather than a
    /// half-applied preview: an empty pattern set corrects nothing, and showing
    /// that as "the effect" would be a lie about the product.
    #[test]
    fn an_unsavable_rule_previews_nothing() {
        let draft = Settings::default();
        let state = state_from("", "");
        let dictionary = Dictionary::new(vec![]);
        let cached = compute(&state, &draft, &dictionary, &[]);
        assert!(!cached.assessment.is_savable());
        assert_eq!(cached.preview.before, cached.preview.after);
        assert_eq!(cached.preview.before, state.sample);
    }

    /// The healthy-word warning reaches the panel with the words named, which is
    /// the whole point of collecting the corpus.
    #[test]
    fn the_assessment_the_panel_shows_names_the_endangered_words() {
        let draft = Settings::default();
        let state = state_from("نیس", "NACE");
        let dictionary = Dictionary::new(vec![]);
        let corpus = vec!["این نیست".to_string()];
        let cached = compute(&state, &draft, &dictionary, &corpus);
        assert!(
            cached
                .assessment
                .warnings
                .iter()
                .any(|w| matches!(w, Warning::InsideWords { words } if words == &["نیست".to_string()])),
            "{:?}",
            cached.assessment.warnings
        );
    }
}
