# دفتر پیگیری موج‌ها

مالک: هماهنگ‌کننده · آخرین به‌روزرسانی: ۲۰۲۶-۱۰-۰۲ (موج ۲ آغاز شد)
مبنا: [AGENT-EXECUTION-PLAN.md](../AGENT-EXECUTION-PLAN.md) · قراردادها: [CONTRACTS.md](CONTRACTS.md)

وضعیت‌های مجاز: برنامه‌ریزی‌شده · در حال اجرا · آمادهٔ ادغام · ادغام‌شده · تأییدشده ·
نیازمند اصلاح · **تأیید جزئی**.

> «تأیید کامل» فقط وقتی مجاز است که سناریوی واقعیِ همان بسته بررسی شده باشد. برای
> بسته‌هایی که به میکروفون/صفحه نیاز دارند، تا آن زمان «تأیید جزئی» ثبت می‌شود.

---

## موج ۰ — شواهد و قراردادها

| شناسه | وضعیت | مالک | خروجی | بررسی انجام‌شده | محدودیت |
|---|---|---|---|---|---|
| B0 | تأیید جزئی | خط مبنا (فقط‌خواندنی) | [B0-orb-baseline.md](B0-orb-baseline.md) | خواندن کامل مسیر: ساخت پنجره، رسم، ناحیه، hit، موقعیت، DPI، کلمک | اعداد از کد مشفق‌اند؛ **هیچ برنامه‌ای اجرا نشد**؛ هاله و تغییر DPI سنجیده نشده |
| T0 | تأیید جزئی | خط مبنا (فقط‌خواندنی) | [T0-text-baseline.md](T0-text-baseline.md) + [fixtures](../../voice-ptt/tests/fixtures/text-baseline/) | ۱۷ نمونه از **اجرای واقعی** API عمومی | نمونهٔ موتور واقعی و تزریق واقعی سنجیده نشده |
| S0 | تأیید جزئی | خط مبنا (فقط‌خواندنی) | [S0-session-baseline.md](S0-session-baseline.md) | نقشهٔ رویداد→اثر→emit؛ جست‌وجوی کامل نبودِ مقصد | لغو-حین-پردازش از ترتیب کد است، **آزمایش نشده** |
| K0/I0 | آمادهٔ ادغام | هماهنگ‌کننده | [CONTRACTS.md](CONTRACTS.md) | ۹ قرارداد با وضعیت «موجود/پیشنهاد» و محل ثبت | هفت قرارداد هنوز در کد نیستند |

**خروجی موج ۰: قابل‌استفاده، اما فقط برای تصمیم‌گیری.** یافته‌های واقعی که به موج‌های بعد
قفل می‌شوند:

| یافته | بستهٔ بعدی که درگیر است | محل |
|---|---|---|
| حلقهٔ مردهٔ کلیک (۲۷٫۹ pt بین hit و ناحیه) | `O1` — **بسته شد** | [O1 بند ۱ و ۳](O1-handoff.md) |
| تغییر DPI یک resize واقعی است | `O1` — باز | [B0 بند ۶-۲](B0-orb-baseline.md) |
| `invalidate_click_region` هیچ فراخوان تولیدی ندارد | `O1` — **بسته شد** | [O1 بند ۴](O1-handoff.md) |
| «کلمات»→«کلم‌ات»، «تمام»→«تم‌ام»، «میکروفون»→«می‌کروفون» | `T1` — **بسته شد** | [T1 بند ۲](T1-handoff.md) |
| هیچ حالت «خام» و هیچ گزینهٔ متنی در تنظیمات | `T1` — **بسته شد** (`[text] mode`) | [T1 بند ۳](T1-handoff.md) |
| **هیچ بررسی مقصدی در کل `src/`** | `T2` (و `V1`, `R1`, `U1`) | [S0 بند ۴](S0-session-baseline.md) |
| لغو حین پردازش بی‌اثر است | `S1` + `T2` | [S0 بند ۵](S0-session-baseline.md) |
| تاریخچه فقط در حافظه است | `V1` | [S0 بند ۷](S0-session-baseline.md) |

---

## مالکیت فایل (فعلی)

| بسته | محدوده | وضعت مالکیت |
|---|---|---|
| B0 | `docs/execution/B0-orb-baseline.md` | تحویل شد |
| T0 | `docs/execution/T0-text-baseline.md` + `voice-ptt/tests/fixtures/text-baseline/` | تحویل شد |
| S0 | `docs/execution/S0-session-baseline.md` | تحویل شد |
| K0/I0 | `docs/execution/CONTRACTS.md` + `STATUS.md` + `docs/INDEX.md` | تحویل شد |

### موج ۱ — ثبت مالکیت فایل (پیش از شروع، مطابق بند ۴ برنامهٔ اجرا)

| بسته | فایل‌هایی که صاحبشان است | فایل‌هایی که **فقط** با اجازهٔ هماهنگ‌کننده | گزارش |
|---|---|---|---|
| `S1` | `state/session.rs`، `state/utterance.rs` | `state/machine.rs`، `state/status.rs`، شنوندهٔ کلید | `docs/execution/S1-handoff.md` |
| `O1` | `gui/orb.rs`، `gui/orb_animation.rs`، `gui/window_shape.rs`، `gui/bootstrap.rs` | `gui/overlay.rs`، ثبت ماژول‌ها | `docs/execution/O1-handoff.md` |
| `T1` | `processing/normalizer.rs` و ماژول‌های اختصاصی سیاست متن | `processing/mod.rs`، `processing/dictionary.rs`، تنظیمات، `machine.rs` | `docs/execution/T1-handoff.md` |

قاعده‌ای که این جدول را الزامی می‌کند: کار در فایل متفاوت به‌تنهایی کافی نیست. اگر
قراردادی که مصرف‌کننده رویش حساب می‌کند هنوز تغییر می‌کند، مصرف‌کننده منتظر می‌ماند.
`S1` قرارداد جلسه/قطعه را **تولید** می‌کند، پس `O1` و `T1` نباید به آن تکیه کنند تا
امضایش ثبت شود.

### پیشرفت موج ۱

| بسته | وضعیت | تست | کاناری | گزارش |
|---|---|---|---|---|
| `S1` | **آمادهٔ ادغام** (تأیید جزئی) | ۳۴۷ پاس (۳۳۹ → ۳۴۷) | ۱۷/۱۷ CAUGHT | [S1-handoff.md](S1-handoff.md) |
| `O1` | **آمادهٔ ادغام** (تأیید جزئی) | ۳۵۴ پاس (۳۴۷ → ۳۵۴) | ۲۱/۲۱ CAUGHT | [O1-handoff.md](O1-handoff.md) |
| `T1` | **آمادهٔ ادغام** (تأیید جزئی) | ۳۶۲ پاس (۳۵۴ → ۳۶۲) | ۲۵/۲۵ CAUGHT | [T1-handoff.md](T1-handoff.md) |

