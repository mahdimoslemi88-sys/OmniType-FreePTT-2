# تحویل `O2` — مرحلهٔ نخست: سیاست خالص، بدون پنجره

مبنا: [O2-DESIGN.md](O2-DESIGN.md) · [AGENT-EXECUTION-PLAN](../AGENT-EXECUTION-PLAN.md) بند `O2` ·
[CONTRACTS.md](CONTRACTS.md) بند ۷

**یک جمله:** بستهٔ `O2` یک **سیاست خالص** است که تصمیم می‌گیرد «آیا درخواستِ جابه‌جایی بده؟»
و «نتیجهٔ جابه‌جاییِ قبلی چه شد؟» — و هیچ چیز دیگری. هیچ پنجره‌ای حرکت نکرد، هیچ
API ویندوزی صدا زده نشد، هیچ فایلِ تنظیماتی خوانده یا نوشته نشد.

---

## ۱. دامنهٔ این مرحله و آنچه عمداً بیرونِ آن است

| فایل | وضعیت |
|---|---|
| [voice-ptt/src/gui/orb_idle_policy.rs](../../voice-ptt/src/gui/orb_idle_policy.rs) | **تازه** — ۵۲۳ سطر، سیاست و انواع |
| [voice-ptt/tests/orb_idle_policy_test.rs](../../voice-ptt/tests/orb_idle_policy_test.rs) | **تازه** — ۷۷۹ سطر، ۲۶ آزمون با ساعتِ ساختگی |
| [O2-DESIGN.md](O2-DESIGN.md) | اصلاحِ نکاتِ همین مرحله (جزئیات در بند ۶) |
| **این فایل** | تازه |

**دست‌نخورده، عمداً:** `gui/mod.rs` · `gui/orb.rs` · `gui/overlay.rs` · `config/settings.rs` ·
`state/status.rs` · `state/coordinator.rs` · `Cargo.toml` · `Cargo.lock`.

ماژول در `gui/mod.rs` ثبت **نشده** است. این یک تصمیم است، نه فراموشی: برای اینکه این مرحله
هیچ سطحی از ریسک را باز نکند، سیاست با هیچ `use`ای به `gui`، `state` یا `config` وابسته نیست
و تنها از راه آزمونِ اختصاصی کامپایل می‌شود. تا وقتی مرحلهٔ ۳ (ثبت ماژول) اجرا نشده، هیچ
چیزی در برنامهٔ اصلی تغییر نکرده — یعنی باگِ احتمالیِ این بسته **نمی‌تواند** به کاربر برسد.

> **رخدادِ دامنه، ثبت‌شده:** در بازبینیِ پایانی یک سطرِ `pub mod orb_idle_policy;`
> در `gui/mod.rs` یافت شد که خلافِ دستورِ «ثبت ماژول را تغییر نده» بود. سطر حذف و فایل با
> `git checkout` به وضعیتِ `HEAD` بازگردانده شد. آزمون‌ها پس از آن بازاجرا شدند و `mod.rs`
> اکنون با `HEAD` یکسان است. این سطر دوباره ظاهر نشد (۴۵ ثانیه پایشِ md5).

---

## ۲. آنچه ساخته شد

### ۲٫۱ API عمومی

