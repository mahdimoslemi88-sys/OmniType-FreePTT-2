# INDEX — نقشهٔ مستندات OmniType FreePTT

این فایل **نقطهٔ ورود** است. اگر فقط یک فایل از این پوشه را بخوانی، این باشد.

> چرا این فایل وجود دارد: در جریان بررسی هالهٔ سفید، سه بار به این نتیجه رسیدیم که «ادعای قبلی»
> درست نبوده چون اندازه‌گیری تکرار نشده بود. ریشهٔ مشکل نبودن ابزار نبود، نبودن **ثبت مکتوب** بود.
> از این پس هر عددِ اندازه‌گیری‌شده و هر درسِ روشی اینجا یا در یکی از دو فایل خوانده می‌شود.

---

## ۱. چهار قاعدهٔ حاکم بر کار در این ریپو

این‌ها اختیاری نیستند. Violate کردنشان دقیقاً همان خطایی را تکرار می‌کند که سه بار تکرار شد.

| # | قاعده | چرا |
|---|---|---|
| ۱ | **اول اندازه بگیر، بعد اصلاح کن.** | سه بار پیش آمد که تشخیص اولیه غلط بود. هر کدام با یک آزمایش ساده رد شد. |
| ۲ | **تغییر پرریسک حذف نمی‌شود؛ غیرفعال/کامنت می‌شود** تا rollback در درخت بماند. | نمونه: `SHOW_TRANSCRIPT_CARD = false` در [overlay.rs](../voice-ptt/src/gui/overlay.rs) و `preview_window.rs` که دست‌نخورده و تست‌شده باقی ماند. |
| ۳ | **commit / push / release فقط با درخواست صریح کاربر.** | نصب روی سیستم کاربر (`%LOCALAPPDATA%`) و انتشار ریلیز، هر دو نیاز به اجازهٔ جداگانه دارند. |
| ۴ | **کامنت باید «چرا» را بگوید نه «چه».** هر عددی که در تست قفل می‌شود باید از رفتار واقعی استخراج شود. | تستی که عددِ حدسی را قفل کند، حدس را به قانون تبدیل می‌کند. |

---

## ۲. نقشهٔ اسناد

### همیشه بخوان
| فایل | چه چیزی |
|---|---|
| **[INDEX.md](INDEX.md)** | همین فایل — نقطهٔ ورود |
| **[REFACTOR-REPORT.md](REFACTOR-REPORT.md)** | گزارش ریفکتور فاز دوم: ۹ گام، ۱۶ ماژول تازه، ۳ نقص واقعی که پیدا شد. |
| **[MEASURED-FACTS.md](MEASURED-FACTS.md)** | اعدادِ اندازه‌گیری‌شدهٔ قفل‌شده + محل دقیق کد. هیچ‌کدام نباید دوباره حدس زده شوند. |
| **[LESSONS-LEARNED.md](LESSONS-LEARNED.md)** | اشتباه‌هایی که در این پروژه کردیم و راه‌حل هرکدام. شامل اشتباه‌های خودِ دستیار. |

### موضوعی
| فایل | چه چیزی |
|---|---|
| [PRODUCT-DEVELOPMENT-ROADMAP.md](PRODUCT-DEVELOPMENT-ROADMAP.md) | رودمپ توسعهٔ محصول: تثبیت فارسی و اُرب، حفظ مقصد تایپ، پیش‌نمایش، گوشهٔ انتظار، بازیابی، پروفایل‌ها و اصلاحات ساختاری؛ با وابستگی‌ها و معیار پذیرش. |
| [AGENT-EXECUTION-PLAN.md](AGENT-EXECUTION-PLAN.md) | دسته‌بندی تمام مراحل، موج‌های موازی با سه ایجنت اجرایی و یک هماهنگ‌کننده، مالکیت فایل‌ها، وابستگی‌ها و پرامپت‌های اجرا و ادغام. |
| **[GUI-WINDOW-ARTIFACT-REPORT.md](GUI-WINDOW-ARTIFACT-REPORT.md)** (۵۷۹+ خط) | گزارش اصلی آرتیفکت پنجره. بخش ۱–۱۵ تاریخچهٔ کامل، بخش ۱۶ وضعیت فعلی هالهٔ سفید. |
| [GUI-BUGFIX-PLAN.md](GUI-BUGFIX-PLAN.md) | پلن‌های اصلاح GUI |
| [HANDOFF.md](HANDOFF.md) | تحویل کار به نفر بعد |
| [TESTING-guide.md](TESTING-guide.md) | راهنمای تست |
| [ui-audit-and-design-options.md](ui-audit-and-design-options.md) · [ui-overhaul-design.md](ui-overhaul-design.md) | طراحی رابط |
| [light-theme-tuning.md](light-theme-tuning.md) | تنظیم تم روشن |
| [Comprehensive-research-program-developing-roadmap.md](Comprehensive-research-program-developing-roadmap.md) · [proposal-0.1.md](proposal-0.1.md) | اسناد برنامهٔ تحقیق — از محدودهٔ فعلی خارج‌اند |
| [PHASE-3-SESSION-LOOP.md](PHASE-3-SESSION-LOOP.md) | شکستن حلقهٔ ضبط به تصمیم‌های خالص (فاز ۳) |

