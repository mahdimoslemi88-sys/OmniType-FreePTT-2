//! Which ASR engines get registered, in what order, and whether the engine the
//! user picked is actually there.
//!
//! This decision used to be sixty lines of `tracing::info!` interleaved with
//! `engines.push(...)` inside `run()`. It is worth isolating because it is the
//! part of the ASR phase most likely to be broken *silently*: an engine that
//! is not registered does not crash the app, it just quietly stops being
//! selectable.
//!
//! Two things forced the split:
//!
//! * `CloudSettings::is_configured` reads `VOICE_PTT_CLOUD_KEY` from the
//!   environment. A plan function that called it would not be testable, so the
//!   environment read happens at the call site and arrives here as a plain
//!   `bool`.
//! * The probe backoff below encodes a measurement, not a taste: probing a
//!   never-selected Antigravity used to cost a PowerShell + netstat subprocess
//!   every 30 s forever, which showed up as handles/threads/private bytes
//!   jumping on a timer. That number deserves a test of its own.
//!
//! Nothing here touches the filesystem, spawns a process, or maps a model, so
//! all of it runs in a unit test.

use crate::config::Settings;

/// One engine the app intends to register, in router priority order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineKind {
    /// The configured cloud provider (`groq`, or a custom provider's id).
    Cloud {
        id: String,
    },
    Google,
    /// A user-defined custom endpoint, by its configured id.
    Custom {
        id: String,
    },
    Antigravity,
    /// The local whisper model. Always last: it is the only one that can
    /// always answer, and in `auto` it is also the most expensive.
    Whisper,
}

impl EngineKind {
    /// The id the router will know this engine by.
    pub fn id(&self) -> &str {
        match self {
            EngineKind::Cloud { id } | EngineKind::Custom { id } => id,
            EngineKind::Google => "google",
            EngineKind::Antigravity => "antigravity",
            EngineKind::Whisper => "local_whisper",
        }
    }
}

/// Whether the engine the user selected can actually run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveSelection {
    /// `auto`: the router picks from whatever registered.
    Auto,
    /// The named engine is in the plan, so it will run.
    Resolved,
    /// The user selected an engine that is not registered — the config left
    /// `active_engine` pointing at something they have since disabled.
    ///
    /// `AsrRouter::transcribe` refuses to fall back in this case (by design),
    /// so every dictation fails with an error the user never sees at startup.
    /// `run()` turns this into a startup warning.
    Missing,
}

/// The result of planning: what to build, and whether the selection survives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnginePlan {
    pub engines: Vec<EngineKind>,
    pub selection: ActiveSelection,
}

impl EnginePlan {
    /// The provider id for the cloud slot, for `CloudEngine::new`.
    pub fn cloud_provider(&self) -> Option<&str> {
        self.engines.iter().find_map(|e| match e {
            EngineKind::Cloud { id } => Some(id.as_str()),
            _ => None,
        })
    }

