# پایپ‌لاین صدای Antigravity — کشف و رمزگشایی

**تاریخ:** ۲۶ سپتامبر ۲۰۲۶
**روش:** اتصال CDP (Chrome DevTools Protocol) به اپ در حال اجرا + خواندن باندل خودش
**نتیجه:** Antigravity یک **تبدیل گفتار به متنِ زندهٔ استریمینگ** دارد — دقیقاً همان
چیزی که برای یک PTT زنده لازم داریم.

---

## ۱. چطور به آن رسیدیم

اپ Antigravity یک VS Code/Electron است و در خط فرمان پروسهٔ renderer اش
`--remote-debugging-port=0` دارد. یعنی **CDP از قبل روشن است** و پورت تصادفی
در فایل زیر نوشته می‌شود:

```
%APPDATA%\Antigravity\DevToolsActivePort     ->    2180
/devtools/browser/0faaed4e-...
```

نسخه‌ها: `Antigravity/2.17.0`, `Chrome/152.0.7977.78`, `Electron/44.3.0`.

با `observe-cdp.mjs --once --expr-file ...` باندل‌های واقعی اپ را از timeline
کارایی پیدا کردیم (`/main.js` ≈ ۹.۵MB) و داخلشان را خواندیم.

---

## ۲. پایپ‌لاین واقعی (از کد خود اپ)

```
navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, sampleRate: { ideal: 48000 },
             echoCancellation: true, noiseSuppression: true } })
        ↓
AudioWorklet  →  PCM chunk (Uint8Array)
        ↓
RPC دوجهته (Connect / gRPC-Web)  →  streamAudioTranscription({
        mimeType: "audio/pcm;rate=16000",   // قالب واقعی: JSON روی gRPC-Web (+json)
        cascadeId })                        // اندازه‌گیری‌شده ۲۶ سپتامبر — جلسهٔ ۲
        ↓   + sendAudioChunk({ sessionId, data, sequenceNumber })   // فقط پس از ready
        ↓
پاسخ‌های استریم:
        { case: "ready",         value: { sessionId } }
        { case: "transcription", value: { text, isFinal } }
        { case: "complete" }
        ↓
endAudioSession(sessionId)
```

### ⛩️ دروازهٔ `ready` — درسِ جلسهٔ ۲ (اندازه‌گیری‌شده)

بدنهٔ واقعی درخواست **JSON** است، نه protobuf؛ فقط دو فیلد دارد و درِ ورودی
`cascadeId` (شناسهٔ محاورهٔ جاری) است:

`Content-Type: application/grpc-web+json` ← `00 | 00 00 00 56 | {"mimeType":"audio/pcm;rate=16000","cascadeId":"…"}`

ترتیب کار هم چنین است: **اول `ready` می‌آید، بعد chunkها می‌روند.** اپ صدا را
محلی بافر می‌کند و تا نرسیدن `ready{sessionId}` هیچ `SendAudioChunk` ی نمی‌فرستد
(جلسهٔ ۱: ۱۲.۶ و ۱۴.۲ ثانیه انتظار، سپس تخلیهٔ یک‌جای ۳۱۴ chunk و قفل شدن روی
دقیقاً ۲۵/s). اگر کاربر پیش از `ready` دکمه را رها کند، اپ همان لحظه استریم را
Abort می‌کند و **کل صدا بی‌صدا گم می‌شود** — این حالت دیگر «حدس» نیست، ضبط شده
است: [`Antigravity-Log-Analysis-02.md`](Antigravity-Log-Analysis-02.md).

رویدادهای داخلی که در کنسول می‌بینیم:
`audio_transcription_started` / `audio_transcription_complete`
با فیلدهای `continuous`, `durationMs`, `textLength`.
(تا امروز فقط `started` ضبط شده — با فیلدهای `source`/`mode`؛ `complete` هنوز
دیده نشده، چون هیچ جلسهٔ خوبی با هوک فعال ضبط نشده است.)

اگر session بیفتد، این خط را می‌دهد:
```
[AudioStreaming] Failed to start: ConnectError: ...
[AudioStreaming] Error sending chunk #N: ...
[AudioStreaming] Stream error: ...
```

بک‌اند: سرویس **Cloud Code / Gemini Code Assist** است
(`core.cloudCodeService`, `loadCodeAssist`, `retrieveUserQuotaSummary`).
متن نهایی هم مستقیم داخل ادیتور (ProseMirror) درج می‌شود، نه در یک کادر جدا.

