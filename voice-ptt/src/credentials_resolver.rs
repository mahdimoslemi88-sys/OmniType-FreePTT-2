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
    explicit_delete_credential, explicit_update_credential, CredentialError, CredentialStore,
    SecretString, WindowsCredentialStore,
};

/// The service name for the built-in cloud key. Used to derive the store target
/// and the settings key together, so the two cannot drift apart.
pub const CLOUD_SERVICE: &str = "cloud";

/// Prefix for a user-added provider's service name in the store.
///
/// The full target is `OmniTypeFreePTT/custom:<provider id>`, so a provider is
/// keyed by its **stable id**, never its display name: renaming a provider must
/// not orphan its key, and two providers that happen to share a display name
/// must not share one either.
const CUSTOM_PREFIX: &str = "custom:";

/// The service name a custom provider's key lives under.
pub fn custom_service(provider_id: &str) -> String {
    format!("{CUSTOM_PREFIX}{provider_id}")
}

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
    *source_slot().write().unwrap_or_else(|e| e.into_inner()) = source;
    true
}

/// The installed key, if any.
pub fn cloud_key() -> Option<SecretString> {
    slot().read().unwrap_or_else(|e| e.into_inner()).clone()
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

/// The key for a custom provider, read directly from the store.
///
/// Custom providers have **no environment override**: the built-in cloud key has
/// one as an escape hatch, but adding a second, provider-named environment
/// variable would invent a configuration surface the panel does not show and
/// the user did not ask for. The store is the one place a custom key lives.
pub fn stored_custom_key(provider_id: &str) -> Option<SecretString> {
    let store = WindowsCredentialStore::new();
    match store.load(&target_for(&custom_service(provider_id))) {
        Ok(secret) if !secret.is_empty() => Some(secret),
        _ => None,
    }
}

/// Stores a custom provider key after an **explicit user save** and returns the
/// store target it wrote under.
///
/// This is the deliberate-overwrite path: the user typed a key and pressed save,
/// so replacing whatever was there is exactly what they asked for. The automatic
/// migration is the opposite case and refuses to overwrite — see
/// [`MigrationError::Conflict`].
///
/// Order is save → read back → compare, so a store that silently fails to persist
/// the new value is reported as a failure and the caller can keep the old state
/// rather than clearing the settings file over a key that never landed.
pub fn store_custom_key(
    provider_id: &str,
    secret: &SecretString,
) -> Result<String, CredentialError> {
    let store = WindowsCredentialStore::new();
    let target = target_for(&custom_service(provider_id));
    explicit_update_credential(&store, &target, secret)?;
    match store.load(&target) {
        Ok(read_back) if read_back.expose_secret() == secret.expose_secret() => Ok(target),
        Ok(_) => Err(CredentialError::CorruptedData(target)),
        Err(err) => Err(err),
    }
}

/// Deletes one custom provider's key, touching no other target.
pub fn delete_custom_key(provider_id: &str) -> Result<(), CredentialError> {
    let store = WindowsCredentialStore::new();
    let target = target_for(&custom_service(provider_id));
    explicit_delete_credential(&store, &target)
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

/// Clears one key from the real settings file, without losing anything else.
///
/// The write itself is not done here: it goes through
/// [`crate::config::settings::Settings::transact`], the read-modify-write form of
/// the settings contract (F1) — atomic replace **and** a version check, so a
/// change another writer made between this persister's read and its write is
/// re-applied on top of rather than overwritten. The dashboard takes the same
/// per-file lock, which is what makes "all writers" true instead of "all writers
/// except this one".
///
/// This type's own job is the **scope** of the clear: exactly the one key the
/// migration verified is now in the store, with every other field — the user's
/// unrelated settings, and a UI edit that landed while the migration worked —
/// left as it is.
pub struct SettingsFilePersister {
    config_path: std::path::PathBuf,
    /// Test seam: run inside the write transaction, after the file has been read
    /// and the clear applied, and before the write decides whether its read is
    /// still current.
    ///
    /// It exists so the interleaving this persister is most likely to meet — the
    /// dashboard writing an unrelated setting while the migration is working —
    /// can be placed **deterministically** in a test rather than raced for with
    /// sleeps. Production hands `None`.
    gate: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl SettingsFilePersister {
    pub fn new(config_path: std::path::PathBuf) -> Self {
        Self {
            config_path,
            gate: None,
        }
    }

    /// The same persister, with a gate that another writer's change runs at.
    #[cfg(test)]
    fn with_gate(
        config_path: std::path::PathBuf,
        gate: std::sync::Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            config_path,
            gate: Some(gate),
        }
    }

    fn gate(&self) -> Option<&dyn Fn()> {
        match &self.gate {
            Some(gate) => Some(gate.as_ref()),
            None => None,
        }
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
        let gate = self.gate();

        // Custom providers first: their target carries the stable provider id,
        // so the settings entry to clear is found by id and not by display name.
        if let Some(provider_id) = target
            .strip_prefix(&target_for(CUSTOM_PREFIX))
            .map(str::to_string)
        {
            return crate::config::settings::Settings::transact_with(
                &self.config_path,
                move |settings| {
                    // Not found is not an error: the provider was already removed
                    // from the file, so its key has nowhere left to be written.
                    if let Some(provider) = settings
                        .custom_providers
                        .iter_mut()
                        .find(|p| p.id == provider_id)
                    {
                        provider.api_key.clear();
                    }
                },
                gate,
            )
            .map(|_| ())
            .map_err(|e| fail(format!("could not persist the cleared key: {e:#}")));
        }

        let Some((_, field)) = CLEARABLE.iter().find(|(service, _)| {
            // The target is the full store name; only the service part is ours.
            target == target_for(service) || target == *service
        }) else {
            // An unknown target is not an error: there is nothing in the
            // settings file for it, so it is already "cleared".
            return Ok(());
        };

        crate::config::settings::Settings::transact_with(
            &self.config_path,
            move |settings| field(settings).clear(),
            gate,
        )
        .map(|_| ())
        .map_err(|e| fail(format!("could not persist the cleared key: {e:#}")))
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
    /// Custom provider ids whose keys were moved into the store.
    pub custom_migrated: Vec<String>,
    /// `(provider id, reason)` for a custom provider whose key was left alone
    /// because the store already held a *different* value for it.
    pub custom_conflict: Vec<(String, String)>,
    /// `(provider id, reason)` for a custom provider whose key could not be
    /// stored; same guarantee, the key stays in `config.toml`.
    pub custom_store_failed: Vec<(String, String)>,
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
            parts.push(format!(
                "store unavailable, key left in config.toml: {reason}"
            ));
        }
        if !self.custom_migrated.is_empty() {
            // Names only, never the keys: a summary line is a log line.
            parts.push(format!(
                "{} custom provider key(s) moved to Credential Manager: {}",
                self.custom_migrated.len(),
                self.custom_migrated.join(", ")
            ));
        }
        for (id, reason) in &self.custom_conflict {
            parts.push(format!(
                "CONFLICT for custom provider '{id}', key left in config.toml: {reason}"
            ));
        }
        for (id, reason) in &self.custom_store_failed {
            parts.push(format!(
                "custom provider '{id}' store unavailable, key left in config.toml: {reason}"
            ));
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
    settings: &crate::config::settings::Settings,
) -> MigrationReport {
    let mut report = MigrationReport::default();
    let store = WindowsCredentialStore::new();
    let target = target_for(CLOUD_SERVICE);
    let persister = SettingsFilePersister::new(config_path.to_path_buf());
    let legacy_config_key = settings.cloud.api_key.as_str();

    if !legacy_config_key.trim().is_empty() {
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

    // ── every custom provider, by its stable id ────────────────────────────
    // The same 5-step pipeline and the same refusal to blind-overwrite: a store
    // that already holds a *different* key for one provider keeps it, and the
    // plaintext copy stays in `config.toml` so the app keeps working.
    for provider in &settings.custom_providers {
        if provider.api_key.trim().is_empty() {
            continue;
        }
        let custom_target = target_for(&custom_service(&provider.id));
        let item = MigrationItem::new(custom_target.clone(), provider.api_key.trim());
        match migrate_credential(&store, &persister, &item).result {
            Ok(MigrationAction::MigratedFresh) | Ok(MigrationAction::MigratedAlreadyPresent) => {
                report.custom_migrated.push(provider.id.clone());
            }
            Ok(MigrationAction::SkippedEmpty) => {}
            Err(MigrationError::Conflict(conflict)) => {
                report
                    .custom_conflict
                    .push((provider.id.clone(), conflict.message.clone()));
                tracing::warn!(
                    target = %custom_target,
                    provider = %provider.id,
                    "custom provider key already stored with a different value; leaving config.toml untouched"
                );
            }
            Err(error) => {
                let message = error.to_string();
                report
                    .custom_store_failed
                    .push((provider.id.clone(), message.clone()));
                tracing::warn!(
                    target = %custom_target,
                    provider = %provider.id,
                    error = %message,
                    "custom provider credential migration failed"
                );
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
        assert_eq!(
            KeySource::Environment.as_str(),
            "environment variable VOICE_PTT_CLOUD_KEY"
        );
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
            ..Default::default()
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
            ..Default::default()
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

    /// A custom provider is keyed by its **id**, not its display name.
    ///
    /// The name is a label the user can change at any moment; if it were part
    /// of the store target, renaming a provider would silently orphan its key
    /// and the next dictation would fail to authenticate.
    #[test]
    fn a_custom_provider_is_keyed_by_id_not_by_its_display_name() {
        assert_eq!(custom_service("groq"), "custom:groq");
        assert_eq!(custom_service("my-local"), "custom:my-local");
        assert_ne!(custom_service("a"), custom_service("b"));

        let target = target_for(&custom_service("my-local"));
        assert_eq!(target, "OmniTypeFreePTT/custom:my-local");
        // And it is not the same target as the built-in cloud key, so the two
        // can never overwrite each other.
        assert_ne!(target, target_for(CLOUD_SERVICE));
    }

    struct TempConfig(std::path::PathBuf);

    impl TempConfig {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "omnitype-persister-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.join("config.toml"))
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            if let Some(parent) = self.0.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    /// F1 through the **real** persister: the file is written atomically, the
    /// named key is cleared, and nothing else in the file is disturbed.
    ///
    /// Deliberately uses a temp `config.toml` and no credential store at all:
    /// the persister's whole job is the file, and a test of it must never touch
    /// the user's real credentials.
    #[test]
    fn the_persister_clears_the_named_cloud_key_and_leaves_everything_else_alone() {
        use crate::config::settings::Settings;

        let temp = TempConfig::new("cloud");
        let mut settings = Settings::default();
        settings.cloud.api_key = "secret-to-remove".into();
        settings.cloud.model = "keep-this-model".into();
        settings.text.mode = "formal".into();
        settings.gui.draft_ttl_secs = 90;
        settings.save(temp.path()).unwrap();

        let persister = SettingsFilePersister::new(temp.path().to_path_buf());
        persister
            .persist_cleared_key(&target_for(CLOUD_SERVICE))
            .expect("clearing a known key succeeds");

        let after = Settings::load_or_create(temp.path()).unwrap();
        assert!(after.cloud.api_key.is_empty(), "the key is gone");
        assert_eq!(
            after.cloud.model, "keep-this-model",
            "switching engines survives"
        );
        assert_eq!(after.text.mode, "formal", "unrelated settings survive");
        assert_eq!(after.gui.draft_ttl_secs, 90);

        // Re-running is idempotent: a second migration finds nothing to clear
        // and must not fail or corrupt the file.
        persister
            .persist_cleared_key(&target_for(CLOUD_SERVICE))
            .expect("clearing an already-cleared key is not an error");
        let again = Settings::load_or_create(temp.path()).unwrap();
        assert!(again.cloud.api_key.is_empty());
        assert_eq!(again.cloud.model, "keep-this-model");
    }

    /// F2 through the real persister: a custom provider's key is cleared by the
    /// target derived from its **id**, and only that provider is touched.
    #[test]
    fn the_persister_clears_one_custom_providers_key_by_id() {
        use crate::config::settings::{CustomProvider, Settings};

        let temp = TempConfig::new("custom");
        let provider = |id: &str, name: &str, key: &str| CustomProvider {
            id: id.into(),
            name: name.into(),
            base_url: "https://example.invalid/v1".into(),
            api_key: key.into(),
            model: "whisper-1".into(),
            language: "fa".into(),
            timeout_secs: 20,
        };
        let settings = Settings {
            custom_providers: vec![
                provider("first", "نام قدیمی", "key-first"),
                provider("second", "Second", "key-second"),
            ],
            ..Settings::default()
        };
        settings.save(temp.path()).unwrap();

        let persister = SettingsFilePersister::new(temp.path().to_path_buf());
        persister
            .persist_cleared_key(&target_for(&custom_service("first")))
            .expect("clearing a known custom key succeeds");

        let after = Settings::load_or_create(temp.path()).unwrap();
        assert_eq!(after.custom_providers.len(), 2, "no provider was removed");
        assert!(
            after.custom_providers[0].api_key.is_empty(),
            "the named provider's key is cleared"
        );
        assert_eq!(
            after.custom_providers[1].api_key, "key-second",
            "an unrelated provider's key must not be touched"
        );
        assert_eq!(
            after.custom_providers[0].name, "نام قدیمی",
            "the display name is not the identity and is left as it was"
        );

        // A target whose provider is no longer in the file is not an error:
        // there is nothing left to clear.
        persister
            .persist_cleared_key(&target_for(&custom_service("never-existed")))
            .expect("an unknown provider is already cleared");
    }

    /// F1: the read-then-write window, closed — through the **real** persister.
    ///
    /// The exact sequence the finding named, in order and without a sleep: the
    /// migration reads the file, the dashboard writes an **unrelated** setting,
    /// the migration carries on and saves. Both changes have to survive. The gate
    /// is what makes the interleaving deterministic; racing for it is a test that
    /// passes on a fast machine and proves nothing on a slow one.
    #[test]
    fn a_dashboard_change_made_while_the_migration_reads_survives_its_save() {
        use crate::config::settings::Settings;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let temp = TempConfig::new("migration-interleave");
        let mut before = Settings::default();
        before.cloud.api_key = "secret-to-remove".into();
        before.gui.draft_ttl_secs = 30;
        before.save(temp.path()).unwrap();

        // What the dashboard does while the migration is working: one unrelated
        // setting, written the way every other writer writes.
        let writes = std::sync::Arc::new(AtomicUsize::new(0));
        let gate_writes = writes.clone();
        let live_path = temp.path().to_path_buf();
        let gate = move || {
            if gate_writes.fetch_add(1, Ordering::SeqCst) > 0 {
                // One user action, once. A gate that wrote on every attempt
                // would be testing the retry bound instead of this.
                return;
            }
            Settings::transact(&live_path, |settings| settings.gui.draft_ttl_secs = 90)
                .expect("the dashboard's own write succeeds");
        };

        let persister =
            SettingsFilePersister::with_gate(temp.path().to_path_buf(), std::sync::Arc::new(gate));
        persister
            .persist_cleared_key(&target_for(CLOUD_SERVICE))
            .expect("clearing a known key succeeds");

        let after = Settings::load_or_create(temp.path()).unwrap();
        assert!(
            after.cloud.api_key.is_empty(),
            "the key the migration verified is in the store must be gone from the file"
        );
        assert_eq!(
            after.gui.draft_ttl_secs, 90,
            "and the setting the user changed while the migration read must survive"
        );
        assert!(
            writes.load(Ordering::SeqCst) >= 1,
            "the gate has to have run, or this scenario never happened"
        );
    }

    /// The same window for a **custom** provider, through the id-keyed target:
    /// the clear is an edit like any other and follows the same contract.
    #[test]
    fn a_custom_providers_clear_also_keeps_a_concurrent_change() {
        use crate::config::settings::{CustomProvider, Settings};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let temp = TempConfig::new("custom-interleave");
        let settings = Settings {
            custom_providers: vec![CustomProvider {
                id: "mine".into(),
                name: "نام".into(),
                base_url: "https://example.invalid/v1".into(),
                api_key: "custom-secret".into(),
                model: "whisper-1".into(),
                language: "fa".into(),
                timeout_secs: 20,
            }],
            ..Settings::default()
        };
        settings.save(temp.path()).unwrap();

        let writes = std::sync::Arc::new(AtomicUsize::new(0));
        let gate_writes = writes.clone();
        let live_path = temp.path().to_path_buf();
        let gate = move || {
            if gate_writes.fetch_add(1, Ordering::SeqCst) > 0 {
                return;
            }
            Settings::transact(&live_path, |settings| settings.text.mode = "formal".into())
                .expect("the dashboard's own write succeeds");
        };

        let persister =
            SettingsFilePersister::with_gate(temp.path().to_path_buf(), std::sync::Arc::new(gate));
        persister
            .persist_cleared_key(&target_for(&custom_service("mine")))
            .expect("clearing a known custom key succeeds");

        let after = Settings::load_or_create(temp.path()).unwrap();
        assert_eq!(
            after.custom_providers.len(),
            1,
            "the provider is not removed"
        );
        assert!(
            after.custom_providers[0].api_key.is_empty(),
            "its key is cleared"
        );
        assert_eq!(
            after.text.mode, "formal",
            "and the unrelated change made while it was read survives"
        );
    }

    /// An unknown target is still not an error — the pre-F2 behaviour that the
    /// cloud migration relies on when a service has no settings field.
    #[test]
    fn an_unknown_target_is_already_cleared() {
        let temp = TempConfig::new("unknown");
        let persister = SettingsFilePersister::new(temp.path().to_path_buf());
        persister
            .persist_cleared_key("OmniTypeFreePTT/not-a-service")
            .expect("nothing in the file to clear is not a failure");
    }
}
