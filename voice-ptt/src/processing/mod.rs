//! Post-processing pipeline: normalization → dictionary correction, plus the
//! chunk-seam stitcher (`seam`) that keeps a long, chunked dictation reading as
//! one continuous piece of text, and the speech-command recogniser
//! (`commands`) that decides whether a phrase is an instruction instead of
//! words — last, so nothing downstream can undo the decision.

pub mod boundary;
pub mod commands;
pub mod dictionary;
pub mod formal;
pub mod normalizer;
pub mod quickfix;
pub mod seam;

pub use boundary::{is_attached_punctuation, needs_boundary_space, BoundaryState, BoundaryTracker};
pub use dictionary::{Correction, Dictionary};
pub use normalizer::Normalizer;
pub use seam::{SeamMerge, SeamOptions, SeamStitcher};

/// How much the pipeline is allowed to change what the engine said.
///
/// T0 measured that no such choice existed: [`process_text`] always
/// normalised and always applied the dictionary, and the only way to get
/// something closer to the raw transcript was to swap in an empty dictionary —
/// which does not even switch off normalisation. For an app whose whole job is
/// writing into someone else's document, "type it exactly as heard" is a
/// setting users are entitled to, and it costs one enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextMode {
    /// Nothing at all: the engine's string is typed verbatim, byte for byte.
    Raw,
    /// Only the changes nobody could disagree with — Arabic codepoints the
    /// Persian layout cannot produce, collapsed whitespace, punctuation
    /// attached to its own word, and the dictionary.
    ///
    /// The half-space (ZWNJ) inference is the one step held back, because it is
    /// the only one that reads Persian morphology and can therefore alter a
    /// word it has not seen before (T0-001…004, measured).
    Conservative,
    /// `Conservative` plus the half-space rules. The default, and what this app
    /// has always done.
    #[default]
    Standard,
    /// `Standard` plus written-register spacing: one space after a clause mark,
    /// and one where a Persian word meets a Latin word or a number.
    ///
    /// Word-independent by construction — see [`formal`]. It never rewrites a
    /// word, never removes anything, and never invents a mark, because
    /// "formal" is a register of *presentation* and not a licence to say what
    /// the user meant.
    Formal,
}

impl TextMode {
    /// Parses a settings value, falling back to [`TextMode::Standard`].
    ///
    /// Falling *forward* to the historical behaviour is deliberate: a typo in
    /// `config.toml` must not silently turn post-processing off, because the
    /// user would find out from the text they typed into a document.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "raw" => TextMode::Raw,
            "conservative" => TextMode::Conservative,
            "formal" | "رسمی" => TextMode::Formal,
            _ => TextMode::Standard,
        }
    }

    /// The value written back to `config.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            TextMode::Raw => "raw",
            TextMode::Conservative => "conservative",
            TextMode::Standard => "standard",
            TextMode::Formal => "formal",
        }
    }
}

/// What the pipeline is allowed to do, beyond the mode itself.
///
/// A struct rather than a bare enum so the seams stay open: adding a per-rule
/// switch later should not mean re-plumbing every call site.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessingOptions {
    pub mode: TextMode,
    /// Whether spoken commands ("خط جدید", "ویرگول", …) are recognised.
    ///
    /// Separate from the mode on purpose: a mode says how much the *pipeline*
    /// may change the text, and this says whether a phrase is an **instruction**
    /// at all. It applies in every mode — including `Raw`, which is exactly
    /// what "عبور خام مطابق گزینهٔ فرمان" asks for — because a user who turned
    /// commands on wants the line break whether or not they also asked for
    /// their words verbatim.
    pub commands: bool,
    /// Which formal-writing groups run — only consulted in
    /// [`TextMode::Formal`], and carried here so a preview and the coordinator
    /// agree about what the mode does.
    pub formal: formal::FormalOptions,
}

impl ProcessingOptions {
    pub fn new(mode: TextMode) -> Self {
        Self {
            mode,
            commands: false,
            formal: formal::FormalOptions::default(),
        }
    }

