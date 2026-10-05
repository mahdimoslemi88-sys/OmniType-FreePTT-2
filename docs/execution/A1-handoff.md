# تحویل `A1` (مرحلهٔ نخست) — قرارداد هستهٔ اعتبارنامه‌ها، مخزن ساختگی و خط لولهٔ مهاجرت امن

**شناسنامه تحویل:**
- **بسته:** `A1` · **موج:** ۲ · **مرحله:** نخست (Core Contract, Mock Store & Migration Engine)
- **تاریخ:** ۲۰۲۶-۱۰-۰۴ · **مالک:** ایجنت طراح بستهٔ A1
- **مبنای ورودی:** [A1-DESIGN.md](A1-DESIGN.md)، [CONTRACTS.md](CONTRACTS.md) بند ۸، [AGENT-EXECUTION-PLAN.md](../AGENT-EXECUTION-PLAN.md)
- **وضعیت:** **آمادهٔ ادغام (تأییدشده در سطح هسته و تست ایزوله)** — ۱۱/۱۱ آزمون اختصاصی پاس، صفر هشدار، قالب‌بندی کاملاً منطبق بر `rustfmt`. بدون تغییر فایل‌های مرکزی یا دستکاری اطلاعات کاربر.

---

## ۱. آنچه ساخته شد و اصلاحات معماری

منطبق بر دستورالعمل‌های تکمیلی، مرحلهٔ نخست بستهٔ A1 بدون دستکاری فایل‌های مرکزی یا اتصال زنده به سیستم‌عامل، در قالب ماژول مستقل و تست‌پذیر تولید شد:

### ۱.۱. محفظهٔ امن کلید محرمانه (`SecretString`) در [`credentials/mod.rs`](../../voice-ptt/src/credentials/mod.rs)
- کپسوله‌سازی رشته در ساختار `SecretString`.
- پیاده‌سازی سفارشی [`fmt::Debug`] و [`fmt::Display`] که مقدار خروجی را به طور قطعی با `[REDACTED_SECRET]` جایگزین می‌کنند.
- کنترل دسترسی صریح به متن محرمانه صرفاً از طریق متد `.expose_secret()`.
- پیاده‌سازی [`Drop`] با بازنویسی بایت‌های حافظه با مقدار صفر به صورت تلاش بهینه (Best-effort)، **بدون ادعای ناممکن ریشه‌کن شدن تمام نسخه‌های پیشین کلید از حافظه** (به دلیل جابه‌جایی‌های تخصیص‌دهندهٔ heap یا paging سیستم‌عامل).

### ۱.۲. خطاهای نوع‌دار (`CredentialError`) و قرارداد مخزن (`CredentialStore`)
- تفکیک قطعی میان «کلید یافت نشد» (`NotFound`) با شکست‌های سیستمی (`AccessDenied`, `CorruptedData`, `Unavailable`, `OsError`, `EmptySecret`).
- تعریف Trait مستقل از پلتفرم `CredentialStore` با متدهای `save`, `load`, `delete`.
- تفکیک صریح عملیات به‌روزرسانی ارادی کاربر (`explicit_update_credential`) و حذف ارادی (`explicit_delete_credential`) از خط لولهٔ مهاجرت خودکار.

### ۱.۳. مخزن ساختگی با تزریق خطا (`MockCredentialStore`) در [`credentials/mock.rs`](../../voice-ptt/src/credentials/mock.rs)
- پیاده‌سازی مخزن درون‌حافظه‌ای امن با `RwLock<HashMap<String, String>>`.
- قابلیت تزریق انواع خطاهای دسترسی، قطعی سرویس و مقدار جایگزین در بازخوانی (`set_save_error`, `set_load_error`, `set_delete_error`, `set_post_save_load_override`).

### ۱.۴. موتور مهاجرت مستقل و درگاه ثبت تنظیمات (`SettingsPersister`) در [`credentials/migration.rs`](../../voice-ptt/src/credentials/migration.rs)
- استقلال ۱۰۰٪ از فایل‌های دیسک، فرمت TOML یا API سیستم‌عامل.
- ورودی و خروجی نوع‌دار (`MigrationItem`, `MigrationOutcome`, `MigrationAction`, `MigrationError`, `MigrationConflict`).
- **مهار تعارض در مهاجرت خودکار:** استعلام پیشینی از مخزن انجام می‌شود؛ اگر کلیدی با مقدار **متفاوت** در مخزن وجود داشته باشد، مهاجرت خودکار از بازنویسی کورکورانه خودداری کرده، خطای `Conflict` را گزارش می‌دهد و **کلید متنی تنظیمات را دست‌نخورده حفظ می‌کند**.
- **درگاه ثبت تنظیمات (`SettingsPersister`):** با توجه به اینکه متد فعلی `Settings::save` با `std::fs::write` اتمیک نبوده و تضمین‌کنندهٔ حفظ فایل قبلی در قطعی ناگهانی نیست، این درگاه قرارداد ثبت امن را تصریح می‌کند.
- **قاعدهٔ عدم از دست رفتن داده (Zero Data Loss):** پاک‌سازی کلید از تنظیمات **فقط و فقط** پس از موفقیت قطعی ذخیره، بازخوانی و تطبیق بایت‌به‌بایت مقادیر مجاز است. در صورت شکست ثبت درگاه تنظیمات، مهاجرت ناموفق گزارش شده و کلید در تنظیمات حذف‌نشده تلقی می‌گردد.

---

## ۲. نتایج اندازه‌گیری‌شدهٔ آزمون‌ها و بررسی قالب‌بندی

