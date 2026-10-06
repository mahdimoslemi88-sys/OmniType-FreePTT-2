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
---

## اعلان نسخهٔ جدید با balloon در ترای (۲۰۲۶-۱۰-۰۵)

پس از انتشار v0.4.0، شکافی که تست زندهٔ گیت‌هاب نشان داده بود بسته شد: تشخیص آپدیت کار می‌کرد،
ولی `UpdateState::Available` فقط به یک جا می‌رسید — بنر داخل تب تنظیمات، و فقط اگر کاربر خودش
آن تب را باز می‌کرد.

### چرا balloon روی همان آیکون ترای نیست

`tray-icon` نه `HWND` و نه `uID` آیکونش را منتشر نمی‌کند، و یک balloon با جفتِ
`(HWND, uID)` آدرس‌دهی می‌شود. دو راه بود: پیدا کردن پنجره‌اش با نام کلاسی که *اتفاقاً*
ثبت کرده و حدس زدن `uID`، یا آوردن آیکون خودمان.

راه اول همان روزی که آن crate نام کلاس یا شمارندهٔ `uID` را عوض کند **بی‌صدا** از کار
افتاد — بدون خطا، بدون balloon، صرفاً یک قابلیت که هیچ‌کس نمی‌فهمد چرا کار نمی‌کند. دقیقاً همان
چیزی که این ریپو بارها از تحویل دادنش پرهیز کرده. پس balloon آیکون خودش را دارد: اضافه می‌شود،
استفاده می‌شود، و پس از ۶۰ ثانیه حذف می‌شود. بهایش یک ورودی دوم در ترای است، فقط وقتی
آپدیتی اعلام می‌شود.

### چهار تصمیمی که اگر اشتباه شوند، یا اسپم می‌شود یا سکوت

**یک‌بار برای هر نسخه، نه یک‌بار برای هر بررسی.** بررسی پس‌زمینه هر ۶ ساعت بیدار می‌شود. balloon
روی هر بررسی موفق یعنی چهار اعلان در روز تا ابد — و کاربری که اعلان‌ها را در ویندوز خاموش کند،
دیگر *هیچ* آپدیتی را نمی‌بیند. نسخهٔ اعلام‌شده در `config.toml` ذخیره می‌شود، پس ری‌استارت هم
تکرار نمی‌کند.

**نسخهٔ جدید دوباره اعلام می‌شود.** حذف تکرار روی *نسخه* است نه یک پرچم «قبلاً هشدار داده شد».
کاربری که آخرین بار دربارهٔ 0.4.0 خبردار شده باید دربارهٔ 0.5.0 هم بشنود.

**هنگامی ثبت می‌شود که واقعاً اعلام شده باشد.** اگر پوسته balloon را رد کند، مقدار نوشته
*نمی‌شود* تا بررسی بعدی دوباره تلاش کند. ثبت‌کردنِ اول یعنی یک balloon ردشده هرگز تکرار نمی‌شود
و کاربر هرگز از آن نسخه باخبر نمی‌شود — بدترین حالت ممکن.

**برش متن بر حسب واحد UTF-16.** `szInfo` و `szInfoTitle` آرایه‌های ثابت `[u16; N]`اند. یک
نام نسخه با ایموجی دو واحد است ولی یک کاراکتر، پس کرانِ `.chars().take(N)` می‌گذارد جفتِ
نیمه‌کاره از انتهای آرایه بیرون بزند. همان تله‌ای که `output::injector` برای صفحه‌کلید حل
کرده بود؛ اینجا به‌جای خراب‌کردن ورودی، جمله را خراب می‌کرد.

### چیزی که تست‌ها باید ثابت می‌کردند و نمی‌کردند

تست‌های واحد همه‌چیز را پوشش می‌دهند جز یک چیز: آیا ویندوز اصلاً ساختار `NOTIFYICONDATAW`
ما را قبول می‌کند؟ `cbSize` غلط، `NIM_SETVERSION` جاافتاده، `HICON` بارنشده — هرکدام
`Shell_NotifyIconW` را `FALSE` برمی‌گردانند و اعلانی هرگز ظاهر نمی‌شود، بدون خطا در
هیچ‌جایی که کاربر یا تست ببیند.

پس [update_balloon_shell.rs](../../voice-ptt/tests/update_balloon_shell.rs) پوستهٔ واقعی
ویندوز را صدا می‌زند و چون هر رد شدن `warn!` می‌دهد، یک subscriber ضبط‌کننده آن شکست خاموش را
به تست شکست‌خور تبدیل می‌کند. **با کنترل مثبت و منفی سنجیده شد**: یک sentinel عمداً از
`tracing` رد می‌شود تا ثابت شود ضبط واقعاً کار می‌کند (وگرنه «هیچ ردّی نبود» با «subscriber
نصب نشده» یکی است)، و یک ردّ ساختگی عمداً تزریق شد تا ثابت شود assertion واقعاً شکست می‌خورد.

### بررسی‌ها

| | |
|---|---|
| `cargo test -p voice-ptt` | **۶۸۷ پاس / ۰ شکست** (`CARGO_EXIT=0`) |
| تست‌های اعلان (۵ تست، با `--include-ignored`) | ۵ پاس — شامل کل زنجیره: حالت → watcher → balloon → ثبت در دیسک → تکرار نکردن |
| کنترل منفی | ردّ ساختگی باعث `FAILED` شد، پس assertion واقعی است |
| clippy `-D warnings` | تمیز، صفر خطا |
| بیلد release | ۳۸٬۲۴۹٬۹۸۴ بایت |
| اجرای زنده | پنجره ساخته شد، **۰ ERROR، ۰ panic** (۴ خط ERROR موجود، همه پیش از این اجرا) |

