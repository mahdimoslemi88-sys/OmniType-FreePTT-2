# ممیزی گروه‌بندی کامیت‌ها — تغییرات کامیت‌نشده

تاریخ ممیزی: ۲۰۲۶-۱۰-۰۴ · ویرایش چهارم (پس از پذیرش سه سند محصول، اجرا و قرارداد، و پس از
ثبت نتیجهٔ بررسی نسخه‌های میانی)
شاخه: `main` · HEAD: `871b2b5 Remember which window the text was meant for`
مخزن: `v-2/` (ریشهٔ مخزن git در `v-2/.git` است؛ پوشهٔ بیرونی git ندارد)

---

## ۰. روش، دامنه و آنچه انجام **نشد**

ویرایشِ متنیِ این سند فقط‌خواندنی بود. دستورهای اجراشده برای همین ویرایش محدود به این‌ها بود:
`git status --porcelain`, `git rev-parse`, `git diff`, `git diff --stat/--numstat/-U0/-U2/-U3`,
`git log --oneline`, `grep`/`sed` روی فایل‌های موجود، و خواندن مستقیم فایل‌های جدید.

**انجام نشد** (طبق دستور): `git add`/`git commit`/`git push`، هر تغییری در index، ساخت exe،
اجرای زنده، اجرای `docs/canary-anchor-check.py` یا هر اسکریپت کاناری، و تغییر کد تولیدی.

**یک استثنا که باید صریح گفته شود:** در مرحلهٔ پیش از این ویرایش، بررسیِ نسخه‌های میانی
(بند ۵٫۱) انجام شد و در آن `cargo test` و `cargo clippy` **اجرا شدند** — ولی نه در این
checkout: در درختی جداگانه به نام `.audit-intermediate/` که از `git archive HEAD` ساخته شده و
`.git` ندارد. آن مرحله چیزی در `v-2/`، در index یا در تاریخچه ننوشت، و همهٔ آن اجراها **پیش از
کامپایل** با موانع محیطی متوقف شدند (بند ۵٫۱). پس «build/test اجرا شد» را نباید «build/test
موفق بود» خواند.

**ویرایش‌های سوم و چهارم فقط روی همین سند و `docs/AGENT-EXECUTION-PLAN.md` انجام شدند** و
همگی ویرایش‌ها متنی‌اند. (ویرایش دوم فقط روی همین سند، `docs/execution/STATUS.md` و
`docs/execution/T2-handoff.md` بود؛ ویرایشِ سه سندِ پذیرفته‌شده پیش از آن انجام شده بود.)

### ۰٫۱ وضعیت شواهد: سه گونه ادعا را از هم جدا نگه دارید

| ادعا | شاهد | وضعیت |
|---|---|---|
| **درخت ترکیبی فعلی** سالم است (ساخت + clippy + آزمون‌ها) | بررسیِ مستقل و موفق، خارج از این ممیزی | **تأییدشده** — همین‌طور که در [STATUS.md](STATUS.md) «کل کتابخانه» و [T2-handoff.md](T2-handoff.md) بند ۶ ثبت شده |
| **بازاعمال ۲۷ فایل روی خط مبنا** درختِ امروز را درست بازمی‌سازد | مقایسهٔ محتوا: ۲۷/۲۷ فایل با `sha256` یکسان، برگشت‌پذیریِ فایلِ تقسیم‌شده در هر دو جهت، ۲۷۱ فایل بیرون از طرح بدون تفاوت (بند ۵٫۱) | **تأییدشده از نظر محتوا** — ساخت و آزمونِ نسخه‌های میانی به‌دلیل موانع محیطی **تأییدنشده** است |
| **هر کامیت پیشنهادیِ زیر** مستقل ساخته می‌شود | ندارد: تلاش برای ساختِ نسخه‌های میانی در هر پنج checkpoint پیش از کامپایل متوقف شد (بند ۵٫۱) | **بررسی‌نشده** |

اولی دربارهٔ **درختی است که همهٔ گروه‌ها با هم دارد**. سومی دربارهٔ **درختی است که فقط یک گروه
دارد**. اولی جانشین سومی نیست، و سومی هم جانشین اولی نیست: «محتوا یکی است» ادعای محتواست،
«ساخته می‌شود» ادعای رفتار است. این ممیزی برای سومی هیچ شاهدی ندارد و آن را «بررسی‌نشده»
اعلام می‌کند؛ برای گرفتن شاهد باید هر گروه جداگانه در یک worktreeٔ موقت ساخته شود، که خارج
از دامنهٔ این مرحله بود.

---

## ۱. موجودی: آغاز ممیزی در برابر وضعیت فعلی

### ۱٫۱ موجودی در لحظهٔ آغاز ممیزی (۲۱ تغییریافته + ۵ جدید)

این‌ها همان‌هایی هستند که این ممیزی تحلیل کرده و **مبنای ارجاع‌های بخش ۲** است.

| فایل | +/− | موضوع |
|---|---|---|
| `voice-ptt/src/processing/normalizer.rs` | +498/−70 | حفظ همزه، پایه‌های مثبت اسم/فعل/صفت، جداکردن علائم |
| `voice-ptt/src/state/machine.rs` | +623/−634 | بازنویسی: `StateMachine` → `MachinePort` + تولیدکنندهٔ ضربان |
| `voice-ptt/src/output/injector.rs` | +317/−30 | `Injection` نوع‌دار + `*_with` (شکستن API) |
| `voice-ptt/src/processing/seam.rs` | +188/−8 | حفظ فاصله/خط جدید؛ غیرفعال‌کردن ترمیم وقتی قطعهٔ پیشین فاصلهٔ انتهایی دارد |
| `docs/execution/T2-handoff.md` | +442/−22 | بندهای ۴، ۴ب–۴ه، ۵، ۶ |
| `docs/execution/STATUS.md` | +94/−10 | تصمیم‌های ۲۰۲۶-۱۰-۰۲، جدول وضعیت فعلی، بستهٔ نرمال‌ساز |
| `voice-ptt/src/state/utterance.rs` | +137/−0 | `InjectOutcome` + `judge_insert` |
| `voice-ptt/src/state/session.rs` | +115/−13 | `DiscardSession(Option<SessionId>)`، لغوی که به جلسهٔ در حال تبدیل می‌رسد |
| `voice-ptt/src/processing/mod.rs` | +105/−0 | ثبت `boundary` + ۶ آزمون متنی |
| `voice-ptt/src/gui/orb.rs` | +56/−0 | لبهٔ press و لاگ اشاره‌گر |
| `docs/mutation-check-session.sh` | +33/−35 | حذف لنگرهای مرده، انتقال C7، یادداشت شکاف |
| `docs/execution/O1-handoff.md` | +32/−1 | بخش «ابزار سنجش» |
| `docs/canary-harness.sh` | +19/−3 | قاعدهٔ ۶، تشخیص شکست بازگردانی، mutate با CRLF |
| `docs/INDEX.md` | +9/−4 | ردیف‌های تازه، درخت `state/` |
| `voice-ptt/src/processing/dictionary.rs` | +16/−30 | **صرفاً قالب‌بندی مجدد (rustfmt)** |
| `voice-ptt/src/gui/overlay.rs` | +13/−2 | لاگ `orb click handled` |
| `voice-ptt/src/output/mod.rs` | +8/−2 | گسترش `pub use` |
| `voice-ptt/src/output/target.rs` | +10/−0 | **صرفاً توضیح مستندات** |
| `voice-ptt/src/state/mod.rs` | +1/−0 | `pub(crate) mod coordinator;` |
| `voice-ptt/Cargo.toml` | +5/−0 | dev-dep `tokio` با `test-util` |
| `voice-ptt/tests/fixtures/text-baseline/cases.json` | +5/−5 | `T0-009` (همزه): UNDECIDED → OK |

