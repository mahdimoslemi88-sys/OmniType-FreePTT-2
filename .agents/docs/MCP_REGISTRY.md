# شناسنامه و رجیستری سرورهای پروتکل کانتکست مدل (MCP Registry v2.0)
## سامانه هوشمند هدایت و ارجاع درخواست‌های صنعتی — پلتفرم کینیکسا آتبین (Kynexa-Aitco)

**نسخه:** ۲٫۰٫۰ | **تاریخ مهاجرت و اعتبارسنجی:** ۲۲ شهریور ۱۴۰۵ (۱۲ سپتامبر ۲۰۲۶)  
**متولی:** تیم ارکستراسیون پلتفرم و معماری سیستم‌ها (`solution-architect` & `project-governance`)  
**قلمرو استقرار:** منحصراً در سطح پروژه (Project-Scoped Only) — بدون هرگونه دستکاری در پیکربندی سراسری کاربر (Zero Global Footprint)  
**منبع فعال Antigravity:** فقط `.agents/mcp_config.json` با چهار سرور؛ سرورهای Plugin فعلاً با `disabled: true` نگهداری می‌شوند.  
**فایل پیکربندی فعال پروژه:**  
- [`.agents/mcp_config.json`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/.agents/mcp_config.json) (نقطه ثبت رسمی در سطح ورک‌اسپیس پروژه)

---

## ۱. گزارش رسمی مهاجرت به ساختار استاندارد Antigravity (Migration Record)

در تاریخ ۲۲ شهریور ۱۴۰۵، کل ساختار پیکربندی عامل‌ها، مهارت‌ها، قوانین و مستندات پروژه از دایرکتوری غیراستاندارد `.agents/` به ساختار استاندارد رسمی **Google Antigravity** یعنی **`.agents/`** مهاجرت داده شد:

