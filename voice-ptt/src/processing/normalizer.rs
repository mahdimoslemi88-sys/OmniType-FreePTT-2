//! Persian text normalizer.
//!
//! Fixes the most common ASR output problems for Persian:
//! 1. Arabic codepoints that should be Persian (ي→ی, ك→ک, ة→ه, numbers…)
//! 2. ZWNJ (نیم‌فاصله) normalization and commonSpacing mistakes
//! 3. Punctuation spacing and placement (، ؛ ؟)
//! 4. Whitespace cleanup
//!
//! The second stage of the pipeline (dictionary) fixes *what* was said; this
//! stage fixes *how it is written*.

/// Characters normalized from Arabic to Persian codepoints.
const ARABIC_TO_PERSIAN: &[(char, char)] = &[
    ('ي', 'ی'), // Arabic yeh → Persian yeh
    ('ك', 'ک'), // Arabic kaf → Persian kaf
    ('ة', 'ه'), // Teh marbuta → Heh
    ('ؤ', 'و'), // Waw with hamza (common ASR artifact)
    ('ٱ', 'ا'), // Alef with wasla
    ('أ', 'ا'), // Alef with hamza above
    ('إ', 'ا'), // Alef with hamza below
    ('٠', '۰'), // Arabic-Indic digits → Extended (Persian)
    ('١', '۱'),
    ('٢', '۲'),
    ('٣', '۳'),
    ('٤', '۴'),
    ('٥', '۵'),
    ('٦', '۶'),
    ('٧', '۷'),
    ('٨', '۸'),
    ('٩', '۹'),
];

/// Suffixes that attach with ZWNJ.
const HALF_SPACE_SUFFIXES: &[&str] = &[
    "ها", "های", "هایی", "تر", "ترین", "ام", "ات", "اش",
];

/// Punctuation that must attach to the *previous* word (no space before,
/// one space after).
const ATTACH_PUNCT: &[char] = &['،', '؛', '؟', '!', '.', ':', ','];

