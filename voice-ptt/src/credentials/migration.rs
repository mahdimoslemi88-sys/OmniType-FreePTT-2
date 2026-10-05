//! موتور مهاجرت مستقل از فایل و سیستم‌عامل برای انتقال کلیدها به مخزن اعتبارنامه
//!
//! این ماژول منطق خالص مهاجرت مرحله‌ای را پیاده‌سازی می‌کند. هیچ وابستگی مستقیمی
//! به مسیرهای دیسک، فرمت TOML یا APIهای سیستم‌عامل ندارد و کلیهٔ اثرات جانبی از
//! طریق درگاه‌های [`CredentialStore`] و [`SettingsPersister`] تزریق می‌شوند.

use std::fmt;

use super::{CredentialError, CredentialStore, SecretString};

/// درگاه قابل‌آزمون برای پاک‌سازی کلیدهای مهاجرت‌یافته از تنظیمات و ثبت پایدار.
///
/// ### قرارداد ثبت امن (Safe Persistence Contract):
/// پیاده‌کنندهٔ این درگاه موظف است:
/// ۱. **عدم تخریب در صورت شکست:** تضمین کند که در صورت بروز خطا در ذخیره (مانند پر شدن دیسک یا لغو سشن)،
///    مقدار پیشین فایل یا سند تنظیمات سالم باقی بماند (مثلاً از طریق الگوی Write-to-temp-then-atomic-rename).
///    نوشتن مستقیم با `std::fs::write` در متد فعلی `Settings::save` این تضمین را فراهم نمی‌کند و
///    پیاده‌سازی امن دیسک خارج از این مرحله خواهد بود.
/// ۲. **گزارش صادقانهٔ موفقیت:** تنها زمانی مقدار `Ok(())` را بازگرداند که تغییر روی ذخیره‌ساز پایدار نوشته شده باشد.
/// ۳. **گزارش شکست:** در صورت عدم موفقیت، خطا را گزارش کند تا موتور مهاجرت بداند کلید از تنظیمات پاک نشده است.
pub trait SettingsPersister: Send + Sync {
    /// پاک‌سازی کلید متنی قدیمی برای شناسهٔ مشخص و ثبت پایدار آن در تنظیمات.
    fn persist_cleared_key(&self, target: &str) -> Result<(), PersistError>;
}

/// خطای عملیات درگاه ثبت تنظیمات.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistError {
    pub target: String,
    pub message: String,
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to persist cleared key for target '{}': {}",
            self.target, self.message
        )
    }
}

impl std::error::Error for PersistError {}

/// ورودی نوع‌دار برای یک فقره کلید متقاضی مهاجرت.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationItem {
    /// شناسهٔ کلید در مخزن اعتبارنامه (مانند `"OmniType:asr:cloud"` یا `"OmniType:asr:custom:groq"`).
    pub target: String,
    /// مقدار کلید قدیمی موجود در تنظیمات.
    pub legacy_secret: SecretString,
}

impl MigrationItem {
    pub fn new(target: impl Into<String>, legacy_secret: impl Into<SecretString>) -> Self {
        Self {
            target: target.into(),
            legacy_secret: legacy_secret.into(),
        }
    }
}

/// نتیجهٔ عملیات اقدام مهاجرت در صورت موفقیت.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationAction {
    /// کلید قدیمی خالی بود و هیچ رکوردی لمس نشد.
    SkippedEmpty,
    /// کلید قبلاً در مخزن نبود؛ با موفقیت ذخیره، بازخوانی تأیید و از تنظیمات پاک شد.
    MigratedFresh,
    /// کلید با همین مقدار پیش‌تر در مخزن نشسته بود (مثلاً اجرای قطع‌شده)؛ بازخوانی تأیید و از تنظیمات پاک شد.
    MigratedAlreadyPresent,
}

