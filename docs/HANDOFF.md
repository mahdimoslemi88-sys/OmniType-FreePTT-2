# OmniType FreePTT v2 — سند تحویل پروژه (Handoff)

> **هدف این سند**: هر مدل/توسعه‌دهنده‌ای بتواند بدون بافت قبلی، کار روی این پروژه را ادامه دهد.
> آخرین به‌روزرسانی: ۲۰۲۶-۰۹-۲۱ · شاخه: `main` · وضعیت: کارکرد کامل + تغییرات کامیت‌نشده (بخش ۹)

---

## ۱. محصول چیست؟

**OmniType FreePTT** — ابزار **dictation با push-to-talk** برای ویندوز (Rust، آفلاین‌محور):

- کلید را نگه می‌داری (پیش‌فرض **CapsLock**)، حرف می‌زنی، رها می‌کنی → متن تایپ‌شده در هر اپی تزریق می‌شود.
- کپسول شناور کوچک (status capsule) + آیکون ترِی + پنجره‌های مدیریت (دیکشنری، موتور، تاریخچه).
- موتورهای ASR به ترتیب اولویت: **Cloud (اگر API key باشد) → Google Free Speech (بدون کلید، آنلاین) → Custom Providers → Local Whisper** (همیشه fallback).
- زبان اصلی: فارسی (نرمالایزر + دیکشنری اصلاح ۱۸۲ قانونی).

## ۲. نقشهٔ ریپو

```
v-2/
├── README.md                  # معرفی، بیلد، نصب با setup، Quick start
├── voice-ptt/                 # ⭐ کل اپلیکیشن (Rust، ~۹٬۰۰۰ LOC)
│   ├── Cargo.toml             # فیچرها: silero-vad (پیش‌فرض)، light-theme
│   ├── src/
│   │   ├── lib.rs             # ⭐ run(): کل bootstrap — تک‌نمونه، ترِی-اول، دانلود پس‌زمینه
│   │   ├── paths.rs           # resolve_* — همه‌چیز exe-first (بخش ۴)
│   │   ├── asr/               # whisper.rs، google.rs، cloud.rs، router.rs، downloader.rs، quota.rs
│   │   ├── audio/             # WASAPI capture + ring buffer
│   │   ├── vad/               # Silero (ONNX) با fallback RMS
│   │   ├── processing/        # نرمالایزر فارسی + دیکشنری TOML
│   │   ├── state/             # state machine ضبط/پردازش
│   │   ├── gui/               # overlay.rs (⭐ همهٔ UI + پالت)، tray.rs، mod.rs
│   │   ├── hotkey/            # هوک کیبورد ویندوز
│   │   ├── output/            # تزریق متن (SendInput)
│   │   └── logging.rs         # لاگ دائمی فایل + redirect صدای whisper.cpp
│   └── tests/ benches/        # ۲۶ فایل تست؛ تست‌های lib: ۹۸ عدد
├── voice-ptt-dist/            # پوشهٔ توزیع (exe + DirectML + VAD + دیکشنری) — مدل‌ها ندارد
├── installer/installer.iss    # ⭐ اسکریپت Inno Setup 6 (بخش ۶)
├── third_party/egui-notify/   # وندور egui-notify 0.15 + پچ max_width (بخش ۸)
└── docs/
    ├── light-theme-tuning.md  # چک‌لیست تیون بصری تم روشن (با دادهٔ WCAG)
    ├── upstream/egui-notify/  # بستهٔ PR/issue بالادستی (بخش ۸)
    └── proposal-0.1.md، TESTING-guide.md، Comprehensive-research-…md
```

## ۳. معماری و ترتیب استارتاپ (مهم — اخیراً بازطراحی شد)

`voice_ptt::run()` در `lib.rs` **به این ترتیب**:

1. `logging::init` → لاگ فایل در `%APPDATA%\voice-ptt\logs\voice-ptt.log.YYYY-MM-DD`
2. **گارد تک-نمونه** — mutex نام‌دار `Local\OmniTypeFreePTT.SingleInstance` (ماژول `single_instance`؛ پیام MessageBox به نمونهٔ دوم). *قبل از هر چیز دیگر.*
3. لود تنظیمات (`Settings::load_or_create` از `resolve_config_path`)
4. resolve نام مدل (سیاست `auto`: GPU→large-v3-turbo، CPU≥8→small، else base)
5. دانلود **VAD** (فایل کوچک) به‌صورت سنکرون — تنها بلاک شبکهٔ استارتاپ
6. **ترِی + hotkeys قبل از دانلود بزرگ** (ترتیب «ترِی-اول»)
7. ساخت ماشین حالت + موتورها
8. **WhisperEngine سرد ساخته می‌شود** اگر فایل مدل نباشد (`health = Failed`) و تسک پس‌زمینه با `downloader::ensure_model` دانلود می‌کند و بعدش **`engine.reload(&path)`** — hot-reload بدون ری‌استارت (در این فاصله Google جواب می‌دهد)
9. ورود به حلقهٔ GUI (eframe)

**قواعد طلایی این بخش**:
- هیچ شبکه‌ای جز `downloader.rs` در اپ نیست.
- هرگز چیزی را قبل از ترِی بلاک نکن؛ UI مرئی بودن = همه‌چیز.
- `WhisperEngine` از `RwLock` داخلی استفاده می‌کند (پیاده‌سازی دستی `Clone`) — `reload` در حین transcription امن است (in-flight به Arc قدیمی چسبیده‌اند).

## ۴. مسیریابی داده‌ها (exe-first)

همه در `paths.rs` — اولویت همیشه: **کنار exe → cwd → `%APPDATA%\voice-ptt`**:

| داده | محلی که پیدا/ساخته می‌شود |
|---|---|
| `config.toml` | exe-first؛ نصب per-user یعنی **کنار اپ** |
| `dictionary.toml` | کنار اپ (نصب‌شده) — ویرایش کاربر حفظ می‌شود |
| `models/*.bin` | `{app}\models` (نصب‌شده) |
| لاگ‌ها | همیشه `%APPDATA%\voice-ptt\logs\` |
| usage/quota | `resolve_usage_path` |

**نتیجهٔ عملی**: پوشهٔ نصب per-user قابل‌نوشتن است ⇒ کانفیگ و دیکشنری کنار اپ‌اند. نصب‌کننده از همین برای پین‌کردن مدل ویزارد استفاده می‌کند (بخش ۶).

## ۵. بیلد و راستی‌آزمایی (دستورهای آماده)

```bash
cd v-2/voice-ptt
cargo check  --all-targets                                   # تم تاریک
cargo clippy --all-targets                                   # ← استاندارد: صفر هشدار
cargo test  --lib                                            # ← ۹۸/۹۸
cargo check  --all-targets --features light-theme            # تم روشن
cargo test  --lib           --features light-theme           # ۹۸/۹۸
cargo build --release                                        # ~۴ دقیقه، exe ≈ ۳۱MB
```

- ریلیز آماده: `voice-ptt/target/release/voice-ptt.exe` — **همیشه قبل از ساخت نصب‌کننده، exe تازه را به `voice-ptt-dist/` کپی کن** (`cp target/release/voice-ptt.exe ../voice-ptt-dist/`)؛ dist را خیس‌خیس به‌روز نگه نداریم.
- تست‌های شبکه‌دار (دانلود Silero و…) نیاز اینترنت دارند و در CI معمولی هم رد نمی‌شوند.

## ۶. نصب‌کننده (Inno Setup 6) — طراحی و درس‌های تلخ

**فایل**: `installer/installer.iss` · بازسازی:
```bash
"/c/Program Files (x86)/Inno Setup 6/ISCC.exe" installer/installer.iss
# خروجی: installer/Output/OmniType-FreePTT-0.1.0-setup.exe  (~۲۲MB)
```

**طراحی**: نصب **per-user بدون UAC** (الگوی VS Code User Setup)، `{app} = %LOCALAPPDATA%\Programs\OmniType FreePTT`. مدل‌ها **باندل نمی‌شوند** — ویزارد بعد از صفحهٔ مسیر، صفحهٔ رادیویی «Which Whisper model?» دارد: base (~۱۴۱MB، پیش‌فرض) / large-v3-turbo (~۱٫۵GB) / None. دانلود داخل ویزارد با `CreateDownloadPage` + **SHA-256 embed** (هش‌ها از فایل‌های dist محاسبه شده‌اند = بایت‌به‌بایت HuggingFace). «None» ⇒ اپ در اولین اجرا خودش دانلود می‌کند (مکانیزم `ensure_model`).

**جریان مدل انتخابی به اپ**: `CurStepChanged(ssInstall)` فایل `{app}\config.toml` می‌نویسد:
```toml
[asr]
model = "base"   # یا large-v3-turbo / auto
language = "fa"
```
چون اپ exe-first می‌خواند، این **بر کانفیگ قدیمی `%APPDATA%` اولویت دارد**. این نوشتن **فقط وقتی هیچ کانفیگی (نه کنار اپ، نه در APPDATA) وجود ندارد** انجام می‌شود — پینِ مدلِ کاربر در آپگرید حفظ می‌شود.

**باگ‌هایی که در راه افتادند (تکرارشان نکن)**:
1. `Unknown constant "appdata"` — ثابت `{appdata}` **در Inno وجود ندارد**؛ `{userappdata}` درست است. کامپایلر رشته‌های runtime را چک نمی‌کند ⇒ فقط در نصب واقعی/لاگ‌دار می‌ترکد (`CurStepChanged raised an exception (fatal)` در لاگ).
2. `SaveStringsToUTF8File` BOM می‌نویسد و پارس TOML در Rust را می‌شکند ⇒ برای TOML خالص‌ASCII از `SaveStringToFile` استفاده شد.
3. کامنت `;` داخل `[Code]` نامعتبر است — Pascal فقط `//`.
4. `CreateInputOptionPage` **۶ پارامتر** دارد (دو Boolean آخر: Exclusive, SubPage).
5. فلگ `unchecked` در `[Components]` نیست — کنترل پیش‌فرض از `[Types]` می‌آید. `{userstartup}` نه `{userautostart}`.

**تست silent** (دقت: Git Bash سوییچ‌های `/LOG=...` را خراب می‌کند — دو env زیر لازم است):
```bash
MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' \
  ./Output/OmniType-FreePTT-0.1.0-setup.exe /VERYSILENT /SUPPRESSMSGBOXES \
  /LOG="C:\\...\\install.log"    # EXIT=0 مورد انتظار؛ لاگ را tail بزن
```

## ۷. سیستم تم (پالت دوتمی + helperها)

همه در `gui/overlay.rs`:

- **`mod palette`** — تنها منبع رنگ؛ دو مد با `#[cfg(feature = "light-theme")]` (تاریک پیش‌فرض). نام‌ها معنایی‌اند (`WINDOW_BG`, `CARD_BG_ALT`, `TEXT_PRIMARY`, …). **قاعده: هیچ `Color32::from_rgb` literal خارج از این ماژول مجاز نیست** (چند استثنای تست/آینهٔ کم‌اهمیت).
- **`apply_theme_visuals`** — ویجت‌های استوک egui را با پالت رنگ می‌کند.
- **helperهای ساخت‌دستی** (هیچ call-site‌ای Frame دست‌ساز نسازد):
  - `manager_central_panel` — قاب پنجره‌های مدیریت
  - `manager_card(fill, stroke)` — کارت محتوایی (گوشهٔ ۸، خط ۱، حاشیهٔ ۱۰)
  - `status_chip(ui, text, bg, color, size, ChipFamily)` — چیپ‌های وضعیت (دو خانوادهٔ Small/Tiny) + شکل‌دهی فارسی یکدست
  - `header_badge`، `capsule_frame(fill, stroke, rounding, margin)` — قاب ۴ حالت کپسول
