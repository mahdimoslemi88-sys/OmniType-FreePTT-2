---
name: security-review
description: Execute security audits, secrets detection, PII sanitization verification, and attachment sandboxing assessments.
---

# Enterprise Security Review Skill

## هدف مهارت
اجرای ممیزی امنیتی روی کدها، پایپلاین‌های پردازش داده و معماری یکپارچگی سیستم‌های آتبین ایستا.

## روش اجرای ممیزی امنیتی
1. **اسکن متغیرهای حساس:** بررسی کدهای پایتون و شل برای اطمینان از عدم درج پسورد یا کلیدها در سورس.
2. **بررسی پایپلاین ماسک‌گذاری (Sanitization Verification):**
   - ارسال نمونه‌های تست حاوی شماره‌های موبایل، کدهای ملی، نام مدیران و مبالغ ریالی و ارزی و تایید جایگزینی آن‌ها با توکن‌های `[REDACTED_PHONE]`, `[REDACTED_NAME]`, `[REDACTED_AMOUNT]`.
3. **ممیزی ارتباطات شبکه:**
   - تایید استفاده از TLS 1.3 / HTTPS در اتصالات IMAP و وب‌سرویس Sarv CRM.
4. **ثبت گزارش بازبینی:** ایجاد سند در `docs/reviews/REV-[NUMBER]-security.md`.