### چه چیزی هنوز آزموده نشده

**اینکه یک انسان balloon را دید.** ویندوز می‌تواند اعلان را قبول کند و هرگز نشان ندهد — Focus
Assist، «Do not disturb»، یا Action Center پر — و هیچ API این را گزارش نمی‌دهد. تست‌ها ثابت
می‌کنند پوسته رد نکرده، نه اینکه چشمی آن را دیده. به همین دلیل بنر داخل برنامه سر جایش
مانده و حذف نشده.

**دومین آیکون ترای** هنگام اعلام برای ۶۰ ثانیه دیده می‌شود. این بهای آگاهانهٔ دور زدن
`tray-icon` است و در ماژول هم نوشته شده.
---

## یک تستِ پرنده که هنگام بررسی balloon لو رفت (۲۰۲۶-۱۰-۰۵)

در آخرین اجرای کامل، `multiple_refused_chunks_do_not_overwrite_any_record` شکست خورد:

```
left: 1   right: 2
neither refused chunk record may be overwritten
```

**جدا اجرا شد: ۵ بار از ۵ بار سبز.** یعنی مسابقه بود، نه رگرسیون — و فقط زیر بار کل
مجموعه آزمون ظاهر می‌شد.

### علت واقعی

تست برای اطمینان از «کارِ حلقه تمام شده» منتظر *خطا* می‌ماند، ولی خطایی که منتظرش بود
خطای **قطعهٔ اول** است. ترتیب در محصول درست است: رکورد **پیش از** خطای خودش push می‌شود.
پس دیدن خطای قطعهٔ اول فقط ثابت می‌کند رکورد قطعهٔ اول نوشته شده — و تست بلافاصله
`kept_records()` را می‌خواند در حالی که تبدیل قطعهٔ دوم هنوز در جریان است.

این «رکورد گم‌شده» نبود؛ یک خواندنِ زودهنگام بود که شبیه گم‌شدن رکورد به نظر می‌رسید — و
همین خطرناکش می‌کند: آزمونی که گاهی سبز و گاهی قرمز است به «فلکی» معروف می‌شود و بعد
کسی که یک بار دیگر شکست خورد وزنش را جدی نمی‌گیرد.

### اصلاح

`Harness::kept_until(n)` اضافه شد: تا رسیدن به `n` رکورد صبر می‌کند (فهرست پشت یک
`Mutex` ساده است و کانال تغییری ندارد، پس با گام‌های ۵ میلی‌ثانیه و سقف `PATIENCE`).
تست حالا منتظر چیزی است که واقعاً ادعا می‌کند، نه یک نمایندهٔ ناقص آن.

### کنترل منفی

اصلاحِ تست بدون اثبات اینکه تست هنوز می‌تواند شکست بخورد بی‌ارزش است. یک باگ واقعی
تزریق شد (`record_kept` به‌جای `push`، `clear` می‌کند) و تست **شکست خورد** — با پیامی
روشن: `waited 10s for 2 refused-text record(s) to be kept`. یعنی حالا اگر رکوردی واقعاً
گم شود، تست با علت درست می‌افتد، نه با یک عدد بی‌توضیح.

سپس فایل بازگردانده شد و سه اجرای کامل پشت سر هم گرفته شد: **۶۸۷ پاس / ۰ شکست، سه بار.**

### بررسی‌ها پس از اصلاح

| | |
|---|---|
| `cargo test -p voice-ptt` ×۳ | `CARGO_EXIT=0` — ۶۸۷ پاس / ۰ شکست، هر سه بار |
| clippy `-D warnings` | صفر خطا، صفر هشدار |
| بیلد release | ۳۸٬۲۴۹٬۹۸۴ بایت |
| `find src -newer` | خالی — باینری به‌روز |
| اجرای زنده | پنجره ساخته شد، **۰ ERROR، ۰ panic** |
---

## P1 — پروفایل برنامه‌ها

هر پنجره می‌تواند قواعد متنی خودش را داشته باشد، و پنجره‌ای که پروفایل ندارد **دقیقاً**
همان رفتار قبلی را می‌گیرد. این دو نیمهٔ یک جمله‌اند: بخش دوم شرط لازم بخش اول است.

### ماژول تصمیم (خالص)

`src/profiles/mod.rs` — همه‌چیز مقدار است. `ProfileSet` از `[[profiles]]` در `config.toml`
خوانده می‌شود، به یک executable بسته می‌شود، و `effective()` قواعد مؤثر یک دیکته را
برمی‌گرداند. هیچ پنجره‌ای در این ماژول خوانده نمی‌شود و هیچ قفل یا دیسکی لمس نمی‌شود.

| معیار (از طرح اجرا) | چه چیزی آن را قفل می‌کند |
|---|---|
| fallback عمومی | `an_empty_set_of_profiles_changes_nothing`، `an_application_without_a_profile_gets_the_general_rules` |
| حذف override | `removing_a_profile_restores_the_general_rules`، `an_override_merges_field_by_field` |
| هویت نامعلوم | `an_unknown_executable_falls_back_to_the_general_rules` |
| ثبات جلسه | `rules_already_resolved_do_not_change_when_the_set_does` + ۸ سناریوی حلقه |

