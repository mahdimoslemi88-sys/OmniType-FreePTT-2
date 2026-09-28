# پل زدن به اپ‌های ChatGPT و Gemini برای تبدیل ویس به متن

**تاریخ:** ۲۶ سپتامبر ۲۰۲۶
**وضعیت:** تحقیق در جریان — ابزار مشاهده آماده است، داده‌ی رفتاری هنوز جمع نشده
**هدف:** بررسی اینکه آیا می‌توان از اپ‌های دسکتاپ ChatGPT و Gemini به‌عنوان یک
موتور تبدیل گفتار به متن (ASR) برای OmniType-FreePTT استفاده کرد یا نه.

---

## ۱. یافته‌های reconnaissance (روی همین دستگاه)

قبل از هر تصمیمی، خود اپ‌ها را بازرسی کردیم:

| اپ | مسیر | فناوری | پروسه‌ها |
| --- | --- | --- | --- |
| Gemini Desktop | `%LOCALAPPDATA%\Google\Gemini\app-1.12.3\Gemini.exe` | **Electron 44.2.0** (`app.asar` + `gemini_native.node`) | main, renderer×2, gpu, **AudioService**, NetworkService, crashpad |
| ChatGPT / Codex | `C:\Program Files\WindowsApps\OpenAI.Codex_26.901.6511.0_x64__2p2nqsd0c76g0\app\` | **Chromium کامل** (`chrome.dll` v152) + `ChatGPT.exe` + `Codex.exe` + `chrome_proxy.exe` | بستهٔ MSIX |
| Gemini Web (PWA) | `C:\Program Files\Google\Chrome\Application\chrome_proxy.exe` | Chrome app-mode window | همان پروسه‌های Chrome |

**نتیجهٔ کلیدی:** هر دو اپ **Chromium-based** هستند. این یعنی هر دو پروتکل
**Chrome DevTools (CDP)** را می‌فهمند — و همین می‌شود نقطهٔ ورودِ پل.

> **اضافهٔ مهم (Antigravity):** اپ **Antigravity** روی این دستگاه نه‌فقط Chromium
> است، بلکه **CDP اش از قبل باز است** (`%APPDATA%\Antigravity\DevToolsActivePort`)
> و یک پایپ‌لاین **تبدیل گفتار به متنِ زندهٔ استریمینگ** دارد (PCM 16kHz با RPC
> دوجهته). این نزدیک‌ترین گزینه به معماری ماست. جزئیات کامل در
> [`Antigravity-Voice-Pipeline.md`](Antigravity-Voice-Pipeline.md).

نکات جانبی:
- اپ Gemini یک utility به اسم `audio.mojom.AudioService` دارد و کد native
  (`gemini_native.node`) هم همراه دارد → دیکته زنده از داخل renderer از طریق
  Web Speech انجام می‌شود و صدا به سرورهای گوگل می‌رود (هیچ مدل محلی‌ای نصب نشده).
- `chrome_proxy.exe` فقط یک لانچر برای حالت app-mode کروم است.
- پورت ۹۲۲۲ روی سیستم باز است ولی مال برنامهٔ LenovoVantage (Edge) است، نه این اپ‌ها.

---

## ۲. سه مسیر ممکن برای «پل»

### مسیر A — اتصال CDP (توصیه‌شده برای تحقیق)

هر دو اپ را با یک پورت دیباگ بالا بیاوریم:

```powershell
# Gemini (Electron)
& "$env:LOCALAPPDATA\Google\Gemini\app-1.12.3\Gemini.exe" `
    --remote-debugging-port=9334 --force-renderer-accessibility

# ChatGPT / Codex (Chromium باندل‌شده)
& "C:\Program Files\WindowsApps\OpenAI.Codex_26.901.6511.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe" `
    --remote-debugging-port=9333 --force-renderer-accessibility

# Gemini web (app-mode)
& "C:\Program Files\Google\Chrome\Application\chrome.exe" `
    --remote-debugging-port=9335 --app=https://gemini.google.com/app
```

با CDP می‌توانیم:
1. **متن کادر ورودی را مستقیم بخوانیم** (DOM) → harvest کردن transcript بدون شبیه‌سازی کیبورد.
2. دکمهٔ میکروفون اپ را کلیک کنیم یا `webkitSpeechRecognition` را در کانتکست صفحه صدا بزنیم.
3. بفهمیم app دقیقاً چه endpointی را برای STT صدا می‌زند (`Network.enable`).

محدودیت قطعی: **این اپ‌ها فقط میکروفون زنده را می‌پذیرند**؛ هیچ‌کدام APIی
ندارند که «یک بافر صوتی» بگیرد و متن بدهد. پس برای پایپ‌لاین فعلی ما
(`Vec<f32>` → متن) مستقیم قابل استفاده نیستند.

### مسیر B — UI Automation

