//! A diagnostic report a user can actually read.
//!
//! Three separate bugs in this app shared one shape: the app worked, looked
//! normal, and quietly did the wrong thing. A hotkey string naming a Persian
//! letter became CapsLock; a selected-but-disabled ASR engine made every
//! dictation fail; a mistyped flag in `config.toml` was ignored. Each wrote a
//! `tracing::warn!` — which nobody opens — and then returned a value
//! indistinguishable from success.
//!
//! This module turns those values into a file next to `config.toml`.
//! `main.rs` is built with `#![windows_subsystem = "windows"]`, so there is no
//! stdout: a report the user cannot print is a report they will not read. Hence
//! a plain text file, rewritten on every normal startup *and* on demand via
//! `--doctor`.
//!
//! The report is assembled from pure values (`Settings`, `EnginePlan`,
//! `HotkeyConfig::problems`) and deliberately does **not** open the microphone,
//! map a model, or touch the network. It has to work on a machine where the app
//! itself cannot start, which is precisely when it is needed.

use std::fmt::Write as _;
use std::path::Path;

use crate::asr::plan::{engine_plan, ActiveSelection};
use crate::config::Settings;
use crate::hotkey::HotkeyConfig;
use crate::paths;

/// How healthy the configuration is, counted rather than stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// Nothing is substituted or missing; the app should behave as configured.
    Clean,
    /// Settings were replaced with working defaults. The app works, but not the
    /// way the user asked — the exact state that was invisible before.
    Substituted,
    /// Something the user selected will not run at all.
    Broken,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::Clean => "OK",
            Verdict::Substituted => "SUBSTITUTED",
            Verdict::Broken => "BROKEN",
        }
    }
}

/// Everything worth telling the user, as data.
///
/// Built by [`diagnose`] from values only, so the whole report can be asserted
/// on in a unit test without a filesystem, a microphone, or a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    pub verdict: Verdict,
    pub version: &'static str,
    pub config_path: String,
    pub models_dir: String,
    pub assets_dir: String,
    /// One line per substituted or missing thing, already phrased for a human.
    pub problems: Vec<String>,
    /// Engine ids in router priority order.
    pub engines: Vec<String>,
    pub active_engine: String,
    /// Whether the API key came from the environment rather than the file. The
    /// report deliberately does not print either value.
    pub cloud_key_source: KeySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    ConfigFile,
    Environment,
    Missing,
}

impl KeySource {
    fn label(self) -> &'static str {
        match self {
            KeySource::ConfigFile => "config.toml",
            KeySource::Environment => "VOICE_PTT_CLOUD_KEY environment variable",
            KeySource::Missing => "nowhere",
        }
    }
}

/// Assembles the diagnosis. Pure: nothing here reads or writes the filesystem.
pub fn diagnose(
    settings: &Settings,
    hotkeys: &HotkeyConfig,
    cloud_key_in_env: bool,
    config_path: &Path,
) -> Diagnosis {
    let plan = engine_plan(settings, cloud_key_in_env);
    let mut problems: Vec<String> = hotkeys.problems().iter().map(|p| p.message()).collect();

    if plan.selection == ActiveSelection::Missing {
        problems.push(format!(
            "ASR engine '{}' is selected but not registered, so every dictation will fail. \
             Re-enable it, or set the active engine to 'auto'.",
            settings.active_engine
        ));
    }
    if settings.cloud.enabled && key_source(settings, cloud_key_in_env) == KeySource::Missing {
        problems.push(
            "The cloud ASR engine is enabled but has no API key in config.toml and none in \
             VOICE_PTT_CLOUD_KEY, so it is not registered."
                .to_string(),
        );
    }

    let verdict = if plan.selection == ActiveSelection::Missing {
        Verdict::Broken
    } else if problems.is_empty() {
        Verdict::Clean
    } else {
        Verdict::Substituted
    };

    Diagnosis {
        verdict,
        version: env!("CARGO_PKG_VERSION"),
        config_path: config_path.display().to_string(),
        models_dir: paths::resolve_models_dir().display().to_string(),
        assets_dir: paths::resolve_assets_dir().display().to_string(),
        problems,
        engines: plan.engines.iter().map(|e| e.id().to_string()).collect(),
        active_engine: settings.active_engine.clone(),
        cloud_key_source: key_source(settings, cloud_key_in_env),
    }
}