### ابزار (کد اجرایی، نه متن)
| مسیر | چه چیزی |
|---|---|
| `reaserch/gui/probes/` | پروب‌های PowerShell برای اندازه‌گیری زندهٔ پنجره. **پیش از استفاده بخش «کوری» فایل‌هایی که تازه ساخته شده‌اند را در [LESSONS-LEARNED.md](LESSONS-LEARNED.md) ببین.** |
| [halo-selftest.py](reaserch/gui/probes/halo-selftest.py) · [halo-hunt.ps1](reaserch/gui/probes/halo-hunt.ps1) | شکار خودکار هالهٔ سفید. اول `halo-selftest.py` (۵ کنترل، دروازهٔ اجرا)، بعد `halo-hunt.ps1` (کشیدن اورب با تأیید هندسی، تحلیل مستطیل ترک‌شده، بازگردانی موقعیت). هر دو **تنها** اجرا شوند. |
| [orb-click-target-probe.ps1](reaserch/gui/probes/orb-click-target-probe.ps1) | هدفِ کلیک اورب را از خودِ پنجره می‌خواند و کنار شعاع‌های پیش‌بینی‌شده می‌گذارد. **پنج کنترل کوری قبل از هر گزارشی**، و عمداً نامِ حالت را نمی‌گوید |
| [list-windows.ps1](reaserch/gui/probes/list-windows.ps1) · [arc-analyse.py](reaserch/gui/probes/arc-analyse.py) | فهرست پنجره‌های یک پروسه در طول زمان (ابزار کشف نقص DPI) · تحلیل‌گر کمان با خط `VERDICT: REPRODUCED\|NOT_AN_ARC\|CLEAN\|INCONCLUSIVE`. |
| [canary-harness.sh](canary-harness.sh) | موتور مشترک کاناری جهشی — اسنپ‌شات پاک، `trap`، تفکیک `NOBUILD` از `MISSED`، و **قفلِ pid** که دو اجرای هم‌پوشان را رد می‌کند (درس ۲۸) |
| [mutation-check-session.sh](mutation-check-session.sh) | کاناری جهشی موج ۱ — `S1` + `O1` + `T1`، ۲۵ تصمیم، از `v-2/voice-ptt` اجرا شود |
| [mutation-check-tray-warning.sh](mutation-check-tray-warning.sh) | کاناری جهشی نشان هشدار ترای — ۲۲ تصمیم |
| [mutation-check-startup.sh](mutation-check-startup.sh) | کاناری جهشی بوت: دانلود مدل + گارد اجرای state machine — ۱۲ تصمیم |
| `../third_party/egui-notify/` | کد وام‌گرفته‌شده، دست‌نخورده |

