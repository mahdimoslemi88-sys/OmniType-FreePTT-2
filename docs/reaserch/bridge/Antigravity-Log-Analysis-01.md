# تحلیل لاگ جلسهٔ ۱ — Antigravity

**فایل خام:** `logs/cdp-antigravity-2026-09-26T20-02-12.jsonl` (۲۵۶۰ خط، ۶۴۷KB)
**خلاصهٔ خودکار:** `logs/cdp-antigravity-2026-09-26T20-02-12-summary.md`
**مدت ضبط:** ۱۱۴.۷ ثانیه | **پورت CDP:** ۲۱۸۰ | **هدف:** تب Antigravity پروژهٔ K1-SITE

---

## ۱. پاسخ کوتاه: صدا به کجا می‌رود؟

> **صدا هیچ‌وقت مستقیم از اپ به گوگل نمی‌رود.**
> همهٔ ۲۱۶۶ درخواست شبکه به `https://127.0.0.1:2182` می‌روند — یعنی به
> **`language_server.exe` خودِ Antigravity که روی همین سیستم اجرا می‌شود.**
> خودِ آن پروسه صاحب توکن حساب کاربر است و صدا را به بک‌اند ابری پروکسی می‌کند.

این مهم‌ترین نتیجهٔ جلسه است: به‌جای شکار کردن host گوگل، یک **درگاه محلی و
مستند** پیدا کردیم.

---

## ۲. آنچه در لاگ ثبت شد

| `kind` | تعداد | معنی |
| --- | --- | --- |
| `request` | ۲۱۶۶ | درخواست‌های شبکه |
| `console` | ۳۴۶ | پیام‌های کنسول |
| `text` | ۴۶ | نمونهٔ متن ادیتور (خودِ ترنسکریپت زنده) |
| `target` | ۱ | تب هدف CDP |
| `summary` | ۱ | خط پایانی |

**هیچ WebSocket ای مشاهده نشد** (`WebSockets: none`). یعنی استریمینگ صدا روی
WebSocket نیست؛ **Connect/gRPC-Web روی `fetch`** است — به‌ازای هر chunk یک
درخواست HTTP جدا.

### توزیع مسیرها

| مسیر | تعداد | نقش |
| --- | --- | --- |
| `…LanguageServerService/SendAudioChunk` | **۲۱۴۷** | ارسال هر chunk صدا |
| `…LanguageServerService/StreamAudioTranscription` | ۲ | **شروع** جلسهٔ ترنسکریپشن |
| `…LanguageServerService/EndAudioSession` | ۲ | **پایان** جلسه |
| `…LanguageServerService/RecordAnalyticsEvent` | ۱۳ | تلمتری (شامل `audio_transcription_started/complete`) |
| `…LanguageServerService/UpdateConversationAnnotations` | ۱ | حاشیه‌نویسی محاوره |
| `…LanguageServerService/GetMendelFlags` | ۱ | فلگ‌های feature |

### ساختار جلسه (از روی timestamp)

```
۲۰:۰۲:۲۶.۲۶۶  StreamAudioTranscription   ← شروع استریم (تلاش ۱)
۲۰:۰۲:۳۸.۸۶۴  اولین SendAudioChunk       ← ۱۲.۵۹۸s بعد
۲۰:۰۳:۲۰.۹۶۱  EndAudioSession            → ۱۳۶۸ chunk در ۴۲.۱s
   (فاصلهٔ ۱۹.۷s — دکمه رها شد و دوباره زده شد)
۲۰:۰۳:۲۶.۴۵۴  StreamAudioTranscription   ← شروع استریم (تلاش ۲)
۲۰:۰۳:۴۰.۶۸۳  اولین SendAudioChunk       ← ۱۴.۲۲۹s بعد
۲۰:۰۳:۵۷.۶۲۰  EndAudioSession            → ۷۷۹ chunk در ۱۷.۰s
```

> اعداد بالا از دادهٔ خام بازخوانی شده‌اند (نسخهٔ اول این بلوک چند timestamp را
> ۴ ثانیه جابه‌جا و تلاش ۲ را ۲۰:۰۳:۳۰ نوشته بود؛ مقدار درست ۲۰:۰۳:۲۶.۴۵۴ است).