هویت برنامه از `TargetIdentity::exe_path` می‌آید، نه از عنوان پنجره — و
`the_window_title_is_not_an_input_to_resolution` همین را قفل می‌کند: عنوان با تایپ کاربر
عوض می‌شود، پس بستن پروفایل به آن یعنی پروفایل در میانهٔ همان سندی که برایش ساخته شده از
کار می‌افتد. resolver دومی برای مقصد ساخته نشد؛ همان `crate::output::target` استفاده می‌شود.

دو تصمیم که اگر اشتباه می‌شدند، بی‌صدا خراب می‌کردند:

**گزینهٔ حل ابهام، «برندهٔ اول» نیست، «امتناع» است.** دو پروفایل برای یک executable یعنی
قواعد عمومی — نه هر کدام که در فایل زودتر آمده. ترتیب فایل ترتیب نمایش کاربر است، نه
تقدّم؛ اگر تقدّم بود، «برای کروم تنظیم کردم و قواعد ترمینال اعمال شد» منتشر می‌شد.

**یک حالت ناشناخته، override را باطل می‌کند، نه کل خط لوله را.** غلط املایی در یک پروفایل
باید فقط همان override را از آن پروفایل بگیرد؛ برگشتن به `Standard` یعنی یک تغییر رفتار
دیگر، سوار بر یک اشتباه تایپی.

### اتصال در حلقه

`rules_for(settings, target)` تنها نقطهٔ تصمیم است؛ هم مسیر تبدیل و هم مسیر درج آن را صدا
می‌زنند، پس «کدام قواعد روی این متن اعمال شد» نمی‌تواند دو جواب متناقض داشته باشد. مقصد
همان مقداری است که **هنگام شروع ضبط** گرفته شده و در `enqueue` روی کار کپی می‌شود — پس
تعویض برنامه در میانهٔ تبدیل، قواعد قطعه‌های همان جلسه را عوض نمی‌کند.

قاعده‌های اختصاصی پروفایل **در حالت `raw` هم اجرا می‌شوند**: `raw` پردازش *خودکار* را
خاموش می‌کند، ولی قاعده‌ای که کاربر برای همان برنامه دستی نوشته، دستور صریح است. اگر
برعکس بود، پنل قاعده را می‌پذیرفت و هیچ‌وقت اعمال نمی‌کرد.

### کنترل منفی

اصلاح بدون اثبات اینکه آزمون می‌تواند بیفتد، بی‌ارزش است. `rules_for` موقتاً وادار شد
پروفایل‌ها را نادیده بگیرد → **۵ سناریو شکست خورد**، سپس فایل بازگردانده شد و ۰ باقیماندهٔ
وصله تأیید شد.

اولین اجرا فقط ۴ شکست داشت: تست «قاعده‌های خودِ پروفایل» روی غلط املایی‌ای ساخته شده بود
که واژه‌نامهٔ پیش‌فرض خودش درستش می‌کرد، پس با پروفایلِ نادیده هم سبز می‌ماند. جفت‌واژه به
چیزی عوض شد که واژه‌نامه هرگز ندیده است (`تستواره` → `تستآوره`) و حالا می‌افتد.

### پنل مستقل

`src/gui/overlay/profiles_panel.rs` — تب تازهٔ «پروفایل‌ها» با فهرست و ویرایشگر: نام، binding،
دکمهٔ «از پنجرهٔ فعال» (executable پنجرهٔ روبه‌رو را می‌خواند تا کاربر `chrome.exe` را از
حافظه ننویسد)، حالت متن، بازبینی پیش از درج با «پیروی از عمومی» برای *برداشتن* override، و
قاعده‌های اختصاصی. ذخیره از همان درفت و همان اعتبارسنج تب تنظیمات رد می‌شود، و پیش از
نوشتن، کل مجموعه بررسی می‌شود: یک binding خالی یا تکراری هرگز روی دیسک نمی‌رود، چون ابهام
در فایل تا اولین دیکته نامرئی است — و آن دیکته بی‌صدا قواعد عمومی را اجرا می‌کند.

### بررسی‌ها

| | |
|---|---|
| `cargo test -p voice-ptt` | `CARGO_EXIT=0` — **۶۲۳ پاس / ۰ شکست** (کتابخانه) + ۱۱۱ آزمون یکپارچه، مجموعاً ۷۲۰ |
| تست‌های همین کار | ۲۶ خالص + ۸ سناریوی حلقه + ۱۱ پنل + ۱ چرخهٔ ذخیره/خواندن از دیسک |
| clippy `--all-targets -D warnings` | صفر خطا، صفر هشدار |
| بیلد release | ۳۸٬۳۷۸٬۴۹۶ بایت؛ `find src -newer target/release/voice-ptt.exe` خالی |
| اجرای زنده | پنجره ساخته شد، **۰ ERROR، ۰ panic**؛ `config.toml` کاربر دست‌نخورده ماند |

### آنچه هنوز آزموده نشده

**چشم‌دید یک انسان از تب.** رندر از روی یک `egui::Context` واقعی در آزمون گذشت (بدون
panic)، ولی «درست به نظر می‌رسد» فقط با نگاه کردن معلوم می‌شود.

**زبان و اعداد در پروفایل نیستند.** طرح اجرا زبان و اعداد را هم نام می‌برد؛ این نسخه
فقط قواعد *متن* را عوض می‌کند (حالت، بازبینی، واژه‌نامه). زبان به امضای `AsrEngine::transcribe`
وابسته است، و عوض کردن خودکار موتور یا زبان بدون تصمیم صریح کاربر همان چیزی است که طرح
صریحاً ممنوع کرده — پس عمداً بیرون گذاشته شد تا با یک تصمیم صریح کاربر اضافه شود.

