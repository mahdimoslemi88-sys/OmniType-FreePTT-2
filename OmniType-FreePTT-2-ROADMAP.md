# نقشهٔ راه اجرایی OmniType-FreePTT-2

## هدف سند

این سند مسیر تبدیل OmniType-FreePTT-2 از یک Prototype/MVP امیدوارکننده به یک Beta پایدار و سپس یک محصول قابل انتشار برای Windows را مشخص می‌کند.

مبنای این roadmap، بررسی کد و ساختار فعلی پروژه است. اولویت با مشکلاتی است که می‌توانند باعث ضبط یا تایپ اشتباه، افت شدید عملکرد، قفل‌شدن برنامه، نشت حریم خصوصی یا شکست fallback شوند.

## تعریف وضعیت‌های خروجی

| وضعیت | معنی |
|---|---|
| Prototype | ایده و مسیر اصلی کار می‌کند، اما خرابی‌های مهم و پوشش تست ناکافی است. |
| Internal Alpha | فقط برای تیم توسعه و دستگاه‌های مشخص؛ دادهٔ واقعی حساس نباید بدون کنترل استفاده شود. |
| Private Beta | برای تعداد محدود کاربر، با لاگ تشخیصی کنترل‌شده و مسیر بازگشت نسخه. |
| Public Beta | برای کاربران عمومی، با installer، مستندات، گزارش خطا و سیاست حریم خصوصی. |
| Release Candidate | فقط خطاهای کم‌ریسک باقی مانده و معیارهای انتشار کامل شده‌اند. |

## اصول اجرایی

1. هر تغییر بحرانی باید با یک تست بازتولیدکننده همراه باشد.
2. قبل از بهینه‌سازی latency، صحت audio و state machine باید تثبیت شود.
3. هیچ ادعای عملکردی بدون benchmark قابل تکرار وارد README نشود.
4. مسیر آفلاین باید بدون API key و بدون شبکه قابل استفاده بماند.
5. cloud باید کاملاً opt-in، قابل مشاهده و قابل خاموش‌کردن باشد.
6. هر فاز باید checkpoint قابل اندازه‌گیری داشته باشد؛ «به نظر درست می‌رسد» معیار قبولی نیست.

---

## فاز صفر — تثبیت baseline و آماده‌سازی کار

### هدف

ساختن یک baseline قابل بازتولید تا تغییرات بعدی با حدس و احساس ارزیابی نشوند.

### کارها

- ثبت commit مبنا و ایجاد شاخهٔ کاری مخصوص hardening.
- ثبت نسخهٔ Rust، Windows SDK، Visual Studio Build Tools و وابستگی‌های native.
- ثبت فهرست کامل dependencyها و license آن‌ها.
- یکسان‌سازی ادعای تعداد تست‌ها در README ریشه و README داخلی.
- تبدیل اعداد فعلی README به دو دستهٔ «هدف طراحی» و «نتیجهٔ اندازه‌گیری‌شده».
- تهیهٔ fixtureهای صوتی ثابت:
  - گفتار فارسی کوتاه و بلند؛
  - سکوت؛
  - نویز پس‌زمینه؛
  - stereo و mono؛
  - نرخ‌های 16kHz، 44.1kHz و 48kHz.

### خروجی

- سند `BASELINE.md` شامل محیط، دستورهای build/test و محدودیت‌ها.
- حداقل یک گزارش benchmark اولیه برای latency، CPU، RAM و اندازهٔ خروجی.
- فهرست issueها با severity و معیار پذیرش.

### Checkpoint صفر

- شخص دیگری بتواند از روی سند، محیط را بسازد.
- `cargo fmt --check` و `cargo clippy` در محیط Windows قابل اجرا باشند.
- یک سناریوی ضبط و injection به‌صورت دستی ثبت‌شده داشته باشید.

---

## فاز یک — اصلاح هستهٔ audio و VAD

**اولویت: P0 — پیش‌نیاز همهٔ فازهای بعدی**

### هدف

