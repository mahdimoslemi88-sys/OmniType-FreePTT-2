# طرح ریفکتور مرحلهٔ دوم — بر پایهٔ اندازه‌گیری

**تاریخ:** ۲۰۲۶-۰۹-۳۰ · **وضعیت:** گام ۱ تا ۵ **انجام شد** · **کامیت‌نشده**
**پایه:** ۱۹٬۲۸۸ خط در ۵۲ فایل · ۲۲۴ تست سبز · clippy صفر warning

> هر عدد این سند با اسکریپت روی کد واقعی به‌دست آمده، نه با چشم. جایی که ادعای قبلی غلط بود
> در بخش ۵ صریح اصلاح شده.

---

## ۱. یافتهٔ اصلی: مشکل بدتر از `overlay.rs` جای دیگری است

`overlay.rs` از ۴۲۵۰ به ۱۱۶۲ خط رسید. ولی بررسی کل درخت نشان داد **بزرگ‌ترین خطر ساختاری جای دیگری است**:

| فایل | مشکل | شواهد |
|---|---|---|
| **`lib.rs`** | `run()` = **۴۹۲ خط** = ۸۲٪ فایل | **صفر تست** در کل فایل |
| `state/machine.rs` | یک `impl` **۵۷۹ خطی** با ۱۹ متد | ۸ متد اول فقط `status_tx`/`status_rx`/`seam` را لمس می‌کنند |
| `asr/antigravity.rs` | ۳۴ آیتم top-level در یک فایل | سه نگرانی بی‌ارتباط در یک فایل |
| `gui/orb.rs` | `impl Orb` ۴۹۷ خطی | کم‌اولویت؛ `impl` یک شیء واحد است |

`run()` بدتر است چون **هیچ پوشش تستی ندارد** و هر تغییر در آن یعنی تغییر در کل مسیر بوت.

---

## ۲. `lib.rs` — طرح اصلی

### آنچه از قبل در کد اعلام شده

`run()` خودش با ۱۵ نشانگر `// ---- ... ----` بخش‌بندی شده. اندازهٔ هر فاز:

| فاز | خطوط | فاز | خطوط |
|---|---|---|---|
| GUI (main thread) | **۱۴۵** | state machine | ۲۱ |
| ASR | **۱۱۳** | run the state machine | ۲۳ |
| tray + hotkeys | ۳۸ | whisper download | ۲۹ |
| models | ۳۰ | VAD | ۱۴ |
| audio | ۲۰ | text processing | ۱۲ |
| bridge | ۱۶ | settings | ۹ |
| logging | ۷ | single instance | ۱۰ |

**۷۱ متغیر top-level** در `run()` تعریف می‌شود. پس «هر فاز را یک تابع کن» غلط است — امضای توابع
فاجعه می‌شود. مرز درست جای دیگری است.

### مرز درست: اندازه‌گیری کوپلینگ

اندازه‌گیری «کدام نام بیرونی را هر فاز می‌خواند» نشان داد دو بلوک، نگرانی مستقل و کم‌ورودی دارند:

| بلوک | خطوط | ورودی واقعی | خروجی |
|---|---|---|---|
| **ASR** | ۱۱۳ | `settings`, `models_dir`, `model_path`, `rt` | ۱۵ نام: `router`, `engine_ready`, `pause`, `antigravity_probe`, … |
| **GUI** | ۱۴۵ | ۴۰ نام — اما گروه‌بندی‌شده | ۲۳ نام، همه محلی |
| whisper download | ۲۹ | `model_name`, `models_dir`, `rt` | ۴ نام |

۴۰ ورودی GUI عملاً ۴ بسته‌اند، نه ۴۰ چیز پراکنده:

1. **تنظیمات** — `settings`, `settings_rwlock`, `config_path`
2. **پرچم‌های داشبورد** — `overlay_flag`, `dict_flag`, `engine_flag`, `history_flag`, `settings_flag`, `quit_flag` (هر شش `Arc<AtomicBool>`)
3. **دستگیره‌ها** — `hotkey_control`, `update_state`, `events_tx`
4. **سرویس‌ها** — `machine`, `router`, `dictionary`

و **نکتهٔ کلیدی: `AppServices` از قبل وجود دارد** (۶ فیلد در [machine.rs](../voice-ptt/src/state/machine.rs) خط ۲۵۷) و `run()` آن را در خط ۳۸۰ درون‌خطی می‌سازد. الگوی درست موجود است؛ فقط اطرافش پخش است.

### سه استخراج

| # | استخراج | خطوط | چه چیزی |
|---|---|---|---|
| **L1** | `DashboardFlags` — تایپ شش‌تایی `Arc<AtomicBool>` | +~۲۵ | شش پرچم که امروز جدا پاس داده می‌شوند |
| **L2** | `fn build_engine_chain(...) -> EngineChain` | ۱۱۳ | بلوک ASR. `EngineChain { router, engine_ready, pause, antigravity_probe, … }` |
| **L3** | `fn run_gui(...)` | ۱۴۵ | بلوک GUI. ورودی: `Startup` (یک ساختار) به‌جای ۴۰ آرگومان |