`S1` دو تصمیمِ متن قرارداد را اصلاح کرد (دو حالت بستن به‌جای سه، و دو حالت
`SessionKind` به‌جای سه) چون پیاده‌سازی نشان داد متن جلوتر از واقعیت نوشته شده بود؛ هر
دو در [CONTRACTS.md بند ۱](CONTRACTS.md) اصلاح شد.

`O1` حلقهٔ مردهٔ کلیک را بست: هدفِ کلیک و ناحیهٔ ویندوز حالا از **یک** تابع می‌آیند
(`interaction_radius_pt`)، پس عرض حلقه در همهٔ حالت‌ها صفر است و کار به `overlay.rs`
نکشید. کشِ ناحیه دیگر تنها منبع حقیقت نیست و از خودِ پنجره پرسیده می‌شود
(`GetWindowRgnBox`) — تنها راهِ پوششِ بازسازیِ پنجره پشت همان مقدار HWND. محدودیت: رفتارِ
روی صفحه هنوز با ماوس آزموده نشده؛ چهار مرحلهٔ دستی در [O1-handoff.md بند ۶](O1-handoff.md)
نوشته شده است و بیلدِ راستی‌آزماییِ آن آماده است.

---

## تصمیم‌های قطعی کاربر (۲۰۲۶-۱۰-۰۱)

هر شش تصمیم بسته شد. ستون «اثر» می‌گوید کدام بخش پیاده‌سازی آزاد شد و کدام بخش از
موج ۱ **متوقف نیست** — قاعده: هر تصمیم فقط بخش وابسته به خودش را متوقف می‌کند.

| # | تصمیم | اثر روی قرارداد | آزاد شد |
|---|---|---|---|
| ۱ | `می روم` با فاصلهٔ صریح ⇒ فاصله حفظ شود | [CONTRACTS](CONTRACTS.md) بند ۵، قاعدهٔ ۲ | `T1` |
| ۲ | همزهٔ تنها (`مسأله`) ⇒ **حفظ شود** | بند ۵، قاعدهٔ ۲ | `T1` |
| ۳ | ارقام لاتین ⇒ حفظ؛ تبدیل فارسی گزینهٔ مستقل | بند ۵ (`NumberStyle`) | `T1` |
| ۴ | **صدا نگه داشته نمی‌شود** — بافر فقط تا پایان تبدیل جاری، بازیابی فقط متن | بند ۹ بازنویسی شد | `R1` با دامنهٔ کوچک‌تر |
| ۵ | تاریخچه فعلاً در حافظه؛ دیسک اختیاری | بند ۶ | `V1` و `D1` بدون وابستگی به ماندگاری |
| ۶ | مقصد = همان پنجره و همان فرایند، با اعلام محدودیت | بند ۲، قاعدهٔ ۴ | `T2` |

### آنچه این تصمیم‌ها عمداً متوقف نمی‌کنند

بررسی هندسهٔ اورب (`O1`)، اصلاح شناسه و لغو جلسه (`S1`) و پردازش محافظه‌کارانه (`T1`)
هیچ‌کدام به ذخیرهٔ تاریخچه، سبک ارقام یا نگه‌داری صدا وابسته نیستند. تنها چیزی که
متوقف است، بخشی از `R1` است که «تلاش مجددِ تبدیل» می‌خواست — که طبق تصمیم ۴ دیگر
اصلاً ساخته نمی‌شود.

### اصلاح‌هایی که این بازبینی به قراردادها وارد کرد

1. **پایانِ ضبط ≠ پایانِ جلسه.** `SessionPhase` اضافه شد (`Recording` / `AwaitingResult` /
   `Completed` / `Cancelled`) تا متنِ قطعهٔ پایانی که طبق تعریف بعد از رهاکردن کلید
   می‌آید، «پاسخ دیررسِ جلسهٔ بسته» قلم نخورد و **خروجی طبیعی رد نشود**.
2. **نتیجهٔ درج یک‌دست شد.** حالت `Rejected` که در بند ۱ ارجاع داده شده بود و در تعریف
   وجود نداشت حذف شد؛ جای آن `NotAttempted { reason }` با دلایل نام‌دار آمد. و
   `PartialThenFailed` که زیر `Failed` (یعنی «هیچ‌چیز نرفت») بود، به `Partial` منتقل شد
   چون معنایش «بخشی رفت و بعد شکست خورد» است.
3. **بند ۹ از نگه‌داری صدا خالی شد** و به «بازیابیِ متنِ درج‌نشده» محدود شد؛ پرامپت `R1` در
   [AGENT-EXECUTION-PLAN.md](../AGENT-EXECUTION-PLAN.md) هم بازنویسی و سطرِ مالکیتش اصلاح شد،
   تا دو نویسنده با دامنهٔ متفاوت شروع نکنند.
4. **بند ۶-۲ گزارش B0** از «همان مسیری است که هاله را می‌سازد» به «فرضیهٔ آزمون‌پذیر»
   پایین آمد؛ آنچه قطعی است فقط خودِ resize است، نه اثرش روی پیکسل‌ها.
5. **بند ۶-2 یک نشتی افشاگری را یادآوری می‌کند:** چون صدا نگه داشته نمی‌شود، «تلاش مجدد
   با موتور دیگر» در این نسخه معنا ندارد و باید از متن رابط حذف شود، نه اینکه کاربر
   دنبالش بگردد و پیدا نکند.
6. **ادعای ناهماهنگی wgpu از ادعای «نبودِ فرمانِ اپ» جدا شد.** اینکه اپ
   `ViewportCommand::InnerSize` را دوباره نمی‌فرستد، از کد قطعی است؛ اینکه سازوکار داخلیِ
   winit/eframe اندازهٔ سطح رندر را هماهنگ نمی‌کند **نیازمند بررسی** است و در [B0 بند ۶-۲](B0-orb-baseline.md)
   به‌عنوان سه ادعای جدا شماره‌گذاری شده.
7. **ترتیب بین‌جلسه‌ای به زمان پاسخ وابسته نیست.** نتیجه‌ها در صفی با کلید
   `(ترتیب شروع جلسه، شمارهٔ قطعه)` می‌نشینند؛ نتیجه‌ای که زودتر برسد صف را دور نمی‌زند.
   حفظ متن اولویت دارد ولی مجوز درج خارج از ترتیب نیست.
8. **«نه بافر» به «نه بافر صوتی برای بازیابی» تغییر کرد.** بافر موقتِ ضبط و تبدیلِ جاری
   لازم است. همچنین `std::mem::take` **حذف نیست، انتقال مالکیت است**؛ شاهد نبودِ
   نگه‌داری، نبودِ هر میدان/فایل/مجموعه‌ای است که صوت را فراتر از همان فراخوانی نگه دارد.

---

## موج ۱ — بسته شد