/// گزارش تعارض در مهاجرت خودکار هنگام مواجهه با رکورد متفاوت در مخزن.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationConflict {
    pub target: String,
    pub message: String,
}

/// خطاهای نوع‌دار چرخهٔ مهاجرت.
///
/// **نکتهٔ امنیتی:** در هیچ‌یک از پیام‌های نمایشی یا قالب‌بندی خطا، مقدار متن محرمانه چاپ نمی‌شود.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    /// شکست در استعلام اولیهٔ وضعیت مخزن (خطای غیر از NotFound).
    StoreProbeFailed {
        target: String,
        error: CredentialError,
    },
    /// شکست در مرحلهٔ ذخیرهٔ کلید در مخزن.
    StoreSaveFailed {
        target: String,
        error: CredentialError,
    },
    /// شکست در بازخوانی کلید ذخیره‌شده جهت اعتبارسنجی.
    StoreReadBackFailed {
        target: String,
        error: CredentialError,
    },
    /// عدم تطابق بایت‌به‌بایت دادهٔ بازخوانی‌شده با کلید اولیه.
    ReadBackMismatch { target: String },
    /// تعارض: مخزن اعتبارنامه از پیش دارای کلیدی با مقدار **متفاوت** است؛
    /// جهت جلوگیری از تخریب ناخواسته داده، مهاجرت خودکار از بازنویسی خودداری کرد.
    Conflict(MigrationConflict),
    /// شکست در پاک‌سازی و ذخیرهٔ تنظیمات؛ کلید قدیمی در تنظیمات حفظ شد.
    SettingsPersistFailed { target: String, error: PersistError },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreProbeFailed { target, error } => {
                write!(f, "store probe failed for '{target}': {error}")
            }
            Self::StoreSaveFailed { target, error } => {
                write!(f, "store save failed for '{target}': {error}")
            }
            Self::StoreReadBackFailed { target, error } => {
                write!(f, "read-back verification failed for '{target}': {error}")
            }
            Self::ReadBackMismatch { target } => {
                write!(
                    f,
                    "read-back verification mismatch for '{target}' (payloads did not match)"
                )
            }
            Self::Conflict(c) => {
                write!(f, "migration conflict for '{}': {}", c.target, c.message)
            }
            Self::SettingsPersistFailed { target, error } => {
                write!(f, "settings persist failed for '{target}': {error}")
            }
        }
    }
}

impl std::error::Error for MigrationError {}

/// خروجی کل مهاجرت برای یک فقره کلید.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub target: String,
    pub result: Result<MigrationAction, MigrationError>,
}

impl MigrationOutcome {
    /// آیا عملیات برای این کلید موفقیت‌آمیز بوده است؟
    pub fn is_success(&self) -> bool {
        self.result.is_ok()
    }
}