- توست‌ها: egui-notify وندورشده + `TOAST_MAX_WIDTH: f32 = 320.0` + ارتفاع تطبیقی؛ هاست توست شفاف (`TOAST_HOST_HEIGHT`).

**ورودی آیندهٔ تم**: پالت طوری ساخته شده که اضافه‌کردن مد سوم (مثلاً high-contrast) فقط ماژول palette را دست می‌زند.

## ۸. زنجیرهٔ بالادستی egui-notify (شبه‌سازنده)

- **وندور**: `third_party/egui-notify/` = نسخهٔ 0.15 + پچ **سقف عرض توست** (`Toasts::with_max_width(f32)` کانال + `Toast::set_max_width(Option<f32>)` per-toast؛ شکستن سخت توکن‌های بی‌فاصله با `LayoutJob` + `break_anywhere`) — متصل با `[patch.crates-io]` در `voice-ptt/Cargo.toml`. در `Cargo.lock` ورودی بدون `source = registry` است = پچ فعال.
- **بالادست**: issue **[ItsEthra/egui-notify#54](https://github.com/ItsEthra/egui-notify/issues/54)** باز شده (با `gh`، حساب `mahdimoslemi88-sys`). پچ آمادهٔ PR روی `main` بالادست (0.23/egui 0.36) در **`docs/upstream/egui-notify/`**: `toast-max-width.patch` (تست‌شده با `git apply --check`)، `examples/toast_width_cap.rs`، `PR.md` (بدنهٔ انگلیسی + دستورهای push به فورک)، `ISSUE.md`.
- **قدم باقی‌مانده (نیاز کاربر)**: فورک + push شاخهٔ `feature/toast-max-width` (کلون در `/tmp/egui-notify` — موقتی! اگر gone بود: پچ را دوباره روی main اعمال کن) و `gh pr create` با `Fixes #54`.
- **نسخه‌های egui**: پروژه روی **egui 0.28 / eframe 0.28** است. ارتقا به 0.29+ بررسی شده بود: بیشترین شکستگی در 0.36 (طراحی جدید `App` با `logic`+`ui`)؛ ارزش فوری ندارد مگر بالادست merge کند.

## ۹. وضعیت گیت — تغییرات کامیت‌نشده (ا کنون)

```
 M README.md                          # بخش نصب + عیب‌یابی محل داده‌ها (با طراحی فعلی هم‌سازگار)
 M installer/installer.iss            # fix {userappdata} + نوشتن config + پاک‌سازی .part
 M voice-ptt/Cargo.toml               # feature ویندوزی Win32_Security (برای CreateMutexW)
 M voice-ptt/src/lib.rs               # تک‌نمونه + ترِی-اول + دانلود پس‌زمینه + hot-reload
 M voice-ptt/src/asr/whisper.rs       # RwLock داخلی + reload() + تست hot-reload
 M docs/upstream/egui-notify/PR.md    # پیوند #54
?? docs/upstream/egui-notify/ISSUE.md # سند issue #54
```

**پیشنهاد تفکیک کامیت**: (۱) `installer.iss` + README → «Fix installer fatal error and pin wizard model via config»; (۲) `lib.rs` + `whisper.rs` + `Cargo.toml` → «Add single-instance guard, tray-first startup, and background model hot-reload»; (۳) دو سند upstream → «Link egui-notify issue #54 into upstream package». سبک پیام ریپو: تک‌خطی توصیفی + بدنهٔ کوتاه فاکس‌محور.

تاریخچهٔ کامیت‌های این دوره (به‌ترتیب): `0775368` بستهٔ PR بالادستی → `1c6c177` وندور egui-notify + کپ عرض → `0bd21e5` پالت دوتمی + helperها → `098c9e1` نصب‌کنندهٔ ویزاردی. هیچ‌کدام push نشده‌اند.

## ۱۰. کارهای ناتمام / ایده‌های بعدی (اولویت‌دار)

1. **شاخص دانلود در UI** — دانلود پس‌زمینه الان فقط در لاگ است؛ نشانگر در کپسول/ترِی («مدل در حال دانلود…») طبیعی‌ترین قدم بعدی است. (وضعیت موتور از `machine.subscribe()` و `AsrHealth` در دسترس است.)
2. **آیتم‌های ترِی خاموش‌اند** — کلیک Dictionary/Engine/History/Settings چیزی باز نمی‌کند؛ پیاده‌سازی بازکردن پنجره‌ها از `flag`های موجود (`dict_flag` و…).
3. **`Cannot create transparent window: the GL config does not support it`** — خطای مکرر eframe؛ کپسول بدون شفافیت واقعی کار می‌کند ولی سزاوار رسیدگی است (renderer/glow + `with_transparent`).
4. **تیون چشمی تم روشن** — `docs/light-theme-tuning.md` چک‌لیست کامل دارد؛ ۴ جفت زیر آستانهٔ WCAG ۳× شناسایی شده (`WARNING`، `SUCCESS` روی بنر، `TEXT_FAINT`، `ACCENT_SOFT`).
5. کامیت + push؛ سپس PR بالادستی (بخش ۸).
6. autostart نصب‌کننده از `{userstartup}` (_startup folder_) می‌سازد — اگر خواستی رفتار «اجرای خودکار» اپ با تنظیم داخلی‌اش هم‌گام شود، هم‌راستا کن.

## ۱۱. عجایب محیط این دستگاه (زمان‌سوز نباش)

- **Git Bash/MSYS**: `//F //IM` (اسلش دوبل برای فلگ‌ها)؛ برای سوییچ‌های `/SILENT`-مانند حتماً `MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'`. مسیر پروژه فاصله دارد (`C:\Users\LENOVO LOQ\...`) — همیشه کوت.
- **`/tmp` ناپایدار است** (کلون بالادستی آنجا بود) — هر چیز ماندگاری لازم دارد به `v-2/third_party/` یا `v-2/docs/` برو.
- **ابزار `write_file` روی فایل‌های بزرگ این ریپو یک‌بار خروجی ناسالم داده** (الگوی تکرارشونده) ⇒ برای ویرایش‌های بزرگ، فایل سالم را از `git show <commit>:<path>` بازیابی کن و با **هانک‌های هدفمند `str_replace`** اعمال کن.
- اپ کاربر **الان در حال اجراست** (از نصب سرتاسری ما) و کانفیگش مدل large را پین کرده؛ قبل از هر اجرای آزمایشی: `taskkill //F //IM voice-ptt.exe` و فراموش نکن گارد تک-نمونه نمونهٔ دوم را با MessageBox بیرون می‌اندازد.
- `gh` CLI نصب و احراز شده (حساب `mahdimoslemi88-sys`) — باز کردن issue/PR مستقیم ممکن است (فقط push به فورک نیاز به تصمیم کاربر دارد).

## ۱۲. مرجع سریع — «برای تغییر X کجاست؟»

| می‌خواهم… | برو به |
|---|---|
| رنگ/تم چیزی را عوض کنم | `gui/overlay.rs` → `mod palette` (literal ممنوع بیرون از آن) |
| فریم/کارت/چیپ جدید | helperهای موجود (بخش ۷) — Frame دست‌ساز نکن |
| مدل جدید به دانلودر | `asr/downloader.rs` → `WHISPER_MODELS` (+ URL نصب‌کننده و هش‌ها هم‌زمان) |
| سیاست انتخاب مدل | `config/settings.rs` → `resolve_model_name` |
| کلید میان‌بر/رفتار ضبط | `config/settings.rs` (Hotkey/Vad) + `state/machine.rs` |
| مسیر داده جدید | `paths.rs` (exe-first را رعایت کن) |
| رفتار نصب/آپگرید | `installer/installer.iss` → بخش‌های `[Files]`, `[Code]` |
| متن‌های ویزارد | `[Messages]` + `InitializeWizard` در همان فایل |