اطمینان از اینکه نمونه‌های صوتی با زمان، نرخ و کانال درست وارد VAD و ASR می‌شوند.

### ۱. اصلاح پردازش تکراری VAD

- بافر capture را از صف مصرف VAD جدا کنید.
- برای هر consumer یک `read_cursor` یا صف chunk داشته باشید.
- فقط داده‌ای که از آخرین poll اضافه شده پردازش شود.
- اندازهٔ chunk و زمان poll را ثابت و قابل تنظیم کنید.
- تستی اضافه کنید که بافر ۵، ۳۰ و ۱۲۰ ثانیه‌ای را بررسی کند.
- تعداد frameهای پردازش‌شده را در حالت debug اندازه‌گیری کنید و نشان دهید با طول ضبط خطی رشد می‌کند، نه مربعی.

### ۲. استانداردسازی sample rate و channel

- config ورودی را بخوانید و نرخ واقعی device را ثبت کنید.
- stereo به mono با downmix مشخص تبدیل شود.
- هر نرخ غیر 16kHz با resampler معتبر به 16kHz تبدیل شود.
- اگر resampler آماده استفاده می‌شود، aliasing و کیفیت آن با fixture صوتی بررسی شود.
- نرخ و channel اعلام‌شده به VAD، Whisper، WAV و cloud باید از یک منبع واحد بیاید.
- اگر دستگاهی قابل تبدیل نیست، خطای واضح و قابل بازیابی نشان داده شود؛ نباید silently فرض شود 16kHz است.

### ۳. حذف تخصیص حافظه از callback

- callback فقط داده را به buffer اختصاص‌یافته منتقل کند.
- تبدیل I16/U16 به f32 به batch یا worker غیر real-time منتقل شود.
- از lock سنگین، log و عملیات شبکه داخل callback جلوگیری شود.
- underrun و overrun شمارش و قابل مشاهده شوند.

### ۴. اثبات ring buffer

- قرارداد producer/consumer را مستند کنید.
- overwrite همزمان و wrap-around را با stress test آزمایش کنید.
- در صورت امکان از پیاده‌سازی استاندارد SPSC استفاده کنید.
- برای بخش‌های unsafe invariant صریح و تست مستقل بنویسید.

### Checkpoint فاز یک

- ورودی‌های 16k mono، 48k stereo و 44.1k stereo به خروجی یکسان 16k mono تبدیل شوند.
- هیچ frame صوتی دوبار به VAD تحویل نشود.
- ضبط ۲ دقیقه‌ای بدون رشد غیرخطی CPU انجام شود.
- تست stress حداقل ۳۰ دقیقه بدون corruption یا drop غیرقابل توضیح اجرا شود.
- در صورت عدم پشتیبانی دستگاه، پیام قابل فهم و بازگشت امن به Idle انجام شود.

### معیار خروج

تا وقتی این فاز قبول نشده، benchmark دقت ASR یا release انجام نشود؛ چون ورودی صوتی هنوز قابل اعتماد نیست.

---

## فاز دو — بازطراحی state machine و چرخهٔ عمر برنامه

**اولویت: P0/P1**

### هدف

برنامه پس از خطا یا shutdown در وضعیت قابل پیش‌بینی بماند و برای ادامهٔ کار نیازمند restart نباشد.

### کارها

- حالت‌های `Idle`، `Recording`، `Processing`، `Injecting`، `Error` و `Stopping` را مستند کنید.
- برای هر event جدول transition بنویسید.
- هر خطای قابل بازیابی به `Idle` یا `Cooldown` برگردد.
- خطای غیرقابل بازیابی پیام کاربرپسند و مسیر restart کنترل‌شده داشته باشد.
- یک shutdown signal مشترک بین hotkey listener، audio stream، worker، tray و GUI ایجاد کنید.
- Ctrl+Alt+Q، tray quit و بسته‌شدن پنجره باید همان مسیر shutdown را اجرا کنند.
- shutdown باید idempotent باشد؛ اجرای دوبارهٔ آن crash یا deadlock ایجاد نکند.
- timeout برای workerهای ASR و cloud تعریف شود.
- هنگام خروج، stream صوتی، threadها و temporary resourceها به‌صورت قطعی آزاد شوند.