**جدید در آغاز ممیزی (۵):** `voice-ptt/src/state/coordinator.rs` (۵۲۶۴ خط) ·
`voice-ptt/src/processing/boundary.rs` (۳۹۸ خط) ·
`docs/mutation-check-coordinator.sh` (۲۷۴ خط) ·
`docs/canary-anchor-check.py` (۷۰ خط) ·
`docs/execution/COORDINATOR-BRIEF-2026-10-02.md` (۳۴۵ خط)

### ۱٫۲ موجودیِ فعلی (پس از این مرحله) — ۲۴ تغییریافته + ۶ جدید

| | فایل | چه چیزی عوض شد |
|---|---|---|
| **در ۱٫۱ هم بود؛ ویرایش دوم ویرایشش کرد** | `docs/execution/STATUS.md` | ادعای Backspace در سطرِ «مرز قطعات» (سطر ۲۴۶) |
| **در ۱٫۱ هم بود؛ ویرایش دوم ویرایشش کرد** | `docs/execution/T2-handoff.md` | ادعای Backspace در سطر ۵۲۱، و تفسیر زمان‌بندی ضربان در سطر ۳۴۴ و ۳۵۲ |
| **در ۱٫۱ نبود؛ پس از آن تغییر کرد** | `docs/PRODUCT-DEVELOPMENT-ROADMAP.md` | بند ۵٫۴ به «بازیابی متنِ درج‌نشده» رفت؛ ردیف «نگهداری صدا» حذف شد؛ یادداشتِ doctor و ردیف «دوبار زدن = قابلیت موجود» اضافه شد |
| **در ۱٫۱ نبود؛ پس از آن تغییر کرد** | `docs/execution/CONTRACTS.md` | بند ۶ (`V1` کامل نیست)، بند ۸ (تصحیحِ ادعای doctor)، بند ۹ (`R1` کامل نیست) |
| **در ۱٫۱ نبود؛ پس از آن و در این ویرایش تغییر کرد** | `docs/AGENT-EXECUTION-PLAN.md` | شرطِ خط مبنای یکسان و مالکِ واحد، جدولِ وضعیتِ بسته‌ها، و تصحیحِ برچسبِ `V1` به «پیش‌نویسِ پیش از درج» |
| **تازه اضافه شد** | `docs/execution/COMMIT-GROUPING-AUDIT.md` | همین سند |

**۲۶ ورودی در آغاز ممیزی ⇒ ۳۰ ورودیِ فعلی (۲۴ تغییریافته + ۶ جدید).** سه ورودیِ تازه،
هر سه سندِ پذیرفته‌شدهٔ `PRODUCT-DEVELOPMENT-ROADMAP.md`، `AGENT-EXECUTION-PLAN.md` و
`CONTRACTS.md` هستند: آن‌ها در آغاز ممیزی **تغییریافته نبودند** و بعداً تغییر کردند، پس شمارشِ
«تغییریافته» از ۲۱ به ۲۴ رفت. ورودیِ چهارم همین سند است که «جدید» است. یعنی ۲۱ + ۳ = ۲۴ و
۵ + ۱ = ۶. از ۲۱ فایلِ تغییریافتهٔ آغاز ممیزی، ۱۹ فایل **کاملاً دست‌نخورده** باقی مانده‌اند
(دو فایلِ ویرایش‌شدهٔ ویرایش دوم به‌شمار می‌آیند) و هر ۵ فایلِ جدیدِ آغاز ممیزی هم دست‌نخورده‌اند.

> **یک کم‌شماری در ویرایش دوم اصلاح شد:** آن نسخه «۲۰ فایلِ تغییریافتهٔ دیگر» نوشته بود، در
> حالی که جدول ۱٫۱ دقیقاً ۲۱ ردیف دارد و دو ردیفِ ویرایش‌شده کنار گذاشته می‌شوند ⇒ **۱۹، نه ۲۰**.

**گزارشِ محیط ساخت، تحویل جداگانه است.** `git status --porcelain` اکنون ۳۱ سطر می‌دهد:
۲۴ `M` و ۷ `??`. عددِ این سند ۳۰ ورودی است، نه هر ورودیِ untrackedِ روی شاخه. سطرِ `??`
هفتم — `docs/execution/O1-MANUAL-ACCEPTANCE.md` (۶۴۳ خط؛ **برنامهٔ آزمون دستیِ `O1`**: هاله،
هدف کلیک، لبه‌های نمایشگر، DPI و کشیدن) — سندی **همزمان و جدا از این ممیزی** است که این
سند آن را نوشته نیست: **در این ۳۰ ورودی شمرده نمی‌شود** و در طرحِ بخش ۳ هم نمی‌آید.
گزارشِ محیط ساخت که ممکن است همزمان ایجاد شود **تحویل جداگانهٔ دیگری** است و همین قاعده
برایش برقرار است: نه در این ۳۰ شمرده می‌شود و نه در این طرح می‌آید، مگر آنکه جداگانه دربارهٔ
کامیتش تصمیم گرفته شود.

> این تفکیک لازم است چون جدول ۱٫۱ «موجودیِ ورودیِ ممیزی» است و جدول ۱٫۲ «موجودیِ امروز».
> اگر این دو یکی شوند، شمارش‌های بخش ۴ و جدول نهاییِ بخش ۳ بی‌مرجع می‌شوند.

### ۱٫۳ تاریخی در برابر فعلی — قاعدهٔ خواندن

`STATUS.md` و `T2-handoff.md` هر دو از قبل بخش‌های تاریخی را **برچسب‌گذاری** کرده‌اند:
عنوان‌های «وضعیت در ۲۰۲۶-۱۰-۰۲ (تاریخی — وضعیت در آن زمان)» و «وضعیت تاریخی در پایان
قطعهٔ ۲». این ممیزی آن‌ها را **وضعیت فعلی معرفی نمی‌کند**. وضعیت فعلی فقط از دو جای برچسب‌خورده
با عنوان «وضعیت فعلی (۲۰۲۶-۱۰-۰۴ — بر اساس پیاده‌سازی و ممیزی)» خوانده می‌شود.

تفاوت این دو مهم است و در بخش ۶ ریسک ۴ هم اثر دارد: بخش تاریخی `STATUS.md`
(«تصمیم‌های هماهنگ‌کننده ۲۰۲۶-۱۰-۰۲») سه ارجاع به کدِ **پیش از** این تغییرات دارد، و آن‌ها
شاهدِ تصمیمِ آن روزند نه ادعای وضعیت امروز.

---

## ۲. دسته‌بندی و وابستگی واقعی (با ارجاع به کد)

> همهٔ ارجاع‌های خطی به **موجودیِ آغاز ممیزی** (بند ۱٫۱) اشاره دارند.

### گروه A — نرمال‌ساز فارسی و خط مبنای متن
**فایل‌ها:** `processing/normalizer.rs` · `tests/fixtures/text-baseline/cases.json` ·
بخش آزمونیِ `processing/mod.rs`

