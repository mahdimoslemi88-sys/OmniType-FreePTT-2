//! آزمون‌های جامع هستهٔ اعتبارنامه‌ها، مخزن ساختگی و خط لولهٔ مهاجرت (بستهٔ A1)

#[path = "../src/credentials/mod.rs"]
mod credentials;

use std::sync::RwLock;

use credentials::{
    explicit_delete_credential, explicit_update_credential, migrate_credential,
    migrate_credentials, CredentialError, CredentialStore, MigrationAction, MigrationError,
    MigrationItem, MockCredentialStore, PersistError, SecretString, SettingsPersister,
};

/// پیاده‌سازی ساختگی درگاه ثبت تنظیمات جهت آزمودن سناریوهای موفقیت و شکست دیسک.
struct MockSettingsPersister {
    cleared: RwLock<Vec<String>>,
    fail_error: RwLock<Option<String>>,
}

impl MockSettingsPersister {
    fn new() -> Self {
        Self {
            cleared: RwLock::new(Vec::new()),
            fail_error: RwLock::new(None),
        }
    }

    fn set_fail_error(&self, err: Option<&str>) {
        *self.fail_error.write().unwrap() = err.map(|s| s.to_string());
    }

    fn is_cleared(&self, target: &str) -> bool {
        self.cleared.read().unwrap().contains(&target.to_string())
    }

    fn cleared_count(&self) -> usize {
        self.cleared.read().unwrap().len()
    }
}

impl SettingsPersister for MockSettingsPersister {
    fn persist_cleared_key(&self, target: &str) -> Result<(), PersistError> {
        if let Some(ref err) = *self.fail_error.read().unwrap() {
            return Err(PersistError {
                target: target.to_string(),
                message: err.clone(),
            });
        }
        self.cleared.write().unwrap().push(target.to_string());
        Ok(())
    }
}

// ── ۱. آزمون خطای ذخیره در مخزن ──
#[test]
fn test_save_error_aborts_migration_without_clearing_settings() {
    let store = MockCredentialStore::new();
    let persister = MockSettingsPersister::new();

    // تزریق خطای عدم دسترسی هنگام ذخیره در مخزن
    store.set_save_error(Some(CredentialError::AccessDenied(
        "OmniType:asr:cloud".into(),
    )));

    let item = MigrationItem::new("OmniType:asr:cloud", "sk-secret-legacy-123");
    let outcome = migrate_credential(&store, &persister, &item);

    assert!(!outcome.is_success());
    match outcome.result {
        Err(MigrationError::StoreSaveFailed { target, error }) => {
            assert_eq!(target, "OmniType:asr:cloud");
            assert!(matches!(error, CredentialError::AccessDenied(_)));
        }
        other => panic!("expected StoreSaveFailed, got: {other:?}"),
    }

    // شرط حیاتی: تنظیمات نباید پاک شده باشند
    assert!(!persister.is_cleared("OmniType:asr:cloud"));
    assert_eq!(persister.cleared_count(), 0);
}

// ── ۲. آزمون خطای بازخوانی از مخزن در استعلام اولیه ──
#[test]
fn test_readback_error_aborts_migration_without_clearing_settings() {
    let store = MockCredentialStore::new();
    let persister = MockSettingsPersister::new();

    // استعلام اولیه خطای عدم دسترسی یا قطعی سرویس می‌دهد
    store.set_load_error(Some(CredentialError::Unavailable(
        "service suspended".into(),
    )));

    let item = MigrationItem::new("OmniType:asr:cloud", "sk-secret-legacy-123");
    let outcome = migrate_credential(&store, &persister, &item);

    assert!(!outcome.is_success());
    match outcome.result {
        Err(MigrationError::StoreProbeFailed { target, error }) => {
            assert_eq!(target, "OmniType:asr:cloud");
            assert!(matches!(error, CredentialError::Unavailable(_)));
        }
        other => panic!("expected StoreProbeFailed, got: {other:?}"),
    }

    // شرط حیاتی: تنظیمات پاک نمی‌شوند
    assert!(!persister.is_cleared("OmniType:asr:cloud"));
}

