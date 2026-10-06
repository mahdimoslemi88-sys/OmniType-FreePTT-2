//! The decisions behind the quick dictionary fix: is this rule safe, what will
//! it change, and what should the user be told before they save it.
//!
//! The panel that shows this (`crate::gui::overlay::dict_fix_panel`) draws
//! values from here and decides nothing itself, for the same reason
//! [`crate::profiles`] exists: the interesting cases are the ones a user meets
//! once — a rule that eats a healthy word, a rule that silently replaces one
//! they wrote last month, a rule that changes nothing because the application
//! it is aimed at runs in `raw` mode — and each of them has exactly one right
//! answer.
//!
//! # What is deliberately not here
//!
//! * **Learning.** Nothing in this module adds a rule on its own. The roadmap
//!   forbids unauthorised automatic learning outright: a fix exists because the
//!   user typed one, saw what it would do, and saved it.
//! * **Deciding the scope.** Whether a fix belongs to the general dictionary or
//!   to one application's profile is the user's choice, made in the panel; this
//!   module only says what the consequence of each choice is.
//! * **Writing.** Saving a general rule is [`crate::processing::Dictionary`]'s
//!   job and saving a profile rule is the settings' job. Here it is all values.

use crate::processing::dictionary::Correction;
use crate::processing::TextRules;

/// One fix the user is about to save.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Candidate {
    /// What the recogniser produced — the word to correct.
    pub from: String,
    /// What should be typed instead.
    pub to: String,
}

impl Candidate {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }

    /// The trimmed pair the rule would actually be built from.
    ///
    /// Trimming here rather than at each comparison is what keeps the panel's
    /// preview and the saved rule talking about the same two strings.
    pub fn trimmed(&self) -> (String, String) {
        (self.from.trim().to_string(), self.to.trim().to_string())
    }
}

/// Which rule set a fix is saved into.
///
/// Not a preference: the two have different *effects*, and the difference is
/// what the preview exists to show. A general rule rides the automatic pipeline
/// and therefore does nothing in a window whose profile asks for `raw`, while a
/// profile's own rules run in every mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The shared dictionary file — every application, except where a profile
    /// turns the automatic pipeline off.
    General,
    /// One application's profile rules.
    Profile,
}

/// Why a fix cannot be saved at all.
///
/// Kept separate from [`Warning`] because these are not judgements the user can
/// overrule: a rule with one empty side does nothing, and a rule that maps a
/// word to itself is not a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Objection {
    /// One side is blank.
    EmptySide,
    /// Both sides are the same word, so the rule would be dropped by
    /// [`crate::processing::Dictionary::new`] anyway.
    IdenticalSides,
}

/// Something worth saying, which does not stop the user saving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// A rule for this word already exists with a different replacement.
    /// Saving replaces it — which is a decision, not a detail.
    ReplacesRule { previous_to: String },
    /// The word appears *inside* a longer word in the user's own text, so a
    /// rule on it is one the matching has to keep off those words. Listed
    /// because the user should see which words are at stake.
    InsideWords { words: Vec<String> },
    /// Another rule's left-hand side swallows this one **as a fragment**: one
    /// of the two is glued to a word character inside the other (`پاتو` inside
    /// `پاتون`). The longer side wins, so the shorter rule never sees that
    /// word — which is the opposite of what the user who added it expected.
    ///
    /// Not raised for a whole word inside a phrase (`اسکریپت` inside
    /// `جاوا اسکریپت`): there both rules fire, on different text.
    OverlapsRule { other_from: String },
}

/// Everything that can be said about a candidate before it is saved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Assessment {
    /// Blocks the save button.
    pub objections: Vec<Objection>,
    /// Shown beside it.
    pub warnings: Vec<Warning>,
}

impl Assessment {
    pub fn is_savable(&self) -> bool {
        self.objections.is_empty()
    }
}

/// How many words a warning will list before it stops naming them.
///
/// A cap rather than a summary: "and 40 more" tells the user nothing they can
/// act on, and a wall of words buries the first three, which are the ones they
/// recognise.
const MAX_NAMED_WORDS: usize = 5;