هر سه بسته **آمادهٔ ادغام با تأیید جزئی** هستند. «تأیید جزئی» یعنی: از کد و تست قطعی است،
از صفحه و میکروفون نه.

| بسته | چه چیزی تغییر کرد | چه چیزی هنوز باید روی صفحه آزموده شود |
|---|---|---|
| `S1` | هر دیکته شناسه گرفت؛ لغو بی‌اثر شد | لغو حین پردازش (نیازمند دیکته و میکروفون) |
| `O1` | حلقهٔ مردهٔ کلیک صفر pt؛ کشِ ناحیه از خودِ پنجره پرسیده می‌شود | چهار مرحلهٔ کلیک در [O1-handoff.md بند ۶](O1-handoff.md) — بیلدِ آماده در [بند ۹](O1-handoff.md) |
| `T1` | `[text] mode` با سه حالت؛ `ات`/`ام`/`اش` از قاعدهٔ نیم‌فاصله حذف شدند | تزریق در برنامهٔ واقعی |

### مرزهایی که از برنامه رد شد (با اجازهٔ هماهنگ‌کننده)

`T1` دو فایلِ غیرمالک را لازم داشت و این‌بار خودِ هماهنگ‌کننده اجازه داد:

| فایل | تغییر | جای ثبت |
|---|---|---|
| `config/settings.rs` | بخش `[text]` | [T1 بند ۵](T1-handoff.md) |
| `state/machine.rs` | دو فراخوانی `process_text` → `process_text_with` | [T1 بند ۵](T1-handoff.md) |

`O1` برخلاف آن **به `overlay.rs` نکشید**: با بردن هر دو شعاع به یک تابع، مسئله در `orb.rs`
تمام شد.

### شکاف‌هایی که به‌جای پنهان‌کردن، ثبت شدند

| شکاف | چرا کاناری ندارد | کجا |
|---|---|---|
| مرز درج در `machine::emit` | فقط با صوت و موتور زنده اجرا می‌شود | [C18](mutation-check-session.sh) |
| اینکه `machine.rs` تنظیم `[text]` را در اجرا می‌خواند | همان مسیر زنده | [C-T1](T1-handoff.md) بند ۷ |
| ناسازگاری سطح رندر با پنجره پس از تغییر DPI | نیازمند دو نمایشگر | [B0 بند ۶-۲](B0-orb-baseline.md) |

### بیلد راستی‌آزمایی (۲۰۲۶-۱۰-۰۲)

برای اینکه تست دستی `O1` روی کدِ کهنه انجام نشود، یک بیلد ریلیز تازه ساخته و **هویتش
اثبات** شد — جزئیات کامل در [O1-handoff.md بند ۹](O1-handoff.md).

| | |
|---|---|
| باینری | `voice-ptt-dist/voice-ptt-O1.exe` (۳۷٬۷۹۳٬۷۹۲ بایت) · نسخهٔ ۲۹ سپتامبر دست‌نخورده ماند |
| اثبات | رشتهٔ `GetWindowRgnBox` در این exe هست و در هر دو نسخهٔ قدیمی نیست |
| مانع | قفلِ تک‌نمونه‌ای یعنی نسخهٔ نصب‌شده باید بسته باشد؛ میان‌برِ همیشگی همان کدِ بدون اصلاح را بالا می‌آورد |

سه نکته که حین آماده‌سازی بیرون آمد و در سند ثبت شد:

1. مختصاتِ orb که در سند نوشته شده بود (`1813, 257`) **غلط** بود و با کانفیگِ واقعی
   (`1113, 107`) نمی‌خواند؛ تست دستی با عددِ سند بیرون orb می‌افتاد و تقصیر را گردنِ اصلاح
   می‌انداخت. اعدادِ بند ۶ اصلاح و هر دو واحد (pt و px) کنار هم ثبت شدند.
2. `[text]` در کانفیگِ واقعیِ کاربر **نیست** و لازم هم نیست: هر دو سطح `Settings` و
   `TextSettings` با `serde(default)` محافظت شده‌اند و `deny_unknown_fields` وجود ندارد، پس
   `T1` هم بیلد تازه را نمی‌شکند و `standard` را می‌گیرد.
3. کانفیگِ کاربر یک **کلید API زنده** دارد. ریپو بررسی شد و کلیدی هم‌شکل در آن نیست و خودِ
   کانفیگ هم داخل ریپو نیست، ولی قاعده ثبت شد: از کانفیگ فقط یک خط خوانده می‌شود، نه کل فایل
   ([O1 بند ۱۱](O1-handoff.md)).

### نتیجهٔ تست دستی (۲۰۲۶-۱۰-۰۲) — وضعیت در آن زمان: باز، با شاهدِ تازه

| گزارش کاربر | سنجیده | نتیجه |
|---|---|---|
| «خیلی بهتر شده» | ناحیهٔ کلیکِ پنجرهٔ زنده = **۶۸ px = ۵۴٫۴۰ pt** (پروب ۵/۵) | هم‌خوان با `Idle` ۵۴٫۲۹؛ پیش از `O1` ۳۳٫۰ بود ⇒ انطباق هندسی روی کد نشان‌دهندهٔ بهبود است، اما ادعای صفر بودن قطعی حلقه بدون آزمون دستی روی صفحه نامعتبر است؛ **O1 همچنان تأیید جزئی و نیازمند آزمون دستی با ماوس است** |
| «هنوز حلقه‌ای هست که کلیکش بی‌اثر است» | زنجیرهٔ کلیک در کد: هر کلیکِ داخلِ ناحیه به `BeginRecording` می‌رسد | ادعای قطعی دربارهٔ یکی نبودن حلقه منتفی است؛ وضعیت نیازمند ثبت لاگ شعاع و آزمون دستی با ماوس روی صفحه است |
| چهار مرحلهٔ بند ۶ | انجام نشد (هدف‌گیری روی پیکسل ممکن نبود) | **آزموده‌نشده** |

آنچه مانعِ بستن پرونده است: هیچ‌جای برنامه ثبت نمی‌کند که کلیکی در چه شعاعی و چه حالتی
رخ داده، و لاگ هم کمکی نمی‌کند چون `SessionKind` از latch می‌آید نه از منبع کلیک. گامِ
ارزانِ بستن: یک خط لاگ در [overlay.rs:1119](../../voice-ptt/src/gui/overlay.rs) که شعاع کلیک
و `AppState` را چاپ کند. تا آن، وضعیت `O1` همچنان **تأیید جزئی** است و موج ۲ شروع می‌شود.

**قاعده‌ای که از این درآمد و برای موج‌های بعدی هم برقرار است:** راستی‌آزمایی دستی سه پیش‌شرط
دارد — کدام باینری باز است، مختصات از کانفیغِ همان اجرا خوانده شود، و وجودِ تغییر در خودِ
فایلِ اجراشونده ثابت شود. «ابزار سالم ≠ نتیجهٔ معتبر» و تست دستی هم ابزار است
([O1 بند ۱۰](O1-handoff.md)).