## D1 — اصلاح سریع واژه‌نامه (۲۰۲۶-۱۰-۰۵)

واژه‌ای که غلط شنیده شده، از همان جمله‌ای که در تاریخچه هست برداشته می‌شود، معادلش نوشته
می‌شود، و **پیش از ذخیره** دیده می‌شود که این قاعده با همین جمله چه می‌کند. ذخیره، واژه را
برای دیکته‌های بعدی درست می‌کند. «آموزش» در این بسته همان قاعدهٔ واژه‌نامه است: هیچ مدلی
دوباره آموزش داده نمی‌شود، چون یادگیری خودکارِ بی‌اجازه در طرح اجرا ممنوع است.

### باگی که پیدا شد، خودِ معیار اول بود

معیار «عدم تغییر زیررشتهٔ کلمهٔ سالم» چیزی نبود که بعد از ساختن پنل با یک تست اضافه شود؛
**از قبل شکسته بود**. `Dictionary::correct` از `matcher.replace_all` استفاده می‌کرد: جایگزینی
زیررشته‌ای خام، بدون نگاه به مرزِ واژه. با قواعد پیش‌فرض خودِ برنامه، روی دستگاهی که برنامه
واقعاً اجرا می‌شود:

| ورودی | پیش از اصلاح | پس از اصلاح |
|---|---|---|
| `این نیست` | `این NACEت` | `این نیست` |

قاعده‌های کوتاهی مثل `نیس → NACE`، `دین → DIN`، `ویت → Vite` و `یمل → YAML` همه داخل
واژه‌های سالم فارسی می‌نشینند، و `replace_all` هیچ فرقی بین «واژه» و «تکه‌ای از واژه»
نمی‌گذاشت.

اصلاح: `correct` روی `find_iter` می‌چرخد و تطبیقی را رد می‌کند که `stands_as_a_word` نیست.
این تابع **هر دو طرف** را نگاه می‌کند (چون نویسه‌بردار نشانه‌گذاری را بی‌فاصله می‌چسباند:
`نیس.` واژه است و `(نیس` هم، ولی `نیست` نه)، و چسبندگی را با حرف/رقم، زیرخط، ZWNJ و
حرکات فارسی/ترکیبی می‌سنجد — سه مورد آخری همان‌هایی‌اند که اگر از قلم بیفتند، قاعده از
داخل `می‌کنم` یا `my_var` هم عبور می‌کند.

### ماژول تصمیم (خالص)

`src/processing/quickfix.rs` — هیچ‌چیز اینجا به egui یا دیسک وصل نیست. `assess` می‌گوید ذخیره
مجاز است یا نه (`EmptySide`، `IdenticalSides`) و چه اخطارهایی دارد (`ReplacesRule`،
`InsideWords`، `OverlapsRule`)؛ `preview` دو خط پیش/پس را می‌سازد؛ `explain_no_effect` تنها
یک دلیل برای «هیچ تغییری نیست» نام می‌برد — و ترتیبش عمدی است.

| معیار (از طرح اجرا) | چه چیزی آن را قفل می‌کند |
|---|---|
| عدم تغییر زیررشتهٔ کلمهٔ سالم | `a_rule_does_not_rewrite_a_healthy_word_that_contains_it`، `words_that_merely_contain_the_word_are_reported` |
| ساخت قاعدهٔ دقیق از انتخاب + جایگزین | `the_selected_phrase_is_the_text_between_the_cursors`، `trimming_is_applied_to_the_saved_rule`، `a_rule_with_a_blank_side_is_refused`، `a_rule_that_maps_a_word_to_itself_is_refused` |
| تعارض قواعد | `an_existing_rule_with_a_different_replacement_is_reported`، `the_same_rule_again_is_not_a_conflict`، `overlapping_rules_are_reported_in_both_directions`، `a_phrase_rule_is_not_reported_as_an_overlap` |
| تقدم عمومی/پروفایل | `a_general_fix_does_nothing_in_a_raw_destination`، `a_profile_fix_takes_effect_even_in_a_raw_destination`، `a_profile_fix_previews_against_that_profiles_mode`، `mode_for_agrees_with_the_resolver` |
| ذخیره/حذف | `add_and_remove_rule_updates_pipeline`، `load_or_create_persists_new_file`، `the_candidate_replaces_a_rule_for_the_same_word` |
| پیش‌نمایش پیش از ذخیره | `the_preview_shows_a_general_fix_taking_effect`، `the_preview_agrees_with_the_pipeline_it_borrows` |

دو تصمیم که اگر اشتباه می‌شدند، پیامِ پنل دروغ می‌شد:

**«جملهٔ نمونه این واژه را ندارد» بر «این مقصد `raw` است» مقدم است.** اولی ایرادِ خودِ
پیش‌نمایش است و کاربر همان‌جا درستش می‌کند؛ دومی واقعیتی دربارهٔ مقصد است که در رسم بعدی
هم هست. اگر برعکس بود، «برنامهٔ من `raw` است» می‌توانست نمونه‌ای را توضیح دهد که هیچ‌چیز
را نشان نداده بود.

**ولی نمونه‌ای که *طرف دوم* را دارد، نمونهٔ بی‌ربط نیست.** اگر متن از قبل همان‌جوری که
قاعده می‌سازد خوانده می‌شود، جواب درست «قاعده لازم نیست» است نه «واژه در جمله نیست» —
وگرنه کاربر نتیجه می‌گیرد قاعده‌اش بی‌فایده است، در حالی که فقط جای نمونه اشتباه بوده.
`AlreadyCorrect` تنها وقتی نام برده می‌شود که نمونه هیچ‌کدام از دو طرف را نداشته باشد و
چیزی برای گفتن نمانده باشد. (این مرز با یک تست شکست‌خورده پیدا شد: تستِ «قاعدهٔ تکراری»
بعد از اضافه شدن بررسیِ نمونه، به `SampleDoesNotContainTheWord` می‌افتاد چون واژهٔ
نامربوطی را با جملهٔ نامربوط می‌سنجید.)