/// Judges `candidate` against the rules it will join and the user's own text.
///
/// `siblings` are the rules of the scope being written to — the general
/// dictionary's rules, or the profile's — because a clash only matters between
/// rules that run together. `corpus` is text the user actually dictated (their
/// history, or the phrase they selected); it is what turns "this word is short"
/// into "this word is inside `نیست`, `نیستم`, `نیستند`".
pub fn assess(candidate: &Candidate, siblings: &[Correction], corpus: &[String]) -> Assessment {
    let (from, to) = candidate.trimmed();
    let mut objections = Vec::new();
    if from.is_empty() || to.is_empty() {
        objections.push(Objection::EmptySide);
    }
    if !from.is_empty() && from == to {
        objections.push(Objection::IdenticalSides);
    }
    if !objections.is_empty() {
        // Nothing below can be said usefully about a rule that will not exist.
        return Assessment {
            objections,
            warnings: Vec::new(),
        };
    }

    let mut warnings = Vec::new();

    if let Some(existing) = siblings.iter().find(|r| r.from.trim() == from) {
        if existing.to.trim() != to {
            warnings.push(Warning::ReplacesRule {
                previous_to: existing.to.trim().to_string(),
            });
        }
    }

    // Fragment overlap only — a whole word inside a phrase is the ordinary case
    // for split-word rules (`اسکریپت` next to `جاوا اسکریپت`) and warning about
    // it would put a warning on half the list a user builds next to the seed.
    for other in siblings {
        let other_from = other.from.trim();
        if other_from.is_empty() || other_from == from {
            continue;
        }
        if occurs_as_fragment(other_from, &from) || occurs_as_fragment(&from, other_from) {
            warnings.push(Warning::OverlapsRule {
                other_from: other_from.to_string(),
            });
        }
    }

    let words = healthy_words(corpus, &from);
    if !words.is_empty() {
        warnings.push(Warning::InsideWords { words });
    }

    Assessment {
        objections,
        warnings,
    }
}

/// The words in the user's own text that *contain* `needle` as a proper part.
///
/// These are the words a rule on `needle` has to leave alone, and the reason the
/// matcher is word-aware: with a bare find/replace, the seed's `نیس` → `NACE`
/// typed `NACEت` for the ordinary word `نیست`.
///
/// Whole words only — an occurrence that *is* `needle` is the thing being
/// corrected, not a casualty — and deduplicated, because a word the user says
/// five times is one word to look at, not five.
pub fn healthy_words(corpus: &[String], needle: &str) -> Vec<String> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for text in corpus {
        for word in split_words(text) {
            if word == needle || !word.contains(needle) {
                continue;
            }
            if !out.iter().any(|seen| seen == word) {
                out.push(word.to_string());
                if out.len() >= MAX_NAMED_WORDS {
                    return out;
                }
            }
        }
    }
    out
}

/// Whether `inner` occurs inside `outer` glued to a word character — a fragment
/// of a word rather than a word of its own.
///
/// This is the difference between the two ways one rule can hide another:
/// `پاتو` in `پاتون` is a fragment, so `پاتون` always wins and `پاتو` never
/// applies there — while `اسکریپت` in `جاوا اسکریپت` is a whole word of a
/// phrase, and both rules have text they can match.
fn occurs_as_fragment(outer: &str, inner: &str) -> bool {
    if inner.is_empty() {
        return false;
    }
    let mut searched = 0;
    while let Some(offset) = outer[searched..].find(inner) {
        let start = searched + offset;
        let end = start + inner.len();
        let before = outer[..start].chars().next_back();
        let after = outer[end..].chars().next();
        if before.is_some_and(is_word_char) || after.is_some_and(is_word_char) {
            return true;
        }
        searched = end;
    }
    false
}

/// Splits text into word-ish runs.
///
/// Boundaries are the things that are not part of a word: whitespace and
/// punctuation. Deliberately the same rule the matcher uses (see
/// `processing::dictionary::joins_words`) stated once more in the direction a
/// panel needs — this one *finds* the words, the matcher *refuses to cut* them.
fn split_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || !is_word_char(c))
        .filter(|w| !w.is_empty())
}

/// Whether `c` is part of a word rather than a separator.
///
/// Letters and digits, plus the ZWNJ that Persian writes inside words and the
/// underscore that identifiers use. Combining marks are attached to the word
/// they sit on, so they belong to it: `خِ` is one word, not two.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\u{200c}' || is_combining_mark(c)
}

/// The Arabic/Persian harakat and the generic combining diacriticals.
fn is_combining_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036f}'
        | '\u{0610}'..='\u{061a}'
        | '\u{064b}'..='\u{065f}'
        | '\u{0670}'
        | '\u{06d6}'..='\u{06dc}'
        | '\u{06df}'..='\u{06e4}'
        | '\u{06e7}'..='\u{06e8}'
        | '\u{06ea}'..='\u{06ed}'
    )
}