## تصمیم‌های هماهنگ‌کننده (۲۰۲۶-۱۰-۰۲) — ثبت‌شده در [گزارش](COORDINATOR-BRIEF-2026-10-02.md)

| # | تصمیم | اثر |
|---|---|---|
| ۱ | `state/machine.rs` **مجاز** با دامنهٔ اتصال مقصد، لغو، و مدیریت خروجی | `T2` دیگر کدِ بی‌اثر نمی‌ماند |
| ۲ | لاگ کلیک اورب: موقعیت، حالت، اندازهٔ ناحیه، عملِ انجام‌شده | **انجام شد** — [O1 بند ۶](O1-handoff.md) |
| ۳ | `O1` تأیید جزئی بماند؛ اندازه‌گیریِ ناحیه جای آزمون کامل کلیک را نگیرد | پرونده باز |
| ۴ | push پس از یک مرحلهٔ منسجم، نه به‌ازای هر کامیت | — |
| ۵ | ترتیب `T2`: لغو هنگام تبدیل ← مقصد و نتیجهٔ درج ← صف و فاصله | ترتیب قبلی عوض شد |
| ۶ | تاریخچه بازنویسی نشود؛ فوتر از اینجا رعایت شود | ۹ کامیت میانی بدون فوتر می‌مانند |

دو ادعای گزارش اولیه اصلاح شد:

1. «از کد ثابت شد این حلقه **نمی‌تواند** همان حلقهٔ مرده باشد» **ضعیف‌تر شد.** کد فقط
   ثابت می‌کند از نقطهٔ `clicked` به بعد شاخهٔ بی‌اثری وجود ندارد — نه اینکه رویدادِ کلیک
   به رابط می‌رسد، نه اینکه مستطیل `ui.interact` همان ناحیه است، و نه اینکه `egui` کلیک
   را تشخیص می‌دهد. تطبیق هندسه شاهد خوبی است، نه اثبات.
2. عدد ۲۷٫۹ از ابتدا **نقطه** بود و در هیچ سندی پیکسل نبود
   ([B0 بند ۱](B0-orb-baseline.md))؛ فقط واحد هر دو جا نوشته شد.

سه مورد بازِ متنی هم در کد بررسی و تأیید شد: حذف همزه هنوز بی‌قید است
([normalizer.rs:19](../../voice-ptt/src/processing/normalizer.rs) — `مسأله`→`مساله`، یعنی
تصمیم ۲ کاربر هنوز پیاده نشده)؛ حالت `Raw` متن را دست‌نخورده برمی‌گرداند ولی درزِ قطعه
**بعد** از آن اجرا می‌شود ([machine.rs:576](../../voice-ptt/src/state/machine.rs)) پس «خام»
کاملاً خام نیست؛ و `inject_text` فقط `Result<usize>` می‌دهد، پس «درج ناقص» نتیجه‌ای
نوع‌دار ندارد.

## موج ۲ — وضعیت تاریخی و وضعیت فعلی (`T2`)

### وضعیت در ۲۰۲۶-۱۰-۰۲ (تاریخی — وضعیت در آن زمان)

| بسته | وضعیت در آن زمان | تست | کاناری | گزارش |
|---|---|---|---|---|
| `T2` | **در حال اجرا** — قطعهٔ ۱، ۲ و ۳ از ۴ (نشانگر/پایانِ منبع/شمارهٔ پنجره/زمان‌بندیِ ضربان + مقصدِ جلسه‌محور و نتیجهٔ درج) | ۴۲۸ پاس (۴۱۲ در lib)، ۰ شکست، ۲ نادیده | ۲۶/۲۶ CAUGHT در اسکریپتِ حلقه | [T2-handoff.md](T2-handoff.md) |

### وضعیت فعلی (۲۰۲۶-۱۰-۰۴ — بر اساس پیاده‌سازی و ممیزی)

| مؤلفه / قطعه | وضعیت فعلی | شواهد و جزئیات پیاده‌سازی | آزمون‌ها |
|---|---|---|---|
| **مقصد (Destination)** | **تأییدشده در آزمون** | هویت `TargetIdentity` و اعتبار `TargetValidity` تفکیک‌شده بر پایهٔ `hwnd` و `pid` (رد مقصد نامعلوم یا جابه‌جاشده پیش از درج) | ۸ سناریوی آزموده در هماهنگ‌کننده |
| **نتیجهٔ نوع‌دار درج** | **تأییدشده در آزمون** | بازنویسی خروجی درج به صورت ساختار نوع‌دار `Injection { accepted, attempted, stopped }` بر حسب رویدادهای UTF-16 و تاش‌شدن به ۴ حالت `Complete / Partial / Failed / NotAttempted` با `judge_insert` | آزمون‌های واحد اختصاصی تزریق و مهار خطا |
| **مرز قطعات (Boundary)** | **تأییدشده در آزمون** | ماژول مستقل `processing/boundary.rs` (`BoundaryTracker`)؛ اعتبارسنجی بر پایهٔ `hwnd/pid` و ابطال حافظه با تغییر پنجره/لغو؛ حفظ فاصله‌ها و خطوط جدید در Raw. **دقت در ادعا:** خودِ مرز هرگز Backspace تولید نمی‌کند — تنها یک فاصلهٔ واحد اضافه می‌کند، و وقتی `backspaces > 0` است (ترمیمِ واژه درون‌جلسه‌ای که درز تعیین می‌کند) عمداً فاصله نمی‌گذارد تا واژهٔ ترمیم‌شده دو تکه نشود. ترمیمِ حدسیِ Backspace در `processing/seam.rs` همچنان وجود دارد و فقط وقتی غیرفعال می‌شود که قطعهٔ پیشین با فاصلهٔ انتهایی تمام شده باشد. | آزمون‌های هماهنگ‌کننده و واحد مرز |
| **نرمال‌ساز و پردازش متنی** | **تأییدشده در آزمون و ممیزی** | حفظ همزه (أ و ؤ)؛ حذف فهرست منفی استثناها و تکیه بر پایه‌های مثبت محدود (`NOUN_STEMS_FOR_HA`)؛ تطبیق کامل بن + شناسه و رد بخش میانی ناشناخته (مانند میرونام)؛ جداسازی و حفظ علائم نگارشی ابتدا/انتها | ۷۲ آزمون در ماژول processing |
| **نگه‌داری رکوردهای اقدام‌لازم (`kept`)** | **پیاده‌سازی‌شده در حافظه** | راه خواندن کدی رکوردهای ارسال‌نشده (`kept_handle()` و `kept_records()`) برای آزمون و مصرف‌کنندهٔ آینده فراهم است؛ رابط کاربری یا API بازیابی هنوز ساخته نشده است. | آزمون‌های بقای رکورد پس از تکمیل |
| **زمان‌بندی ضربان (Heartbeat)** | **تأییدشده در آزمون** | پیاده‌سازی بر اساس فرمول حسابی صریح `std::cmp::max(at + period, now + period)` در تابع `next_beat_at` جهت جلوگیری از انباشت ضربان‌ها پس از توقف طولانی؛ بدون ادعای اثبات‌نشده مبنی بر انطباق کامل با رفتار درونی کتابخانه‌ها. | آزمون حسابی دیرکرد ضربان |
| **کل کتابخانه (`voice-ptt`)** | **سبز پایدار** | ۴۶۸ آزمون واحد موفق، ۱۶ آزمون ادغامی، ۲ نادیده (زنده)، ۰ شکست؛ Clippy کد ۰؛ قالب‌بندی فایل‌های بسته تمیز. | ۴۸۴ آزمون کل |

