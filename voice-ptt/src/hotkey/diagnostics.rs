//! Turning a hotkey string into virtual-key codes — and reporting when that
//! did not work.
//!
//! The behaviour here is not new: an unparseable hotkey, or one whose key has
//! no virtual-key equivalent, has always fallen back to the built-in default
//! rather than disabling push-to-talk. That fallback is deliberate and stays.
//!
//! What was missing is the *report*. Both failure paths ended in a
//! `tracing::warn!` and a return value indistinguishable from success, so a
//! `config.toml` with a typo in it produced a working app on the wrong key and
//! no way to find out why. This module makes the failure a value:
//! [`Resolution::problem`]. Callers that want to warn still do; callers that
//! want to *show* the user now can, which is the root of the planned
//! `--doctor` report.
//!
//! Two distinct failures are kept apart on purpose, because they are
//! distinguishable by the user:
//!
//! * [`HotkeyProblem::Unparseable`] — the string is not a chord at all
//!   (`"Ctrl++Alt"`, an empty setting).
//! * [`HotkeyProblem::NoVirtualKey`] — the string is a *valid chord* naming a
//!   key Windows cannot poll for, e.g. `Shift+ف`: `char_to_vk` returns `None`
//!   for every non-ASCII character, so a Persian letter parses fine and then
//!   silently becomes CapsLock. This is the confusing one, and the one a user
//!   is least likely to connect to a settings mistake.

use crate::hotkey::binding::{key_vk_code, modifier_vk_code, HotkeyBinding, HotkeyParseError};

/// Which of the three configurable hotkeys a resolution came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyRole {
    Record,
    ToggleOverlay,
    Quit,
}

impl HotkeyRole {
    /// The name used in log fields and user-facing messages.
    pub fn label(self) -> &'static str {
        match self {
            HotkeyRole::Record => "record",
            HotkeyRole::ToggleOverlay => "toggle overlay",
            HotkeyRole::Quit => "quit",
        }
    }
}

/// Why a hotkey from settings could not be used as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyProblem {
    /// The string is not a parseable chord.
    Unparseable {
        role: HotkeyRole,
        spec: String,
        error: HotkeyParseError,
    },
    /// The chord parsed, but its main key has no virtual-key equivalent, so
    /// `GetAsyncKeyState` cannot observe it.
    NoVirtualKey { role: HotkeyRole, spec: String },
}

impl HotkeyProblem {
    /// One line a user can act on, with the substitution spelled out.
    pub fn message(&self) -> String {
        match self {
            HotkeyProblem::Unparseable { role, spec, .. } => {
                format!(
                    "{} hotkey '{spec}' could not be read — using CapsLock",
                    role.label()
                )
            }
            HotkeyProblem::NoVirtualKey { role, spec } => format!(
                "{} hotkey '{spec}' names a key Windows cannot watch — using CapsLock",
                role.label()
            ),
        }
    }
}

/// One binding resolved to virtual-key codes: `vk` is the main key, `mods` are
/// the modifiers that must accompany it (empty = bare key).
#[derive(Debug, Clone)]
pub struct ResolvedBinding {
    pub(crate) vk: u16,
    pub(crate) mods: Vec<u16>,
}

impl PartialEq for ResolvedBinding {
    fn eq(&self, other: &Self) -> bool {
        self.vk == other.vk && self.mods == other.mods
    }
}

/// A usable binding, plus whatever had to be substituted to get one.
#[derive(Debug, Clone)]
pub struct Resolution {
    /// Always usable. Falls back to the built-in default when needed.
    pub binding: ResolvedBinding,
    /// `None` means the settings were used exactly as written.
    pub problem: Option<HotkeyProblem>,
}

impl Resolution {
    /// Whether the user's own hotkey survived.
    pub fn is_exact(&self) -> bool {
        self.problem.is_none()
    }
}

/// Resolves a binding string, falling back to the built-in default if it
/// cannot be used.
///
/// Falling back rather than disabling the key is a product decision, not a
/// shortcut: a typo in one line of `config.toml` must not leave the user with
/// no push-to-talk at all. The cost was that the substitution was invisible, so
/// it is reported here as well as logged.
pub fn resolve_or_default(spec: &str, role: HotkeyRole) -> Resolution {
    // The built-in default is known-good; failing to resolve it would be a typo
    // in this file rather than a user mistake, so it may panic.
    fn default_resolved() -> ResolvedBinding {
        let binding = HotkeyBinding::parse("CapsLock").expect("default hotkey must parse");
        resolve_binding(&binding).expect("default hotkey must resolve")
    }

    // One parse answering two questions: is the user's chord usable, and if not,
    // why not. Deciding first and resolving afterwards (the obvious shape) is a
    // latent panic: any future branch that set no problem but also produced no
    // binding would reach an `expect` on the user's own settings and abort
    // startup. Here the binding and its problem are produced together.
    let (binding, problem) = match HotkeyBinding::parse(spec) {
        Ok(parsed) => match resolve_binding(&parsed) {
            Some(binding) => (binding, None),
            None => (
                default_resolved(),
                Some(HotkeyProblem::NoVirtualKey {
                    role,
                    spec: spec.to_owned(),
                }),
            ),
        },
        Err(error) => (
            default_resolved(),
            Some(HotkeyProblem::Unparseable {
                role,
                spec: spec.to_owned(),
                error,
            }),
        ),
    };

    if let Some(problem) = &problem {
        tracing::warn!(
            spec,
            what = role.label(),
            problem = ?problem,
            "hotkey unusable; using default"
        );
    }

    Resolution { binding, problem }
}