/// `rules` with `candidate` applied — replacing an existing rule for the same
/// word rather than adding a second one.
///
/// Replacing is what [`crate::processing::Dictionary::add_rule`] does, so a
/// preview built this way cannot show a different rule set from the one that
/// would be saved.
pub fn with_candidate(rules: &[Correction], candidate: &Candidate) -> Vec<Correction> {
    let (from, to) = candidate.trimmed();
    let mut out: Vec<Correction> = rules
        .iter()
        .filter(|r| r.from.trim() != from)
        .cloned()
        .collect();
    out.push(Correction {
        from,
        to,
        category: Some("quickfix".to_string()),
    });
    out
}

/// What the pipeline does to `sample` today, and what it would do once the
/// candidate is saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub before: String,
    pub after: String,
}

impl Preview {
    /// Whether the rule changes anything at all for this destination.
    pub fn changes_anything(&self) -> bool {
        self.before != self.after
    }
}

/// Runs both rule sets over one sample.
///
/// Both sides go through [`TextRules::apply`] — the same value the coordinator
/// applies to real dictations — so \"before\" is genuinely what the user would
/// get now, not a paraphrase of it.
pub fn preview(sample: &str, before: TextRules<'_>, after: TextRules<'_>) -> Preview {
    Preview {
        before: before.apply(sample),
        after: after.apply(sample),
    }
}

/// Why a saved rule would change nothing for the destination being previewed.
///
/// The case that matters is the first: a general rule is part of the automatic
/// pipeline, and an application whose profile asks for `raw` has switched that
/// pipeline off — so a correct-looking fix silently does nothing there. That is
/// a real answer to \"why did my rule not work\", and the panel can only give it
/// if this module names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoEffect {
    /// The sample contains neither the word the rule is about nor the text the
    /// rule would produce, so the preview says nothing about the rule either
    /// way — the sample is what needs changing, not the rule.
    SampleDoesNotContainTheWord,
    /// The destination runs `raw`, so the general dictionary never runs.
    RawDestinationIgnoresGeneral,
    /// The text already comes out this way — the rule is redundant, not wrong.
    AlreadyCorrect,
}