قطعهٔ ۱ یعنی **مقصد**: `output/target.rs` با `TargetIdentity` / `TargetValidity` /
`TargetTracker`. بند ۲ قرارداد این را «پیش از موج ۲» لازم می‌دانست و تا حالا در کل `src/`
نبود — حالا تصمیم‌اش یک تابع خالص است و جدولش بدون نشست دسکتاپ تست می‌شود.

قطعهٔ ۲ یعنی **دریافت رویداد و لغو هنگام تبدیل**: `state/coordinator.rs` تنها حلقهٔ
برنامه است و تنها جایی که اثری اعمال می‌شود؛ `state/machine.rs` به نیمهٔ سخت‌افزار
(میکروفون + VAD به‌صورت دادهٔ خام) کوچک شد. بیست‌وهفت سناریو اجرا می‌شوند — ۲۳ تا روی **همان** حلقه و چهار تا روی
تولیدکنندهٔ رویدادِ `machine.rs` (بدون میکروفون، با ساعتِ مجازی) — با میکروفون، موتور و صفحه‌کلید ساختگی،
بدون پیاده‌سازیِ دومِ مخصوص تست. این
قطعه شکافِ بازِ [S0 بند ۵](S0-session-baseline.md) را می‌بندد.

ادامهٔ همین قطعه — ترتیبِ پایانِ بی‌صوت، درزِ وابسته به `SessionId`، مالکیت نشانگر،
پنجرهٔ خطا به‌عنوان رویداد حلقه، و صفِ تک‌به‌تک — در [T2-handoff.md](T2-handoff.md)
بند ۴ب آمده: ۸ سناریوی تازه، ۶ جهش تازه و دو شکافِ ثبت‌شده؛ و بند ۴ج ادامهٔ همان
قطعه: نشانگرِ hands-free که از قاعده می‌آید نه از اثر، پایانِ کانالِ رویداد که صریحاً به توقف
می‌رسد، و دو خطای دقیقاً هم‌متن که شمارهٔ پنجره را از «شاید لازم شود» به تصمیمِ آزموده تبدیل
کرد — ۴ سناریو و ۴ جهش تازه (۱۵/۱۵ CAUGHT). ادعاهای مطلقِ «هیچ تستی نمی‌تواند» از اسناد
برداشته شد؛ برای شاخه‌های آزموده‌نشده نوشته شده «در این تحویل آزموده نشده». و بند ۴د زمان‌بندیِ
ضربان را اصلاح کرد: مهلت دیگر با ساختِ future جابه‌جا نمی‌شود و قاعدهٔ حسابی دیرکرد (`max(at + period, now + period)`)
جداگانه آزموده شده است (۱۷/۱۷ CAUGHT؛ نیمهٔ «توقفِ طولانی» فقط حسابِ خالص است).

قطعهٔ ۳ یعنی **مقصدِ جلسه‌محور و نتیجهٔ صادقانهٔ درج**: `TargetPort` تازه (ثبت در شروعِ
ضبط، اعتبارسنجی یک بار درست پیش از اولین کلید)، یک مقصد برای هر `SessionId` — نه یک
`TargetTracker` مشترک، چون پاسخِ یک دیکتهٔ قدیمی می‌تواند بعد از شروعِ دیکتهٔ بعدی برسد
— و `inject_*` که به‌جای `Result<usize>` یک `Injection` {`accepted`, `attempted`,
`stopped`} می‌دهد و `judge_insert` آن را به چهار حالتِ `Complete / Partial / Failed /
NotAttempted` تاش می‌کند. مقدارِ گزارش‌شده **جفتِ رویدادِ پذیرفته‌شده** است، نه نویسهٔ
تایپ‌شده: `SendInput` پذیرش را می‌گوید، نه رسیدن به سند. هشت سناریوی تازه روی **همان**
حلقه (از جمله دو جلسه با دو مقصد، و درجِ ناموفقی که حافظهٔ درز را می‌کُشد تا قطعهٔ بعدی
متنِ کاربر را حذف نکند) و **۲۶ از ۲۶ CAUGHT**. بند ۴ه در [T2-handoff.md](T2-handoff.md).

آنچه هنوز باز است و ثبت شده: `kept` (متنِ ارسال‌نشده) راه خواندن کدی دارد (`kept_handle` / `kept_records`)،
اما رابط کاربری یا API بازیابی هنوز ساخته نشده است؛ و رفتارِ واقعیِ `SendInput` روی این ماشین اجرا نشده
چون ساختِ exe و اجرای زنده ممنوع است — همهٔ این اعداد از آزمونِ ساختگی‌اند.

سیاستِ مرز و نرمال‌ساز فارسی پیاده‌سازی و در ممیزی تأیید شد. صف بین‌جلسه‌ای
از فهرست لازم خارج شد: ترتیبِ درون‌جلسه‌ای با شمارهٔ تحویل حل شد و صف عمومی چیزی به آن
اضافه نمی‌کند.

### بستهٔ اصلاح نرمال‌ساز فارسی و پالایش املایی (تأییدشده در ممیزی)

