# برنامهٔ اصلاح باگ‌های UI — Orb / باکس متن / RAM / طول ضبط

**تاریخ بررسی:** ۱۴۰۵-۰۷-۰۷ (2026-09-29)
**شاخه:** `main` · آخرین کامیت مرتبط: `193a523` (decouple transcript toast from orb)
**وضعیت:** بررسی کامل انجام شد — **فازهای ۰، ۱، ۲، ۳ و ۳٫۱ پیاده و بیلد شدند** (بیلد تازه در `voice-ptt-dist/` نصب شد)

| فاز | وضعیت | راستی‌آزمایی کد | راستی‌آزمایی بصری |
|---|---|---|---|
| ۰ — قفل دست‌آزاد (دوبار ضربه) | ✅ | ۵ تست واحد | تست دستی کلید |
| ۱ — مالکیت قطعی پنجره (کادرهای شبح) | ✅ | clippy + تست ها | ⏳ نیازمند بستن اپ |
| ۲ — باکس متن + گیت پروب + گارد مدل لوکال | ✅ | ۱۵۸ تست + clippy | ⏳ نیازمند بستن اپ |
| ۳ — ضبط طولانی و چانک‌بندی | ✅ | ۵ تست سیاست + تست شبیه‌سازی ۶۰ ثانیه + clippy | ⏳ نیازمند تست صوتی واقعی |
| ۳٫۱ — کیفیت متن درزها (ادغام چانک‌ها) | ✅ | ۱۸ تست واحد `processing::seam` + clippy | ⏳ نیازمند تست صوتی واقعی |
| ۳٫۲ — قاب پنجره، ثبات ظاهر در چانک، سکوت | ✅ | clippy + ۱۸۳ تست + شاهد عددی style | ✅ با probe روی بیلد در حال اجرا |

### تست دستی سریع (وقتی اپ بسته شد)

```bash
# ۱) بیلد تازه را در پوشهٔ توزیع بگذار (ابد پلیر باید بسته باشد)
cp voice-ptt/target/release/voice-ptt.exe voice-ptt-dist/
# ۲) اپ را اجرا کن، سپس چند دیکتهٔ واقعی بگیر و هندسهٔ پنجره‌ها را ببین:
powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File docs/reaserch/gui/probes/window-probe.ps1 -Samples 6 -IntervalSec 5
```

جوابِ قبولی: فقط پنجرهٔ `Window Class` روی هندسهٔ orb باشد؛ `Winit Thread Event Target` و
`tray_icon_app` هندسهٔ اصلی خودشان را نگه دارند؛ `winit` خودش `Winit Thread Event Target` را با
`0x0` می‌سازد ولی در وضعیت خراب اندازهٔ orb را گرفته بود.

در همان زمان، در یک ترمینال دیگر لاگ را دنبال کن تا فاز ۳ هم دیده شود:

```bash
tail -f "$APPDATA/voice-ptt/logs/voice-ptt.log.$(date +%F)"   # در Git Bash: "$APPDATA"/voice-ptt/logs/...
```

انتظار در یک دیکتهٔ بلند (بیشتر از ۳۰ ثانیه):

```
INFO ...: flushing mid-session chunk audio_secs=12.4 still_recording=true
INFO ...: chunk text ready raw=... typed=...
INFO ...: chunk text injected chars=57
INFO ...: asr success engine="google" elapsed_ms=1900 chars=57
```

و در پایان دیکته، به‌جای یک ردیف `audio_secs=30.01` (رفتار قبل)، متن به‌صورت چند بستهٔ مرتب تایپ شده است.
اگر مدل لوکال استفاده نشود، خط `loading whisper model on first use` نباید هیچ‌جا ظاهر شود.

> **قاعدهٔ کاری تأیید‌شدهٔ کاربر:** هر تغییر پرریسک **حذف نمی‌شود** — غیرفعال/کامنت می‌شود تا کد اصلی در
> درخت بماند و بتوان بعداً A/B تست گرفت. نمونه: `apply_window_shapes_all_legacy` (تابع کامل، بدون
> فراخوانی)، `SetWindowTextW` و `FindWindowW` (کامنت‌شده با توضیح دلیل).
**بیلد نصب‌شده در توزیع:** `voice-ptt-dist/voice-ptt.exe`، `md5 = ff5ab23da5e07c7363e2c979893770c6`
(= همان `voice-ptt/target/release/voice-ptt.exe`)، شامل فازهای ۰ تا ۳٫۳.
اپ با همین بیلد در حال اجراست (PID ۳۶۷۶۴، private ۲۹۸MB ⇒ مدل لوکال لود نشده) و probe تأیید می‌کند:
`class=Window Class style=0x96000000 caption=False popup=True` — یعنی frameless.
> `%APPDATA%\voice-ptt\config.toml` هم به `silence_ms = 1200` و کلیدهای `seam_*` به‌روز شد
> (نسخهٔ قبلی کنارش با پسوند `.bak-YYYYMMDD-HHMM` نگه داشته شد).
> نکته: کلیدهای جدید `[streaming] seam_*` در `%APPDATA%\voice-ptt\config.toml` نیامده‌اند ولی بیدردسر
> مقدار پیش‌فرض (روشن) می‌گیرند؛ اپ خودش در اولین ذخیره‌سازی آن‌ها را می‌نویسد.

---

## ۰. خلاصهٔ مدیریتی

| # | شکایت کاربر | علت ریشه‌ای پیدا‌شده | ریسک |
|---|---|---|---|
| ۱ | کادرهای شبح بالای سرِ Orb که با هر بار استفاده بیشتر و روی هم جمع می‌شوند | «حل HWND با حدسِ عنوان پنجره» + `place()` که هر پنجره‌ای را به هندسهٔ Orb می‌کشد | بالا (نشتی پنجره/DWM) |
| ۲ | باکس متن روی Orb سوار است + مصرف بالای RAM | پنجرهٔ توست هم توسط همان مکانیزم به Orb کشیده می‌شود؛ `egui_notify` هرگز رندر نمی‌شود ⇒ حلقهٔ رندر ۳۰fps دائمی؛ پروب‌های دوره‌ای Antigravity | بالا |
| ۳ | قطع خودکار ضبط در ۳۰ ثانیه | `ring_seconds = 30` ⇒ `buffer.len() >= capture.capacity()` شیر اطمینان در ماشین حالت | قطعی (ساختاری) |

