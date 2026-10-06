//! Per-application profiles: the rules a dictation runs under, chosen by the
//! application it is aimed at.
//!
//! The problem: one set of text rules cannot be right for every window. A
//! terminal wants the recogniser's words verbatim; a chat window wants the
//! dictionary to fix `پاتون` to `پایتون`; a code review wants to read the text
//! before it is typed. Today the answer is a trip to Settings and back, every
//! time the user changes window.
//!
//! # Where the application identity comes from
//!
//! From [`crate::output::target::TargetIdentity::exe_path`] — the executable
//! behind the window, read from the process when the dictation started. **Not**
//! the window title: titles repeat, change as the user types, and a title like
//! `config.rs - project - Visual Studio Code` would bind a profile to a file
//! name rather than to an application. `title_at_capture` is deliberately not
//! consulted here, and the tests below pin that down.
//!
//! # What is deliberately not here
//!
//! * **Its own target capture.** The destination is already captured once per
//!   session by [`crate::output::target`]. A second resolver would be a second
//!   answer to "which window", and the two would disagree exactly when it
//!   mattered.
//! * **Browser-site detection.** Out of scope for this version: the first
//!   version binds to an executable by hand, and a URL is not an executable.
//! * **Choosing an engine.** A profile can change the *text rules*, not which
//!   cloud the audio goes to. Switching a cloud engine on because a window
//!   matched a rule would send audio off the machine without the user asking,
//!   which is the one thing the roadmap forbids outright.
//!
//! # The shape of the decisions
//!
//! Everything here is a value in and a value out, like
//! [`crate::gui::tray_warning`] and [`crate::gui::tray_balloon`]: the panel
//! builds a [`ProfileSet`], the coordinator asks for [`EffectiveRules`] once
//! per session, and nothing in this file touches a window, a lock or the disk.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::processing::dictionary::Correction;
use crate::processing::TextMode;

/// What one application's profile changes, relative to the general settings.
///
/// Every field is `Option`, and that is the whole merge rule in one type: an
/// override that is absent falls back to the general value, and a profile that
/// says nothing gets exactly the behaviour a user without any profiles has.
/// A concrete default here would be a second, silent source of truth for a
/// value the general settings already own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overrides {
    /// `raw`, `conservative` or `standard`. **A string, not the enum**, for the
    /// same reason [`crate::config::settings::TextSettings::mode`] is: this is a
    /// user-facing value in a hand-editable file, and a typo must degrade to the
    /// general mode rather than fail the whole config's load. The degradation
    /// happens in [`effective`], where it is visible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_mode: Option<String>,
    /// Show the text before typing it, for this application only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_before_insert: Option<bool>,
    /// Extra dictionary rules that apply only in this application.
    ///
    /// Additive, never subtractive: a profile can teach a term, and cannot
    /// un-teach one. Removing a general rule from one application is what
    /// `D1`'s general/profile precedence is for, and inventing it here would
    /// give the same concept two spellings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub corrections: Vec<Correction>,
}

impl Overrides {
    /// Whether this override set changes nothing at all.
    ///
    /// An empty profile is not an error, but it is worth knowing about: it
    /// means the entry is inert, and a panel can say so instead of implying the
    /// user configured something.
    pub fn is_empty(&self) -> bool {
        self.text_mode.is_none() && self.review_before_insert.is_none() && self.corrections.is_empty()
    }
}

/// One application's profile.
///
/// Every field defaults to the empty/no-override value, and that *is* the
/// useful default: a half-written profile in the file must be inert rather
/// than capture an application (an empty binding matches nothing) or change a
/// rule (no override falls back to the general value).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppProfile {
    /// Shown in the panel and the log. Never used for matching: a display name
    /// the user renames must not silently unbind the profile.
    pub name: String,
    /// The executable this profile is bound to.
    ///
    /// Either a bare file name (`code.exe`) or a full path
    /// (`C:\Program Files\Code\Code.exe`). A bare name matches that executable
    /// wherever it runs from; a full path narrows it to one installation. See
    /// [`exe_matches`].
    pub exe: String,
    #[serde(flatten)]
    pub overrides: Overrides,
}

impl AppProfile {
    pub fn new(name: impl Into<String>, exe: impl Into<String>, overrides: Overrides) -> Self {
        Self {
            name: name.into(),
            exe: exe.into(),
            overrides,
        }
    }

    /// Whether this profile is bound to `exe_path`.
    pub fn matches(&self, exe_path: &Path) -> bool {
        exe_matches(&self.exe, exe_path)
    }
}