// ── ۳. آزمون عدم تطابق داده در بازخوانی (Mismatch) پس از ذخیره ──
#[test]
fn test_readback_mismatch_aborts_migration_without_clearing_settings() {
    let store = MockCredentialStore::new();
    let persister = MockSettingsPersister::new();

    // استعلام اولیه کلید را نمی‌یابد (NotFound)، ذخیره انجام می‌شود، اما در مرحله ۴ بازخوانی داده دستکاری‌شده برمی‌گرداند
    store.set_post_save_load_override(Some("corrupted_different_key".into()));

    let item = MigrationItem::new("OmniType:asr:cloud", "sk-secret-legacy-123");
    let outcome = migrate_credential(&store, &persister, &item);

    assert!(!outcome.is_success());
    match outcome.result {
        Err(MigrationError::ReadBackMismatch { target }) => {
            assert_eq!(target, "OmniType:asr:cloud");
        }
        other => panic!("expected ReadBackMismatch, got: {other:?}"),
    }

    // تنظیمات به هیچ عنوان پاک نمی‌شود
    assert!(!persister.is_cleared("OmniType:asr:cloud"));
}

// ── ۴. آزمون شکست ثبت تنظیمات ──
#[test]
fn test_settings_persist_failure_leaves_outcome_failed() {
    let store = MockCredentialStore::new();
    let persister = MockSettingsPersister::new();

    // ذخیره در مخزن و بازخوانی موفقیت‌آمیز است، اما ثبت تنظیمات خطا می‌دهد (مثلاً دیسک پر است)
    persister.set_fail_error(Some("disk full or permission denied"));

    let item = MigrationItem::new("OmniType:asr:cloud", "sk-secret-legacy-123");
    let outcome = migrate_credential(&store, &persister, &item);

    assert!(!outcome.is_success());
    match outcome.result {
        Err(MigrationError::SettingsPersistFailed { target, error }) => {
            assert_eq!(target, "OmniType:asr:cloud");
            assert!(error.message.contains("disk full"));
        }
        other => panic!("expected SettingsPersistFailed, got: {other:?}"),
    }

    assert!(!persister.is_cleared("OmniType:asr:cloud"));
}

// ── ۵. آزمون تکرارپذیری مهاجرت (Idempotency) ──
#[test]
fn test_repeated_migration_is_idempotent() {
    let store = MockCredentialStore::new();
    let persister = MockSettingsPersister::new();

    let item = MigrationItem::new("OmniType:asr:cloud", "sk-secret-legacy-123");

    // اجرای نوبت اول: ذخیره و تأیید موفق
    let outcome1 = migrate_credential(&store, &persister, &item);
    assert_eq!(outcome1.result.unwrap(), MigrationAction::MigratedFresh);
    assert!(persister.is_cleared("OmniType:asr:cloud"));
    assert_eq!(
        store.raw_get("OmniType:asr:cloud").as_deref(),
        Some("sk-secret-legacy-123")
    );

    // اجرای نوبت دوم: فرض کنید تنظیمات در اجرای بعدی خالی شده است
    let clean_item = MigrationItem::new("OmniType:asr:cloud", "");
    let outcome2 = migrate_credential(&store, &persister, &clean_item);
    assert_eq!(outcome2.result.unwrap(), MigrationAction::SkippedEmpty);

    // اجرای نوبت سوم: سناریوی قطع برنامه مابین ذخیره و ثبت دیسک
    // کلید با همان مقدار در مخزن هست و دوباره مهاجرت فراخوانی می‌شود
    let outcome3 = migrate_credential(&store, &persister, &item);
    assert_eq!(
        outcome3.result.unwrap(),
        MigrationAction::MigratedAlreadyPresent
    );
    assert_eq!(
        store.raw_get("OmniType:asr:cloud").as_deref(),
        Some("sk-secret-legacy-123")
    );
}