### ۲.۱. اجرای آزمون اختصاصی هسته (`credentials_core_test`)
یک مجموعه آزمون جامع در [`tests/credentials_core_test.rs`](../../voice-ptt/tests/credentials_core_test.rs) با مهار ایزوله و بدون نیاز به ثبت در `lib.rs` پیاده‌سازی و اجرا شد:

```text
running 11 tests
test test_save_error_aborts_migration_without_clearing_settings ... ok
test test_readback_error_aborts_migration_without_clearing_settings ... ok
test test_readback_mismatch_aborts_migration_without_clearing_settings ... ok
test test_settings_persist_failure_leaves_outcome_failed ... ok
test test_repeated_migration_is_idempotent ... ok
test test_conflict_refuses_blind_overwrite_and_preserves_settings ... ok
test test_explicit_update_and_delete_operations ... ok
test test_empty_and_whitespace_secrets ... ok
test test_no_secret_leakage_in_debug_display_and_errors ... ok
test test_mock_store_crud_and_error_injection ... ok
test test_batch_migration_handles_mixed_results ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### ۲.۲. بررسی قالب‌بندی و هشدارها
- اجرای `rustfmt --check`: **کاملاً پاک (Exit code 0)**.
- اجرای `cargo test`: **صفر هشدار کامپایلر (Zero warnings)**.

---

## ۳. فایل‌های تغییریافته و مرزهای مالکیت

### فایل‌های تحت مالکیت و تولیدشدهٔ بستهٔ A1:
1. [`voice-ptt/src/credentials/mod.rs`](../../voice-ptt/src/credentials/mod.rs) — انواع مشترک، Trait مخزن و کپسوله‌سازی SecretString
2. [`voice-ptt/src/credentials/mock.rs`](../../voice-ptt/src/credentials/mock.rs) — مخزن ساختگی MockCredentialStore با تزریق خطا
3. [`voice-ptt/src/credentials/migration.rs`](../../voice-ptt/src/credentials/migration.rs) — موتور مهاجرت نوع‌دار و درگاه SettingsPersister
4. [`voice-ptt/tests/credentials_core_test.rs`](../../voice-ptt/tests/credentials_core_test.rs) — ۱۱ آزمون رفتاری جامع
5. [`docs/execution/A1-DESIGN.md`](A1-DESIGN.md) — به‌روزرسانی نکات غیراتمیک بودن Settings::save، اصلاح ادعای Drop و مهار تعارض مهاجرت
6. [`docs/execution/A1-handoff.md`](A1-handoff.md) — همین گزارش تحویل

### فایل‌های مرکزی که دست‌نخورده باقی ماندند:
- `voice-ptt/src/lib.rs` (دست‌نخورده)
- `voice-ptt/Cargo.toml` (دست‌نخورده)
- `voice-ptt/src/config/settings.rs` (دست‌نخورده)
- `voice-ptt/src/asr/cloud.rs` (دست‌نخورده)
- `voice-ptt/src/gui/overlay/engine_panel.rs` (دست‌نخورده)
- `voice-ptt/src/doctor.rs` (دست‌نخورده)

---

## ۴. درخواست‌های اتصال برای هماهنگ‌کننده (Integration Requests)

برای اتصال نهایی بستهٔ A1 در مرحلهٔ ادغام I2، هماهنگ‌کننده تنها تغییرات زیر را اعمال خواهد نمود:

### ۴.۱. ثبت ماژول در `voice-ptt/src/lib.rs`
افزودن خط زیر به بخش تعریف ماژول‌ها:
```rust
pub mod credentials;
```

### ۴.۲. فعال‌سازی فیچر در `voice-ptt/Cargo.toml`
افزودن فیچر `"Win32_Security_Credentials"` به بخش وابستگی ویندوز برای پیاده‌سازی بومی مرحلهٔ بعد:
```toml
windows = { version = "0.58", features = [
    # ...
    "Win32_Security",
    "Win32_Security_Credentials",
    # ...
] }
```

### ۴.۳. پیاده‌سازی بتنی `SettingsPersister` روی فایل تنظیمات
پیاده‌سازی یک نگه‌دارندهٔ ذخیرهٔ امن با الگوی نوشتن اتمیک (Write temp file then rename):
```rust
pub struct DiskSettingsPersister {
    pub config_path: std::path::PathBuf,
    pub settings: std::sync::Arc<std::sync::RwLock<crate::config::Settings>>,
}

impl crate::credentials::SettingsPersister for DiskSettingsPersister {
    fn persist_cleared_key(&self, target: &str) -> Result<(), crate::credentials::PersistError> {
        // ۱. پاک‌سازی کلید در شیء Settings در حافظه
        // ۲. نوشتن امن در فایل موقت و تعویض اتمیک نام فایل
        // ۳. در صورت بروز خطا، بازگرداندن PersistError
        Ok(())
    }
}
```

---

## ۵. محدودیت‌های باقی‌مانده و مراحل بعد

1. **پیاده‌سازی درایور بومی ویندوز (`WindowsCredentialStore`):** در مرحلهٔ دوم با هماهنگی هماهنگ‌کننده و پس از فعال‌سازی فیچر Cargo انجام خواهد شد.
2. **اتصال رابط کاربری و موتور ابری:** در مرحله ادغام I2 انجام خواهد گرفت.
3. **آزمون مخزن واقعی سیستم‌عامل:** بر اساس پروتکل موقت مشخص‌شده در طرح A1 و پس از پیاده‌سازی درایور بومی اجرا خواهد شد.