/// The profiles the user has defined.
///
/// Order is the user's order and is preserved, because the panel shows it. It
/// is **not** precedence: [`ProfileSet::resolve`] refuses ambiguity rather than
/// letting the first entry win, since a silent precedence rule is how "I set it
/// for Chrome and it used my Terminal rules" gets shipped.
///
/// # Why `Deserialize` is written by hand
///
/// The natural TOML for a list is `[[profiles]]` (an array of tables), which is
/// what this app writes and what serialisation produces here. But the *table*
/// spelling — `[profiles]` with nothing under it — is what someone writes by
/// hand to see what the section looks like, and under a plain
/// `#[serde(transparent)]` it fails with `invalid type: map, expected a
/// sequence`. That failure is not confined to the section: it aborts the load
/// of the whole `config.toml`, so a user exploring the format loses every
/// setting they have.
///
/// The same reasoning already governs [`crate::config::settings::TextSettings::mode`],
/// which is a `String` rather than an enum so that a typo cannot cost the file.
/// An empty table therefore means "no profiles", and a **non-empty** table is
/// still an error — that is a real mistake (entries written without the second
/// pair of brackets), and silently dropping them would be worse than saying so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ProfileSet {
    profiles: Vec<AppProfile>,
}

impl<'de> Deserialize<'de> for ProfileSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SetVisitor;

        impl<'de> serde::de::Visitor<'de> for SetVisitor {
            type Value = ProfileSet;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a list of profiles, written as [[profiles]]")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<ProfileSet, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut out = Vec::new();
                while let Some(profile) = seq.next_element::<AppProfile>()? {
                    out.push(profile);
                }
                Ok(ProfileSet::new(out))
            }

            fn visit_map<A>(self, mut map: A) -> Result<ProfileSet, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                use serde::de::Error as _;
                // Consume the first key to find out whether the table actually
                // holds anything. An empty table is tolerated; a populated one
                // is the `[profile]`-instead-of`[[profile]]` mistake and gets
                // named as such.
                match map.next_key::<String>()? {
                    None => Ok(ProfileSet::default()),
                    Some(first) => Err(A::Error::custom(format!(
                        "`[profiles]` is a table holding `{first}`, but profiles are a list. \
                         Write each profile as a `[[profiles]]` entry instead."
                    ))),
                }
            }
        }

        // `deserialize_any` so one type can accept both spellings; every format
        // this config is read from (TOML) is self-describing.
        deserializer.deserialize_any(SetVisitor)
    }
}

impl ProfileSet {
    pub fn new(profiles: Vec<AppProfile>) -> Self {
        Self { profiles }
    }

    pub fn profiles(&self) -> &[AppProfile] {
        &self.profiles
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn push(&mut self, profile: AppProfile) {
        self.profiles.push(profile);
    }

    /// Removes the profile at `index`, returning it.
    pub fn remove(&mut self, index: usize) -> Option<AppProfile> {
        (index < self.profiles.len()).then(|| self.profiles.remove(index))
    }

    /// The entry at `index`, mutably.
    ///
    /// For the panels, which edit one rule inside one profile — the binding is
    /// what identifies an entry in the file, but it is also what an editor is
    /// allowed to change, so an editor holds an index for as long as it is open.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut AppProfile> {
        self.profiles.get_mut(index)
    }

    /// Adds or replaces the profile bound to the same executable.
    ///
    /// Keyed on the **binding**, not the display name: two entries pointing at
    /// `code.exe` would be an ambiguity with no answer, and the panel says the
    /// same thing when it saves an edit.
    pub fn upsert(&mut self, profile: AppProfile) {
        let key = binding_key(&profile.exe);
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|p| binding_key(&p.exe) == key)
        {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
    }

    /// The profile for this executable, if exactly one is bound to it.
    ///
    /// `None` for an unknown application, and `None` for an executable two
    /// profiles both claim — the second case is a configuration mistake and the
    /// answer to it is the general rules, not the first entry in the file.
    pub fn resolve(&self, exe_path: Option<&Path>) -> Option<&AppProfile> {
        let exe_path = exe_path?;
        let mut matches = self.profiles.iter().filter(|p| p.matches(exe_path));
        let first = matches.next()?;
        if matches.next().is_some() {
            // Ambiguous: refuse rather than pick. The caller logs which
            // executable, so the user can find the duplicate.
            return None;
        }
        Some(first)
    }
}