### پیش‌نمایش، همان خط لوله است نه شبیه آن

`src/processing/mod.rs` یک مقدار تازه دارد: `TextRules { mode, normalizer, dictionary, corrections }`
با `apply(text)`. ترتیب اجرا — حالت، بعد واژه‌نامهٔ عمومی، بعد قواعد خودِ مقصد — یک‌جا نوشته
شده و **دو صاحب دارد**: حلقهٔ واقعی (`coordinator::process_with_rules` هم به همین مقدار
واگذاری شده) و کارتِ پیش‌نمایش. یک پیش‌نمایشی که ترتیب را دوباره پیاده می‌کرد، دیر یا زود
با محصول اختلاف پیدا می‌کرد، و پیش‌نمایشِ دروغ از نبودنش بدتر است. `Copy` است چون هر دو
طرف مقایسه به همان مجموعهٔ قواعد نیاز دارند.

### اتصال‌ها — محل‌های دقیق

| محل | چه می‌کند |
|---|---|
| `history_panel.rs` — دکمهٔ «اصلاح واژه» در هر سطر | فقط یک `FixRequest { word: خالی, sentence }` تولید می‌کند؛ به پنل دیگری وصل نیست |
| `overlay.rs::render_history_body` | درخواست را می‌گیرد، `dict_fix.seed(...)` را صدا می‌زند و تب را به `Dictionary` می‌برد |
| `overlay.rs::render_dict_body` | از `self.history` پیکره (corpus) می‌سازد و کارت اصلاح را بالای تب واژه‌نامه رسم می‌کند |
| `processing/mod.rs::TextRules` | تنها پیاده‌سازی «با این متن چه می‌شود» |
| `profiles::mode_for` | حالت مؤثر مقصد را هم به `effective` می‌دهد و هم به پیش‌نمایش |

پیکره **از تاریخچهٔ واقعی کاربر** ساخته می‌شود، نه از یک نمونهٔ ساختگی: تنها شاهدِ اینکه یک
قاعدهٔ کوتاه داخل کدام واژه‌های *سالم* می‌نشیند، جمله‌هایی است که کاربر واقعاً دیکته کرده.
این پیکره در هر رسم از نو ساخته می‌شود ولی کارِ سنگین (ماتچر) پشتِ یک کلیدِ هش‌شده کش می‌شود،
چون پنل ۶۰ بار در ثانیه رسم می‌شود.

دکمهٔ سطر، واژه را **پیشنهاد نمی‌کند، بلکه جمله را می‌آورد**: واژه با انتخاب کاربر روی همان
جمله در کادر بالا برداشته می‌شود. دلیلی که واژه خالی می‌ماند این است که برنامه مطمئن نیست
کدام واژه غلط بوده؛ حدس زدن و از قبل پر کردن، همان «matching مبهم» است که طرح ممنوع کرده.
**تاریخچه در این مسیر فقط خوانده می‌شود**؛ نوشتن قاعده هیچ سطری را حذف یا عوض نمی‌کند.

### ذخیره و حذف

مسیر عمومی و مسیر پروفایل هر دو از قواعدی رد می‌شوند که از قبل برای همان کار آزموده شده‌اند:
ذخیرهٔ عمومی به `Dictionary::add_rule` + `save_to_file` می‌رود که قاعدهٔ هم‌واژه را *جایگزین*
می‌کند نه اینکه دومی بسازد، و ذخیرهٔ پروفایل کل دفتر (draft) را از `validate_settings` و
`profiles_panel::validate_set` می‌گذراند و بعد می‌نویسد — نه فقط یک قاعده را، چون draft تنها
نسخهٔ ویرایش‌های ذخیره‌نشدهٔ کاربر است و نوشتن یک قاعده از داخلش بقیه را دور می‌ریخت.
حذف هم فقط قاعدهٔ همان واژه را برمی‌دارد.

### کنترل منفی

`stands_as_a_word` موقتاً وادار شد همیشه `true` برگرداند (یعنی همان رفتار `replace_all`).
تست `a_rule_does_not_rewrite_a_healthy_word_that_contains_it` افتاد، با همان رشته‌ای که
زنده دیده شده بود:

```text
assertion `left == right` failed: "این نیست" was rewritten by a rule that only matched inside a word
  left: "این NACEت"
 right: "این نیست"
```

سپس فایل بازگردانده شد و md5 آن با پیش از کنترل یکسان درآمد
(`453ca264f8007f60bdd83e08f3575434`)، و ۱۱ تست واژه‌نامه سبز شدند.

### بررسی‌ها

| | |
|---|---|
| `cargo test -p voice-ptt` | `CARGO_EXIT=0` — **۶۵۸ پاس / ۰ شکست** (کتابخانه) + ۱۱۱ آزمون یکپارچه، مجموعاً ۷۶۹ |
| تست‌های همین بسته | ۲۰ خالص (`quickfix`) + ۱۲ پنل (`dict_fix_panel`) + ۱ تستِ واژهٔ سالم + ۱ رندرِ سرتاسریِ کارت با واژهٔ پرشده روی `egui::Context` واقعی |
| clippy `--all-targets -D warnings` | صفر خطا، صفر هشدار |
| بیلد release | ۳۸٬۴۳۸٬۹۱۲ بایت؛ `find src Cargo.toml -newer target/release/voice-ptt.exe` خالی |
| اجرای زنده | پنجره ساخته شد، ۰ ERROR، ۰ panic؛ `md5` فایل `config.toml` کاربر (۴٫۲ مگابایت) پس از اجرا دست‌نخورده |

