//! Boundary policy for contiguous dictation and continued sessions.
//!
//! Inserts clean separators between chunks and across continued dictations
//! without altering Persian text rules, destroying raw whitespace, or
//! deleting text.
//!
//! # Rules
//! 1. In a valid continuation, a single space `' '` is placed between two pieces
//!    of text.
//! 2. If whitespace (space, tab, newline) already exists at the end of the previous
//!    text or at the start of the incoming text, no extra separator is made.
//! 3. No space is added before attached punctuation (e.g. `.`, `،`, `!`, `؟`, `:`, `؛`, `…`, `»`, `)`).
//! 4. If `backspaces > 0`, the new chunk is repairing a truncated word tail from
//!    the previous chunk. No boundary separator is added so that repaired words
//!    (e.g. "میخوا" -> "میخواهم") are not split into two words.
//! 5. Raw mode whitespace and newlines within the text are preserved without
//!    being collapsed by `split_whitespace`.

use crate::output::target::TargetIdentity;
use crate::state::session::SessionId;

/// Characters that attach to the preceding word without an intervening space.
pub fn is_attached_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '.' | '،'
            | ','
            | '!'
            | '؟'
            | '?'
            | ':'
            | '؛'
            | ';'
            | '…'
            | ')'
            | ']'
            | '}'
            | '»'
            | '٪'
            | '%'
            | '”'
            | '’'
    )
}

/// Zero-width non-joiner and joiner codes.
pub fn is_non_spacing_joiner(ch: char) -> bool {
    ch == '\u{200C}' || ch == '\u{200D}'
}

/// Decides whether a single space separator is required between `prev_last_char`
/// and the beginning of `next_text`.
pub fn needs_boundary_space(prev_last_char: Option<char>, next_text: &str) -> bool {
    let Some(prev_char) = prev_last_char else {
        // No previous text in memory.
        return false;
    };
    if prev_char.is_whitespace() || is_non_spacing_joiner(prev_char) {
        // Preceding text already ended with whitespace or ZWNJ.
        return false;
    }
    let Some(next_first_char) = next_text.chars().next() else {
        // Empty incoming text.
        return false;
    };
    if next_first_char.is_whitespace() || is_non_spacing_joiner(next_first_char) {
        // Incoming text already begins with whitespace or ZWNJ.
        return false;
    }
    if is_attached_punctuation(next_first_char) {
        // Attached punctuation attaches directly to preceding word.
        return false;
    }
    true
}

/// Active memory of the last successfully injected text boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryState {
    /// The target window where text was last successfully injected.
    pub target: Option<TargetIdentity>,
    /// The session id of the last successfully injected text.
    pub session: Option<SessionId>,
    /// The last character of the injected text.
    pub last_char: char,
}

/// Tracks boundary continuity across consecutive chunks and continued sessions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoundaryTracker {
    state: Option<BoundaryState>,
}

impl BoundaryTracker {
    pub fn new() -> Self {
        Self { state: None }
    }

    /// Whether there is active, valid boundary memory matching `target`.
    ///
    /// # Heuristic Definition (تخمین بر اساس همان پنجره)
    /// Valid if and only if both the remembered target and the current target are
    /// known (`Some`), and both share the exact same `hwnd` and `pid`.
    ///
    /// # Limitation: Caret Continuity (اثبات پیوستگی مکان‌نما نیست)
    /// Matching `hwnd` and `pid` is merely a heuristic estimate based on the same window,
    /// NOT proof of caret/insertion-point continuity within the document.
    /// Manual cursor movement or text editing by the user between sessions cannot be
    /// reliably detected via Win32 polling; detection of manual edits remains unimplemented.
    ///
    /// # Unknown Destination (مقصد نامعلوم)
    /// An unknown destination (`None`) is NEVER considered valid.
    /// Window title and display information are deliberately ignored, as titles can change
    /// dynamically during user typing without changing the underlying window or process.
    pub fn is_valid_for(&self, target: &Option<TargetIdentity>) -> bool {
        let (Some(st), Some(curr)) = (&self.state, target) else {
            return false;
        };
        let Some(prev) = &st.target else {
            return false;
        };
        prev.hwnd == curr.hwnd && prev.pid == curr.pid
    }

    /// Last remembered character, if memory is active and valid for `target`.
    pub fn last_char_for(&self, target: &Option<TargetIdentity>) -> Option<char> {
        if self.is_valid_for(target) {
            self.state.as_ref().map(|st| st.last_char)
        } else {
            None
        }
    }