// ── ۶. آزمون تعارض رکورد و منع بازنویسی خودکار ──
#[test]
fn test_conflict_refuses_blind_overwrite_and_preserves_settings() {
    // مخزن از قبل کلید متفاوتی را در خود دارد
    let store = MockCredentialStore::new()
        .with_credential("OmniType:asr:cloud", "existing-secret-in-store-abc");
    let persister = MockSettingsPersister::new();

    // کلید موجود در تنظیمات متفاوت است
    let item = MigrationItem::new("OmniType:asr:cloud", "different-secret-in-config-xyz");
    let outcome = migrate_credential(&store, &persister, &item);

    assert!(!outcome.is_success());
    match outcome.result {
        Err(MigrationError::Conflict(conflict)) => {
            assert_eq!(conflict.target, "OmniType:asr:cloud");
            assert!(conflict.message.contains("refusing blind overwrite"));
        }
        other => panic!("expected MigrationError::Conflict, got: {other:?}"),
    }

    // شرط حیاتی بند ۴: مقدار موجود در مخزن دست‌نخورده باقی می‌ماند
    assert_eq!(
        store.raw_get("OmniType:asr:cloud").as_deref(),
        Some("existing-secret-in-store-abc")
    );

    // کلید از تنظیمات پاک نمی‌شود تا داده از بین نرود
    assert!(!persister.is_cleared("OmniType:asr:cloud"));
}

// ── ۷. آزمون تفکیک به‌روزرسانی و حذف صریح از مهاجرت خودکار ──
#[test]
fn test_explicit_update_and_delete_operations() {
    let store = MockCredentialStore::new().with_credential("OmniType:asr:cloud", "initial-secret");

    assert!(store.contains("OmniType:asr:cloud"));

    // به‌روزرسانی صریح
    let new_secret = SecretString::new("user-updated-explicit-secret");
    let result = explicit_update_credential(&store, "OmniType:asr:cloud", &new_secret);
    assert!(result.is_ok());
    assert_eq!(
        store.raw_get("OmniType:asr:cloud").as_deref(),
        Some("user-updated-explicit-secret")
    );

    // حذف صریح
    let del_res = explicit_delete_credential(&store, "OmniType:asr:cloud");
    assert!(del_res.is_ok());
    assert!(!store.contains("OmniType:asr:cloud"));

    // حذف مجدد باید NotFound بدهد
    let del_res2 = explicit_delete_credential(&store, "OmniType:asr:cloud");
    assert!(matches!(del_res2, Err(CredentialError::NotFound(_))));
}

// ── ۸. آزمون مهار مقادیر خالی و فاصله‌ای ──
#[test]
fn test_empty_and_whitespace_secrets() {
    let store = MockCredentialStore::new();
    let persister = MockSettingsPersister::new();

    // مهاجرت رشتهٔ خالی
    let empty_item = MigrationItem::new("test:empty", "");
    let outcome1 = migrate_credential(&store, &persister, &empty_item);
    assert_eq!(outcome1.result.unwrap(), MigrationAction::SkippedEmpty);

    // مهاجرت رشتهٔ فقط فاصله
    let ws_item = MigrationItem::new("test:ws", "   \t\n  ");
    let outcome2 = migrate_credential(&store, &persister, &ws_item);
    assert_eq!(outcome2.result.unwrap(), MigrationAction::SkippedEmpty);

    // به‌روزرسانی صریح با رشتهٔ خالی باید خطای صریح EmptySecret بدهد
    let err1 = explicit_update_credential(&store, "test:target", &SecretString::new(""));
    assert_eq!(err1.unwrap_err(), CredentialError::EmptySecret);

    let err2 = explicit_update_credential(&store, "test:target", &SecretString::new("   "));
    assert_eq!(err2.unwrap_err(), CredentialError::EmptySecret);
}