```rust
// واحد مختصات و مقصد
pub struct PhysicalPoint { pub x: i32, pub y: i32 }
pub enum  Corner { TopLeft, TopRight, BottomLeft, BottomRight }
pub enum  MonitorChoice { FollowOrb, Primary, Fixed(u8) }
pub struct MonitorRef { pub index: u8, pub is_primary: bool }
pub struct SpotTarget { pub point: PhysicalPoint, pub monitor: MonitorRef, pub geometry_version: u64 }
impl  SpotTarget { pub fn new(x: i32, y: i32, geometry_version: u64) -> Self }

// تنظیمات
pub struct IdleReturnSettings { enabled, timeout: Duration, retry_delay: Duration,
                                pinned, corner, monitor }   // + Default
pub const PROPOSED_TIMEOUT: Duration   = 60s;
pub const PROPOSED_RETRY_DELAY: Duration = 5s;

// آنچه فراخوان می‌دهد
pub struct Activity { recording, processing, inserting, dragging, pending_text, interacting }
pub enum  ReturnBlocker { Recording, Processing, Inserting, Dragging, PendingText }
impl Activity { fn blocker(&self) -> Option<ReturnBlocker>; fn is_busy(&self) -> bool; }

// درخواست و نتیجه — دو لحظهٔ جدا
pub struct MoveRequestId(pub u64);
pub enum  MoveKind { ToWaitingSpot, ToManualSpot }
pub struct MoveRequest { id, period, kind, to, context: Option<SpotTarget>, issued_at }
pub enum  MoveOutcome { Success, Failure }
pub enum  AckIgnored { NoOutstanding, NotTheOutstandingRequest }
pub enum  MoveAck { Accepted { id, outcome, consumed_this_period }, Ignored { id, reason } }

// تصمیم
pub enum NoReturnReason { Disabled, Pinned, Blocked(ReturnBlocker), Interacting,
                           TargetChanged, TargetUnavailable, AlreadyReturnedThisPeriod,
                           AwaitingResult, RetryBackoff, DeadlineNotReached, NoManualSpot }  // ۱۱ گزینه
pub enum Decision { No(NoReturnReason), Move(MoveRequest) }

// مشاهده
pub struct PolicySnapshot { period, returned_this_period, outstanding, deadline,
                             retry_after, manual_spot, last_result }

// سیاست
impl IdlePolicy {
    fn new(settings, now) -> Self;
    fn evaluate(&mut self, now, activity, target: Option<&SpotTarget>) -> Decision;
    fn report_move_result(&mut self, now, id, outcome) -> MoveAck;
    fn ask_return_to_manual(&mut self, now) -> Decision;
    fn note_manual_move(&mut self, to: PhysicalPoint);
    fn set_settings(&mut self, settings);  fn settings(&self) -> &IdleReturnSettings;
    fn manual_spot(&self) -> Option<PhysicalPoint>;  fn snapshot(&self) -> PolicySnapshot;
}
```

### ۲٫۲ ترتیب تصمیم — قراردادی که در آزمون قفل شده

| # | شرط | خروجی |
|---|---|---|
| ۱ | `!enabled` | `No(Disabled)` |
| ۲ | `pinned` | `No(Pinned)` |
| ۳ | `activity.blocker()` | `No(Blocked(b))` **و** دورهٔ تازه |
| ۴ | `interacting` | `No(Interacting)` **و** دورهٔ تازه |
| ۵ | `target` با `known_target` فرق دارد | `No(TargetChanged)`؛ درخواستِ باز باطل، ساعت **نمی‌شود** |
| ۶ | `target` غایب | `No(TargetUnavailable)`؛ درخواستِ باز باطل |
| ۷ | `returned_this_period` | `No(AlreadyReturnedThisPeriod)` |
| ۸ | درخواستِ در جریان | `No(AwaitingResult)` |
| ۹ | `now < retry_after` | `No(RetryBackoff)` |
| ۱۰ | `now < period_started + timeout` | `No(DeadlineNotReached)` |
| ۱۱ | همه‌چیز روشن | `Move(request)`، `outstanding = Some(request)` |

سطرهای ۱ و ۲ **مقدم بر درخواستِ در جریان‌اند**: اگر کاربر وسطِ انتظار برای نتیجه، قابلیت را
خاموش یا محل را ثابت کند، همان لحظه اثر می‌گذارد و دلیلش هم درست گزارش می‌شود — نه اینکه
زیرِ `AwaitingResult` پنهان شود.

---

## ۳. آزمون و نتیجه

```
$ cargo test --test orb_idle_policy_test
running 26 tests
…
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
CARGO_EXIT=0        # و صفر خطای کامپایل
```

| بررسی | نتیجه |
|---|---|
| `cargo test --test orb_idle_policy_test` | **۲۶ از ۲۶ سبز**، exit 0 |
| هشدارِ کامپایل | **صفر** (`grep -ci warning` = 0، پس از `touch` برای کامپایلِ تازه) |
| `rustfmt --edition 2021 --check` روی هر دو فایل | exit 0، خروجی خالی |
| `sleep` در آزمون‌ها | هیچ — `Instant::now()` فقط یک‌بار برای مبدأ |
| `cargo build --release`، اجرای زنده، نصب | **انجام نشد** (خارج از دامنهٔ درخواست) |