چهار اصلاح مشخص در خط پردازش متن پیاده‌سازی و آزموده شد:
1. **حفظ همزه و رفع افت اطلاعات:** حذف نگاشت‌های حذف‌کنندهٔ `أ` و `ؤ` از جدول تبدیل عربی؛ همزه در هر دو حالت پیش‌فرض (Standard) و محافظه‌کارانه (Conservative) حفظ می‌شود. پروندهٔ خط مبنای `T0-009` برای واژهٔ «مسأله» به `OK` ارتقا یافت.
2. **حذف فهرست منفی و کاربرد پایه‌های مثبتِ محدود برای جمع:** فهرست سیاه استثناها (`is_inherent_ha_word`) کاملاً حذف شد و نیم‌فاصلهٔ «ها/های/هایی» به پایه‌های مثبت اسمیِ محدود (`NOUN_STEMS_FOR_HA`) مقید گردید. واژه‌های ریشه‌ای نظیر «اژدها»، «تنها»، «اشتها»، «انتها» بدون بلک‌لیست سالم می‌مانند.
3. **تطبیق کامل بن + شناسهٔ صرفی مجاز و رد بخش میانی ناشناخته:** تفکیک بن‌های مضارع و ماضی؛ کلمه تنها در صورتی فعل متصرف با `می/نمی` شناخته می‌شود که پس از بن، دقیقاً یک شناسهٔ مجاز قرار گیرد. هرگونه بخش میانی ناشناخته (مانند نمونهٔ منفی ساختگی «میرونام») اکیداً رد شده و دست‌نخورده حفظ می‌شود.
4. **جداسازی و حفظ علائم ابتدا و انتهای واژه:** علائم نگارشی نظیر گیومه و پرانتز پیش از تحلیل هستهٔ واژه جدا شده و پس از اصلاح، در موقعیت اصلی حفظ می‌شوند (`«کتابها»` → `«کتاب‌ها»`).

**آمار آزمون‌ها:** ۴۶۸ آزمون واحد موفق (`cargo test --lib`، بدون شکست و بدون نادیده)، ۱۶ آزمون ادغامی موفق، ۲ آزمون محیط زنده نادیده (`#[ignore]`)، در مجموع ۴۸۴ آزمون سبز. کاناری هماهنگ‌کننده: ۲۶ از ۲۶ لنگر منطبق.

**محدودیت صریح ثبت‌شده:** پوشش قواعد نیم‌فاصله صرفاً بر پایهٔ شواهد مثبتِ شناسایی‌شده عمل می‌کند و هیچ ادعایی مبنی بر «تضمین سلامت همهٔ کلمات زبان فارسی» وجود ندارد. در غیاب شواهد مثبت، واژه بدون تغییر حفظ می‌شود.

### گام بعدی

موج ۲ (`T2` بررسیِ مقصد + نتیجهٔ درج) پیش‌نیازِ `R1` است، چون قرارداد بند ۲
می‌گوید متن فقط به همان پنجره‌ای می‌رود که کاربر در آن بوده. `V1`/`D1` و `P1`/`P2`
مستقل‌اند و به قرارداد تازه‌ای وابسته نیستند.
---

## موج ۳ — اتصالِ قابلیت‌های تحویل‌شده به محصول (۲۰۲۶-۱۰-۰۴)

موج‌های قبل چند ماژول را تحویل دادند که **هیچ مسیر تولیدی‌ای به آن‌ها نداشت**:
کد وجود داشت، تست‌ها سبز بودند، و هیچ‌کس نمی‌توانست از آن‌ها استفاده کند. این موج
آن‌ها را به رابط کاربری و به چرخهٔ اجرا وصل کرد.

| شناسه | وضعیت | اتصال انجام‌شده | شواهد |
|---|---|---|---|
| M1 | تأیید جزئی | `MicUseGate`/`LiveMicGate` واقعی شد و به `OverlayApp` و `StatusChannel` وصل شد؛ پنل «تست میکروفون» به‌عنوان تب داشبورد اضافه شد | `cargo test`: سبز؛ کلیپی `-D warnings`: سبز |
| O2 | تأیید جزئی | سیاست idle-return به حرکت واقعی اورب وصل شد (`orb_idle_adapter` + مانیتور/ناحیهٔ کاری در `window_shape`)؛ چهار تنظیم در پنل تنظیمات | همان |
| A1 | تأیید جزئی | `WindowsCredentialStore` به Credential Manager واقعی وصل شد؛ `migrate_on_startup` در استارتاپ اجرا می‌شود؛ planner/doctor/store منبع کلید را می‌بینند | تست انتها‌به‑انتها روی سیستم واقعی (پایین) |

### A1 — سه باگ واقعی که فقط اجرای زنده پیدا کرد

مهاجرت کلید از `config.toml` به Credential Manager نوشته شده بود و همهٔ تست‌های
ساختگی‌اش سبز بودند. اجرای باینری واقعی سه نقص نشان داد که **هیچ‌کدام با mock
قابل کشف نبودند**:

1. **HRESULT به‌جای کد Win32.** `err.code().0` یک HRESULT است
   (`0x80070000 | code`)، نه کد Win32. بنابراین `ERROR_NOT_FOUND` (۱۱۶۸) با
   ثابت خامش مطابقت نمی‌کرد و «هنوز ذخیره نشده» به‌صورت `OsError` مبهم برمی‌گشت.
   **نتیجه:** روی هر ماشینی که هنوز کلیدی ذخیره نکرده بود — یعنی دقیقاً ماشین‌هایی
   که این قابلیت برایشان ساخته شده — مهاجرت استور را «خراب» تشخیص می‌داد، رد می‌شد،
   و کلید برای همیشهplaintext می‌ماند. اصلاح: باز کردن HRESULT به کد Win32.
2. **اختلاف رمزگذاری بین `save` و `load`.** `save` با UTF-8 می‌نوشت و `load` همان
   بایت‌ها را UTF-16 می‌خواند. راستی‌آزماییِ بازخوانی هرگز مطابقت نمی‌کرد، پس
   مهاجرت همیشه «ناسازگاری» گزارش می‌کرد و **کلید را از فایل پاک نمی‌کرد**.
   اصلاح: خواندن به‌عنوان UTF-8 (هم‌راستا با نویسنده)، با fallback به UTF-16 فقط
   برای اعتبارسنامه‌هایی که ابزار دیگری نوشته.
3. **مهاجرت اصلاً به استارتاپ وصل نبود.** تابع وجود داشت و تست داشت، ولی هیچ
   فراخوان تولیدی نداشت. حالا بلافاصله پس از بارگذاری تنظیمات اجرا می‌شود و پس از
   مهاجرت، کلید از نسخهٔ درون‌حافظه‌ای تنظیمات هم پاک می‌شود تا ذخیرهٔ بعدی آن را
   دوباره روی دیسک ننویسد.

`--doctor` عمداً مهاجرت را **اجرا نمی‌کند** (بازنویسی فایل کلید کاربر در پاسخ به
پرسیدن «چه اشکالی دارد؟» زمان اشتباهی است) و به‌جای آن مستقیم از خود استور
می‌پرسد؛ وگرنه برای کلیدی که در استور است «کلیدی یافت نشد» گزارش می‌داد.

**تست انتها‑به‑انتها روی سیستم واقعی:** یک کلید آزمایشی در `config.toml` گذاشته شد،
استور خالی شد، برنامه اجرا شد. لاگ: `migrated from config.toml to Credential
Manager`؛ `api_key` در فایل خالی شد؛ اعتبارنامه در `cmdkey /list` ظاهر شد؛ اجرای
بعدی `--doctor` گزارش داد `cloud key from Windows Credential Manager`. سپس کلید
آزمایشی از استور و فایل پاک شد.