/// The general values a profile merges onto, resolved once per session.
///
/// Passed in rather than read from [`crate::config::settings::Settings`] so
/// this module has no opinion about where settings live — and so the merge is
/// testable without a config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneralRules {
    pub mode: TextMode,
    pub review_before_insert: bool,
    /// Whether spoken commands are recognised at all.
    ///
    /// General-only on purpose: a profile says how one application's *text* is
    /// shaped, and "is a phrase an instruction" is a statement about the whole
    /// app — letting a profile turn commands on would mean the same sentence
    /// became a line break in one window and words in the next, with nothing
    /// on screen to say why.
    pub commands: bool,
}

impl GeneralRules {
    pub fn new(mode: TextMode, review_before_insert: bool, commands: bool) -> Self {
        Self {
            mode,
            review_before_insert,
            commands,
        }
    }
}

impl From<&crate::config::settings::Settings> for GeneralRules {
    fn from(settings: &crate::config::settings::Settings) -> Self {
        Self {
            mode: settings.text.mode(),
            review_before_insert: settings.gui.review_before_insert,
            commands: settings.text.commands,
        }
    }
}

/// The rules one dictation actually runs under.
///
/// **A value, not a lookup.** The coordinator resolves this once, when the
/// recording starts, and carries it with the work — so a later chunk of the
/// same session cannot be governed by a different profile just because the user
/// switched windows while it was being transcribed. Same reason
/// [`crate::output::target::TargetIdentity`] is carried rather than re-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRules {
    /// The text pipeline's mode, after the override or the general fallback.
    pub mode: TextMode,
    /// Whether to hold the text for review, after the override or the fallback.
    pub review_before_insert: bool,
    /// Whether spoken commands are recognised — always the general setting,
    /// never overridden. See [`GeneralRules::commands`].
    pub commands: bool,
    /// Extra dictionary rules from the matched profile, if any.
    pub corrections: Vec<Correction>,
    /// The name of the profile that matched, or `None` for the general rules.
    ///
    /// Kept as the *name* and not a reference: the log line and the panel need
    /// to say which application's rules applied, and holding the set alive to
    /// answer that would tie a dictation's lifetime to the settings object.
    pub profile: Option<String>,
}

impl EffectiveRules {
    /// Whether these rules came from a profile rather than the general settings.
    pub fn is_profiled(&self) -> bool {
        self.profile.is_some()
    }
}

/// Merges the matching profile's overrides onto the general rules.
///
/// The single entry point every caller uses, so the four cases the roadmap
/// names are all decided in one place: no profiles at all, an application with
/// no profile, a profile that overrides nothing, and a profile that overrides
/// some fields and not others.
pub fn effective(
    set: &ProfileSet,
    exe_path: Option<&Path>,
    general: GeneralRules,
) -> EffectiveRules {
    let Some(profile) = set.resolve(exe_path) else {
        return EffectiveRules {
            mode: general.mode,
            review_before_insert: general.review_before_insert,
            commands: general.commands,
            corrections: Vec::new(),
            profile: None,
        };
    };

    EffectiveRules {
        mode: mode_for(&profile.overrides, general.mode),
        review_before_insert: profile
            .overrides
            .review_before_insert
            .unwrap_or(general.review_before_insert),
        commands: general.commands,
        corrections: profile.overrides.corrections.clone(),
        profile: Some(profile_label(profile)),
    }
}

/// The mode one profile runs under, given the general mode.
///
/// Extracted because two callers need the same answer: the resolver above, and
/// the dictionary panel's preview, which shows the user what a rule would do in
/// one application without resolving a destination at all. Two spellings of
/// "what does an unset override mean" would eventually disagree.
///
/// An unrecognised mode string falls back to the **general** mode rather than to
/// `Standard`: a typo in one profile should cost that profile its override and
/// nothing more, and `Standard` would be a behaviour change the user never asked
/// for on top of the one they mistyped.
pub fn mode_for(overrides: &Overrides, general: crate::processing::TextMode) -> crate::processing::TextMode {
    overrides
        .text_mode
        .as_deref()
        .map(parse_mode_or_general)
        .map(|m| m.unwrap_or(general))
        .unwrap_or(general)
}

/// The name to show for a profile, falling back to the executable.
///
/// A profile the user never named still has to be identifiable in the log; the
/// binding is what they chose, so it is the honest fallback.
fn profile_label(profile: &AppProfile) -> String {
    let name = profile.name.trim();
    if name.is_empty() {
        profile.exe.trim().to_string()
    } else {
        name.to_string()
    }
}

/// Parses a mode string, or `None` when it is not one of the three modes.
///
/// Split from [`effective`] so "the string was a typo" and "the string was
/// absent" stay distinguishable: both end at the general mode, but only one of
/// them is worth a warning.
fn parse_mode_or_general(value: &str) -> Option<TextMode> {
    let trimmed = value.trim();
    [
        TextMode::Raw,
        TextMode::Conservative,
        TextMode::Standard,
    ]
    .into_iter()
    .find(|m| m.as_str().eq_ignore_ascii_case(trimmed))
}

