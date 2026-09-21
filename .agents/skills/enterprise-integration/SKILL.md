---
name: enterprise-integration
description: Implement reliable enterprise integrations with Sarv CRM API, IMAP email listener, transactional outbox pattern, and idempotent task workers.
---

# Enterprise Integration Skill (مهارت یکپارچه‌سازی سازمانی و صف‌های نامتقارن)

## هدف مهارت
این مهارت راهنمای جامع اتصال ایمن و تاب‌آور سامانه هوشمند ارجاع به صندوق ایمیل سازمان (`IMAP SSL`) و نرم‌افزار مدیریت ارتباط با مشتریان (`Sarv CRM REST API`) است.

## گردش کار مهارت (Workflow)
1. **شنود ناهمگام ایمیل (IMAP Non-blocking Listener):**
   - اتصال دوره‌ای با SSL، دریافت فراداده، ذخیره امن پیوست‌ها روی دیسک، و صدور تسک در صف.
2. **الگوی ثبت مطمئن پرونده (Transactional Outbox Pattern):**
   - ثبت کلیه درخواست‌ها ابتدا در دیتابیس رابطه‌ای محلی همراه با کلید یکتایی (`Idempotency Key` بر مبنای هش فرستنده-تاریخ-موضوع) تا رکوردهای تکراری در CRM ایجاد نشود.
3. **مکانیزم تکرار خودکار با تاخیر تصاعدی (Exponential Backoff):**
   - در صورت بروز خطای شبکه یا قطعی موقت درگاه CRM، تلاش مجدد در فواصل زمانی تصاعدی انجام شده و در صورت شکست مکرر به صف پیام‌های مرده (DLQ) هدایت می‌شود.
4. **مدیریت مدارشکن (Circuit Breaker):**
   - توقف موقت فراخوانی سرویس خارجی در زمان خطاهای پیاپی سرور مقصد جهت حفظ پایداری سیستم.