**نتیجهٔ پیش‌بینی‌شده:** `run()` از ۴۹۲ به حدود **۱۵۰ خط** می‌رسد و ۴ تابع با هر کدام زیر ۱۵۰ خط.

**ریسک:** متوسط. `run()` در مسیر بوت است، ولی بدنهٔ هر سه استخراج **جابه‌جایی مکانی** است —
نه منطق تازه. قاعدهٔ اطمینان: `L1` و `L3` داده‌ها را جابه‌جا می‌کنند، `L2` سازندهٔ چیزهاست.
همهٔ جابه‌جایی‌ها در Rust **exhaustive** هستند ⇒ حداکثر خطای کامپایل.

---

## ۳. `state/machine.rs` — مرز اندازه‌گیری‌شده

`impl StateMachine` = ۵۷۹ خط، ۱۹ متد. گروه‌بندی بر اساس فیلدهایی که لمس می‌کنند:

| گروه | متدها | خطوط | فیلدهای لمس‌شده |
|---|---|---|---|
| **کانال وضعیت** | `stitch_seam`, `reset_seam`, `subscribe`, `set_state`, `publish_partial`, `set_latched`, `set_chunk_busy`, `set_last_text` | **۷۰** | فقط `status_tx`, `status_rx`, `seam` |
| **حلقهٔ ضبط** | `run`, `begin_recording`, `take_and_stop` | ۱۷۰ | `services` + چند setter |
| **قطعه‌ها** | `poll_vad`, `chunk_overlap_samples`, `chunk_flush_due`, `take_chunk`, `process_chunk`, `resume_after_chunk` | ۱۵۶ | فقط `services` |
| **پایان** | `finalize` | ۱۰۱ | `services`, `seam`, `status_rx` |

گروه اول یک نگرانی کاملاً جداست: **پروتکل `watch`**. ⇒ `state/status.rs` با یک newtype به نام
`StatusChannel`. مزیت: پروتکل وضعیت بدون نیاز به ماشین کامل قابل تست می‌شود.

**نتیجه:** `impl` از ۵۷۹ به حدود ۵۰۹ خط.

**ریسک:** پایین — همه‌اش جابه‌جایی است.

---

## ۴. `asr/antigravity.rs` — سه نگرانی، سه فایل

نقشهٔ دقیق ۳۴ آیتم top-level:

| بخش | خطوط | محتوا | خالص؟ |
|---|---|---|---|
| **discovery** | **۳۲۷** (L51-377) | ۱۷ آیتم: `candidate_endpoints`, `parse_netstat_ports`, `discover_endpoint`, `devtools_port`, `fallback_cascade_id`, … | خیر (subprocess دارد) |
| **codec** | **۱۲۳** (L382-504) | `encode_frame`, `FrameDecoder`, `to_pcm16k_mono`, `request_headers`, `transcription_text` | **بله — کاملاً خالص** |
| **engine** | **۴۱۶** (L508-923) | `AntigravityEngine` + `impl AsrEngine` + `collect_final` | خیر |

بخش `codec` **۱۷ تست** دارد و هیچ I/O ندارد ⇒ بهترین کاندید برای اولین برش.

**نتیجه:** `antigravity.rs` ۱۱۴۲ → حدود ۶۹۰، به‌علاوهٔ `codec.rs` ۱۲۳ و `discovery.rs` ۳۲۷.

**ریسک:** پایین. `codec` خالص است پس تست‌هایش بی‌تغییر کار می‌کنند و ثابت می‌کنند چیزی نشکسته.

---

## ۵. اصلاح دو ادعای قبلی

| ادعا | وضعیت |
|---|---|
| «`window_shape.rs` ۴ تابع را تکرار کرده» | **غلط.** آن ۴ مورد ۲۱ خط `#[cfg(not(windows))]` هستند — الگوی ایدیوماتیک. از ۱۰۵۳ خط، ۱۰۳۲ خط کد واقعی windows-only است. **دست‌نخورده.** |
| «`orb-drag-probe` کار نمی‌کند» | **غلط بود.** پروب درست بود؛ من هم‌زمان با شما بر سر نشانگر رقابت می‌کردم. ثبت شد در [LESSONS-LEARNED.md](LESSONS-LEARNED.md) بند ۵. |

---

## ۶. ترتیب — و آنچه واقعاً انجام شد

| گام | کار | نتیجه | ریسک |
|---|---|---|---|
| ۱ ✅ | `antigravity/protocol.rs` | `antigravity.rs` ۱۱۴۲ → `mod.rs` ۹۷۰ + `protocol.rs` ۲۳۴ · ۷ تست منتقل و اجرا شدند · ۱۹۹ سبز · ۰ clippy · release سبز | خیلی پایین |
| ۲ ✅ | `state/status.rs` + `StatusChannel` | `impl StateMachine` ۵۷۹ → **۵۲۲** خط · `machine.rs` ۱۱۲۷ → ۱۰۳۹ · ۱۹۹ سبز · ۰ clippy | پایین |
| ۳ ⚡ | `lib.rs` L1 — `DashboardFlags` | `OverlayApp::new` از **۱۳ به ۹** پارامتر · `tray::spawn` از ۸ به ۳ · `OverlayApp` از ۴۳ به **۳۰** فیلد · ۴ تست تازه · ۲۱۱ سبز · ۰ clippy · release سبز | پایین |
| ۴ | `lib.rs` L3 — `run_gui` | — | متوسط |
| ۵ | `antigravity/discovery.rs` | — | پایین |