// ── ۹. آزمون عدم افشای مقدار محرمانه در Debug و Display و پیام‌های خطا ──
#[test]
fn test_no_secret_leakage_in_debug_display_and_errors() {
    let raw = "sk-super-secret-production-token-999888";
    let secret = SecretString::new(raw);

    // بررسی Debug
    let debug_str = format!("{secret:?}");
    assert!(
        !debug_str.contains(raw),
        "secret leaked in Debug: {debug_str}"
    );
    assert_eq!(debug_str, "[REDACTED_SECRET]");

    // بررسی Display
    let display_str = format!("{secret}");
    assert!(
        !display_str.contains(raw),
        "secret leaked in Display: {display_str}"
    );
    assert_eq!(display_str, "[REDACTED_SECRET]");

    // بررسی خطاهای CredentialError و عدم افشای مقدار
    let errors = [
        CredentialError::NotFound("target_1".into()),
        CredentialError::AccessDenied("target_2".into()),
        CredentialError::CorruptedData("target_3".into()),
        CredentialError::Unavailable("service down".into()),
        CredentialError::OsError {
            code: 5,
            message: "access denied".into(),
        },
        CredentialError::EmptySecret,
    ];
    for e in &errors {
        let text = format!("{e}");
        assert!(!text.contains(raw));
    }

    // بررسی خطای MigrationError
    let err = MigrationError::StoreSaveFailed {
        target: "OmniType:asr:groq".into(),
        error: CredentialError::AccessDenied("OmniType:asr:groq".into()),
    };
    let err_str = format!("{err}");
    assert!(!err_str.contains(raw));
    assert!(err_str.contains("OmniType:asr:groq"));

    // استخراج عمدی متن محرمانه صرفاً از متد کنترل‌شدهٔ expose_secret میسر است
    assert_eq!(secret.expose_secret(), raw);
}

// ── ۱۰. آزمون تمایز NotFound از شکست دسترسی در مخزن و تزریق خطای حذف ──
#[test]
fn test_mock_store_crud_and_error_injection() {
    let store = MockCredentialStore::new();

    // کلید موجود نیست
    let missing = store.load("non_existent_key");
    match missing {
        Err(CredentialError::NotFound(target)) => assert_eq!(target, "non_existent_key"),
        other => panic!("expected NotFound, got: {other:?}"),
    }

    // ذخیره
    let res = store.save("test_key", &SecretString::new("secret_val"));
    assert!(res.is_ok());

    // خواندن موفق
    let loaded = store.load("test_key").unwrap();
    assert_eq!(loaded.expose_secret(), "secret_val");

    // تزریق خطای حذف
    store.set_delete_error(Some(CredentialError::AccessDenied("test_key".into())));
    assert!(matches!(
        store.delete("test_key"),
        Err(CredentialError::AccessDenied(_))
    ));
    store.set_delete_error(None);

    // حذف موفق
    assert!(store.delete("test_key").is_ok());

    // پس از حذف باید NotFound بدهد
    let after_delete = store.load("test_key");
    assert!(matches!(after_delete, Err(CredentialError::NotFound(_))));
}

// ── ۱۱. آزمون مهاجرت دسته‌ای ──
#[test]
fn test_batch_migration_handles_mixed_results() {
    let store = MockCredentialStore::new().with_credential("conflict_target", "store_val");
    let persister = MockSettingsPersister::new();

    let items = vec![
        MigrationItem::new("fresh_target", "fresh_val"),
        MigrationItem::new("conflict_target", "different_legacy_val"),
        MigrationItem::new("empty_target", ""),
    ];

    let outcomes = migrate_credentials(&store, &persister, &items);
    assert_eq!(outcomes.len(), 3);

    assert_eq!(
        outcomes[0].result.as_ref().unwrap(),
        &MigrationAction::MigratedFresh
    );
    assert!(matches!(
        outcomes[1].result.as_ref().unwrap_err(),
        MigrationError::Conflict(_)
    ));
    assert_eq!(
        outcomes[2].result.as_ref().unwrap(),
        &MigrationAction::SkippedEmpty
    );

    // فقط fresh_target پاک شده است
    assert!(persister.is_cleared("fresh_target"));
    assert!(!persister.is_cleared("conflict_target"));
    assert!(!persister.is_cleared("empty_target"));
}

// ──────────────────────────── مخزن واقعی ویندوز ────────────────────────────

