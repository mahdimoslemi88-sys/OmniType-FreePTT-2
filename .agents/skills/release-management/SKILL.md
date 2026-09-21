---
name: release-management
description: Manage release readiness, semantic versioning, deliverable packaging, and deployment smoke tests.
---

# Release Management Skill

## هدف مهارت
مدیریت فرآیند انتشار نسخه‌ها، شماره‌گذاری معنایی (Semantic Versioning)، بسته‌بندی خروجی‌ها و تایید آمادگی عملیاتی.

## مراحل انتشار (Release Lifecycle)
1. **بررسی چک‌لیست انتشار:**
   - آیا کلیه آزمون‌های QA با موفقیت پاس شده‌اند؟
   - آیا اسناد `docs/INDEX.md` و گزارش‌های فنی به‌روز هستند؟
   - آیا تاییدیه‌های امنیتی اخذ شده است؟
2. **برچسب‌گذاری نسخه (Tagging & Changelog):**
   - ثبت تغییرات در `CHANGELOG.md` با شرح دلایل و ویژگی‌های جدید.
3. **تست دود (Smoke Test):**
   - اجرای یک استعلام آزمایشی واقعی و دنبال کردن کل چرخه از دریافت ایمیل تا استخراج OCR، پیشنهاد ارجاع و پیش‌نمایش در داشبورد.
