//! Speech commands (C1): phrases that mean an **operation**, not words to type.
//!
//! Saying "خط جدید" should move the caret, not write the two words. The rest of
//! the pipeline has the opposite job — it turns what was heard into the text
//! that gets typed — so the command decision is made here, once, on a value,
//! and the caller decides what to do with the answer.
//!
//! # The contract, and why each rule is the way it is
//!
//! * **Off by default.** In normal dictation the phrase "خط جدید" is content,
//!   and an app that silently turned a spoken sentence into a line break would
//!   be editing somebody's document on a guess. The switch is
//!   [`TextSettings::commands`](crate::config::settings::TextSettings).
//! * **A command is recognised in exactly two shapes** (see [`parse`]): the
//!   whole utterance *is* the phrase, or the utterance **starts with the
//!   trigger** ("دستور" / "command") and what follows is a command sequence.
//!   An embedded mention — "یک خط جدید بزن" — stays text. False positives
//!   here cost a paragraph; false negatives cost nothing.
//! * **An unknown command performs no operation and loses no text.** After the
//!   trigger, a phrase that names no command comes back as [`Op::Unknown`] and
//!   is rendered as itself. Dropping it would be the app deleting speech it
//!   simply did not understand, and keeping text is the rule the roadmap puts
//!   first.
//! * **Order is preserved.** [`parse`] answers with an ordered `Vec<Op>`, not
//!   with keystrokes: "نقطه خط جدید" is a period *then* a line break, and who
//!   sends which key is [`crate::output`]'s business, not the parser's.
//!
//! # Where this sits in the pipeline
//!
//! Last in [`process_text_with`](super::process_text_with), after the mode and
//! the general dictionary. Two reasons, both about survival: the marker a
//! newline becomes (`'\n'`) must not be collapsed by the normalizer's
//! whitespace pass, and the dictionary must not get a chance to rewrite the
//! phrase before it is recognised. [`crate::output`] then types `'\n'` as the
//! Enter key, which is why the marker is a newline and not a private sentinel
//! character nobody would type.
//!
//! The trigger is a **word the user speaks**, not punctuation: automatic
//! speech recognition does not reliably emit ":", so requiring one would make
//! a feature that looks configured and never fires.

/// The operations a spoken phrase can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    NewLine,
    NewParagraph,
    Comma,
    Period,
    Question,
    Exclamation,
    Colon,
    Semicolon,
}

impl Command {
    /// What this command becomes in the text the injector receives.
    ///
    /// A line break is a real `'\n'`, not a sentinel: [`crate::output`] sends
    /// it as the Enter key, and a raw-mode dictation that happens to contain a
    /// newline gets the same treatment. Punctuation is the character itself —
    /// the Persian comma and question mark, not their ASCII lookalikes, so the
    /// text matches what the normalizer would have produced.
    pub fn marker(self) -> &'static str {
        match self {
            Command::NewLine => "\n",
            Command::NewParagraph => "\n\n",
            Command::Comma => "،",
            Command::Period => ".",
            Command::Question => "؟",
            Command::Exclamation => "!",
            Command::Colon => ":",
            Command::Semicolon => "؛",
        }
    }
}

/// One ordered piece of a parsed utterance.
///
/// `Unknown` exists so "no operation" and "no text" stay separate answers: a
/// phrase the app cannot act on is reported, and still typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Text, passed through untouched.
    Text(String),
    /// A phrase that names an operation.
    Command(Command),
    /// After the trigger, a phrase that names no command: no operation, kept.
    Unknown(String),
}

/// The phrase that opens a command sequence.
///
/// Spoken, so it has to be a word — see the module note on punctuation. The
/// trigger is only looked for at the very start of a trimmed utterance.
pub const TRIGGER: &str = "دستور";
/// The English spelling of the trigger, for mixed speech.
pub const TRIGGER_EN: &str = "command";

