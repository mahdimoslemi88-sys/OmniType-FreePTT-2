---
name: architecture-review
description: Conduct rigorous architectural reviews, trade-off evaluations, modular decoupling audits, and non-functional requirements assessments.
---

# Architecture Review Skill

## هدف مهارت
ارزیابی انتقادی و اعتبارسنجی طرح‌های معماری پیش از آغاز کدنویسی جهت حصول اطمینان از مقیاس‌پذیری، ماژولار بودن، سهولت نگهداری و انطباق با محدودیت‌های سرور کارفرما.

## چک‌لیست بازبینی معماری (Review Checklist)
1. **تفکیک وظایف (Separation of Concerns):** آیا لایه شنود ایمیل، موتور OCR، موتور هوش مصنوعی و اتصال CRM مستقل از هم هستند؟
2. **پایداری در برابر خطا (Fault Tolerance):** در صورت قطع شبکه، کندی اینترنت یا مسدود شدن سرویس خارجی، آیا صف‌ها قابلیت Retry دارند؟
3. **تطابق با زیرساخت سرور:** آیا مصرف حافظه رم (محدود به ۳۲ گیگابایت سرور G8) و پردازنده در حد مجاز است؟
4. **قابلیت تست‌پذیری (Testability):** آیا ماژول‌ها بدون وابستگی مستقیم به سخت‌افزار یا ایمیل زنده قابل شبیه‌سازی و تست هستند؟
5. **ارائه گزارش بازبینی:** مستندسازی یافته‌ها با تمپلیت `.agents/templates/review-template.md`.