/// The normalised comparison key for a binding.
///
/// Lowercased because Windows paths and executable names are case-insensitive,
/// and trimmed because a binding pasted from a file manager often trails a
/// space. Without this, `Code.exe` and `code.exe` would be two profiles for one
/// application — and `resolve` would then call the pair ambiguous and fall back
/// to the general rules, which looks exactly like the feature not working.
pub fn binding_key(binding: &str) -> String {
    binding.trim().to_ascii_lowercase()
}

/// Whether `binding` refers to `exe_path`.
///
/// Two spellings, both case-insensitive:
///
/// * a bare file name — `code.exe` — matches that executable wherever it runs
///   from, which is what a user means when they pick an app;
/// * a path containing a separator — `C:\apps\code.exe` — matches only that
///   installation, for the case where two copies of an executable do different
///   work.
///
/// A trailing `\` or `/` is ignored, and the comparison is on the whole path,
/// so `C:\apps\code.exe` does not match `C:\apps\old\code.exe`.
pub fn exe_matches(binding: &str, exe_path: &Path) -> bool {
    let binding = binding.trim();
    if binding.is_empty() {
        // An empty binding must not match everything: a half-finished profile
        // in the file would otherwise capture every application.
        return false;
    }
    let binding_norm = binding.replace('/', "\\").to_ascii_lowercase();
    let path_norm = exe_path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let path_norm = path_norm.trim_end_matches('\\');

    if binding_norm.contains('\\') {
        // A full-path binding: compare the whole thing, trimmed of any trailing
        // separator on the binding side too.
        binding_norm.trim_end_matches('\\') == path_norm
    } else {
        // A bare name: compare the file name only.
        match exe_path.file_name() {
            Some(name) => name.to_string_lossy().to_ascii_lowercase() == binding_norm,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn exe(p: &str) -> PathBuf {
        PathBuf::from(p)
    }

    fn profile(name: &str, exe: &str, overrides: Overrides) -> AppProfile {
        AppProfile::new(name, exe, overrides)
    }

    fn raw_mode() -> Overrides {
        Overrides {
            text_mode: Some("raw".into()),
            ..Default::default()
        }
    }

    fn general() -> GeneralRules {
        GeneralRules::new(TextMode::Standard, false, false)
    }

    // ── criterion 1: general fallback ──────────────────────────────────────

    /// The one that matters most: a user with no profiles must see no change at
    /// all. Anything else is this feature silently rewriting everyone's output.
    #[test]
    fn an_empty_set_of_profiles_changes_nothing() {
        let set = ProfileSet::default();
        let rules = effective(&set, Some(&exe("C:/x/code.exe")), general());
        assert_eq!(rules.mode, TextMode::Standard);
        assert!(!rules.review_before_insert);
        assert!(!rules.is_profiled());
        assert!(rules.corrections.is_empty());
    }

    /// …and the general values that come back are the ones that went in, not a
    /// hardcoded default wearing their name.
    #[test]
    fn the_general_values_are_passed_through_verbatim() {
        let set = ProfileSet::default();
        let g = GeneralRules::new(TextMode::Conservative, true, false);
        let rules = effective(&set, Some(&exe("C:/x/other.exe")), g);
        assert_eq!(rules.mode, TextMode::Conservative);
        assert!(rules.review_before_insert);
    }

    /// An application with no profile gets the general rules — the common case
    /// for most windows on the machine.
    #[test]
    fn an_application_without_a_profile_gets_the_general_rules() {
        let set = ProfileSet::new(vec![profile("Terminal", "wt.exe", raw_mode())]);
        let rules = effective(&set, Some(&exe("C:/x/chrome.exe")), general());
        assert_eq!(rules.mode, TextMode::Standard);
        assert!(!rules.is_profiled());
    }

    // ── criterion 2: override removal ─────────────────────────────────────

    /// A profile that overrides nothing is a profile that changes nothing. The
    /// entry still matches, so `is_profiled` is true, but every value must be
    /// the general one.
    #[test]
    fn a_profile_with_no_overrides_behaves_exactly_like_no_profile() {
        let set = ProfileSet::new(vec![profile("Empty", "code.exe", Overrides::default())]);
        let rules = effective(&set, Some(&exe("C:/x/code.exe")), general());
        assert_eq!(rules.mode, TextMode::Standard);
        assert!(!rules.review_before_insert);
        assert!(rules.corrections.is_empty());
        assert!(
            rules.is_profiled(),
            "the entry did match, which the panel should be able to say"
        );
    }

    /// Partial overrides merge field by field. This is the "override merges
    /// with the general value" rule, and the bug it prevents is a profile that
    /// sets only a mode silently resetting everything else.
    #[test]
    fn an_override_merges_field_by_field() {
        let set = ProfileSet::new(vec![profile(
            "Terminal",
            "wt.exe",
            Overrides {
                text_mode: Some("raw".into()),
                // review deliberately left unset
                ..Default::default()
            },
        )]);

        let g = GeneralRules::new(TextMode::Standard, true, false);
        let rules = effective(&set, Some(&exe("C:/x/wt.exe")), g);
        assert_eq!(rules.mode, TextMode::Raw, "the override must apply");
        assert!(
            rules.review_before_insert,
            "an unset field must fall back to the general value, not to false"
        );

        // …and the other way round: only the review flag overridden.
        let set = ProfileSet::new(vec![profile(
            "Chat",
            "slack.exe",
            Overrides {
                review_before_insert: Some(false),
                ..Default::default()
            },
        )]);
        let g = GeneralRules::new(TextMode::Standard, true, false);
        let rules = effective(&set, Some(&exe("C:/x/slack.exe")), g);
        assert_eq!(rules.mode, TextMode::Standard, "mode must stay general");
        assert!(
            !rules.review_before_insert,
            "an explicit false is an override, not an absence"
        );
    }

    /// Deleting the entry returns the application to the general rules — with
    /// no state left behind from the time it had one.
    #[test]
    fn removing_a_profile_restores_the_general_rules() {
        let mut set = ProfileSet::new(vec![
            profile("Terminal", "wt.exe", raw_mode()),
            profile("Chat", "slack.exe", Overrides::default()),
        ]);
        let path = exe("C:/x/wt.exe");
        assert_eq!(effective(&set, Some(&path), general()).mode, TextMode::Raw);

        set.remove(0);
        let rules = effective(&set, Some(&path), general());
        assert_eq!(rules.mode, TextMode::Standard);
        assert!(!rules.is_profiled());

        // And removing the last one is not a special case.
        set.remove(0);
        assert!(set.is_empty());
        assert_eq!(effective(&set, Some(&path), general()).mode, TextMode::Standard);
    }

    /// A mode string the user mistyped must fall back to the **general** mode,
    /// not to `Standard`. Otherwise a typo in a profile that was meant to say
    /// `conservative` would silently switch a user's whole pipeline — an
    /// unrelated change riding in on a spelling mistake.
    #[test]
    fn an_unrecognised_mode_falls_back_to_the_general_mode() {
        let set = ProfileSet::new(vec![profile(
            "Typo",
            "code.exe",
            Overrides {
                text_mode: Some("standart".into()),
                ..Default::default()
            },
        )]);
        let g = GeneralRules::new(TextMode::Conservative, false, false);
        let rules = effective(&set, Some(&exe("C:/x/code.exe")), g);
        assert_eq!(
            rules.mode,
            TextMode::Conservative,
            "a typo must cost the profile its override, not change the pipeline"
        );
    }

    /// The three real mode strings must be accepted, whatever their casing and
    /// surrounding space, because they are typed by hand into a TOML file.
    #[test]
    fn every_mode_string_is_accepted_in_any_case() {
        for (written, expected) in [
            ("raw", TextMode::Raw),
            ("RAW", TextMode::Raw),
            (" conservative ", TextMode::Conservative),
            ("Standard", TextMode::Standard),
        ] {
            let set = ProfileSet::new(vec![profile(
                "P",
                "app.exe",
                Overrides {
                    text_mode: Some(written.into()),
                    ..Default::default()
                },
            )]);
            let rules = effective(&set, Some(&exe("C:/x/app.exe")), general());
            assert_eq!(rules.mode, expected, "config said {written:?}");
        }
    }

    /// The dictionary panel's preview asks [`mode_for`] directly — "what mode
    /// does this profile run under?" — without resolving a destination at all,
    /// so it has to give the same answer the coordinator gets from [`effective`].
    /// Two spellings of "what does an unset override mean" would eventually
    /// disagree, and the panel would then preview a rule against a pipeline the
    /// application is not running.
    #[test]
    fn mode_for_agrees_with_the_resolver() {
        let set = ProfileSet::new(vec![profile("Terminal", "wt.exe", raw_mode())]);
        for general_mode in [TextMode::Raw, TextMode::Conservative, TextMode::Standard] {
            let g = GeneralRules::new(general_mode, false, false);
            let via_resolver = effective(&set, Some(&exe("C:/x/wt.exe")), g).mode;
            assert_eq!(
                via_resolver,
                mode_for(&set.profiles()[0].overrides, general_mode),
                "general mode {general_mode:?}"
            );

            // A profile that says nothing gets the general mode from both.
            assert_eq!(mode_for(&Overrides::default(), general_mode), general_mode);
        }
    }

    // ── criterion 3: unknown identity ─────────────────────────────────────

    /// A destination whose process could not be read (`exe_path: None`) has an
    /// unknown identity, and unknown means general. Guessing from the title is
    /// exactly what this must not do.
    #[test]
    fn an_unknown_executable_falls_back_to_the_general_rules() {
        let set = ProfileSet::new(vec![profile("Terminal", "wt.exe", raw_mode())]);
        let rules = effective(&set, None, general());
        assert_eq!(rules.mode, TextMode::Standard);
        assert!(!rules.is_profiled());
    }

    /// A binding saved with the wrong case or a stray space must still match.
    /// If it did not, the user's profile would look broken for a reason no
    /// panel could explain.
    #[test]
    fn bindings_match_case_insensitively_and_ignore_stray_space() {
        for binding in ["Code.exe", "CODE.EXE", " code.exe ", "code.EXE"] {
            let set = ProfileSet::new(vec![profile("VSCode", binding, raw_mode())]);
            let rules = effective(&set, Some(&exe("C:/Program Files/Code/Code.exe")), general());
            assert_eq!(rules.mode, TextMode::Raw, "binding {binding:?} did not match");
        }
    }

    /// An empty binding must not capture every application. A half-written
    /// profile in the file would otherwise take over the whole machine.
    #[test]
    fn an_empty_binding_matches_nothing() {
        let set = ProfileSet::new(vec![profile("Half-written", "", raw_mode())]);
        for path in ["C:/x/code.exe", "C:/x/wt.exe"] {
            let rules = effective(&set, Some(&exe(path)), general());
            assert_eq!(rules.mode, TextMode::Standard, "empty binding took {path}");
            assert!(!rules.is_profiled());
        }
    }

    /// A bare name matches the executable wherever it runs from — that is what
    /// a user means when they pick an application.
    #[test]
    fn a_bare_name_matches_any_installation() {
        let set = ProfileSet::new(vec![profile("VSCode", "code.exe", raw_mode())]);
        for path in [
            "C:/Program Files/Microsoft VS Code/Code.exe",
            "D:/portable/VSCode/code.exe",
        ] {
            assert_eq!(
                effective(&set, Some(&exe(path)), general()).mode,
                TextMode::Raw,
                "bare name did not match {path}"
            );
        }
    }

    /// A full path narrows the match to one installation — and must not match a
    /// *different* installation whose path merely ends the same way.
    #[test]
    fn a_full_path_binding_narrows_to_one_installation() {
        let set = ProfileSet::new(vec![profile(
            "Work copy",
            "C:\\apps\\code.exe",
            raw_mode(),
        )]);
        assert_eq!(
            effective(&set, Some(&exe("C:\\apps\\code.exe")), general()).mode,
            TextMode::Raw
        );
        // Same file name, different directory: not this profile.
        assert_eq!(
            effective(&set, Some(&exe("C:\\apps\\old\\code.exe")), general()).mode,
            TextMode::Standard,
            "a full-path binding leaked onto another installation"
        );
    }

    /// Two profiles claiming one executable is a configuration mistake, and the
    /// answer must be the general rules — not whichever entry happened to be
    /// first. A silent precedence rule is how "I set it for Chrome and got my
    /// Terminal rules" ships.
    #[test]
    fn an_ambiguous_binding_refuses_rather_than_choosing() {
        let set = ProfileSet::new(vec![
            profile("One", "code.exe", raw_mode()),
            profile("Two", "CODE.EXE", Overrides::default()),
        ]);
        let rules = effective(&set, Some(&exe("C:/x/code.exe")), general());
        assert_eq!(
            rules.mode,
            TextMode::Standard,
            "an ambiguous binding must not silently pick an entry"
        );
        assert!(!rules.is_profiled());
    }

    // ── criterion 4: session stability ────────────────────────────────────

    /// The rules are a value. Resolving them once and re-using that value is
    /// what makes a chunk governable by the profile of the window the dictation
    /// *started* in — so this test states the property the coordinator relies
    /// on: two resolutions of the same facts agree, and a later change to the
    /// set is not visible in a value already returned.
    #[test]
    fn rules_already_resolved_do_not_change_when_the_set_does() {
        let mut set = ProfileSet::new(vec![profile("Terminal", "wt.exe", raw_mode())]);
        let path = exe("C:/x/wt.exe");
        let at_start = effective(&set, Some(&path), general());
        assert_eq!(at_start.mode, TextMode::Raw);

        // The user edits their profiles mid-dictation: the profile is deleted.
        set.remove(0);

        assert_eq!(
            at_start.mode,
            TextMode::Raw,
            "a value already handed out must not follow the set"
        );
        assert_eq!(
            effective(&set, Some(&path), general()).mode,
            TextMode::Standard,
            "…but a fresh lookup must see the deletion"
        );
    }

    /// The rules are cloned onto each piece of work, so a second dictation's
    /// rules cannot reach back into the first one's.
    #[test]
    fn rules_are_independent_values() {
        let set = ProfileSet::new(vec![profile("Terminal", "wt.exe", raw_mode())]);
        let a = effective(&set, Some(&exe("C:/x/wt.exe")), general());
        let b = effective(&set, Some(&exe("C:/x/chrome.exe")), general());
        assert_ne!(a, b);
        assert_eq!(a.mode, TextMode::Raw);
        assert_eq!(b.mode, TextMode::Standard);
    }

    // ── identity comes from the executable, not the title ─────────────────

    /// The specification is explicit that identity comes from valid destination
    /// information and **not** from the window title. There is no title in this
    /// module's inputs at all, and this test records why: a title changes as the
    /// user types, so binding to one would make a profile stop matching halfway
    /// through the document it was configured for.
    #[test]
    fn the_window_title_is_not_an_input_to_resolution() {
        // Same executable, two titles a VS Code window would produce. Both
        // resolve to the same profile, because neither title is consulted.
        let set = ProfileSet::new(vec![profile("VSCode", "code.exe", raw_mode())]);
        let path = exe("C:/apps/Code.exe");
        assert_eq!(effective(&set, Some(&path), general()).mode, TextMode::Raw);
        assert_eq!(effective(&set, Some(&path), general()).mode, TextMode::Raw);
        // A title that looks like an executable name must not match anything.
        let set = ProfileSet::new(vec![profile("Trap", "code.exe", raw_mode())]);
        assert!(!set
            .resolve(Some(&exe("C:/apps/other.exe")))
            .map(|p| p.name == "Trap")
            .unwrap_or(false));
    }

    // ── the profile's own dictionary scope ────────────────────────────────

    /// Profile corrections ride with the rules and are additive.
    #[test]
    fn a_profiles_corrections_travel_with_its_rules() {
        let set = ProfileSet::new(vec![profile(
            "Chat",
            "slack.exe",
            Overrides {
                corrections: vec![Correction {
                    from: "پاتون".into(),
                    to: "پایتون".into(),
                    category: Some("profile".into()),
                }],
                ..Default::default()
            },
        )]);
        let rules = effective(&set, Some(&exe("C:/x/slack.exe")), general());
        assert_eq!(rules.corrections.len(), 1);
        assert_eq!(rules.corrections[0].to, "پایتون");

        // A different application gets none of it.
        let other = effective(&set, Some(&exe("C:/x/code.exe")), general());
        assert!(other.corrections.is_empty());
    }

    /// A profile name is a label; the executable is the binding. Renaming must
    /// not unbind, and an unnamed profile still has to be identifiable.
    #[test]
    fn the_display_name_never_decides_the_match() {
        let set = ProfileSet::new(vec![profile("My favourite app", "code.exe", raw_mode())]);
        assert_eq!(
            effective(&set, Some(&exe("C:/x/code.exe")), general()).mode,
            TextMode::Raw
        );

        // No name at all: the binding stands in, so a log line can still say
        // which rules applied.
        let set = ProfileSet::new(vec![profile("", "wt.exe", raw_mode())]);
        let rules = effective(&set, Some(&exe("C:/x/wt.exe")), general());
        assert_eq!(rules.profile.as_deref(), Some("wt.exe"));
    }

    #[test]
    fn upsert_replaces_the_entry_for_the_same_binding() {
        let mut set = ProfileSet::new(vec![profile("Old", "code.exe", raw_mode())]);
        set.upsert(profile(
            "New",
            "CODE.EXE",
            Overrides {
                text_mode: Some("standard".into()),
                ..Default::default()
            },
        ));
        assert_eq!(set.len(), 1, "a second entry for one executable is ambiguous");
        assert_eq!(set.profiles()[0].name, "New");

        // A different binding is a new entry.
        set.upsert(profile("Terminal", "wt.exe", raw_mode()));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn an_empty_override_set_reports_itself_as_empty() {
        assert!(Overrides::default().is_empty());
        assert!(!Overrides {
            review_before_insert: Some(false),
            ..Default::default()
        }
        .is_empty());
        assert!(!raw_mode().is_empty());
    }

    /// The profile set survives the round trip through TOML that the settings
    /// file actually performs — a rename of a field would otherwise be visible
    /// only as "my profile does nothing".
    #[test]
    fn a_profile_set_round_trips_through_toml() {
        #[derive(Serialize, Deserialize, Default)]
        struct Wrapper {
            #[serde(default)]
            profiles: ProfileSet,
        }

        let original = Wrapper {
            profiles: ProfileSet::new(vec![
                profile(
                    "Terminal",
                    "wt.exe",
                    Overrides {
                        text_mode: Some("raw".into()),
                        ..Default::default()
                    },
                ),
                profile(
                    "Chat",
                    "slack.exe",
                    Overrides {
                        review_before_insert: Some(true),
                        corrections: vec![Correction {
                            from: "a".into(),
                            to: "b".into(),
                            category: None,
                        }],
                        ..Default::default()
                    },
                ),
            ]),
        };

        let text = toml::to_string_pretty(&original).expect("serialises");
        let back: Wrapper = toml::from_str(&text).expect("deserialises");

        assert_eq!(back.profiles.len(), 2);
        assert_eq!(back.profiles.profiles()[0].exe, "wt.exe");
        assert_eq!(
            effective(&back.profiles, Some(&exe("C:/x/wt.exe")), general()).mode,
            TextMode::Raw
        );
        assert!(effective(&back.profiles, Some(&exe("C:/x/slack.exe")), general())
            .review_before_insert);
        assert_eq!(
            effective(&back.profiles, Some(&exe("C:/x/slack.exe")), general())
                .corrections
                .len(),
            1
        );
    }

    /// A config written before profiles existed — no `[profiles]` at all — must
    /// load as an empty set rather than fail, or every existing user's file
    /// stops parsing on upgrade.
    #[test]
    fn a_config_without_profiles_loads_as_an_empty_set() {
        let back: Wrapper = toml::from_str("[other]\nkey = 1\n").expect("loads without profiles");
        assert!(back.profiles.is_empty());
    }

    #[derive(Debug, Serialize, Deserialize, Default)]
    struct Wrapper {
        #[serde(default)]
        profiles: ProfileSet,
    }

    /// Someone exploring the format writes `[profiles]` with nothing under it.
    /// That must not cost them the rest of their settings: the failure would be
    /// a `config.toml` that no longer loads at all.
    #[test]
    fn an_empty_profiles_table_loads_as_an_empty_set() {
        let empty: Wrapper = toml::from_str("[profiles]\n").expect("an empty table must load");
        assert!(empty.profiles.is_empty());

        // …and alongside other settings, which are the thing actually at risk.
        #[derive(Serialize, Deserialize, Default)]
        struct WithOther {
            #[serde(default)]
            profiles: ProfileSet,
            #[serde(default)]
            other: std::collections::BTreeMap<String, i64>,
        }
        let both: WithOther =
            toml::from_str("[profiles]\n\n[other]\nkey = 7\n").expect("loads");
        assert!(both.profiles.is_empty());
        assert_eq!(both.other.get("key"), Some(&7));
    }

    /// A *populated* `[profiles]` table is the `[profiles]`-instead-of
    /// `[[profiles]]` mistake. It must be reported, not silently dropped — the
    /// user wrote profiles and needs to be told why they did not take.
    #[test]
    fn a_populated_profiles_table_is_an_error_that_names_the_fix() {
        let err = toml::from_str::<Wrapper>("[profiles]\nname = \"x\"\nexe = \"a.exe\"\n")
            .expect_err("a table of profiles must not be accepted silently");
        let text = err.to_string();
        assert!(
            text.contains("[[profiles]]"),
            "the error must name the spelling that works: {text}"
        );
    }

    /// The array-of-tables spelling — what the app itself writes — is the one
    /// that must round-trip.
    #[test]
    fn the_written_form_loads_back() {
        let original = Wrapper {
            profiles: ProfileSet::new(vec![profile("Terminal", "wt.exe", raw_mode())]),
        };
        let text = toml::to_string_pretty(&original).expect("serialises");
        assert!(
            text.contains("[[profiles]]"),
            "the written form must be the array-of-tables spelling: {text}"
        );
        let back: Wrapper = toml::from_str(&text).expect("round-trips");
        assert_eq!(back.profiles.len(), 1);
        assert_eq!(
            effective(&back.profiles, Some(&exe("C:/x/wt.exe")), general()).mode,
            TextMode::Raw
        );
    }
}