/// Every recognised phrase, longest first so a two-word phrase is never
/// shadowed by one of its own words.
///
/// "نقطه" is in the list on purpose — ending a sentence is the single most
/// natural way to ask for a period — and it is only ever matched as a whole
/// utterance or after the trigger, never inside a sentence.
const PHRASES: &[(&[&str], Command)] = &[
    (
        &[
            "خط جدید",
            "خط تازه",
            "new line",
            "newline",
            "line break",
        ],
        Command::NewLine,
    ),
    (
        &[
            "پاراگراف جدید",
            "پاراگراف تازه",
            "بند جدید",
            "پاراگراف",
            "new paragraph",
            "new paragraph please",
            "paragraph",
        ],
        Command::NewParagraph,
    ),
    (&["ویرگول", "کاما", "comma"], Command::Comma),
    (&["نقطه", "dot", "period", "full stop"], Command::Period),
    (
        &["علامت سوال", "علامت پرسش", "question mark"],
        Command::Question,
    ),
    (
        &["علامت تعجب", "exclamation mark", "exclamation"],
        Command::Exclamation,
    ),
    (&["دو نقطه", "دونقطه", "colon"], Command::Colon),
    (
        &["نقطه ویرگول", "نقطه‌ویرگول", "semicolon"],
        Command::Semicolon,
    ),
];

/// Punctuation a listener may have attached to a word: stripped before the
/// word is compared with a phrase, and only before — a token that is *only*
/// punctuation is kept as it is, because stripping it would delete a mark the
/// user actually dictated.
const TRAILING_PUNCT: &[char] = &['،', ',', '.', '؛', ';', '؟', '?', '!', ':', '…', ')'];

