---
name: code-review
description: Perform strict peer code reviews focusing on clean architecture, Persian encoding stability, type safety, error handling, and performance.
---

# Code Review Skill

## هدف مهارت
کنترل کیفیت کد منبع، رعایت استانداردهای Clean Code، پایداری خطا و عملکرد بهینه در ماژول‌های سامانه ارجاع آتبین ایستا.

## معیارهای پذیرش کد (Acceptance Checklist)
1. **قانون Comment WHY not WHAT:** توضیحات کد باید چرایی تصمیم فنی را بازگو کنند، نه شرح بدیهیات دستورات.
2. **مدیریت خطای صریح (Explicit Error Handling):** ممانعت از `try ... except: pass` بدون لاگ و استفاده از انواع صریح به جای `any`.
3. **پایداری انکودینگ:** تنظیم اجباری `sys.stdout.reconfigure(encoding='utf-8')` در کلیه اسکریپت‌های پایتون ویندوز جهت جلوگیری از `UnicodeEncodeError`.
4. **تست‌های متناظر:** وجود تست‌های واحد یا شواهد اجرایی برای عملکردهای حیاتی.