### اجرای رودمپ (موج صفر اجرا شده)
| فایل | چه چیزی |
|---|---|
| [execution/STATUS.md](execution/STATUS.md) | **دفتر پیگیری موج‌ها** — وضعیت هر بسته، مالکیت فایل، یافته‌های قفل‌شده، و شش تصمیم باز که مال کاربر است |
| [execution/CONTRACTS.md](execution/CONTRACTS.md) | قراردادهای مشترک (K0) — ۹ قرارداد، هرکدام با وضعیت «موجود» یا «پیشنهاد» و محل ثبت پیشنهادی |
| [execution/B0-orb-baseline.md](execution/B0-orb-baseline.md) | خط مبنای اُرب: مسیر پنجره، چهار شعاع، ماتریس حالت‌ها، سه یافتهٔ باز، و معیارهای پذیرش `O1` |
| [execution/T0-text-baseline.md](execution/T0-text-baseline.md) | خط مبنای متن: جدول ۱۷ نمونهٔ اجراشد��، سه کلمهٔ سالمِ خراب‌شده، و آنچه در کد اصلاً وجود ندارد |
| [execution/S0-session-baseline.md](execution/S0-session-baseline.md) | خط مبنای جلسه: نمودار رویداد→اثر→emit، نبودِ شناسه و مقصد، لغوی که بی‌اثر است |
| [../voice-ptt/tests/fixtures/text-baseline/](../voice-ptt/tests/fixtures/text-baseline/) | نمونه‌های متن. **از `T1` یک تست زنده‌اند**: هر `current` و هر مقدار زیر `modes` از یک اجرا آمده و `the_t0_fixture_still_describes_this_pipeline` آن را دوباره اجرا و مقایسه می‌کند |
| [execution/S1-handoff.md](execution/S1-handoff.md) | تحویل `S1` — هر دیکته یک شناسه گرفت؛ «کلید رها شد» دیگر با «پایان یافت» یکی نیست |
| [execution/O1-handoff.md](execution/O1-handoff.md) | تحویل `O1` — هدفِ کلیک و ناحیهٔ پنجره یک عدد شدند؛ حلقهٔ مردهٔ کلیک صفر pt |
| [execution/T1-handoff.md](execution/T1-handoff.md) | تحویل `T1` — سه حالت متن، و قاعدهٔ نیم‌فاصله به‌جای فهرستِ استثنا |
| [execution/T2-handoff.md](execution/T2-handoff.md) | تحویل `T2` — مقصدِ متن کجا می‌رود، و چه وقتی اصلاً نمی‌رود (در حال اجرا) |

> هر دو اسکریپت کاناری از [canary-harness.sh](canary-harness.sh) استفاده می‌کنند. این هارنس روی درخت قرمز `ABORT` می‌کند و از **یک اسنپ‌شات پاک** بازگردانی می‌کند، چون `.bak` چرخشی پشته است و یک اجرای قطع‌شده می‌تواند جهش‌هایش را ابدی کند (بندهای ۱۶–۱۹ [LESSONS-LEARNED.md](LESSONS-LEARNED.md)).

---

## ۳. وضعیت فعلی (snapshot)

> این بخش عمداً کوتاه نگه داشته می‌شود؛ اعداد تفصیلی در [MEASURED-FACTS.md](MEASURED-FACTS.md).

- **کد:** ۱۸٬۶۹۵ خط Rust در ۴۲ فایل (پیش از ریفکتور `overlay/`).
- **تست:** ۲۵۹ تست `--lib`، همه سبز. `cargo clippy --all-targets` صفر warning.
- **ریفکتور انجام‌شده:** `gui/overlay.rs` از ۴۲۵۰ خط به ۱۱۶۲ خط و ۹ ماژول مستقل رفت. `OverlayApp` از ۶۳ فیلد به ۴۳ فیلد رسید. **کامیت نشده.**
- **ریلیز:** `v0.3.0` منتشر شده. لینک در بخش ۵.
- **هالهٔ سفید بالای اورب:** **وجود دارد و درمان نشده.** ببین [بخش ۱۶ گزارش](GUI-WINDOW-ARTIFACT-REPORT.md).

---

## ۴. مسائل باز (به ترتیب اولویت)

| # | مسئله | وضعیت |
|---|---|---|
| ۱ | **هالهٔ سفید/روشن بالای اورب** | محرک شناسایی شده: **جابه‌جایی پنجره** (کلیک-کشیدن). نه به حالت ضبط ربطی دارد. درمان نشده. |
| ۲ | **`--doctor`** | توافق شده، ساخته نشده. ریشه‌اش: [listener.rs](../voice-ptt/src/hotkey/listener.rs) در خطای parse *بی‌صدا* هات‌کی را به `CapsLock` برمی‌گرداند و فقط در فایل لاگ می‌نویسد. |
| ۳ | **`state/machine.rs`** | **انجام شد (فاز ۳).** ۱٬۰۳۹ ⇒ **۶۷۷** خط. `run()` ۱۴۴ ⇒ **۴۸** خط و هیچ تصمیمی نمی‌گیرد؛ تصمیم‌ها در [session.rs](../voice-ptt/src/state/session.rs) و [utterance.rs](../voice-ptt/src/state/utterance.rs) خالص شدند. ۱۱ جهش از ۱۱ سوخته. |
| ۴ | **`lib.rs` — `run()` = ۴۹۲ خط، صفر تست** | **بدترین مشکل ساختاری کل پروژه.** ۱۵ فاز که خود کد با `// ----` اعلام کرده. طرح در [REFACTOR-PLAN.md](REFACTOR-PLAN.md) |
| ۵ | **`asr/antigravity.rs`** ۱۱۴۲ خط | سه بخش با مرز روشن: discovery ۳۲۷ / codec ۱۲۳ (خالص) / engine ۴۱۶ |
| ۶ | **`gui/orb.rs`** ۸۸۴ خط، `impl Orb` ۴۹۷ خط | کم‌اولویت: `impl` یک شیء واحد است و ۸ متدش هم‌بسته‌اند. |

