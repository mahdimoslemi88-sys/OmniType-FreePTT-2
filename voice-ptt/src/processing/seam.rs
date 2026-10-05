//! Chunk-seam repair for continuous dictation.
//!
//! Phase 3 cuts a long dictation into ~20 s chunks and injects each transcript
//! as soon as it arrives, which creates two artefacts *at the seam only*:
//!
//! 1. **Repeated words.** The next chunk deliberately re-sends `overlap_ms` of
//!    audio so a word cannot be clipped by the cut. The recogniser then
//!    transcribes those words again, and the tail of the previous chunk gets
//!    typed twice.
//! 2. **A truncated word.** If the cut fell *inside* a word, chunk N ends with a
//!    fragment (`… می‌خوا`) and chunk N+1 spells the same word out in full
//!    (`می‌خواهم …`).
//!
//! This module is the pure, testable fix for both: [`SeamStitcher::stitch`]
//! takes the freshly transcribed chunk and returns
//!
//! * the text that should actually be injected (duplicated head words removed),
//! * how many characters of the previous chunk's tail must be deleted because
//!   this chunk repeats that word in full (only with `backspace` enabled).
//!
//! Nothing here touches audio, IO or the injector: it is all string logic, so
//! the whole policy is covered by unit tests below.

use std::fmt;

/// How this stitcher should behave. Mirrors `[streaming]` in `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeamOptions {
    /// Drop head words that the previous chunk already typed.
    pub dedupe: bool,
    /// Delete the last word of the previous chunk when this one spells it out
    /// in full (a word that the cut truncated) before typing the new text.
    pub backspace: bool,
    /// Treat near-identical words (diacritics, ZWNJ, one typo) as the same word.
    pub fuzzy: bool,
    /// Upper bound on how many head words may be treated as the repeated
    /// overlap. The overlap audio is a few hundred milliseconds, so a handful of
    /// words is plenty — and the cap keeps a speaker who genuinely repeats a
    /// phrase from losing text.
    pub max_overlap_words: usize,
}

impl Default for SeamOptions {
    fn default() -> Self {
        Self {
            dedupe: true,
            backspace: true,
            fuzzy: true,
            max_overlap_words: 6,
        }
    }
}

impl SeamOptions {
    /// All repairs off: every chunk is injected exactly as transcribed.
    pub const fn off() -> Self {
        Self {
            dedupe: false,
            backspace: false,
            fuzzy: false,
            max_overlap_words: 0,
        }
    }

    /// Builds the options from the `[streaming]` settings section.
    pub fn from_streaming(cfg: &crate::config::settings::StreamingSettings) -> Self {
        if !cfg.seam_merge {
            return Self::off();
        }
        Self {
            dedupe: true,
            backspace: cfg.seam_backspace,
            fuzzy: cfg.seam_fuzzy,
            max_overlap_words: cfg.seam_max_words,
        }
    }
}

/// What to do with one chunk of transcript.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeamMerge {
    /// Text to inject for this chunk (empty ⇒ the chunk was pure overlap).
    pub text: String,
    /// Head words dropped because the previous chunk already typed them.
    pub dropped_words: usize,
    /// Characters to delete from the previous chunk's tail before typing.
    pub backspaces: usize,
}

impl fmt::Display for SeamMerge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "text={:?} dropped={} backspaces={}",
            self.text, self.dropped_words, self.backspaces
        )
    }
}

/// Stitches consecutive chunks of one dictation session into one stream.
///
/// Keeps only the tail of what was actually typed (a few words) — enough to
/// recognise the overlap, nothing that grows with a long session. Call
/// [`SeamStitcher::reset`] when a new recording starts: two dictations in the
/// same document may legitimately start with the same words.
#[derive(Debug)]
pub struct SeamStitcher {
    opts: SeamOptions,
    /// Last words of the text injected for this session, in order.
    tail: Vec<String>,
    /// Trailing characters of the previous chunk after its last word (spaces, tabs, newlines, or empty).
    prev_trailing: Option<String>,
}