**وابستگی‌های سخت:**
- `cases.json` ↔ `normalizer.rs` — **دوطرفه و بدون استثنا.** آزمون
  `processing/mod.rs:231` (`the_t0_fixture_still_describes_this_pipeline`) هر مقدار `modes`
  و `current` فیکسچر را اجرا می‌کند و در پایان `assert_eq!(checked, 42)` دارد (`mod.rs:273`).
  `T0-009` در فیکسچر اکنون `مسأله` را برای `standard` و `conservative` انتظار دارد؛ این فقط با
  حذف نگاشت‌های `('ؤ','و')` و `('أ','ا')` از `ARABIC_TO_PERSIAN` (`normalizer.rs:21`) ممکن است.
  فقط یک طرف را commit کردن ⇒ آزمون lib می‌شکند.
- آزمون‌های تازهٔ `processing/mod.rs` (`hamza_is_preserved_…`, `healthy_words_are_preserved…`,
  `valid_samples_corrected_without_breaking_similar_words`, `explicit_space_in_mi_rom_preserved…`,
  `latin_digits_are_preserved_without_conversion`, `raw_mode_bypasses_…`) به
  `NOUN_STEMS_FOR_HA`، `PRESENT_VERB_STEMS`/`PAST_VERB_STEMS` و `ADJECTIVE_STEMS` تکیه دارند.
  نمونهٔ قاطع: `process_text("بزرگترین") == "بزرگ‌ترین"` فقط با `ADJECTIVE_STEMS` ممکن است و
  `process_text("اژدها") == "اژدها"` فقط با حذف `is_standalone_word` ممکن است.

**استقلال ساخت: بررسی‌نشده.**

---

### گروه B — درزِ قطعه: حفظ فاصلهٔ خام، و غیرفعال‌کردن ترمیم پس از فاصلهٔ انتهایی
**فایل:** `processing/seam.rs`

**آنچه واقعاً رفتار است (نه آنچه در نسخهٔ اول این سند گفته شد):**

> **اصلاح (نکتهٔ ۱):** ترمیمِ حدسیِ Backspace **همچنان وجود دارد** و حذف نشده است.
> `SeamStitcher::backspace_count` فقط در **یک** حالت زودتر بازمی‌گردد: وقتی `prev_trailing`
> نشان دهد قطعهٔ پیشین با فاصلهٔ انتهایی تمام شده است — که آن‌وقت ثابت می‌شود واژه در میانه
> بریده نشده بود. در بقیهٔ حالت‌ها، مسیر حدسیِ پیشین با همان آستانهٔ `MIN_FRAGMENT` و همان فهرست
> `NEVER_FRAGMENTS` برقرار است.
> پس ادعای درست «حذف ترمیم حدسی» نیست؛ ادعای درست «مشروط‌کردن ترمیم به نبودِ فاصلهٔ انتهایی» است.

- `extract_trailing` فاصلهٔ انتهایی قطعهٔ جاری را نگه می‌دارد و `reset()` آن را پاک می‌کند.
- `find_word_remainder_after_drop` به‌جای `join(" ")`، جداکنندهٔ واقعی (خط جدید، tab، فاصلهٔ
  چندگانه) را نگه می‌دارد؛ اگر جداکننده فقط یک فاصلهٔ معمولی باشد، برش از کلمهٔ بعد شروع می‌شود.

**وابستگی‌ها:** هیچ‌کدام به فایل‌های تغییریافتهٔ دیگر. امضای عمومی
`SeamStitcher::stitch`/`SeamMerge`/`SeamOptions` دست‌نخورده است؛ تنها فیلد خصوصی
`prev_trailing` و دو تابع کمکی خصوصی اضافه شده‌اند. هر دو مصرف‌کننده
(`machine.rs` قدیمی و `coordinator.rs` جدید) بدون تغییر کار می‌کنند.

**استقلال ساخت: بررسی‌نشده.**

---

### گروه C — لاگِ اشاره‌گر اورب
**فایل‌ها:** `gui/orb.rs` · `gui/overlay.rs` · بخش «ابزار سنجش» در `docs/execution/O1-handoff.md`

**وابستگی‌ها:**
- `orb.rs` هیچ وابستگی تازه‌ای ندارد: `win::cursor_position()` از قبل در همان فایل
  (`orb.rs:369`, `orb.rs:380`, `orb.rs:968`) استفاده می‌شود.
- `overlay.rs` فقط `tracing::info!` اضافه می‌کند و شاخهٔ `orb_out.clicked` را به یک `let action`
  تبدیل می‌کند — بدون تغییر منطق.
- **این تنها گروهی است که کاملاً مستقل از زنجیرهٔ processing → output → state است.**
- `O1-handoff.md` سندِ همین دو خط لاگ است (`O1-handoff.md:320` و `O1-handoff.md:322` هر دو
  رشتهٔ تولیدی کد را نقل می‌کنند)؛ جداکردن آن از کد، سند را به ادعای بی‌شاهد تبدیل می‌کند.

**استقلال ساخت: بررسی‌نشده.**

---

### گروه D — گزارش صادقانهٔ درج
**فایل‌ها:** `output/injector.rs` · `output/mod.rs` · `state/utterance.rs`

**وابستگی‌های سخت:**
- `utterance.rs:13` → `use crate::output::Injection;` و امضای
  `judge_insert(steps: &[Injection]) -> InjectOutcome` (`utterance.rs:151`). بدون `injector.rs`
  این فایل کامپایل نمی‌شود.
- `output/mod.rs` راه `crate::output::Injection` را می‌سازد؛ در HEAD این نام باز-منتشر نشده و
  `coordinator.rs:44` هم دقیقاً همین مسیر را می‌گیرد.

**⚠ شکستن API:** `inject_text` از `Result<usize>` به `Injection` و `inject_backspaces` از
`Result<()>` به `Injection` تغییر کرده است (`injector.rs:89`, `injector.rs:135`). تنها مصرف‌کننده
در HEAD، `state/machine.rs` قدیمی است:

```
match inject_text(text) { Ok(chars) => EmitOutcome::Typed { chars }, Err(e) => … }
if let Err(e) = inject_backspaces(backspaces) { … }
```

**استقلال ساخت: بررسی‌نشده.** طبق بند ۶، این گروه باید در همان بستهٔ هماهنگ‌کننده بنشیند؛
برای جدا کردنش **آداپتور موقت نوشته نمی‌شود**.

---

### گروه E — لغوِ نشست‌محور
**فایل:** `state/session.rs`

**تغییر امضا:** `Effect::DiscardSession` از واحد به `DiscardSession(Option<SessionId>)`
(`session.rs:172`) و `cancelled()` به `cancelled(id: Option<SessionId>)` (`session.rs:377`).

**⚠ همان شکستن API:** در HEAD، `machine.rs` صدا می‌زند `self.with_session(|s| s.cancelled())`.
آزمون‌های خودِ `session.rs` به‌روز شده‌اند (`session.rs:995`, `1007`, `1033`, `1062`) و فایل با
خودش سازگار است — اما با فراخوانِ قدیمیِ `machine.rs` نیست.

**استقلال ساخت: بررسی‌نشده.** طبق بند ۶، در همان بستهٔ هماهنگ‌کننده می‌نشیند.

---

