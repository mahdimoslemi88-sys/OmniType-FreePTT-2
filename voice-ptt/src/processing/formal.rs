//! The formal writing mode (P2): punctuation and spacing a written document
//! expects, on top of [`Normalizer::normalize`].
//!
//! The mode sits **after** the ordinary pipeline and touches only whitespace
//! around marks and at script boundaries — it never rewrites a word and never
//! infers meaning. That is the line the roadmap draws: نگارش means نشانه‌گذاری and
//! spacing, not بازنویسی معنایی, and a service that rewrites what the user said
//! is out of scope by decision, not by omission.
//!
//! **What it does to whitespace, precisely.** It inserts one space where a
//! written document wants one, and it **absorbs** a run of spaces that sits
//! before a mark — the "none before" half of the punctuation group, since
//! `سلام ، خوبی` is not written Persian. No other character is touched, at any
//! time, which is what `nothing_is_removed` pins.
//!
//! # The groups, and why each is its own switch
//!
//! Every group is a function over the text with its own [`FormalOptions`] flag,
//! so a user who disagrees with one of them can turn just that one off instead
//! of losing the mode. The flags live in `[text]` of `config.toml`.
//!
//! * [`FormalOptions::punctuation`] — one space **after** `، , . ؟ ? ! : ؛ ; …`
//!   when a letter follows, and none before. Speech recognisers emit
//!   `سلام،خوبی` routinely; written Persian does not.
//!   A mark **inside a Latin token** is notation, not a clause boundary, and is
//!   left alone: `example.com`, `report.docx` and `http://site` are each one
//!   word, and the same rule governs the normalizer's attachment pass
//!   ([`super::continues_latin_token`]). Measured before that rule existed:
//!   `سایت example.com را ببین` came out as `سایت example. com را ببین`.
//! * [`FormalOptions::mixed_spacing`] — one space at the boundary between a
//!   Persian/Arabic word and a Latin word or a digit, in either direction
//!   (`ازPython` → `از Python`, `۱۰۰درصد` → `۱۰۰ درصد`). Persian does not join
//!   two scripts, and the join is invisible in speech, which is why it is
//!   exactly the kind of thing a *formal* mode exists to repair.
//!
//! Dots between digits (`نسخه 2.5`) and punctuation followed by punctuation
//! (`چرا؟!`) are deliberately left alone: both are correct as they are.
//!
//! # Ordering
//!
//! `normalize` → **here** → dictionary. Running before the dictionary keeps a
//! rule from splitting a word the dictionary is about to match, and running
//! after the normalizer means the codepoint mapping has already happened —
//! Arabic kaf and yeh are Persian by the time this code sees them.

/// Which rule groups run in [`apply`].
///
/// Every group defaults to on: the mode is the choice, and the flags are there
/// for the reader who disagrees with one specific rule, not for making the mode
/// quietly do nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormalOptions {
    /// Space after sentence and clause punctuation, none before.
    pub punctuation: bool,
    /// Space where a Persian word meets a Latin word or a number.
    pub mixed_spacing: bool,
}

impl Default for FormalOptions {
    fn default() -> Self {
        Self {
            punctuation: true,
            mixed_spacing: true,
        }
    }
}

/// Marks that end or separate a clause in Persian — and their ASCII twins,
/// because a recogniser emitting `?` instead of `؟` is common.
const CLAUSE_MARKS: [char; 10] = ['،', ',', '.', '؟', '?', '!', ':', '؛', ';', '…'];

/// A character the spacing rules may insert a space **after** a mark for.
///
/// Letters only: `نسخه 2.5` has a dot followed by a digit and must not become
/// `نسخه 2. 5`.
fn is_letter(ch: char) -> bool {
    ch.is_alphabetic()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    /// A Persian or Arabic letter.
    PersianLetter,
    /// A Persian or Arabic digit (`۰`–`۹`).
    PersianDigit,
    /// A Latin letter or an ASCII digit.
    AsciiAlnum,
    /// Anything else: whitespace, punctuation, marks, other scripts.
    Other,
}

fn class_of(ch: char) -> Class {
    let code = ch as u32;
    match ch {
        '۰'..='۹' => Class::PersianDigit,
        _ if (0x0600..=0x06FF).contains(&code) || (0x0750..=0x077F).contains(&code) => {
            if ch.is_alphabetic() {
                Class::PersianLetter
            } else {
                Class::Other
            }
        }
        _ if ch.is_ascii_alphanumeric() => Class::AsciiAlnum,
        _ => Class::Other,
    }
}

