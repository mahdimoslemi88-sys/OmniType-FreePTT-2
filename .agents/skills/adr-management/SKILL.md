---
name: adr-management
description: Manage the complete lifecycle of Architecture Decision Records (ADRs) from drafting and trade-off analysis to approval and superseding.
---

# ADR Management Skill

## هدف مهارت
این مهارت راهنمای جامع مدیریت چرخه حیات اسناد تصمیم معماری (ADR) در پروژه آتبین ایستا است.

## گردش کار مهارت (Workflow)
1. **شناسایی نیاز به تصمیم معماری:**
   - انتخاب مدل زبانی، موتور OCR، دیتابیس، استراتژی ابری/محلی، یا ساختار صف.
2. **انتخاب شماره یکتا:**
   - جستجو در پوشه `docs/ADR/` و تخصیص شماره توالی بعدی (مثلاً `ADR-001`, `ADR-002`, ...).
3. **ایجاد پیش‌نویس بر اساس تمپلیت:**
   - کپی و تکمیل ساختار از `.agents/templates/ADR-template.md`.
4. **تحلیل چندگزینه‌ای (Multi-Option Evaluation):**
   - تحلیل حداقل ۲ الی ۳ گزینه متمایز با برشمردن نقاط قوت، ضعف و برآورد فنی.
5. **ارسال برای بازبینی و تعیین وضعیت:**
   - ثبت در وضعیت `Proposed` و درخواست تایید از ذینفعان.
6. **به‌روزرسانی شاخص اسناد:**
   - ثبت شناسه و عنوان ADR در `docs/INDEX.md`.
