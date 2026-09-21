---
name: prompt-engineering
description: Craft high-precision Few-Shot prompts, structured JSON outputs, chain-of-thought industrial reasoning, and taxonomy classification instructions.
---

# Industrial Prompt Engineering Skill

## هدف مهارت
طراحی، بهینه‌سازی و کالیبراسیون پرامپت‌های مهندسی برای طبقه‌بندی ۵ لایه‌ای استعلام‌های صنعتی آتبین ایستا.

## الگوهای طراحی پرامپت (Prompt Design Patterns)
1. **فرمت خروجی ساختاریافته اجباری (Enforced JSON Schema):**
   - مدل باید بدون متن اضافه، خروجی را در قالب یک شیء JSON با فیلدهای مشخص لایه‌های پنج‌گانه بازگرداند.
2. **الگوی چندمثاله متوازن (Balanced Few-Shot Examples):**
   - ارائه نمونه‌های حل‌شده واقعی از هر واحد، به ویژه واحدهای اقلیت (حفاظت کاتدیک، پایپینگ، توربوماشینری) برگرفته از آرشیو تاریخی.
3. **زنجیره تفکر صنعتی (Chain-of-Thought for Technical Reasoning):**
   - الزام مدل به تحلیل اولیه استانداردهای فنی ذکر شده در نامه مشتری پیش از اعلام برچسب نهایی واحد ارجاع.
4. **مهار توهم (Hallucination Mitigation):**
   - اضافه کردن دستور صریح: «در صورتی که هیچ نشانه‌ای از کالای مورد نظر در کاتالوگ وجود ندارد، برچسب UNKNOWN یا OTHER بازگردانده شود و از حدس زدن پرهیز گردد».