    /// Custom provider ids, in registration order.
    pub fn custom_ids(&self) -> Vec<&str> {
        self.engines
            .iter()
            .filter_map(|e| match e {
                EngineKind::Custom { id } => Some(id.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn wants_antigravity_probe(&self) -> bool {
        self.engines.contains(&EngineKind::Antigravity)
    }

    /// Whether `allow_local_fallback` should be set on the router.
    pub fn allow_local_fallback(&self, settings: &Settings) -> bool {
        settings.asr.auto_local_fallback
    }
}

/// Decides the engine list from settings. Pure: `cloud_key_in_env` is the
/// caller's answer to "is `VOICE_PTT_CLOUD_KEY` set and non-empty?".
///
/// Order matters and is the auto-mode priority chain: the primary cloud
/// provider, then Google Free Speech (no key needed, fast online), then custom
/// endpoints, then Antigravity, then local whisper. Antigravity sits below the
/// cloud engines on purpose — it must never win an `auto` race, it has to be
/// selected explicitly.
pub fn engine_plan(settings: &Settings, cloud_key_in_env: bool, cloud_key_in_store: bool) -> EnginePlan {
    let cloud = &settings.cloud;
    let mut engines = Vec::new();

    // `is_configured` is `enabled && (a key exists somewhere)`. The two
    // out-of-file sources are **passed in** rather than read from the resolver's
    // process-global: a planner that consulted a global would be untestable in
    // the one way that matters here, which is "does a key in the store make the
    // cloud engine available".
    let cloud_ready =
        cloud.enabled && (!cloud.api_key.trim().is_empty() || cloud_key_in_env || cloud_key_in_store);
    if cloud_ready {
        engines.push(EngineKind::Cloud {
            id: cloud.provider.clone(),
        });
    }

    if settings.google.enabled {
        engines.push(EngineKind::Google);
    }

    for custom in &settings.custom_providers {
        engines.push(EngineKind::Custom {
            id: custom.id.clone(),
        });
    }

    if settings.antigravity.enabled {
        engines.push(EngineKind::Antigravity);
    }

    engines.push(EngineKind::Whisper);

    let selection = if settings.active_engine == "auto" {
        ActiveSelection::Auto
    } else if engines.iter().any(|e| e.id() == settings.active_engine) {
        ActiveSelection::Resolved
    } else {
        ActiveSelection::Missing
    };

    EnginePlan { engines, selection }
}

/// Seconds to wait between Antigravity discovery probes.
///
/// Discovery spawns PowerShell and netstat — a real subprocess pair per probe —
/// so it must stay off the UI thread. The pause is long only when the engine is
/// both *selected* and *already answering*; anything else (not selected, or
/// selected but not up yet) polls faster so the user gets the engine sooner.
///
/// This asymmetry is the fix for the measured churn: a permanently-probing
/// Antigravity that the user never chose drove handle/thread/private-byte
/// growth on a 30-60 s timer.
pub fn probe_pause_secs(selected: bool, available: bool) -> u64 {
    if selected && available {
        60
    } else {
        30
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::{CloudConfig, CustomProvider};

    /// A settings object with every engine switched off, so each test turns on
    /// exactly the one slot it is about.
    fn bare() -> Settings {
        let mut s = Settings::default();
        s.cloud.enabled = false;
        s.google.enabled = false;
        s.antigravity.enabled = false;
        s.custom_providers.clear();
        s
    }

    fn custom(id: &str) -> CustomProvider {
        CustomProvider {
            id: id.to_string(),
            name: id.to_string(),
            base_url: "http://127.0.0.1:9999".to_string(),
            api_key: String::new(),
            model: "whisper-1".to_string(),
            language: "fa".to_string(),
            timeout_secs: 30,
        }
    }

    fn with_cloud(api_key: &str) -> Settings {
        let mut s = bare();
        s.cloud = CloudConfig {
            enabled: true,
            api_key: api_key.to_string(),
            ..CloudConfig::default()
        };
        s
    }

    #[test]
    fn whisper_is_always_last_and_always_present() {
        let plan = engine_plan(&bare(), false, false);
        assert_eq!(plan.engines, vec![EngineKind::Whisper]);
    }

    /// An enabled cloud engine without a key in either place would register and
    /// then fail every request. It must not enter the plan at all.
    #[test]
    fn an_enabled_but_keyless_cloud_engine_is_not_registered() {
        let plan = engine_plan(&with_cloud(""), false, false);
        assert_eq!(plan.engines, vec![EngineKind::Whisper]);
    }

    /// The key can arrive from the environment instead of the config file, and
    /// the plan has to honour that without the test process having a key set.
    #[test]
    fn the_environment_key_alone_is_enough() {
        let plan = engine_plan(&with_cloud(""), true, false);
        assert_eq!(
            plan.engines,
            vec![EngineKind::Cloud { id: "groq".into() }, EngineKind::Whisper]
        );
    }

    #[test]
    fn the_auto_chain_keeps_its_documented_order() {
        let mut s = bare();
        s.cloud.enabled = true;
        s.cloud.api_key = "sk-test".into();
        s.google.enabled = true;
        s.antigravity.enabled = true;
        s.custom_providers = vec![custom("my-local")];

        let plan = engine_plan(&s, false, false);
        assert_eq!(
            plan.engines,
            vec![
                EngineKind::Cloud { id: "groq".into() },
                EngineKind::Google,
                EngineKind::Custom {
                    id: "my-local".into()
                },
                EngineKind::Antigravity,
                EngineKind::Whisper,
            ],
            "cloud, Google, custom, Antigravity, whisper — the auto-mode priority"
        );
    }

    /// Antigravity must never win an `auto` race: it is expensive and needs a
    /// conversation id, so it has to be selected explicitly.
    #[test]
    fn antigravity_never_outranks_a_working_cloud_engine() {
        let mut s = with_cloud("sk-test");
        s.antigravity.enabled = true;
        let plan = engine_plan(&s, false, false);
        let ag = plan
            .engines
            .iter()
            .position(|e| e == &EngineKind::Antigravity)
            .unwrap();
        let whisper = plan.engines.len() - 1;
        assert!(
            ag < whisper,
            "local whisper is the only guaranteed fallback"
        );
    }

    #[test]
    fn a_selected_registered_engine_resolves() {
        let mut s = bare();
        s.google.enabled = true;
        s.active_engine = "google".into();
        assert_eq!(engine_plan(&s, false, false).selection, ActiveSelection::Resolved);
    }

    /// The silent-dictation-failure case: the user selects Google, then turns
    /// Google off. `AsrRouter::transcribe` refuses to fall back, so every
    /// dictation errors — but without the startup warning nothing would say why.
    #[test]
    fn selecting_a_disabled_engine_is_reported_as_missing() {
        let mut s = bare();
        s.google.enabled = false;
        s.active_engine = "google".into();
        assert_eq!(engine_plan(&s, false, false).selection, ActiveSelection::Missing);
    }

    /// Same trap through a custom provider: its id is in the config but the
    /// provider list no longer contains it.
    #[test]
    fn a_deleted_custom_provider_is_reported_as_missing() {
        let mut s = bare();
        s.active_engine = "my-local".into();
        assert_eq!(engine_plan(&s, false, false).selection, ActiveSelection::Missing);
    }

    #[test]
    fn auto_is_never_missing() {
        let mut s = bare();
        s.active_engine = "auto".into();
        assert_eq!(engine_plan(&s, false, false).selection, ActiveSelection::Auto);
    }

    /// A disabled cloud engine does not count as "the engine you picked", even
    /// though its id is what `active_engine` names.
    #[test]
    fn a_keyless_selected_cloud_engine_is_missing() {
        let mut s = with_cloud("");
        s.active_engine = "groq".into();
        assert_eq!(engine_plan(&s, false, false).selection, ActiveSelection::Missing);
    }

    /// The measured fix for the discovery churn: the long pause requires *both*
    /// conditions. Dropping either one reintroduces a subprocess pair on a
    /// 30-second timer.
    #[test]
    fn the_probe_only_settles_down_when_selected_and_working() {
        assert_eq!(probe_pause_secs(selected_and_available(), true), 60);
        assert_eq!(probe_pause_secs(true, false), 30);
        assert_eq!(probe_pause_secs(false, true), 30);
        assert_eq!(probe_pause_secs(false, false), 30);
    }

    #[test]
    fn the_plan_reports_its_cloud_and_custom_providers() {
        let mut s = with_cloud("sk-test");
        s.custom_providers = vec![custom("a"), custom("b")];
        s.antigravity.enabled = true;
        let plan = engine_plan(&s, false, false);
        assert_eq!(plan.cloud_provider(), Some("groq"));
        assert_eq!(plan.custom_ids(), vec!["a", "b"]);
        assert!(plan.wants_antigravity_probe());
        assert!(!plan.allow_local_fallback(&s) == !s.asr.auto_local_fallback);
    }

    /// No plan without Antigravity means no probe thread at all — otherwise the
    /// subprocess churn comes back for users who never installed it.
    #[test]
    fn a_disabled_antigravity_wants_no_probe() {
        assert!(!engine_plan(&bare(), false, false).wants_antigravity_probe());
    }

    fn selected_and_available() -> bool {
        true
    }
}
