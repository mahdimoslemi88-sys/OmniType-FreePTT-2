//! Getting the API key to the engine **without** putting it back on disk.
//!
//! The problem this solves is narrow and specific. The key used to live in
//! `config.toml`, and every "save settings" path in the app writes that file —
//! so simply reading the key into memory at startup is not enough: the next save
//! would write it straight back out. Keeping the resolved key *out* of
//! [`crate::config::Settings`] is therefore the whole design, not a detail.
//!
//! Precedence, highest first:
//!
//! 1. `VOICE_PTT_CLOUD_KEY` — an explicit per-launch override, and the escape
//!    hatch for a machine where Credential Manager is unavailable.
//! 2. The credential store, where [`migrate_on_startup`] put it.
//! 3. `config.toml`, which is read *only* if the migration did not run — so a
//!    user whose migration fails keeps working rather than losing their key.
//!
//! The installed value lives in a process-global, matching the existing
//! `asr::progress::set_sink` pattern in this codebase: a handle threaded through
//! `AsrRouter` → `CloudEngine` → `CloudConfig` would touch a dozen signatures
//! and every one of them would then have to decide whether the key may be
//! persisted — which is the decision that must be made in exactly one place.
//!
//! # Why this is a top-level module and not `credentials::resolver`
//!
//! Because it needs [`crate::config::settings::Settings`], and
//! `tests/credentials_core_test.rs` compiles `credentials/` standalone through
//! `#[path]`. Nesting the wiring inside the contract would drag the whole
//! settings model into a test that is deliberately about the store alone. The
//! store contract stays self-contained; the app's use of it lives here.

use std::sync::{OnceLock, RwLock};

use crate::credentials::migration::{
    migrate_credential, MigrationAction, MigrationError, MigrationItem, PersistError,
    SettingsPersister,
};
use crate::credentials::windows::target_for;
use crate::credentials::{
    CredentialError, CredentialStore, SecretString, WindowsCredentialStore,
};

/// The service name for the built-in cloud key. Used to derive the store target
/// and the settings key together, so the two cannot drift apart.
pub const CLOUD_SERVICE: &str = "cloud";

/// Where the key the app is using came from.
///
/// Kept distinct from "is the key valid" on purpose: a user whose key came from
/// the environment needs to be told *that*, because the thing they are about to
/// edit in settings will not change anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    Store,
    ConfigFile,
    Missing,
}

impl KeySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment variable VOICE_PTT_CLOUD_KEY",
            Self::Store => "Windows Credential Manager",
            Self::ConfigFile => "config.toml",
            Self::Missing => "nowhere",
        }
    }
}

/// The installed key, plus where it came from.
static CLOUD_KEY: OnceLock<RwLock<Option<SecretString>>> = OnceLock::new();
static CLOUD_SOURCE: OnceLock<RwLock<KeySource>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<SecretString>> {
    CLOUD_KEY.get_or_init(|| RwLock::new(None))
}

fn source_slot() -> &'static RwLock<KeySource> {
    CLOUD_SOURCE.get_or_init(|| RwLock::new(KeySource::Missing))
}

/// Installs the resolved key for the rest of the process.
///
/// Returns `false` when a key was already installed, because the second call
/// would silently change which credential the app is using — and a key that
/// changes under a running session is how a user ends up billed to one account
/// while believing they switched to another.
pub fn install_cloud_key(secret: SecretString, source: KeySource) -> bool {
    let mut key = slot().write().unwrap_or_else(|e| e.into_inner());
    if key.is_some() {
        return false;
    }
    *key = Some(secret);
    *source_slot()
        .write()
        .unwrap_or_else(|e| e.into_inner()) = source;
    true
}