/// Upper bound on the retained tail: the dedupe cap plus a small margin so the
/// stitcher can still see the words around it.
const TAIL_WORDS: usize = 16;
/// Longest fragment we are willing to delete with backspaces.
const MAX_BACKSPACES: usize = 24;
/// Shortest word that may be treated as the fragment of a truncated word.
/// Below this, ordinary complete words start to collide with longer ones.
const MIN_FRAGMENT: usize = 4;
/// Comparison keys (Persian, no joiners) of common complete words that a longer
/// word happens to start with (کار/کارخانه, دست/دستگاه, روز/روزنامه…). They must
/// never be deleted as a "truncated fragment": the chance that the speaker simply
/// used the short word is far too high for the payoff.
const NEVER_FRAGMENTS: &[&str] = &[
    "است", "باشد", "همین", "طور", "مورد", "وقت", "دست", "کار", "روز", "سال", "ماه", "باز", "بار",
    "زبان", "کتاب", "بیا", "بگو", "بچه", "مثل", "حتی", "ولی", "هرگز",
];

impl SeamStitcher {
    pub fn new(opts: SeamOptions) -> Self {
        Self {
            opts,
            tail: Vec::new(),
            prev_trailing: None,
        }
    }

    /// Forgets the previous chunk (new recording session).
    pub fn reset(&mut self) {
        self.tail.clear();
        self.prev_trailing = None;
    }

    /// Words remembered from the previous chunks of this session.
    pub fn tail_words(&self) -> usize {
        self.tail.len()
    }

    /// Stitches `text` (a fresh chunk transcript) onto the running session and
    /// records what was typed so the next chunk can be stitched too.
    pub fn stitch(&mut self, text: &str) -> SeamMerge {
        let head: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
        if head.is_empty() {
            return SeamMerge::default();
        }

        let mut merge = SeamMerge::default();
        let mut start = 0usize;

        if self.opts.dedupe {
            start = self.repeated_head_words(&head);
            merge.dropped_words = start;
        }

        if start == 0 {
            merge.backspaces = self.truncated_tail_word(&head);
        }
        if merge.backspaces > 0 {
            // The truncated fragment is about to be deleted from the document,
            // so it is no longer part of the typed text; this chunk's complete
            // word (and everything after it) replaces it.
            self.tail.pop();
        }

        let injected: Vec<String> = head[start..].to_vec();
        // Preserve raw text formatting (multiple spaces, tabs, newlines) for the
        // un-dropped remainder rather than collapsing them with `join(" ")`.
        // Delimiters (spaces, newlines, tabs) between the last dropped word and the
        // un-dropped remainder are preserved.
        // If all words are dropped as duplicate overlap (start >= head.len()), merge.text is empty.
        merge.text = if start >= head.len() {
            String::new()
        } else if start == 0 {
            text.to_string()
        } else {
            Self::find_word_remainder_after_drop(text, start)
                .unwrap_or("")
                .to_string()
        };
        self.remember(&injected);
        if start < head.len() {
            self.prev_trailing = Self::extract_trailing(text);
        }
        merge
    }

    /// Extracts characters following the last non-whitespace word in `text`.
    /// Returns `None` if `text` contains no non-whitespace characters.
    fn extract_trailing(text: &str) -> Option<String> {
        let trimmed = text.trim_end();
        if trimmed.is_empty() {
            None
        } else {
            Some(text[trimmed.len()..].to_string())
        }
    }

    /// Finds the slice of `text` starting after the last dropped word (`drop_count - 1`).
    ///
    /// Preserves delimiters separating the last dropped word from the remainder of the text
    /// (especially newlines, tabs, and multiple spaces).
    /// If the delimiter is just a single normal space, returns the slice starting at the next word.
    fn find_word_remainder_after_drop(text: &str, drop_count: usize) -> Option<&str> {
        if drop_count == 0 {
            return Some(text);
        }
        let mut words_seen = 0;
        let mut in_word = false;
        let mut last_dropped_word_end = None;
        let mut next_word_start = None;

        for (idx, ch) in text.char_indices() {
            if ch.is_whitespace() {
                if in_word {
                    in_word = false;
                    words_seen += 1;
                    if words_seen == drop_count {
                        last_dropped_word_end = Some(idx);
                    }
                }
            } else if !in_word {
                in_word = true;
                if words_seen == drop_count {
                    next_word_start = Some(idx);
                    break;
                }
            }
        }

        if in_word && words_seen + 1 == drop_count {
            last_dropped_word_end = Some(text.len());
        }

        let end = last_dropped_word_end?;
        let next_start = next_word_start.unwrap_or(text.len());
        let delimiter = &text[end..next_start];

        // Preserve delimiters especially with newlines, tabs, or multiple spaces.
        // A single ordinary space yields the slice starting at next_word_start.
        if delimiter.contains('\n')
            || delimiter.contains('\r')
            || delimiter.contains('\t')
            || delimiter.chars().count() > 1
        {
            Some(&text[end..])
        } else {
            Some(&text[next_start..])
        }
    }

