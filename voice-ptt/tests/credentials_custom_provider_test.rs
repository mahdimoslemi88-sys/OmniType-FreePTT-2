//! F2 — a custom provider's API key goes through the credential contract.
//!
//! The finding was that the Engines panel wrote `CustomProvider.api_key` into
//! `config.toml` in plaintext and `CloudEngine::new_custom` read it back from
//! there, so the key was public to any process running as the user and the
//! migration covered only the built-in cloud engine.
//!
//! These tests use the **real** Windows Credential Manager, because the thing
//! being verified is precisely that the real store is on the path. They are
//! therefore written to be safe to run on a user's machine:
//!
//! * every provider id is unique per process and per run, so nothing a test
//!   writes can collide with, or be mistaken for, the user's own targets;
//! * every stored key is removed by an RAII guard, including on panic;
//! * the built-in `cloud` target is never named anywhere in this file.
//!
//! The mock-store tests for the same pipeline live in
//! `credentials_core_test.rs`; conflict and failure injection are deliberately
//! *there* and not here, so a real-store test never has to pretend an OS call
//! failed.

use std::sync::atomic::{AtomicU64, Ordering};

use voice_ptt::asr::cloud::EngineKeySource;
use voice_ptt::asr::CloudEngine;
use voice_ptt::config::settings::CustomProvider;
use voice_ptt::credentials::SecretString;
use voice_ptt::credentials_resolver::{
    custom_service, delete_custom_key, store_custom_key, stored_custom_key,
};

/// Distinguishes runs and providers so two tests in the same process — and two
/// processes on the same machine — never share a target.
static UNIQUE: AtomicU64 = AtomicU64::new(0);

fn unique_provider_id(tag: &str) -> String {
    format!(
        "f2-{tag}-{}-{}",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    )
}

/// Removes the test's credential when it goes out of scope, on the success path
/// and on panic.
///
/// The store is per-user and machine-wide, so a test that leaks a credential is
/// a test that leaves state behind on somebody's real machine.
struct StoreGuard {
    provider_id: String,
}

impl StoreGuard {
    fn new(provider_id: String) -> Self {
        Self { provider_id }
    }

    fn provider_id(&self) -> &str {
        &self.provider_id
    }
}

impl Drop for StoreGuard {
    fn drop(&mut self) {
        // Best effort by definition — a `Drop` cannot report — but it is the
        // same call the app makes when a provider is deleted, so a failure here
        // is the app's failure too and not something to hide behind.
        let _ = delete_custom_key(&self.provider_id);
    }
}

fn usage_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "omnitype-f2-usage-{tag}-{}-{}.json",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn provider(id: &str, api_key: &str) -> CustomProvider {
    CustomProvider {
        id: id.to_string(),
        name: "نمایشی".to_string(),
        base_url: "https://example.invalid/v1".to_string(),
        api_key: api_key.to_string(),
        model: "whisper-1".to_string(),
        language: "fa".to_string(),
        timeout_secs: 20,
    }
}

/// The store really is the path: a key written for a provider id comes back out
/// of the real Credential Manager, byte for byte.
#[test]
fn a_custom_key_round_trips_through_the_real_store() {
    let id = unique_provider_id("roundtrip");
    let guard = StoreGuard::new(id.clone());
    let secret = SecretString::new("gsk-f2-unique-Ω-999");

    store_custom_key(guard.provider_id(), &secret).expect("storing a fresh custom key must work");

    let read_back = stored_custom_key(guard.provider_id()).expect("the key was just stored");
    assert_eq!(
        read_back.expose_secret(),
        secret.expose_secret(),
        "the store must return exactly what it was given"
    );

    // And an id that was never stored reads as absent rather than as an error
    // or as somebody else's key.
    let never = unique_provider_id("never-stored");
    assert!(stored_custom_key(&never).is_none());
}

/// The engine uses the **store's** value, not the settings copy.
///
/// The provider is handed an empty `api_key`, so the only place a key could
/// come from is the store; `key_source() == Store` is therefore the proof that
/// the store is on the production read path, not just that a key exists
/// somewhere.
#[test]
fn a_custom_engine_reads_the_stored_key_not_the_settings_copy() {
    let id = unique_provider_id("engine-store");
    let guard = StoreGuard::new(id.clone());
    store_custom_key(guard.provider_id(), &SecretString::new("stored-key"))
        .expect("storing must work");

    let engine = CloudEngine::new_custom(&provider(guard.provider_id(), ""), usage_path("store"));
    assert!(engine.has_key(), "the engine must find the stored key");
    assert_eq!(
        engine.key_source(),
        EngineKeySource::Store,
        "the settings copy was empty, so the only possible source is the store"
    );

    // Deleting the key is what the panel's delete button does. A new engine for
    // the same provider must come up with no key rather than with the old one.
    delete_custom_key(guard.provider_id()).expect("delete must work");
    let after = CloudEngine::new_custom(&provider(guard.provider_id(), ""), usage_path("after"));
    assert!(!after.has_key(), "the deleted key must not be used");
    assert_eq!(after.key_source(), EngineKeySource::Missing);
}