خط کنسول مربوطه (فقط یک‌بار):
```
[AudioStreaming] Failed to start: ConnectError: [canceled] signal is aborted without reason
```
> ⏮️ **تصحیح پس از جلسهٔ ۲ (۲۶ سپتامبر):** این تفسیر درست نبود. timestamp خودِ
> پیام در همین فایل `20:02:12.651Z` است — یعنی **پیش از** نخستین
> `StreamAudioTranscription` این جلسه (`20:02:26.266Z`)؛ پس یک پیامِ replayشدهٔ
> کنسول از تلاشی است که *قبل از شروع ضبط* انجام شده بود. آن ۱۲.۶ ثانیه هم
> نتیجهٔ «تلاش ناموفق» نبود، بلکه **انتظار برای `ready`** بود — همان چیزی که در
> جلسهٔ ۲ دوباره و این بار ناتمام دیده شد. جزئیات:
> [`Antigravity-Log-Analysis-02.md`](Antigravity-Log-Analysis-02.md) بند ۴.

---

## ۳. مشخصات استریم صدا (فارغ از حدس)

- **نرخ chunk:** دقیقاً **۲۵ chunk در ثانیه** در کل جلسه (۲۵ در هر ثانیهٔ
  ثابت). ⇒ فاصلهٔ هر chunk = **۴۰ میلی‌ثانیه**.
- با `mimeType: "audio/pcm;rate=16000"` و mono و PCM16 ⇒ هر chunk ≈
  **۱۲۸۰ بایت ≈ ۶۴۰ نمونه** (۴۰ms × 16kHz × 2 بایت × ۱ کانال).
- **انفجار آغازین:** در همان ثانیهٔ اول، ۳۱۴ chunk در جلسهٔ ۱ و ۳۶۳ chunk در
  جلسهٔ ۲ ثبت می‌شود. یعنی اپ **از لحظهٔ زدن دکمهٔ میکروفون ضبط می‌کند و در
  حافظه بافر می‌کند**، و وقتی اتصال برقرار شد یکجا تخلیه می‌کند. (به همین
  دلیل هم آن ۱۲.۶ ثانیهٔ تأخیر اولِ جلسهٔ اول، صدا را از دست نداده.)
- **استریمینگ دوطرفه:** متنِ بازگشتی به‌صورت **پارشیال و تدریجی** می‌آید —
  نمونه‌های `text` در خلاصه نشان می‌دهد یک جمله کلمه‌به‌کلمه بلند می‌شود و
  بعد نهایی می‌شود. پس فیلد `isFinal` واقعاً کار می‌کند.
- **تأخیر متن:** اولین نمونهٔ متن ۵.۰ ثانیه بعد از اولین chunk
  (`۲۰:۰۲:۳۸.۸` → `۲۰:۰۲:۴۳.۹`). پایان جلسه ۲۰:۰۳:۲۰، متن نهایی تا ۲۰:۰۳:۲۲
  تثبیت شده ⇒ **تأخیر finalize ≈ ۱–۲ ثانیه**.
- **رندر متن:** ۳۳۲ بار پیام کنسول `Unknown node type: ghost-text` — یعنی متن
  زنده به‌عنوان گرهٔ `ghost-text` در ادیتور ProseMirror درج می‌شود، نه در یک
  کادر جدا. (این توضیح می‌دهد چرا در `observe-asr.ps1` هیچ Edit/Document
  ای پیدا نشد.)

---

## ۴. بک‌اند واقعی — از خط فرمان خودِ سرور

از `Get-CimInstance Win32_Process` روی PID ۱۶۱۰۸:

```
"C:\…\Antigravity\resources\bin\language_server.exe" --standalone
    --override_ide_name antigravity --subclient_type hub
    --override_ide_version 2.17.0 --override_user_agent_name antigravity
    --https_server_port 0
    --csrf_token b59284eb-da36-4868-bea1-653e9c8dfc84
    --app_data_dir antigravity
    --api_server_url https://generativelanguage.googleapis.com
    --cloud_code_endpoint https://daily-cloudcode-pa.googleapis.com
    --enable_sidecars
    --host_bridge_url=http://127.0.0.1:2181
    --host_bridge_token=438878b1bb1e3dd8c511b05e33fe14d471bb1428824620bbb76467a75b0b34d7
```

نکات کلیدی:

1. **`--cloud_code_endpoint https://daily-cloudcode-pa.googleapis.com`** ⇒
   بک‌اند ترنسکریپشن همان **Google Cloud Code / Gemini Code Assist** است
   (کانال `daily` = پیش‌انتشار). `--api_server_url` هم Gemini API معمولی است.
2. **`--csrf_token <uuid>`** ⇒ سرور محلی با یک توکن CSRF محافظت می‌شود. توکن
   **هر اجرا عوض می‌شود** و روی خط فرمان پروسه است.
3. **`--https_server_port 0`** ⇒ خودش پورت نمی‌گیرد؛ سرور پورت را انتخاب می‌کند
   (اینجا ۲۱۸۲) و از طریق IPC به renderer می‌گوید. برای کشف از بیرون:
   `netstat -ano | grep <PID language_server.exe>`.