/// The installed key, if any.
pub fn cloud_key() -> Option<SecretString> {
    slot()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Where the installed key came from.
pub fn cloud_key_source() -> KeySource {
    *source_slot().read().unwrap_or_else(|e| e.into_inner())
}

/// Whether a key is available from the environment or the store, *without*
/// consulting `config.toml`.
///
/// This is what the engine planner asks. Answering it from `config.toml` too
/// would keep reporting "configured" for a key the migration has just moved —
/// and then, when the store read failed, the user would get an authentication
/// error with a settings screen showing the key they thought was fine.
pub fn cloud_key_is_stored_or_env(env_key_present: bool) -> bool {
    env_key_present || cloud_key().is_some()
}

/// Whether the credential **store itself** holds the cloud key, read directly.
///
/// [`cloud_key_is_stored_or_env`] answers from the process-global slot, which
/// is only populated once [`migrate_on_startup`] has run. `voice-ptt --doctor`
/// deliberately does not run the migration — rewriting a key file as a side
/// effect of asking what is broken is the wrong moment — so it would consult an
/// empty slot and report "no key anywhere" for a user whose key is sitting in
/// Credential Manager. This asks the store directly, which is both read-only
/// and true before anything else has run.
pub fn store_holds_cloud_key() -> bool {
    let store = WindowsCredentialStore::new();
    matches!(store.load(&target_for(CLOUD_SERVICE)), Ok(secret) if !secret.is_empty())
}

// ─────────────────────────────── مهاجرت ───────────────────────────────

/// Clears one key from the real settings file, atomically enough to be trusted.
///
/// `Settings::save` is a plain `std::fs::write`, which is *not* atomic: a crash
/// between truncate and write leaves a settings file with no key anywhere. That
/// is a real risk for the one operation where losing the key means losing the
/// user's paid configuration, so the write goes to a sibling temp file first and
/// only then replaces the original.
pub struct SettingsFilePersister {
    config_path: std::path::PathBuf,
}

impl SettingsFilePersister {
    pub fn new(config_path: std::path::PathBuf) -> Self {
        Self { config_path }
    }
}

/// Where one service's plaintext key lives inside [`crate::config::Settings`].
type SettingsKeyField = fn(&mut crate::config::settings::Settings) -> &mut String;

/// The services whose key this persister can clear.
///
/// Fixed rather than a field because the app has exactly one built-in service
/// key, and a general "clear whichever key you were asked about" would mean
/// mapping store target names onto settings paths — a lookup table that has to
/// be kept in step with `Settings` by hand.
const CLEARABLE: &[(&str, SettingsKeyField)] = &[(
    CLOUD_SERVICE,
    |s: &mut crate::config::settings::Settings| &mut s.cloud.api_key,
)];

impl SettingsPersister for SettingsFilePersister {
    fn persist_cleared_key(&self, target: &str) -> Result<(), PersistError> {
        let fail = |message: String| PersistError {
            target: target.to_string(),
            message,
        };
        let Some((_, field)) = CLEARABLE.iter().find(|(service, _)| {
            // The target is the full store name; only the service part is ours.
            target == target_for(service) || target == *service
        }) else {
            // An unknown target is not an error: there is nothing in the
            // settings file for it, so it is already "cleared".
            return Ok(());
        };

        let mut settings = crate::config::settings::Settings::load_or_create(&self.config_path)
            .map_err(|e| fail(format!("could not read the settings back: {e:#}")))?;
        field(&mut settings).clear();

        settings.save(&self.config_path).map_err(|e| {
            fail(format!("could not persist the cleared key: {e:#}"))
        })
    }
}

/// What the startup migration did, in a form the log and the UI can both use.
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    /// The key was moved out of `config.toml` and into the store.
    pub migrated: bool,
    /// The store already had this exact key, so only the settings file changed.
    pub already_stored: bool,
    /// The migration refused and **nothing** was changed. The plaintext key is
    /// still in `config.toml` and the app keeps working.
    pub conflict: Option<String>,
    /// The store could not be written at all. Same guarantee: nothing changed.
    pub store_failed: Option<String>,
    /// Where the key the app will use came from, after the migration.
    pub source: Option<KeySource>,
}

impl MigrationReport {
    /// One line for the log, with no key material in it.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.migrated {
            parts.push("migrated from config.toml to Credential Manager".to_string());
        }
        if self.already_stored {
            parts.push("already in Credential Manager".to_string());
        }
        if let Some(reason) = &self.conflict {
            parts.push(format!("CONFLICT, key left in config.toml: {reason}"));
        }
        if let Some(reason) = &self.store_failed {
            parts.push(format!("store unavailable, key left in config.toml: {reason}"));
        }
        if let Some(source) = self.source {
            parts.push(format!("using key from {}", source.as_str()));
        }
        if parts.is_empty() {
            "no key configured".to_string()
        } else {
            parts.join("; ")
        }
    }
}