### Checkpoint فاز دو

- خطای مصنوعی ASR برنامه را در کمتر از یک چرخه به Idle برگرداند.
- خطای injection باعث قفل دائمی نشود.
- هر سه مسیر hotkey، tray و GUI برنامه را کامل و تمیز ببندند.
- تست shutdown پنجاه بار تکرار شود، بدون hang یا process باقی‌مانده.

---

## فاز سه — کامل‌کردن configuration و UX ایمن

**اولویت: P1**

### هدف

تمام گزینه‌های مستندشده واقعاً به runtime متصل باشند و رفتار تایپ قابل پیش‌بینی شود.

### کارها

- hotkeyها از config به listener تزریق شوند؛ هیچ hotkey اصلی hard-code نماند.
- `show_overlay` و `theme` به GUI منتقل و در startup اعمال شوند.
- threshold و پارامترهای VAD از config خوانده و validation شوند.
- برای مقادیر نامعتبر، default امن و warning واضح ارائه شود.
- focus مقصد هنگام شروع ضبط snapshot شود.
- قبل از injection بررسی شود مقصد هنوز همان پنجره است یا کاربر آگاهانه مقصد را تغییر داده است.
- گزینهٔ تأیید قبل از injection در نسخهٔ beta اضافه شود.
- حالت‌های recording، processing، cloud و error با indicator روشن نمایش داده شوند.
- متن خطا برای کاربر ساده باشد و جزئیات فنی فقط در diagnostic log کنترل‌شده ثبت شود.

### Checkpoint فاز سه

- تغییر هر گزینه در config بدون ویرایش کد قابل مشاهده و تست باشد.
- تعویض پنجره هنگام پردازش به تایپ ناخواسته در مقصد اشتباه منجر نشود.
- کاربر بداند صوت محلی است یا قرار است به cloud ارسال شود.
- overlay در حالت خاموش واقعاً نمایش داده نشود.

---

## فاز چهار — امنیت، حریم خصوصی و cloud

**اولویت: P1**

### هدف

اطمینان از اینکه دادهٔ صوتی، transcript و API key بدون اطلاع و کنترل کاربر مدیریت نمی‌شوند.

### کارها

- cloud به‌صورت پیش‌فرض خاموش بماند.
- قبل از نخستین upload، رضایت روشن و قابل لغو دریافت شود.
- indicator دائمی برای cloud recording/upload نمایش داده شود.
- متن خام گفتار از log عمومی حذف یا redact شود.
- API key در صورت امکان در Windows Credential Manager یا secret store نگه‌داری شود.
- اگر plaintext config باقی می‌ماند، هشدار و ACL مناسب اضافه شود.
- `base_url` به فهرست endpointهای مجاز محدود شود یا هشدار بسیار واضح داشته باشد.
- برای مدل‌های دانلودی hash و size validation اضافه شود.
- دانلود ناقص یا مدل خراب هرگز silently فعال نشود.
- temporary audio fileها در همهٔ مسیرها پاک شوند.
- سیاست retention برای transcript، log و quota نوشته شود.
- `cargo audit` و بررسی license در CI اجرا شود.

### Checkpoint فاز چهار

- با cloud خاموش، هیچ request شبکه‌ای ساخته نشود.
- با cloud روشن، کاربر قبل از upload پیام واضح ببیند.
- API key در log، crash report و خطای HTTP ظاهر نشود.
- hash مدل قبل از load بررسی شود.
- تست قطع شبکه، پاسخ 401، پاسخ 429 و پاسخ ناقص رفتار قابل پیش‌بینی داشته باشد.

---

## فاز پنج — ASR، fallback و quota قابل اعتماد

**اولویت: P1**

### هدف

مسیری بسازیم که در صورت شکست یک engine، بدون از دست‌دادن ضبط یا قفل‌شدن برنامه به engine بعدی برود.

### کارها