4. `--enable_sidecars`، `--host_bridge_url=http://127.0.0.1:2181` و
   `cua-driver.exe` ⇒ یک لایهٔ «کنترل کامپیوتر» هم اجرا می‌شود. جالب اینکه خود
   LS هم به CDP سوئیچ می‌زند:
   ```
   [CDP Discovery] Successfully discovered Electron WS URL:
       ws://127.0.0.1:2180/devtools/browser/0faaed4e-…
   ```
5. در `logs/language_server.log` **هیچ ردی از URL ترنسکریپشن نیست** (۰ تطابق
   برای `audio`/`transcri`)؛ فقط `streamGenerateContent` و `loadCodeAssist` و
   `fetchAvailableModels` لاگ می‌شوند. پس مسیر صدا از یک زیرسیستم جدا رد
   می‌شود که در سطح لاگ فعلی ثبت نمی‌شود.

### آزمون احراز هویت (عملی، همین حالا)

```
POST https://127.0.0.1:2182/exa.language_server_pb.LanguageServerService/GetMendelFlags
   بدون هدر            →  401
   با x-codeium-csrf-token: <توکن>  →  200
```

نام هدر از خود باندل اپ تأیید شد: `c.header.set("x-codeium-csrf-token", a)`.

> **نتیجهٔ عملی:** سرور محلی زبان فقط با یک هدر قابل صدا زدن است، و آن هدر را
> می‌توان از خط فرمان `language_server.exe` خواند. یعنی از نظر فنی
> **می‌توانیم صدای خودمان را به همان RPC بدهیم.** (این «می‌توانیم» است، نه
> «باید» — بند ۶ ریسک‌ها را ببینید.)

---

## ۵. پاسخ چک‌لیستِ `Antigravity-Voice-Pipeline.md`

| پرسش | پاسخ |
| --- | --- |
| endpoint دقیق استریم صدا؟ | `https://127.0.0.1:2182/exa.language_server_pb.LanguageServerService/{StreamAudioTranscription, SendAudioChunk, EndAudioSession}` — سرور محلی؛ اپ به `daily-cloudcode-pa.googleapis.com` پروکسی می‌کند |
| WebSocket یا fetch؟ | **fetch** (Connect/gRPC-Web)، یک HTTP POST برای هر chunk؛ WebSocket صفر |
| فارسی را چطور می‌فهمد؟ | در لاگ ما زبان صریحی در RPC دیده نشد؛ اپ احتمالاً از زبان UI محاوره یا مدل چندزبانهٔ Cloud Code استفاده می‌کند. فارسی **درست** برگشت |
| تأخیر `isFinal`؟ | ≈ ۱–۲ ثانیه پس از قطع؛ پارشیال‌ها در جریان گفتار پیوسته می‌آیند |
| توکن احراز هویت؟ | در اختیار `language_server.exe`؛ renderer فقط CSRF محلی را می‌فرستد |

---

## ۶. پیامد برای OmniType-FreePTT

سه مسیر روی میز است؛ پیشنهاد من **مسیر ۲** است.

**مسیر ۱ — تماشای صرف (کم‌ریسک، کم‌ارزش):** فقط همان `observe-cdp.mjs`. برای
تحقیق خوب است ولی چیزی به محصول اضافه نمی‌کند.

**مسیر ۲ — «Live Dictation» روی سرور محلی (پیشنهادی):** یک موتور جدید بسازیم که
مثل خود Antigravity با `language_server.exe` حرف بزند:
- پورت را با `netstat` روی PID زبان‌سرور پیدا کن؛
- `--csrf_token` را از خط فرمان همان PID بخوان؛
- `x-codeium-csrf-token` را ست کن و `StreamAudioTranscription` →
  `SendAudioChunk` (۴۰ms/۱۲۸۰B) → `EndAudioSession` را با PCM 16kHz خودمان
  صدا بزن؛ پارشیال‌های `transcription{text,isFinal}` را مستقیم بگیر.

مزیت: **بدون API key**، استریمینگ واقعی، همان بافر موجود اپ ما (۱۶kHz) بدون
هیچ resample. مشکل: به نصب و حساب Antigravity کاربر گره می‌خورد، پروتکل
miniified/غیررسمی است، و **مرزِ شرایط استفاده** دارد.

