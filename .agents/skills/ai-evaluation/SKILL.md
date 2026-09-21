---
name: ai-evaluation
description: Benchmark AI models, classification pipelines, prompt iterations, and OCR accuracy against ground truth test datasets using Macro-F1 and confusion matrices.
---

# AI Evaluation & Benchmarking Skill

## هدف مهارت
سنجش کمی و کیفی عملکرد خطوط لوله هوش مصنوعی، مدل‌های دسته‌بندی و موتورهای OCR با تکیه بر پایگاه داده آزمون مرجع.

## مراحل ارزیابی (Benchmarking Procedure)
1. **آماده‌سازی مجموعه آزمون:** استفاده از مجموعه ۱٬۰۰۰ تایی متوازن (`data/dataset_sample_1000.csv`).
2. **محاسبه متریک‌های استاندارد:**
   - **Macro F1-Score:** میانگین نامتقارن امتیاز F1 برای هر ۸ واحد سازمانی.
   - **Precision & Recall به تفکیک کلاس:** شناسایی کلاس‌های ضعیف (مانند پایپینگ یا ساخت تجهیزات).
   - **ماتریس درهم‌ریختگی (Confusion Matrix):** تحلیل اینکه کدام واحدها به اشتباه به عنوان گسکت شناسایی می‌شوند.
3. **ارزیابی کالیبراسیون اطمینان (Confidence Calibration):**
   - تعیین آستانه تفکیک برای ارجاع خودکار (مثلاً اطمینان > ۹۰٪) در برابر ارجاع به کارتابل بازبینی انسانی (اطمینان < ۹۰٪).
4. **ثبت سند گزارش ارزیابی:** ذخیره در `docs/evaluations/EVAL-[NUMBER].md`.