### ۳٫۱ نگاشت خواسته‌های پرامپت به آزمون‌های واقعی

| خواسته | آزمون |
|---|---|
| مرز مهلت | `one_millisecond_before_the_deadline_nothing_is_requested` · `exactly_at_the_deadline_the_move_is_requested` · `after_the_deadline_the_move_is_requested` |
| تعامل نزدیک مهلت | `interaction_one_millisecond_before_the_deadline_restarts_the_clock` |
| رفع مانع | `clearing_a_blocker_starts_a_full_new_timeout_not_the_remainder` · `every_blocker_reports_its_own_reason` (۵ مانع، هرکدام دلیلِ خودش) |
| دو tick متوالی | `no_second_request_while_a_result_is_outstanding` (۵ tick پیاپی، همه `AwaitingResult`) · `a_successful_result_consumes_the_period_and_two_later_ticks_do_nothing` |
| موفقیت/شکست حرکت | `a_failed_result_does_not_consume_the_period_and_holds_off_the_next_attempt` (۲۰ tick فقط `RetryBackoff`) · `a_failure_then_a_success_consumes_the_period_exactly_once` |
| تأیید دیررس | `a_late_ack_after_interaction_cannot_close_the_new_period` · `an_ack_for_a_different_id_is_ignored` |
| تغییر هندسه | `a_new_geometry_version_invalidates_the_outstanding_request` |
| تغییر مقصد | `a_different_corner_is_a_target_change` |
| مقصد نامعتبر | `an_unavailable_target_is_reported_without_a_coordinate` · `losing_the_target_invalidates_a_request_already_in_flight` |
| ثابت/خاموش هنگام انتظار نتیجه | `pinning_or_disabling_while_a_result_is_pending_takes_effect_at_once` · `disabled_pinned_and_deadline_are_three_different_reasons` |
| استقلال محل دستی | `going_to_the_corner_and_going_back_to_the_manual_spot_are_two_operations` · `a_manual_return_needs_no_deadline_and_outranks_the_auto_timeout` · `a_manual_return_while_a_result_is_pending_is_refused` |
| بی‌اعتنایی به گوشه/نمایشگر | `the_policy_is_indifferent_to_the_corner_and_the_monitor_preference` (۴ گوشه × ۳ نمایشگر = ۱۲ ترکیب) |
| عبور بی‌دخالت نقطه | `the_auto_return_passes_the_supplied_point_through_untouched` |
| تصویر وضعیت و پیش‌فرض‌ها | `the_snapshot_reports_the_deadline_and_the_pending_request` · `the_proposed_defaults_are_sixty_seconds_and_five` · `is_busy_covers_both_a_blocker_and_interaction` |

### ۳٫۲ سه نقصی که همین آزمون‌ها پیدا کردند

ثبت می‌شوند چون **آزمونی که چیزی پیدا نکند، نمی‌گوید کار کرده** — فقط می‌گوید اجرا شد:

1. **`target == None` به‌اشتباه `TargetChanged` می‌داد.** الان `TargetUnavailable` می‌دهد و
   درخواستِ باز را باطل می‌کند. قفل در `an_unavailable_target_is_reported_without_a_coordinate`.
2. **مقایسهٔ `SpotTarget` با `&SpotTarget` کامپایل نمی‌شد** (`E0614`) و در ابتدا به
   `PartialEq` روی مرجع تکیه کرده بود. با `target.copied()` رفع شد.
3. **شش هشدارِ dead-code** در اولین کامپایل. حذف نشدند؛ با آزمونِ واقعی پوشش داده شدند
   (`is_busy_covers_both_a_blocker_and_interaction`،
   `the_snapshot_reports_the_deadline_and_the_pending_request`،
   `the_proposed_defaults_are_sixty_seconds_and_five`، و…).

---

## ۴. تصمیم‌های طراحی که ارزشِ دانستن دارند

### ۴٫۱ بازگشتِ دستی هم بودجهٔ یک‌بارِ دوره را مصرف می‌کند