    /// Session id of last successful injection, if memory is active and valid for `target`.
    pub fn last_session_for(&self, target: &Option<TargetIdentity>) -> Option<SessionId> {
        if self.is_valid_for(target) {
            self.state.as_ref().and_then(|st| st.session)
        } else {
            None
        }
    }

    /// Evaluates incoming text against current boundary memory.
    ///
    /// # Word Repair Guarantee
    /// If `backspaces > 0`, the incoming chunk is replacing a truncated word tail.
    /// No boundary space is prepended, so the word remains continuous.
    ///
    /// # Spacing Guarantee
    /// If boundary memory is valid for `target` and `backspaces == 0`, checks whether
    /// a single space separator is needed and prepends `" "` if so.
    /// Internal multiple spaces and newlines of `text` are untouched.
    pub fn apply(&self, text: &str, backspaces: usize, target: &Option<TargetIdentity>) -> String {
        if text.is_empty() || backspaces > 0 {
            return text.to_string();
        }

        if !self.is_valid_for(target) {
            return text.to_string();
        }

        let prev_char = self.state.as_ref().map(|st| st.last_char);
        if needs_boundary_space(prev_char, text) {
            format!(" {text}")
        } else {
            text.to_string()
        }
    }

    /// Updates boundary memory after a completely accepted injection (`InjectOutcome::Complete`).
    ///
    /// Empty text does NOT advance memory.
    pub fn record_success(
        &mut self,
        typed_text: &str,
        session: Option<SessionId>,
        target: Option<TargetIdentity>,
    ) {
        if let Some(last_char) = typed_text.chars().last() {
            self.state = Some(BoundaryState {
                target,
                session,
                last_char,
            });
        }
    }