- **مسیر قبلی:** `C:\Users\LENOVO LOQ\Desktop\projects\Kynexa-Aitco\.agent\`
- **مسیر جدید مصوب:** `C:\Users\LENOVO LOQ\Desktop\projects\Kynexa-Aitco\.agents\`
- **آمار اجزای منتقل‌شده بدون کوچک‌ترین تغییر یا حذفیات:**
  - **۱۷ عامل هوشمند سفارشی** در [`.agents/agents/`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/.agents/agents/)
  - **۱۸ مهارت تخصصی** در [`.agents/skills/`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/.agents/skills/)
  - **۹ قانون دائمی حاکمیت مهندسی** در [`.agents/rules/`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/.agents/rules/)
  - **۵ الگوی استاندارد اسناد** در [`.agents/templates/`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/.agents/templates/)
  - **کلیه مستندات رجیستری** در [`.agents/docs/`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/.agents/docs/)

---

## ۲. سرورهای فعال ثبت‌شده در فاز کنونی (Active Initial MCP Servers)

بر اساس نیازمندی فاز استقرار اولیه و به جهت حفظ تمرکز و حداقل‌سازی لایه‌های واسط، **تنها ۴ سرور کلیدی و محوری پروژه** در فایل [`.agents/mcp_config.json`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/.agents/mcp_config.json) ثبت و فعال شده‌اند:

| ردیف | نام سرور (Server ID) | ماژول اجرایی (Script Location) | پورت / ارتباط | وضعیت آزمون | ابزارهای ارائه‌شده |
| :---: | :--- | :--- | :---: | :---: | :--- |
| ۱ | `kynexa-filesystem` | [`tools/mcp/server_filesystem.py`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/tools/mcp/server_filesystem.py) | Stdio (JSON-RPC) | ✅ initialize/tools-list tested | `list_directory`, `read_file`, `search_files` |
| ۲ | `kynexa-docai` | [`tools/mcp/server_docai.py`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/tools/mcp/server_docai.py) | Stdio (JSON-RPC) | ✅ initialize/tools-list tested | `inspect_pdf`, `extract_pdf_page_text` |
| ۳ | `kynexa-eval` | [`tools/mcp/server_eval.py`](file:///c:/Users/LENOVO LOQ/Desktop/projects/Kynexa-Aitco/tools/mcp/server_eval.py) | Stdio (JSON-RPC) | ✅ initialize/tools-list tested | `compute_macro_f1`, `evaluate_confidence_routing` |
| ۴ | `kynexa-database` | [`tools/mcp/server_database.py`](file:///c:/Users/LENOVO%20LOQ/Desktop/projects/Kynexa-Aitco/tools/mcp/server_database.py) | Stdio (JSON-RPC) | ✅ initialize/tools-list tested | `inspect_schema`, `execute_read_query` |

> [!NOTE]
> سرورهای اضافی (نظیر git, docs, diagram, api, browser, security) در این فاز از فایل `mcp_config.json` خارج شدند تا پردازش سبک، متمرکز و بدون تداخل باقی بماند. ماژول‌های آن‌ها در `tools/mcp/` برای بهره‌برداری‌های اختصاصی آینده نگهداری می‌شوند.

---

## ۳. شرح فنی ۴ سرور فعال

### ۱. `kynexa-filesystem`
- **هدف:** کاوش ساختار فایل‌ها و خواندن امن کدهای پروژه.
- **انطباق حاکمیتی:** اجرای خودکار قانون `05-data-protection.md` و مسدودسازی دسترسی مستقیم به آرشیو `backup.pst` (۳٫۶۸ گیگابایتی).
- **ابزارها:**
  - `list_directory`: پیمایش پوشه‌ها با نمایش حجم و ساختار.
  - `read_file`: خواندن فایل‌ها با سقف بایت مجاز و فیلتر فایل‌های باینری و حساس.
  - `search_files`: جست‌وجوی فایل‌ها با الگوی glob در محدوده پروژه.

### ۲. `kynexa-docai`
- **هدف:** تریاژ اسناد فنی، تحلیل متادیتا و جداسازی فایل‌های پیوست ایمیل‌های صنعتی.
- **منطق تصمیم‌گیری:** تفکیک اسناد PDF دارای لایه متن دیجیتال (Digital Bypass) از اسناد اسکن‌شده نیازمند خط لوله PaddleOCR.
- **ابزارها:**
  - `inspect_pdf`: بررسی چگالی متنی، شمارش صفحات و ارزیابی استخراج‌پذیری مستقیم.
  - `extract_pdf_page_text`: استخراج متن دیجیتال از یک صفحه مشخص PDF.

### ۳. `kynexa-eval`
- **هدف:** تضمین کیفیت مدل‌ها بر مبنای سنجه مصوب در سند MDR-003.
- **ابزارها:**
  - `compute_macro_f1`: محاسبه تفکیکی Precision/Recall/F1 و سنجه کلیدی Macro-F1 (هدف MDR-003 برابر ۰٫۸۵).
  - `evaluate_confidence_routing`: تخصیص سه سطح مسیر اطمینان (Auto-Approved / HITL Review / Manager Alert).

### ۴. `kynexa-database`
- **هدف:** تعامل امن با پایگاه داده محلی SQLite (`data/kynexa_local.db`) جهت توسعه و ارزیابی آفلاین.
- **ابزارها:**
  - `inspect_schema`: دریافت ساختار جداول، ستون‌ها و شمارش رکوردها.
  - `execute_read_query`: اجرای کوئری‌های صریحاً فقط‌خواندنی (`SELECT`/`PRAGMA`) و مسدودسازی دستورات ویرایشی.

---

## ۴. مرزهای امنیتی و مهار ایزولاسیون (Project Scope Isolation)

1. **عدم دستکاری کانفیگ سراسری:** هیچ تغییری در `~/.gemini/config/` و `~/.gemini/antigravity/` ایجاد نشده است.
2. **ارتباط محلی:** تمامی سرورها به صورت Stdio JSON-RPC 2.0 و توسط مفسر پایتون محلی سیستم اجرا می‌شوند.
3. **مستقل از شبکه برای چهار سرور فعال:** سرورهای فعال اولیه در `.agents/mcp_config.json` از نظر کد اجرایی به شبکه نیاز ندارند؛ این ادعا شامل سرورهای اختیاری Plugin، وابستگی‌های خارجی PDF یا رندر Mermaid نمی‌شود.

---

## ۵. نتیجه اعتبارسنجی خودکار (Automated Verification Result)

تست صحت کارکرد ۴ سرور فعال در ترمینال:

```bash
python -c "
import subprocess, json
servers = ['server_filesystem.py', 'server_docai.py', 'server_eval.py', 'server_database.py']
for s in servers:
    p = subprocess.Popen(['python', f'tools/mcp/{s}'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, cwd='c:/Users/LENOVO LOQ/Desktop/projects/Kynexa-Aitco')
    init_cmd = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2024-11-05'}}) + '\n'
    out, _ = p.communicate(input=init_cmd, timeout=5)
    res = json.loads(out.strip())
    print(f'[PASS] {s} -> {res[\"result\"][\"serverInfo\"][\"name\"]}')
"
```

این آزمون فقط پاسخ `initialize` را بررسی می‌کند و به‌تنهایی اثبات‌کننده کشف خودکار توسط Antigravity نیست. آزمون جامع فعلی را با اجرای `python tools/mcp/verify_all_mcps.py` انجام دهید.

خروجی مورد انتظار پایه:
```text
[OK] kynexa-filesystem
[OK] kynexa-docai
[OK] kynexa-eval
[OK] kynexa-database
```