/// Maps a parsed binding to VK codes. Returns `None` if the main key has no
/// VK equivalent.
fn resolve_binding(b: &HotkeyBinding) -> Option<ResolvedBinding> {
    Some(ResolvedBinding {
        vk: key_vk_code(b.key)?,
        mods: b.modifiers.iter().map(|m| modifier_vk_code(*m)).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::binding::{Key, Modifier};

    /// Every hotkey the parser accepts must survive resolution, or the app
    /// silently runs on the wrong key. This is the invariant that no single
    /// existing test covered: `config_falls_back_on_parse_error` only checked
    /// that a *bad* string still produced a working binding.
    #[test]
    fn every_parsable_ascii_hotkey_survives_resolution() {
        for spec in [
            "CapsLock",
            "F5",
            "F1",
            "F24",
            "Shift+F5",
            "Ctrl+Alt+P",
            "Ctrl+Shift+Q",
            "A",
            "Z",
            "0",
            "9",
            "Ctrl+Alt+Delete",
            "Shift+Space",
        ] {
            let r = resolve_or_default(spec, HotkeyRole::Record);
            assert!(
                r.is_exact(),
                "{spec:?} parsed but did not resolve: {:?}",
                r.problem
            );
        }
    }

    #[test]
    fn a_missing_modifier_is_not_a_problem() {
        let r = resolve_or_default("F5", HotkeyRole::Record);
        assert!(r.is_exact());
        assert!(r.binding.mods.is_empty(), "F5 is a bare key");
    }

    #[test]
    fn modifiers_are_carried_through() {
        let r = resolve_or_default("Shift+F5", HotkeyRole::Record);
        assert!(r.is_exact());
        assert_eq!(r.binding.mods, vec![modifier_vk_code(Modifier::Shift)]);
    }

    /// The invisible failure this module exists for: a Persian letter is a
    /// perfectly good "one character" to the parser, and no VK to Windows.
    #[test]
    fn a_persian_letter_parses_but_cannot_be_watched() {
        let parsed = HotkeyBinding::parse("Shift+ف");
        assert!(
            parsed.is_ok(),
            "the parser accepts it, which is exactly why the failure was silent"
        );
        let r = resolve_or_default("Shift+ف", HotkeyRole::Record);
        let problem = r.problem.expect("a Persian key has no VK equivalent");
        assert_eq!(
            problem,
            HotkeyProblem::NoVirtualKey {
                role: HotkeyRole::Record,
                spec: "Shift+ف".into()
            }
        );
        assert!(
            problem.message().contains("CapsLock"),
            "{}",
            problem.message()
        );
    }

    /// Even though the key is unusable, PTT must still work.
    #[test]
    fn an_unusable_key_still_yields_a_working_binding() {
        let r = resolve_or_default("Shift+ف", HotkeyRole::Record);
        assert_eq!(r.binding.vk, key_vk_code(Key::CapsLock).unwrap());
    }

    #[test]
    fn an_unparseable_string_is_reported_as_such() {
        let r = resolve_or_default("Ctrl++Alt", HotkeyRole::Quit);
        match r.problem {
            Some(HotkeyProblem::Unparseable { role, spec, .. }) => {
                assert_eq!(role, HotkeyRole::Quit);
                assert_eq!(spec, "Ctrl++Alt");
            }
            other => panic!("expected Unparseable, got {other:?}"),
        }
    }

    /// The two failure kinds must not collapse into one: only
    /// `NoVirtualKey` can be fixed by binding a different key, while
    /// `Unparseable` can be a typo in an otherwise-correct key.
    #[test]
    fn the_two_failure_kinds_stay_distinguishable() {
        let unparseable = resolve_or_default("", HotkeyRole::ToggleOverlay);
        let no_vk = resolve_or_default("Shift+ف", HotkeyRole::ToggleOverlay);
        assert!(matches!(
            unparseable.problem,
            Some(HotkeyProblem::Unparseable { .. })
        ));
        assert!(matches!(
            no_vk.problem,
            Some(HotkeyProblem::NoVirtualKey { .. })
        ));
        assert_ne!(unparseable.problem, no_vk.problem);
    }

    /// A usable settings file must report nothing, or every startup warns and
    /// the warning stops meaning anything.
    #[test]
    fn good_settings_report_no_problem() {
        for role in [
            HotkeyRole::Record,
            HotkeyRole::ToggleOverlay,
            HotkeyRole::Quit,
        ] {
            let r = resolve_or_default("Ctrl+Alt+P", role);
            assert!(r.is_exact(), "{role:?} wrongly reported {:?}", r.problem);
        }
    }

    /// The role is what tells a user *which* setting to fix; three identical
    /// messages would be useless.
    #[test]
    fn each_role_names_itself() {
        assert_eq!(HotkeyRole::Record.label(), "record");
        assert_eq!(HotkeyRole::ToggleOverlay.label(), "toggle overlay");
        assert_eq!(HotkeyRole::Quit.label(), "quit");
        for role in [
            HotkeyRole::Record,
            HotkeyRole::ToggleOverlay,
            HotkeyRole::Quit,
        ] {
            let msg = HotkeyProblem::NoVirtualKey {
                role,
                spec: "X".into(),
            }
            .message();
            assert!(msg.contains(role.label()), "{msg} must name the setting");
            assert!(msg.contains("CapsLock"), "{msg} must say what happened");
        }
    }
}