/// Folds a string into the form phrases are compared in.
///
/// Arabic yeh and kaf become Persian ones and the zero-width non-joiner becomes
/// a space, because the recogniser may run in `Raw` mode where the normalizer
/// never gets the chance — a command spelled the way a recogniser often emits
/// it should still work when the user asked for their text verbatim. Latin is
/// lowercased; nothing else is changed.
fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\u{200c}' | '\u{200d}' => out.push(' '),
            'ي' => out.push('ی'),
            'ك' => out.push('ک'),
            'ﻻ' => out.push_str("لا"),
            other => {
                for lower in other.to_lowercase() {
                    out.push(lower);
                }
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The command a phrase names, or `None`.
///
/// Matching is on the [folded](fold) form on both sides, so the spelling the
/// recogniser produced and the spelling in the table do not have to agree.
pub fn command_for(phrase: &str) -> Option<Command> {
    let wanted = fold(phrase);
    if wanted.is_empty() {
        return None;
    }
    PHRASES
        .iter()
        .find(|(forms, _)| forms.iter().any(|form| fold(form) == wanted))
        .map(|(_, command)| *command)
}

/// Strips punctuation hanging off one word, without emptying it.
fn core(word: &str) -> &str {
    let trimmed = word.trim_matches(|ch| TRAILING_PUNCT.contains(&ch));
    if trimmed.is_empty() { word } else { trimmed }
}

/// Parses one command sequence: the text after the trigger.
///
/// Two words are tried before one, so "خط جدید" is a line break and not the
/// word "خط" followed by an unknown. Anything that names no command becomes
/// [`Op::Unknown`] and keeps its text.
fn parse_sequence(rest: &str) -> Vec<Op> {
    let words: Vec<&str> = rest
        .split(|ch: char| ch.is_whitespace() || ch == '،' || ch == ',')
        .filter(|w| !w.is_empty())
        .collect();

    let mut ops = Vec::with_capacity(words.len());
    let mut index = 0;
    while index < words.len() {
        if index + 1 < words.len() {
            let pair = format!("{} {}", core(words[index]), core(words[index + 1]));
            if let Some(command) = command_for(&pair) {
                ops.push(Op::Command(command));
                index += 2;
                continue;
            }
        }
        let one = core(words[index]);
        if let Some(command) = command_for(one) {
            ops.push(Op::Command(command));
        } else {
            ops.push(Op::Unknown(one.to_string()));
        }
        index += 1;
    }
    ops
}

/// True when the trimmed utterance starts with the trigger word.
///
/// The trigger must be its own word: "دستورکار" is a noun, not an instruction.
fn after_trigger(trimmed: &str) -> Option<&str> {
    for trigger in [TRIGGER, TRIGGER_EN] {
        let Some(rest) = trimmed.strip_prefix(trigger) else {
            continue;
        };
        if rest.is_empty() {
            return Some("");
        }
        let mut chars = rest.chars();
        let first = chars.next()?;
        if first.is_whitespace() || TRAILING_PUNCT.contains(&first) {
            let start = rest.len() - chars.as_str().len();
            return Some(rest[start..].trim_start_matches(['،', ',', ' ', ':']));
        }
        // The trigger's own letters glued to something else: not a trigger.
    }
    None
}

/// Parses an utterance into ordered operations.
///
/// Three shapes, in this order:
///
/// 1. The whole (trimmed) utterance **is** a phrase → that one command.
///    This is what makes saying just "خط جدید" work without a preamble.
/// 2. It **starts with the trigger** → everything after is a command sequence.
///    Only returned when the sequence contains at least one real command; a
///    sentence that merely opens with the word "دستور" ("دستور قاضی…") parses
///    to nothing useful and therefore falls through untouched.
/// 3. Anything else → the utterance as [`Op::Text`], byte for byte.
///
/// The parser never sees the switch: [`apply`] is where "off" means "identity",
/// so a test can ask what a sentence *would* parse to without turning the
/// feature on.
pub fn parse(text: &str) -> Vec<Op> {
    let trimmed = text.trim();

    if let Some(command) = command_for(core(trimmed)) {
        // One utterance that is exactly a command. Surrounding whitespace and
        // punctuation the recogniser appended are not content — there is
        // nothing to separate the command from.
        return vec![Op::Command(command)];
    }

    if let Some(rest) = after_trigger(trimmed) {
        let ops = parse_sequence(rest);
        if ops.iter().any(|op| matches!(op, Op::Command(_))) {
            return ops;
        }
        // A sentence that opens with the trigger and contains no command is a
        // sentence. Falling through keeps it whole.
    }

    vec![Op::Text(text.to_string())]
}

/// Renders ordered operations back into the text the injector will receive.
pub fn render(ops: &[Op]) -> String {
    let mut out = String::new();
    for op in ops {
        match op {
            Op::Text(text) => out.push_str(text),
            Op::Unknown(word) => {
                // A single separating space, unless the text is empty or
                // already stands at a boundary — a line break is a boundary.
                if !out.is_empty()
                    && !out.ends_with(char::is_whitespace)
                    && !out.ends_with('\n')
                {
                    out.push(' ');
                }
                out.push_str(word);
            }
            Op::Command(command) => out.push_str(command.marker()),
        }
    }
    out
}

/// [`parse`] then [`render`], with the switch applied.
///
/// Off returns the input unchanged — not a re-render, because re-rendering
/// would normalise whitespace the caller was entitled to keep.
pub fn apply(text: &str, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    render(&parse(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Criterion 1 from the plan: with commands off, a sentence *containing*
    /// a command's name is ordinary text.
    #[test]
    fn off_is_identity_even_for_a_sentence_naming_a_command() {
        for text in [
            "خط جدید",
            "یک خط جدید بزن لطفا",
            "دستور قاضی را اجرا کرد",
            "در آخر هم نقطه بگذار",
            "",
            "   ",
        ] {
            assert_eq!(apply(text, false), text, "{text:?} changed while off");
        }
    }

    /// Criterion 2: on, only valid commands act — and the two shapes are the
    /// only ways in.
    #[test]
    fn an_embedded_mention_is_still_text_when_on() {
        for text in ["یک خط جدید بزن لطفا", "گفت که نقطه بگذارم"] {
            assert_eq!(apply(text, true), text, "{text:?} was acted on");
        }
    }

    /// The whole utterance being the phrase is the first shape.
    #[test]
    fn a_whole_utterance_that_is_a_command_becomes_the_command() {
        assert_eq!(apply("خط جدید", true), "\n");
        assert_eq!(apply("  خط جدید  ", true), "\n");
        assert_eq!(apply("پاراگراف جدید", true), "\n\n");
        assert_eq!(apply("ویرگول", true), "،");
        assert_eq!(apply("نقطه", true), ".");
        assert_eq!(apply("علامت سوال", true), "؟");
        assert_eq!(apply("علامت تعجب", true), "!");
        assert_eq!(apply("نقطه ویرگول", true), "؛");
        assert_eq!(apply("دو نقطه", true), ":");
    }

    /// The second shape: the trigger opens a sequence, order is preserved, and
    /// text before the first command survives.
    #[test]
    fn the_trigger_opens_an_ordered_sequence() {
        assert_eq!(
            parse("دستور سلام، نقطه خط جدید"),
            vec![
                Op::Unknown("سلام".into()),
                Op::Command(Command::Period),
                Op::Command(Command::NewLine),
            ]
        );
        assert_eq!(apply("دستور سلام، نقطه خط جدید", true), "سلام.\n");

        assert_eq!(
            apply("دستور خط جدید بعد ویرگول", true),
            "\nبعد،",
            "order must survive: newline, then the word, then a comma"
        );
    }

    /// Criterion 3: an unrecognised command does nothing and costs no text.
    #[test]
    fn an_unknown_command_does_nothing_and_keeps_the_words() {
        let ops = parse("دستور فلان چیز خط جدید");
        assert_eq!(
            ops,
            vec![
                Op::Unknown("فلان".into()),
                Op::Unknown("چیز".into()),
                Op::Command(Command::NewLine),
            ]
        );
        // The words are still there, and only the valid command acted.
        assert_eq!(apply("دستور فلان چیز خط جدید", true), "فلان چیز\n");
    }

    /// A sentence that merely *opens* with the trigger word is a sentence.
    /// Acting on it would be the app mangling ordinary speech that happens to
    /// start with the word "دستور".
    #[test]
    fn a_sentence_opening_with_the_trigger_falls_through_when_nothing_matches() {
        let text = "دستور قاضی را اجرا کردند";
        assert_eq!(parse(text), vec![Op::Text(text.to_string())]);
        assert_eq!(apply(text, true), text);
    }

    /// The trigger must be a whole word.
    #[test]
    fn a_word_that_merely_starts_with_the_trigger_is_not_the_trigger() {
        let text = "دستورکارش را کامل کرد";
        assert_eq!(apply(text, true), text);
    }

    /// English, mixed speech, and the spellings a recogniser is likely to emit.
    #[test]
    fn latin_and_recogniser_spellings_are_recognised() {
        assert_eq!(apply("command: new line", true), "\n");
        assert_eq!(apply("دستور new paragraph", true), "\n\n");
        assert_eq!(apply("دستور comma", true), "،");
        assert_eq!(
            apply("دستور question mark", true),
            "؟",
            "a two-word English phrase is matched as a phrase"
        );
    }

    /// Arabic yeh/kaf and the half-space must not decide whether a command
    /// fires — `Raw` mode never runs the normalizer over the input.
    #[test]
    fn folded_spellings_still_match() {
        assert_eq!(command_for("خط جديد"), Some(Command::NewLine));
        assert_eq!(
            command_for("خط\u{200c}جدید"),
            Some(Command::NewLine),
            "a half-space in the middle of the phrase must not decide it"
        );
        assert_eq!(command_for("پاراگراف"), Some(Command::NewParagraph));
        assert_eq!(
            command_for("نقطه\u{200c}ویرگول"),
            Some(Command::Semicolon)
        );
        assert_eq!(
            command_for("سطر جدید"),
            None,
            "not every similar phrase acts"
        );
    }

    /// Recognition is last in the pipeline, so a rendered result must not
    /// contain a phrase that a second pass would act on again.
    #[test]
    fn applying_twice_changes_nothing_the_second_time() {
        for text in [
            "دستور سلام، نقطه خط جدید",
            "خط جدید",
            "دستور فلان چیز",
            "یک جملهٔ معمولی",
        ] {
            let once = apply(text, true);
            let twice = apply(&once, true);
            assert_eq!(once, twice, "{text:?} is not stable under a second pass");
        }
    }

    /// An empty utterance is not a command sequence.
    #[test]
    fn an_empty_utterance_stays_empty() {
        assert_eq!(parse(""), vec![Op::Text(String::new())]);
        assert_eq!(apply("   ", true), "   ");
    }

    /// Punctuation hanging off a phrase is the recogniser's, not the user's
    /// word: "نقطه،" inside a sequence is still the period command.
    #[test]
    fn punctuation_glued_to_a_phrase_is_not_part_of_it() {
        assert_eq!(
            apply("دستور نقطه، بعد نقطه", true),
            ". بعد.",
            "the comma is the recogniser's, and a period after it is still a period"
        );
        assert_eq!(apply("نقطه،", true), ".", "a trailing mark is not content");
    }
}