/// True when two neighbours from these classes are written apart.
///
/// Both orders, because `ازPython` and `Pythonها` are both real. Persian
/// letter-to-letter is **not** listed: `کتابخانه` is one word, and Persian
/// script joins its own letters by design.
fn needs_script_space(left: Class, right: Class) -> bool {
    matches!(
        (left, right),
        (Class::PersianLetter, Class::AsciiAlnum)
            | (Class::AsciiAlnum, Class::PersianLetter)
            | (Class::PersianLetter, Class::PersianDigit)
            | (Class::PersianDigit, Class::PersianLetter)
    )
}

/// One space after a clause mark that a letter follows; none before it.
///
/// The "none before" half is redundant with the normalizer's attachment stage
/// and is here anyway: this group has to be able to stand on its own when the
/// other groups are off, and a rule whose result depends on which *other*
/// mode ran first is a rule nobody can reason about.
///
/// It absorbs the **whole run** of spaces before a mark, not one of them: a
/// rule that ate a single space would leave `سلام  ، خوبی` as `سلام ، خوبی`
/// — half-corrected, and different depending on how many spaces the source
/// happened to have. The mark itself decides nothing about the next character
/// when that character continues a Latin token.
fn punctuate(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if CLAUSE_MARKS.contains(&ch) {
            // The character the mark attaches to, read before the spaces below
            // are absorbed: it is what tells `example.com` from `سلام.خوبی`.
            let before = out.trim_end().chars().next_back();
            // No space before a mark.
            while out.ends_with(' ') {
                out.pop();
            }
            out.push(ch);
            // One space after it, only when a letter actually follows — and
            // never inside a Latin token, where the mark is notation.
            if let Some(&next) = chars.peek() {
                let notation = before.is_some_and(super::continues_latin_token)
                    && super::continues_latin_token(next);
                if is_letter(next) && !notation && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// One space where a Persian word meets a Latin word or a number.
///
/// Insertion only — never removal — so a boundary the user typed with a space
/// is left exactly as it is, and a second pass finds a space already there.
fn space_scripts(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut previous = None;
    for ch in text.chars() {
        if let Some(prev) = previous {
            if needs_script_space(class_of(prev), class_of(ch)) {
                out.push(' ');
            }
        }
        out.push(ch);
        previous = Some(ch);
    }
    out
}

/// Runs the enabled groups, in the order they are documented.
///
/// Order matters only in one place: spacing a script boundary can put a letter
/// after a mark that [`punctuate`] has already decided about, so punctuation
/// runs first and gets the final word on the space that follows a mark.
pub fn apply(text: &str, options: FormalOptions) -> String {
    let mut out = text.to_string();
    if options.punctuation {
        out = punctuate(&out);
    }
    if options.mixed_spacing {
        out = space_scripts(&out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formal(text: &str) -> String {
        apply(text, FormalOptions::default())
    }

    /// The samples the plan asks for: conversational, formal, proper nouns,
    /// technical terms, numbers, mixed script, punctuation. Each one states
    /// what changes **and** what must not — a formal mode is judged by the
    /// sentences it leaves alone as much as by the ones it fixes.
    #[test]
    fn the_samples_the_plan_asked_for() {
        for (input, expected) in [
            // محاوره
            ("سلام،خوبی؟حرف بزن", "سلام، خوبی؟ حرف بزن"),
            ("چرا؟چون", "چرا؟ چون"),
            // رسمی
            ("جناب آقای محمدی،پیرو نامهٔ قبلی", "جناب آقای محمدی، پیرو نامهٔ قبلی"),
            // نام خاص — no rule has anything to do with it
            ("دانشگاه تهران", "دانشگاه تهران"),
            ("شرکت پردازش اطلاعات", "شرکت پردازش اطلاعات"),
            // اصطلاح فنی
            ("ازPython استفاده کن", "از Python استفاده کن"),
            ("وC++ بلدم", "و C++ بلدم"),
            // اعداد
            ("۱۰۰درصد", "۱۰۰ درصد"),
            ("۲مین مرحله", "۲ مین مرحله"),
            ("نسخه 2.5 را نصب کنید", "نسخه 2.5 را نصب کنید"),
            // ترکیبی
            // توکن لاتین: نقطه و دو نقطه‌اش نشانهٔ بند نیستند. این نمونه‌ها پیش از
            // اصلاح خراب می‌شدند: `example. com`، `report. docx`،
            // `http: //site. com`، `file_name-v2. txt`.
            ("سایت example.com را ببین", "سایت example.com را ببین"),
            ("فایل report.docx را باز کن", "فایل report.docx را باز کن"),
            ("آدرس http://site.com را باز کن", "آدرس http://site.com را باز کن"),
            ("test@site.com", "test@site.com"),
            ("من Python3 و راست کار میکنم", "من Python3 و راست کار میکنم"),
            // نشانه‌گذاری
            ("چرا؟!", "چرا؟!"),
            ("مهم:صبر کن", "مهم: صبر کن"),
        ] {
            assert_eq!(formal(input), expected, "sample {input:?}");
        }
    }

    /// The mode only ever inserts, except for the run of spaces the "none
    /// before" half of the punctuation group absorbs. No other character a user
    /// wrote may disappear — not a mark, not a letter.
    #[test]
    fn nothing_is_removed() {
        for input in [
            "سلام، خوبی ؟",
            "  فاصله‌ها  ",
            "کتابخانه",
            "نسخه 2.5",
            "«نقل‌قول» و (پرانتز)",
        ] {
            let before: Vec<char> = input.chars().filter(|c| !c.is_whitespace()).collect();
            let after: Vec<char> = formal(input).chars().filter(|c| !c.is_whitespace()).collect();
            assert_eq!(before, after, "{input:?} lost or gained content");
        }
    }

    /// Applying the mode twice must change nothing the second time, or the
    /// text a user sees would depend on how many dictations ran through it.
    #[test]
    fn the_mode_is_idempotent() {
        for input in [
            "سلام،خوبی؟حرف بزن",
            "ازPython و ۱۰۰درصد",
            "چرا؟چون",
            "مهم:صبر کن",
            "شاید هم نه...بله",
        ] {
            let once = formal(input);
            assert_eq!(formal(&once), once, "{input:?} is not stable");
        }
    }

    /// Each group stands alone: turning one off must not change what the
    /// other does, and both off is the identity.
    #[test]
    fn each_group_can_be_turned_off_on_its_own() {
        let punctuation_only = FormalOptions {
            punctuation: true,
            mixed_spacing: false,
        };
        let spacing_only = FormalOptions {
            punctuation: false,
            mixed_spacing: true,
        };
        let neither = FormalOptions {
            punctuation: false,
            mixed_spacing: false,
        };

        assert_eq!(apply("سلام،خوبی", punctuation_only), "سلام، خوبی");
        assert_eq!(apply("سلام،خوبی", spacing_only), "سلام،خوبی");

        assert_eq!(apply("ازPython", punctuation_only), "ازPython");
        assert_eq!(apply("ازPython", spacing_only), "از Python");

        for input in [
            "سلام،خوبی؟  ازPython و ۱۰۰درصد",
            "شاید هم نه...بله",
            "مهم:صبر کن",
        ] {
            assert_eq!(apply(input, neither), input, "all groups off is identity");
        }
    }

    /// Two marks in a row are left alone, a mark at the end gains nothing,
    /// and a run of marks yields exactly one space — after the last mark, only
    /// when a letter follows. "نه...بله" is not written Persian; "نه... بله"
    /// is, and the space belongs to the boundary, not to each dot.
    #[test]
    fn a_mark_followed_by_another_mark_gets_no_space() {
        assert_eq!(formal("چرا؟"), "چرا؟", "a mark at the end gains nothing");
        assert_eq!(formal("چرا؟!"), "چرا؟!", "two marks in a row");
        assert_eq!(formal("شاید هم نه..."), "شاید هم نه...");
        assert_eq!(
            formal("شاید هم نه...بله"),
            "شاید هم نه... بله",
            "one space after the last mark of the run"
        );
        assert_eq!(formal("۱۲،۳۴"), "۱۲،۳۴", "a thousands separator is not a clause");
    }

    /// The group is about *spacing around* marks, not about inventing marks:
    /// a sentence without a full stop does not acquire one.
    #[test]
    fn the_mode_never_invents_punctuation() {
        for input in ["سلام دنیا", "بیا بریم", "تمام شد"] {
            assert_eq!(formal(input), input);
        }
    }

    /// The "none before" half takes the whole run, so the result does not
    /// depend on how many spaces the source carried. Before this, a two-space
    /// run came out as `سلام ، خوبی`: one space absorbed, one left standing.
    #[test]
    fn the_space_run_before_a_mark_is_absorbed_whole() {
        assert_eq!(formal("سلام ، خوبی"), "سلام، خوبی");
        assert_eq!(formal("سلام  ، خوبی"), "سلام، خوبی");
        assert_eq!(formal("پایان   ."), "پایان.");
        assert_eq!(formal("چرا ؟"), "چرا؟");
        assert_eq!(formal("مهم :\nصبر کن"), "مهم:\nصبر کن");
    }
}