### گروه F — هماهنگ‌کننده (بستهٔ اتمیک)
**فایل‌ها:** `state/coordinator.rs` (جدید) · `state/machine.rs` · `state/mod.rs` ·
`voice-ptt/Cargo.toml` · `output/target.rs` (فقط مستندات)

**نقشهٔ وابستگی (همه سخت):**

| از | به | شاهد |
|---|---|---|
| `machine.rs` | `coordinator.rs` | `use super::coordinator::{Coordinator, Input, KeystrokeSink, PollOutcome, Port, Speech, SystemClock, WindowTargets}` |
| `state/mod.rs` | `coordinator.rs` | `pub(crate) mod coordinator;` — بدون این، `super::coordinator` ناشناخته |
| `coordinator.rs` | `injector.rs` | `crate::output::Injection`، `inject_text`، `inject_backspaces` (`coordinator.rs:44,65,66,73,77`) |
| `coordinator.rs` | `utterance.rs` | `is_transient, judge_insert, plan_typing, InjectOutcome, TypePlan, ERROR_READABLE` (`coordinator.rs:50`) |
| `coordinator.rs` | `session.rs` | `Effect::DiscardSession(id)` در بازوی لغو؛ `ChunkId, SessionDriver, SessionId` |
| `coordinator.rs` | `boundary.rs` | `crate::processing::boundary::BoundaryTracker` (`coordinator.rs:45`) و `boundary.apply(...)` در `apply` |
| `machine.rs` | `Cargo.toml` | `#[tokio::test(start_paused = true)]` در `machine.rs:614,661,733,769` و `tokio::time::advance` در `670,740,777` ⇒ ویژگی `test-util` لازم است |
| `output/target.rs` | `coordinator.rs` | لینک مستنداتی `[`crate::state::coordinator`]` در توضیح `TargetTracker` — تا وقتی coordinator ثبت نشود، لینک شکسته است |

**دربارهٔ زمان‌بندی ضربان (نکتهٔ ۲):** قاعدهٔ واقعیِ اعمال‌شده در کد، تابع
`next_beat_at` با فرمول `std::cmp::max(at + period, now + period)` است — یعنی اگر حلقه به
مهلت نرسید، مهلت بعدی از `now + period` حساب می‌شود تا رگبار ضربان پس از توقف طولانی رخ ندهد.
این قاعده **قاعدهٔ خودِ برنامه است** و ارجاعی به رفتار درونی Tokio یا بازتولید
`MissedTickBehavior::Skip` ندارد؛ همین در [T2-handoff.md](T2-handoff.md) بند ۴د و در
[STATUS.md](STATUS.md) سطرِ «زمان‌بندی ضربان» هم تصحیح شد. نامِ آزمون
`a_late_beat_is_followed_by_one_a_full_period_later` **تغییر نکرده است**.

**نکتهٔ لیست‌های قدیمی:** `coordinator.rs` از `crate::output::target::{TargetIdentity,
TargetValidity}` و `capture_target()`/`validate_target()` استفاده می‌کند، اما **همهٔ این‌ها در
HEAD وجود دارند** (`target.rs:27,44,68,150,172`) و `target.rs` فقط یک توضیح مستنداتی گرفته.

**چرا `lib.rs` در diff نیست:** `lib.rs:453` (`StateMachine::new(AppServices { … })`) و
`lib.rs:519` (`machine.run(events_rx)`) و `state/mod.rs:10` (`pub use machine::{AppServices,
StateMachine}`) هر سه بدون تغییر باقی مانده‌اند — یعنی **سازگاری سطح عمومی حفظ شده است**.

**استقلال ساخت: بررسی‌نشده.** درخت ترکیبی با موفقیت بررسی شده (بند ۰٫۱)، ولی استقلال این
بسته به‌تنهایی بررسی‌نشده است.

---

### گروه G — سیاست مرز (بند ۳ دستور)

**فایل‌ها:** `processing/boundary.rs` (جدید) · بخش ثبت ماژول در `processing/mod.rs`

#### ۳٫۱ وابستگی `boundary.rs` به `SessionId`

```rust
// processing/boundary.rs
use crate::output::target::TargetIdentity;
use crate::state::session::SessionId;
```

مصرف‌های واقعی آن در `BoundaryState` و `BoundaryTracker`:
```rust
pub struct BoundaryState { pub target: Option<TargetIdentity>, pub session: Option<SessionId>, pub last_char: char }
pub fn last_session_for(&self, target: &Option<TargetIdentity>) -> Option<SessionId>
pub fn record_success(&mut self, typed_text: &str, session: Option<SessionId>, target: Option<TargetIdentity>)
```

**یافتهٔ کلیدی:** `SessionId` در `session.rs:188` تعریف شده و **در HEAD هم دقیقاً همین‌جاست و
همین‌قدر عمومی است** (`pub struct SessionId(pub u64)`).

> `boundary.rs` به **تغییرات** `session.rs` وابسته نیست — فقط به `session.rs` در وضعیت HEAD
> وابسته است. گروه G می‌تواند **پیش از** گروه‌های D/E/F بیاید.

همین برای `TargetIdentity` صادق است: چهار میدان `hwnd`/`pid`/`exe_path`/`title_at_capture`
(`target.rs:29,31,35,39`) در HEAD موجودند و تغییر `target.rs` فقط متن توضیحی است.

#### ۳٫۲ معرفی در `processing/mod.rs`

```rust
pub mod boundary;                                    // بدون این، فایل اصلاً کامپایل نمی‌شود
pub use boundary::{is_attached_punctuation, needs_boundary_space, BoundaryState, BoundaryTracker};
```

- بدون خط `pub mod boundary;`، فایل جدید یک جزیرهٔ کامپایل‌نشده می‌ماند.
- `coordinator.rs:45` مسیر **ماژول** را می‌گیرد (`processing::boundary::BoundaryTracker`)، نه
  `pub use` را؛ ولی `pub use` عمومی‌بودن `BoundaryTracker` را از کریت بیرون می‌برد.
- **ملاحظهٔ طراحی (نه خطای ساخت):** `state/session` با `pub(crate) mod session;` اعلام شده و
  در مقابل `processing` و `boundary` عمومی‌اند. یعنی فیلد `pub session` و پارامترهای `pub`
  متدهای ردیاب، نوعی با دید مؤثر `pub(crate)` را از مسیری عمومی عبور می‌دهند.
  **بررسیِ مستقلِ درخت ترکیبی و clippy هر دو موفق بوده‌اند**، پس این در وضعیت فعلی مانع ساخت
  نیست. آنچه اینجا ثبت می‌شود یک **ملاحظهٔ طراحی** است: آیا دیدِ این دو باید عمداً متفاوت بماند
  (نوع‌های مرزی فقط درون‌کریتی بمانند) یا یکدست شود. **این سند هیچ تغییر کدی را پیشنهاد یا
  الزام نمی‌کند** و در جدول بخش ۳ به‌عنوان پیش‌نیاز کامیت هم نیامده است.
- `is_non_spacing_joiner` عمومی است ولی نه باز-منتشر شده و نه مصرف‌کنندهٔ بیرونی دارد (۳ استفاده،
  همه درون `boundary.rs`) — همان ملاحظهٔ سبک.

#### ۳٫۳ ترتیب اجباری
`boundary.rs` **باید پیش از** گروه F بیاید، چون `coordinator.rs` آن را import می‌کند.

