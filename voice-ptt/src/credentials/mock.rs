//! مخزن ساختگی اعتبارنامه برای آزمون‌های واحد و رفتاری (Mock)
//!
//! این ماژول امکان تزریق انواع خطاهای دسترسی، نبود کلید، خرابی سرویس
//! و بررسی وضعیت داده‌ها در حافظه را بدون تماس با سیستم‌عامل فراهم می‌کند.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

use super::{CredentialError, CredentialStore, SecretString};

/// مخزن ساختگی در حافظه با قابلیت تزریق خطا.
pub struct MockCredentialStore {
    data: RwLock<HashMap<String, String>>,
    save_error: RwLock<Option<CredentialError>>,
    load_error: RwLock<Option<CredentialError>>,
    delete_error: RwLock<Option<CredentialError>>,
    load_override: RwLock<Option<String>>,
    /// مقدار جایگزین فقط پس از ذخیره (جهت شبیه‌سازی عدم تطابق در بازخوانی مرحله ۴)
    post_save_load_override: RwLock<Option<String>>,
    save_count: AtomicUsize,
}

impl Default for MockCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCredentialStore {
    /// ایجاد مخزن ساختگی خالی.
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            save_error: RwLock::new(None),
            load_error: RwLock::new(None),
            delete_error: RwLock::new(None),
            load_override: RwLock::new(None),
            post_save_load_override: RwLock::new(None),
            save_count: AtomicUsize::new(0),
        }
    }

    /// مقداردهی اولیهٔ زنجیره‌ای با یک کلید مشخص.
    pub fn with_credential(self, target: impl Into<String>, secret: impl Into<String>) -> Self {
        self.data
            .write()
            .unwrap()
            .insert(target.into(), secret.into());
        self
    }

    /// تزریق خطا در عملیات ذخیره‌سازی (`save`).
    pub fn set_save_error(&self, err: Option<CredentialError>) {
        *self.save_error.write().unwrap() = err;
    }

    /// تزریق خطا در عملیات خواندن (`load`).
    pub fn set_load_error(&self, err: Option<CredentialError>) {
        *self.load_error.write().unwrap() = err;
    }

    /// تزریق خطا در عملیات حذف (`delete`).
    pub fn set_delete_error(&self, err: Option<CredentialError>) {
        *self.delete_error.write().unwrap() = err;
    }

    /// تزریق مقدار بازگشتی دستکاری‌شده در عملیات خواندن.
    #[allow(dead_code)]
    pub fn set_load_override(&self, val: Option<String>) {
        *self.load_override.write().unwrap() = val;
    }

    /// تزریق مقدار بازگشتی دستکاری‌شده فقط پس از انجام موفق ذخیره (برای تست بازخوانی مرحله ۴).
    pub fn set_post_save_load_override(&self, val: Option<String>) {
        *self.post_save_load_override.write().unwrap() = val;
    }

    /// بررسی وجود یک شناسه در مخزن بدون دستکاری خطاها.
    pub fn contains(&self, target: &str) -> bool {
        self.data.read().unwrap().contains_key(target)
    }

    /// دریافت مقدار خام کلید (صرفاً برای ادعاهای تستی).
    pub fn raw_get(&self, target: &str) -> Option<String> {
        self.data.read().unwrap().get(target).cloned()
    }

    /// تعداد دفعات فراخوانی ذخیره
    #[allow(dead_code)]
    pub fn save_count(&self) -> usize {
        self.save_count.load(Ordering::SeqCst)
    }
}

impl CredentialStore for MockCredentialStore {
    fn save(&self, target: &str, secret: &SecretString) -> Result<(), CredentialError> {
        if let Some(err) = self.save_error.read().unwrap().as_ref() {
            return Err(err.clone());
        }
        if secret.is_empty() {
            return Err(CredentialError::EmptySecret);
        }
        self.save_count.fetch_add(1, Ordering::SeqCst);
        self.data
            .write()
            .unwrap()
            .insert(target.to_string(), secret.expose_secret().to_string());
        Ok(())
    }

    fn load(&self, target: &str) -> Result<SecretString, CredentialError> {
        if let Some(err) = self.load_error.read().unwrap().as_ref() {
            return Err(err.clone());
        }
        // اگر ذخیره‌ای انجام شده و مقدار post_save تنظیم شده، آن را برمی‌گردانیم
        if self.save_count.load(Ordering::SeqCst) > 0 {
            if let Some(ref over) = *self.post_save_load_override.read().unwrap() {
                return Ok(SecretString::new(over.clone()));
            }
        }
        if let Some(ref over) = *self.load_override.read().unwrap() {
            return Ok(SecretString::new(over.clone()));
        }
        match self.data.read().unwrap().get(target) {
            Some(val) => Ok(SecretString::new(val.clone())),
            None => Err(CredentialError::NotFound(target.to_string())),
        }
    }

    fn delete(&self, target: &str) -> Result<(), CredentialError> {
        if let Some(err) = self.delete_error.read().unwrap().as_ref() {
            return Err(err.clone());
        }
        let mut map = self.data.write().unwrap();
        if map.remove(target).is_some() {
            Ok(())
        } else {
            Err(CredentialError::NotFound(target.to_string()))
        }
    }
}