### محدودیت‌های ثبت‌شده

- تزریق واقعی متن (`SendInput`) هنوز روی سند واقعی آزموده نشده؛ این موج فقط مسیر
  مهاجرت و راه‌اندازی را پوشش می‌دهد.
- ذخیرهٔ اعتبارنامه با `CRED_PERSIST_LOCAL_MACHINE` انجام می‌شود؛ روی سیستم‌های
  چندکاربره این یعنی کلید برای همهٔ کاربران قابل‌خواندن است. بررسی و تصمیم دربارهٔ
  `CRED_PERSIST_LOCAL_MACHINE` در برابر `CRED_PERSIST_ENTERPRISE` ثبت نشده.
- پنل تست میکروفون و سیاست idle-return فقط از نظر ساخت و چرخهٔ اجرا تأیید شده‌اند؛
  سنجهٔ کاربردی (کاربر واقعی، فیدبک) انجام نشده است.

**آمار:** ۶۰۶ آزمون سبز، بدون شکست و بدون نادیده‌گرفتن. کلیپی با `-D warnings` سبز.

---

## Update notification — verified against the live GitHub API (2026-10-05)

Question asked: if a new release is published, will the app show an update?

**Detection works. It was proven, not assumed.**

`tests/update_live_check.rs` queries the real endpoint
(`api.github.com/repos/mahdimoslemi88-sys/OmniType-FreePTT-2/releases/latest`)
rather than a hand-written fixture, because a fixture only proves the code
agrees with a JSON literal someone wrote by hand.

Measured results:

- Endpoint answers **HTTP 200**; newest release is **`v0.3.0`**, published
  2026-09-29, asset `OmniType-FreePTT-0.3.0-setup.exe`.
- The positive case — a published release **newer** than the running build being
  surfaced — was verified end to end by asking the app to behave as build
  `0.0.1`. Result: `Available`, installer name and `https://` release page both
  populated. This is the assertion that would catch a renamed repo, a changed
  response shape, or a tag format nobody anticipated, and it fails *before* a
  release ships.
- Current build is `0.3.0` and the published tag is `v0.3.0`, so today's honest
  answer is "up to date" — which is correct, not a defect.

Run it with:
`cargo test -p voice-ptt --test update_live_check -- --include-ignored --nocapture`

Checks: `cargo test` 632 passed / 0 failed; `clippy -D warnings` clean.

### The gap that remains — no passive notification

Detection feeds `UpdateState::Available`, and that state reaches the user in
**one** place: the update banner inside the dashboard's Settings tab
(`gui/overlay.rs`, inside `render_dashboard`).

Two paths that look like they notify are **not** live:

- The toast built in `overlay.rs` when an update is found is **commented out**.
  The comment is correct and the reason is real: the toast channel is never
  rendered (`toasts.show()` does not exist), so every toast queued one that
  could never expire and kept a 30 fps repaint loop running forever — measured
  at 2.5–5 % of a core while idle.
- The tray has **no** balloon/notification at all (no `NIF_INFO` /
  `ShellNotifyIcon` data path). It has a menu item that opens the update card,
  which is a reasonable place to *go*, but not a notification.

So the practical answer today: **an update is detected, but the user only finds
out if they open the dashboard and land on the Settings tab.** Nothing interrupts
them. That is a design gap, not a bug, and it is recorded here rather than
silently treated as working.

### Checks worth adding before a real release

- `prerelease` is parsed but **never consulted**. `/releases/latest` excludes
  prereleases, so today it is harmless — but if that endpoint is ever widened,
  a prerelease would be offered as a normal update.
- The installer is matched by substring (`.exe` / `setup`), so a `.msix` or a
  portable `.zip` release would publish with **no download button** and the user
  would be sent to the release page instead. Silent, and only visible after
  shipping.
- `is_newer_version` parses three dot-separated numbers. A tag like `v1.0`
  parses, but a non-numeric segment (`v2026.10`) makes the parse fail and the
  comparison return `false` — i.e. **no update is ever shown**, silently.
---

## V1 + R1 — review before insert, and recovery of un-inserted text (۲۰۲۶-۱۰-۰۵)

Roadmap §5.2 and §5.4, delivered together because they are the same moment seen
from two sides: text that exists, is not in any document, and needs a decision.
Splitting them would have meant two windows and two answers to "is any text
waiting?".

### What was actually missing before this

Not the decision logic — `KeptRecord` and `unaccepted_text()` already existed
and were unit-tested. What was missing was a **place for the user to act**:

- `Coordinator::kept_handle()` was marked `#[allow(dead_code)]` and was called
  from nothing outside its own tests.
- `OverlayApp::text_awaiting_user` was initialised `false` and **never written
  to**. `Activity.pending_text`, which the orb-idle policy reads to suppress the
  automatic return while text waits, was therefore permanently `false`.
- A refused or failed insert produced a log line and a three-second error badge,
  and then the text was unreachable. The user's dictation was gone.

So the gap was the same shape as the M1/O2/A1 wave: delivered modules with no
production call site.

### What was built

**`src/state/review.rs`** — the pure decision layer. `DraftKind` (`Review` /
`Undelivered`), `PendingDraft`, `ReviewCommand` (`Insert{id,text}` /
`Copy{id}` / `Cancel{id}`), `ReviewOutcome`, and `DraftStore` with
injectable-clock expiry. Three rules are enforced by construction rather than by
review:

1. a draft is inert — no path from `PendingDraft` to the keyboard skips a
   `ReviewCommand`;
2. one decision, one delivery — commands name the draft they are about, so a
   duplicate click resolves to `ReviewOutcome::Stale`;
3. retry repeats the **insert**, never the conversion — there is no audio behind
   a draft and no engine to ask again.

**`src/state/review_channel.rs`** — the wire. Loop → GUI is a revision-counter
`watch`; GUI → loop is an `mpsc` whose receiver is **taken once**, so a second
loop cannot become a second owner of the keyboard.

**`src/gui/overlay/review_panel.rs`** — one window, three buttons
(«درج» / «کپی» / «لغو»), an editable box, and the destination shown by name.
The clipboard is written on the GUI thread through `egui::Context::copy_text`, so
no new dependency was added.

**Coordinator** — `on_review` / `insert_reviewed` act on a decision;
`recover_from` turns a `KeptRecord` into an offer; `expire_drafts_due` is a loop
event sharing the existing single `sleep_until` with the error windows.

### Three decisions worth recording

- **The approved text is re-validated.** `insert_reviewed` asks the desktop
  again rather than trusting the identity captured when the draft was raised.
  Minutes may have passed and the user may have clicked into another window
  *because* the draft was sitting there. Without this, review mode would be the
  less careful of the two modes.
- **Backspaces are not replayed on approval.** A seam repair erases a fragment
  this app typed a moment earlier; if the user edited the text in between, the
  erase would delete their words. An approved insert is a plain type.
