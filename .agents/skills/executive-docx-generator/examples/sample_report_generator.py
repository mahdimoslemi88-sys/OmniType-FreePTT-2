# -*- coding: utf-8 -*-
"""
Sample Executive Report Generator using docx_builder
Demonstrates all features of the executive DOCX template
"""
import sys
from pathlib import Path

# Add script directory to sys.path
SCRIPT_DIR = Path(__file__).resolve().parent.parent / "scripts"
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from docx_builder import (
    create_executive_document,
    add_classification_banner,
    add_cover_page,
    add_part_heading,
    add_chapter_heading,
    add_subheading,
    add_body_paragraph,
    add_bullet_item,
    add_callout_box,
    add_styled_table,
    save_document
)

def generate_sample_report(output_path):
    doc = create_executive_document()
    
    # 1. Classification Banner
    add_classification_banner(doc, "طبقه‌بندی: محرمانه تجاری  |  اسناد راهبردی مهندسی و بازرگانی")
    
    # 2. Cover Page
    metadata = [
        ("کارفرما", "شرکت فنی و مهندسی آتبین ایستا"),
        ("نوع سند", "گزارش نمونه استاندارد سازمانی"),
        ("نسخه سند", "نسخه ۱٫۰ مصوب"),
        ("تاریخ تدوین", "شهریور ۱۴۰۵ (سپتامبر ۲۰۲۶)")
    ]
    add_cover_page(
        doc,
        title="سامانه هوشمند مدیریت و هدایت\nاستعلام‌های مهندسی آتبین ایستا",
        subtitle_1="اولین گام در ایجاد زیرساخت هوشمند مدیریت فرآیندهای مهندسی، فروش و عملیات سازمانی آتبین ایستا",
        subtitle_2="مبتنی بر تحلیل هوشمند اسناد مهندسی، دانش فنی محصولات و یکپارچه‌سازی با Sarv CRM",
        scope_note="طرح جامع معماری جریان داده‌ها، هوش اسناد و استقرار فاز نخست (Enterprise Production MVP)",
        metadata=metadata
    )
    
    # 3. Part 1 & Chapter 1
    add_part_heading(doc, "بخش ۱: طرح مدیریتی و اجرایی سامانه هوشمند")
    add_chapter_heading(doc, "فصل ۱: خلاصه مدیریتی، ارزش‌های راهبردی و نقشه تحول دیجیتال")
    
    add_body_paragraph(
        doc,
        text="شرکت‌های فعال در زنجیره تامین صنایع بالادستی نفت، گاز و پتروشیمی روزانه با حجم چشمگیری از استعلام‌های خرید (RFQ)، اسناد مناقصه و مکاتبات مهندسی مواجه هستند. در این عرصه فوق‌تخصصی، سرعت و دقت در ارزیابی اولیه، تشخیص دپارتمان تخصصی، استخراج مشخصات فنی کالا و ارجاع به‌موقع درخواست‌ها، مرز باریک میان پیروزی در قراردادهای بزرگ صنعتی یا حذف از زنجیره تامین کارفرمایان است.",
        bold_prefix="جایگاه استراتژیک در زنجیره ارزش:"
    )
    
    add_body_paragraph(
        doc,
        text="فاز نخست این پروژه به عنوان هسته اولیه معماری تحول دیجیتال فرآیندهای مهندسی و بازرگانی آتبین ایستا طراحی می‌شود. این سامانه به عنوان یک شتاب‌دهنده فرآیندی در کنار کارشناسان عمل کرده و با تحلیل اسناد، تطبیق با کاتالوگ‌های ۵‌گانه آتبین ایستا و سنجش ضریب اطمینان، پرونده کامل استعلام را همراه با جدول اقلام در نرم‌افزار سازمانی Sarv CRM ثبت می‌نماید.",
        bold_prefix="جایگاه فاز نخست در نقشه تحول دیجیتال:"
    )
    
    # Callout box
    add_callout_box(
        doc,
        text="تصمیم ارجاع = تطبیق کاتالوگ‌های ۵‌گانه + انطباق استانداردهای ASME/API + سنجش ضریب اطمینان\nدر این الگو، هیچ استعلامی بدون شفافیت کامل منشأ و شواهد متنی به CRM ارسال نخواهد شد.",
        title="رابطه بنیادین استنتاج هوشمند در سامانه آتبین ایستا:",
        box_type="info"
    )
    
    # Subheading & Bullet points
    add_subheading(doc, "پنج ارزش راهبردی سامانه برای آتبین ایستا:")
    add_bullet_item(doc, text="کاهش زمان تریاژ و آماده‌سازی پرونده از چندین ساعت به کمتر از ۱۵ دقیقه در چرخه کلان.", bold_prefix="ارزش ۱ (سرعت پاسخگویی):")
    add_bullet_item(doc, text="آزادسازی مهندسان ارشد از تایپ دستی ردیف‌های اقلام MTO و معطوف شدن تمرکز به محاسبات فنی.", bold_prefix="ارزش ۲ (ارتقای ظرفیت تیم فروش):")
    add_bullet_item(doc, text="تصمیم‌گیری بر پایه تطبیق دقیق عبارات متن با ۵ کاتالوگ مهندسی به جای حدس و گمان.", bold_prefix="ارزش ۳ (کاهش خطای ارجاع):")
    add_bullet_item(doc, text="تبدیل دانش تجربی افراد کلیدی سازمان به یک پایگاه دانش دیجیتال پایدار و ضد ضربه.", bold_prefix="ارزش ۴ (نهادینه‌سازی دارایی دانشی):")
    add_bullet_item(doc, text="استقرار کامل محلی درون‌سازمانی (On-Premise) بر روی سرور موجود شرکت HP G8 بدون خروج داده.", bold_prefix="ارزش ۵ (استقرار امن محلی):")
    
    # Styled Table
    add_subheading(doc, "جدول ارزیابی شاخص‌های کلیدی هدف (Target SLAs):")
    headers = ["شاخص کلیدی هدف (Target SLA)", "وضعیت فرآیند سنتی فعلی", "هدف‌گذاری با استقرار سامانه هوشمند"]
    rows = [
        ["زمان کلی آماده‌سازی پرونده و ارجاع", "۴ الی ۶ ساعت پس از وصول ایمیل", "کمتر از ۱۵ دقیقه (کاهش بیش از ۸۰٪)"],
        ["زمان پردازش اسناد متنی دیجیتال", "۲۰ الی ۴۵ دقیقه برای هر استعلام", "کمتر از ۲ دقیقه به صورت تمام‌خودکار"],
        ["زمان بررسی در کارتابل انسانی (HITL)", "بررسی دستی مجدد و طولانی", "کمتر از ۳۰ ثانیه (تایید با ۱ کلیک)"],
        ["دقت دسته‌بندی و تطبیق استانداردها", "وابسته به حضور کارشناس مشخص", "دقت پایدار و تطبیق مستند با کاتالوگ‌ها"]
    ]
    add_styled_table(doc, headers, rows, col_widths=[2.40, 2.20, 2.20], align_center_cols=[1, 2])
    
    # Dark Callout
    add_callout_box(
        doc,
        text="سامانه هوشمند مدیریت استعلام‌های مهندسی آتبین ایستا، نخستین گام بنیادین در مسیر تبدیل شرکت فنی مهندسی آتبین ایستا به پیشگام فناوری‌های هوش مصنوعی در صنعت تجهیزات نفت، گاز و پتروشیمی کشور است.",
        title="پیام نهایی به مدیریت ارشد سازمان:",
        box_type="navy"
    )
    
    save_document(doc, output_path)

if __name__ == "__main__":
    out = Path(__file__).resolve().parent / "sample_output.docx"
    generate_sample_report(out)