هر سه مورد از یک منبع قابل مشاهده در کد و لاگ اثبات شده‌اند (شواهد در بخش ۱).

---

## ۱. یافته‌ها با شواهد

### ۱-۱. باگ کادرهای شبح (شکایت ۱)

**مکانیزم:**

1. پنجرهٔ اصلی با عنوان خالی ساخته می‌شود: `lib.rs` → `.with_title("")`.
2. `enable_true_transparency()` ([overlay.rs:952](voice-ptt/src/gui/overlay.rs#L952)) علاوه بر استایل‌زدایی، عنوان پنجره را هم با `SetWindowTextW(hwnd, "")` **پاک می‌کند**.
3. `apply_window_shapes_all()` ([overlay.rs:1130](voice-ptt/src/gui/overlay.rs#L1130)) با `EnumThreadWindows` **همهٔ** پنجره‌های ترد را می‌گردد و هر پنجره‌ای را که «عنوانش خالی است یا دقیقاً `OmniType` است» پنجرهٔ اپ فرض می‌کند:
   - `MAIN_HWND` را روی آن ست می‌کند (آخرین تطبیق برنده است، نه پنجرهٔ درست)،
   - `enable_true_transparency` را رویش اجرا می‌کند.
4. `Orb::show()` هر فریم `MAIN_HWND` را به `OrbWindow::place()` می‌دهد و `place()` با `SetWindowPos(hwnd, x, y, side, side)` آن پنجره را **به هندسهٔ Orb منتقل و ری‌سایز می‌کند** ([orb.rs:576](voice-ptt/src/gui/orb.rs#L576)).

پنجره‌های دیگری که «عنوان خالی» دارند و قربانی می‌شوند:

- `Winit Thread Event Target` — پنجرهٔ داخلی winit (با `WS_VISIBLE | WS_POPUP` ساخته می‌شود و هیچ‌وقت نقاشی نمی‌شود)،
- `tray_icon_app` — پنجرهٔ پیام ترِی (`tray-icon`)،
- و **پنجرهٔ پیش‌نمایش متن** که خود همین تابع عنوانش را پاک می‌کند و از آن به بعد در شرط `title.is_empty()` می‌افتد.

**شواهد عینی (خروجی probe روی پروسهٔ در حال اجرا):**

```
hwnd=0x001B05C4 class=Winit Thread Event Target  visible=True  rect=(1329,436 162x162)
hwnd=0x00480F14 class=Window Class               visible=True  rect=(1329,436 162x162)   ← پنجرهٔ واقعی Orb
hwnd=0x009111A6 class=tray_icon_app             visible=False rect=(1329,436 162x162)
```

هر سه پنجره **دقیقاً همان rect** پنجرهٔ Orb را دارند. یعنی دو پنجرهٔ غیرِاپ (event-target و ترِی) توسط کد ما به موقعیت/اندازهٔ Orb منتقل شده‌اند. یک پنجرهٔ topmost که هرگز نقاشی نمی‌شود = مستطیل شبح که هرگز پاک نمی‌شود.

**چرا تعدادشان زیاد می‌شود؟** `apply_window_shapes_all()` در این مسیرها صدا زده می‌شود:

- تا ۱۰ فریم اول استارتاپ ([overlay.rs:3897](voice-ptt/src/gui/overlay.rs#L3897))،
- **بعد از هر متن تایپ‌شده** ([overlay.rs:4110](voice-ptt/src/gui/overlay.rs#L4110))،
- در هر درخواست باز کردن داشبورد/تنظیمات/تری‌منو،
- و **هر فریم تا وقتی یک توست روی صفحه است** — چون داخل کلوژرِ viewport توست صدا زده می‌شود ([overlay.rs:3281](voice-ptt/src/gui/overlay.rs#L3281)).

هر بار ممکن است HWND به پنجرهٔ دیگری حل شود ⇒ یک پنجرهٔ دیگر «پارک» می‌شود ⇒ کادرها روی هم جمع می‌شوند. این با توصیف کاربر («بعد از هر بار ضبط/جای‌گذاری بیشتر می‌شود») دقیقاً می‌خواند.

**شاهد جانبی:** در لاگ ۲۰۲۶-۰۹-۲۸ تعداد **۵٬۶۹۹** هشدار `wgpu_hal::vulkan::conv: Unrecognized present mode` و در ۰۹-۲۹ تعداد **۱٬۱۷۳** ثبت شده است (در بیلدهای قبل از مهاجرت به wgpu: صفر). این یعنی سطح (surface) مرتباً بازپیکربندی می‌شود؛ بخشی از آن به‌خاطر `ViewportCommand::InnerSize` است که **هر فریم** (حتی بدون تغییر) فرستاده می‌شود ([orb.rs:139](voice-ptt/src/gui/orb.rs#L139)).

### ۱-۲. باکس متن و مصرف RAM (شکایت ۲)

**الف) باکس متن روی Orb سوار است:** پوزیشن‌دهی درست است (`taskbar_bottom_center_pt` = پایین‌وسط بالای تسک‌بار)، ولی همان مکانیزم بند ۱-۱ پنجرهٔ توست را (بعد از پاک‌شدن عنوانش) به هندسهٔ Orb می‌کشد ⇒ کارت متن روی Orb ظاهر می‌شود.

**ب) `egui_notify` کاملاً مرده است ولی مدام پر می‌شود:**

- `self.toasts.add(toast)` در دو نقطه صدا زده می‌شود ([overlay.rs:4010](voice-ptt/src/gui/overlay.rs#L4010) و [4090](voice-ptt/src/gui/overlay.rs#L4090)).
- در تمام پروژه **هیچ‌جا `toasts.show(...)` وجود ندارد** (جست‌وجو شد) ⇒ این توست‌ها هرگز رندر و هرگز منقضی نمی‌شوند.
- `needs_animation_frames()` ([overlay.rs:3668](voice-ptt/src/gui/overlay.rs#L3668)) شرط `!self.toasts.toasts_mut().is_empty()` را دارد ⇒ **بعد از اولین دیکته، حلقهٔ رندر ۳۰fps تا ابد روشن می‌ماند.**

**اندازه‌گیری روی همین دستگاه (idle، بدون دیکته):**

```
03:42:29  ws=263.3MB  priv=350.3MB  handles=912  threads=58  cpu= 99.0s
03:43:05  ws=263.7MB  priv=350.3MB  handles=912  threads=58  cpu=104.9s   ← ~0.15–0.3s CPU در هر ۶ ثانیه
03:43:35  ws=290.5MB  priv=402.6MB  handles=1002 threads=67  cpu=110.9s   ← جهش دوره‌ای
```

- مصرف CPU در حالت بیکار ≈ ۲٫۵–۵٪ یک هسته = اثر همان حلقهٔ دائمی.
- جهش‌های دوره‌ای (private ۳۵۰→۴۰۲MB، handles ۹۱۲→۱۰۰۲، threads ۵۸→۶۷) با پروبِ پس‌زمینهٔ موتور **Antigravity** هم‌زمان است: ترد نگه‌دارنده هر ۳۰–۶۰ ثانیه `maintain()` را صدا می‌زند که **PowerShell + netstat** را اجرا می‌کند ([lib.rs:331](voice-ptt/src/lib.rs#L331)، [antigravity.rs:558](voice-ptt/src/asr/antigravity.rs#L558)) — حتی وقتی کاربر آن موتور را انتخاب نکرده و اصلاً Antigravity نصب/اجرا نیست.

**پ) مدل لوکال: الان لود نمی‌شود ولی دو تله باز باقی است**

- تأیید شد: private = 348MB ≪ ۱٫۶GB ⇒ مدل whisper در این سشن لود نشده. (فایل `ggml-large-v3-turbo.bin` با حجم ۱٫۶GB در `voice-ptt-dist/models/` هست و کنارش یک نسخهٔ تکراری `ggml-large-v3-turbo.bin1` هم افتاده.)
- تلهٔ اول: `WhisperEngine::health()` ([whisper.rs:307](voice-ptt/src/asr/whisper.rs#L307)) برای موتور سردْ **Ready** برمی‌گرداند فقط به این خاطر که *فایل* روی دیسک هست. پس در حالت `auto` اگر Google شکست بخورد (قطعی شبکه، خطای HTTP، یا صدای بلندتر از سقف سرویس)، نوبت به لوکال می‌رسد و همان **لود ۱٫۶ گیگابایتی + inference سنگین CPU** اتفاق می‌افتد — همان حادثه‌ای که قبلاً اپ را مجبور به ری‌استارت می‌کرد. لاگ نشان می‌دهد `whisper.cpp` یک‌بار واقعاً اجرا شده است.
- تلهٔ دوم: `resolve_model_name` روی هر دستگاه با GPU، `large-v3-turbo` را انتخاب می‌کند و اپ در پس‌زمینه ۱٫۶GB دانلود می‌کند ([lib.rs:411](voice-ptt/src/lib.rs#L411)).

### ۱-۳. قطع خودکار ضبط (شکایت ۳)

**علت ساختاری، نه محدودیت گوگل:**

- `config.toml` → `audio.ring_seconds = 30` ⇒ `RingBuffer::new(16_000 × 30) = 480_000` نمونه ([capture.rs:70](voice-ptt/src/audio/capture.rs#L70)).
- در ماشین حالت: `buffer.len() >= self.services.capture.capacity()` ⇒ `finalize()` ([machine.rs:236](voice-ptt/src/state/machine.rs#L236)).
- `vad.cutoff_on_hold = false` ⇒ VAD هرگز وسط نگه‌داشتنِ کلید ضبط را قطع نمی‌کند؛ پس تنها قطعِ خودکار همان شیر اطمینان ۳۰ ثانیه‌ای است.

**شاهد از لاگ‌ها:** بیشینهٔ `audio_secs` در همهٔ لاگ‌ها **۳۰٫۰۱ و ۳۰٫۰۲ ثانیه** و به‌تکرار است — یعنی دقیقاً روی سقف بافر، نه روی سکوت.

**آیا چانک‌به‌چانک ممکن است؟ بله.** مسیر پیشنهادی در فاز ۳ آمده؛ نکتهٔ مهم این است که سرویس رایگان گوگل (Chromium v2) برای utterance کوتاه طراحی شده و همان چانک‌بندی، هم سقف زمانی را برمی‌دارد و هم پایداری گوگل را بالا می‌برد.

---

## ۲. برنامهٔ اجرایی (۴ فاز)

### فاز ۰ — قفل دست‌آزاد با دوبار زدن کلید (خواستهٔ جدید کاربر) ✅ انجام شد

**رفتار جدید** (روی همان کلید شورتکات، پیش‌فرض CapsLock):

| ورودی | نتیجه |
|---|---|
| نگه‌داشتن کلید | مثل قبل: ضبط تا رها کردن (hold-to-talk) |
| یک ضربهٔ کوتاه | بعد از `tap_max_ms` (پیش‌فرض ۳۵۰ms) تا `double_tap_window_ms` (پیش‌فرض ۶۰۰ms) منتظر ضربهٔ دوم می‌ماند، بعد مثل قبل finalize می‌کند (و چون گفتاری ندارد، دور ریخته می‌شود) |
| دو ضربهٔ پشت‌سرهم | ضبط **قفل** می‌شود و بعد از رها کردن کلید هم ادامه پیدا می‌کند |
| فشار بعدی (یا Esc/Cancel) | قفل ضبط را تمام می‌کند و متن تایپ می‌شود |

**پیاده‌سازی:** `LatchPolicy` خالص و یونیت‌تست‌شده در [machine.rs](voice-ptt/src/state/machine.rs) (۵ تست:
نگه‌داشتن، ضربهٔ تنها، دوبار ضربه، ضربهٔ دوم دیرهنگام، حالت خاموش) + فیلد `latched` در `AppStatus` تا UI
بتواند وضعیت را نشان دهد. تنظیمات جدید در `[hotkey]`: `double_tap_latch = true`، `tap_max_ms = 350`،
`double_tap_window_ms = 600` (کانفیگ‌های موجود چون `#[serde(default)]` دارند بی‌دردسر مقدار پیش‌فرض می‌گیرند).

### فاز ۱ — مالکیت قطعی پنجره (ریشهٔ باگ ۱ و نصف باگ ۲) ✅ انجام شد

| تغییر | فایل |
|---|---|
| `register_main_hwnd()`: گرفتن HWND واقعی از `eframe::Frame` (ترِیت `HasWindowHandle`) + `invalidate_main_hwnd()` | [overlay.rs](voice-ptt/src/gui/overlay.rs)، [Cargo.toml](voice-ptt/Cargo.toml) (`raw-window-handle = "0.6"`) |
| `apply_window_shapes_all()` ⇒ به `apply_window_shapes_all_legacy` تغییر نام یافت، بدون فراخوانی، `#[allow(dead_code)]` | [overlay.rs](voice-ptt/src/gui/overlay.rs) |
| `shape_preview_window(generation)`: فقط پنجرهٔ پیش‌نمایش، یک‌بار به‌ازای هر بابل | [overlay.rs](voice-ptt/src/gui/overlay.rs) |
| کامنت‌شدن `SetWindowTextW("")` و fallback عنوان‌محورِ `FindWindowW` | [overlay.rs](voice-ptt/src/gui/overlay.rs)، [orb.rs](voice-ptt/src/gui/orb.rs) |
| حذف سه نقطهٔ فراخوانی shape در مسیرهای داغ (توست، بنر آپدیت، بعد از هر متن، باز شدن داشبورد) | [overlay.rs](voice-ptt/src/gui/overlay.rs) |
| `InnerSize` فقط در صورت تغییر اندازه + `invalidate_main_hwnd` روی شکست `SetWindowPos` | [orb.rs](voice-ptt/src/gui/orb.rs) |

**راستی‌آزمایی کد:** `cargo check/clippy --all-targets` بدون هشدار، `cargo test --lib` = **۱۵۷/۱۵۷**
(۵ تست جدید قفل)، `--features light-theme` هم سبز (۲۳ هشدار «const بلااستفاده» از قبل موجود است
و به این فاز مربوط نیست — پاک‌سازی‌اش در فاز ۲).

**راستی‌آزمایی بصری (باقی‌مانده — نیازمند بستن اپ در حال اجرا):** باید exe تازه در dist قرار بگیرد و با
[window-probe.ps1](docs/reaserch/gui/probes/window-probe.ps1) ثابت شود که `Winit Thread Event Target`
و `tray_icon_app` دیگر روی هندسهٔ orb نمی‌نشینند.

**exe آماده:** `voice-ptt/target/release/voice-ptt.exe` (بیلد فاز ۰–۲). کپی به `voice-ptt-dist/`
فقط وقتی ممکن است که اپ در حال اجرا بسته شده باشد (فایل exe توسط پروسهٔ در حال اجرا قفل است).

| کار | فایل |
|---|---|
| گرفتن HWND واقعی از خودِ eframe به‌جای حدس با عنوان: `frame.window_handle()` + `RawWindowHandle::Win32` (ترِیت `HasWindowHandle` روی `eframe::Frame` پیاده است) و ذخیرهٔ یک‌بارهٔ آن | `gui/overlay.rs`، `Cargo.toml` (`raw-window-handle = "0.6"`) |
| حذف کامل حلقهٔ `EnumThreadWindows` + `SetWindowTextW` + منطق «عنوان خالی = پنجرهٔ من» | `gui/overlay.rs` |
| جایگزینی `SetWindowPos` در `OrbWindow::place()` با `ViewportCommand::OuterPosition` (egui 0.28 دارد) — فقط وقتی موقعیت واقعاً تغییر کرده باشد | `gui/orb.rs` |
| فرستادن `InnerSize` فقط در صورت تغییر مقدار | `gui/orb.rs` |
| شکل‌دهی DWM فقط برای HWND اصلی و **یک‌بار** (حذف صدا‌زدن‌ها از داخل کلوژر توست و از مسیر هر متن) | `gui/overlay.rs` |

**معیار پذیرش:** با probe ثابت شود در طول ۵ دیکته، تنها یک پنجرهٔ `Window Class` در هندسهٔ Orb وجود دارد و `Winit Thread Event Target` / `tray_icon_app` در هندسهٔ اولیهٔ خودشان می‌مانند؛ هیچ کادر شبحی ظاهر نمی‌شود.

### فاز ۲ — باکس متن + بودجهٔ حافظه (نصف دیگر باگ ۲) ✅ انجام شد

| کار | وضعیت | فایل |
|---|---|---|
| قطع خوراک مسیر مردهٔ `egui_notify` (دو `self.toasts.add(...)` کامنت شدند و کد ساخت توست برای rollback باقی ماند) — این همان چیزی بود که حلقهٔ ۳۰fps را تا ابد روشن نگه می‌داشت | ✅ | `gui/overlay.rs` |
| کارت متن: anchor پایین‌وسط بالای تسک‌بار و **کاملاً مستقل از Orb**، با سوئیچ `gui.show_transcript_bubble` (پیش‌فرض روشن؛ خاموش = هیچ پنجره‌ای ساخته نمی‌شود) | ✅ | `gui/overlay.rs`، `config/settings.rs` |
| گیت پروب پس‌زمینهٔ Antigravity: فقط وقتی این موتور **انتخاب شده** باشد (پس دیگر در حالت `auto`/گوگل، هر ۳۰–۶۰ ثانیه PowerShell+netstat اجرا نمی‌شود) | ✅ | `lib.rs` |
| گارد حافظهٔ مدل لوکال: ترِیت `AsrEngine::implicit_fallback()` (پیش‌فرض true، برای whisper false) + `AsrRouter::set_allow_local_fallback` + تنظیم `asr.auto_local_fallback = false` ⇒ `auto` هرگز ناخواسته مدل ۱٫۶GB را لود نمی‌کند؛ انتخاب صریح «Local Whisper» همچنان کار می‌کند | ✅ | `asr/engine.rs`، `asr/whisper.rs`، `asr/router.rs`، `lib.rs`، `config/settings.rs` |
| ~~لاگ working-set در `logging.rs`~~ → به ابزار probe منتقل شد (خود `window-probe.ps1` حافظه/هندل/ترد را گزارش می‌دهد) | ↩︎ | `docs/reaserch/gui/probes/window-probe.ps1` |
| ~~`antigravity.probe_interval_secs`~~ → گیتِ انتخاب‌شدن کافی بود؛ نابِ اضافی اضافه نشد | ↩︎ | — |

**راستی‌آزمایی کد:** `cargo clippy --all-targets` بدون هشدار · `cargo test --lib` = **۱۵۸/۱۵۸**
(تست جدید `auto_skips_opt_in_engines_unless_explicitly_allowed`: اثبات می‌کند در `auto` موتور لوکال
صدا زده نمی‌شود، ولی انتخاب صریح و `auto_local_fallback = true` کار می‌کنند).

**معیار پذیرش (اصلاح‌شده):** در حالت بیکار مصرف CPU باید فقط تاوان انیمیشن نفس‌کشیدنِ خود Orb باشد
(`repaint_interval` حالت Idle = 80ms ⇒ ~۱۲٫۵fps) و نه حلقهٔ ۳۰fps؛ private memory باید در ۱۰ دقیقهٔ
بیکار ثابت بماند و با هر دیکته پله‌ای بالا نرود؛ با موتور ابری، مدل لوکال در هیچ مسیری لود نشود
(قابل کنترل با probe: `private` باید حول ~۳۵۰MB بماند، نه ۱٫۹GB).

### فاز ۳ — ضبط طولانی + پردازش چانک‌به‌چانک (باگ ۳) ✅ انجام شد

| کار | پیاده‌سازی | فایل |
|---|---|---|
| برداشتن سقف ۳۰ ثانیه | شیر اطمینان `buffer.len() >= capacity()` دیگر با streaming روشن، session را تمام نمی‌کند (فقط حالت خاموشِ streaming و سقف مطلق `max_utterance_seconds` باقی مانده) | `state/machine.rs` |
| جدا کردن ظرفیت بافر از طول ضبط | `ring_seconds` پیش‌فرض ۳۰ ⇒ **۶۰** (تا انتظار یک ترنسکرایب چانک، صدای زنده را overwrite نکند) | `config/settings.rs`، `audio/capture.rs` |
| سیاست برش چانک (خالص و تست‌شده) | `should_flush_chunk(cfg, rate, chunk, trailing_silence, speech)` ⇒ زیر `min_chunk_seconds` هرگز، در `chunk_seconds` همیشه (حتی وسط جمله)، در استراتژی `silence` روی سکوت ≥ `silence_ms` با ≥۲۵۰ms گفتار | `state/machine.rs` |
| برداشت چانک + درز امن | `take_chunk` برش را برمی‌دارد و `overlap_ms` انتهایی را برای چانک بعدی نگه می‌دارد (cursor هم جابه‌جا می‌شود تا هیچ نمونه‌ای دوبار به VAD داده نشود) | `state/machine.rs` |
| درج بدون پایان‌دادن به session | `process_chunk`: ترنسکرایب + نرمالایز + inject و سپس **بازگشت به Recording** (نه Idle)؛ شکست یک چانک فقط warn می‌شود و ضبط ادامه می‌یابد | `state/machine.rs` |
| ترتیب | ترنسکرایب چانک در همان تسک و به‌صورت سری انجام می‌شود ⇒ ترتیب تایپ قطعاً همان ترتیب گفتار است (بدون صف موازی) | `state/machine.rs` |
| UI | بابل متن در هر چانک به‌روز می‌شود و دیگر در مرز چانک‌ها پاک نمی‌شود (پاک‌سازی فقط در شروع session جدید)؛ تاریخچه یک ردیف به‌ازای هر **session** می‌ماند نه هر چانک | `gui/overlay.rs` |
| Config | بخش جدید `[streaming]`: `enabled=true`، `chunk_seconds=20`، `min_chunk_seconds=4`، `overlap_ms=300`، `silence_ms=600`، `strategy="silence"`، `max_utterance_seconds=600` | `config/settings.rs`، `README.md` |

**نکته دربارهٔ قرارداد موتورها:** به‌جای افزودن `preferred_chunk_secs()` به ترِیت موتورها (که در برنامهٔ اولیه بود)، سیاست
برش در ماشین حالت و از روی تنظیمات اعمال می‌شود — یک منبع تصمیم، قابل تنظیم توسط کاربر، و مستقل از موتور. سقف ۲۰s
تصادفی انتخاب نشده: خیلی پایین‌تر از محدودیت خودِ endpoint رایگان گوگل است و در عوض تأخیر تایپ هر بسته ~۲s است.

**راستی‌آزمایی کد:** `cargo clippy --all-targets` بدون هشدار · `cargo test --lib` = **۱۶۳/۱۶۳** (۵ تست جدید:
سقف سخت، حداقل طول، برش روی سکوت، استراتژی fixed، حالت خاموش) — با فاز ۳٫۱ به **۱۸۳/۱۸۳** رسید.

**معیار پذیرش (نیازمند تست صوتی):** دیکتهٔ پیوستهٔ ۳ دقیقه‌ای بدون قطع ۳۰ ثانیه‌ای ⇒ در لاگ باید چند ردیف
`flushing mid-session chunk` با `still_recording=true` دیده شود و متن به ترتیب تایپ شود؛ اولین بستهٔ هر جمله
≤ ~۳ ثانیه بعد از بسته‌شدنش درج شود. شکست یک چانک نباید session را قطع کند.
اکنون این معیار یک ابزار دارد: `dictation-report.ps1` (بخش ۳٫۲) همان لاگ را می‌خواند و `[PASS]/[FAIL]` می‌دهد.

**محدودیت شناخته‌شده:** ترنسکرایب هر چانک به‌صورت سری در همان تسک انجام می‌شود، پس اگر در همان لحظه کلید را رها کنید،
پایان ضبط به اندازهٔ latency همان یک چانک (برای گوگل ~۲s) عقب می‌افتد؛ داده‌ای گم نمی‌شود (در ring buffer می‌ماند).

### فاز ۳٫۱ — کیفیت درز چانک‌ها: «یک دیکتهٔ پیوسته، نه چند تکهٔ تکراری» ✅ انجام شد

چانک‌بندی دو آرتیفکت مخصوص **مرز چانک** می‌سازد که با هیچ تنظیمی قابل حذف نیست، چون هر دو نتیجهٔ خودِ طراحی
(همپوشانی صوتی برای نصف‌نشدن کلمه + برش وقتی طول چانک پر شود) هستند:

1. **کلمه‌های تکراری:** همان `overlap_ms` صوتی که برای امنیت درز نگه داشته می‌شود، باعث می‌شود موتور تشخیص آن
   کلمه‌ها را دوباره ترنسکرایب کند (در لاگ‌های واقعی: `شد`/`است`/`می‌خواهم` دوبار تایپ می‌شد).
2. **کلمهٔ بریده:** اگر برش وسط یک کلمه بیفتد، چانک N با یک قطعه تمام می‌شود (`… می‌خوا`) و چانک N+1 همان
   کلمه را کامل می‌گوید (`می‌خواهم …`) ⇒ قطعه در متن می‌ماند.

| کار | پیاده‌سازی | فایل |
|---|---|---|
| ماژول خالص و تست‌شدهٔ درز | `SeamStitcher::stitch(chunk) -> SeamMerge { text, dropped_words, backspaces }`؛ فقط رشته/کلمه‌بازی، بدون IO ⇒ ۱۸ تست واحد | `processing/seam.rs` |
| حذف کلمه‌های تکراری | بزرگ‌ترین `k` که «k کلمهٔ آخر حافظهٔ session» == «k کلمهٔ اول چانک جدید» باشد (لنگرشده در درز، پیوسته) حذف می‌شود؛ `max_overlap_words = 6` | `processing/seam.rs` |
| تطبیق مقاوم ولی بی‌‌گمان | برابری کلید تبدیل‌شده (ی/ك عربی، ZWNJ، حرکت‌ها، نقطه‌گذاری) و یک «لغزش تشخیص» با فاصلهٔ ویرایشی ۱؛ **هیچ‌وقت** تطبیق پیشوندی برای حذف (آن حالت مالِ backspace است) | `processing/seam.rs` |
| حذف قطعهٔ بریده | فقط اگر: طول قطعه ≥ ۴ حرف، کلمهٔ کامل حداقل ۲ حرف بلندتر باشد، قطعه پیشوندِ کلمهٔ جدید باشد، و قطعه در فهرست سیاه کلمه‌های پرکاربرد نباشد (کار/کارخانه، روز/روزنامه، دست/دستگاه…) | `processing/seam.rs` |
| درج backspace | `inject_backspaces(n)` با `VK_BACK` و سقف ۲۴ حرف — همیشه به اندازهٔ کلمه‌ای که خودمان لحظهٔ قبل تایپ کرده‌ایم | `output/injector.rs` |
| اتصال به ماشین حالت | `stitch_seam()` قبل از درج، در **هم** `process_chunk` و **هم** `finalize`؛ حافظهٔ درز با هر `begin_recording` صفر می‌شود (دو دیکته در یک سند ممکن است با همان کلمه شروع شوند) | `state/machine.rs` |
| چانکِ صرفاً همپوشانی | اگر کل چانک همان تکرار درز قبلی باشد، هیچ چیزی تایپ نمی‌شود (به‌جای تایپ یک نسخهٔ تکراری) | `state/machine.rs` |
| Config | `[streaming] seam_merge = true`، `seam_backspace = true`، `seam_max_words = 6`، `seam_fuzzy = true` (کانفیگ‌های قدیمی بیدردسر پیش‌فرض می‌گیرند) | `config/settings.rs`، `README.md` |

**فلسفهٔ ایمنی (طبق قاعدهٔ کاربر):** تطبیق «به‌جای حدس، ترجیح می‌دهد کاری نکند» — اگر حالتی مبهم باشد، تکرار در متن می‌ماند
(خطای قابل‌بخشش) و هیچ کلمهٔ واقعی حذف نمی‌شود. `seam_backspace = false` (یا `seam_merge = false`) برای برگشت فوری
به رفتار خام در دسترس است؛ هیچ کدی حذف نشد.

**راستی‌آزمایی کد:** `cargo clippy --all-targets` بدون هشدار · `cargo test` = **۱۸۳ lib + ۵ + ۳ + ۴ + ۴** سبز.
تست‌های کلیدی: `repeated_overlap_words_are_dropped`، `a_truncated_word_is_replaced_with_backspaces` (شامل
شبیه‌سازی کل session روی یک رشته)، `a_common_word_is_never_deleted_as_a_fragment`، `overlap_dedupe_is_capped`،
`a_pure_overlap_chunk_injects_nothing`، `reset_starts_a_clean_session`، `tail_memory_stays_bounded_in_a_long_session`.

**آزمون سرتاسری بدون سخت‌افزار:** `a_sixty_second_session_is_chunked_and_stitched_in_order` در `state/machine.rs`:
۶۰ ثانیه گفتار پیوسته (بدون سکوت) را فریم‌به‌فریم به همان سیاست برش می‌دهد، قاعدهٔ `overlap_ms` را اعمال می‌کند و
متن‌ها را با `SeamStitcher` می‌دوزد ⇒ سه برش در ۲۰s/۴۰s/۶۰s، ضبط پس از ۳۰ ثانیه هنوز زنده، و متن نهایی
یکدست و به ترتیب («قسمت ۱ … قسمت ۲ … قسمت ۳ …»). یعنی منطقِ «قطع‌نشدن در ۳۰ ثانیه» الان *خودکار* اثبات می‌شود،
نه فقط با چشم.

### فاز ۳٫۲ — «قاب پنجره برگشت»، «قطع/وصل نمایش در هر چانک»، «سکوت کوتاه» ✅ انجام شد

**شکایت کاربر:** دور orb یک قاب واقعی ویندوز با دکمه‌های minimize/maximize/close دیده می‌شود، و هر بار
minimize/restore یک قاب دیگر اضافه می‌شود؛ باکس متن زشت و خالی و همه‌چیز در یک گوشه است؛ در هر چانک شکل نمایش
عوض می‌شود؛ و با کوتاه‌ترین سکوت ضبط قطع می‌شود.

**ریشه‌یابی با شاهد عددی (probe روی پروسهٔ در حال اجرا):**

```
قبل: class=Window Class style=0x16CB0000 ex=0x00040118 caption=True  popup=False
بعد: class=Window Class style=0x96000000 ex=0x00040018 caption=False popup=True
```

یعنی `WS_CAPTION|WS_THICKFRAME|WS_SYSMENU|WS_MINIMIZEBOX|WS_MAXIMIZEBOX` **بعد از** شکل‌دهی ما برمی‌گشتند.
علت قطعی هم در لاگ بیلد جدید دیده شد — یک خط پس از ثبت handle:

```
INFO  main window handle registered from eframe's raw window handle hwnd=25496724
WARN  window frame drifted back; re-stripped caption/border repairs=1 hwnd=25496724
```

یعنی eframe/winit (به‌ویژه در مسیر minimize/restore) attributeهای پنجره را دوباره اعمال می‌کند و قاب را برمی‌گرداند.
فاز ۱ فقط **یک‌بار** شکل‌دهی می‌کرد ⇒ قاب تا ریستارت بعدی می‌ماند. باکس متن هم همان پنجرهٔ preview بود که همین
قاب را می‌گرفت (زمینهٔ سیستمی + کارت در گوشهٔ بالا: «زشت و خالی»).

| کار | پیاده‌سازی | فایل |
|---|---|---|
| گارد خودترمیم قاب | `enforce_frameless_window(hwnd)`: دو `GetWindowLongW` در هر فریم؛ فقط اگر قاب برگشته باشد style/DWM/repaint دوباره اعمال می‌شود (لاگ هشدار تا ۱۰ بار) | `gui/overlay.rs` |
| تشخیص پنجرهٔ بازساخته‌شده | `register_main_hwnd` اکنون هر فریم HWND واقعی را می‌خواند و **مقایسه** می‌کند؛ اگر عوض شده باشد پنجرهٔ جدید را شکل‌دهی می‌کند (قبلاً فقط وقتی کش خالی بود ⇒ پنجرهٔ بازساخته‌شده هرگز قاب‌برداری نمی‌شد) | `gui/overlay.rs` |
| از بین بردن «ردپای» پنجره | `RedrawWindow(RDW_INVALIDATE|ERASE|ALLCHILDREN|UPDATENOW|FRAME)` در پایان هر شکل‌دهی + `force_repaint` بعد از هر جابجایی orb ⇒ مستطیل‌های توی‌هم‌رفتهٔ باقی‌مانده (که شبیه قاب‌های تودرتو دیده می‌شدند) پاک می‌شوند | `gui/overlay.rs`، `gui/orb.rs` |
| باکس متن | شکل‌دهی preview دیگر «یک‌بار به‌ازای هر bubble» نیست؛ هر فریم drift چک می‌شود (بدون هزینه) | `gui/overlay.rs` |
| ثبات ظاهر در چانک‌ها | چانک mid-session دیگر `set_state(Processing)` نمی‌کند؛ فیلد `AppStatus.chunk_busy` اضافه شد و state روی `Recording` می‌ماند ⇒ orb در كل session یک شکل دارد | `state/machine.rs` |
| پایان‌دادن فقط با کاربر | سکوت با `vad.cutoff_on_hold = false` هیچ‌وقت session را تمام نمی‌کند (فقط release کلید، کلیک روی orb، یا سقف مطلق `max_utterance_seconds`) | `state/machine.rs`، config |
| سکوت چانک | `streaming.silence_ms`: ۶۰۰ ⇒ **۱۲۰۰** (تست‌های سیاست به‌روز شدند) | `config/settings.rs` |

**راستی‌آزمایی زندهٔ همین الان:** بیلد فاز ۳٫۲ نصب و اجرا شد (PID 21576، private ۲۸۵MB ⇒ مدل لوکال لود نشد) و
probe عددی بالا را تأیید کرد: `caption=False popup=True` روی پنجرهٔ orb.

### فاز ۳٫۳ — «کادر آبی بالای orb و کادر سفید دور باکس متن» (رجعت خودفاز ۳٫۲) ✅ انجام شد

**علت: خودِ من.** در فاز ۳٫۲ برای پاک‌کردن «ردّ پیکسلی» یک
`RedrawWindow(RDW_INVALIDATE|RDW_ERASE|RDW_FRAME|…)` اضافه کرده بودم. این پنجره با
`DwmExtendFrameIntoClientArea(-1)` ساخته می‌شود، یعنی *کل* پنجره از نظر DWM «ناحیهٔ فریم» است؛
`RDW_FRAME` باعث می‌شود DWM همان ناحیه را با لایهٔ روشن گلس رنگ کند. نتیجه: نوار آبی روشن بالای orb و
مستطیل سفید دور کارت متن (در دو نقطه: `RDW_ERASE` در پایان هر شکل‌دهی و `force_repaint` بعد از هر جابجایی).

**شاهد عددی از اسکرین‌شات کاربر** (تحلیل پیکسلی فایل PNG او): نوار `x=70..272, y=33..60` (۲۰۳×۲۸) با
میانگین رنگ `220,235,250` و مستطیل سفید `15036` پیکسل `255,255,255` دور کارت.

| کار | پیاده‌سازی |
|---|---|
| حذف `RDW_ERASE`/`RDW_FRAME` از repaint ها؛ فقط `RDW_INVALIDATE\|RDW_ALLCHILDREN\|RDW_UPDATENOW` | `gui/overlay.rs` |
| حذف کامل `RedrawWindow` از پایان `enable_true_transparency` (مسیر rollback کامنت شد؛ `SWP_FRAMECHANGED` موجود کافی است چون egui هر فریم دوباره نقاشی می‌کند) | `gui/overlay.rs` |

**راستی‌آزمایی پیکسلی (خودکار، بدون چشم):** با اسکریپت `lightbar`، داخل نوار بالا و نواحی شفاف پنجرهٔ orb با
«پس‌زمینهٔ پشت پنجره» مقایسه شد:

```
بیرون پنجره (بالا):            255,255,255
داخل پنجره، نوار بالا (y+20):  255,255,255   <- شفاف: پس‌زمینهٔ دسکتاپ دیده می‌شود
بیرون پنجره (چپ/پایین):        255,255,255 / 252,252,252
```

یعنی دیگر هیچ لایهٔ روشنی روی ناحیهٔ خالی پنجره نیست (قبل: نوار `220,235,250`).

### فاز ۴ — راستی‌آزمایی و بستن پرونده

- اجرای probe قبل/بعد از هر فاز و ضمیمهٔ خروجی به همین سند.
- `cargo check --all-targets` + `--features light-theme`، `cargo clippy --all-targets`، `cargo test --lib`، `cargo test`.
- تست واحد جدید: «شیر اطمینان نباید ضبط را قطع کند» + تست چانک‌بندی (ترتیب، همپوشانی، flush پایانی) با موتور mock.
- ساخت exe تازه و کپی به `voice-ptt-dist/` (طبق قاعدهٔ HANDOFF: dist خیس‌خیس به‌روز نشود).
- به‌روزرسانی `docs/HANDOFF.md` و حذف نسخهٔ تکراری `ggml-large-v3-turbo.bin1` (۱٫۶GB دیسک).

---

## ۳. ابزار راستی‌آزمایی

`docs/reaserch/gui/probes/window-probe.ps1` (اضافه شد) — لیست پنجره‌های top-level پروسهٔ `voice-ptt.exe` + حافظه/هندل/ترد:

```bash
powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File docs/reaserch/gui/probes/window-probe.ps1 -Samples 6 -IntervalSec 5
```

این اسکریپت باید «کادرهای شبح» را به یک عدد تبدیل کند: تعداد پنجره‌ها به‌ازای کلاس و هندسهٔ هر پنجره. قبل از اصلاح ⇒ چند پنجره روی هندسهٔ Orb؛ بعد از اصلاح ⇒ فقط یکی.

**شاهد عددی (همین الان، بیلد فاز ۰–۳٫۱، PID 1708):**

```
Window Class                 visible=True  rect=(1370,88 162x162)   <- orb واقعی
Winit Thread Event Target    visible=True  rect=(0,0 14x14)        <- دیگر روی orb نیست (قبل: 162x162 روی orb)
tray_icon_app                visible=False rect=(42,42 922x470)     <- هندسهٔ خودش
```

`Winit Thread Event Target` را خودِ winit با `WS_VISIBLE|WS_POPUP` و `WS_EX_LAYERED|WS_EX_TRANSPARENT|WS_EX_NOACTIVATE`
در `0x0` می‌سازد (کد winit: «it isn't displayed to the user because of the LAYERED style») — دیده‌شدن آن طبیعی است؛
مشکل قبلی این بود که کد ما آن را به هندسهٔ orb *منتقل و ریسایز* می‌کرد.

### ۳٫۲ گزارش دیکته (invocation با یک دستور)

`docs/reaserch/gui/probes/dictation-report.ps1` (اضافه شد) — لاگ روز را می‌خواند و سه شکایت اصلی را «قابل قبولی/رد» می‌کند:

```bash
powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File docs/reaserch/gui/probes/dictation-report.ps1              # لاگ امروز
powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File docs/reaserch/gui/probes/dictation-report.ps1 -Date 2026-09-28   # لاگ روز دیگر
```

خروجی شامل: طول هر session، تعداد چانک‌های mid-session، بیشینهٔ `audio_secs`، تعداد ترمیم درزها
(`dropped_words` / `backspaces`)، اینکه مدل لوکال لود شده یا نه، و چهار check خودکار (کد خروج ۱ اگر چیزی رد شود).

**بیس‌لاین «قبل» (لاگ ۲۰۲۶-۰۹-۲۸، بیلد قدیم):**

```
max audio_secs    : 30.01   (12 بار «whole-utterance transcribe of 30.0s»)
chunks flushed    : 0
seam repairs      : 0
local model loads : 1  (loading whisper model on first use)
result: 2 passed, 2 failed        <- دقیقاً همان دو شکایت «قطع ۳۰s» و «مدل لوکال»
```

**انتظار «بعد»:** `chunks flushed > 0`، `max audio_secs > 30`، `seam repairs > 0` (اگر جمله‌ها در مرز چانک ادامه داشته باشند)،
`local model loads = 0` ⇒ `result: 4 passed, 0 failed`.

**شاهد زندهٔ چانک‌بندی (لاگ ۲۰۲۶-۰۹-۲۹، ساعت ۰۱:۲۲–۰۱:۲۳ UTC، بیلد فاز ۳):** یک session با سه چانک پشت سر هم،
همه با میکروفن زنده — یعنی دیگر قطع ۳۰ ثانیه‌ای روی رخداد نیست:

```
01:22:32  recording started
01:22:37  flushing mid-session chunk audio_secs=4.42   still_recording=true   -> chunk text injected chars=3
01:22:46  flushing mid-session chunk audio_secs=9.34   still_recording=true   -> chunk text injected chars=70
01:22:51  flushing mid-session chunk audio_secs=4.54   still_recording=true   -> chunk text injected chars=35
```

**وضعیت checkها روی لاگ امروز با بیلد جدید:** ۳ از ۴ PASS (`[FAIL]` باقیمانده دقیقاً همان چیزی است که
فقط با یک **دیکتهٔ صوتی واقعی پیوسته‌تر از ۳۰ ثانیه** پر می‌شود — کاری که ربات نمی‌تواند انجام دهد):

```
[PASS] at least one recording session
[PASS] every flushed chunk is mid-session (still_recording=true)
[FAIL] longest audio seen is under/at 30 s - dictate longer to prove it   <- نیازمند صدای کاربر
[PASS] local whisper model was not loaded
```

---

## ۴. ریسک‌ها و نکات

1. حذف کد DWM/عنوان‌دهی می‌تواند شفافیت واقعی را در برخی درایورها برگرداند به حالت قبل ⇒ در فاز ۱ شفافیت با wgpu + `with_transparent(true)` ارزیابی چشمی شود و اگر لازم بود `DwmExtendFrameIntoClientArea` فقط برای HWND درست نگه داشته شود.
2. `ViewportCommand::OuterPosition` در درگ ممکن است یک فریم عقب بیفتد ⇒ حس درگ باید با ماوس تست شود.
3. تغییر پیش‌فرض `ring_seconds` روی مصرف حافظه اثر دارد (۱۲۰ ثانیه = ۷٫۷MB بافر، ناچیز).
4. چانک‌بندی روی دقت اثر دارد؛ `overlap_ms` و `strategy = silence` باید با fixture فارسی اندازه‌گیری شود نه با حدس.
5. بیلد فعلی در حال اجراست (PID 22164) ⇒ قبل از هر تست دستی `taskkill //F //IM voice-ptt.exe` (گارد تک‌نمونه در غیر این صورت پیام می‌دهد).