**مسیر ۳ — موتور whisper استریمینگ محلی با همان قرارداد:** trait
`LiveDictationEngine { begin/finish }` را بسازیم و پشتش whisper محلی بگذاریم؛
لایهٔ RPC مسیر ۲ هم می‌تواند بعداً به‌عنوان یک پیاده‌سازی دیگر همان trait را
بگیرد. این کار قابل اتکا و بی‌ریسک است ولی «رایگان بودنِ» مسیر ۲ را ندارد.

> **تصمیم باقی‌مانده برای کاربر:** آیا ریسکِ گره خوردن به Antigravity (مسیر ۲)
> را می‌پذیریم یا اول یک موتور استریمینگ محلی می‌سازیم (مسیر ۳)؟

---

## ۷. کاستی‌های همین ضبط (برای جلسهٔ بعد)

- بدنهٔ درخواست‌ها ثبت نشد؛ `SendAudioChunk` فقط به‌صورت URL دیده می‌شود.
  برای دیدن protobuf واقعی باید از `Fetch.requestPaused` با
  `Network.getRequestPostData` استفاده کنیم.
- `--poll 1000` بود؛ برای اندازه‌گیری نرخ واقعی پارشیال‌ها `--poll 200` بهتر است.
- `RecordAnalyticsEvent` ها فیلدهای `durationMs/textLength` دارند — اگر بدنه‌شان
  را بگیریم، عدد دقیق تأخیر را خودِ اپ می‌گوید.
- نقطهٔ شروع را باید بلافاصله قبل از زدن دکمهٔ میکروفون بگیریم تا آن ۱۲ ثانیهٔ
  شروعِ نافرجام تکرار نشود.

---

## ۸. ابزار نسخهٔ ۲ — ضبط با بدنهٔ درخواست‌ها

هر سه کاستی بند ۷ با دو فایل جدید حل شده است:

| ابزار | کار |
| --- | --- |
| `probes/asr-fetch-hook.js` | داخل صفحه `window.fetch` را می‌پیچد و **بایت‌های دقیق** هر RPC زبان‌سرور را base64 می‌فرستد (درخواست *و* پاسخ) |
| `probes/proto-dump.mjs` | همان base64 را بدون فایل `.proto` می‌خواند: شمارهٔ فیلد، نوع wire، متن، و اگر بایت‌ها PCM باشند `rms`/`peak` |

`observe-cdp.mjs` هم دو امکان گرفت: `--hook` (تزریق) و `--list`/`--target`
(انتخاب پنجرهٔ درست — چون اپ چند هدف دارد و ضبط به اشتباه روی پنجرهٔ
onboarding/login می‌افتد).

### دستور ضبط (همان `record-cdp.cmd`)

```cmd
node "%~dp0observe-cdp.mjs" --app antigravity --duration 150 --poll 200 --hook "%~dp0probes\asr-fetch-hook.js"
```

### آنچه باید در ضبط بعدی انجام دهی

1. پنجرهٔ **پروژه** (یک محاوره) باز باشد، نه onboarding/login — اول
   `node observe-cdp.mjs --app antigravity --list` را بزن.
2. خط `attached -> …` را نگاه کن و مطمئن شو همان پنجرهٔ پروژه است.
3. **حدود ۲ ثانیه بعد** از شروع ضبط، دکمهٔ میکروفون را بزن، ~۱۰–۱۵ ثانیه
   فارسی بگو، و رها کن.
4. بعد از پایان، بدنه‌ها را با این بخوان:

```cmd
node probes\proto-dump.mjs --jsonl logs\cdp-antigravity-<stamp>.jsonl --rpc StreamAudioTranscription
node probes\proto-dump.mjs --jsonl logs\cdp-antigravity-<stamp>.jsonl --rpc SendAudioChunk --limit 2
```

انتظار ما از بدنه‌ها: `SendAudioChunk{sessionId, data(bytes), sequenceNumber}` با
`data` به طول ۱۲۸۰B و مقدار PCM غیرصفر (`rms` بالا)، و پاسخ
`StreamAudioTranscription` شامل `ready{sessionId}` و بعد
`transcription{text, isFinal}`. با همین، موتور «Live Dictation» قابل ساخت است.

> اگر `bodyKind` در لاگ `stream` بود (بدنه یک `ReadableStream` بوده)، یعنی اپ
> صدا را به‌صورت استریمینگ آپلود می‌کند و باید هک را به `TransformStream`
> ارتقا دهیم — همان ضبط این را هم روشن می‌کند.

### یادآوری مسیر شبکه

گام محلی (`127.0.0.1:<port زبان‌سرور>`) از VPN رد نمی‌شود؛ فقط گام ابری یعنی
`daily-cloudcode-pa.googleapis.com` از VPN می‌رود. جزئیات و تنظیمات مسیردهی در
`Antigravity-VPN-Endpoints.md`.