/// Moves the plaintext key out of `config.toml` and installs the resolved one.
///
/// The order is the whole safety property and is not negotiable:
///
/// 1. **Store first.** Write to Credential Manager.
/// 2. **Read it back** and compare byte-for-byte.
/// 3. **Only then** clear the settings file.
///
/// Every early exit before step 3 leaves `config.toml` exactly as it was, so the
/// worst case is "the key is in two places" rather than "the key is gone". A
/// migration that deletes first and stores second can lose a user's paid key
/// with no way back, and no test can undo that.
pub fn migrate_on_startup(
    config_path: &std::path::Path,
    legacy_config_key: &str,
) -> MigrationReport {
    let mut report = MigrationReport::default();
    let store = WindowsCredentialStore::new();
    let target = target_for(CLOUD_SERVICE);

    if !legacy_config_key.trim().is_empty() {
        let persister = SettingsFilePersister::new(config_path.to_path_buf());
        let item = MigrationItem::new(target.clone(), legacy_config_key.trim());
        match migrate_credential(&store, &persister, &item).result {
            Ok(MigrationAction::MigratedFresh) => report.migrated = true,
            Ok(MigrationAction::MigratedAlreadyPresent) => {
                report.migrated = true;
                report.already_stored = true;
            }
            Ok(MigrationAction::SkippedEmpty) => {}
            Err(MigrationError::Conflict(conflict)) => {
                report.conflict = Some(conflict.message.clone());
                tracing::warn!(
                    target = %target,
                    "credential store already holds a different key; leaving config.toml untouched"
                );
            }
            Err(error) => {
                // Every other failure — a store that cannot be read, written, or
                // read back — leaves `config.toml` exactly as it was, because the
                // persister is only reached after a verified round-trip.
                let message = error.to_string();
                report.store_failed = Some(message.clone());
                tracing::warn!(target = %target, error = %message, "credential migration failed");
            }
        }
    }

    // Resolve: environment beats store beats config. Read **after** the
    // migration so a key that was just written is found in the store.
    let env_key = std::env::var("VOICE_PTT_CLOUD_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    let (secret, source) = if let Some(env) = env_key {
        (SecretString::new(env), KeySource::Environment)
    } else {
        match store.load(&target) {
            Ok(secret) if !secret.is_empty() => (secret, KeySource::Store),
            Ok(_) | Err(CredentialError::NotFound(_)) if !legacy_config_key.trim().is_empty() => {
                // The store has nothing, but the settings file does — either the
                // migration refused, or there is no store on this platform.
                // Working is strictly better than not working here.
                (
                    SecretString::new(legacy_config_key.trim()),
                    KeySource::ConfigFile,
                )
            }
            _ => {
                report.source = Some(KeySource::Missing);
                tracing::info!(summary = %report.summary(), "credential migration");
                return report;
            }
        }
    };

    install_cloud_key(secret, source);
    report.source = Some(source);
    tracing::info!(summary = %report.summary(), "credential migration");
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The precedence order is a promise about which account gets billed, so it
    /// is worth naming in one place rather than in three branches.
    #[test]
    fn the_source_names_where_a_key_came_from() {
        assert_eq!(KeySource::Environment.as_str(), "environment variable VOICE_PTT_CLOUD_KEY");
        assert_eq!(KeySource::Store.as_str(), "Windows Credential Manager");
        assert_eq!(KeySource::ConfigFile.as_str(), "config.toml");
        assert_eq!(KeySource::Missing.as_str(), "nowhere");
    }

    /// Nothing in a report may contain key material, or a log line would become
    /// the plaintext leak this whole change exists to close.
    #[test]
    fn a_report_never_contains_key_material() {
        let report = MigrationReport {
            migrated: true,
            already_stored: true,
            conflict: Some("already holds a different secret".into()),
            store_failed: None,
            source: Some(KeySource::Store),
        };
        let summary = report.summary();
        for forbidden in ["sk-", "api_key", "Bearer"] {
            assert!(
                !summary.contains(forbidden),
                "the summary leaked {forbidden:?}: {summary}"
            );
        }
    }

    /// A conflict is the case where the app must keep working from the old
    /// place. The summary has to say that plainly, because "nothing happened"
    /// would leave a user with a plaintext key on disk and no idea why.
    #[test]
    fn a_conflict_is_reported_as_a_leftover_key_not_as_a_success() {
        let report = MigrationReport {
            migrated: false,
            already_stored: false,
            conflict: Some("a different secret is already stored".into()),
            store_failed: None,
            source: Some(KeySource::ConfigFile),
        };
        let summary = report.summary();
        assert!(summary.contains("CONFLICT"), "{summary}");
        assert!(summary.contains("left in config.toml"), "{summary}");
        assert!(
            !summary.contains("migrated"),
            "a conflict must not read as a success: {summary}"
        );
    }

    /// The inverse: a store failure must say the key is still on disk, or the
    /// user concludes the migration finished and stops looking.
    #[test]
    fn a_store_failure_says_the_key_is_still_in_the_settings_file() {
        let report = MigrationReport {
            store_failed: Some("access denied".into()),
            source: Some(KeySource::ConfigFile),
            ..Default::default()
        };
        let summary = report.summary();
        assert!(summary.contains("left in config.toml"), "{summary}");
        assert!(summary.contains("using key from config.toml"), "{summary}");
    }

    /// An empty report is the "nothing configured" case, and it has to read as
    /// that rather than as an empty string in a log nobody can interpret.
    #[test]
    fn an_empty_report_reads_as_no_key_configured() {
        assert_eq!(MigrationReport::default().summary(), "no key configured");
    }
}