در سه اجرای کامل پشت‌سرهم و هشت اجرای ماژول `gui::overlay` هیچ شکستی نبود. در یکی از
اجراهای میانی (زیر بار: ۱۴٫۹ ثانیه در مقابل ۴٫۵ ثانیه) یک تست افتاد و **نامش ثبت نشد**؛ در
اجراهای بعدی تکرار نشد، پس دلیلش تأیید نشده و اینجا فقط به‌عنوان مشاهده گزارش می‌شود. تنها
تستی که در درخت واقعاً `thread::sleep` می‌کند در `mic_test_panel` است که از قبل بوده و در
این بسته دست نخورده.

### آنچه هنوز آزموده نشده

**چشم‌دید یک انسان از کارت.** رندر با یک واژهٔ واقعی روی `egui::Context` واقعی و ۲۵ موقعیت
نشانگر گذشت (بدون panic)، ولی «درست به نظر می‌رسد» فقط با نگاه کردن معلوم می‌شود.

**چرخهٔ کامل با دیکتهٔ واقعی انجام نشد.** یعنی: گفتن، دیدن خطای واژه در تاریخچه، ذخیرهٔ
قاعده و بعد دیکتهٔ دوباره با `SendInput` واقعی. حلقه با مقدارهای واقعی (`TextRules`) آزموده
شده، ولی ثبت/درج سخت‌افزاری در این بسته تکرار نشد.

**انحراف از متن طرح، ثبت‌شده.** طرح D1 می‌گوید `dictionary` و `dict_panel` را ویرایش کن.
`dictionary.rs` ویرایش شد، ولی کارت اصلاح در فایل تازهٔ `src/gui/overlay/dict_fix_panel.rs`
است و `dict_panel.rs` دست‌نخورده مانده. این با قاعدهٔ خودِ طرح («پنل اختصاصی موازی، اتصال
مرکزی ترتیبی») هم‌خوان است و کارت داخل همان تب واژه‌نامه و بالای جدول موجود رسم می‌شود،
ولی اگر هماهنگ‌کننده ادغام در `dict_panel` را لازم می‌داند، این یک جابه‌جایی است نه بازنویسی.

### یک نکتهٔ عملیاتی: قفل تک‌نسخه بین بیلد توسعه و نسخهٔ نصب‌شده مشترک است

هنگام بررسی زنده، یکی از اجراها با «another instance is already running» مرد، در حالی که
`tasklist` هیچ `voice-ptt.exe` زنده‌ای نشان نمی‌داد. لاگ همان روز توضیح داد: نود ثانیه پیش از
آن، نسخهٔ **نصب‌شدهٔ** کاربر (`%LOCALAPPDATA%\Programs\OmniType FreePTT\voice-ptt.exe`)
شروع شده بود و mutex نام‌دار `Local\OmniTypeFreePTT.SingleInstance` را گرفته بود. پس این یک
نقص نبود و توضیحش در لاگ بود، نه در کد — و اولین برداشت (نبودِ نمونهٔ زنده) غلط بود.

ولی همین واقعیت برای بررسی‌ها مهم است: آن mutex بین بیلد توسعه و نسخهٔ نصب‌شده **مشترک**
است، پس اجرای `target/release` از این درخت جای نسخهٔ نصب‌شده را می‌گیرد و تا وقتی زنده است،
برنامهٔ نصب‌شده بالا نمی‌آید و به کاربر پیام «در حال اجراست» نشان می‌دهد. نمونهٔ توسعه در
پایان این کار زنده گذاشته شد (بیلد `target/release`) تا کارت تازه دیده شود؛ پیش از بررسی
بعدی باید آگاهانه بسته شود.

---

## C1 — فرمان‌های گفتاری (۲۰۲۶-۱۰-۰۶)

آخرین بستهٔ موج ۴ که عقب مانده بود. قرارداد در
[CONTRACTS بند ۱۰](CONTRACTS.md) ثبت شد؛ آنچه در اینجا می‌آید، آنچه است و آنچه نیست.

### چه ساخته شد

| لایه | چه شد |
|---|---|
| پارسر | `processing/commands.rs` — خالص، بدون I/O: `parse(&str) -> Vec<Op>` با `Op::{Text, Command, Unknown}`، به‌علاوهٔ `render` و `apply`. جدول عبارت‌ها فارسی و انگلیسی است و با `fold` (ی/ک عربی، نیم‌فاصله) مقایسه می‌شود تا در حالت `raw` هم کار کند |
| کلید | `TextSettings.commands` (پیش‌فرض **false**) در `[text]`، و چک‌باکس «فرمان‌های گفتاری» در تب تنظیمات کنارِ «بازبینی متن پیش از درج» |
| خط لوله | `ProcessingOptions.commands` → آخرِ `process_text_with`، یعنی بعد از حالت و واژه‌نامهٔ عمومی و پیش از قواعدِ مقصد |
| اتصال | `TextRules.commands` ← `EffectiveRules.commands` ← `GeneralRules.commands` ← `settings.text.commands`. پنل‌های واژه‌نامه و پروفایل همین مقدار را می‌فرستند تا پیش‌نمایش متنی را نشان ندهد که تایپ نمی‌شود |
| تزریق | `output/injector` — `\n` در هر دو مسیر burst و paced به **کلید Enter** می‌رود، نه نویسهٔ U+000A |