/// Where the cloud key comes from — never its value.
fn key_source(settings: &Settings, cloud_key_in_env: bool) -> KeySource {
    if !settings.cloud.api_key.trim().is_empty() {
        KeySource::ConfigFile
    } else if cloud_key_in_env {
        KeySource::Environment
    } else {
        KeySource::Missing
    }
}

/// Renders the report as plain text.
///
/// Plain text, not JSON or markdown: the person reading this is a user whose
/// push-to-talk stopped working, opening the file in Notepad. Every line has to
/// be readable without a renderer.
pub fn render(d: &Diagnosis) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "voice-ptt diagnostic report");
    let _ = writeln!(out, "version  {} ({})", d.version, d.verdict.label());
    let _ = writeln!(out);

    let _ = writeln!(out, "paths");
    let _ = writeln!(out, "  config  {}", d.config_path);
    let _ = writeln!(out, "  models  {}", d.models_dir);
    let _ = writeln!(out, "  assets  {}", d.assets_dir);
    let _ = writeln!(out);

    let _ = writeln!(out, "asr");
    let _ = writeln!(out, "  active    {}", d.active_engine);
    let _ = writeln!(out, "  engines   {}", d.engines.join(" -> "));
    let _ = writeln!(out, "  cloud key from {}", d.cloud_key_source.label());
    let _ = writeln!(out);

    if d.problems.is_empty() {
        let _ = writeln!(out, "problems");
        let _ = writeln!(out, "  none — the configuration is being used as written");
    } else {
        let _ = writeln!(out, "problems ({} found)", d.problems.len());
        for (i, problem) in d.problems.iter().enumerate() {
            let _ = writeln!(out, "  {}. {problem}", i + 1);
        }
    }

    out
}

/// Writes the report, creating the file if needed. Best-effort: a diagnostic
/// tool that refuses to start because its own output file is unwritable would
/// be worse than useless.
pub fn write_report(d: &Diagnosis, path: &Path) -> std::io::Result<()> {
    std::fs::write(path, render(d))
}