### ⚠️ یافتهٔ گام ۲ که باید ثبت شود

canary در `StatusChannel::set_state` **هرگز نسوخت** ⇒ هیچ تستی به کد منتقل‌شده نمی‌رسد.
۱۴ تست `state::machine` فقط توابع خالص را می‌آزمایند (`should_flush_chunk`، `LatchPolicy`،
`SeamStitcher`) و هرگز `StateMachine` نمی‌سازند.

یعنی «۱۹۹ تست سبز» در این مرحله فقط می‌گوید **چیزی نشکست**، نه اینکه کد منتقل‌شده درست کار
می‌کند. تضمین از کامپایلر می‌آید. جزئیات: [LESSONS-LEARNED.md](LESSONS-LEARNED.md) بند ۸.

**بعداً جبران شد:** ۸ تست برای `StatusChannel` نوشته شد (۱۹۹ → ۲۰۷) و با **آزمون جهش** اعتبارسنجی
شد — دو قاعده عمداً شکسته شدند و هر بار دقیقاً همان تست درست قرمز شد.

### ⚠️ یافتهٔ گام ۳

canary در `DashboardFlags::take` هم **نسوخت**: هیچ تستی `OverlayApp::update` را صدا نمی‌کند.
ولی **خودِ منطق** پرچم‌ها با ۴ تست تازه پوشش دارد (۲۰۷ → ۲۱۱). چیزی که بی‌پوشش مانده فقط
سیم‌کشیِ مصرف در `update()` است.

نکتهٔ جدا: `overlay_toggles_visibility` قبلاً یک تاپل **پنج‌تایی** پرچم به‌علاوهٔ یک `settings_flag`
جدا می‌ساخت — چون سازنده شش پارامتر هم‌نوع می‌خواست. این دقیقاً همان باگی است که ساختار
جلویش را می‌گیرد و کامپایلر نمی‌توانست بگیرد.

## ۷. قاعدهٔ اجرا

بعد از **هر گام**:
1. `cargo test --lib` ⇒ باید دقیقاً ۱۹۹ پاس بماند، نه کمتر نه بیشتر
2. `cargo clippy --all-targets` ⇒ صفر warning
3. `cargo build --release` ⇒ سبز
4. برای هر پنل/بخش جابه‌جاشده، یک **canary** موقت ⇒ تست باید واقعاً به آن برسد.
   **و اگر canary نسوخت، یعنی پوشش نیست — این را ثبت کن، نه اینکه نادیده بگیری.**
5. commit فقط با اجازهٔ صریح

---

## ۹. نتیجهٔ گام ۴ و ۵ (۲۰۲۶-۰۹-۳۰)

هر دو گام فقط با جابه‌جایی کد تمام نشدند؛ در هر دو، کدِ منتقل‌شده **قابل تست** شد —
چون همان چیزی بود که در بخش ۱ «بزرگ‌ترین خطر» نامیده شده بود.

| | قبل | بعد | تست | canary |
|---|---|---|---|---|
| `lib.rs` GUI block | ۱۴۵ خط درون‌خطی | ۱۴ خط + [bootstrap.rs](../voice-ptt/src/gui/bootstrap.rs) ۳۸۵ | **۱۱** | ۶ تست از ۴ جهش قرمز شد |
| `antigravity` discovery | ۳۲۲ خط در `mod.rs` | [discovery.rs](../voice-ptt/src/asr/antigravity/discovery.rs) ۵۳۰ | **۹** | ۲ تست از ۲ جهش قرمز شد |
| `lib.rs` | ۵۸۴ خط | **۴۷۷ خط** | | |
| تست `--lib` | ۱۹۹ | **۲۲۴** | | |

### آنچه کشف شد

- `candidate_endpoints` پورت ۰ را از مسیر `netstat` عبور می‌داد ⇒ یک خط `continue`.
- تست‌های پنجره، مختصاتِ محاسبه‌شده را به `with_position` وصل نمی‌کردند ⇒ تست ۱۱ام اضافه شد.
- `rustfmt --skip-children` روی این ماشین کار نمی‌کند (بند ۱۱ [LESSONS-LEARNED.md](LESSONS-LEARNED.md)).

### کارهای باقی‌مانده

- `run()` هنوز ۴۶۳ خط است و **صفر تست** دارد — بخش‌های ASR (۱۱۳ خط) و tray+hotkeys (۳۸) بعدی‌اند.
- `state/machine.rs`: حلقهٔ ضبط ۱۷۰ خط · قطعه‌ها ۱۵۶ · `finalize` ۱۰۱.
- `OverlayApp` ۳۰ فیلد دارد؛ کاهش بیشتر به testutil وابسته است.