pub struct Normalizer {
    zwnj: char,
}

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Normalizer {
    pub fn new() -> Self {
        Self {
            zwnj: '\u{200C}', // ZWNJ
        }
    }

    /// Normalizes an ASR transcript.
    pub fn normalize(&self, input: &str) -> String {
        // Stage 1: codepoint mapping + whitespace collapse.
        let mapped: String = input
            .chars()
            .filter_map(|c| {
                // Normalize all whitespace (incl. NBSP) to plain spaces.
                if c.is_whitespace() && c != ' ' {
                    Some(' ')
                } else {
                    ARABIC_TO_PERSIAN
                        .iter()
                        .find(|(from, _)| *from == c)
                        .map(|(_, to)| *to)
                        .or(Some(c))
                }
            })
            .collect();

        // Stage 2: collapse runs of spaces.
        let mut collapsed = String::with_capacity(mapped.len());
        let mut prev_space = false;
        for c in mapped.chars() {
            if c == ' ' {
                if !prev_space {
                    collapsed.push(' ');
                }
                prev_space = true;
            } else {
                collapsed.push(c);
                prev_space = false;
            }
        }

        // Stage 3: punctuation attachment ("سلام ." → "سلام.").
        let mut punct_fixed = String::with_capacity(collapsed.len());
        let chars: Vec<char> = collapsed.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if ATTACH_PUNCT.contains(&c) {
                // Remove a space immediately before the punctuation.
                if punct_fixed.ends_with(' ') {
                    punct_fixed.pop();
                }
                punct_fixed.push(c);
                // Ensure exactly one space after, unless next is also punct or end.
                if let Some(&next) = chars.get(i + 1) {
                    if next != ' ' && !ATTACH_PUNCT.contains(&next) {
                        punct_fixed.push(' ');
                    }
                }
            } else {
                punct_fixed.push(c);
            }
        }

        // Stage 4: ZWNJ insertion for known prefix/suffix patterns.
        self.fix_half_spaces(&punct_fixed)
            .trim()
            .to_string()
    }

    /// Inserts ZWNJ in known compound patterns.
    fn fix_half_spaces(&self, text: &str) -> String {
        let mut out: Vec<String> = Vec::new();
        for word in text.split(' ') {
            out.push(self.fix_word(word));
        }
        out.join(" ")
    }

    fn fix_word(&self, word: &str) -> String {
        // If the word already contains ZWNJ, trust it.
        if word.contains(self.zwnj) {
            return word.to_string();
        }
        // Whole-word stop-list: real words that merely *end or start* with a
        // prefix/suffix look-alike (سلام, میوه, میز, …) must never be split.
        if is_standalone_word(word) {
            return word.to_string();
        }

        // prefix + suffix compounds like "میرفتها" → "می‌رفت‌ها"
        // (handled by two passes: strip known prefix, then known suffix)
        let mut w = word.to_string();

        // Prefix: "می" / "نمی" + rest (rest must be at least 2 chars of letters)
        for prefix in ["نمی", "می"] {
            if w.starts_with(prefix) && w.chars().count() > prefix.chars().count() + 1 {
                let rest = &w[prefix.len()..];
                // Heuristic guard: rest must not itself be a known standalone
                // word that merely starts with these letters (e.g. "میوه").
                if !is_standalone_word(rest) {
                    w = format!("{prefix}{}{rest}", self.zwnj);
                    break;
                }
            }
        }

        // Suffix pass: "کتابها" → "کتاب‌ها" (skip if ZWNJ already inserted above).
        for suffix in HALF_SPACE_SUFFIXES {
            if w.ends_with(suffix) && w.chars().count() > suffix.chars().count() + 1 {
                let stem = &w[..w.len() - suffix.len()];
                // Do not split words where the "suffix" is intrinsic
                // (e.g. "ستر" is a word, not "ست"+"تر").
                if !is_standalone_word(stem) && stem.chars().any(|c| !c.is_whitespace()) {
                    let has_zwnj = w.contains(self.zwnj);
                    let base = if has_zwnj {
                        w.trim_end_matches(suffix).to_string()
                    } else {
                        stem.to_string()
                    };
                    let sep: String = if has_zwnj {
                        String::new()
                    } else {
                        self.zwnj.to_string()
                    };
                    w = format!("{base}{sep}{suffix}");
                    break;
                }
            }
        }

        w
    }
}

/// Small stop-list of real Persian words that would otherwise be mistaken for
/// prefix/suffix boundaries by the heuristic.
fn is_standalone_word(word: &str) -> bool {
    const STANDALONE: &[&str] = &[
        "میوه", "میان", "میلاد", "مینا", "میز", "میگ", // می-
        "بید", "بین", "بیل", // بی-
        "همنشین", // هم-
        "چند", "یک",
        "ستر", "بتر", "ختری", // -تر
        "شام", "پیام", "سلام", "کتابخانم", // -ام guard
    ];
    STANDALONE.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teh_marbuta_becomes_heh() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("مدرسة"), "مدرسه");
    }

    #[test]
    fn yeh_and_kaf_normalized() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("كتاب يك"), "کتاب یک");
    }

    #[test]
    fn arabic_digits_become_persian() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("١٢٣"), "۱۲۳");
    }

    #[test]
    fn punctuation_attaches_to_previous_word() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("سلام ، خوبی ؟"), "سلام، خوبی؟");
    }

    #[test]
    fn whitespace_collapsed() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("سلام   دنیا"), "سلام دنیا");
    }

    #[test]
    fn mi_prefix_gets_zwnj() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("میروم"), "می‌روم");
        assert_eq!(n.normalize("نمیخواهم"), "نمی‌خواهم");
    }

    #[test]
    fn mi_standalone_words_not_split() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("میوه"), "میوه");
        assert_eq!(n.normalize("میز"), "میز");
    }

    #[test]
    fn ha_suffix_gets_zwnj() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("کتابها"), "کتاب‌ها");
    }

    #[test]
    fn existing_zwnj_is_preserved() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("می‌روم"), "می‌روم");
    }

    #[test]
    fn empty_string_is_safe() {
        let n = Normalizer::new();
        assert_eq!(n.normalize(""), "");
        assert_eq!(n.normalize("   "), "");
    }
}
