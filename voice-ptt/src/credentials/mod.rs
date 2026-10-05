//! ماژول مدیریت اعتبارنامه‌ها و کلیدهای محرمانه (بستهٔ A1)
//!
//! این ماژول قراردادهای نوع‌دار ذخیره‌سازی، خواندن، حذف و مهاجرت کلیدهای
//! سرویس‌های ASR را فراهم می‌کند و از افشای آن‌ها در لاگ، خطای سیستم و Debug جلوگیری می‌کند.

pub mod migration;
pub mod mock;
pub mod windows;

use std::fmt;

#[allow(unused_imports)]
pub use migration::{
    migrate_credential, migrate_credentials, MigrationAction, MigrationConflict, MigrationError,
    MigrationItem, MigrationOutcome, PersistError, SettingsPersister,
};
pub use mock::MockCredentialStore;
pub use windows::WindowsCredentialStore;

/// محفظهٔ امن برای نگهداری کلید محرمانه در حافظه.
///
/// اهداف:
/// ۱. جلوگیری از افشای تصادفی مقدار کلید در خروجی‌های [`fmt::Debug`] و [`fmt::Display`].
/// ۲. کنترل دسترسی صریح به متن محرمانه تنها در نقطهٔ مصرف نهایی از طریق [`expose_secret`].
///
/// **نکتهٔ امنیتی:** پیاده‌سازی [`Drop`] این ساختار تلاش بهینه (Best-effort) برای صفر کردن
/// بایت‌های بافر جاری انجام می‌دهد؛ اما ادعای تضمین ریشه‌کن شدن تمام نسخه‌های پیشین کلید از حافظه
/// (مانند رونوشت‌های احتمالی تخصیص‌دهندهٔ حافظه، کش سیستم یا فضای swap سیستم‌عامل) را ندارد.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// ایجاد نمونهٔ جدید از مقدار محرمانه.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// دسترسی مستقیم و کنترل‌شده به رشتهٔ محرمانه جهت ارسال به سرویس (مثلاً هدر HTTP).
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// بررسی خالی بودن یا صرفاً فاصله بودن مقدار محرمانه.
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

// جلوگیری از افشای مقدار محرمانه در خروجی فرمت‌بندی خطایاب
impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED_SECRET]")
    }
}

// جلوگیری از افشای مقدار محرمانه در خروجی نمایشی
impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED_SECRET]")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // بازنویسی بافر جاری با مقدار صفر به صورت تلاش بهینه (Best-effort).
        // این کار ماندگاری رشته در heap جاری را کاهش می‌دهد، اما تضمین‌کنندهٔ
        // پاک‌شدن تمام نسخه‌های قبلی یا بهینه‌سازی‌های کامپایلر نیست.
        unsafe {
            let bytes = self.0.as_bytes_mut();
            for b in bytes.iter_mut() {
                std::ptr::write_volatile(b, 0);
            }
        }
    }
}

/// خطاهای نوع‌دار حوزهٔ مخزن اعتبارنامه.
///
/// تفکیک صریح میان «نبود کلید» (`NotFound`) و «خطای دسترسی/سیستم‌عامل» تضمین می‌کند
/// که منطق بالادستی تصمیم نادرست نگیرد.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    /// کلید برای شناسهٔ درخواستی در مخزن یافت نشد.
    NotFound(String),

    /// دسترسی به مخزن توسط سیستم‌عامل یا سرویس رد شد.
    AccessDenied(String),

    /// محتوای بازیابی‌شده نامعتبر یا غیرقابل رمزگشایی است.
    CorruptedData(String),

    /// سرویس یا مخزن اعتبارنامه موقتاً یا دائم در دسترس نیست.
    Unavailable(String),

    /// خطای بومی سیستم‌عامل همراه با کد خطا.
    OsError { code: u32, message: String },

    /// تلاش برای ذخیرهٔ مقدار خالی یا صرفاً فاصله؛ باید از متد delete استفاده شود.
    EmptySecret,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(target) => write!(f, "credential not found for target '{target}'"),
            Self::AccessDenied(target) => {
                write!(f, "access denied to credential store for target '{target}'")
            }
            Self::CorruptedData(target) => {
                write!(
                    f,
                    "corrupted or invalid credential data for target '{target}'"
                )
            }
            Self::Unavailable(msg) => write!(f, "credential store unavailable: {msg}"),
            Self::OsError { code, message } => {
                write!(f, "operating system error {code}: {message}")
            }
            Self::EmptySecret => write!(
                f,
                "cannot store an empty or whitespace-only secret; use delete instead"
            ),
        }
    }
}

impl std::error::Error for CredentialError {}

/// قرارداد مشترک و مستقل از پلتفرم مخزن اعتبارنامه.
pub trait CredentialStore: Send + Sync {
    /// ذخیره یا بازنویسی کلید محرمانه برای شناسهٔ مشخص.
    fn save(&self, target: &str, secret: &SecretString) -> Result<(), CredentialError>;

    /// خواندن کلید محرمانه برای شناسهٔ مشخص.
    fn load(&self, target: &str) -> Result<SecretString, CredentialError>;

    /// حذف کلید محرمانه برای شناسهٔ مشخص.
    fn delete(&self, target: &str) -> Result<(), CredentialError>;
}

/// به‌روزرسانی صریح کلید توسط اقدام ارادی کاربر یا رابط کاربری.
///
/// **تفکیک از مهاجرت خودکار:** این تابع برای سناریویی است که کاربر آگاهانه مقدار
/// جدیدی را برای یک سرویس وارد کرده و بازنویسی رکورد پیشین هدف قطعی عملیات است.
pub fn explicit_update_credential(
    store: &dyn CredentialStore,
    target: &str,
    new_secret: &SecretString,
) -> Result<(), CredentialError> {
    if new_secret.is_empty() {
        return Err(CredentialError::EmptySecret);
    }
    store.save(target, new_secret)
}

/// حذف صریح کلید از مخزن توسط اقدام ارادی کاربر.
pub fn explicit_delete_credential(
    store: &dyn CredentialStore,
    target: &str,
) -> Result<(), CredentialError> {
    store.delete(target)
}
