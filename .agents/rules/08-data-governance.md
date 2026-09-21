# Rule 08: Enterprise Data Governance, Lineage & Lifecycle (حاکمیت داده‌ها، شجره‌نامه و چرخه حیات)
**Scope:** Data Engineer, Document AI Engineer, Security Reviewer  
**Authority:** Data Engineer & Project Governance  

---

## ۱. تبار داده‌ها (Data Lineage & Provenance)
کلیه تبدیلات بر روی داده‌ها از فایل خام آرشیو `backup.pst` تا خروجی‌های پایگاه داده باید در سند [`data/DATASET_CHANGELOG.md`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/data/DATASET_CHANGELOG.md) مستند و با اسکریپت‌های تکرارپذیر تضمین شوند.

## ۲. طبقه‌بندی چهارلایه محرمانگی داده‌ها (Data Classification Tiers)
1. **سطح عمومی (Public - Tier 1):** مشخصات عمومی کاتالوگ‌ها و استانداردهای باز بین‌المللی.
2. **سطح داخلی (Internal - Tier 2):** متون استعلام پالایش‌شده و بدون نام خریدار و بدون مبالغ مالی.
3. **سطح محرمانه (Confidential - Tier 3):** ایمیل‌های حاوی نام شرکت‌های پتروشیمی، شماره مناقصات و شماره تماس کارشناسان.
4. **سطح به‌کلی سری (Restricted - Tier 4):** ارقام مالی، قیمت‌های پیشنهادی، فرمول‌های ساخت گسکت و نقشه‌های محرمانه انحصاری.

*قانون قطعی:* داده‌های سطوح ۳ و ۴ تحت هیچ شرایطی نباید به سرویس‌های خارج از سرور محلی شرکت ارسال شوند.

## ۳. سیاست امحای امن کش‌های موقت (Data Purge Policy)
فایل‌های موقت استخراج‌شده از بسته‌های ZIP، تصاویر کراپ‌شده OCR و لاگ‌های خام فرآیندی باید حداکثر پس از ۷۲ ساعت از پردازش موفق به صورت خودکار از دیسک محلی پاکسازی شوند.