/// اجرای خط لولهٔ ۵ مرحله‌ای مهاجرت برای یک کلید مشخص.
///
/// مراحل:
/// ۱. بررسی خالی بودن کلید (در صورت خالی بودن: `SkippedEmpty`).
/// ۲. استعلام مخزن و کشف تعارض (در صورت وجود کلید متفاوت: گزارش `Conflict` و حفظ کلید تنظیمات).
/// ۳. ذخیره در مخزن (در صورت عدم حضور کلید).
/// ۴. بازخوانی و اعتبارسنجی تطابق (Read-back Verification).
/// ۵. پاک‌سازی کلید از تنظیمات از طریق درگاه [`SettingsPersister`].
///
/// **قاعدهٔ عدم از دست رفتن داده (Zero Data Loss):**
/// پیش از موفقیت قطعی مرحلهٔ ۴، هرگز درگاه ثبت تنظیمات (مرحله ۵) صدا زده نمی‌شود.
/// اگر مرحلهٔ ۵ نیز با خطا روبرو شود، کلید در تنظیمات حذف‌نشده تلقی می‌گردد.
pub fn migrate_credential(
    store: &dyn CredentialStore,
    persister: &dyn SettingsPersister,
    item: &MigrationItem,
) -> MigrationOutcome {
    let target = &item.target;

    // ۱. اگر کلید در تنظیمات خالی است، کاری لازم نیست
    if item.legacy_secret.is_empty() {
        return MigrationOutcome {
            target: target.clone(),
            result: Ok(MigrationAction::SkippedEmpty),
        };
    }

    // ۲. بررسی وجود رکورد در مخزن و محافظت در برابر بازنویسی کورکورانه در مهاجرت خودکار
    let already_present = match store.load(target) {
        Ok(existing) => {
            if existing.expose_secret() == item.legacy_secret.expose_secret() {
                // دقیقاً همین کلید قبلاً ثبت شده؛ ادامه به اعتبارسنجی و پاک‌سازی
                true
            } else {
                // تعارض: رکوردی با مقدار متفاوت در مخزن وجود دارد.
                // طبق بند ۴ الزامات: رکورد موجود کورکورانه بازنویسی نمی‌شود و کلید تنظیمات حفظ می‌گردد.
                return MigrationOutcome {
                    target: target.clone(),
                    result: Err(MigrationError::Conflict(MigrationConflict {
                        target: target.clone(),
                        message: "credential store already contains a different secret for this target; refusing blind overwrite in automatic migration".into(),
                    })),
                };
            }
        }
        Err(CredentialError::NotFound(_)) => {
            // کلید در مخزن نیست، ادامهٔ طبیعی مهاجرت
            false
        }
        Err(err) => {
            // خطای زیرساختی در استعلام مخزن (دسترسی، سرویس، ...)
            return MigrationOutcome {
                target: target.clone(),
                result: Err(MigrationError::StoreProbeFailed {
                    target: target.clone(),
                    error: err,
                }),
            };
        }
    };

    // ۳. ذخیره در مخزن تنها اگر از قبل موجود نباشد
    if !already_present {
        if let Err(err) = store.save(target, &item.legacy_secret) {
            return MigrationOutcome {
                target: target.clone(),
                result: Err(MigrationError::StoreSaveFailed {
                    target: target.clone(),
                    error: err,
                }),
            };
        }
    }

    // ۴. بازخوانی و تأیید انطباق دقیق (Read-back Verification)
    match store.load(target) {
        Ok(loaded) => {
            if loaded.expose_secret() != item.legacy_secret.expose_secret() {
                return MigrationOutcome {
                    target: target.clone(),
                    result: Err(MigrationError::ReadBackMismatch {
                        target: target.clone(),
                    }),
                };
            }
        }
        Err(err) => {
            return MigrationOutcome {
                target: target.clone(),
                result: Err(MigrationError::StoreReadBackFailed {
                    target: target.clone(),
                    error: err,
                }),
            };
        }
    }

    // ۵. ثبت امن پاک‌سازی کلید در تنظیمات (فقط و فقط پس از تأیید کامل مرحله ۴)
    if let Err(err) = persister.persist_cleared_key(target) {
        return MigrationOutcome {
            target: target.clone(),
            result: Err(MigrationError::SettingsPersistFailed {
                target: target.clone(),
                error: err,
            }),
        };
    }

    // ۶. تعیین شناسهٔ اقدام نهایی
    let action = if already_present {
        MigrationAction::MigratedAlreadyPresent
    } else {
        MigrationAction::MigratedFresh
    };

    MigrationOutcome {
        target: target.clone(),
        result: Ok(action),
    }
}

/// اجرای دسته‌ای مهاجرت برای مجموعه‌ای از کلیدها.
pub fn migrate_credentials(
    store: &dyn CredentialStore,
    persister: &dyn SettingsPersister,
    items: &[MigrationItem],
) -> Vec<MigrationOutcome> {
    items
        .iter()
        .map(|item| migrate_credential(store, persister, item))
        .collect()
}