    /// Largest `k` such that the last `k` remembered words equal the first `k`
    /// words of `head` (anchored at the seam, contiguous).
    ///
    /// The match is deliberately *equal-or-one-slip*, never prefix-shaped: a
    /// remembered word that is merely the beginning of the new word is a word the
    /// cut truncated, and dropping it would leave the fragment in the document —
    /// that case belongs to [`Self::truncated_tail_word`].
    fn repeated_head_words(&self, head: &[String]) -> usize {
        let cap = self
            .opts
            .max_overlap_words
            .min(self.tail.len())
            .min(head.len());
        let mut best = 0usize;
        for k in 1..=cap {
            let tail_slice = &self.tail[self.tail.len() - k..];
            let head_slice = &head[..k];
            let matches = tail_slice
                .iter()
                .zip(head_slice)
                .all(|(a, b)| same_word(a, b, self.opts.fuzzy));
            if matches {
                best = k;
            }
        }
        best
    }

    /// Characters to backspace when this chunk completes a word the cut
    /// truncated at the end of the previous chunk.
    ///
    /// Deliberately strict: the remembered word must be a real prefix of the new
    /// first word (so the new text is its completion), it must be at least three
    /// characters long (short words like «می» are complete words on their own and
    /// deleting them would eat real text), and it must be a plausible fragment
    /// length. When in doubt we return 0 and only fix the duplication, not the
    /// truncation.
    fn truncated_tail_word(&self, head: &[String]) -> usize {
        if !self.opts.backspace {
            return 0;
        }
        // If after the candidate incomplete word there is whitespace (space, newline, tab),
        // disable Backspace repair: the word was not truncated mid-word by the chunk seam.
        // Speculative deletion of completed words is explicitly prevented.
        if let Some(trailing) = &self.prev_trailing {
            if trailing.chars().any(char::is_whitespace) {
                return 0;
            }
        }
        let (Some(tail_word), Some(head_word)) = (self.tail.last(), head.first()) else {
            return 0;
        };
        let tail_key = compare_key(tail_word);
        let head_key = compare_key(head_word);
        let tail_len = tail_key.chars().count();
        let head_len = head_key.chars().count();
        // Strict on purpose: a fragment is only recognised mid-word (≥ `MIN_FRAGMENT`
        // characters), only when the completed word is clearly longer, and never
        // for a word that is also a common word on its own (see `NEVER_FRAGMENTS`).
        if tail_len < MIN_FRAGMENT || head_len < tail_len + 2 || MAX_BACKSPACES < tail_len {
            return 0;
        }
        if NEVER_FRAGMENTS.contains(&tail_key.as_str()) {
            return 0;
        }
        if !head_key.starts_with(&tail_key) {
            return 0;
        }

        // Count what was really typed: the raw word as injected, not its
        // comparison key (ZWNJ and Persian letters are what the editor holds).
        tail_word.chars().count().min(MAX_BACKSPACES)
    }

    fn remember(&mut self, injected: &[String]) {
        for word in injected {
            self.tail.push(word.clone());
        }
        if self.tail.len() > TAIL_WORDS {
            let extra = self.tail.len() - TAIL_WORDS;
            self.tail.drain(..extra);
        }
    }
}

/// True when two words are the same word for seam purposes: identical apart from
/// formatting, or one recognition slip away (with `fuzzy`).
fn same_word(a: &str, b: &str, fuzzy: bool) -> bool {
    let (ka, kb) = (compare_key(a), compare_key(b));
    if ka == kb {
        return true;
    }
    if !fuzzy || ka.is_empty() || kb.is_empty() {
        return false;
    }
    let (short, long) = if ka.chars().count() <= kb.chars().count() {
        (&ka, &kb)
    } else {
        (&kb, &ka)
    };
    // A single recognition slip (one substitution/insertion/deletion).
    short.chars().count() >= 4 && within_one_edit(short, long)
}

/// Comparison key: lowercase, Persian/Arabic letters unified, diacritics and
/// joiners removed, surrounding punctuation stripped.
fn compare_key(word: &str) -> String {
    let unified: String = word
        .chars()
        .filter(|c| !is_ignorable(*c))
        .map(unify_letter)
        .collect::<String>()
        .to_lowercase();
    let trimmed = unified.trim_matches(|c: char| !c.is_alphanumeric());
    trimmed.to_string()
}