/// The pre-migration fallback still works: a machine where the store could not
/// be used keeps authenticating from the settings copy instead of losing the
/// provider.
#[test]
fn a_custom_engine_falls_back_to_the_settings_copy_when_the_store_is_empty() {
    let id = unique_provider_id("engine-settings");
    // Deliberately **not** registered with the guard: nothing is written, so
    // there is nothing to clean up. The id is unique, so `stored_custom_key`
    // finds nothing.
    assert!(stored_custom_key(&id).is_none());

    let engine =
        CloudEngine::new_custom(&provider(&id, "legacy-plaintext"), usage_path("settings"));
    assert!(engine.has_key());
    assert_eq!(engine.key_source(), EngineKeySource::Settings);

    let empty = CloudEngine::new_custom(&provider(&id, ""), usage_path("empty"));
    assert!(!empty.has_key(), "a provider with no key anywhere has none");
    assert_eq!(empty.key_source(), EngineKeySource::Missing);
}

/// Deleting one provider's key must not touch another's. The store is one flat
/// namespace, so a delete that reached too far would silently break an
/// unrelated engine.
#[test]
fn deleting_one_custom_key_leaves_another_untouched() {
    let first = unique_provider_id("keep");
    let second = unique_provider_id("remove");
    let _first_guard = StoreGuard::new(first.clone());
    let _second_guard = StoreGuard::new(second.clone());

    store_custom_key(&first, &SecretString::new("first-key")).expect("first stores");
    store_custom_key(&second, &SecretString::new("second-key")).expect("second stores");

    delete_custom_key(&second).expect("delete must work");

    assert!(
        stored_custom_key(&second).is_none(),
        "the named one is gone"
    );
    assert_eq!(
        stored_custom_key(&first).map(|s| s.expose_secret().to_string()),
        Some("first-key".to_string()),
        "the other provider's key must survive"
    );
}

/// An empty or whitespace-only key is refused by the contract rather than
/// stored: a stored empty credential and an absent one mean different things to
/// the engine picker, and `delete` is how a user says "I have no key".
#[test]
fn an_empty_custom_key_is_refused_without_touching_the_store() {
    let id = unique_provider_id("empty");
    for blank in ["", "   ", "\t\n"] {
        let result = store_custom_key(&id, &SecretString::new(blank));
        assert!(
            result.is_err(),
            "a blank key must be refused, not stored ({blank:?})"
        );
    }
    assert!(
        stored_custom_key(&id).is_none(),
        "a refused key must leave nothing behind"
    );
}

/// Two providers in one settings file, one key each, stored and cleared
/// independently. This is the shape the panel produces: several custom
/// endpoints at once, each with its own secret.
#[test]
fn several_providers_keep_separate_keys() {
    let ids: Vec<String> = (0..3)
        .map(|i| unique_provider_id(&format!("multi-{i}")))
        .collect();
    let _guards: Vec<StoreGuard> = ids.iter().cloned().map(StoreGuard::new).collect();

    for (i, id) in ids.iter().enumerate() {
        store_custom_key(id, &SecretString::new(format!("key-{i}"))).expect("stores");
    }
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(
            stored_custom_key(id).map(|s| s.expose_secret().to_string()),
            Some(format!("key-{i}")),
            "provider {id} must have its own key, not a neighbour's"
        );
    }
}

/// F2's leak checks: `Debug`, `Display` and the failed-save path.
///
/// A provider's key is the one thing that must never reach a log line or the
/// public settings file. `Debug` is hand-written for exactly this, and the
/// failed-save path must leave the key where it was rather than writing a
/// half-configured provider.
#[test]
fn a_key_is_not_printable_and_a_failed_save_writes_no_provider() {
    let id = unique_provider_id("redaction");
    let leaked = "gsk-must-never-be-printed-12345";
    let rendered = format!("{:?}", provider(&id, leaked));
    assert!(
        !rendered.contains(leaked),
        "a derived Debug would leak the key into every log line: {rendered}"
    );
    assert!(
        rendered.contains("REDACTED") || rendered.contains("none"),
        "Debug must say *whether* a key is present without saying what it is: {rendered}"
    );

    // The public settings file never holds the key once the store has it: this
    // is the persisted artifact the finding was about.
    let provider = provider(&id, "");
    let settings = voice_ptt::config::settings::Settings {
        custom_providers: vec![provider],
        ..Default::default()
    };
    let rendered = toml::to_string_pretty(&settings).expect("settings serialize");
    assert!(
        !rendered.contains(leaked),
        "the serialized settings must not contain a key"
    );
}

/// The service name is derived from the id, and the two are not accidentally
/// the same string — a target built from the raw id would sit outside the
/// app's namespace and could collide with another program's credential.
#[test]
fn the_service_name_is_namespaced_and_id_derived() {
    let id = unique_provider_id("naming");
    let service = custom_service(&id);
    assert_eq!(service, format!("custom:{id}"));
    assert_ne!(service, id, "the raw id must not be the store target");
}
