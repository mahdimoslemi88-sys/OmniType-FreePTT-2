---
name: industrial-ontology
description: Industrial ontology modeling, equipment taxonomy structuring, engineering specification mapping, and knowledge graph relations for petroleum and mechanical components.
---

# Industrial Ontology & Equipment Taxonomy Skill

## هدف مهارت (Skill Goal)
مدل‌سازی آنتولوژی صنعتی، ساختاردهی طبقه‌بندی تجهیزات مهندسی، نگاشت استانداردهای پایپینگ و آب‌بندی (ASME/API/DIN) و ایجاد گراف دانش ارتباطات میان اقلام و قطعات فنی شرکت فنی و مهندسی آتبین ایستا.

## اصول و روش‌شناسی (Methodology)
1. **طبقه‌بندی ۵ لایه‌ای تجهیزات (5-Layer Equipment Taxonomy):**
   - لایه ۱: دسته کلان (Major Category - مثال: Piping & Flow Control)
   - لایه ۲: خانواده محصول (Product Family - مثال: Gaskets & Sealing Solutions)
   - لایه ۳: نوع فرعی و متریال (Sub-type & Metallurgy - مثال: Spiral Wound Gasket / SS316 + Graphite)
   - لایه ۴: رده استاندارد و کلاس فشاری (Standard & Pressure Class - مثال: ASME B16.20 / Class 150-2500)
   - لایه ۵: پارت‌نامبر سازنده یا مشخصه استعلام (Part Number / Tag)

2. **روابط هستی‌شناسی (Ontological Relationships):**
   - `is_compatible_with`: سازگاری اتصالات با فلنج‌ها و کلاس‌های فشاری
   - `manufactured_by`: نگاشت تولیدکنندگان استاندارد و تاییدیه‌های AVL (Approved Vendor List)
   - `replaces_or_equivalent`: جایگزینی و هم‌ارزی استانداردهای بین‌المللی با اقلام تاریخی
   - `requires_component`: وابستگی اقلام (نظیر واشر، پیچ و مهره Stud Bolt به فلنج)

3. **قوانین اعتبارسنجی دانش:**
   - هر رابطه فنی باید مستند به استانداردهای بین‌المللی یا کاتالوگ‌های رسمی سازنده باشد.
   - ثبت ابهام در صورت عدم تطابق سایز (NPS) و کلاس فشاری بر مبنای سیاست عدم پیش‌فرض‌سازی.