fn is_ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{064B}'..='\u{0652}' // Arabic harakat
        | '\u{0670}'            // superscript alef
        | '\u{0640}'            // tatweel
        | '\u{200C}'            // ZWNJ (نیم‌فاصله)
        | '\u{200D}'            // ZWJ
        | '\u{200E}' | '\u{200F}' // bidi marks
        | '\u{FEFF}'            // BOM
        | '\u{0653}'..='\u{0655}'
    )
}

/// Arabic → Persian codepoints, mirroring the normalizer's table (kept local so
/// the stitcher has no dependency on the pipeline order).
fn unify_letter(c: char) -> char {
    match c {
        'ي' | 'ى' => 'ی',
        'ك' => 'ک',
        'ة' => 'ه',
        'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
        'ؤ' => 'و',
        'ئ' => 'ی',
        _ => c,
    }
}

/// True when the edit distance between `a` and `b` is at most one.
/// Both inputs are short (single words), so the simple DP is fine.
fn within_one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    // Rolling row of the edit-distance DP; bail out as soon as every cell
    // exceeds 1 (a cheap early exit for genuinely different words).
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(cur[j]);
        }
        if row_min > 1 {
            return false;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stitcher() -> SeamStitcher {
        SeamStitcher::new(SeamOptions::default())
    }

    /// Simulates a session, returning the text the editor would contain.
    fn run_session(chunks: &[&str]) -> (String, Vec<SeamMerge>) {
        let mut s = stitcher();
        let mut typed: Vec<String> = Vec::new();
        let mut merges = Vec::new();
        for chunk in chunks {
            let merge = s.stitch(chunk);
            // Backspaces delete characters from the end of the typed text.
            if merge.backspaces > 0 {
                let mut joined = typed.join(" ");
                let keep = joined.chars().count().saturating_sub(merge.backspaces);
                joined = joined.chars().take(keep).collect();
                typed = split_words(&joined);
            }
            if !merge.text.is_empty() {
                typed.push(merge.text.clone());
            }
            merges.push(merge);
        }
        (
            typed
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            merges,
        )
    }

    fn split_words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn repeated_overlap_words_are_dropped() {
        let (typed, merges) =
            run_session(&["سلام این یک تست طولانی است", "تست طولانی است اما ادامه دارد"]);
        assert_eq!(typed, "سلام این یک تست طولانی است اما ادامه دارد");
        assert_eq!(merges[1].dropped_words, 3);
        assert_eq!(merges[1].backspaces, 0);
    }

    #[test]
    fn unrelated_chunks_keep_every_word() {
        let (typed, merges) = run_session(&["یک دو سه", "چهار پنج شش"]);
        assert_eq!(typed, "یک دو سه چهار پنج شش");
        assert_eq!(merges[1].dropped_words, 0);
    }

    #[test]
    fn a_single_duplicated_word_is_dropped() {
        let (typed, merges) = run_session(&["امروز هوا خوب است", "است و فردا بهتر"]);
        assert_eq!(typed, "امروز هوا خوب است و فردا بهتر");
        assert_eq!(merges[1].dropped_words, 1);
    }

    #[test]
    fn punctuation_and_spacing_do_not_hide_a_duplicate() {
        let (typed, merges) = run_session(&["این جمله تمام شد.", "شد ولی ادامه دارد"]);
        assert_eq!(typed, "این جمله تمام شد. ولی ادامه دارد");
        assert_eq!(merges[1].dropped_words, 1);
    }

    #[test]
    fn arabic_codepoints_and_zwnj_still_match() {
        // Arabic yeh/kaf + ZWNJ vs the Persian forms the normalizer produces.
        let (typed, merges) = run_session(&["من مي‌خواهم بروم", "میخواهم بروم خانه"]);
        assert_eq!(typed, "من مي‌خواهم بروم خانه");
        assert_eq!(merges[1].dropped_words, 2);
    }

    #[test]
    fn a_truncated_word_is_replaced_with_backspaces() {
        let mut s = stitcher();
        assert_eq!(s.stitch("و بعد او گفت می‌خوا").text, "و بعد او گفت می‌خوا");
        let merge = s.stitch("می‌خواهم بروم");
        assert_eq!(merge.dropped_words, 0);
        assert_eq!(merge.backspaces, "می‌خوا".chars().count());
        assert_eq!(merge.text, "می‌خواهم بروم");

        // The session reads as one sentence: the fragment is gone and the full
        // word took its place.
        let mut s = stitcher();
        let mut typed = String::new();
        for chunk in ["و بعد او گفت می‌خوا", "می‌خواهم بروم"] {
            let merge = s.stitch(chunk);
            for _ in 0..merge.backspaces {
                typed.pop();
            }
            typed.push_str(&merge.text);
        }
        assert_eq!(typed, "و بعد او گفت می‌خواهم بروم");
    }

    #[test]
    fn a_short_tail_word_is_never_deleted() {
        // «می» is a complete word; deleting it would eat real text.
        let mut s = stitcher();
        s.stitch("او گفت می");
        let merge = s.stitch("می‌خواهم بروم");
        assert_eq!(merge.backspaces, 0);
        assert_eq!(merge.text, "می‌خواهم بروم");
    }

    #[test]
    fn backspace_can_be_disabled() {
        let mut s = SeamStitcher::new(SeamOptions {
            backspace: false,
            ..SeamOptions::default()
        });
        s.stitch("و بعد گفت می‌خوا");
        let merge = s.stitch("می‌خواهم بروم");
        assert_eq!(merge.backspaces, 0);
        assert_eq!(merge.dropped_words, 0);
        assert_eq!(merge.text, "می‌خواهم بروم");
    }

    #[test]
    fn dedupe_can_be_disabled_entirely() {
        let mut s = SeamStitcher::new(SeamOptions::off());
        s.stitch("سلام این یک تست");
        let merge = s.stitch("این یک تست دیگر");
        assert_eq!(merge.dropped_words, 0);
        assert_eq!(merge.text, "این یک تست دیگر");
    }

    #[test]
    fn overlap_dedupe_is_capped() {
        let mut s = SeamStitcher::new(SeamOptions {
            max_overlap_words: 2,
            ..SeamOptions::default()
        });
        s.stitch("یک دو سه چهار پنج");
        // Two repeated words: inside the cap, so they are dropped.
        let merge = s.stitch("چهار پنج شش");
        assert_eq!(merge.dropped_words, 2);
        assert_eq!(merge.text, "شش");

        // Three repeated words with a cap of two: the run does not fit, so it is
        // left alone rather than half-eaten (duplicates are the safe failure).
        let mut s = SeamStitcher::new(SeamOptions {
            max_overlap_words: 2,
            ..SeamOptions::default()
        });
        s.stitch("یک دو سه چهار پنج");
        let merge = s.stitch("سه چهار پنج شش");
        assert_eq!(merge.dropped_words, 0);
        assert_eq!(merge.text, "سه چهار پنج شش");
    }

    #[test]
    fn a_pure_overlap_chunk_injects_nothing() {
        let mut s = stitcher();
        s.stitch("آخرین کلمه‌ها همین بود");
        let merge = s.stitch("کلمه‌ها همین بود");
        assert_eq!(merge.text, "");
        assert_eq!(merge.dropped_words, 3);
    }

    #[test]
    fn fuzzy_matching_absorbs_one_slip() {
        // A one-letter recognition slip on the repeated word does not leave the
        // word stranded at the start of the next chunk.
        let mut s = stitcher();
        s.stitch("او گفت سلام");
        let merge = s.stitch("سلیم و رفت");
        assert_eq!(merge.dropped_words, 1);
        assert_eq!(merge.text, "و رفت");

        // A longer form of the last word is *not* treated as a duplicate (the
        // fragment has to be removed, not skipped) — that is the backspace case.
        let mut s = stitcher();
        s.stitch("و درباره کامپیوتر");
        let merge = s.stitch("کامپیوترها را ببین");
        assert_eq!(merge.dropped_words, 0);
        assert_eq!(merge.backspaces, "کامپیوتر".chars().count());
        assert_eq!(merge.text, "کامپیوترها را ببین");
    }

    #[test]
    fn a_common_word_is_never_deleted_as_a_fragment() {
        // «کار» vs «کارخانه»: the short word is far more likely to be real.
        let mut s = stitcher();
        s.stitch("من با کار");
        let merge = s.stitch("کارخانه صحبت کردم");
        assert_eq!(merge.backspaces, 0);
        assert_eq!(merge.dropped_words, 0);
        assert_eq!(merge.text, "کارخانه صحبت کردم");
    }

    #[test]
    fn reset_starts_a_clean_session() {
        let mut s = stitcher();
        s.stitch("این یک جمله است");
        s.reset();
        assert_eq!(s.tail_words(), 0);
        let merge = s.stitch("این یک جمله است");
        assert_eq!(merge.dropped_words, 0);
        assert_eq!(merge.text, "این یک جمله است");
    }

    #[test]
    fn tail_memory_stays_bounded_in_a_long_session() {
        let mut s = stitcher();
        for i in 0..500 {
            s.stitch(&format!("کلمه شماره {i} ادامه"));
        }
        assert!(s.tail_words() <= TAIL_WORDS);
    }

    #[test]
    fn a_genuine_repetition_inside_a_chunk_is_untouched() {
        let (typed, _) = run_session(&["خیلی خیلی خوب بود", "و تمام"]);
        assert_eq!(typed, "خیلی خیلی خوب بود و تمام");
    }

    #[test]
    fn empty_chunk_is_a_noop() {
        let mut s = stitcher();
        s.stitch("سلام دنیا");
        let merge = s.stitch("   ");
        assert_eq!(merge, SeamMerge::default());
        assert_eq!(s.tail_words(), 2);
    }

    #[test]
    fn one_edit_helpers_behave() {
        assert!(within_one_edit("کامپیوتر", "کامپیوتر"));
        assert!(!within_one_edit("کامپیوتر", "کامپیوترها"));
        assert!(within_one_edit("سلام", "سلیم"));
        assert!(!within_one_edit("سلام", "درود"));
        assert!(same_word("خوب", "خوب.", true));
        assert!(!same_word("خوب", "بد", true));
        // Not prefix-shaped: a longer form is a different word here.
        assert!(!same_word("کار", "کارخانه", true));
    }

    #[test]
    fn seam_preserves_multiple_spaces_and_newlines_in_raw_mode() {
        let mut s = stitcher();
        let chunk1 = "خط اول   فاصله زیاد\nخط دوم";
        let m1 = s.stitch(chunk1);
        assert_eq!(
            m1.text, chunk1,
            "first chunk must retain original raw whitespace"
        );
        assert_eq!(m1.dropped_words, 0);

        // Chunk 2 repeats "فاصله زیاد\nخط دوم" and adds "و خط سوم   چهار"
        let chunk2 = "فاصله زیاد\nخط دوم و خط سوم   چهار";
        let m2 = s.stitch(chunk2);
        assert_eq!(m2.dropped_words, 4);
        assert_eq!(
            m2.text, "و خط سوم   چهار",
            "overlapping head dropped while preserving internal whitespace of remainder"
        );
    }

    #[test]
    fn seam_trailing_whitespace_prevents_speculative_backspace_repair() {
        let mut s = stitcher();
        let m1 = s.stitch("می‌خوا  ");
        assert_eq!(m1.dropped_words, 0);
        assert_eq!(m1.backspaces, 0);

        let m2 = s.stitch("می‌خواهم");
        assert_eq!(
            m2.backspaces, 0,
            "trailing spaces on previous chunk prove word was not cut mid-word; zero backspaces"
        );
        assert_eq!(m2.dropped_words, 0);
        assert_eq!(m2.text, "می‌خواهم");
    }

    #[test]
    fn seam_overlap_preserves_newline_separator() {
        let mut s = stitcher();
        let m1 = s.stitch("سلام دنیا");
        assert_eq!(m1.dropped_words, 0);

        // Chunk 2 overlaps "دنیا" and continues after newline:
        let m2 = s.stitch("دنیا\nخط دوم ادامه");
        assert_eq!(m2.dropped_words, 1);
        assert_eq!(
            m2.text, "\nخط دوم ادامه",
            "newline following dropped word must be preserved"
        );
    }

    #[test]
    fn seam_overlap_preserves_tab_and_multiple_spaces_separator() {
        let mut s = stitcher();
        let m1 = s.stitch("سلام دنیا");
        assert_eq!(m1.dropped_words, 0);

        // Chunk 2 overlaps "دنیا" and continues after tab and spaces:
        let m2 = s.stitch("دنیا\t   بخش جدید");
        assert_eq!(m2.dropped_words, 1);
        assert_eq!(
            m2.text, "\t   بخش جدید",
            "tab and multiple spaces following dropped word must be preserved"
        );
    }

    #[test]
    fn seam_complete_duplicate_chunk_produces_empty_text_and_zero_backspaces() {
        let mut s = stitcher();
        let m1 = s.stitch("سلام دنیا");
        assert_eq!(m1.dropped_words, 0);

        // Exact duplicate
        let m2 = s.stitch("سلام دنیا");
        assert_eq!(m2.dropped_words, 2);
        assert_eq!(m2.backspaces, 0);
        assert_eq!(
            m2.text, "",
            "complete duplicate chunk must yield empty text"
        );
    }
}