/// Names why the preview shows no change, if it does not.
///
/// One answer, not a list, and the order is deliberate: "your sample does not
/// contain this word" outranks "this destination ignores general rules",
/// because the first is a defect in the preview the user can fix in place while
/// the second is a fact about the destination that reappears on the next redraw.
/// Reporting the surprising one first would let it explain away a sample that
/// demonstrated nothing.
pub fn explain_no_effect(
    preview_of: &Preview,
    sample: &str,
    candidate: &Candidate,
    scope: Scope,
    destination_mode: crate::processing::TextMode,
) -> Option<NoEffect> {
    if preview_of.changes_anything() {
        return None;
    }
    let (from, to) = candidate.trimmed();
    // The sample can demonstrate the rule two ways: it contains the word the
    // rule is about, or — when the rule already exists — it already reads the
    // way the rule makes it read. Only a sample that shows *neither* side has
    // nothing to say, and only then is "already correct" an answer worth
    // withholding in favour of "the sample is the wrong one".
    let mentions_word = !from.is_empty() && sample.contains(&from);
    let shows_result = !to.is_empty() && sample.contains(&to);
    if !mentions_word && !shows_result {
        return Some(NoEffect::SampleDoesNotContainTheWord);
    }
    if scope == Scope::General && destination_mode == crate::processing::TextMode::Raw {
        return Some(NoEffect::RawDestinationIgnoresGeneral);
    }
    Some(NoEffect::AlreadyCorrect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::{Dictionary, Normalizer, TextMode};

    fn rule(from: &str, to: &str) -> Correction {
        Correction {
            from: from.into(),
            to: to.into(),
            category: None,
        }
    }

    fn corpus(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    // ── what blocks a save ────────────────────────────────────────────────

    #[test]
    fn a_rule_with_a_blank_side_is_refused() {
        for (from, to) in [("", "پایتون"), ("پاتون", ""), ("  ", "x"), ("x", "  ")] {
            let a = assess(&Candidate::new(from, to), &[], &[]);
            assert_eq!(a.objections, vec![Objection::EmptySide], "{from:?}->{to:?}");
            assert!(!a.is_savable());
        }
    }

    #[test]
    fn a_rule_that_maps_a_word_to_itself_is_refused() {
        // Surrounding space must not make a self-rule look like a real one: the
        // trimmed pair is what would be saved.
        let a = assess(&Candidate::new("  پاتون  ", "پاتون"), &[], &[]);
        assert_eq!(a.objections, vec![Objection::IdenticalSides]);
    }

    #[test]
    fn an_objection_suppresses_the_advice_about_a_rule_that_cannot_exist() {
        let a = assess(&Candidate::new("", ""), &[rule("پاتون", "پایتون")], &corpus(&["پاتون"]));
        assert!(a.warnings.is_empty());
    }

    // ── conflicts ─────────────────────────────────────────────────────────

    /// Saving a word that already has a rule overwrites that rule. That is
    /// allowed — it is how a user changes their mind — but it must be said out
    /// loud, with the old replacement, because the old rule is gone afterwards.
    #[test]
    fn an_existing_rule_with_a_different_replacement_is_reported() {
        let siblings = vec![rule("پاتون", "پایتون")];
        let a = assess(&Candidate::new("پاتون", "Python"), &siblings, &[]);
        assert!(a.is_savable());
        assert_eq!(
            a.warnings,
            vec![Warning::ReplacesRule {
                previous_to: "پایتون".into()
            }]
        );
    }

    /// Re-saving the same rule is not a conflict, and warning about it would
    /// teach the user to ignore the warning.
    #[test]
    fn the_same_rule_again_is_not_a_conflict() {
        let siblings = vec![rule("پاتون", "پایتون")];
        let a = assess(&Candidate::new("پاتون", "پایتون"), &siblings, &[]);
        assert!(a.warnings.is_empty(), "{:?}", a.warnings);
    }

    /// Two rules that can match the same span: the longer side wins, so one of
    /// them is either useless or is about to become useless. The panel names
    /// the other rule rather than deciding.
    #[test]
    fn overlapping_rules_are_reported_in_both_directions() {
        let shorter_exists = vec![rule("پاتو", "Pato")];
        let a = assess(&Candidate::new("پاتون", "پایتون"), &shorter_exists, &[]);
        assert_eq!(
            a.warnings,
            vec![Warning::OverlapsRule {
                other_from: "پاتو".into()
            }]
        );

        let longer_exists = vec![rule("پاتون", "پایتون")];
        let b = assess(&Candidate::new("پاتو", "Pato"), &longer_exists, &[]);
        assert_eq!(
            b.warnings,
            vec![Warning::OverlapsRule {
                other_from: "پاتون".into()
            }]
        );
    }

    /// A phrase rule for split words is not an overlap with one of its words:
    /// the two are about different spans, and reporting it would put a warning
    /// on almost every rule a user adds next to the seed list.
    #[test]
    fn a_phrase_rule_is_not_reported_as_an_overlap() {
        let siblings = vec![rule("جاوا اسکریپت", "جاوااسکریپت")];
        let a = assess(&Candidate::new("اسکریپت", "Script"), &siblings, &[]);
        assert!(a.warnings.is_empty(), "{:?}", a.warnings);
    }

    // ── the criterion that names this feature ─────────────────────────────

    /// The healthy-word check, run against the user's own history: the words a
    /// rule on `نیس` would sit inside of.
    #[test]
    fn words_that_merely_contain_the_word_are_reported() {
        let a = assess(
            &Candidate::new("نیس", "NACE"),
            &[],
            &corpus(&["این نیست", "او هم نیستم", "نیس درست است"]),
        );
        assert_eq!(
            a.warnings,
            vec![Warning::InsideWords {
                words: vec!["نیست".into(), "نیستم".into()],
            }],
            "the word itself is not a casualty, and neither are repeats"
        );
    }

    #[test]
    fn a_word_that_appears_once_is_reported_once() {
        let words = healthy_words(&corpus(&["نیست و نیست و نیست", "باز هم نیست"]), "نیس");
        assert_eq!(words, vec!["نیست".to_string()]);
    }

    /// The list is capped, and the cap is real: a warning that names forty words
    /// is a warning nobody reads.
    #[test]
    fn the_named_words_are_capped() {
        let line: Vec<String> = (0..20).map(|i| format!("نیس{i}")).collect();
        let words = healthy_words(&line, "نیس");
        assert_eq!(words.len(), MAX_NAMED_WORDS);
    }

    /// Punctuation is not part of a word: a word at the end of a sentence is
    /// still that word.
    #[test]
    fn punctuation_does_not_hide_a_word() {
        let words = healthy_words(&corpus(&["او نیست، من هم نیستم."]), "نیس");
        assert_eq!(words, vec!["نیست".to_string(), "نیستم".to_string()]);
    }

    /// A ZWNJ is inside a word, so a rule must not be shown as safe merely
    /// because the text is joined with a half-space.
    #[test]
    fn a_half_space_is_not_a_word_boundary() {
        let words = healthy_words(&corpus(&["می‌نیستی"]), "نیس");
        assert_eq!(words, vec!["می‌نیستی".to_string()]);
    }

    // ── building the rule set the preview runs ────────────────────────────

    #[test]
    fn the_candidate_replaces_a_rule_for_the_same_word() {
        let existing = vec![rule("کوئری", "Query"), rule("پاتون", "پایتون")];
        let after = with_candidate(&existing, &Candidate::new("کوِئری", "Request"));
        assert_eq!(after.len(), 3);
        let after = with_candidate(&existing, &Candidate::new("کوئری", "Request"));
        assert_eq!(after.len(), 2, "the old rule for the same word must go");
        assert!(after.iter().any(|r| r.to == "Request"));
        assert!(!after.iter().any(|r| r.to == "Query"));
    }

    #[test]
    fn trimming_is_applied_to_the_saved_rule() {
        let after = with_candidate(&[], &Candidate::new("  پاتون  ", "  پایتون "));
        assert_eq!(after[0].from, "پاتون");
        assert_eq!(after[0].to, "پایتون");
    }

    // ── the preview runs the real pipeline ────────────────────────────────

    fn pipeline<'a>(
        mode: TextMode,
        normalizer: &'a Normalizer,
        dictionary: &'a Dictionary,
        corrections: &'a [Correction],
    ) -> TextRules<'a> {
        TextRules {
            mode,
            normalizer,
            dictionary,
            corrections,
            // The preview's subject is a rule, not the command switch; off
            // keeps this harness about the dictionary it is testing.
            commands: false,
            formal: crate::processing::formal::FormalOptions::default(),
        }
    }

    /// A general fix, seen from an ordinary window: the misheard word is
    /// corrected in the sample.
    #[test]
    fn the_preview_shows_a_general_fix_taking_effect() {
        let normalizer = Normalizer::new();
        let dictionary = Dictionary::new(vec![rule("پاتون", "پایتون")]);
        let sample = "من با پاتون کار می‌کنم";

        let before = pipeline(TextMode::Standard, &normalizer, &dictionary, &[]);
        let widened = with_candidate(dictionary.rules(), &Candidate::new("کوئری", "Query"));
        let with_fix = Dictionary::new(widened);
        let after = pipeline(TextMode::Standard, &normalizer, &with_fix, &[]);

        let seen = preview(sample, before, after);
        assert_eq!(seen.before, "من با پایتون کار می‌کنم");
        assert_eq!(seen.after, seen.before, "a rule for another word changes nothing here");

        let sample2 = "این کوئری کند است";
        let seen2 = preview(
            sample2,
            pipeline(TextMode::Standard, &normalizer, &dictionary, &[]),
            pipeline(TextMode::Standard, &normalizer, &with_fix, &[]),
        );
        assert!(seen2.changes_anything());
        assert!(seen2.after.contains("Query"), "{seen2:?}");
    }

    /// The precedence case the panel has to be able to explain: a general rule
    /// does nothing in a window whose profile says `raw`, because `raw` switches
    /// the whole automatic pipeline off. A preview that hid this would send the
    /// user off to edit a rule that was never going to run.
    #[test]
    fn a_general_fix_does_nothing_in_a_raw_destination() {
        let normalizer = Normalizer::new();
        let dictionary = Dictionary::new(vec![rule("پاتون", "پایتون")]);
        let widened = Dictionary::new(with_candidate(
            dictionary.rules(),
            &Candidate::new("کوئری", "Query"),
        ));
        let sample = "این کوئری کند است";

        let seen = preview(
            sample,
            pipeline(TextMode::Raw, &normalizer, &dictionary, &[]),
            pipeline(TextMode::Raw, &normalizer, &widened, &[]),
        );
        assert_eq!(seen.before, sample, "raw types the sample untouched");
        assert!(!seen.changes_anything());
        assert_eq!(
            explain_no_effect(
                &seen,
                sample,
                &Candidate::new("کوئری", "Query"),
                Scope::General,
                TextMode::Raw
            ),
            Some(NoEffect::RawDestinationIgnoresGeneral)
        );
    }

    /// …and the destination's *own* rules do run in raw mode, because they are
    /// the user's explicit instruction for that application rather than part of
    /// the automatic pipeline. So the same fix works when saved to the profile.
    #[test]
    fn a_profile_fix_takes_effect_even_in_a_raw_destination() {
        let normalizer = Normalizer::new();
        let dictionary = Dictionary::new(vec![rule("پاتون", "پایتون")]);
        let sample = "این کوئری کند است";

        let before = pipeline(TextMode::Raw, &normalizer, &dictionary, &[]);
        let own = with_candidate(&[], &Candidate::new("کوئری", "Query"));
        let after = pipeline(TextMode::Raw, &normalizer, &dictionary, &own);

        let seen = preview(sample, before, after);
        assert!(seen.changes_anything(), "{seen:?}");
        assert!(seen.after.contains("Query"), "{seen:?}");
        assert_eq!(
            explain_no_effect(
                &seen,
                sample,
                &Candidate::new("کوئری", "Query"),
                Scope::Profile,
                TextMode::Raw
            ),
            None,
            "a profile fix that worked must not be explained away"
        );
    }

    /// A redundant rule is called redundant, not broken.
    #[test]
    fn a_rule_the_text_already_satisfies_is_named_as_redundant() {
        let normalizer = Normalizer::new();
        let dictionary = Dictionary::new(vec![rule("پاتون", "پایتون")]);
        // The sample already reads the way the rule would make it read — the
        // misheard form is gone and the replacement is present.
        let sample = "من با پایتون کار می‌کنم";
        let seen = preview(
            sample,
            pipeline(TextMode::Standard, &normalizer, &dictionary, &[]),
            pipeline(TextMode::Standard, &normalizer, &dictionary, &[]),
        );
        assert!(!seen.changes_anything());
        assert!(
            !sample.contains("پاتون"),
            "the test only exercises redundancy if the misheard form is absent"
        );
        assert_eq!(
            explain_no_effect(
                &seen,
                sample,
                &Candidate::new("پاتون", "پایتون"),
                Scope::General,
                TextMode::Standard
            ),
            Some(NoEffect::AlreadyCorrect)
        );
    }

    /// A preview of a word that is not in the sample is not evidence about the
    /// rule, and saying "the text already comes out this way" would be worse than
    /// saying nothing: the user would conclude their rule was pointless when the
    /// sample was simply the wrong one. A sample that contains *either* side of
    /// the pair counts as evidence, however — that is what separates this case
    /// from the redundant one above.
    #[test]
    fn a_sample_without_the_word_is_named_as_such_not_as_redundancy() {
        let normalizer = Normalizer::new();
        let dictionary = Dictionary::new(vec![]);
        let sample = "این جمله کوئری ندارد";
        let rules = pipeline(TextMode::Standard, &normalizer, &dictionary, &[]);
        let seen = preview(sample, rules, rules);
        assert_eq!(
            explain_no_effect(
                &seen,
                sample,
                &Candidate::new("پاتون", "پایتون"),
                Scope::General,
                TextMode::Raw
            ),
            Some(NoEffect::SampleDoesNotContainTheWord),
            "the sample outranks the destination: it is the thing the user can fix"
        );
        // …and once the sample contains it, the destination's own answer is
        // what is left.
        assert_eq!(
            explain_no_effect(
                &seen,
                "این کوئری کند است",
                &Candidate::new("کوئری", "Query"),
                Scope::General,
                TextMode::Raw
            ),
            Some(NoEffect::RawDestinationIgnoresGeneral)
        );
    }

    /// A preview may not contradict the product: the sample above goes through
    /// exactly the value the coordinator applies, in the same order.
    #[test]
    fn the_preview_agrees_with_the_pipeline_it_borrows() {
        let normalizer = Normalizer::new();
        let general = Dictionary::new(vec![rule("کوئری", "Query")]);
        let own = vec![rule("Query", "کوئری")];

        // General first, then the destination's rules — so the two undo each
        // other, and the preview must show *that*, not an isolated effect.
        let rules = pipeline(TextMode::Standard, &normalizer, &general, &own);
        assert_eq!(rules.apply("این کوئری است"), "این کوئری است");

        let seen = preview("این کوئری است", rules, rules);
        assert!(!seen.changes_anything());
        assert_eq!(seen.before, seen.after);
    }
}