    /// Sets the command switch; the rest is unchanged.
    pub fn with_commands(mut self, commands: bool) -> Self {
        self.commands = commands;
        self
    }

    /// Sets the formal-writing groups; the rest is unchanged.
    pub fn with_formal(mut self, formal: formal::FormalOptions) -> Self {
        self.formal = formal;
        self
    }

    pub fn raw() -> Self {
        Self::new(TextMode::Raw)
    }

    pub fn conservative() -> Self {
        Self::new(TextMode::Conservative)
    }

    pub fn standard() -> Self {
        Self::new(TextMode::Standard)
    }

    pub fn formal() -> Self {
        Self::new(TextMode::Formal)
    }
}

/// Runs the full text post-processing pipeline in order.
pub fn process_text(text: &str, normalizer: &Normalizer, dictionary: &Dictionary) -> String {
    process_text_with(text, normalizer, dictionary, ProcessingOptions::default())
}

/// Everything one dictation's text goes through, as a value.
///
/// Exists so there is **one** implementation of "what happens to this text":
/// the coordinator runs it for real, and the dictionary panel's quick-fix
/// preview runs it to show the user the result before they save a rule. A
/// preview that re-implemented the order — mode first, general dictionary next,
/// the destination's own rules last — would sooner or later disagree with the
/// product, and a preview that lies is worse than no preview.
///
/// Borrowed rather than owned because both callers already hold these values:
/// the coordinator for the whole of a session, the panel for one frame — and
/// `Copy`, because a preview needs the same rules twice, once for each side of
/// the comparison.
#[derive(Clone, Copy)]
pub struct TextRules<'a> {
    pub mode: TextMode,
    pub normalizer: &'a Normalizer,
    /// The general dictionary: normalizer plus the user's own file rules.
    pub dictionary: &'a Dictionary,
    /// The destination's extra rules, applied **after** the general dictionary
    /// and **regardless of the mode** — they are the user's explicit
    /// instruction for that application, not part of the automatic pipeline.
    pub corrections: &'a [Correction],
    /// Whether spoken commands are recognised — the general setting, carried
    /// here so the quick-fix and profile previews run the same value the
    /// coordinator will. A preview that left the switch out would show text
    /// that is not what gets typed.
    pub commands: bool,
    /// Which formal-writing groups run, for [`TextMode::Formal`].
    pub formal: formal::FormalOptions,
}

impl TextRules<'_> {
    /// Applies everything, in the order above.
    pub fn apply(&self, text: &str) -> String {
        let processed = process_text_with(
            text,
            self.normalizer,
            self.dictionary,
            ProcessingOptions::new(self.mode)
                .with_commands(self.commands)
                .with_formal(self.formal),
        );
        if self.corrections.is_empty() {
            return processed;
        }
        // Compiled per call rather than cached: a destination holds a handful of
        // rules, and the cost sits beside an engine call measured in hundreds of
        // milliseconds.
        Dictionary::new(self.corrections.to_vec()).correct(&processed)
    }
}

/// A character a Latin word, file name, domain or URL can continue with.
///
/// Shared by the two stages that decide whether a mark ends a clause —
/// [`normalizer`]'s attachment pass and [`formal`]'s punctuation group — so the
/// two cannot drift apart about the same question. Every character a token may
/// carry on with after a `.`, `:` or `,` is here: `example.com`,
/// `http://site`, `test@site.com`, `file_name-v2.txt`, `50%`, `a=1`.
///
/// Deliberately made of ASCII only. A mark followed by a **Persian** letter is
/// the case this whole pipeline exists to repair (`سلام.خوبی` → `سلام. خوبی`),
/// and that must keep splitting.
pub(crate) fn continues_latin_token(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || "/%#&?=@_~+$-".contains(ch)
}