### تصمیم‌هایی که عمدی است

- **پیش‌فرض خاموش، و کلید از حالتِ متن جداست.** «خام» می‌گوید چقدر خط لوله می‌تواند متن را
  عوض کند؛ این کلید می‌گوید یک عبارت اصلاً **دستور** هست یا نه. هر دو با هم معنا دارند و
  پذیرشِ طرح («عبور خام مطابق گزینهٔ فرمان») همین است.
- **دو شکلِ معتبر، نه تطبیقِ متن‌به‌متن.** کلِ گفتار یک عبارت باشد، یا گفتار با واژهٔ
  `دستور`/`command` شروع شود. «یک خط جدید بزن» متن می‌ماند، چون برنامه‌ای که جملهٔ آدم را
  به خط جدید تبدیل کند بدون اجازه، از برنامه‌ای که هیچ کاری نمی‌کند خطرناک‌تر است.
- **نامعلوم = بدون عمل، بدون حذف.** `Op::Unknown` خودش تایپ می‌شود؛ حذفش یعنی پاک کردنِ
  سخنِ کاربر به خاطر چیزی که برنامه نمی‌فهمد. جمله‌ای که با «دستور» شروع می‌شود ولی فرمانی
  ندارد («دستور قاضی را اجرا کردند») همان‌طور که هست برمی‌گردد — این تست دارد.
- **پارسر کلید نمی‌فرستد.** خروجی ترتیبِ عملیات است؛ اینکه هر عمل کِی کلید بخورد کارِ
  `output` است. وگرنه پارسر و تزریق به هم گره می‌خوردند و تستِ پارسر بدون Win32 ممکن نبود.
- **`\n` یعنی Enter، و این به‌تنهایی یک اصلاح است.** پیش از این، متنِ حاوی newline با
  `KEYEVENTF_UNICODE` برای U+000A تایپ می‌شد که در بیشتر برنامه‌ها بی‌اثر است؛ حالا هم
  فرمانِ تازه و هم newlineِ موجود در متنِ خام به خط جدید می‌روند.

### بررسی‌ها

| | |
|---|---|
| `cargo test --all-targets` | `TEST_EXIT=0` — **۸۰۷ پاس / ۰ شکست / ۱۱ نادیده** (پیش‌تر ۷۸۸؛ +۱۹ تست تازه) |
| تست‌های تازه | ۱۲ در `commands.rs` (چهار معیارِ پذیرشِ طرح، پایداریِ اعمالِ دوباره، تا‌کِردنِ املا) + ۴ در `processing/mod.rs` (پیش‌فرض خاموش در هر حالت، بقاِ نشانه، خام+فرمان، متنِ اطرافِ فرمان) + ۳ در `output/injector.rs` (Enter در میانه، Enter در دو سر، برابریِ burst و paced) |
| `clippy --all-targets -D warnings` | `CLIPPY=0` — صفر خطا، صفر هشدار |

### آنچه آزموده نشده

**اجرای زنده.** فرمان را کسی نگفته و تزریقِ واقعیِ Enter را کسی ندیده؛ فقط همان قرارداد
در سه جا (پارسر، خط لوله، تزریق) به‌صورت خودکار آزموده شده. نسخهٔ نصب‌شدهٔ روی همین
سیستم ۰.۴.۰ است و قفل تک‌نسخه اجازهٔ اجرای هم‌زمان نمی‌دهد.

**پیش‌فرض خاموش است**، پس برای کاربرِ عادیِ امروز هیچ چیزی عوض نشده؛ روشنش کردن یک
تصمیمِ شخصی در تب تنظیمات است.

### باقی‌ماندهٔ رودمپ

بسته‌هایی که **هنوز اجرا نشده‌اند** (به‌روزرسانی پس از P2؛ P2 از این فهرست رفت):

| بسته | چیست | چرا مانده |
|---|---|---|
| `U1` | بازگردانیِ مشروط (اول داخلِ پیش‌نویسِ خودِ برنامه) | نیازمند `I4` که بسته شد؛ هنوز شروع نشده |
| `Q1` | بازبینیِ مستقلِ فقط‌خواندنی روی خطِ مبنای موج ۴ | باید پس از I5 تکرار شود؛ هنوز شروع نشده |
| `I5` | ادغامِ نهایی | پس از U1 و Q1 |

### همان روز، جدا از C1: ریلیز و نصب‌کننده

`v0.6.0` و `v0.6.1` منتشر شد (دومی رفعِ باگی که حذفِ نصب، `dictionary.toml` کاربر را
می‌پراند — اندازه‌گیری‌شده، قبل و بعد)، دو اینستالرِ موازی به یکی رسید و ادعاهای
کهنهٔ README و سه یادداشتِ ریلیز تصحیح شد. شواهد در
[INSTALLER-AUDIT.md](../INSTALLER-AUDIT.md).

---

## P2 — حالت نگارش رسمی (۲۰۲۶-۱۰-۰۶)

بستهٔ نخستِ موج ۵. قرارداد در [CONTRACTS بند ۱۱](CONTRACTS.md) ثبت شد؛ اینجا همان چیزی که
ساخته شد و آنچه آزموده نشده است.

### چه ساخته شد