**استقلال ساخت: بررسی‌نشده.**

---

### گروه H — ابزار سنجش و کاناری
**فایل‌ها:** `docs/canary-harness.sh` · `docs/canary-anchor-check.py` (جدید) ·
`docs/mutation-check-coordinator.sh` (جدید) · `docs/mutation-check-session.sh`

**وابستگی‌ها:**
- `canary-harness.sh` (قاعدهٔ ۶، تشخیص شکست `cp`، `newline=''` در `mutate`) — **مستقل از کد**.
  دلیلش هم روشن است: `.gitattributes` برای `*.sh` خط تیرهٔ LF و برای بقیه `text=auto` تعیین
  می‌کند، و `mutate` قدیمی با پایتونِ بدون `newline=''` هر CRLF را به LF می‌نوشت.
- `mutation-check-coordinator.sh` — **وابسته به کدِ** گروه‌های D، E، F، چون لنگرهایش رشته‌های
  درون‌کدِ تازه‌اند. با `grep -F` روی درخت فعلی تأیید شد که هفت لنگرِ نمونه موجودند:
  `std::cmp::max(at + period, now + period)` (C45)،
  `let mut due = tokio::time::Instant::now() + period;`،
  `self.whole() && self.accepted.is_multiple_of(2)` (C53، در `injector.rs`)،
  `if steps.iter().all(|step| step.whole_pairs())` (C52، در `utterance.rs`)،
  `None => TargetValidity::Unknown,` (C47)، `if !outcome.is_whole() {` (C51)،
  `let steps = if validity.allows_insert() {` (C46). هدف‌ها:
  `K=src/state/coordinator.rs`, `U=src/state/session.rs`, `M=src/state/machine.rs`,
  `E=src/state/utterance.rs`, `I=src/output/injector.rs` (خطوط ۱۶–۲۰).

  > **جای‌گذاری ابزار (نکتهٔ ۴):** **ابزار باید پس از کدِ هدف قرار گیرد.** دلیلش جفت‌شدگیِ
  > لنگرهاست: ابزار وقتی معنا دارد که رشته‌هایش در کد حاضر باشند. **نتیجهٔ اجرای این ابزار روی
  > نسخه‌های میانی بررسی‌نشده است**، چون در این مرحله هیچ اسکریپتی اجرا نشد؛ و آنچه در
  > نسخهٔ اول این سند دربارهٔ رفتار ابزار *پیش از* کد گفته شده بود، پیش‌بینیِ اجرایی بود و
  > این سند دیگر چیزی دربارهٔ آن قطعی ادعا نمی‌کند.
- `mutation-check-session.sh` — لنگر تازهٔ C7
  (`self.recording = false; / self.latch.reset(); / vec![Effect::DiscardSession(target)]`)
  فقط در `on_cancel` **تازه** وجود دارد ⇒ وابسته به گروه E. لنگرهای C12/C13/C17 حذف شده‌اند
  چون کدی که جهش می‌دادند در بازآرایی `machine.rs` از بین رفته — این حذف‌ها **شاهدی مستقل از
  diff کد** هستند و با گروه F هم‌جهت‌اند.
- `canary-anchor-check.py` — عمومی است (هارنس را import نمی‌کند و شکل `mutate` را خودش بازسازی
  می‌کند)، پس وابستگی کدی ندارد؛ ارزشش با `mutation-check-coordinator.sh` کامل می‌شود.

**استقلال ساخت: بررسی‌نشده.** (وضعیت واقعی `CAUGHT`/`SKIP` در این مرحله اجرا نشد و نامعلوم است.)

---

### گروه I — اسناد وضعیت، گزارش‌ها و سه سندِ پذیرفته‌شده
**فایل‌ها:** `docs/INDEX.md` · `docs/execution/STATUS.md` · `docs/execution/T2-handoff.md` ·
`docs/execution/CONTRACTS.md` · `docs/AGENT-EXECUTION-PLAN.md` ·
`docs/PRODUCT-DEVELOPMENT-ROADMAP.md` · `docs/execution/COORDINATOR-BRIEF-2026-10-02.md` (جدید) ·
بخش سنجشِ `docs/execution/O1-handoff.md`

**وابستگی‌ها:**
- `INDEX.md` سه چیز را با هم به‌روز می‌کند: ردیف اسکریپت‌های تازه و تعداد تصمیم‌های
  `mutation-check-session.sh` (گروه H)، و بلوک درخت `state/` که `coordinator.rs` را معرفی
  می‌کند (گروه F).
- `T2-handoff.md` بندهای ۴ب/۴ج/۴د/۴ه و ۶ را اضافه می‌کند — توصیف کدِ گروه‌های F، E، D، A.
- `COORDINATOR-BRIEF-2026-10-02.md` تاریخ‌دار و **تاریخی** است: گزارشِ وضعیت پیش از این تغییرات.
- **سه سندِ پذیرفته‌شده** (`PRODUCT-DEVELOPMENT-ROADMAP.md`، `AGENT-EXECUTION-PLAN.md`،
  `CONTRACTS.md`) هم‌زمان و پس از ویرایش دوم تغییر کردند. هر سه **متنی‌اند**: هیچ‌کدام کدِ
  تولیدی را تغییر نمی‌دهد و هیچ وابستگیِ ساختی به هیچ گروهی ندارند ⇒ از نظر ترتیبِ کامیت
  آزادند. آنچه در آن‌ها اصلاح شد فقط **ادعا** بود:
  - `CONTRACTS.md` — بند ۶ (`V1` کامل نیست)، بند ۸ (تصحیحِ ادعای افشای کلید توسط doctor؛
    قاعدهٔ درست «کلید در doctor رندر نمی‌شود» است، نه «doctor کلید را نشان می‌دهد»)، بند ۹
    (`R1` کامل نیست؛ شاهد `kept`).
  - `AGENT-EXECUTION-PLAN.md` — دو شرطِ غیرقابل‌مذاکرهٔ اجرای موازی (خط مبنای یکسان و مالکِ
    واحدِ فایل‌های مرکزی)، جدولِ «وضعیت فعلیِ بسته‌ها»، و تصحیحِ برچسبِ `V1` به «پیش‌نویسِ
    قابل‌ویرایش و تأییدِ **پیش از درج**» (بند ۲ سند حاضر).
  - `PRODUCT-DEVELOPMENT-ROADMAP.md` — بند ۵٫۴ به «بازیابی متنِ درج‌نشده» تغییر کرد و ردیفِ
    «نگهداری صدا» حذف شد؛ یادداشتِ doctor و ردیفِ «دوبار زدن = قابلیت موجود» اضافه شد.

  این سه سند **همان ادعاهایی را می‌گویند که این ممیزی می‌گوید** (چه چیزی هست و چه چیزی نیست)،
  پس جداکردنشان از بقیهٔ اسنادِ همین گروه توضیح را سخت‌تر می‌کند و فایده‌ای ندارد: در یک
  کامیتِ اسنادی می‌نشینند.

**استقلال ساخت: بررسی‌نشده** (برای اسناد بی‌معنا است؛ درستی ارجاع‌ها بررسی‌نشده).

---

### گروه J — قالب‌بندی
**فایل:** `processing/dictionary.rs` (+۱۶/−۳۰)