### چرا این مهم است

| ویژگی | Gemini/ChatGPT اپ | **Antigravity** |
| --- | --- | --- |
| نوع ورودی | toggle (بزن/قطع کن) | **استریمینگ پیوسته** |
| نرخ صدا | نامعلوم | **PCM 16kHz** (همان نرخ ما) |
| خروجی | متن نهایی | **`transcription {text, isFinal}` زنده** |
| دسترسی برنامه‌ای | بسته (باید CDP را با فلگ روشن کرد) | **CDP از قبل باز است** |

یعنی Antigravity تنها گزینه‌ای است که هم **زنده** است و هم **قابل مشاهده از
بیرون** — و فقط با یک لاگ‌خوانی می‌توان رفتارش را ضبط کرد.

---

## ۳. ابزار: `observe-cdp.mjs`

بدون هیچ dependency (فقط Node 21+ که `WebSocket` و `fetch` سراسری دارد).

```powershell
cd v-2\docs\reaserch\bridge

# ۱۲۰ ثانیه ضبط کن؛ در این مدت در Antigravity روی دکمهٔ میکروفون بزن و حرف بزن
node observe-cdp.mjs --app antigravity --duration 120 --poll 1000

# یک عبارت JS را در صفحه اجرا کن و خارج شو (برای کشف سریع)
node observe-cdp.mjs --app antigravity --once --expr "document.title"
node observe-cdp.mjs --app antigravity --once --expr-file probes/antigravity-audio.expr.js
```

چه چیزی ثبت می‌کند:
- **هر درخواست شبکه‌ای** (`Network.requestWillBeSent`) → معلوم می‌شود صدا به کجا می‌رود
- **WebSocket ها** + اندازهٔ frame ها → استریمینگ را نشان می‌دهد
- **کنسول** (شامل خطوط `[AudioStreaming]`)
- **متن عنصر فعال** هر ثانیه → خودِ ترنسکریپت زنده

خروجی: `logs\cdp-antigravity-<stamp>.jsonl` + `-summary.md`.

---

## ۴. سؤال‌هایی که لاگ جواب داد ✅ (جلسهٔ ۱، ۲۶ سپتامبر ۲۰۲۶)

تجزیهٔ کامل در [`Antigravity-Log-Analysis-01.md`](Antigravity-Log-Analysis-01.md).

- [x] endpoint: **`https://127.0.0.1:2182/exa.language_server_pb.LanguageServerService/`**
  — یعنی سرور محلی `language_server.exe`، نه host گوگل. خودش به
  `https://daily-cloudcode-pa.googleapis.com` پروکسی می‌کند.
- [x] پروتکل: **fetch** (Connect/gRPC-Web)، یک POST برای هر chunk؛ WebSocket صفر.
- [x] نرخ: **۲۵ chunk/s = هر ۴۰ms** (PCM 16kHz mono ⇒ ۱۲۸۰B)
- [x] تأخیر `isFinal`: ≈ ۱–۲ ثانیه پس از قطع؛ پارشیال زنده دارد.
- [x] احراز هویت: هدر محلی **`x-codeium-csrf-token`** (از `--csrf_token` خط فرمان
  زبان‌سرور). بدون آن → `401`، با آن → `200`.

---

## ۵. دو سطح استفاده

1. **مشاهده (تکمیل شد):** رفتار ضبط شد و endpoint مشخص شد.
2. **استفاده به‌عنوان موتور (باید تصمیم بگیریم):** توکن حساب کاربر لازم
   **نیست** — چون سرور محلی خودش آن را دارد و فقط CSRF می‌خواهد. ولی پروتکل
   غیررسمی/minified است، به نصب و حساب Antigravity گره می‌خورد، و **مرزِ
   شرایط استفاده** دارد. هر آپدیت اپ می‌تواند پروتکل را عوض کند.

---

## ۶. مراجع

- [`observe-cdp.mjs`](observe-cdp.mjs)
- [`probes/antigravity-audio.expr.js`](probes/antigravity-audio.expr.js)
- [`ChatGPT-Gemini-Bridge.md`](ChatGPT-Gemini-Bridge.md)
- [`Antigravity-Log-Analysis-02.md`](Antigravity-Log-Analysis-02.md) — جلسهٔ ۲: بدنهٔ JSON، دروازهٔ `ready`، و حالت شکست
