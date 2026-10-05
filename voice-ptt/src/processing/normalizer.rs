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
///
/// Hamza preservation (Requirement 1):
/// 'أ' (Alef with hamza above, U+0623), 'إ' (Alef with hamza below, U+0625), and
/// 'ؤ' (Waw with hamza above, U+0624) are legitimate Persian orthographic characters
/// (e.g. مسأله, تأیید, رأی, مؤمن, مؤثر). Deleting them causes irreversible loss
/// of orthographic information. Removing the three hamza-deleting mappings preserves
/// hamza in Conservative and Standard modes while still normalizing yeh, kaf, teh marbuta,
/// and digits.
const ARABIC_TO_PERSIAN: &[(char, char)] = &[
    ('ي', 'ی'), // Arabic yeh → Persian yeh
    ('ك', 'ک'), // Arabic kaf → Persian kaf
    ('ة', 'ه'), // Teh marbuta → Heh
    ('ٱ', 'ا'), // Alef with wasla → Bare alef
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

/// Known Persian noun stems that take plural suffixes `-ها`, `-های`, and `-هایی`.
///
/// In standard Persian morphology, naive suffix splitting on "ها" corrupts single-morpheme
/// words naturally ending in "ها" (e.g. «اژدها», «تنها», «اشتها», «انتها», «ابتدا»).
/// In accordance with Rule 3 (positive evidence only; no negative blacklists), plural suffix
/// insertion is strictly restricted to positive evidence of known noun stems.
const NOUN_STEMS_FOR_HA: &[&str] = &[
    "کتاب",
    "دست",
    "پا",
    "سر",
    "چشم",
    "دل",
    "جان",
    "رو",
    "مو",
    "روز",
    "شب",
    "سال",
    "ماه",
    "هفته",
    "ساعت",
    "لحظه",
    "زمان",
    "وقت",
    "کار",
    "راه",
    "بار",
    "نام",
    "پیام",
    "نامه",
    "صفحه",
    "خط",
    "کلمه",
    "خانه",
    "اتاق",
    "در",
    "دیوار",
    "شهر",
    "کوه",
    "دریا",
    "رود",
    "باغ",
    "گل",
    "درخت",
    "برگ",
    "سنگ",
    "آب",
    "باد",
    "خاک",
    "هوا",
    "زمین",
    "ستاره",
    "ابر",
    "انسان",
    "زن",
    "مرد",
    "پسر",
    "دختر",
    "بچه",
    "کودک",
    "دوست",
    "یار",
    "همراه",
    "مادر",
    "پدر",
    "برادر",
    "خواهر",
    "استاد",
    "شاگرد",
    "معلم",
    "دانشجو",
    "کارمند",
    "فیلم",
    "عکس",
    "صدا",
    "تصویر",
    "ساز",
    "آهنگ",
    "قلم",
    "دفتر",
    "بخش",
    "درس",
    "فصل",
    "نکته",
    "مورد",
    "چیز",
    "گروه",
    "تیم",
    "دسته",
    "رنگ",
    "لباس",
    "ماشین",
    "خودرو",
    "ابزار",
    "قطعه",
    "برنامه",
    "سامانه",
    "سیستم",
    "پروژه",
    "سایت",
    "کاربر",
    "مدیر",
    "داده",
    "روش",
    "راهکار",
    "سوال",
    "سؤال",
    "پاسخ",
    "جواب",
    "خبر",
    "اتفاق",
    "حادثه",
    "مشکل",
    "مسأله",
    "مساله",
    "بازی",
    "هدف",
    "داستان",
    "شعر",
    "قصه",
    "نقشه",
    "طرح",
];

/// Known Persian verbal present stems (بن مضارع) conjugated with `می-` and `نمی-`.
const PRESENT_VERB_STEMS: &[&str] = &[
    "رو", "گو", "بین", "خور", "زن", "کن", "نویس", "خوان", "خر", "فروش", "دان", "توان", "رس",
    "خواه", "باش", "شو", "آی", "آور", "دار", "گیر", "ده", "ساز", "سوز", "کش", "بر", "بند", "شنو",
    "پرس", "پوش", "نشین", "خواب", "افت", "ریز", "پر", "پز", "شناس", "بخش", "تاب", "ترس", "چش",
    "چرخ", "خند", "گرد", "طلب", "سنج", "چسب", "گذار", "گذر", "مان", "فهم",
];

/// Known Persian verbal past stems (بن ماضی) conjugated with `می-` and `نمی-`.
const PAST_VERB_STEMS: &[&str] = &[
    "رفت",
    "گفت",
    "دید",
    "خورد",
    "زد",
    "کرد",
    "نوشت",
    "خواند",
    "خرید",
    "فروخت",
    "دانست",
    "توانست",
    "رسید",
    "خواست",
    "بود",
    "شد",
    "آمد",
    "آورد",
    "داشت",
    "گرفت",
    "داد",
    "ساخت",
    "سوخت",
    "کشید",
    "برد",
    "بست",
    "شنید",
    "پرسید",
    "پوشید",
    "نشست",
    "خوابید",
    "افتاد",
    "ریخت",
    "پرید",
    "پخت",
    "شناخت",
    "بخشید",
    "تابید",
    "ترسید",
    "چشید",
    "چرخید",
    "خندید",
    "گریست",
    "گردید",
    "طلبید",
    "سنجید",
    "چسبید",
    "گذاشت",
    "گذشت",
    "ماند",
    "فهمید",
];

/// Checks whether `rest` represents a valid Persian verb form after `می` or `نمی`.
///
/// Requirement 2: Exact stem + valid finite personal ending. Rejects unknown middle segments.
/// A candidate like "میرونام" (starts with root "رو" and ends with personal ending "م", but with
/// unknown middle segment "نا") is strictly rejected.
fn is_persian_verb_after_mi(rest: &str) -> bool {
    // 1. Exact match with present stems + finite endings (م, ی, د, یم, ید, ند):
    for &stem in PRESENT_VERB_STEMS {
        if let Some(suffix) = rest.strip_prefix(stem) {
            if matches!(suffix, "م" | "ی" | "د" | "یم" | "ید" | "ند") {
                return true;
            }
        }
    }

    // 2. Exact match with past stems + past endings (empty for 3rd-sg, م, ی, یم, ید, ند):
    for &stem in PAST_VERB_STEMS {
        if let Some(suffix) = rest.strip_prefix(stem) {
            if matches!(suffix, "" | "م" | "ی" | "یم" | "ید" | "ند") {
                return true;
            }
        }
    }

    false
}

/// Known Persian adjective stems that take comparative/superlative suffixes `-تر` and `-ترین`.
///
/// In Persian, `-تر` is exclusively an adjective suffix (صفت تفضیلی).
/// Many common Persian nouns naturally end in "تر" (e.g. کبوتر, دختر, دفتر, انگشتر, دکتر, اختر).
/// Naive suffix splitting based purely on string endings corrupts these nouns into "کبو‌تر",
/// "دخ‌تر", etc. Suffix insertion for `-تر` and `-ترین` is strictly restricted to positive
/// evidence of known adjective stems. In ambiguous cases, the word is preserved as-is.
const ADJECTIVE_STEMS: &[&str] = &[
    "بزرگ",
    "کوچک",
    "سخت",
    "آسان",
    "خوب",
    "بد",
    "بلند",
    "کوتاه",
    "بیش",
    "کم",
    "روشن",
    "تاریک",
    "زیبا",
    "قوی",
    "ضعیف",
    "ساده",
    "جدید",
    "قدیم",
    "پهن",
    "تنگ",
    "گرم",
    "سرد",
    "پیر",
    "جوان",
    "نو",
    "کهنه",
    "تیز",
    "تند",
    "کند",
    "دور",
    "نزدیک",
    "پاک",
    "نرم",
    "تلخ",
    "شیرین",
    "مهم",
    "عالی",
    "مناسب",
    "مفید",
    "سریع",
    "آرام",
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

    /// Normalizes an ASR transcript, including the half-space rules.
    ///
    /// This is [`TextMode::Standard`](super::TextMode): the whole pipeline, and
    /// what this app has always done. For the part of it that can still be
    /// wrong on an unfamiliar word, see [`Self::normalize_conservative`].
    pub fn normalize(&self, input: &str) -> String {
        self.fix_half_spaces(&self.normalize_conservative(input))
            .trim()
            .to_string()
    }

    /// Everything here is a change no reader could disagree with.
    ///
    /// Arabic codepoints the Persian layout cannot produce, runs of whitespace,
    /// and punctuation attached to the word it belongs to. What it
    /// deliberately does **not** do is infer half-spaces: that step reads
    /// Persian morphology, and an unfamiliar word comes out of it altered
    /// rather than untouched. It is the mode to choose when the engine's output
    /// is already correct and a guess makes it worse.
    pub fn normalize_conservative(&self, input: &str) -> String {
        self.map_codepoints_and_punct(input).trim().to_string()
    }

    fn map_codepoints_and_punct(&self, input: &str) -> String {
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

        // Stage 4 is the half-space inference, and it lives in `normalize`, not
        // here: `normalize_conservative` stops after stage 3 on purpose.
        punct_fixed
    }

    /// Checks if a character is punctuation that may enclose or attach to words.
    fn is_punctuation_char(c: char) -> bool {
        c.is_ascii_punctuation()
            || matches!(
                c,
                '«' | '»' | '،' | '؛' | '؟' | '…' | '“' | '”' | '‘' | '’' | '‹' | '›' | 'ـ'
            )
    }

    /// Separates leading and trailing punctuation from the token so the core word can be
    /// analyzed and fixed without punctuation interfering, while preserving all punctuation intact.
    fn split_enclosing_punct(token: &str) -> (&str, &str, &str) {
        let mut chars = token.char_indices().peekable();
        let mut start_idx = token.len();
        while let Some(&(i, c)) = chars.peek() {
            if !Self::is_punctuation_char(c) {
                start_idx = i;
                break;
            }
            chars.next();
        }

        if start_idx == token.len() {
            return (token, "", "");
        }

        let end_idx = token
            .char_indices()
            .rfind(|(_, c)| !Self::is_punctuation_char(*c))
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(token.len());

        (
            &token[..start_idx],
            &token[start_idx..end_idx],
            &token[end_idx..],
        )
    }

    /// Inserts ZWNJ in known compound patterns.
    fn fix_half_spaces(&self, text: &str) -> String {
        let mut out: Vec<String> = Vec::new();
        for word in text.split(' ') {
            out.push(self.fix_word(word));
        }
        out.join(" ")
    }

    fn fix_word(&self, token: &str) -> String {
        if token.is_empty() {
            return String::new();
        }

        // Requirement 3: Separate leading and trailing punctuation from the core word so that
        // punctuation does not interfere with morphological analysis, and is preserved intact.
        let (leading_punct, core, trailing_punct) = Self::split_enclosing_punct(token);
        if core.is_empty() {
            return token.to_string();
        }

        let fixed_core = self.fix_core_word(core);
        if fixed_core == core {
            token.to_string()
        } else {
            format!("{leading_punct}{fixed_core}{trailing_punct}")
        }
    }

    fn fix_core_word(&self, core: &str) -> String {
        // If the core already contains ZWNJ, trust it.
        if core.contains(self.zwnj) {
            return core.to_string();
        }

        let mut w = core.to_string();

        // 1. Prefix pass: "نمی" / "می" + verified verb stem + exact finite suffix.
        // Requires positive morphological evidence: exact stem + valid personal suffix.
        // Rejects any unknown middle segment (e.g. synthetic "میرونام" is rejected).
        // Non-verbs starting with "می" (e.g. میکروفون, میوه, میز, میلیون) remain intact.
        for prefix in ["نمی", "می"] {
            if w.starts_with(prefix) && w.chars().count() > prefix.chars().count() + 1 {
                let rest = &w[prefix.len()..];
                if is_persian_verb_after_mi(rest) {
                    w = format!("{prefix}{}{rest}", self.zwnj);
                    break;
                }
            }
        }

        // 2. Suffix pass for plural -ها, -های, -هایی:
        // Restricted to positive morphological evidence of known noun stems (NOUN_STEMS_FOR_HA).
        // Single-morpheme words ending in "ها" (e.g. اژدها, تنها, اشتها, انتها, ابتدا)
        // have no noun stem evidence and remain intact without needing any negative blacklist.
        for suffix in ["هایی", "های", "ها"] {
            if w.ends_with(suffix) && w.chars().count() > suffix.chars().count() + 1 {
                let stem = &w[..w.len() - suffix.len()];
                if NOUN_STEMS_FOR_HA.contains(&stem) {
                    let has_zwnj = w.contains(self.zwnj);
                    let base = if has_zwnj {
                        w.trim_end_matches(suffix).to_string()
                    } else {
                        stem.to_string()
                    };
                    let sep = if has_zwnj { "" } else { "\u{200c}" };
                    w = format!("{base}{sep}{suffix}");
                    break;
                }
            }
        }

        // 3. Suffix pass for adjective comparative/superlative -ترین and -تر:
        // Restricted to positive evidence of known adjective stems (ADJECTIVE_STEMS).
        // Common Persian nouns naturally ending in "تر" (e.g. کبوتر, دختر, دفتر, انگشتر, دکتر, اختر)
        // lack adjective evidence and are preserved as-is. In ambiguous cases, the word is preserved.
        for suffix in ["ترین", "تر"] {
            if w.ends_with(suffix) && w.chars().count() > suffix.chars().count() + 1 {
                let stem = &w[..w.len() - suffix.len()];
                if ADJECTIVE_STEMS.contains(&stem) {
                    let has_zwnj = w.contains(self.zwnj);
                    let base = if has_zwnj {
                        w.trim_end_matches(suffix).to_string()
                    } else {
                        stem.to_string()
                    };
                    let sep = if has_zwnj { "" } else { "\u{200c}" };
                    w = format!("{base}{sep}{suffix}");
                    break;
                }
            }
        }

        w
    }
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

    #[test]
    fn hamza_is_preserved_in_conservative_and_standard() {
        let n = Normalizer::new();
        // Hamza on alef (أ), waw (ؤ), and bare hamza:
        for word in ["مسأله", "مؤمن", "تأیید", "رأی", "مؤثر", "سؤال", "مأخذ"]
        {
            assert_eq!(
                n.normalize_conservative(word),
                word,
                "conservative mode must preserve hamza in {word}"
            );
            assert_eq!(
                n.normalize(word),
                word,
                "standard mode must preserve hamza in {word}"
            );
        }
    }

    #[test]
    fn healthy_words_are_preserved_without_corruption() {
        let n = Normalizer::new();
        // Words specified by requirement 2 that must not be broken:
        for word in [
            "کلمات",
            "تمام",
            "اتمام",
            "میکروفون",
            "کبوتر",
            "اژدها",
            "دختر",
            "دفتر",
            "انگشتر",
            "تنها",
            "میوه",
            "میز",
            "میلاد",
            "میلیون",
        ] {
            assert_eq!(
                n.normalize(word),
                word,
                "healthy word must survive untouched without being broken: {word}"
            );
        }
    }

    #[test]
    fn valid_compounds_get_zwnj_with_positive_evidence() {
        let n = Normalizer::new();
        // Verbs with verified stems:
        assert_eq!(n.normalize("میروم"), "می‌روم");
        assert_eq!(n.normalize("نمیخواهم"), "نمی‌خواهم");
        assert_eq!(n.normalize("میکنم"), "می‌کنم");
        assert_eq!(n.normalize("میشود"), "می‌شود");

        // Plurals:
        assert_eq!(n.normalize("کتابها"), "کتاب‌ها");
        assert_eq!(n.normalize("کتابهای"), "کتاب‌های");
        assert_eq!(n.normalize("کتابهایی"), "کتاب‌هایی");

        // Adjectives with verified stems:
        assert_eq!(n.normalize("بزرگترین"), "بزرگ‌ترین");
        assert_eq!(n.normalize("سختتر"), "سخت‌تر");
    }

    #[test]
    fn explicit_space_in_mi_rom_is_preserved() {
        let n = Normalizer::new();
        // Explicit space between "می" and "روم" must not be converted to ZWNJ:
        assert_eq!(n.normalize("می روم"), "می روم");
        assert_eq!(n.normalize_conservative("می روم"), "می روم");
    }

    #[test]
    fn latin_digits_are_preserved() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("123"), "123");
        assert_eq!(n.normalize("کد 456 تست"), "کد 456 تست");
        assert_eq!(n.normalize_conservative("123"), "123");
    }

    #[test]
    fn test_synthetic_verb_with_unknown_middle_is_rejected() {
        let n = Normalizer::new();
        // "میرونام": starts with "می", contains root "رو" and ends with "م",
        // but has unknown middle segment "نا" -> must NOT be broken or altered!
        assert_eq!(n.normalize("میرونام"), "میرونام");
        assert_eq!(n.normalize("میروشم"), "میروشم");
    }

    #[test]
    fn test_punctuation_around_words_is_preserved_during_compound_fixing() {
        let n = Normalizer::new();
        // Leading and trailing punctuation must not prevent compound fixing,
        // and must remain intact in place:
        assert_eq!(n.normalize("«کتابها»"), "«کتاب‌ها»");
        assert_eq!(n.normalize("(میروم)"), "(می‌روم)");
        assert_eq!(n.normalize("«میروم»!"), "«می‌روم»!");
        assert_eq!(n.normalize("[کتابها]،"), "[کتاب‌ها]،");
    }

    #[test]
    fn test_ha_plural_uses_positive_stems_without_negative_blacklist() {
        let n = Normalizer::new();
        // Valid plurals based on positive noun stems:
        assert_eq!(n.normalize("کتابها"), "کتاب‌ها");
        assert_eq!(n.normalize("کتابهای"), "کتاب‌های");
        assert_eq!(n.normalize("کتابهایی"), "کتاب‌هایی");
        assert_eq!(n.normalize("دستها"), "دست‌ها");
        assert_eq!(n.normalize("روزها"), "روز‌ها");

        // Words ending in ها/های that lack noun stems must survive without any negative blacklist:
        for word in [
            "اژدها",
            "تنها",
            "اشتها",
            "انتها",
            "ابتدا",
            "ادعا",
            "انشا",
            "امضا",
            "رها",
        ] {
            assert_eq!(
                n.normalize(word),
                word,
                "single morpheme word ending in ها must be preserved without blacklist: {word}"
            );
        }
    }
}