`ask_return_to_manual` از مهلت و از `returned_this_period` مستقل است، ولی اگر حرکتش **موفق**
شود، `report_move_result` همان `returned_this_period` را روشن می‌کند. یعنی پس از «برگشت به
محلِ من»، بازگشتِ خودکارِ همان دوره تکرار نمی‌شود.

**چرا این‌طور است:** اگر مصرف نمی‌کرد، چند ثانیه بعد از فرمانِ کاربر، بازگشتِ خودکار دوباره
اورب را به گوشه می‌برد و عملاً نظرِ کاربر را خنثی می‌کرد. این یک تصمیمِ عمدی است، نه یک
پیامد، و در `going_to_the_corner_and_going_back_to_the_manual_spot_are_two_operations`
مستقیماً قفل شده است. اگر روزی نظرِ محصول برعکس بود، فقط همین یک سطر عوض می‌شود.

### ۴٫۲ `TargetUnavailable` و `TargetChanged` دو دلیل‌اند، نه یکی

* `TargetUnavailable` = «الان هیچ مقصدی وجود ندارد» (نمایشگر رفته، ناحیهٔ کاری نامعتبر). سیاست
  **هیچ مختصه‌ای حدس نمی‌زند**. درخواستِ باز هم باطل می‌شود، چون دیگر مقصدی برای رسیدن نیست.
* `TargetChanged` = «مقصد یا نسخهٔ هندسه عوض شد». ساعت **عمداً از نو نمی‌شود**: کاربر منتظرِ
  همان مهلتی است که از قبل می‌دید، نه چند ده ثانیه عقب‌تر.

یکی‌کردنشان یعنی گزارشِ دروغ به پنل: «نمایشگرت رفت» برای کاربری که فقط نمایشگرش عوض شده،
پیامِ غلطی است.

### ۴٫۳ سیاست هیچ حسابی روی مختصات نمی‌کند

`corner_px`، `clamp`، شعاع، `keep_out_px` و حاشیهٔ لبه — همگی از سیاست بیرون رفتند و به
مالکِ هندسه رفتند. تنها کاری که سیاست با نقطه می‌کند این است که همان عددِ تحویل‌شده را
برگرداند. این با یک آزمونِ عجیب قفل است: نقطهٔ آزمون `-40, 7000` است — بیرون از هر فرمولی.
اگر جایی فرمولی روی مختصات اعمال می‌شد، آن آزمون خودش را لو می‌داد.

پیامدِ این تصمیم خوب است: ریسکِ «سیاست با فرمولِ `O1` از هم جدا بیفتد» **حذف شد**، چون دیگر
هیچ فرمولی برای جدا افتادن وجود ندارد.

### ۴٫۴ `Activity` شش واقعیتِ مستقل است، نه یک حالتِ ترکیبی

سیاست خودش از `AppState` جمع نمی‌کند، چون جمع‌کردن یعنی دوباره حدس‌زدن از روی حالت — و
[O2-DESIGN.md بند ۲٫۲](O2-DESIGN.md) نشان داده که این حدس از آب درمی‌آید. هر واقعیت را
فراخوان می‌سازد. ترتیبِ `blocker()` ثابت است تا گزارشِ دلیل پیش‌بینی‌پذیر بماند.

`interacting` **مانع نیست**، دلیلِ جداست، چون تعامل رویداد است نه کارِ در جریان: ساعت را از
نو می‌کند و درخواستِ باز را باطل می‌کند، ولی در `Blocked` گزافه نمی‌شود.

### ۴٫۵ شکستِ حرکت، فاصله می‌خرد

بدون فاصله، هر tick یک درخواستِ تازه تولید می‌شد و UI هر فریم `SetWindowPos` می‌زد.
`PROPOSED_RETRY_DELAY` (۵ ثانیه) جلویش را می‌گیرد و آزمونِ
`a_failed_result_does_not_consume_the_period_and_holds_off_the_next_attempt` بیست tickِ پیاپی
را نشان می‌دهد که هیچ‌کدام درخواستِ تازه نمی‌دهند، و بعد یک تلاشِ روشن با شناسهٔ تازه.

---

## ۵. درخواست‌های اتصال — برای هماهنگ‌کننده