بررسی diff: فقط شکستن سطرها و جابه‌جایی آرایه‌های `default_corrections` (حذف خط خالی) و
`compile_rules`/`Correction{…}`/تست‌ها. **هیچ تغییر معنایی یا امضایی نیست.**

نکته: `Dictionary::with_defaults()` در آزمون‌های تازهٔ گروه A استفاده می‌شود، ولی آن API در
HEAD موجود است ⇒ **گروه J و گروه A بی‌ارتباط‌اند.**

---

## ۳. طرح نهایی کامیت‌ها

**قاعدهٔ یکدستی:** هر فایل — یا هر بخش مشخص از diff یک فایل — **دقیقاً در یک کامیت** می‌نشیند.
جایی که یک فایل به دو کامیت تقسیم می‌شود، محدودهٔ دقیقِ هر بخش ذکر شده و مجموعشان کلِ diff
آن فایل است. **هیچ فایل یا بخشی در دو کامیت تکرار نمی‌شود.**

| # | عنوان پیشنهادی | فایل‌ها / بخش‌های دقیق diff | پیش‌نیاز |
|---|---|---|---|
| **۱** | Harden the canary harness so a poisoned run cannot silently keep mutating | `docs/canary-harness.sh` — کل diff | — |
| **۲** | Log the orb press edge and the click that was actually handled | `voice-ptt/src/gui/orb.rs` — کل diff · `voice-ptt/src/gui/overlay.rs` — کل diff · `docs/execution/O1-handoff.md` — کلِ تنها hunk موجود (`@@ -312,5 +312,36 @@`، بخش «ابزار سنجش») | — |
| **۳** | Preserve hamza, and require positive stem evidence before joining a suffix | `voice-ptt/src/processing/normalizer.rs` — کل diff · `voice-ptt/tests/fixtures/text-baseline/cases.json` — کل diff · `voice-ptt/src/processing/mod.rs` — **فقط** `@@ -210,6 +212,8 @@` و `@@ -293,4 +297,105 @@` (آزمون‌های متنی) | — |
| **۴** | Reformat the correction table (no behaviour change) | `voice-ptt/src/processing/dictionary.rs` — کل diff | — |
| **۵** | Keep raw spacing at the seam, and disable backspace repair once the previous chunk ended in whitespace | `voice-ptt/src/processing/seam.rs` — **کل diff** (رفتار + قالب‌بندی با هم؛ ببین توضیح زیر جدول) | — |
| **۶** | Add the chunk and continuation boundary policy | `voice-ptt/src/processing/boundary.rs` — فایل جدید · `voice-ptt/src/processing/mod.rs` — **فقط** `@@ -2,10 +2,12 @@` (`pub mod boundary;` و `pub use boundary::{…}`) | **پیش از ۷** |
| **۷** | One loop that owns every effect: typed insertion results, session-scoped cancel, and the coordinator | `voice-ptt/src/output/injector.rs` · `voice-ptt/src/output/mod.rs` · `voice-ptt/src/output/target.rs` · `voice-ptt/src/state/utterance.rs` · `voice-ptt/src/state/session.rs` · `voice-ptt/src/state/coordinator.rs` · `voice-ptt/src/state/mod.rs` · `voice-ptt/src/state/machine.rs` · `voice-ptt/Cargo.toml` — همه با **کل diff** و در **یک کامیت** | ۳، ۵، ۶ |
| **۸** | Measure the coordinator stage with its own canary set | `docs/canary-anchor-check.py` (جدید) · `docs/mutation-check-coordinator.sh` (جدید) · `docs/mutation-check-session.sh` — کل diff | **پس از ۷** (ابزار باید بعد از کدِ هدف بنشیند) |
| **۹** | Record the coordinator stage, the normalizer package and the three accepted documents | `docs/INDEX.md` · `docs/execution/STATUS.md` · `docs/execution/T2-handoff.md` · `docs/execution/CONTRACTS.md` · `docs/AGENT-EXECUTION-PLAN.md` · `docs/PRODUCT-DEVELOPMENT-ROADMAP.md` · `docs/execution/COORDINATOR-BRIEF-2026-10-02.md` (جدید) · `docs/execution/COMMIT-GROUPING-AUDIT.md` (همین سند) — همه با **کل diff** | ۷، ۸ |

### چرا `seam.rs` در کامیت ۵ تقسیم نمی‌شود
نسخهٔ اول این سند پیشنهاد می‌کرد رفتار و قالب‌بندیِ این فایل از هم جدا شوند. برای یکدست‌شدن طرح
(نکتهٔ ۶) این پیشنهاد برداشته شد: هر دو بخش، خروجی `rustfmt` یک فایل‌اند، و جداسازی‌شان کامیتی
می‌سازد که فایل در آن قالب‌بندی‌نشده است — یعنی وضعیتی که خودِ [STATUS.md](STATUS.md) به‌عنوان
«قالب‌بندی فایل‌های بسته تمیز» گزارش می‌کند. یک فایل، یک کامیت.

### تنها تقسیمِ لازم: `processing/mod.rs`
این تنها فایلی است که **باید** به دو کامیت برود، و دلیلش ترتیب اجباری است:
آزمون‌های متنی به `normalizer.rs` گروه ۳ وابسته‌اند، ولی ثبت ماژول `boundary` باید پیش از
گروه ۷ بیاید. محدوده‌ها در جدول بالا دقیقاً مشخص شده‌اند و جمعشان کلِ diff فایل است.

### چرا گروه‌های D و E از گروه F جدا نمی‌شوند
`inject_text`/`inject_backspaces` و `Effect::DiscardSession`/`cancelled` هر دو در HEAD فقط توسط
`machine::emit` و `perform` مصرف می‌شوند، و هر دو مصرف‌کننده در بازآرایی گروه ۷ حذف می‌شوند.
کوچک‌کردن این کامیت‌ها یعنی نوشتن **آداپتور موقت یا کد تازه در حالت میانی** — که طبق دستور
انجام **نمی‌شود**. بنابراین «درج نوع‌دار + لغوِ نشست‌محور + هماهنگ‌کننده» یک بستهٔ واحد می‌ماند و
برای دیدن تاریخچهٔ ریزتر باید کار دیگری انجام شود، نه کامیت‌شکنیِ همین diff.

---

## ۴. موجودی نهایی در برابر طرح بالا (تطبیق یک‌به‌یک)

| کامیت | فایل‌ها | در موجودیِ ۱٫۲ هست؟ |
|---|---|---|
| ۱ | `canary-harness.sh` | ✓ |
| ۲ | `orb.rs`, `overlay.rs`, `O1-handoff.md` | ✓ ✓ ✓ |
| ۳ | `normalizer.rs`, `cases.json`, `processing/mod.rs` (بخش آزمون) | ✓ ✓ ✓ |
| ۴ | `dictionary.rs` | ✓ |
| ۵ | `seam.rs` | ✓ |
| ۶ | `boundary.rs` (جدید), `processing/mod.rs` (بخش ثبت) | ✓ ✓ |
| ۷ | `injector.rs`, `output/mod.rs`, `output/target.rs`, `utterance.rs`, `session.rs`, `coordinator.rs` (جدید), `state/mod.rs`, `machine.rs`, `Cargo.toml` | ✓ ✓ ✓ ✓ ✓ ✓ ✓ ✓ |
| ۸ | `canary-anchor-check.py` (جدید), `mutation-check-coordinator.sh` (جدید), `mutation-check-session.sh` | ✓ ✓ ✓ |
| ۹ | `INDEX.md`, `STATUS.md`, `T2-handoff.md`, `CONTRACTS.md`, `AGENT-EXECUTION-PLAN.md`, `PRODUCT-DEVELOPMENT-ROADMAP.md`, `COORDINATOR-BRIEF-2026-10-02.md` (جدید), `COMMIT-GROUPING-AUDIT.md` (جدید) | ✓ ✓ ✓ ✓ ✓ ✓ ✓ ✓ |

