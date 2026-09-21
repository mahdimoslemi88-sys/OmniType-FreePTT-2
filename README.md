# OmniType-FreePTT-2 — v2

نسخهٔ دوم **OmniType-FreePTT**: تایپ صوتی Push-to-Talk برای ویندوز، فارسی‌محور،
کاملاً آفلاین و رایگان — بازنویسی‌شده از Python (v1) به **Rust**.

The v2 rewrite of [OmniType-FreePTT](https://github.com/mahdimoslemi88-sys/OmniType-FreePTT)
(Python) as a native Windows Push-to-Talk voice-typing tool built in Rust.

## Goals

| Goal | Target |
|---|---|
| Latency | < 1 s for a 5 s utterance |
| Persian accuracy | WER < 10 % |
| Cost | free — no mandatory API key |
| Footprint | RAM < 50 MB, small binary (whisper.cpp + egui statically linked) |
| Network | 100 % offline core; optional OpenAI-compatible cloud engine with auto-fallback |

## Repository layout

```
v-2/
├── voice-ptt/    # The Rust application (cpal → ring buffer → VAD → whisper → injection)
├── docs/         # Research notes, proposal, testing guide
└── .gitignore    # Keeps weights, build artifacts, and secrets out of git
```

The v1 Python implementation is intentionally **not** part of this repository.

## Install with the setup wizard

For end users there is a step-by-step Windows installer (no admin rights
needed — it installs for the current user only, like VS Code's user setup):

```powershell
cd installer
& "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer.iss   # output → installer\Output\OmniType-FreePTT-<ver>-setup.exe
```

### چه چیزی نصب می‌شود

ستاپ **سبک** است (~۲۲MB — فقط اپ) و مدل whisper داخلش باندل **نمی‌شود**. در
میان ویزارد، صفحهٔ «Which Whisper model?» با سه گزینه:

| گزینه | چیزی که می‌شود |
|---|---|
| **Whisper base (~141 MB)** — پیش‌فرض | دانلود داخل ویزارد با نوار پیشرفت + صحت‌سنجی SHA-256 |
| **Whisper large-v3-turbo (~1.5 GB)** | همان، برای بالاترین دقت |
| **None now** | هیچ دانلودی در نصب؛ اپ در اولین اجرا خودش مدل را دانلود می‌کند |

همیشه نصب می‌شود: اپ + `DirectML.dll` + مدل Silero VAD + دیکشنری پیش‌فرض.
برای نصب آفلاین (بدون اینترنت)، پوشهٔ `voice-ptt-dist` را مستقیم کپی کنید.

نکات مهم:

- مدل‌ها دقیقاً از همان URLهای رسمی و با همان SHA-256 فایل‌های HuggingFace
  دانلود می‌شوند که دانلودر خود اپ استفاده می‌کند — نتیجه بایت‌به‌بایت یکی است.
- در دانلود مدل بزرگ، موقتاً تا ~۲× حجم مدل فضای دیسک لازم است (موقت + مقصد).
- دانلود ناموفق؟ نصب ادامه می‌یابد — گزینهٔ «None» را انتخاب کنید و اپ را
  در اولین اجرا بگذارید دانلود کند (همان مکانیزم داخلی اپ).
- **آپگرید**: اجرای setup روی نصب موجود، اپ را در‌جا به‌روز می‌کند و
  `dictionary.toml` شما دست‌نخورده می‌ماند.
- **حذف نصب** مدل‌های دانلودشده را هم پاک می‌کند (تا ~1.7 GB واقعاً آزاد شود).
- **دستهٔ راه‌اندازی با ویندوز** (اختیاری): کپسول push-to-talk بعد از لاگین
  در دسترس باشد.
- **محل داده‌ها در نصب معمولی** (per-user، پوشهٔ قابل‌نوشتن):
  تنظیمات و دیکشنری کنار اپ (`config.toml` در اولین اجرا ساخته می‌شود)،
  مدل‌ها در `models\` همان پوشه، و لاگ‌ها در `%APPDATA%\voice-ptt`.
  (اگر پوشهٔ نصب قابل‌نوشتن نباشد، تنظیمات/دیکشنری هم به `%APPDATA%\voice-ptt`
  منتقل می‌شوند — اولویت exe-first در کد.)

## Build & test

```powershell
cd voice-ptt
cargo build --release      # produces target/release/voice-ptt.exe
cargo test                 # 97 tests (add --features light-theme to test that build too)
cargo clippy --all-targets # zero warnings expected
```

## Quick start

See **[`voice-ptt/README.md`](voice-ptt/README.md)** for the full pipeline
description, configuration reference (settings/dictionary resolution is
exe-first — see the install section above), model download notes, and the
optional cloud (Groq) setup with daily-quota tracking.

For hands-on manual testing (tray, hotkey, VAD auto-stop, cloud failover), see
**[`docs/TESTING-guide.md`](docs/TESTING-guide.md)**.

## Docs

- [`docs/proposal-0.1.md`](docs/proposal-0.1.md) — product proposal
- [`docs/Comprehensive-research-program-developing-roadmap.md`](docs/Comprehensive-research-program-developing-roadmap.md) — roadmap
- [`docs/reaserch/WASAPI Audio Pipeline.md`](docs/reaserch/WASAPI%20Audio%20Pipeline.md) — capture research
- [`docs/reaserch/whisper.cpp Performance Benchmark.md`](docs/reaserch/whisper.cpp%20Performance%20Benchmark.md) — engine benchmarks
- [`docs/reaserch/Web Speech API.md`](docs/reaserch/Web%20Speech%20API.md) — browser-engine research