/// نامِ موقتِ آزمون. **هرگز** یک هدف واقعی نوشته نمی‌شود: اگر آزمونی روی هدفِ
/// کاربر بخورد و شکست بخورد، کلید کاربر از دست می‌رود — و هیچ آزمونی نباید
/// بتواند چنین کندی.
///
/// با یک شناسهٔ یکتا ساخته می‌شود تا دو اجرای هم‌زمان روی یک ماشین همدیگر را
/// پاک نکنند.
fn temp_target(label: &str) -> String {
    format!(
        "OmniTypeFreePTT/test/{label}/{}",
        std::process::id()
    )
}

/// آزمونِ زندهٔ مخزن واقعی: `#[ignore]` است، چون روی «مخزن اعتبارنامهٔ کاربرِ
/// واقعی» می‌نویسد و پاک می‌کند.
///
/// عمداً پیش‌فرض نیست. اجرای خودکارش در هر `cargo test` یعنی هر بار که کسی
/// کد را می‌سازد، یک ورودی در vault سیستم ساخته و پاک می‌شود — و اگر اجرا
/// وسط کار قطع شود، ورودی می‌ماند. یک آزمون واقعی که فقط با درخواست اجرا
/// می‌شود، این را به انتخابِ کسی که واقعاً دنبالش است تبدیل می‌کند.
#[test]
#[ignore = "writes to the real Windows Credential Manager; run deliberately"]
fn the_real_windows_store_round_trips_a_secret() {
    use credentials::windows::target_for;
    use credentials::WindowsCredentialStore;

    let store = WindowsCredentialStore::new();
    let target = temp_target("roundtrip");

    // پاک‌سازی هر چیزی که از اجرای قبلی مانده، تا آزمون به دادهٔ خودش بخورد.
    let _ = store.delete(&target);

    let secret = SecretString::new("sk-not-a-real-key-0123456789");
    store.save(&target, &secret).expect("save must succeed");

    let loaded = store.load(&target).expect("load must succeed");
    assert_eq!(
        loaded.expose_secret(),
        secret.expose_secret(),
        "the key must come back byte-for-byte"
    );

    store.delete(&target).expect("delete must succeed");
    assert!(
        matches!(store.load(&target), Err(CredentialError::NotFound(_))),
        "a deleted key must read as absent, not as empty"
    );

    // هدف واقعی برنامه، برای اینکه نام‌گذاریِ آن قفل شود.
    assert_eq!(target_for("cloud"), "OmniTypeFreePTT/cloud");
}

/// غیبتِ کلید باید «پیدا نشد» باشد، نه «خالی» — چون موتورها این دو را یکی
/// نمی‌گیرند و یکی از آن‌ها کاربر را دنبال اشتباهی می‌فرستد.
#[test]
#[ignore = "reads the real Windows Credential Manager; run deliberately"]
fn an_absent_key_reads_as_not_found() {
    use credentials::WindowsCredentialStore;

    let store = WindowsCredentialStore::new();
    let target = temp_target("absent");
    let _ = store.delete(&target);

    match store.load(&target) {
        Err(CredentialError::NotFound(name)) => assert_eq!(name, target),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

/// پاک‌سازی، حتی وقتی آزمون وسط کار شکست بخورد.
///
/// بدون این، یک `assert!` ناموفق در بالا ورودیِ آزمون را در vault کاربر جا
/// می‌گذارد — و آن‌وقت هر اجرای بعدی به دادهٔ اجرای شکست‌خورده برمی‌خورد.
#[test]
#[ignore = "writes to the real Windows Credential Manager; run deliberately"]
fn the_temp_target_is_left_behind_nothing() {
    use credentials::WindowsCredentialStore;

    let store = WindowsCredentialStore::new();
    let target = temp_target("cleanup");

    struct Cleanup<'a>(&'a WindowsCredentialStore, String);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = self.0.delete(&self.1);
        }
    }
    let _guard = Cleanup(&store, target.clone());

    store
        .save(&target, &SecretString::new("temporary"))
        .expect("save must succeed");
    drop(_guard);

    assert!(
        matches!(store.load(&target), Err(CredentialError::NotFound(_))),
        "cleanup must leave the vault as it found it"
    );
}