- interface مشترک engineها و error taxonomy مشخص شود.
- local engine و cloud engine با mock قابل تست باشند.
- timeout، retry محدود و backoff برای cloud تعریف شود.
- retry روی خطاهای غیرقابل تکرار مثل 401 انجام نشود.
- quota بعد از پاسخ واقعی و طبق قرارداد provider محاسبه شود.
- ذخیرهٔ quota با atomic replace سازگار با Windows انجام شود.
- crash وسط write و اجرای همزمان دو instance تست شود.
- cooldown تا نیمه‌شب و timezone به‌صورت شفاف تست شود.
- اگر local engine موجود نیست، پیام و مسیر نصب مدل واضح باشد.
- انتخاب engine و دلیل fallback در diagnostic log ثبت شود، بدون ثبت transcript.

### Checkpoint فاز پنج

- cloud timeout باعث برگشت امن به local شود.
- 401 باعث retry بی‌نهایت نشود.
- quota پس از restart و قطع برق منطقی باقی بماند.
- mock provider تمام مسیرهای success، empty، 401، 429، 500 و malformed response را پوشش دهد.

---

## فاز شش — تست جامع و CI ویندوز

**اولویت: P0 پیش از Beta**

### لایه‌های تست

#### Unit

- VAD و endpointing
- resampling/downmix
- normalizer و dictionary فارسی
- state transitionها
- config validation
- quota calculation

#### Integration

- capture تا VAD
- VAD تا ASR mock
- ASR تا injection mock
- fallback local/cloud
- shutdown کامل

#### System/E2E روی Windows

- WASAPI با میکروفن واقعی و مجازی
- tray و overlay
- hotkeyهای واقعی
- SendInput و UIPI
- تعویض پنجره هنگام پردازش
- خواب/بیداری سیستم
- چند monitor و DPI مختلف
- قطع شبکه و restart برنامه

### CI پیشنهادی

- Windows 10 و Windows 11
- `cargo fmt --check`
- `cargo test --all-targets`
- `cargo clippy --all-targets -- -D warnings`
- `cargo audit`
- تست‌های mock cloud
- build release
- تولید artifact با version مشخص
- ثبت checksum artifact

### Checkpoint فاز شش

- هیچ تست integration به شبکه یا API واقعی وابسته نباشد.
- تست‌ها silent skip نشوند؛ skip باید علت و وضعیت واضح داشته باشد.
- تست‌های غیر Windows hang نکنند.
- حداقل دو اجرای متوالی CI بدون flaky failure انجام شود.

---

## فاز هفت — benchmark و معیارهای محصول

### معیارهای اصلی

| معیار | روش اندازه‌گیری پیشنهادی |
|---|---|
| Latency | از پایان گفتار تا پایان injection، روی fixtureهای ۵، ۱۰ و ۳۰ ثانیه‌ای |
| WER فارسی | مجموعهٔ متن فارسی ثابت با transcript مرجع و گزارش تفکیک‌شده بر اساس نویز |
| CPU | میانگین و p95 هنگام capture و inference |
| RAM | baseline، هنگام load مدل و peak پردازش |
| دقت endpointing | false stop، late stop و missed stop |
| پایداری | تعداد session موفق از ۱۰۰۰ session آزمایشی |
| fallback | زمان و نرخ موفقیت پس از خطای engine اصلی |

### قواعد گزارش

- سخت‌افزار، مدل، نسخهٔ OS و تنظیمات ثبت شوند.
- نتیجهٔ میانگین همراه p50 و p95 باشد.
- target با measured result در README جدا باشد.
- benchmark قابل اجرای مجدد با یک دستور باشد.

### Checkpoint فاز هفت

- برای هر ادعای عددی یک script و artifact وجود داشته باشد.
- هیچ عددی بدون dataset، سخت‌افزار و نسخهٔ مدل منتشر نشود.

---

## فاز هشت — Internal Alpha

### دامنه

- ۳ تا ۵ دستگاه Windows با سخت‌افزار صوتی متفاوت.
- فقط کاربران توسعه یا افراد مورد اعتماد.
- cloud خاموش مگر برای تست مشخص.

