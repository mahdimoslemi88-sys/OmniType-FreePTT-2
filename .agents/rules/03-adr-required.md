# Rule 03: Architecture Decision Record Requirement (الزام ثبت ADR)
**Scope:** Solution Architect & Engineering Agents  
**Authority:** Solution Architect & Project Governance  

---

## 1. اصل اساسی (Fundamental Principle)
هرگونه تصمیم معماری، تغییر فناوری، انتخاب مدل هوش مصنوعی، ساختار پایگاه داده، یا پروتکل ارتباطی باید پیش از اجرا در قالب یک **سند تصمیم معماری (ADR)** تدوین، ارزیابی و مصوب شود.

## 2. ساختار اجباری ADR
هر سند ADR در مسیر `docs/ADR/` با نام‌گذاری استاندارد `ADR-XXX-<title-slug>.md` بر پایه تمپلیت `.agents/templates/ADR-template.md` ایجاد می‌شود و باید شامل بخش‌های زیر باشد:
- **Status:** Proposed / Accepted / Rejected / Deprecated
- **Context:** چرایی نیاز به تصمیم و شرایط فعلی سیستم
- **Problem Statement:** مسئله دقیق فنی که باید حل شود
- **Options Considered:** حداقل ۲ الی ۳ گزینه با تحلیل مزایا و معایب
- **Decision:** گزینه منتخب و استدلال فنی
- **Consequences:** پیامدهای مثبت و منفی تصمیم
- **Risks & Mitigations:** ریسک‌های فنی و راهکارهای مهار آن
- **Approval Required:** نیازمندی به تایید انسانی کارفرما یا معمار ارشد

## 3. آستانه الزام ADR (When ADR is Mandatory)
- انتخاب استراتژی ابری / محلی / ترکیبی (Cloud vs On-Premise vs Hybrid)
- انتخاب موتور OCR و پردازش اسناد صنعتی
- انتخاب معماری خط لوله هوش مصنوعی (RAG vs Fine-tuning vs Rule-Engine)
- انتخاب فریم‌ورک بک‌اند، دیتابیس یا پیام‌رسان صف‌ها
- تغییر فرمت خروجی‌های استاندارد دیتاست