/// [`process_text`] with the mode chosen by the caller.
///
/// The old three-argument entry point is kept and still means `Standard`, so
/// `state::machine` — which owns the injection boundary and not this policy —
/// needs no change to gain the option; it only has to be asked for it later.
pub fn process_text_with(
    text: &str,
    normalizer: &Normalizer,
    dictionary: &Dictionary,
    options: ProcessingOptions,
) -> String {
    let processed = match options.mode {
        TextMode::Raw => text.to_string(),
        TextMode::Conservative => dictionary.correct(&normalizer.normalize_conservative(text)),
        TextMode::Standard => dictionary.correct(&normalizer.normalize(text)),
        // The ordinary pipeline first, then the formal groups: `formal` is
        // spacing on top of the same text, never a second opinion about it.
        TextMode::Formal => {
            let written = formal::apply(&normalizer.normalize(text), options.formal);
            dictionary.correct(&written)
        }
    };
    // Last, after the mode and the general dictionary: the marker a command
    // becomes must survive the whitespace pass, and no rule gets to rewrite a
    // phrase before it is recognised. See `commands` for the whole contract.
    commands::apply(&processed, options.commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_pipeline_normalizes_then_corrects() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        // Arabic kaf + mishearing in one string.
        let out = process_text("من با پاتون و لینوكس کار میکنم", &n, &d);
        assert_eq!(out, "من با پایتون و لینوکس کار می‌کنم");
    }

    /// `Raw` must be exactly that: no codepoint mapping, no trimming, no
    /// dictionary. A single leading space is the cheapest possible witness,
    /// because every stage above would have eaten it.
    #[test]
    fn raw_mode_types_the_string_verbatim() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for input in [
            "  كلمات تمام  ",
            "من با پاتون و لينوكس كار ميكنم",
            "مي‌روم",
            "این یک جمله است . واقعاً ؟",
        ] {
            assert_eq!(
                process_text_with(input, &n, &d, ProcessingOptions::raw()),
                input,
                "raw mode altered {input:?}"
            );
        }
    }

    /// The three modes are strictly ordered: each one changes less than the one
    /// before it. If a stage is ever moved across the boundary by accident, the
    /// outputs stop nesting and this fails.
    #[test]
    fn each_mode_changes_less_than_the_one_before_it() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        let input = "كتابهاي من با پاتون و لينوكس كار ميكنم كلمات";
        let standard = process_text_with(input, &n, &d, ProcessingOptions::standard());
        let conservative = process_text_with(input, &n, &d, ProcessingOptions::conservative());
        let raw = process_text_with(input, &n, &d, ProcessingOptions::raw());
        assert_ne!(
            standard, conservative,
            "the half-space step does nothing now"
        );
        assert_ne!(
            conservative, raw,
            "the codepoint/whitespace/punctuation stages do nothing now"
        );
        // Conservative is Standard minus half-spaces, and nothing else: taking
        // the half-spaces out of Standard has to reproduce it exactly.
        assert_eq!(standard.replace('\u{200c}', ""), conservative);
    }

    #[test]
    fn conservative_never_inserts_a_half_space() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for input in [
            "میروم",
            "نمیخواهم",
            "کتابها",
            "بزرگترین",
            "کلمات",
            "تمام",
            "اتمام",
            "ميكروفون",
        ] {
            let out = process_text_with(input, &n, &d, ProcessingOptions::conservative());
            assert!(
                !out.contains('\u{200c}'),
                "conservative mode invented a half-space in {out:?}"
            );
        }
        // ...while the default still does the productive ones.
        assert_eq!(
            process_text("میروم", &n, &d),
            "می\u{200c}روم",
            "Standard mode lost the half-space rules altogether"
        );
    }

    /// The three words T0 measured as broken, locked to their fixed output.
    ///
    /// They are the reason this package exists, and they are the three the
    /// suffix rule fixed *by rule* — `ات` and `ام` are not productive, so no
    /// stop-list entry was added to save them.
    #[test]
    fn the_words_the_suffix_rule_used_to_break_survive() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for (input, want) in [
            ("كلمات", "کلمات"),
            ("تمام", "تمام"),
            ("اتمام", "اتمام"),
            ("ميكروفون", "میکروفون"),
            ("کبوتر", "کبوتر"),
            ("اژدها", "اژدها"),
        ] {
            assert_eq!(process_text(input, &n, &d), want, "{input}");
        }
    }

    /// The T0 fixture, replayed.
    ///
    /// `cases.json` is measurement output, not documentation: every `current`
    /// and every entry under `modes` was produced by running the pipeline, and
    /// this test is what stops it from drifting away from a behaviour nobody
    /// re-measures. It lives inside the lib rather than in `tests/` because the
    /// canary harness runs `cargo test --lib` — a test the harness never runs
    /// would be a test nothing ever watches.
    #[test]
    fn the_t0_fixture_still_describes_this_pipeline() {
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/text-baseline/cases.json"
        ))
        .expect("the T0 fixture parses");
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        let mut checked = 0;
        for case in doc["cases"]
            .as_array()
            .expect("`cases` is an array")
            .iter()
            .filter(|c| c["stage"] != "seam")
        {
            let id = case["id"].as_str().expect("case id");
            let input = case["input"].as_str().expect("case input");
            let modes = &case["modes"];
            for (name, options) in [
                ("standard", ProcessingOptions::standard()),
                ("conservative", ProcessingOptions::conservative()),
                ("raw", ProcessingOptions::raw()),
            ] {
                let want = modes[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("{id} has no measured `{name}` output"));
                assert_eq!(
                    process_text_with(input, &n, &d, options),
                    want,
                    "{id} in {name} mode no longer matches the measured fixture"
                );
                checked += 1;
            }
            // `current` must keep meaning the default mode, or the fixture is
            // describing a mode nobody is running.
            if name_is_default(modes) {
                assert_eq!(
                    process_text(input, &n, &d),
                    case["current"].as_str().expect("case current"),
                    "{id}: `current` is stale"
                );
            }
        }
        // 18 cases × 3 modes, the three `seam` ones being replay-tested elsewhere.
        // Grew from 42 when the post-I5 scan added T0-018 (a mark inside a Latin
        // token), which is exactly what this counter is for: a fixture cannot
        // gain or lose a row without somebody saying so here.
        assert_eq!(checked, 45, "the fixture gained or lost a case");
    }

    /// The fixture is only meaningful if its `current` column is the default.
    fn name_is_default(modes: &serde_json::Value) -> bool {
        modes.get("standard").is_some()
    }

    /// An unrecognised setting value must fall **forward** to the historical
    /// behaviour, never to `Raw`: a typo in `config.toml` should not quietly
    /// stop post-processing, because the user finds out from a document.
    #[test]
    fn an_unknown_mode_name_keeps_the_old_behaviour() {
        for value in ["", "  ", "Standard", "conservative", "nonsense", "formel"] {
            assert_ne!(TextMode::parse(value), TextMode::Raw, "{value:?}");
        }
        // The spelling is matched case-insensitively, which is a deliberate
        // second line of defence against a typo, not an accident.
        assert_eq!(TextMode::parse("raw"), TextMode::Raw);
        assert_eq!(TextMode::parse("RAW"), TextMode::Raw);
        assert_eq!(TextMode::parse(" Conservative "), TextMode::Conservative);
        assert_eq!(TextMode::parse("standard"), TextMode::Standard);
        // ...and every mode round-trips through its settings spelling.
        for mode in [TextMode::Raw, TextMode::Conservative, TextMode::Standard] {
            assert_eq!(TextMode::parse(mode.as_str()), mode);
        }
    }

    #[test]
    fn hamza_is_preserved_in_conservative_and_standard_pipeline() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for input in ["مسأله", "مؤمن", "تأیید", "رأی"] {
            assert_eq!(
                process_text_with(input, &n, &d, ProcessingOptions::conservative()),
                input,
                "conservative mode must preserve hamza in {input}"
            );
            assert_eq!(
                process_text_with(input, &n, &d, ProcessingOptions::standard()),
                input,
                "standard mode must preserve hamza in {input}"
            );
        }
    }

    #[test]
    fn healthy_words_are_preserved_in_default_mode() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for input in ["کلمات", "تمام", "اتمام", "میکروفون", "کبوتر", "اژدها"]
        {
            assert_eq!(
                process_text(input, &n, &d),
                input,
                "default standard mode must not corrupt healthy word: {input}"
            );
        }
    }

    #[test]
    fn valid_samples_corrected_without_breaking_similar_words() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        // Valid corrections:
        assert_eq!(process_text("کتابها", &n, &d), "کتاب‌ها");
        assert_eq!(process_text("میروم", &n, &d), "می‌روم");
        assert_eq!(process_text("نمیخواهم", &n, &d), "نمی‌خواهم");
        assert_eq!(process_text("بزرگترین", &n, &d), "بزرگ‌ترین");

        // Similar looking healthy words must NOT be broken:
        assert_eq!(process_text("اژدها", &n, &d), "اژدها");
        assert_eq!(process_text("تنها", &n, &d), "تنها");
        assert_eq!(process_text("کبوتر", &n, &d), "کبوتر");
        assert_eq!(process_text("دختر", &n, &d), "دختر");
        assert_eq!(process_text("دفتر", &n, &d), "دفتر");
        assert_eq!(process_text("میکروفون", &n, &d), "میکروفون");
        assert_eq!(process_text("میوه", &n, &d), "میوه");
    }

    #[test]
    fn explicit_space_in_mi_rom_preserved_across_all_modes() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        let input = "می روم";
        for mode in [
            ProcessingOptions::standard(),
            ProcessingOptions::conservative(),
            ProcessingOptions::raw(),
        ] {
            assert_eq!(
                process_text_with(input, &n, &d, mode),
                input,
                "explicit space between می and روم must be preserved in {mode:?}"
            );
        }
    }

    #[test]
    fn latin_digits_are_preserved_without_conversion() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for input in ["123", "کد 456 و 789", "port 8080"] {
            assert_eq!(
                process_text_with(input, &n, &d, ProcessingOptions::standard()),
                input,
                "Latin digits must remain Latin in standard mode: {input}"
            );
            assert_eq!(
                process_text_with(input, &n, &d, ProcessingOptions::conservative()),
                input,
                "Latin digits must remain Latin in conservative mode: {input}"
            );
        }
    }

    #[test]
    fn raw_mode_bypasses_persian_normalizer_and_dictionary() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        // Input with Arabic kaf/yeh, dictionary trigger (پاتون), multi-spaces, punctuation spacing:
        let raw_input = "  كتابهاي من با پاتون و لينوكس  كار ميكنم . واقعاً ؟  ";
        let out = process_text_with(raw_input, &n, &d, ProcessingOptions::raw());
        assert_eq!(
            out, raw_input,
            "Raw mode must not touch text with Persian normalizer or dictionary"
        );
    }

    // ── commands (C1) through the real pipeline ────────────────────────

    /// Off is the default, in every mode, and it means the phrase is text.
    #[test]
    fn commands_are_off_by_default_in_every_mode() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for options in [
            ProcessingOptions::standard(),
            ProcessingOptions::conservative(),
            ProcessingOptions::raw(),
        ] {
            assert!(!options.commands);
            for input in ["خط جدید", "یک خط جدید بزن"] {
                assert_eq!(
                    process_text_with(input, &n, &d, options),
                    input,
                    "{input:?} changed with commands off under {options:?}"
                );
            }
        }
    }

    /// The marker is produced **after** the mode pipeline, so the whitespace
    /// pass cannot eat it: a newline is the whole output.
    #[test]
    fn a_command_survives_the_mode_pipeline_as_a_marker() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        assert_eq!(
            process_text_with(
                "خط جدید",
                &n,
                &d,
                ProcessingOptions::standard().with_commands(true)
            ),
            "\n"
        );
        assert_eq!(
            process_text_with(
                "پاراگراف جدید",
                &n,
                &d,
                ProcessingOptions::conservative().with_commands(true)
            ),
            "\n\n"
        );
    }

    /// Acceptance from the plan: the raw pass-through follows the command
    /// switch, not the mode. Raw keeps the Arabic yeh exactly as heard — and
    /// the command still fires, because recognition folds the spelling.
    #[test]
    fn raw_passes_text_through_and_still_recognises_commands() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        let heard = "خط جديد";
        assert_eq!(
            process_text_with(heard, &n, &d, ProcessingOptions::raw()),
            heard
        );
        assert_eq!(
            process_text_with(
                heard,
                &n,
                &d,
                ProcessingOptions::raw().with_commands(true)
            ),
            "\n"
        );
    }

    /// Text around a command still goes through the normalizer and the
    /// dictionary — the switch does not turn the pipeline off.
    #[test]
    fn text_around_a_command_is_still_processed() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        let out = process_text_with(
            "دستور كلينت نقطه",
            &n,
            &d,
            ProcessingOptions::standard().with_commands(true)
        );
        assert_eq!(out, "کلینت.", "the word was normalized, the command acted");
    }

    // ── formal (P2) through the real pipeline ─────────────────────────

    /// The mode is reachable by the spelling `config.toml` uses and by the one
    /// a Persian reader would write, and `as_str` is what round-trips back
    /// into the file and the profile drop-down.
    #[test]
    fn formal_is_reachable_by_both_spellings() {
        assert_eq!(TextMode::parse("formal"), TextMode::Formal);
        assert_eq!(TextMode::parse("رسمی"), TextMode::Formal);
        assert_eq!(TextMode::Formal.as_str(), "formal");
        assert_eq!(
            TextMode::parse(TextMode::Formal.as_str()),
            TextMode::Formal,
            "what is written back must parse as what it was"
        );
        assert_eq!(
            ProcessingOptions::formal().formal,
            formal::FormalOptions::default(),
            "the mode's groups default to on"
        );
    }

    /// Acceptance from the plan: raw and conservative behave exactly as they
    /// did before this mode existed. Raw is byte-for-byte, and conservative
    /// does not pick up the formal mode's own contribution — the script
    /// boundary — by accident.
    #[test]
    fn the_formal_groups_never_run_outside_the_formal_mode() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        for input in ["سلام،خوبی؟ چرا؟چون", "ازPython و ۱۰۰درصد"] {
            assert_eq!(
                process_text_with(input, &n, &d, ProcessingOptions::raw()),
                input,
                "raw must stay verbatim for {input:?}"
            );
        }
        assert_eq!(
            process_text_with("ازPython و ۱۰۰درصد", &n, &d, ProcessingOptions::conservative()),
            "ازPython و ۱۰۰درصد",
            "conservative gained formal spacing"
        );
    }

    /// The formal mode is the standard pipeline *plus* spacing: the
    /// normalizer runs first (so Arabic codepoints are Persian by the time the
    /// spacing rules look at them) and the dictionary still runs afterwards,
    /// so a mishearing is corrected in this mode too.
    #[test]
    fn formal_is_the_standard_pipeline_plus_spacing() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        assert_eq!(
            process_text_with("من با پاتون ازPython", &n, &d, ProcessingOptions::formal()),
            "من با پایتون از Python",
            "dictionary correction and script spacing both happened"
        );
    }

    /// Turning a group off changes only that group, through the same path the
    /// coordinator and the previews use. ASCII `?` is the witness: the
    /// normalizer has never covered it, so a space after it can only have come
    /// from the formal punctuation group.
    #[test]
    fn a_formal_group_can_be_turned_off_through_the_pipeline() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        let spacing_only = formal::FormalOptions {
            punctuation: false,
            mixed_spacing: true,
        };
        assert_eq!(
            process_text_with(
                "چرا?چون ازPython",
                &n,
                &d,
                ProcessingOptions::formal().with_formal(spacing_only)
            ),
            "چرا?چون از Python",
            "punctuation off: only the script boundary moved"
        );
        assert_eq!(
            process_text_with("چرا?چون ازPython", &n, &d, ProcessingOptions::formal()),
            "چرا? چون از Python",
            "both groups on"
        );
    }
}