| لایه | چه شد |
|---|---|
| ماژول | `processing/formal.rs` — `FormalOptions { punctuation, mixed_spacing }`، هر گروه تابعِ جدا با پرچمِ خودش؛ خالص، بدون I/O، فقط درجِ فاصله |
| حالت | `TextMode::Formal` با پذیرش `"formal"` و `"رسمی"`؛ شاخهٔ خودش در `process_text_with` با ترتیبِ نرمال‌ساز → formal → واژه‌نامهٔ عمومی، و فرمان‌ها (C1) همچنان آخر |
| کلیدها | `text.formal_punctuation` و `text.formal_mixed_spacing` (هر دو پیش‌فرض **true**) + `TextSettings::formal_options()`؛ دو چک‌باکس در تب تنظیمات که فقط وقتی حالت `formal` است دیده می‌شوند |
| اتصال | `GeneralRules.formal` → `EffectiveRules.formal` → `TextRules.formal` → `ProcessingOptions.formal`؛ پروفایل override نمی‌کند (همان استدلال `commands` در C1) |
| رابط | گزینهٔ «رسمی (نگارش رسمی)» در `MODE_CHOICES` پروفایل‌ها، برچسب «رسمی» در `mode_label`، و پیش‌نمایش‌های واژه‌نامه/پروفایل با همان مقدار |
| نرمال‌ساز | **اصلاحِ یک باگِ قدیمی که همین بسته آشکارش کرد:** مرحلهٔ چسباندنِ نشانه، نشانهٔ میانِ دو رقم را هم فاصله می‌داد — `نسخه 2.5` می‌شد `نسخه 2. 5` و `۱۲،۳۴` می‌شد `۱۲، ۳۴`، در **همهٔ حالت‌ها به‌جز raw**. اندازه‌گیری: `n.normalize("نسخه 2.5 را نصب کنید")` قبل از اصلاح ← `"نسخه 2. 5 …"` |

### تصمیم‌هایی که عمدی است

- **نگارش است، نه بازنویسی.** حالت فقط فاصله می‌گذارد: نشانه‌گذاری و مرزِ فارسی/لاتین و رقم.
  جملهٔ سالم دست نمی‌خورد، واژه بازنویسی نمی‌شود، نشانه اختراع نمی‌شود، و هیچ نویسه‌ای
  حذف نمی‌شود — اینها چهار تست جدا دارند، نه ادعا.
- **هر گروه با کلید خودش.** معیارِ طرح: «هر گروه قواعد قابل‌خاموش‌کردن باشد». هر دو خاموش
  باید دقیقاً همان متن بدهد، و یکی خاموش نباید دیگری را عوض کند — هر دو تست دارد.
- **فاصله فقط وقتی حرف بیاید.** `چرا؟!` و `۱۲،۳۴` و `نسخه 2.5` دست‌نخورده می‌مانند؛
  نشانهٔ پشتِ نشانه فاصله نمی‌گیرد و نشانهٔ آخرِ جمله چیزی اضافه نمی‌کند.
- **عمومی، نه پروفایلی.** کسی که با قاعده‌ای مخالف است می‌خواهد همه‌جا خاموش باشد، و
  پیش‌نمایش نباید بسته به مقصد دو شکل بگیرد.
- **پیش‌فرض حالت عوض نشد.** `mode` همچنان `standard` است؛ روشن کردنِ رسمی یک انتخاب است.

### بررسی‌ها

| | |
|---|---|
| `cargo test --all-targets` | `TEST_EXIT=0` — **۸۱۹ پاس / ۰ شکست / ۱۱ نادیده** (پیش‌تر ۸۰۷؛ +۱۲) |
| تست‌های تازه | ۶ در `formal.rs` (نمونه‌های طرح: محاوره/رسمی/نام خاص/فنی/اعداد/ترکیبی + عدم حذف + دوباره‌اعمالِ پایدار + خاموش‌شدنِ هر گروه + نشانهٔ پشتِ نشانه + اختراع‌نشدنِ نشانه) + ۴ در `processing/mod.rs` (هر دو املا، نبودنِ گروه‌ها در raw/conservative، استاندارد+فاصله، خاموش‌کردنِ گروه از مسیر خط لوله) + ۱ در `config/settings.rs` (کلیدها به خط لوله می‌رسند) + ۱ در `normalizer.rs` (نشانهٔ میانِ دو رقم) |
| `clippy --all-targets -D warnings` | `CLIPPY_EXIT=0` — صفر خطا، صفر هشدار |

### آنچه آزموده نشده

**اجرای زنده.** حالت را کسی در یک سند واقعی ندیده؛ نسخهٔ نصب‌شدهٔ روی همین سیستم ۰.۴.۰
است و قفل تک‌نسخه اجازهٔ اجرای هم‌زمان نمی‌دهد. طبق خودِ طرح، کیفیتِ حالت رسمی **با همان
نمونه‌ها گزارش شده، نه با ادعای صحتِ همهٔ فارسی**.

**مرزِ پوشش.** نشانه‌های `?` و `;` و `…` را فقط خودِ حالت می‌شناسد (نرمال‌ساز از همیشه
`، ؛ ؟ ! . : ,` را می‌شناخت)؛ یعنی این سه در حالت‌های دیگر همچنان بی‌فاصله می‌مانند. این
آگاهانه تقسیم شده تا رفتارِ حالت‌های موجود با این بسته عوض نشود.

### باقی‌ماندهٔ رودمپ

`U1` (بازگردانی مشروط)، `Q1` (بازبینی مستقل فقط‌خواندنی) و `I5` (ادغام نهایی) —
که در [برنامهٔ اجرایی](../AGENT-EXECUTION-PLAN.md) ثبت شده‌اند.