هیچ‌کدام انجام نشده‌اند؛ همه خارج از مالکیتِ `O2` و خارج از دامنهٔ این مرحله‌اند.

| # | فایل | کار | دادهٔ لازم | چرا |
|---|---|---|---|---|
| ۱ | `gui/mod.rs` | `pub mod orb_idle_policy;` | — | تا حالا تنها آزمون این فایل را کامپایل می‌کند |
| ۲ | `config/settings.rs` | لایهٔ `serde` روی `IdleReturnSettings` + میدانِ `idle_return` در `GuiSettings` با `#[serde(default)]` | مقدارِ کاربر | تا کانفیگِ قدیمی نشکند. اعداد در فایل ثانیه و در Rust از نوع `Duration` |
| ۳ | `gui/overlay.rs` | ساختنِ سیاست یک‌بار در startup؛ ساختنِ `Activity` هر فریم | `AppStatus`، وضعیتِ پنل، وضعیتِ کشیدن | نگاشتِ ۲٫۱ به `AppState` کارِ فراخوان است، نه سیاست |
| ۴ | `gui/overlay.rs` | اجرای `MoveRequest`: **`home` را عوض کند، نه ذخیره** | `MoveRequest` | بازگشتِ خودکار هرگز نباید از مسیر `persist_orb_position` بگذرد |
| ۵ | `state/status.rs` | حملِ واقعیت‌های صریحِ فعالیت | `chunk_busy`، `partial`، `latched` (همه موجودند) | تا فراخوان مجبور نباشد حدس بزند |
| ۶ | `state/coordinator.rs` | تولید `pending_text` | `kept`، `SessionId` | تنها مانعی که امروز تولیدکننده ندارد |
| ۷ | هماهنگ‌کننده + `orb.rs` | ساختِ `SpotTarget`: نقطهٔ بریده + نمایشگر + **مهرِ هندسه** | `keep_out_px`، `work_area_at`، `clamp_center` | و یک **درزِ حداقلی** در `orb.rs`: `set_home_px` که `home` را تنظیم و `anim.snap_position` کند. `home` امروز فقط از راه کشیدن نوشته می‌شود، پس بدون این درز اصلاً راهی برای جابه‌جاییِ برنامه‌ای نیست |
| ۸ | `gui/overlay.rs` | دکمهٔ «بازگشت به محل دستی» | `ask_return_to_manual` | عملی جدا از رفتن به گوشه |

**سه شرطی که اتصال باید رعایت کند و در کد قفل شده‌اند:**

* **نتیجه را باید گزارش کنید.** `report_move_result` تنها راهی است که `returned_this_period` را
  روشن می‌کند. اگر فراخوان نتیجه را ندهد، سیاست برای همیشه `AwaitingResult` می‌ماند و قابلیت
  یک‌بار کار می‌کند. یعنی **گزارشِ نتیجه، اختیاری نیست**.
* **نتیجهٔ دیررس را نادیده نگیرید.** اگر کاربر در میانهٔ حرکت اورب را کشیده، تأییدِ شما
  دربارهٔ دورهٔ تازه است و **نباید** دورهٔ تازه را ببندد. `MoveAck::Ignored` را ببینید و رد شوید.
* **نسخهٔ هندسه را پایدار نگه دارید.** `geometry_version` باید فقط وقتی عوض شود که تعریفِ
  هندسهٔ `O1` عوض شده — نه هر فریم، و نه برای هر تغییرِ DPI مگر واقعاً مختصات عوض شود. عددی که
  بی‌دلیل عوض شود، بازگشت را بی‌پایان می‌کند.

---

## ۶. اصلاحاتِ همین مرحله در [O2-DESIGN.md](O2-DESIGN.md)

سند طراحی در پایان به‌روز شد تا با کدی که واقعاً نوشته شده بخواند، نه با نیتی که داشت:

| بند | اصلاح |
|---|---|
| ۳٫۱ | `Interacting` از فهرست موانع برداشته شد و **رویداد** شد؛ `Inserting` و `PendingText` جای آن نشستند |
| ۳٫۲ | فهرست انواع **از روی خودِ فایل** بازنویسی شد: `IdleBlocker` → `ReturnBlocker` (۵ گزینه)، `SpotGeometry`/`WorkAreaPx` حذف و `SpotTarget` جایگزین، `timeout_secs: u32` → `timeout: Duration` + `retry_delay`، توابعِ آزاد → متدهای `IdlePolicy`، انواعِ درخواست/نتیجه افزوده شد |
| ۳٫۳ | جدولِ ترتیبِ تصمیم، ۱۱ سطری و با شماره‌هایی که همان ترتیبِ اجرا در کدند |
| ۳٫۴ | بند تازه: چرا `corner_px` حذف شد و چرا تولیدِ مقصد کارِ مرکزی است |
| ۴ | هر هفت قاعده با ارجاع به سطرِ دقیقِ همان جدول |
| ۶٫۱ | یادداشت صریح که `serde` هنوز نوشته نشده و تصمیمِ محلِ آن قطعی نیست |
| ۶٫۳ | ستونِ داده با نام‌های واقعیِ کد هم‌خوان شد |
| ۷ | عنوان از «جدول» به «آزمون‌ها» تغییر کرد، چون جدولِ `T1..T22` **حذف شد** و جایش نگاشتِ ۲۶ آزمونِ واقعی نشست؛ سه تصمیمِ بند ۴ هم در ۷٫۰ ثبت شد |
| ۸ | مرحله‌های ۱ و ۲ به «انجام‌شده» و ۲۶ آزمون به‌روز شد |
| ۱۰ | معیار ۲ از «۲۲ ردیف» به «۲۶ آزمون» تغییر کرد؛ معیار ۶ دوباره نوشته شد (آزمونِ برابری حذف شده چون فرمولی نمانده) |
| — | شماره‌گذاریِ تکراریِ `۳٫۱`/`۳٫۲` به `۳٫۳`/`۳٫۴` اصلاح شد |

---

## ۷. آنچه آزموده‌نشده می‌ماند

| مورد | چرا آزموده نشده | چه‌وقت |
|---|---|---|
| **حرکتِ واقعیِ پنجره** | نیازمند ماوس، اجرای زنده و اتصال — که هر دو خارج از دامنهٔ این مرحله‌اند | مرحلهٔ ۸، فقط پس از بسته‌شدن `O1` ([شرطِ بند ۱۲](O1-MANUAL-ACCEPTANCE.md)) |
| **درستیِ `SpotTarget` روی نمایشگرِ واقعی** | سیاست آن را نمی‌سازد؛ سازنده‌اش هنوز نوشته نشده | مرحلهٔ ۷ |
| **`pending_text` و `inserting` در حالتِ واقعی** | تولیدکننده‌شان هنوز وجود ندارند؛ `inserting` در کد امروز متناظری ندارد | مرحلهٔ ۵ و ۷ |
| **مقصدِ نامعتبر در دنیای واقعی** (خاموش شدن نمایشگر) | نیازمند دو نمایشگر و دستکاریِ تنظیمات | مرحلهٔ ۸ |
| **عددِ ۶۰ ثانیه و ۵ ثانیه** | پیشنهادند، نه تصمیمِ ثبت‌شدهٔ کاربر | هر وقت کاربر تصمیم بگیرد |
| **بیلدِ release و رفتارِ باینری** | خارج از دامنهٔ درخواست | جداگانه |

**ادعای درست:** این سیاست تصمیم‌هایش درست است و آزمون‌ها هم نشان می‌دهند. **ادعای نادرست:**
اینکه بازگشت روی صفحهٔ واقعی بی‌هنگام و بی‌نقص است — تا آن روز، «آزموده‌نشده» است.

---

## ۸. وضعیت کار

* `O2` مرحلهٔ ۱ و ۲: **انجام‌شده** — سیاست و آزمون‌ها، ۲۶/۲۶ سبز، صفر هشدار.
* `O2` مرحلهٔ ۳ تا ۷: **شروع نشده** — همه به اتصال نیاز دارند و در بند ۵ فهرست شده‌اند.
* `O2` مرحلهٔ ۸: **مسدود** تا بسته‌شدن `O1`.
* هیچ کامیت، push، بیلدِ release یا اجرای زنده‌ای انجام نشده است.