### ❌ مواردی که **مشکل نیستند** (اندازه‌گیری رد کرد)

| ادعای قدیمی | واقعیت اندازه‌گیری‌شده |
|---|---|
| «`window_shape.rs` ۴ تابع را تکرار کرده چون شاخهٔ windows/non-windows دارد» | **غلط بود.** آن ۴ مورد فقط **۲۱ خط** `#[cfg(not(windows))]` هستند — یعنی الگوی ایدیوماتیک Rust. از ۱۰۵۳ خط، **۱۰۳۲ خط کد واقعی windows-only** است. دست‌زدن به آن churn بی‌فایده است. |

---

## ۵. ساختار ریپو

```
v-2/
├── voice-ptt/          crate اصلی (src/)
├── voice-ptt-dist/     خروجی نصب‌شده — gitignored
├── installer/          Inno Setup
├── docs/               همین پوشه
├── third_party/        وام‌گرفته‌ها، دست‌نخورده
└── .agents/skills/
```

`installer/Output/` و `voice-ptt-dist/` در `.gitignore` هستند؛ آرتیفکت فقط از راه release منتقل می‌شود.

ریموت: `origin` = `https://github.com/mahdimoslemi88-sys/OmniType-FreePTT-2.git` — شاخهٔ `main`.

---

## ۶. ساختار کد — نقشهٔ سریع

```
src/
├── main.rs            نقطهٔ ورود. #![windows_subsystem = "windows"] ⇒ stdout ندارد.
├── lib.rs             run(): ۱۵ فاز بوت، هنوز یک تابع — ۴۷۷ خط
├── gui/
│   ├── mod.rs         ثبت ماژول + re-export
│   ├── bootstrap.rs   راه‌اندازی یک‌بارهٔ eframe: هندسهٔ پنجره، فونت‌ها، ویژوال‌ها
│   ├── flags.rs       DashboardFlags: شش پرچم داشبورد + enum Toggle
│   ├── overlay.rs     پوستهٔ OverlayApp: state، update()، قاب داشبورد
│   ├── overlay/       settings_panel · dict_panel · engine_panel · history_panel
│   │                  · theme · text · toast · tests · testutil
│   ├── orb.rs         نقاشی اورب + جای‌گذاری و کشیدن پنجره (Win32)
│   ├── orb_animation.rs  فنر مقیاس
│   ├── window_shape.rs   Win32/DWM + ClickRegion
│   └── preview_window.rs ماژول dormant (۱۱ تست)
├── asr/  plan.rs (ترتیب موتورها — خالص و تست‌شده) · antigravity/{mod,protocol,
│        discovery} · cloud · downloader · google
│        progress · quota · router · whisper
├── audio/  capture · device · ring_buffer
├── state/  machine.rs (ماشین: فقط اجرا) · session.rs (تصمیم‌های جلسه — خالص)
│           utterance.rs (ارزش یک رونویسی — خالص) · status.rs (StatusChannel)
│   پنجرهٔ اُرب: بوم از هندسهٔ واقعی رسم مشتق می‌شود (mod reach)؛ هم بوم و هم
│   منطقهٔ کلیک از یک تابع ⇒ کوچک‌شدن یکی بدون دیگری ممکن نیست
├── vad/    mod · silero
├── config/settings.rs
├── hotkey/  binding (VK) · diagnostics (چرا هات‌کی عوض شد) · listener
├── processing/  dictionary · normalizer · seam
└── paths.rs · updates.rs · logging.rs
```

---

## ۷. پیش از هر کار جدید

۱. [MEASURED-FACTS.md](MEASURED-FACTS.md) را بخوان — شاید جواب قبلاً اندازه‌گیری شده باشد.
۲. [LESSONS-LEARNED.md](LESSONS-LEARNED.md) بخش «پروب‌های کور» را بخوان.
۳. اگر مسئله پنجره/اورب است: [بخش ۱۶ گزارش](GUI-WINDOW-ARTIFACT-REPORT.md).