**۲۴ تغییریافته + ۶ جدید = ۳۰ ورودی، هر کدام دقیقاً یک جا.** هیچ فایلی جاافتاده یا تکراری نیست.
(شمارش با `git status --porcelain` بررسی شد: از ۳۱ سطرِ موجود، ۲۴ سطر `M` و ۶ سطر `??` به این
۳۰ ورودی تعلق دارند.) سطرِ `??` هفتم — `docs/execution/O1-MANUAL-ACCEPTANCE.md`، برنامهٔ آزمون
دستیِ `O1` — سندی **همزمان و تحویل جداگانه** است: نه در این ۳۰ شمرده می‌شود و نه در این طرح
می‌آید (بند ۱٫۲). گزارشِ محیط ساخت هم اگر افزوده شود، همین قاعده را دارد.

---

## ۵. وضعیت «استقلال ساخت» — بررسی‌نشده

| گروه / کامیت | استقلال ساخت | دلیل |
|---|---|---|
| A / ۳ نرمال‌ساز | **بررسی‌نشده** | فقط از روی متن diff و فراخوانی‌ها |
| B / ۵ درز | **بررسی‌نشده** | — |
| C / ۲ لاگ اورب | **بررسی‌نشده** | — |
| D گزارش درج | **بررسی‌نشده** | امضاهایش با گروه F شکسته می‌شود؛ طبق بند ۳ یکی می‌شوند |
| E لغو | **بررسی‌نشده** | همان دلیل |
| F / ۷ هماهنگ‌کننده | **بررسی‌نشده** | درخت ترکیبی تأییدشده (بند ۰٫۱) که **شاهدِ استقلال نیست**؛ بستهٔ تنها تأییدنشده |
| G / ۶ مرز | **بررسی‌نشده** | ترتیب اجباری روشن است، استقلال نه |
| H / ۸ کاناری | **بررسی‌نشده** | اسکریپت‌ها اجرا نشدند؛ `CAUGHT`/`SKIP` واقعی نامعلوم |
| I / ۹ اسناد | **بررسی‌نشده** | ارجاع‌های تاریخیِ `STATUS.md` عمداً تاریخی نگه داشته شدند |
| J / ۴ قالب‌بندی | **بررسی‌نشده** | — |

**چرا نتیجهٔ سبزِ درخت، استقلال کامیت‌ها را ثابت نمی‌کند:** درخت فعلی همهٔ گروه‌ها را با هم دارد.
یک کامیت وقتی مستقل است که **درختِ حاصل از فقط آن کامیت** ساخته شود و آزمون‌های خودش سبز
شوند. این گزارش هیچ چنین درختی نمی‌سازد و هیچ build/test اجرا نکرده است. ارقام «۴۸۴ آزمون سبز»
و «۲۶/۲۶ CAUGHT» — که بند ۰٫۱ شاهدِ معتبرشان را تأییدشده می‌داند — توصیف **همین درخت ترکیبی**
است و دربارهٔ کامیت ۱..۹ چیزی نمی‌گوید.

### ۵٫۱ نتیجهٔ بررسی نسخه‌های میانی — محتوا تأیید، ساخت و آزمون تأییدنشده

**«نسخهٔ میانی» یعنی چه:** درختی که از اعمالِ گروه‌ها به‌ترتیبِ طرحِ بخش ۳ روی خط مبنا
(`871b2b5`) به‌دست می‌آید؛ یعنی همان چیزی که پس از هر کامیتِ پیشنهادی روی شاخه می‌نشیند.

**کجا و چگونه اجرا شد:** درختی جدا به نام `.audit-intermediate/` که با `git archive HEAD`
(بدون `.git`) ساخته شد و گروه‌ها **یکی‌یکی و به‌ترتیب** روی آن اعمال شدند — با کپیِ کاملِ
فایل‌ها، و تنها استثنا `processing/mod.rs` که در گروه ۶ به دو خطِ CRLFِ مشخص تقسیم شد و پیش
از گروه ۷ به حالتِ گروه ۳ بازگردانده شد. نه آن درخت و نه آن بررسی چیزی در checkout اصلی، در
index یا در تاریخچه ننوشتند.

| گام بررسی | نتیجه | شاهد |
|---|---|---|
| بازاعمال ۲۷ فایلِ موجودیِ ویرایش دوم (۲۱ تغییریافته + ۶ جدید) به‌ترتیبِ گروه‌ها | **تأییدشده** | مقایسهٔ محتوا: ۲۷/۲۷ فایل با `sha256` یکسان · برگشت‌پذیریِ فایلِ تقسیم‌شده در هر دو جهت · ۲۷۱ فایل بیرون از طرح در برابر صادراتِ HEAD: ۰ تفاوت و ۰ غیبت |
| خواناییِ نحویِ درختِ تجمعی | **تأییدشده** | `cargo fmt --check` همهٔ فایل‌های کریت را پارس کرد؛ ۴۶ اختلاف قالب‌بندی، **همه بیرون از ۱۵ فایل `.rs`ِ تغییریافته** (هم‌پوشانی صفر) |
| ساخت (build) نسخه‌های میانی | **تأییدنشده** | تلاش شد و **پیش از کامپایل** با مانع محیطی متوقف شد (جدول بعد) |
| آزمون نسخه‌های میانی | **تأییدنشده** | هیچ باینریِ آزمونی اجرا نشد: ۰ سبز، ۰ ناموفق، ۰ نادیده‌گرفته‌شده |
| در نتیجه: استقلال ساختِ هر کامیت | **بررسی‌نشده** | جدولِ بخش ۵ بدون تغییر می‌ماند |

#### ۵٫۱٫۱ checkpointها — همه پیش از کامپایل متوقف شدند

| checkpoint | فرمان | کد خروج | مانع |
|---|---|---|---|
| پس از گروه ۲ | `cargo test --lib --offline` | ۱۰۱ | `whisper-rs-sys v0.15.0` |
| پس از گروه ۳ | `cargo test --lib --offline` | ۱۰۱ | همان |
| پس از گروه ۵ | `cargo test --lib --offline` | ۱۰۱ | همان |
| پس از گروه ۶ | `cargo test --lib --offline` | ۱۰۱ | همان |
| پس از گروه ۷ | `cargo test --lib --offline` | ۱۰۱ | همان |
| پس از گروه ۷ | `cargo clippy --offline --all-targets -- -D warnings` | ۱۰۱ | پر شدنِ دیسک (`os error 112`) |
| پس از گروه ۷ | `cargo test --offline` (یکپارچه) | ۱۰۱ | همان |