- **Recovery ignores the review setting.** `should_hold` returns `true` for
  `Undelivered` regardless of `review_before_insert`. Someone who never asked to
  see drafts still gets told their text did not land.

### Four test bugs found by running the scenarios

The pure layer was green immediately; the loop scenarios were not, and each
failure was a real defect in the test rather than in the product:

1. `copying_a_draft_types_nothing` asserted "the draft is resolved" in the same
   breath as sending the answer — it observed the store **before the loop had a
   turn**. Added `wait_resolved`, which every "it must be gone by now" claim now
   goes through.
2. `recovering_undelivered_text_types_it_once_the_window_is_back` restored the
   window with `moved_to(1)`, but `ScriptedDesktop::new()` opens `0x1000`. The
   recovery was correctly refused against a window that was never in front.
3. `approving_a_draft_whose_window_moved_…` waited for "the newest draft" after
   answering, which races the loop and finds the *old* draft still pending. Now
   waits for a **different id**.
4. The same recovery scenario asserted `kept_records().is_empty()` right after
   the keystroke. The keystroke and the bookkeeping that follows it are two
   moments; added `wait_nothing_kept`.

### One pre-existing test defect fixed

`asr::downloader::tests::silero_vad_url_serves_a_valid_onnx` guards the
*connect* against being offline — "Offline machine: nothing to assert, but never
a false failure" — but `resp.bytes().unwrap()` was unguarded. The body is a
multi-megabyte ONNX, so it times out on a link that connected fine. It failed in
the full-suite run with `reqwest::Error { kind: Decode, source: TimedOut }`. The
same guard was extended to the download, matching the test's own stated intent.
This was a real flaky failure, not an environmental one being waved away.

### Verification

| Check | Result |
|---|---|
| `cargo test -p voice-ptt` | `TEST_EXIT=0` — **667 passed / 0 failed / 0 ignored** |
| `cargo clippy --lib --all-targets -- -D warnings` | `CARGO_EXIT=0`, clean |
| `cargo build --release --bin voice-ptt` | `CARGO_EXIT=0`, 38 MB |
| `voice-ptt.exe --doctor` | exit 0, `problems: none` |
| live GUI run | window created, **0 ERROR lines** |
| `find src -newer target/release/voice-ptt.exe` | empty — binary is current |

`Coordinator::new` crossed clippy's argument limit at eight. Rather than add a
fifth bundle, the constructor carries `#[allow(clippy::too_many_arguments)]` with
a note, matching the existing precedent on `OverlayApp::new` — the collaborators
already fall into honest groups (`port` / `speech` / the loop's own seams) and a
wrapper around `status`/`sink`/`desktop`/`clock` would only hide that they are
four different things.

### Open, found while reading the logs — not from this change

The Google engine key is a **hardcoded constant in the source**:

    src/asr/google.rs:23  pub const CHROMIUM_SPEECH_KEY: &str = "<the key is not repeated here>"

and it reaches the log file through `reqwest` error text — four occurrences in
today's log, e.g. a failed conversion logging the full request URL. This is the
Chromium public key, which is published in Google's own repo, so it is not a
user secret; but roadmap §7.2 says logs must not contain keys, and it currently
does. Not fixed here because it is outside this package; recorded so it is not
mistaken for something this wave introduced.

### Not done in this package

- **`draft_ttl_secs` has no settings-panel control yet** (the default of 120 s
  applies). `DraftStore::expire` and the loop's use of it are tested with a
  hand-moved clock, but no scenario drives expiry through the loop.
- **`text_awaiting_user` is still never written.** With the review window now
  reading the store directly, the orb-idle policy's `Activity.pending_text` is
  still permanently `false`, so the orb can walk back to its corner while a
  draft is waiting. That is the remaining half of O2's acceptance criteria.### O2 leftover closed — the orb no longer walks away from a pending text

`OverlayApp::text_awaiting_user` was an `Arc<AtomicBool>` that **nothing ever
wrote**. Its own comment said so, and it was still feeding
`Activity.pending_text` — the blocker that stops the orb auto-returning to its
corner while the user owes it a decision. So that half of O2's acceptance
criteria had been decorative since it landed: the field existed, the policy
branch above it existed, and the value was permanently `false`.

Rather than write the flag from the new code, the flag was **deleted** and
`pending_text` now answers from the store itself:

    pending_text: !self.review.snapshot().is_empty()

One place decides what is pending, so the orb cannot walk away from a draft it
does not know about. `ReturnBlocker::PendingText` was already correct and is
unchanged.

Re-verified after the change: `cargo test -p voice-ptt` **667 passed / 0 failed**,
clippy `-D warnings` clean, release build 38 MB, `--doctor` `problems: none`, live
run with **0 ERROR lines and 0 panics**.### Correction — the first review-window render tests were vacuous

Four tests were added to `review_panel.rs` to cover the window body, the one
part of this package no loop scenario executes. They passed on the first run.

They were proving nothing. A headless `egui::Context` reports **only the root
viewport**, so `show_viewport_immediate` never invokes its closure — proved by a
temporary `eprintln!` probe inside the viewport closure, which printed **zero**
times. Every assertion phrased as "the window opened" was satisfied by a window
that was never created.

Found because the assertions were written against the *claim* ("a draft must
open one window") rather than against the code path, and the claim could not
hold. Rewritten as two honest halves:

- **`draw_body` tests** call `render_body` inside a real `Ui`, and assert on
  `output.shapes.len() > 5`. Shapes only exist if widgets were laid out, so a
  non-running draw fails instead of passing. Covers both draft kinds, a
  destination with a title, and a draft with **no** destination.
- **`loading_a_draft_puts_its_text_in_the_box`** drives `review_render`, whose
  loader runs *before* the viewport is opened and therefore does execute. It
  covers load, and that a second dictation **replaces** the box rather than
  appending — two dictations in one box would type both at once.

The lesson is the same one this repo has already recorded about a "pass" that was
a vacuous skip: a test that cannot fail is not evidence. `render` itself — the
`show_viewport_immediate` call — remains **untested**; only the body and the
loader are covered. A full check needs a real OS window.

### Final verification numbers (re-run after the above)

| Check | Exit | Result |
|---|---|---|
| `cargo test -p voice-ptt` | `TEST_EXIT=0` | **671 passed / 0 failed**, 0 FAILED suites |
| `cargo clippy --lib --all-targets -- -D warnings` | `CLIPPY_EXIT=0` | 0 errors, 0 warnings |
| `cargo build --release --bin voice-ptt` | `BUILD_EXIT=0` | 38,183,424 bytes |
| `voice-ptt.exe --doctor` | `DOCTOR_EXIT=0` | `problems: none` |
| live GUI run | — | window created, **0 ERROR, 0 panic** |
| `find src -newer target/release/voice-ptt.exe` | — | empty — binary is current |