/// Writes the report to the standard location and returns where it went.
pub fn write_default(d: &Diagnosis) -> std::path::PathBuf {
    let path = paths::resolve_doctor_report_path();
    if let Err(e) = write_report(d, &path) {
        tracing::error!(error = %e, path = %path.display(), "could not write diagnostic report");
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::CloudConfig;
    use crate::config::HotkeySettings;

    fn settings() -> Settings {
        Settings::default()
    }

    fn good_hotkeys() -> HotkeyConfig {
        HotkeyConfig::from_settings(&HotkeySettings::default())
    }

    fn diag(settings: &Settings, hotkeys: &HotkeyConfig, env: bool) -> Diagnosis {
        diagnose(settings, hotkeys, env, Path::new("C:/x/config.toml"))
    }

    /// The default install must come out clean, or every user gets a report
    /// that says something is wrong and the report stops meaning anything.
    #[test]
    fn a_default_install_is_clean() {
        let d = diag(&settings(), &good_hotkeys(), false);
        assert_eq!(d.verdict, Verdict::Clean);
        assert!(d.problems.is_empty());
    }

    /// The bug this whole feature exists for: the app works, on the wrong key.
    /// It must not read as OK.
    #[test]
    fn a_substituted_hotkey_is_not_ok() {
        let hotkeys = HotkeyConfig::from_settings(&HotkeySettings {
            record: "Shift+ف".into(),
            ..HotkeySettings::default()
        });
        let d = diag(&settings(), &hotkeys, false);
        assert_eq!(d.verdict, Verdict::Substituted);
        assert_eq!(d.problems.len(), 1);
        assert!(
            d.problems[0].contains("CapsLock"),
            "the user must learn what replaced their key: {}",
            d.problems[0]
        );
    }

    /// A dead engine selection is worse than a substituted one: nothing works.
    #[test]
    fn a_dead_engine_selection_is_broken() {
        let mut s = settings();
        s.active_engine = "google".into();
        s.google.enabled = false;
        let d = diag(&s, &good_hotkeys(), false);
        assert_eq!(d.verdict, Verdict::Broken);
        assert!(d.problems[0].contains("google"), "{}", d.problems[0]);
    }

    /// Broken outranks substituted: with both present, "your dictation does
    /// nothing" is the fact the user needs first.
    #[test]
    fn broken_outranks_substituted() {
        let mut s = settings();
        s.active_engine = "google".into();
        s.google.enabled = false;
        let hotkeys = HotkeyConfig::from_settings(&HotkeySettings {
            record: "nonsense+".into(),
            ..HotkeySettings::default()
        });
        let d = diag(&s, &hotkeys, false);
        assert_eq!(d.verdict, Verdict::Broken);
        assert_eq!(d.problems.len(), 2, "both must still be listed");
    }

    #[test]
    fn an_enabled_but_keyless_cloud_engine_is_reported() {
        let mut s = settings();
        s.cloud = CloudConfig {
            enabled: true,
            ..CloudConfig::default()
        };
        let d = diag(&s, &good_hotkeys(), false);
        assert_eq!(d.verdict, Verdict::Substituted);
        assert!(d.problems[0].contains("no API key"), "{}", d.problems[0]);
    }

    /// The report is written to a file that gets emailed around. Neither the
    /// key nor anything derived from it may appear in it.
    #[test]
    fn no_secret_is_ever_rendered() {
        let mut s = settings();
        s.cloud = CloudConfig {
            enabled: true,
            api_key: "sk-secret-value-12345".into(),
            ..CloudConfig::default()
        };
        let d = diag(&s, &good_hotkeys(), false);
        let text = render(&d);
        assert!(!text.contains("sk-secret-value-12345"), "{text}");
        assert!(
            text.contains("config.toml"),
            "the source is still named: {text}"
        );
    }

    /// A key that only exists in the environment must be reported as such —
    /// otherwise a user reading "key from nowhere" would go edit the file and
    /// never find what they were looking for.
    #[test]
    fn the_environment_key_is_named() {
        let mut s = settings();
        s.cloud = CloudConfig {
            enabled: true,
            ..CloudConfig::default()
        };
        let d = diag(&s, &good_hotkeys(), true);
        assert_eq!(d.cloud_key_source, KeySource::Environment);
        assert!(render(&d).contains("VOICE_PTT_CLOUD_KEY"));
    }

    /// The rendered text is the entire user interface of this feature.
    #[test]
    fn the_report_names_the_version_and_the_verdict() {
        let text = render(&diag(&settings(), &good_hotkeys(), false));
        assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
        assert!(text.contains("OK"), "{text}");
        assert!(
            text.contains("none — the configuration is being used"),
            "{text}"
        );
    }

    /// The engine list is the answer to "which engines am I actually running?",
    /// so it must show priority order, not a set.
    ///
    /// Measured, not assumed: `GoogleConfig` and `AntigravityConfig` both
    /// default to enabled, so a fresh install is a three-engine chain. The
    /// earlier version of this test assumed two and failed — which is the
    /// point of running it.
    #[test]
    fn the_engine_chain_is_shown_in_order() {
        let d = diag(&settings(), &good_hotkeys(), false);
        assert_eq!(
            d.engines,
            vec!["google", "antigravity", "local_whisper"],
            "both cloud engines ship enabled; whisper is the always-available tail"
        );
        assert!(render(&d).contains("google -> antigravity -> local_whisper"));
    }

    /// Turning one off must visibly shorten the chain the report prints.
    #[test]
    fn a_disabled_engine_leaves_the_reported_chain() {
        let mut s = settings();
        s.antigravity.enabled = false;
        let d = diag(&s, &good_hotkeys(), false);
        assert_eq!(d.engines, vec!["google", "local_whisper"]);
    }

    #[test]
    fn writing_actually_writes_readable_text() {
        let dir = std::env::temp_dir().join("voice-ptt-doctor-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("report.txt");
        let d = diag(&settings(), &good_hotkeys(), false);
        write_report(&d, &path).expect("write must succeed");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("voice-ptt diagnostic report"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