    /// Invalidates boundary memory.
    ///
    /// Must be called on destination refusal, failure, partial injection,
    /// explicit cancellation, or target window change.
    pub fn invalidate(&mut self) {
        self.state = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attached_punctuation_detection() {
        assert!(is_attached_punctuation('.'));
        assert!(is_attached_punctuation('،'));
        assert!(is_attached_punctuation('!'));
        assert!(is_attached_punctuation('؟'));
        assert!(is_attached_punctuation(':'));
        assert!(is_attached_punctuation('؛'));
        assert!(is_attached_punctuation('…'));
        assert!(is_attached_punctuation('»'));
        assert!(is_attached_punctuation(')'));
        assert!(is_attached_punctuation('٪'));

        // Opening punctuation is not attached to preceding text
        assert!(!is_attached_punctuation('('));
        assert!(!is_attached_punctuation('«'));
        assert!(!is_attached_punctuation('['));
    }

    #[test]
    fn needs_boundary_space_decisions() {
        // No prior text -> false
        assert!(!needs_boundary_space(None, "سلام"));

        // Two Persian words -> true
        assert!(needs_boundary_space(Some('م'), "دوست"));

        // Preceding space or newline -> false
        assert!(!needs_boundary_space(Some(' '), "دوست"));
        assert!(!needs_boundary_space(Some('\n'), "دوست"));
        assert!(!needs_boundary_space(Some('\r'), "دوست"));
        assert!(!needs_boundary_space(Some('\t'), "دوست"));

        // Following space or newline -> false
        assert!(!needs_boundary_space(Some('م'), " دوست"));
        assert!(!needs_boundary_space(Some('م'), "\nدوست"));
        assert!(!needs_boundary_space(Some('م'), "\tدوست"));

        // Attached punctuation -> false
        assert!(!needs_boundary_space(Some('م'), "."));
        assert!(!needs_boundary_space(Some('م'), "،"));
        assert!(!needs_boundary_space(Some('م'), "!"));
        assert!(!needs_boundary_space(Some('م'), "؟"));
        assert!(!needs_boundary_space(Some('م'), ":"));
        assert!(!needs_boundary_space(Some('م'), "؛"));
        assert!(!needs_boundary_space(Some('م'), "…"));
        assert!(!needs_boundary_space(Some('م'), "»"));
        assert!(!needs_boundary_space(Some('م'), ")"));
        assert!(!needs_boundary_space(Some('م'), "٪"));

        // Opening punctuation preceded by word -> true
        assert!(needs_boundary_space(Some('م'), "(کلمه)"));
        assert!(needs_boundary_space(Some('م'), "«کلمه»"));

        // ZWNJ -> false
        assert!(!needs_boundary_space(Some('\u{200C}'), "ها"));
        assert!(!needs_boundary_space(Some('م'), "\u{200C}ها"));

        // Empty next text -> false
        assert!(!needs_boundary_space(Some('م'), ""));
    }

    #[test]
    fn tracker_word_repair_does_not_split_words() {
        let mut tracker = BoundaryTracker::new();
        let target = Some(TargetIdentity {
            hwnd: 0x1234,
            pid: 100,
            exe_path: None,
            title_at_capture: "Test".to_string(),
        });
        tracker.record_success("میخوا", Some(SessionId(1)), target.clone());

        // Repaired word with backspaces: must NOT prepend space
        let repaired = tracker.apply("میخواهم", 5, &target);
        assert_eq!(
            repaired, "میخواهم",
            "word repair must not be prepended with a space"
        );
    }

    #[test]
    fn tracker_preserves_raw_formatting() {
        let mut tracker = BoundaryTracker::new();
        let target = Some(TargetIdentity {
            hwnd: 0x1234,
            pid: 100,
            exe_path: None,
            title_at_capture: "Test".to_string(),
        });
        tracker.record_success("سلام", Some(SessionId(1)), target.clone());

        let raw_text = "متن   با   فاصله‌های   زیاد\nو خط دوم";
        let adjusted = tracker.apply(raw_text, 0, &target);
        assert_eq!(
            adjusted,
            format!(" {raw_text}"),
            "boundary space prepended without altering internal spaces or newlines"
        );
    }

    #[test]
    fn tracker_invalidation_prevents_unwanted_spacing() {
        let mut tracker = BoundaryTracker::new();
        let target1 = Some(TargetIdentity {
            hwnd: 0x1111,
            pid: 100,
            exe_path: None,
            title_at_capture: "App 1".to_string(),
        });
        let target2 = Some(TargetIdentity {
            hwnd: 0x2222,
            pid: 200,
            exe_path: None,
            title_at_capture: "App 2".to_string(),
        });

        tracker.record_success("سلام", Some(SessionId(1)), target1.clone());

        // Different destination -> no space added
        assert_eq!(tracker.apply("خوبی", 0, &target2), "خوبی");

        // Explicit invalidation -> no space added even for same destination
        tracker.invalidate();
        assert_eq!(tracker.apply("خوبی", 0, &target1), "خوبی");
    }

    #[test]
    fn tracker_validates_on_hwnd_and_pid_ignoring_title() {
        let mut tracker = BoundaryTracker::new();
        let target1 = Some(TargetIdentity {
            hwnd: 0x5000,
            pid: 1234,
            exe_path: None,
            title_at_capture: "Document - Word".to_string(),
        });
        let target2_updated_title = Some(TargetIdentity {
            hwnd: 0x5000,
            pid: 1234,
            exe_path: None,
            title_at_capture: "Document [Modified] - Word".to_string(),
        });

        tracker.record_success("سلام", Some(SessionId(1)), target1);

        // Same hwnd and pid with updated title must remain valid
        assert!(tracker.is_valid_for(&target2_updated_title));
        assert_eq!(tracker.apply("دنیا", 0, &target2_updated_title), " دنیا");
    }

    #[test]
    fn tracker_unknown_destination_is_never_valid() {
        let mut tracker = BoundaryTracker::new();
        assert!(!tracker.is_valid_for(&None));

        // Even after recording success with a valid target, None is never valid
        let target = Some(TargetIdentity {
            hwnd: 0x5000,
            pid: 1234,
            exe_path: None,
            title_at_capture: "App".to_string(),
        });
        tracker.record_success("سلام", Some(SessionId(1)), target);
        assert!(!tracker.is_valid_for(&None));
        assert_eq!(tracker.apply("دنیا", 0, &None), "دنیا");

        // And recording success with None target must never be valid for None
        tracker.record_success("سلام", Some(SessionId(2)), None);
        assert!(!tracker.is_valid_for(&None));
    }

    #[test]
    fn tracker_different_pid_or_hwnd_is_invalid() {
        let mut tracker = BoundaryTracker::new();
        let target1 = Some(TargetIdentity {
            hwnd: 0x5000,
            pid: 100,
            exe_path: None,
            title_at_capture: "App".to_string(),
        });
        let same_hwnd_different_pid = Some(TargetIdentity {
            hwnd: 0x5000,
            pid: 999,
            exe_path: None,
            title_at_capture: "App".to_string(),
        });
        let different_hwnd_same_pid = Some(TargetIdentity {
            hwnd: 0x6000,
            pid: 100,
            exe_path: None,
            title_at_capture: "App".to_string(),
        });

        tracker.record_success("سلام", Some(SessionId(1)), target1);
        assert!(!tracker.is_valid_for(&same_hwnd_different_pid));
        assert!(!tracker.is_valid_for(&different_hwnd_same_pid));
    }
}