خواندن کادر چت با UIA. تست زنده نشان داد روی پنجرهٔ Gemini
**۰ عنصر Edit/Document** برمی‌گردد، چون Chromium فقط وقتی درخت دسترس‌پذیری را
می‌سازد که با `--force-renderer-accessibility` اجرا شود. شکننده و کند است؛
فقط به‌عنوان fallback.

### مسیر C — استخراج همان API ابری

اپ‌ها توکن حساب کاربر را دارند و به OpenAI/Google وصل می‌شوند. می‌شود توکن را
از پروفایل اپ خواند و مستقیم به endpoint زد. اما این کار **از نظر شرایط استفاده
مرز دارد** و به توکن حساب کاربر وابسته است. برای Google، ما همین حالا نسخهٔ
قانونی‌ترش را داریم: `src/asr/google.rs` (Chromium Speech v2، بدون کلید).

---

## ۳. حکم صادقانه: آیا بدون هیچ هزینه‌ای «موتور پرمیوم» می‌گیریم؟

- برای **جایگزین کردن بافر ما**: نه. اپ‌ها بافر نمی‌گیرند.
- برای یک **حالت دیکتهٔ زنده (Live Dictation)** جدید: **بله، کاملاً عملی است** و
  دقیقاً همان «قدرت» ChatGPT/Gemini را می‌دهد.

ایدهٔ معماری پیشنهادی:

```
OmniType در حالت Live Dictation:

[نگه‌داشتن Caps Lock]
      ↓
فوکوس کردن پنجرهٔ اپ مقصد (Gemini / ChatGPT) از طریق CDP یا SetForegroundWindow
      ↓
فعال‌کردن میکروفون خودِ آن اپ (CDP ==> کلیک/JS)
      ↓
[رها کردن Caps Lock]
      ↓
خواندن متن نهایی از DOM آن اپ (CDP Runtime.evaluate)
      ↓
پاک‌کردن کادر اپ + نرمال‌سازی فارسی + SendInput به پنجرهٔ کاربر
```

این با trait فعلی `AsrEngine` (که `&AudioUtterance` می‌گیرد) **جور نیست**؛
چون ما صدا را ضبط نمی‌کنیم. پس لازم است یک trait موازی، مثلاً
`LiveDictationEngine { fn begin(&self, target) -> Result<()>; fn finish(&self) -> Result<String>; }`
اضافه شود و در GUI کنار engineهای موجود انتخاب شود.

مزیت: دقت ابری ChatGPT/Gemini، بدون هیچ کلید API و بدون مدل محلی.
هزینه: صدا از دستگاه خارج می‌شود (همان‌قدر که خود کاربر با آن اپ‌ها دارد)،
وابسته به اینترنت، و شکننده در برابر تغییر UI آن اپ‌ها.

---

## ۴. ابزار مشاهده — چطور رفتارشان را ضبط کنیم

`observe-asr.ps1` در همین پوشه، پروفایل کاملی از رفتار اپ‌ها هنگام تبدیل ویس
به متن می‌سازد. کاملاً read-only است (نه ورودی تزریق می‌کند، نه صدا می‌خواند).

```powershell
cd v-2\docs\reaserch\bridge
powershell -ExecutionPolicy Bypass -File observe-asr.ps1 -DurationSec 180 -ResolveDns
```

سپس در اپ Gemini (یا ChatGPT) چند بار دیکته کنید و وسطِ گفتار **Enter** بزنید
تا marker بگذارد. در پایان دو فایل ساخته می‌شود:

- `logs\asr-observation-<stamp>.jsonl` — داده خام، هر تیک یک JSON.
- `logs\asr-observation-<stamp>-summary.md` — خلاصهٔ خوانا.

هر تیک این‌ها را ثبت می‌کند: پنجرهٔ فعال، پروسه‌ها + CPU/RAM، اتصالات TCP هر
PID (با rDNS)، زمان آخرین استفاده از میکروفون
(`HKCU\...\CapabilityAccessManager\ConsentStore\microphone`)، و تغییرات متن
کادر ورودی از طریق UI Automation. هزینهٔ هر تیک ~۱۱۰ms است.

نتایج تست ابزار روی همین دستگاه:
- تیک اولیه ۷۲۰۰ms بود (چون `Get-NetTCPConnection -OwningProcess` هر PID را
  جدا اسکن می‌کند)؛ با `netstat -ano` به ~۱۱۰ms رسید.
- UI Automation روی پنجرهٔ Gemini بدون فلگ دسترس‌پذیری: صفر عنصر.

---

## ۵. چک‌لیست فرضیه‌ها (باید با لاگ پر شود)

به [`observations-log.md`](observations-log.md) مراجعه کنید.

---

## ۶. مراجع

- [`../../voice-ptt/src/asr/google.rs`](../../../voice-ptt/src/asr/google.rs) — موتور Google Free فعلی
- [`../../voice-ptt/src/asr/engine.rs`](../../../voice-ptt/src/asr/engine.rs) — trait `AsrEngine`
- [`Web Speech API.md`](../Web%20Speech%20API.md) — تحقیق قبلی روی Web Speech
