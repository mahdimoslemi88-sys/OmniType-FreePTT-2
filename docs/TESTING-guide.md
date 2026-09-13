# 🧪 راهنمای تست end-to-end — voice-ptt

> آخرین به‌روزرسانی: Sep 2026 — شامل اصلاح مسیر مدل (exe-anchored) و
> سخت‌سازی‌های ابری (timeout، quota روزانهٔ Groq).

## ۱. دستورات فوری (در پوشهٔ `voice-ptt/`)

```powershell
cargo test                  # تست‌های خودکار — باید سبز شود
cargo clippy --all-targets  # لینتر — بدون warning
cargo build --release       # ساخت نسخهٔ نهایی
.\target\release\voice-ptt.exe
```

## ۲. سناریوی تست دستی (گام به گام)

```
۱. exe را اجرا کنید → آیکون در System Tray ظاهر می‌شود
۲. فایل config ساخته می‌شود: %APPDATA%\voice-ptt\config.toml
۳. اجرای اول: دانلود مدل whisper شروع می‌شود (نیاز به اینترنت!)
۴. Notepad یا VS Code را باز کنید (پنجرهٔ فعال = مقصد تایپ)
۵. کلید Caps Lock را نگه دارید → نشانگر شناور به "recording" تغییر می‌کند
۶. فارسی صحبت کنید (مثلاً: «سلام حالت چطوره»)
۷. کلید را رها کنید → وضعیت "processing" → متن تایپ می‌شود
۸. تست VAD: Caps Lock را نگه دارید و ۲ ثانیه ساکت بمانید → خودش متوقف می‌شود
```

## ۳. مدل‌ها — یک بار اینترنت، برای همیشه آفلاین

مسیرهای جستجوی مدل (به همین ترتیب):

1. `<پوشهٔ exe>\models\` ← پایدار برای double-click / Shortcut / Startup
2. `.\models\` (relative به working directory — برای توسعه)
3. `%APPDATA%\voice-ptt\models\`

| مدل | حجم | لینک |
|-----|-----|------|
| tiny | ۷۵MB | `huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin` |
| base | ۱۴۲MB | `huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin` |
| small | ۴۶۶MB | `huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin` |

> 💡 پیشنهاد: `ggml-base.bin` (۱۴۲MB) — دانلودش با نت موبایل چند دقیقه است و
> دقت قابل قبولی دارد. مدل VAD (`silero_vad.onnx`) هم به‌صورت خودکار از
> مخازن رسمی silero-vad دانلود می‌شود (multi-mirror + connect timeout).

## ۴. چه چیزهایی بدون اینترنت قابل تست است؟

| کامپوننت | بدون نت؟ | چطور؟ |
|----------|----------|-------|
| Build + تست‌های خودکار | ✅ | `cargo test` |
| اجرای exe + Tray icon | ✅ | exe را اجرا کنید |
| Hotkey (Caps Lock) | ✅ | نگه دارید، نشانگر تغییر کند |
| Audio Capture (WASAPI) | ✅ | صحبت کنید |
| VAD (Silero) | ✅ | مدل لوکال |
| State Machine | ✅ | idle → recording → processing |
| Config | ✅ | فایل TOML ساخته شود |
| Normalizer + Dictionary | ✅ | `cargo test` |
| Text Injection | ⚠️ نسبی | با مدل tiny/base بعد از کپی دستی |
| ASR (whisper) | ❌ | نیاز به فایل مدل |
| Cloud Engine | ❌ | نیاز به اینترنت |

## ۵. چک‌لیست تست end-to-end ابری

### مرحله A — آماده‌سازی (یک بار، با اینترنت)

```
۱. کلید Groq: console.groq.com → API Keys → Create  (کلید gsk_...)
۲. %APPDATA%\voice-ptt\config.toml:
   [cloud]
   enabled = true
   api_key = "gsk_..."        # یا env var: VOICE_PTT_CLOUD_KEY
   model = "whisper-large-v3-turbo"
   language = "fa"
   daily_limit = 300          # سهمیهٔ روزانه؛ اتمام → استراحت تا نیمه‌شب
۳. (توصیه) ggml-base.bin را در <پوشهٔ exe>\models\ بگذارید ← fallback آفلاین
```

### مرحله B — تست

```
۱. exe را اجرا کنید → Tray icon ✓
۲. Notepad را باز و active کنید
۳. Caps Lock را نگه دارید → overlay = "recording"
۴. بگویید: «سلام حالت چطوره»
۵. رها کنید → "processing" → متن تایپ شود
   ⏱ انتظار: < ۲ ثانیه (cloud)
۶. جملهٔ فنی: «من میخوام با پایتون یک ای پی آی بنویسم»
   → بررسی شود «پایتون»→Python و «ای پی آی»→API توسط dictionary اصلاح شود
```

### مرحله C — تست failover (مهم‌ترین)

```
۱. اینترنت را قطع کنید
۲. همان جمله را بگویید
   ⏳ انتظار: timeout کوتاه ابری → cooldown → سوییچ به whisper لوکال
   → متن (با دقت کمتر) تایپ شود
۳. اگر مدل لوکال ندارید: باید پیام خطای واضح بدهد، نه کرش
```

### تست quota روزانهٔ Groq

```
۱. در config مقدار daily_limit = 3 بگذارید (برای تست)
۲. سه بار دیکته کنید → بار چهارم باید بی‌درنگ با whisper لوکال اجرا شود
   (هیچ درخواست ابری ارسال نمی‌شود؛ cooldown سی‌ثانیه‌ای تکرار نمی‌شود)
۳. وضعیت در لاگ: "quota exhausted (resets in N min)"
```

### معیار پذیرش

- [ ] Cloud: متن در < ۲s تایپ شود
- [ ] دقت فارسی قابل قبول (جملات ساده درست)
- [ ] اصطلاحات فنی توسط dictionary اصلاح شود
- [ ] Failover به لوکال کار کند (یا خطای تمیز)
- [ ] اتمام سهمیهٔ روزانه → سوییچ دائمی به لوکال تا نیمه‌شب
- [ ] بدون کرش در هیچ حالت

## ۶. عیب‌یابی

| مشکل | راه‌حل |
|------|--------|
| exe از Shortcut/Startup مدل را پیدا نمی‌کند | مدل را در `<پوشهٔ exe>\models\` بگذارید (مسیر ۱ در بخش ۳) |
| Groq از منطقه بلاک است | `base_url` را به `https://openrouter.ai/api/v1` تغییر دهید |
| کلید commit نشود | `.gitignore` شامل `config.toml`، `*.key`، `.env` است؛ یا از env var استفاده کنید |
| خطا هنگام تست | exe را از PowerShell اجرا کنید و خروجی console را بفرستید |
