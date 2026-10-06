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
needed — it installs for the current user only, like VS Code's user setup).
One script builds it, and it is the same one every release is published from:

```bash
bash voice-ptt/installer/build-installer.sh
# → voice-ptt/installer/Output/OmniType-FreePTT-<ver>-setup.exe
```

The version is read from `Cargo.toml` and checked to look like a version
before Inno Setup ever runs, so the file name, the tag and the version inside
the binary cannot drift apart.

### چه چیزی نصب می‌شود

ستاپ **سبک** است (~۱۰٫۵ مگابایت) و **فقط `voice-ptt.exe` را** می‌گذارد. بقیه در
**اولین اجرا** ساخته یا دانلود می‌شوند:

| چیز | از کجا می‌آید |
|---|---|
| `voice-ptt.exe` | همان `target\release\voice-ptt.exe` که `cargo build --release` ساخته |
| مدل whisper (۱۴۱MB تا ~۱٫۵GB) | دانلود در **پس‌زمینهٔ اولین اجرا**، بعد بارگذاری داغ |
| مدل Silero VAD (~۲MB) | دانلود در اولین اجرا، پیش از بالا آمدن ترای |
| `dictionary.toml` | در اولین اجرا، با قواعد پیش‌فرض نوشته می‌شود |
| `config.toml` | در اولین اجرا |

نکات مهم:

- **اولین اجرا به اینترنت نیاز دارد** — بدون مدل محلی و بدون موتور ابری، دیکته‌ای
  ساخته نمی‌شود. دانلود همراه نوار پیشرفت است و شکستش فقط موتور محلی را خاموش
  می‌گذارد؛ خودِ برنامه بالا می‌ماند.
- مدل‌ها از همان URLهای رسمی و با همان SHA-256 دانلود می‌شوند که دانلودر خودِ اپ
  استفاده می‌کند — و در دانلودِ مدل بزرگ موقتاً تا ~۲× حجمِ آن فضای دیسک لازم است.
- **آپگرید**: اجرای setup روی نصب موجود، اپ را در‌جا به‌روز می‌کند و
  `dictionary.toml` شما دست‌نخورده می‌ماند.
- **حذف نصب** (از **۰.۶.۱** به بعد) فایل‌های شما را نگه می‌دارد و فقط `models\` را
  پاک می‌کند تا ~۱٫۷GB واقعاً آزاد شود. در **۰.۴.۰ تا ۰.۶.۰** باگی بود که
  `dictionary.toml` را هم پاک می‌کرد؛ پیش از حذفِ نصب حتماً ارتقا بدهید
  ([یادداشت ۰.۶.۱](release-notes-0.6.1.md)).
- **محل داده‌ها** (per-user، پوشهٔ قابل‌نوشتن): اولویت **exe-first** است، یعنی
  `config.toml`، `dictionary.toml` و `models\` کنار اپ نشسته‌اند و فقط وقتی آن
  پوشه قابل‌نوشتن نباشد به `%APPDATA%\voice-ptt` می‌روند. لاگ‌ها همیشه در
  `%APPDATA%\voice-ptt\logs`.
- **دستهٔ راه‌اندازی با ویندوز** (اختیاری): کپسول push-to-talk بعد از لاگین
  در دسترس باشد.
- **نصبِ آفلاین**: این ستاپ مدلی باندل نمی‌کند. برای ماشینِ بی‌اینترنت، پوشهٔ
  `voice-ptt-dist` را مستقیم کپی کنید و یک مدل whisper را دستی در `models\` بگذارید —
  اولین جایی که اپ دنبالش می‌گردد همان‌جا است.

## Build & test

```powershell
cd voice-ptt
cargo build --release      # produces target/release/voice-ptt.exe
cargo test                 # 788 tests (add --features light-theme to test that build too)
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