**ریشهٔ نخست محیطی است، نه نقصِ گروه‌بندی:** `whisper-rs` در `voice-ptt/Cargo.toml:31`
غیراختیاری است، پس هر هدفِ کریت به `whisper-rs-sys` نیاز دارد و build script آن `cmake` را
صدا می‌زند؛ `cmake` نه در PATH و نه در مسیرهای نصبِ متعارف هست، و `cl`، `clang` و `gcc` هم
غایب‌اند. یکسان بودنِ این خطا در هر پنج checkpoint نشانهٔ نبودِ زنجیرهٔ ابزار است، نه نبودِ یک
یالِ وابستگی. **ریشهٔ دوم مستقل است:** حجم به ۱۰۰٪ رسید (۴۷۶ گیگابایت مصرف، ۵۱۲ مگابایت
آزاد) و همین هم شکست clippy و آزمون یکپارچه و هم توقفِ کپیِ گروه‌های ۸ و ۹ را ساخت؛ گروه‌های
۸ و ۹ پس از آزادکردن `target` موقت دوباره و کامل اعمال شدند. چون `whisper-rs-sys` در گرافِ
وابستگیِ هر هدف است، clippy و آزمون یکپارچه حتی با دیسکِ آزاد هم به همان دیوارِ `cmake`
می‌خوردند؛ تکرار عمداً انجام نشد تا حجم دوباره پر نشود.

دو نکتهٔ باقی‌مانده از همین مرحله: گروه ۴ (`dictionary.rs`) از نظر قالب‌بندی تمیز است —
تنها ویژگی‌ای که طرح دربارهٔ آن ادعا کرده بود و اینجا آزموده شد؛ و نکتهٔ دیدِ `SessionId`
(بند ۳٫۲) هشداری نداد، ولی فقط چون کامپایل هرگز به کریت `voice-ptt` نرسید — یعنی
**آزموده‌نشده** است، نه ردشده. **هیچ اصلاحی در گروه‌بندی لازم نشد:** ترتیب و دامنهٔ گروه‌ها
سرِ جایشان ماند.

سه چیزی که این جدول‌ها **نمی‌دهند** و نباید از آن‌ها خوانده شود:

1. **تأییدِ محتوا، تأییدِ ساخت نیست.** اینکه درختِ بازسازیده همان محتوا را دارد، فقط می‌گوید
   چیزی در حین بازاعمال گم نشده است. اینکه آن درخت کامپایل می‌شود و آزمون‌هایش سبز می‌شوند،
   هنوز نامعلوم است. تنها بررسیِ بی‌نیاز از کامپایل که سیگنالِ واقعی داشت، `cargo fmt` بود که
   Rustِ نامعتبرِ نحوی را رد می‌کند؛ خطای نوعی را نمی‌گیرد.
2. **نتیجهٔ سبزِ درخت ترکیبی، شاهدِ استقلالِ کامیت‌ها نیست.** ارقام «۴۸۴ آزمون سبز» و
   «۲۶/۲۶ CAUGHT» که در [STATUS.md](STATUS.md) («کل کتابخانه») و [T2-handoff.md](T2-handoff.md)
   بند ۶ ثبت شده‌اند **تأییدشده‌اند** (بند ۰٫۱)، ولی توصیفِ درختی‌اند که **همهٔ** گروه‌ها را با
   هم دارد. استقلالِ کامیت یعنی ساخته‌شدنِ درختی که فقط **یک** گروه دارد؛ هیچ‌کدام از آن دو عدد
   چنین چیزی نمی‌گویند و نباید به‌عنوان شاهدِ آن نقل شوند.
3. **عدد ۲۷ به موجودیِ ویرایش دوم تعلق دارد.** سه سندِ تازهٔ بند ۱٫۲ (`CONTRACTS.md`،
   `AGENT-EXECUTION-PLAN.md`، `PRODUCT-DEVELOPMENT-ROADMAP.md`) پس از آن بازاعمال افزوده شدند و
   متنی‌اند؛ در آن ۲۷ فایل نبوده‌اند و چون ساخت و آزمون نشده‌اند، برای آن‌ها هم شاهدی از این
   دست وجود ندارد. موجودیِ امروز ۳۰ فایل است.

---

## ۶. یادداشت‌های باز برای تصمیم شما

1. **اتمیک بودن کامیت ۷.** بزرگ‌ترین تصمیم این طرح است و عمداً پذیرفته شده: کوچک‌کردن آن به
   آداپتور موقت نیاز دارد که طبق دستور نوشته نشده است.
2. **دید مؤثر `SessionId`** (بند ۳٫۲) یک **ملاحظهٔ طراحی** است، نه خطای ساخت: بررسی مستقلِ
   درخت ترکیبی و clippy هر دو موفق بوده‌اند. این سند هیچ تغییر کدی را پیشنهاد یا الزام نمی‌کند.
3. **ادعاهای Backspace** در `STATUS.md` و `T2-handoff.md` اصلاح شدند: مرز هرگز Backspace
   تولید نمی‌کند، و ترمیمِ حدسی در `seam.rs` فقط پس از فاصلهٔ انتهاییِ قطعهٔ پیشین غیرفعال
   می‌شود — نه به‌طور کلی.
4. **ارجاع‌های کهنه در بخش تاریخی `STATUS.md`** (`normalizer.rs:19`، `machine.rs:576`،
   «`inject_text` فقط `Result<usize>` می‌دهد») **عمداً دست‌نخورده** ماندند: آن بخش تاریخِ
   ۲۰۲۶-۱۰-۰۲ را ثبت می‌کند و عنوانش صریحاً «تاریخی» است. وضعیت فعلی در بخش «وضعیت فعلی
   (۲۰۲۶-۱۰-۴)» همان سند است. اگر ترجیح می‌دهید این سه ارجاع هم به‌روز شوند، آن‌ها به ادعای
   تاریخی تبدیل می‌شوند و باید با برچسب تاریخ نگه داشته شوند.
5. **جای‌گذاری ابزار کاناری.** `mutation-check-coordinator.sh` بعد از کامیت ۷ می‌نشیند، چون
   لنگرهایش درون کدِ همان کامیت‌اند. **نتیجهٔ اجرای آن روی نسخه‌های میانی بررسی‌نشده است**؛
   در این مرحله هیچ اسکریپتی اجرا نشد (بند ۵٫۱).
6. **هشدار CRLF** در خروجی `git diff` برای ۱۵ فایل (`injector.rs`، `machine.rs`، …). این تبدیل
   است و تغییر محتوایی نیست، ولی اگر commit با CRLF وارد شود، diffهای بعدی را نویزی می‌کند.
7. **نامِ آزمون `a_late_beat_is_followed_by_one_a_full_period_later` عمداً تغییر نکرد.** فقط
   تفسیر مستنداتی در `T2-handoff.md` بند ۴د دقیق شد تا قاعده را
   `std::cmp::max(at + period, now + period)` — قاعدهٔ خودِ برنامه — بنامد، نه بازتولید رفتار
   درونی Tokio.
8. **آنچه برای تأییدِ ساخت لازم است.** بند ۵٫۱ نشان می‌دهد مانع، محیط است نه ترتیبِ گروه‌ها:
   `cmake` و یک کامپایلرِ C/C++، به‌علاوهٔ فضای آزاد روی حجم. تا وقتی این‌ها نباشند، هر تلاشِ
   دوباره دقیقاً در همان نقطه متوقف می‌شود و جدولِ بند ۵ تغییری نمی‌کند. این سند هیچ نصب و هیچ
   تغییرِ سیستمی پیشنهاد یا انجام نمی‌دهد؛ این تصمیم با شماست.
