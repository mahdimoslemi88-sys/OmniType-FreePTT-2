---
name: question-generation
description: Detect missing requirements, resolve domain ambiguity, and generate structured clarification questions for stakeholders using the standard template.
---

# Question Generation Skill

## هدف مهارت
شناسایی به‌موقع گپ‌های اطلاعاتی در نیازمندی‌ها، ممانعت از حدس‌زدن، و فرموله کردن سوالات رسمی شفاف بر اساس اصل Zero Assumption.

## چه زمانی این مهارت فعال می‌شود؟
- مشاهده کالایی در استعلام که در کاتالوگ یا تاکسونومی ۵ لایه تعریف نشده است.
- ابهام در منطق اولویت‌بندی درخواست‌های چندقلمی (مثلاً گسکت به همراه شیرآلات).
- ابهام در نحوه اتصال به نسخه محلی Sarv CRM یا پروتکل‌های ایمیل.

## مراحل اجرا (Procedure)
1. ساخت سند پرسش در مسیر `docs/questions/open/` با قالب `.agents/templates/question-template.md`.
2. تعیین شناسه پرسش (مانند `Q-001`, `Q-002`).
3. تشریح شفاف سوال، ریسک پیش‌فرض‌سازی، گزینه‌های محتمل و اثر هر گزینه.
4. ارجاع سوال به ذینفع مشخص (کارفرما، IT، مدیر واحد فنی).
5. پس از دریافت پاسخ رسمی: تکمیل بخش Resolution و انتقال سند به `docs/questions/resolved/`.
