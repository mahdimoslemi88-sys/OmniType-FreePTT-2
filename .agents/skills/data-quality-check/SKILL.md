---
name: data-quality-check
description: Validate dataset hygiene, detect duplicates and spam, verify UTF-8-BOM encoding, and ensure schema completeness.
---

# Data Quality Check Skill

## هدف مهارت
پایش پیوسته سلامت، انکودینگ، صحت مقادیر ستون‌ها و عدم تخریب داده‌های تاریخی پروژه آتبین ایستا.

## چک‌لیست اعتبارسنجی داده (Validation Checklist)
1. **تایید انکودینگ UTF-8-BOM:**
   - خواندن بایت‌های آغازین فایل CSV (`EF BB BF`) برای اطمینان از سازگاری کامل با اکسل ویندوز.
2. **بررسی مقادیر مفقوده (Null Values Audit):**
   - اطمینان از مقداردهی فیلدهای حیاتی (`email_id`, `date`, `department_label`, `original_text`).
3. **صحت طبقه‌بندی بولین نویزها:**
   - تطابق رکوردهای پرچم‌خورده با فلگ‌های `is_spam_or_promo` و `is_duplicate`.
4. **عدم انحراف در شمارش کل رکوردهای مرجع:**
   - تایید ثابت بودن تعداد ۳٬۰۹۷ رکورد در دیتابیس کامل و ۱٬۰۰۰ رکورد در نمونه منتخب.