### سناریوهای اجباری

- ۱۰۰ session کوتاه و بلند برای هر دستگاه.
- تغییر پنجره وسط پردازش.
- قطع و وصل میکروفن.
- sleep/wake.
- نبودن مدل.
- خراب‌بودن config.
- قطع شبکه.
- ورود API key اشتباه.

### خروجی

- crash-free session rate
- لیست خطاهای واقعی با severity
- گزارش تجربهٔ کاربر
- تصمیم go/no-go برای Beta

### معیار قبولی

- هیچ P0 باز نماند.
- خطای P1 فقط با workaround مستند باقی بماند.
- حداقل ۹۵٪ sessionهای تستی بدون restart کامل شوند.

---

## فاز نه — Private Beta

### کارها

- installer قابل حذف و نصب مجدد.
- version و migration برای config.
- crash report بدون transcript و API key.
- صفحهٔ راه‌اندازی و راهنمای حریم خصوصی.
- feature flag برای cloud و قابلیت‌های پرریسک.
- کانال بازخورد و قالب گزارش bug.
- امکان rollback به نسخهٔ قبلی.

### Checkpoint

- ۱۰ تا ۲۰ کاربر محدود، حداقل یک هفته استفادهٔ واقعی.
- هیچ incident امنیتی یا ارسال ناخواستهٔ cloud.
- نرخ session موفق و latency در محدودهٔ هدف محصول باشد.

---

## فاز ده — Public Beta و Release Candidate

### پیش‌نیاز Public Beta

- installer امضاشده یا دست‌کم hash قابل تأیید.
- صفحهٔ release notes.
- مستندات نصب مدل و حذف کامل برنامه.
- privacy notice و توضیح دقیق cloud.
- issue template و security contact.
- CI سبز برای آخرین commit.
- artifact reproducible یا حداقل قابل ردیابی.

### Release Candidate gate

- صفر P0 و صفر P1 بدون برنامهٔ اصلاح فوری.
- تست E2E روی حداقل سه نوع device صوتی.
- تست clean install، upgrade و uninstall.
- بررسی Windows Defender و false positive.
- تأیید دستی مسیرهای hotkey، tray، overlay، offline و fallback.

---

## پیشنهاد تقسیم کار

### Milestone A — Audio correctness

VAD cursor، resampling، downmix، ring buffer و callback allocation.

### Milestone B — Reliability

state machine، recovery، shutdown، timeout و focus safety.

### Milestone C — Configuration and privacy

config wiring، cloud consent، redaction، secret storage و checksum مدل.

### Milestone D — Verification

mockها، E2E Windows، CI، benchmark و مستندسازی ادعاها.

### Milestone E — Distribution

installer، signing، release notes، telemetry حداقلی و Private Beta.

## Definition of Done نهایی

پروژه زمانی آمادهٔ انتشار عمومی است که:

- مسیر capture تا injection در سخت‌افزارهای مختلف پایدار باشد.
- هیچ باگ P0 باز نباشد.
- خطاهای موقت بدون restart recover شوند.
- cloud کاملاً opt-in و قابل مشاهده باشد.
- مدل و artifact قابل اعتبارسنجی باشند.
- CI ویندوزی build، test، clippy و audit را پوشش دهد.
- benchmarkها قابل تکرار باشند.
- installer، uninstall، upgrade و rollback تست شده باشند.
- README فقط ادعاهای اثبات‌شده یا targetهای واضح داشته باشد.

## جمع‌بندی تصمیمی

مسیر پیشنهادی این است: ابتدا correctness صوت و reliability، سپس امنیت و configuration، بعد CI و benchmark، و فقط پس از آن Beta و انتشار. اگر تیم کوچک است، انتشار عمومی را تا پایان Milestone C عقب بیندازید؛ چون مشکلات فعلی می‌توانند مستقیماً باعث تایپ در پنجرهٔ اشتباه، شکست ضبط و قفل‌شدن برنامه شوند.
