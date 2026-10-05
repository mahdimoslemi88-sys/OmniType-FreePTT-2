# طراحی بستهٔ `O2` — سیاست بازگشت خودکار اورب به محل انتظار

تاریخ: ۲۰۲۶-۱۰-۰۴ · وضعیت: **پیش‌نویس طراحی، پیش از شروع پیاده‌سازی**
مبنا: [AGENT-EXECUTION-PLAN.md](../AGENT-EXECUTION-PLAN.md) بند `O2` · [CONTRACTS.md](CONTRACTS.md) بند ۷ ·
[O1-handoff.md](O1-handoff.md) · [O1-MANUAL-ACCEPTANCE.md](O1-MANUAL-ACCEPTANCE.md)

---

## ۰. دامنه، مرزها و آنچه این سند **نیست**

این سند فقط **طراحی** است. هیچ کدی نوشته نشد، هیچ ساخت یا آزمونی اجرا نشد، هیچ پنجره‌ای
حرکت نکرد و هیچ فایل دیگری تغییر نکرد. تنها فایلی که این دور ساخته شد همین سند است.

سه مرز که این طراحی عمداً از آن‌ها عبور نمی‌کند:

1. **هندسه مالکیت `O1` است.** `O2` سیاست است، نه هندسه. هیچ ثابتِ شعاع، حاشیه یا بُعدی
   در این سند تعریف نمی‌شود؛ هر عددِ هندسی از بیرون به سیاست داده می‌شود. (بند ۶)
2. **اتصال حرکتِ واقعی جدا می‌ماند.** سیاست فقط *درخواست* بازگشت تولید می‌کند. هیچ فراخوانی
   `SetWindowPos`، هیچ دست‌زدن به `Orb::home` و هیچ تغییری در `orb.rs` در دامنهٔ `O2` نیست.
3. **پروندهٔ باز `O1` دست‌نخورده می‌ماند.** هیچ علت تازه‌ای برای هاله یا بریدگی پیکسل حدس
   زده نمی‌شود. چهار فرضیهٔ `H1..H4` در [O1-MANUAL-ACCEPTANCE](O1-MANUAL-ACCEPTANCE.md)
   بند ۱۴ **بی‌اثبات** می‌مانند و `O2` چیزی به آن‌ها اضافه یا از آن‌ها کم نمی‌کند.

**دربارهٔ عدد ۶۰ ثانیه:** در [پرامپت `O2`](../../docs/AGENT-EXECUTION-PLAN.md) با عبارت
«مقدار پیشنهادی ۶۰ ثانیه است» آمده. در این سند هم یک **پیشنهاد** است با استدلال زیر، نه
تصمیم قطعیِ ثبت‌شدهٔ کاربر. اگر تصمیم دیگری گرفته شد، تنها ثابتِ
`PROPOSED_TIMEOUT` عوض می‌شود و هیچ‌چیز دیگری از این طراحی تکان نمی‌خورد.

---

## ۱. وضعیت موجود — کد-مبنا، با ارجاع

| کمیت | آنچه **هست** | شاهد |
|---|---|---|
| حالت‌های قابل‌مشاهده | `AppState { Idle, Recording, Processing, Typing, Error(String) }` | [status.rs:12-18](../../voice-ptt/src/state/status.rs#L12) |
| بستهٔ وضعیت | `AppStatus { state, last_text, vad_engine, partial, latched, chunk_busy }` | [status.rs:22-41](../../voice-ptt/src/state/status.rs#L22) |
| نگاشت به شکل اورب | `impl From<&AppState> for OrbMode` — `Typing → Complete` | [overlay.rs:60-69](../../voice-ptt/src/gui/overlay.rs#L60) |
| حالت‌های اورب | `OrbMode { Idle, Recording, Processing, Complete, Error }` | [orb_animation.rs](../../voice-ptt/src/gui/orb_animation.rs) |
| کشیدن فقط در دو حالت | `hoverable() == Idle \| Error` — و همین، دروازهٔ `drag_started` است | [orb_animation.rs:71-73](../../voice-ptt/src/gui/orb_animation.rs#L71) |
| مرکزِ ساکن | `Orb.home: Pos2` با یادداشت «Idle resting center, **physical screen pixels**» | [orb.rs:134-135](../../voice-ptt/src/gui/orb.rs#L134) |
| راه‌های نوشتن روی `home` | فقط کشیدن ([orb.rs:391](../../voice-ptt/src/gui/orb.rs#L391)) و `clamp_home` ([orb.rs:450](../../voice-ptt/src/gui/orb.rs#L450)) | همان دو |
| خواندن مختصات | `Orb::home_position() -> (i32, i32)` | [orb.rs:207-209](../../voice-ptt/src/gui/orb.rs#L207) |
| ماندن روی مرکزِ ذخیره‌شده | هر فریم و بیرون از کشیدن، `anim.set_target_position(self.home)` | [orb.rs:225-227](../../voice-ptt/src/gui/orb.rs#L225) |
| ذخیرهٔ محل دستی | `moved_to` در پایان کشیدن → `persist_orb_position` → `gui.orb_position_x/y` روی دیسک | [orb.rs:396](../../voice-ptt/src/gui/orb.rs#L396) → [overlay.rs:335-349](../../voice-ptt/src/gui/overlay.rs#L335) |
| بازخوانی محل | `s.gui.orb_position_x.zip(s.gui.orb_position_y)` → `Orb::new` | [overlay.rs:282-286](../../voice-ptt/src/gui/overlay.rs#L282) |
| واحد مختصات در تنظیمات | `Option<i32>` با یادداشت «Orb center, physical screen pixels» | [settings.rs:386-390](../../voice-ptt/src/config/settings.rs#L386) |
| پنجره | یک بار در بزرگ‌ترین بوم ساخته می‌شود و **هرگز** resize نمی‌شود؛ فقط جابه‌جا می‌شود | [orb.rs:294-303](../../voice-ptt/src/gui/orb.rs#L294) |
| `ppp` | `GetDpiForWindow` → `ppp = dpi / 96.0` | [window_shape.rs:187-191](../../voice-ptt/src/gui/window_shape.rs#L187) |
| محدودهٔ قابل‌استفاده | `MonitorFromPoint` + `GetMonitorInfoW().rcWork` — **ناحیهٔ کاری**، نه کلِ نمایشگر | [orb.rs:1015-1043](../../voice-ptt/src/gui/orb.rs#L1015) |
| فاصله از لبه | `EDGE_MARGIN_PT = 6.0` نقطه، و `keep_out_px(scale, ppp) = (painted_reach_pt(scale, true) + 6.0) * ppp` | [orb.rs:44](../../voice-ptt/src/gui/orb.rs#L44)، [orb.rs:1105-1112](../../voice-ptt/src/gui/orb.rs#L1105) |
| بریدن به ناحیهٔ کاری | `win::clamp_center`، با نگهبان `if lo <= hi` برای ناحیهٔ تهی | [orb.rs:987-1009](../../voice-ptt/src/gui/orb.rs#L987) |
| مرکزِ پیش‌فرض | `primary_screen_center()` از `SM_CXSCREEN`/`SM_CYSCREEN` — **کلِ صفحهٔ اصلی**، نه ناحیهٔ کاری | [orb.rs:973-976](../../voice-ptt/src/gui/orb.rs#L973) |
| ساعت قابل‌تزریقِ موجود | `trait Clock { now, sleep_until }` + `SystemClock` + `ManualClock` در آزمون‌ها | [coordinator.rs:178-191](../../voice-ptt/src/state/coordinator.rs#L178)، [coordinator.rs:2135-2206](../../voice-ptt/src/state/coordinator.rs#L2135) |
| متنِ منتظرِ اقدام | `Coordinator.kept` با `KeptRecord { session, chunk, seq, planned_text, destination, outcome, … }` | [coordinator.rs:471](../../voice-ptt/src/state/coordinator.rs#L471)، [coordinator.rs:329-343](../../voice-ptt/src/state/coordinator.rs#L329) |
| فازهای جلسه | `SessionPhase { Recording, AwaitingResult, Completed, Cancelled }` و `SessionKind { PushToTalk, HandsFree }` | [session.rs:214+](../../voice-ptt/src/state/session.rs#L214)، [session.rs:202-205](../../voice-ptt/src/state/session.rs#L202) |

### ۱٫۱ سه شکافی که طراحی بر آن‌ها بنا شده

1. **درزِ اتصال وجود ندارد.** `home` خصوصی است و هیچ setter عمومی ندارد؛ تنها راهِ جابه‌جایی
   برنامه‌ریزی‌شده، کشیدنِ دستِ کاربر است ([orb.rs:391](../../voice-ptt/src/gui/orb.rs#L391)).
   پس `O2` به‌تنهایی **نمی‌تواند** اورب را برگرداند؛ یک درزِ حداقلی در `orb.rs` لازم است که
   کارِ هماهنگ‌کننده است، نه کار `O2` (بند ۹).
2. **`Idle` ظاهری کافی نیست.** سه واقعیت مستقل وجود دارند که هیچ‌کدام در `AppState` دیده
   نمی‌شوند: `chunk_busy` (قطعهٔ میانی در پرواز، با این قاعده که **هرگز** `state` را عوض
   نمی‌کند، [status.rs:32-40](../../voice-ptt/src/state/status.rs#L32))، `partial` (فقط در
   `Processing` پر است) و `latched` (ضبطِ بدون‌دست). یک اوربِ سبز در حالی که قطعه‌ای در پرواز
   است، **بی‌کار نیست**.
3. **محلِ دستی و محلِ انتظار امروز یکی‌اند.** `orb_position_x/y` تنها مختصاتِ ماندگار است و
   هر `moved_to` آن را بازنویسی می‌کند. اگر بازگشتِ خودکار از همین مسیر برود، پس از یک
   بی‌کاریِ عادی جای دستیِ کاربر برای همیشه از بین می‌رود (بند ۵).

> یک نکتهٔ کناری که به دام می‌اندازد: `OverlayApp.last_hover_time: Option<Instant>`
> ([overlay.rs:215](../../voice-ptt/src/gui/overlay.rs#L215)) در کد **مرده** است — تنها دو
> حضور دارد، تعریف و مقداردهی اولیه، و هرگز خوانده نمی‌شود. این بسته آن را به کار نمی‌گیرد و
> نباید بهانهٔ استفاده از آن شود.

---

## ۲. تعریف دقیق بی‌کاری

**قاعدهٔ بنیادی:** اورب بی‌کار است اگر و تنها اگر **همهٔ** واقعیت‌های زیر روشن باشند. هیچ‌کدام
از «ظاهر `Idle` بودن» به‌تنهایی ملاک نیست.

### ۲٫۱ واقعیت‌های مسدودکننده

| # | مانع | چه چیزی آن را روشن/تاریک می‌کند | منبع امروز | اگر امروز تولید نمی‌شود |
|---|---|---|---|---|
| ۱ | `Recording` | `state == Recording` **یا** `latched == true` | [status.rs](../../voice-ptt/src/state/status.rs) | موجود |
| ۲ | `Processing` | `state == Processing` **یا** `partial.is_some()` **یا** `chunk_busy == true` | [status.rs:28](../../voice-ptt/src/state/status.rs#L28)، [status.rs:40](../../voice-ptt/src/state/status.rs#L40) | موجود — `chunk_busy` عمداً `state` را عوض نمی‌کند |
| ۳ | `Dragging` | اورب در کشیدن است، یا در همان فریم `moved_to` داده | [orb.rs:368-398](../../voice-ptt/src/gui/orb.rs#L368) | موجود، ولی داخل `orb.rs` است؛ باید به سیاست برسد |
| ۴ | `Inserting` | درجِ متن در جریان است؛ نتیجهٔ نوع‌دار و درجِ ناقص، هر دو | `kept` و `InjectOutcome` در [coordinator.rs:329-343](../../voice-ptt/src/state/coordinator.rs#L329) | موجود، ولی باید به سیاست برسد |
| ۵ | `PendingDraft` → در کد `PendingText` | متنی وجود دارد که **کاربر باید درباره‌اش تصمیم بگیرد** | **هیچ** — `Draft`/`DraftStatus` در کد نیست ([CONTRACTS بند ۶](CONTRACTS.md)) | رزرو شده؛ تولیدکننده‌اش کارِ `V1` است |

> **اصلاحِ مرحلهٔ نخست:** مانعِ `Interacting` که در نسخهٔ نخست این جدول بود **به رویداد تبدیل شد**، نه اینکه حذف شود. تعامل، خودش ساعت را از نو می‌کند و هر درخواستِ باز را باطل می‌کند (بند ۳٫۱، سطرهای ۳ و ۴)، ولی دلیلِ جداگانه‌ای برای «اقدامی نشد» تولید نمی‌کند؛ دلیلش `NoReturnReason::Interacting` است. در مقابل `Inserting` مانعِ واقعی است و در کد امروز چیزی متناظر با آن ندارد.

| وضعیت ظاهری | واقعیت پنهان | چرا سیاست را فریب می‌دهد |
|---|---|---|
| `Idle` | یک `KeptRecord` از جلسهٔ **قدیمی‌تر** باقی مانده | رکورد به `Option<SessionId>` گره خورده و تا درجِ لغو نشود زنده می‌ماند؛ جلسهٔ فعلی هیچ ربطی به آن ندارد |
| `Recording` | `chunk_busy == true` | قاعدهٔ عمدی کد: حالتِ دیدنی عوض نمی‌شود، پس اورب «ضبط» نشان می‌دهد در حالی که رونویسی و درج هنوز در راه است |
| `Idle` | `AwaitingResult` برای جلسه‌ای که کلیدش رها شده | توقف میکروفون پایان جلسه نیست؛ نتیجهٔ آخرین قطعه **بعد از** رها کردن کلید می‌رسد ([session.rs:208-212](../../voice-ptt/src/state/session.rs#L208)) |

**قاعدهٔ مالکیت:** تشخیص «متنِ منتظرِ اقدام» کارِ هماهنگ‌کننده است، چون اوست که
`SessionId`، `ChunkId` و `InjectOutcome` را می‌بیند. `O2` حقیقت را **می‌گیرد**، نمی‌سازد.

### ۲٫۳ چه چیزی ساعت را از نو آغاز می‌کند

| رویداد | اثر |
|---|---|
| کلیک روی اورب | `Interacting` روشن، سپس خاموش ⇒ دورهٔ تازه از **لحظهٔ خاموش‌شدن** |
| کشیدن و رهاکردن | `Dragging` روشن و خاموش ⇒ دورهٔ تازه؛ ضمناً **دورهٔ قبلی بسته می‌شود** حتی اگر بازگشتی نشده باشد |
| ورود/خروج نشانگر از روی اورب | تعامل است: دوره از نو |
| باز/بسته شدن داشبورد یا پنل | تعامل است: دوره از نو |
| شروع/پایان ضبط، آغاز/پایان پردازش | دوره از نو از لحظهٔ **پاک شدن آخرین مانع** |
| آمدن/رفتن متنِ منتظرِ اقدام | دوره از نو از لحظهٔ پاک شدن |
| صرفِ گذر زمان بدون هیچ رویدادی | **ساعت را از نو آغاز نمی‌کند** — این همان چیزی است که مهلت را می‌شمارد |

**قاعدهٔ طلایی:** ساعت از «آخرین رویداد» می‌آید، نه از «آخرین بازگشت». بازگشت خودش
رویداد نیست؛ اگر بود، هر بازگشت دورهٔ تازه می‌ساخت و بعد از مدتی دوباره بازمی‌گشت.

---

## ۳. قرارداد سیاست — ورودی، خروجی، انواع

### ۳٫۱ اصل حاکم

سیاست یک **تابع خالص با وضعیت** است:

* **نه** پنجره را جابه‌جا می‌کند، **نه** `SetWindowPos` صدا می‌زند، **نه** فایل می‌خواند،
  **نه** تنظیمات را ذخیره می‌کند، **نه** ساعت واقعی را می‌خواند.
* زمان، موقعیت، محدوده و فعالیت همگی **ورودیِ داده**‌اند. زمان از بیرون تزریق می‌شود، پس
  آزمون با ساعت ساختگی بدون `sleep` کار می‌کند — همان رویکردی که `ManualClock` در
  [coordinator.rs:2135-2206](../../voice-ptt/src/state/coordinator.rs#L2135) جا انداخته.
* تنها کاری که سیاست می‌کند: تصمیم می‌گیرد و یک **مختصات** بیرون می‌دهد. اجرای آن مختصات،
  کارِ فراخوان است.

### ۳٫۲ انواع — همان چیزی که در مرحلهٔ نخست پیاده شد

> فهرست زیر از روی [orb_idle_policy.rs](../../voice-ptt/src/gui/orb_idle_policy.rs) نوشته شده،
> نه از روی نیت. هر چیزی که در آن فایل نیست، اینجا هم نیست؛ و آنچه اینجا هست، در آن فایل هست.

```rust
// ---------- واحد مختصات: همیشه پیکسل فیزیکی، هرگز نقطه ----------

/// مختصات فیزیکی روی دسکتاپ. مبنای ذخیرهٔ فعلی هم همین است
/// (`GuiSettings::orb_position_x/y`، [settings.rs:386](../../voice-ptt/src/config/settings.rs#L386)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicalPoint { pub x: i32, pub y: i32 }
```

> **چرا `WorkAreaPx` اینجا نیست:** نسخهٔ نخست این سند ناحیهٔ کاری را به سیاست می‌داد. حالا
> ناحیهٔ کاری را **نمی‌بیند**، چون فقط نقطهٔ بریده‌شده را می‌گیرد. نگهبانِ
> `right <= left || bottom <= top` که `win::clamp_center` دارد
> ([orb.rs:1034](../../voice-ptt/src/gui/orb.rs#L1034))، در همان‌جایی می‌ماند که امروز است —
> مالکِ هندسه — و دست‌نخورده.

```rust
// ---------- تنظیمات (انتقالی؛ ظاهر و رفتار از هم جدا) ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Corner { TopLeft, TopRight, BottomLeft, BottomRight }

/// «خودکار» یعنی: نمایشگری که همین حالا اورب روی آن است.
/// «اصلی» یعنی `SM_CXSCREEN` — همان تعریفی که کد امروز دارد
/// ([orb.rs:973](../../voice-ptt/src/gui/orb.rs#L973)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitorChoice { FollowOrb, Primary, Fixed(u8) }

/// مهلت و فاصلهٔ تلاشِ دوباره `Duration` هستند نه عددِ ثانیه: سیاست زمان را
/// **مقایسه** می‌کند و نباید برای هر مقایسه به واحد تبدیل کند.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleReturnSettings {
    pub enabled: bool,
    pub timeout: Duration,
    /// فاصله تا تلاشِ دوباره پس از شکستِ حرکت؛ بدون آن هر tick یک درخواست تازه می‌شد.
    pub retry_delay: Duration,
    /// ثابت‌کردن محل، بازگشت خودکار را **مسدود** می‌کند (بند ۵). دستورِ صریحِ کاربر
    /// («برگشت به محل دستی») از این مستثنا است.
    pub pinned: bool,
    /// خوانده نمی‌شود؛ پنل و فراخوان از آن استفاده می‌کنند تا مقصد را بسازند.
    pub corner: Corner,
    /// خوانده نمی‌شود؛ انتخابِ نمایشگر کارِ فراخوان است.
    pub monitor: MonitorChoice,
}

// ---------- آنچه فراخوان دربارهٔ کارِ در جریان می‌داند ----------

/// شش واقعیتِ مستقل، نه یک حالتِ ترکیبی. `O2` خودش جمع نمی‌کند؛
/// چون جمع‌کردنش یعنی دوباره حدس‌زدن از `AppState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub recording: bool,    // AppState::Recording || latched
    pub processing: bool,   // AppState::Processing || partial.is_some() || chunk_busy
    pub inserting: bool,    // درجِ متن در جریان (نتیجهٔ نوع‌دار و ناقص، هر دو)
    pub dragging: bool,     // کشیدنِ جاری، یا moved_to در همین فریم
    pub pending_text: bool, // متنِ منتظرِ تصمیم کاربر — امروز تولیدکننده ندارد
    pub interacting: bool,  // hover / دکمهٔ پایین / داشبورد یا پنلِ باز
}

/// مانعِ بازگشت. هرکدام دلیلِ خودش را دارد و به چیز دیگری گزافه نمی‌شود.
/// `interacting` اینجا نیست: خودش دلیلِ جدا دارد (`NoReturnReason::Interacting`)،
/// و `pinned` هم مانع نیست، تنظیم است.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnBlocker { Recording, Processing, Inserting, Dragging, PendingText }

impl Activity {
    /// نخستین مانعِ فعال، با ترتیبِ ثابت تا گزارشِ دلیل پیش‌بینی‌پذیر بماند.
    pub fn blocker(&self) -> Option<ReturnBlocker>;

    pub fn is_busy(&self) -> bool;
}

// ---------- ورودیِ تصمیم ----------

/// مقصدی که **مالکِ هندسه** ساخته و بریده است (بند ۳٫۴).
/// `keep_out_px` — همان `(painted_reach_pt(scale, true) + EDGE_MARGIN_PT) * ppp`
/// ([orb.rs:1105](../../voice-ptt/src/gui/orb.rs#L1105)) — و فرمولِ بریدن، هر دو کارِ
/// فراخوان‌اند و سیاست آن‌ها را نمی‌بیند.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpotTarget {
    /// از پیش داخلِ ناحیهٔ کاری و با فاصلهٔ لبهٔ لازم — سیاست دوباره حساب نمی‌کند.
    pub point: PhysicalPoint,
    pub monitor: MonitorRef,
    /// مهرِ روی مختصات: با هر تغییرِ تعریفِ هندسهٔ `O1` عوض می‌شود.
    pub geometry_version: u64,
}

impl SpotTarget {
    pub fn new(x: i32, y: i32, geometry_version: u64) -> Self;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MonitorRef { pub index: u8, pub is_primary: bool }

// ---------- خروجی ----------

pub enum Decision {
    /// هیچ اقدامی نکن — و بگو چرا، تا پنل بتواند توضیح بدهد.
    No(NoReturnReason),
    /// درخواستِ جابه‌جایی با مختصاتِ مشخص. این یک دستورِ ذخیره نیست.
    Move(MoveRequest),
}

/// دلیلِ «اقدامی نشد». خاموش، ثابت، هر مانع، نرسیدنِ مهلت، در انتظارِ نتیجه و فاصلهٔ
/// تلاشِ دوباره — هرکدام دلیلِ خودش، و هیچ‌کدام به دیگری گزافه نمی‌شود.
pub enum NoReturnReason {
    Disabled,
    Pinned,
    Blocked(ReturnBlocker),
    Interacting,
    TargetChanged,
    TargetUnavailable,
    AlreadyReturnedThisPeriod,
    AwaitingResult,
    RetryBackoff,
    DeadlineNotReached,
    NoManualSpot,
}


// ---------- درخواست و نتیجه: دو لحظهٔ جدا، نه یکی ----------

/// شناسهٔ یکتا و یک‌بارمصرف. نتیجهٔ دیررسِ یک شناسهٔ کهنه با شناسهٔ درخواستِ جاری
/// برابر نیست و نادیده گرفته می‌شود.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MoveRequestId(pub u64);

/// دو عملِ جدا. «رفتن به گوشه» هرگز به «برگشتن به جایی که کاربر گذاشته» تبدیل نمی‌شود.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind { ToWaitingSpot, ToManualSpot }

/// یک درخواستِ جابه‌جایی. **این یک دستورِ ذخیره نیست** — نوعِ داده هیچ حالتی برای نوشتنِ
/// محلِ دستی ندارد، پس سیاست نمی‌تواند چنین دستوری تولید کند.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveRequest {
    pub id: MoveRequestId,
    pub period: u64,          // دورهٔ بی‌کاری‌ای که در آن صادر شد
    pub kind: MoveKind,
    pub to: PhysicalPoint,
    /// مهرِ هندسه‌ای که مختصات با آن ساخته شده.
    pub context: Option<SpotTarget>,
    pub issued_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveOutcome { Success, Failure }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckIgnored {
    /// درخواستِ بازی در جریان نیست — یا هرگز نبوده، یا با تعامل/تغییر مقصد باطل شده.
    NoOutstanding,
    /// درخواستی در جریان هست، ولی این تأیید مربوط به آن نیست.
    NotTheOutstandingRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveAck {
    Accepted { id: MoveRequestId, outcome: MoveOutcome,
               /// فقط **تأییدِ موفق** آن `true` است.
               consumed_this_period: bool },
    Ignored { id: MoveRequestId, reason: AckIgnored },
}

/// وضعیتِ داخلیِ سیاست. کوچک است و عمداً فقط همین‌ها را نگه می‌دارد: دوره، ساعتِ دوره،
/// بودجهٔ یک‌بار، درخواستِ در جریان، آخرین نتیجه، فاصلهٔ تلاشِ دوباره، و آخرین محلِ
/// **دستی** که کاربر گذاشته.
#[derive(Clone, Debug)]
pub struct IdlePolicy {
    settings: IdleReturnSettings,
    period: u64,
    period_started: Instant,
    returned_this_period: bool,
    /// درخواستی که فرستاده شده و نتیجه‌اش هنوز نیامده.
    outstanding: Option<MoveRequest>,
    last_result: Option<MoveResultRecord>,
    retry_after: Option<Instant>,
    /// بازگشتِ خودکار هرگز این را عوض نمی‌کند.
    last_manual: Option<PhysicalPoint>,
    /// آخرین `SpotTarget` دیده‌شده؛ برای تشخیصِ تغییرِ مقصد و مهرِ هندسه.
    known_target: Option<SpotTarget>,
    next_id: u64,
}

impl IdlePolicy {
    pub fn new(settings: IdleReturnSettings, now: Instant) -> Self;

    /// تصمیمِ بازگشتِ خودکار. خالص، بدون I/O، بدون ساعتِ واقعی.
    ///
    /// `target` همان چیزی است که **مالکِ هندسه** ساخته و بریده است؛ سیاست آن را
    /// دست‌نخورده برمی‌گرداند و هیچ حسابی روی مختصات نمی‌کند.
    pub fn evaluate(&mut self, now: Instant, activity: Activity,
                    target: Option<&SpotTarget>) -> Decision;

    /// نتیجهٔ حرکت. تنها راهی که `returned_this_period` را روشن می‌کند.
    pub fn report_move_result(&mut self, now: Instant, id: MoveRequestId,
                              outcome: MoveOutcome) -> MoveAck;

    /// عملِ جدا و صریحِ «برگشتن به محل دستیِ قبلی».
    pub fn ask_return_to_manual(&mut self, now: Instant) -> Decision;

    /// فراخوان این را در پایانِ هر کشیدن صدا می‌زند؛ تنها راهِ به‌روزرسانیِ محل دستی.
    pub fn note_manual_move(&mut self, to: PhysicalPoint);

    pub fn set_settings(&mut self, settings: IdleReturnSettings);
    pub fn settings(&self) -> &IdleReturnSettings;
    pub fn manual_spot(&self) -> Option<PhysicalPoint>;
    pub fn snapshot(&self) -> PolicySnapshot;
}
```

### ۳٫۳ ترتیب دقیق تصمیم (شماره‌ها ترتیب اجرا هستند)

> **این جدول، پس از مرحلهٔ نخست، همان چیزی است که در [orb_idle_policy.rs](../../voice-ptt/src/gui/orb_idle_policy.rs) اجرا می‌شود.**

| # | شرط | خروجی |
|---|---|---|
| ۱ | `!settings.enabled` | `No(Disabled)` |
| ۲ | `settings.pinned` | `No(Pinned)` |
| ۳ | `activity.blocker()` | `No(Blocked(b))` و **دورهٔ تازه**: `period_started = now` |
| ۴ | `activity.interacting` | `No(Interacting)` و **دورهٔ تازه** |
| ۵ | `target` با `known_target` فرق دارد | `No(TargetChanged)`؛ درخواستِ باز باطل، ولی **ساعت عمداً از نو نمی‌شود** |
| ۶ | `target` غایب است | `No(TargetUnavailable)`؛ اگر پیش‌تر هدفی بود، درخواستِ باز باطل می‌شود |
| ۷ | `returned_this_period` | `No(AlreadyReturnedThisPeriod)` |
| ۸ | درخواستی در جریان است | `No(AwaitingResult)` |
| ۹ | `now < retry_after` | `No(RetryBackoff)` |
| ۱۰ | `now < period_started + timeout` | `No(DeadlineNotReached)` |
| ۱۱ | همه‌چیز روشن | `Move(request)` و `outstanding = Some(request)` |

**دو نکته که ترتیب را می‌سازند:**

* **سطرهای ۱ و ۲ مقدم بر همه‌اند**، حتی بر درخواستِ در جریان. اگر کاربر وسطِ انتظار برای
  نتیجه، قابلیت را خاموش یا محل را ثابت کند، همان لحظه اثر می‌گذارد و دلیلش هم
  درست گزارش می‌شود — نه اینکه زیرِ `AwaitingResult` پنهان شود.
* **سطر ۵ ساعت را از نو نمی‌کند ولی سطرهای ۳ و ۴ می‌کنند.** تعامل و کارِ در جریان
  رویدادند و کاربر منتظرشان است؛ عوض‌شدنِ نمایشگر یا نسخهٔ هندسه رویداد نیست، فقط
  یعنی درخواستِ قبلی دربارهٔ دنیای قدیمی بوده است.

### ۳٫۴ مختصهٔ مقصد از بیرون می‌آید — اصلاحِ مرحلهٔ نخست

نسخهٔ نخست این سند یک تابع `corner_px` را **داخل سیاست** می‌گذاشت که خودش نقطهٔ گوشه
را حساب می‌کرد. آن کنار گذاشته شد، و این یک اصلاح کوچک نیست:

* سیاست دیگر **هیچ حسابی روی مختصات نمی‌کند** — نه `clamp`، نه شعاع، نه حاشیهٔ لبه، نه
  `keep_out_px`. تنها کاری که با نقطه می‌کند این است که همان عددِ تحویل‌شده را برگرداند،
  و این با آزمونِ `the_auto_return_passes_the_supplied_point_through_untouched` قفل است
  (نقطهٔ آزمون عمداً بیرون از هر فرمولی است: `-40, 7000`).
* **ساختِ مقصد و انتخابِ نمایشگر، مسئولِ اتصالِ مرکزی است.** فراخوان باید
  `SpotTarget { point, monitor, geometry_version }` را بسازد و تحویل دهد؛ `None` یعنی
  نمایشگر در دسترس نیست یا ناحیهٔ کاری معتبر نیست، و سیاست در آن حالت
  `TargetUnavailable` می‌گوید و **هیچ مختصه‌ای حدس نمی‌زند**.
* **آزمونِ برابریِ هندسه حذف شد** (`T19` در نسخهٔ نخستِ این سند). دیگر چیزی نیست که
  با `win::clamp_center` برابر شود: سیاست دیگر فرمولی ندارد که ممکن است از او جدا
  بیفتد. یعنی آن ریسک حذف شد، نه اینکه کم‌سنجشده بماند.
* در نتیجه `SpotGeometry` و `keep_out_px` دیگر ورودیِ سیاست نیستند؛ تنها `SpotTarget`
  هست که شاملِ نقطهٔ از پیش بریده‌شده و مهرِ هندسه است.

## ۴. قواعد حرکت

| # | قاعده | چگونه در سیاست پیاده می‌شود | چه چیزی را ثابت می‌کند |
|---|---|---|---|
| ۱ | هر دورهٔ بی‌کاری فقط **یک** بازگشت | `returned_this_period`؛ **فقط** `report_move_result` با `Success` آن را روشن می‌کند | بند ۷٫۰: «دو tick متوالی» |
| ۲ | تعاملِ تازه، دورهٔ تازه می‌سازد | سطرهای ۳ و ۴ جدول ۳٫۱: `period_started = now` و `returned_this_period = false` | بند ۷٫۰: «تعامل نزدیک مهلت» |
| ۳ | محل دستی و محل انتظار **دو مقدار جدا** | `last_manual` فقط با `note_manual_move` عوض می‌شود؛ `MoveKind` دو عملِ جدا دارد و **هیچ حالتی برای ذخیره در سیاست وجود ندارد** | بند ۷٫۰: «استقلال محل دستی» |
| ۴ | ثابت‌کردن، بازگشت را مسدود می‌کند | سطر ۲ جدول ۳٫۱، مقدم بر درخواستِ در جریان | بند ۷٫۰: «ثابت و خاموش» |
| ۵ | پس از رفع مانع، رفتارِ مهلت **مشخص** باشد | سطر ۳ جدول ۳٫۱: هر tickِ مانع‌دار `period_started` را روی `now` می‌گذارد، پس پس از رفع، مهلتِ کامل و تازه از صفر است | بند ۷٫۰: «رفع مانع» |
| ۶ | خروج نمایشگر، مقصدِ قابل‌دسترسی بدهد | سطر ۶ جدول ۳٫۱: `target == None` ⇒ `TargetUnavailable`؛ درخواستِ باز هم باطل می‌شود و **هیچ مختصه‌ای حدس زده نمی‌شود** | بند ۷٫۰: «مقصد نامعتبر» |
| ۷ | بازگشت دستی، یک‌باره و مستقل از تایمر است | `ask_return_to_manual` به مهلت نگاه نمی‌کند و خاموش/ثابتِ خودکار هم آن را نمی‌گیرد؛ ولی درخواستِ در جریان را می‌پذیرد | بند ۷٫۰: «استقلال محل دستی» |

### ۴٫۱ قاعدهٔ ۳ در عمل — چرا مهم‌ترین قاعدهٔ این سند است

مسیر امروزِ کشیدن چنین است: `orb.rs:396` مقدار `moved_to` را می‌دهد → `overlay.rs:1116`
آن را می‌گیرد → `overlay.rs:335` روی `gui.orb_position_x/y` نوشته و فایل را ذخیره می‌کند.
اگر بازگشتِ خودکار از همین مسیر عبور کند، **هر بار** که اورب بی‌کار شد جای دستیِ کاربر را
بازنویسی می‌کند و کاربر هرگز برنمی‌گردد.

پس تصمیم طراحی: بازگشتِ خودکار یک **جابه‌جایی موقت** است و هیچ‌وقت ذخیره نمی‌شود.
مبدأِ حرکت هم جدا نگه داشته می‌شود، و این جدایی در کد با `MoveKind` نشسته است:

```rust
/// دو عملِ جدا. «رفتن به گوشه» هرگز به «برگشتن به جایی که کاربر گذاشته» تبدیل نمی‌شود.
pub enum MoveKind { ToWaitingSpot, ToManualSpot }
```

محلِ دستی فقط با `note_manual_move` عوض می‌شود، یعنی فقط از راه کشیدنِ واقعیِ کاربر.
`ask_return_to_manual` همان مقدار را **می‌خواند** و هرگز نمی‌نویسد. در نتیجه هیچ مسیری
از بازگشتِ خودکار به `persist_orb_position` نمی‌رسد، چون سیاست اصلاً دستورِ ذخیره تولید
نمی‌کند: تنها `MoveRequest` بیرون می‌دهد، و اینکه آن را ذخیره شود یا نه، تصمیمِ فراخوان
در `overlay.rs` است — و همین‌جا باید «موقت» انتخاب شود.

---

## ۵. هماهنگی با هندسهٔ `O1`

| موضوع | تصمیم | شاهد |
|---|---|---|
| **واحد** | همهٔ چیزی که از سیاست بیرون می‌آید یا وارد آن می‌شود، **پیکسل فیزیکی** است. نقطه فقط داخل `orb.rs` و فقط هنگام تبدیل با `ppp` | `home` و `orb_position_x/y` هر دو پیکسل فیزیکی‌اند ([orb.rs:134-135](../../voice-ptt/src/gui/orb.rs#L134)، [settings.rs:386](../../voice-ptt/src/config/settings.rs#L386)) |
| **DPI** | سیاست `ppp` را **نمی‌بیند**. لازم نیست: `keep_out_px` از قبل ضرب‌شده به پیکسل تحویل می‌شود. اگر روزی `ppp` عوض شود، `keep_out_px` عوض می‌شود و مختصات خودبه‌خود درست است | `keep_out_px` تنها جایی است که نقطه به پیکسل تبدیل می‌شود ([orb.rs:1105-1112](../../voice-ptt/src/gui/orb.rs#L1105)) |
| **محدودهٔ قابل‌استفاده** | `rcWork` همان نمایشگر، نه کلِ نمایشگر و نه دسکتاپ مجازی | `win::clamp_center` عمداً ناحیهٔ کاری را انتخاب کرده چون `SM_CXVIRTUALSCREEN` نوار وظیفه را هم شامل می‌شود ([orb.rs:978-986](../../voice-ptt/src/gui/orb.rs#L978)) |
| **فاصله از لبه** | از بیرون تزریق می‌شود؛ سیاست هیچ ثابتِ هندسی ندارد | `EDGE_MARGIN_PT` ([orb.rs:44](../../voice-ptt/src/gui/orb.rs#L44)) |
| **مقصدِ پیش‌فرضِ اولین اجرا** | با `MonitorChoice::FollowOrb` صریح می‌شود. امروز نبودِ مختصات یعنی مرکزِ **کلِ** صفحهٔ اصلی ([orb.rs:973](../../voice-ptt/src/gui/orb.rs#L973))؛ تغییرِ این رفتار تصمیمِ محصول است و در این سند انجام نمی‌شود | همان |
| **پنجره** | سیاست به ضلعِ پنجره دست نمی‌زند. پنجره یک بار در بومِ بزرگ ساخته می‌شود و جابه‌جایی، تنها کاری است که `place` می‌کند | [orb.rs:294-303](../../voice-ptt/src/gui/orb.rs#L294) |

**اتصال حرکتِ واقعی از اینجا جدا می‌ماند.** جدول بالا دربارهٔ اعدادِ هندسه است، نه دربارهٔ
بازتابشان روی صفحه. تا وقتی محدودیت‌های `O1` روشن نشده — یعنی تا وقتی
[شرطِ بستن `O1`](O1-MANUAL-ACCEPTANCE.md) بند ۱۲ برقرار نشده — بستهٔ `O2` به پنجره وصل
نمی‌شود و مرحلهٔ ۸ (بند ۸) اجرا نمی‌شود. هیچ‌کس در این سند ادعا نمی‌کند که جابه‌جاییِ
خودکار روی صفحهٔ واقعی بی‌نقص است؛ عبارت درست برای آن تا آن روز «آزموده‌نشده» است.

### ۵٫۱ چیزی که دربارهٔ هاله و بریدگی گفته نمی‌شود

`O2` **هیچ** توضیحی برای هاله یا بریدگی پیکسل ارائه نمی‌دهد. چهار فرضیهٔ
[`H1..H4`](O1-MANUAL-ACCEPTANCE.md) بی‌اثبات‌اند و این سند نه تأییدشان می‌کند نه رد.
دو نکته که هنگام اتصال حرکتِ واقعی **باید** رعایت شوند و از همین حالا در طراحی لحاظ شده‌اند:

* جابه‌جاییِ پنجرهٔ شفاف مسیرِ مشکوکِ دست‌کمِ یکی از آرتیفکت‌هاست؛ بنابراین بازگشتِ
  خودکار **یک بار** در هر دوره انجام می‌شود، نه پیوسته، و هرگز همراه با resize.
* اگر پس از اتصال، آرتیفکتی دیده شد، عبارت درست «در این مسیر بازتولید نشد» است، نه
  «رفع شد» ([O1-MANUAL-ACCEPTANCE بند ۱۳](../../docs/execution/O1-MANUAL-ACCEPTANCE.md)).

---

## ۶. رابط تنظیمات و محل‌های اتصال

### ۶٫۱ شکل پیشنهادی در تنظیمات

```toml
[gui.idle_return]
enabled  = true     # پیشنهاد اولیه؛ تصمیم قطعیِ ثبت‌شده نیست
timeout_secs = 60   # پیشنهاد؛ «مقدار پیشنهادی» در پرامپت O2
retry_delay_secs = 5  # پیشنهاد؛ فاصله تا تلاشِ دوباره پس از شکستِ حرکت
corner   = "TopRight"      # پیشنهاد
monitor  = "FollowOrb"     # FollowOrb | Primary | Fixed:<n>
pinned   = false    # پیشنهاد
```

**نکتهٔ مرحلهٔ نخست:** ساختارِ `IdleReturnSettings` امروز در خودِ
[orb_idle_policy.rs](../../voice-ptt/src/gui/orb_idle_policy.rs) تعریف شده و `serde` ندارد.
آنچه اینجا لازم است فقط **لایهٔ ذخیره** است، نه تعریفِ دوباره: `#[derive(Serialize,
Deserialize)]` و تبدیلِ `*_secs` به `Duration`، همراه با `#[serde(default)]` تا کانفیگِ کاربرانِ
قدیمی بشکند. اینکه این لایه روی همان نوع بنشیند یا در `config` یک نوعِ جدا باشد، تصمیمِ
مرحلهٔ ۳ است و این سند آن را قطعی نمی‌کند. آنچه اینجا قطعی است: `Default` باید همان
`PROPOSED_TIMEOUT` و `PROPOSED_RETRY_DELAY` را بدهد، و میدان باید زیرِ `GuiSettings` موجود
([settings.rs:379-391](../../voice-ptt/src/config/settings.rs#L379)) بنشیند.

چرا زیر‌ساختار و نه چند میدانِ تخت: `O2` فقط همین یک بلوک را می‌خواهد و هر بلوکِ جدا
اجازه می‌دهد بعداً `O2` غیرفعال شود بدون آنکه بقیهٔ `gui` را لمس کند.

### ۶٫۲ نام گزینه‌ها و پیام‌های کوتاه

| کلید | برچسبِ پیشنهادی | راهنمای کوتاه | پیام وضعیت |
|---|---|---|---|
| `enabled` | «بازگشت خودکار اورب» | «پس از چند ثانیه بی‌کاری، اورب به گوشهٔ انتظار برگردد.» | «بازگشت خودکار خاموش است.» |
| `timeout_secs` | «مهلت بی‌کاری (ثانیه)» | «تا وقتی ضبط، پردازش، کشیدن یا متنِ منتظرِ اقدام داریم، این زمان از نو شروع می‌شود.» | «تا {n} ثانیه دیگر به {گوشه} برمی‌گردد.» |
| `corner` | «گوشهٔ انتظار» | «محلِ برگشتن. با جابه‌جا کردن دستیِ اورب، این گوشه عوض نمی‌شود.» | — |
| `monitor` | «نمایشگر» | «نمایشگری که اورب روی آن است، نمایشگر اصلی، یا یک شمارهٔ ثابت.» | «نمایشگرِ انتخابی در دسترس نیست.» |
| `pinned` | «محل اورب ثابت بماند» | «وقتی روشن باشد، اورb هرگز خودش جابه‌جا نمی‌شود.» | «محل ثابت است؛ بازگشت خودکار انجام نمی‌شود.» |
| — | «بازگشت به محل دستی» (دکمه) | — | «به محل دستی برگشت.» |

همهٔ رشته‌ها با `format_persian_display` نوشته می‌شوند، همان که همین حالا هم در
`validate_settings` استفاده می‌شود ([overlay.rs:87-92](../../voice-ptt/src/gui/overlay.rs#L87)).

**دربارهٔ پیامِ «در دسترس نیست»:** عمداً ساده است. سیاست در حالتِ `TargetUnavailable` فقط
می‌داند که **نمی‌تواند** برگردد و **هیچ مختصه‌ای هم حدس نمی‌زند**؛ نمی‌داند کدام نمایشگر
رفته یا ناحیهٔ کاری چرا نامعتبر شده است. جملهٔ دقیق‌تر متعلق به فراخوانی است که خودش
`SpotTarget` را ساخته و فهرست نمایشگرها را دارد.

### ۶٫۳ محل‌های اتصال — و داده‌ای که از هرکدام لازم است

| فایل | مرکزی؟ | کارِ پیشنهادی | دادهٔ لازم |
|---|---|---|---|
| `gui/orb_idle_policy.rs` | **مالک `O2`** (پیاده شد) | همین سند: `evaluate`، انواع، ۲۶ آزمون | فقط ورودی‌های بند ۳ |
| `gui/mod.rs` | مرکزی | ثبت `pub mod orb_idle_policy;` | — |
| `config/settings.rs` | مرکزی | لایهٔ `serde` + میدانِ `idle_return` در `GuiSettings` | مقدارِ کاربر |
| `gui/overlay.rs` | مرکزی | ساختنِ سیاست، ساختنِ `Activity` هر فریم، اجرای `MoveRequest`، پنل | `AppStatus`، وضعیتِ پنل، `ppp` |
| `state/status.rs` | مرکزی | حملِ واقعیت‌های صریحِ فعالیت به‌جای حدس از `AppState` | `chunk_busy`، `partial`، `latched` (همه موجودند) |
| `state/coordinator.rs` | مرکزی | تولید `pending_text`، حلِ نمایشگرِ قابل‌دسترسی | `kept`، `SessionId`، فهرست نمایشگرها |
| `gui/orb.rs` | مالک `O1` | **یک درزِ حداقلی**: `set_home_px` که `home` را با `keep_out_px` تنظیم و `anim.snap_position` کند | `keep_out_px` |
| `gui/overlay.rs` (دکمهٔ بازگشت) | مرکزی | «بازگشت به محل دستی» ← `ask_return_to_manual` | `MoveKind::ToManualSpot` |

**دو کار که `O2` انجام نمی‌دهد و باید در مالکیتِ خودش بمانند:** هیچ تغییری در `orb.rs`
(هندسه و درزِ حرکت) و هیچ تغییری در `settings.rs`/`overlay.rs` (ثبت ماژول و اتصال).

---

## ۷. آزمون‌ها با ساعت ساختگی

ساعت در این آزمون‌ها **داده** است، نه شیء: `Clock::at_ms(n) = origin + Duration::from_millis(n)`
که در آن `origin` فقط **یک‌بار** گرفته می‌شود. هیچ `sleep`ای در کار نیست. الگو، همان
`ManualClock` موجود در [coordinator.rs:2135-2206](../../voice-ptt/src/state/coordinator.rs#L2135)
است، ولی لازم نیست حتی آن هم باشد: `evaluate` مقدار `now` را می‌گیرد.

پیش‌زمینهٔ مشترک: `timeout = 60_000ms`، `retry_delay = 5s`، `pinned = false`،
`GEOMETRY_V1 = 7` و مقصدِ ثابتِ `SpotTarget::new(1836, 84, 7)` — مگر آنچه خودِ ردیف خلافش
را بگوید. توجه کنید که در این پیش‌زمینه **هیچ ناحیهٔ کاری و هیچ `keep_out_px`ای در کار
نیست**: سیاست آن‌ها را نمی‌بیند (بند ۳٫۴).

### ۷٫۰ آنچه در مرحلهٔ نخست واقعاً اجرا شد

جدولِ برنامه‌ریزی‌شدهٔ این سند (`T1..T22`) پیش‌بینی بود و **حذف شد**، چون یک جدولِ
پیش‌بینی که اجرا نشده بیش از آنکه کمک کند، بدهی جا می‌اندازد. آنچه در
[orb_idle_policy_test.rs](../../voice-ptt/tests/orb_idle_policy_test.rs) اجرا می‌شود
**۲۶ آزمون** است و نگاشتِ خواستهٔ این مرحله به آن‌ها چنین است:

| خواسته | آزمون‌ها |
|---|---|
| مرزِ مهلت | `one_millisecond_before_the_deadline…` · `exactly_at_the_deadline…` · `after_the_deadline…` |
| تعامل نزدیکِ مهلت | `interaction_one_millisecond_before_the_deadline_restarts_the_clock` |
| رفعِ مانع و مهلتِ تازه | `clearing_a_blocker_starts_a_full_new_timeout_not_the_remainder` · `every_blocker_reports_its_own_reason` |
| دو tick متوالی | `no_second_request_while_a_result_is_outstanding` · `a_successful_result_consumes_the_period_and_two_later_ticks_do_nothing` |
| موفقیت و شکستِ حرکت | `a_failed_result_does_not_consume_the_period_and_holds_off_the_next_attempt` · `a_failure_then_a_success_consumes_the_period_exactly_once` |
| تأییدِ دیررس | `a_late_ack_after_interaction_cannot_close_the_new_period` · `an_ack_for_a_different_id_is_ignored` |
| تغییرِ هندسه و مقصد | `a_new_geometry_version_invalidates_the_outstanding_request` · `a_different_corner_is_a_target_change` |
| مقصدِ نامعتبر | `an_unavailable_target_is_reported_without_a_coordinate` · `losing_the_target_invalidates_a_request_already_in_flight` |
| ثابت و خاموش هنگامِ انتظارِ نتیجه | `pinning_or_disabling_while_a_result_is_pending_takes_effect_at_once` · `disabled_pinned_and_deadline_are_three_different_reasons` |
| استقلالِ محلِ دستی | `going_to_the_corner_and_going_back_to_the_manual_spot_are_two_operations` · `a_manual_return_needs_no_deadline…` · `a_manual_return_while_a_result_is_pending_is_refused` · `the_auto_return_passes_the_supplied_point_through_untouched` |
| بی‌اعتنایی به گوشه و نمایشگر | `the_policy_is_indifferent_to_the_corner_and_the_monitor_preference` |

ساعتِ همهٔ این‌ها **ساختگی** است: `Instant::now()` فقط یک‌بار برای مبدأ، و بقیهٔ زمان‌ها
جمعِ `Duration`. هیچ `sleep`ای در فایل نیست.

**سه تصمیمی که این آزمون‌ها قفل کردند و ارزشِ خواندن دارند:**

* **بازگشتِ دستی هم بودجهٔ دوره را مصرف می‌کند.** این عمدی است: پس از اینکه کاربر
  گفت «برگشت به جایی که من گذاشته‌ام»، بازگشتِ خودکارِ همان دوره تکرار نشود. در غیر این صورت
  اورب در چند ثانیه، نظرِ کاربر را خنثی می‌کرد. قفل در
  `going_to_the_corner_and_going_back_to_the_manual_spot_are_two_operations`.
* **`TargetUnavailable` با `TargetChanged` یکی نیست.** نخستین یعنی «الان مقصدی وجود ندارد
  (نمایشگر رفته، ناحیهٔ کاری نامعتبر)» و سیاست هیچ مختصه‌ای حدس نمی‌زند؛ دومی یعنی «مقصد
  عوض شد» و درخواستِ باز را باطل می‌کند. یکی‌کردنشان یعنی گزارشِ دروغ به پنل.
* **سیاست به `corner` و `monitor` بی‌اعتناست.** چون ساختنِ مقصد و انتخابِ نمایشگر کارِ
  فراخوان است، `the_policy_is_indifferent_to_the_corner_and_the_monitor_preference` هر
  دوازده ترکیبِ ممکن را می‌پیماید و در همه، همان نقطهٔ تحویل‌شده را برمی‌گرداند.

### ۷٫۱ آزمون‌هایی که عمداً در این فایل **نیستند**

آزمونِ حرکتِ واقعیِ پنجره، آزمونِ بریدگی پیکسل و آزمونِ هاله در این فایل جایی ندارند،
چون به ماوس، دو نمایشگر و پروندهٔ بازِ `O1` نیاز دارند. آن‌ها متعلق به
[O1-MANUAL-ACCEPTANCE](O1-MANUAL-ACCEPTANCE.md) گروه‌های `A`، `C`، `D` و `E` هستند و
[`O2` تا روشن‌شدن محدودیت‌های `O1` وصل نمی‌شود](../../docs/AGENT-EXECUTION-PLAN.md).

---

## ۸. مراحل پیاده‌سازی

هر مرحله یک کامیتِ قابل‌بازبینی است و هیچ‌کدام به مرحلهٔ بعد وابسته نیست مگر آنچه
صریحاً نوشته شده.

| مرحله | محتوا | پیش‌نیاز |
|---|---|---|
| **۱** | `gui/orb_idle_policy.rs` با انواع بند ۳ و `evaluate`؛ بدون هیچ وابستگی به `gui`, `state` یا `config` | — |
| **۲** | آزمون‌ها در `tests/orb_idle_policy_test.rs` با ساعتِ ساختگی (۲۶ آزمون، بند ۷٫۰) | ۱ |
| **۳** | `IdleReturnSettings` + `Default` + `#[serde(default)]` در `config/settings.rs` و میدانِ `idle_return` در `GuiSettings` | ۱ |
| **۴** | پنل مستقلِ اورب: شش ردیف بند ۶٫۲ + دکمهٔ «بازگشت به محل دستی»؛ فقط نمایش و ویرایش تنظیمات، بدون سیاست | ۳ |
| **۵** | `Activity` در `overlay.rs` از `AppStatus` (`state`، `partial`، `latched`، `chunk_busy`) + وضعیتِ پنل + وضعیتِ کشیدن | ۱ |
| **۶** | فراخوانی `evaluate` در حلقهٔ رندر و ثبتِ دلیلِ `Hold` در لاگ، هم‌سبکِ لاگ‌های `O1` | ۵ |
| **۷** | **کارِ هماهنگ‌کننده، نه `O2`:** درزِ `set_home_px` در `orb.rs`، ساختِ `SpotTarget` (نقطهٔ بریده + نمایشگر + نسخهٔ هندسه)، تولیدِ `pending_text` در `coordinator.rs` | ۶ |
| **۸** | آزمون دستیِ حرکتِ واقعی، فقط بعد از بسته‌شدنِ `O1` طبق [شرطِ بستن](O1-MANUAL-ACCEPTANCE.md) بند ۱۲ | ۷ |

مرحلهٔ ۷ عمداً از ۱ تا ۶ جداست: تا وقتی درزِ حرکت وجود ندارد، بستهٔ `O2` کامل و
آزمون‌پذیر است و فقط **پیشنهاد** می‌دهد.

---

## ۹. مالکیت فایل

| فایل | مالک | وضعیت در این سند |
|---|---|---|
| `voice-ptt/src/gui/orb_idle_policy.rs` | **`O2`** | **در مرحلهٔ ۱ ساخته شد** و با ۲۶ آزمون سبز است |
| `voice-ptt/src/gui/mod.rs` | هماهنگ‌کننده | ثبت ماژول — خارج از `O2` |
| `voice-ptt/src/config/settings.rs` | هماهنگ‌کننده | افزودنِ میدان — خارج از `O2` |
| `voice-ptt/src/gui/overlay.rs` | هماهنگ‌کننده | اتصال و پنل — خارج از `O2` |
| `voice-ptt/src/state/status.rs` | هماهنگ‌کننده | حملِ واقعیت‌های فعالیت — خارج از `O2` |
| `voice-ptt/src/state/coordinator.rs` | هماهنک‌کننده | `pending_text` و نمایشگرِ قابل‌دسترسی — خارج از `O2` |
| `voice-ptt/src/gui/orb.rs` | **`O1`** | **دست‌نخورده**؛ درزِ حرکت کارِ هماهنگ‌کننده و با هماهنگی `O1` |
| `voice-ptt/src/gui/orb_animation.rs` · `window_shape.rs` · `orb_palette.rs` | `O1` | **دست‌نخورده** |
| `voice-ptt/src/state/session.rs` · `output/*` · `processing/*` | بسته‌های دیگر | **دست‌نخورده** |

موافقت‌نامهٔ موجود در [AGENT-EXECUTION-PLAN](../../docs/AGENT-EXECUTION-PLAN.md) رعایت شده
است: «اول `O1`؛ `O2` سیاست مستقل؛ اتصال توسط هماهنگ‌کننده».

---

## ۱۰. معیار پذیرش

| # | معیار | چطور سنجیده می‌شود |
|---|---|---|
| ۱ | `evaluate` خالص است: نه `Instant::now()`، نه `fs`، نه `SetWindowPos`، نه شبکه | بازبینی چشمیِ فایل + این واقعیت که کل آزمون‌ها با زمانِ تزریف‌شده سبز می‌شوند |
| ۲ | هر ۲۶ آزمونِ بند ۷٫۰ سبز است | `cargo test --test orb_idle_policy_test` |
| ۳ | هیچ `sleep` و هیچ انتظارِ زمانیِ واقعی در آزمون‌ها | همان اثباتِ ۲ |
| ۴ | فایلِ سیاست به `gui`, `state`, `config` یا هیچ crate دیگری وابسته نیست | نبودِ هر `use` به این مسیرها |
| ۵ | سیاست هیچ عددِ هندسیِ ثابتی ندارد | جست‌وجویی که ثابت‌های سیاست را فقط در آزمون‌ها بیابد |
| ۶ | سیاست هیچ محاسبه‌ای روی مختصات نمی‌کند | آزمونِ عبورِ بی‌دخالتِ نقطه (بند ۷٫۰)؛ در نبودِ فرمول، آزمونِ برابری معنا ندارد |
| ۷ | بازگشتِ خودکار هرگز `gui.orb_position_x/y` را نمی‌نویسد | بازبینی: تنها نویسندهٔ آن دو میدان، `persist_orb_position` از راه `moved_to` است |
| ۸ | هیچ تغییری در `orb.rs`، `orb_animation.rs`، `window_shape.rs`، `settings.rs` و `overlay.rs` در بستهٔ ۱ تا ۶ نیست | `git diff --name-only` همان بسته |
| ۹ | هر ردیفِ آزموده‌نشده با دلیلش ثبت شده، نه سکوت | جدول نتیجه، مثل [شرطِ ۴ بستن `O1`](O1-MANUAL-ACCEPTANCE.md) بند ۱۲ |

**آنچه این معیارها تضمین نمی‌کنند:** اینکه بازگشت روی صفحهٔ واقعی بی‌نقص دیده می‌شود.
آن تا بسته‌شدن `O1` و تا اجرای مرحلهٔ ۸ **آزموده‌نشده** است و باید همین‌طور نوشته شود.

---

## ۱۱. آنچه این سند ادعا نمی‌کند

* **ادعا نمی‌کند** که ۶۰ ثانیه تصمیمِ کاربر است. پیشنهاد است.
* **ادعا نمی‌کند** بی‌کاری را درست تشخیص داده‌ایم؛ فقط نشان می‌دهد **کدام** واقعیت‌ها لازم‌اند
  و کدام‌ها امروز اصلاً تولید نمی‌شوند (`pending_text`).
* **ادعا نمی‌کند** `home` جابه‌جا می‌شود؛ درزِ لازم برای آن هنوز ساخته نشده (بند ۱٫۱).
* **ادعا نمی‌کند** جابه‌جاییِ خودکار بی‌هنگام است. آزمونِ حرکتِ واقعی در بند ۷ نیست.
* **ادعا نمی‌کند** هاله یا بریدگی را توضیح داده؛ `H1..H4` دست‌نخورده و بی‌اثبات‌اند.
* **ادعا نمی‌کند** `O2` کامل است. مرحلهٔ نخست فقط یک **سیاست خالص** است؛ اتصال، تولیدِ
  `SpotTarget`، تولیدکنندهٔ مانعِ `pending_text`، و حرکتِ واقعی کارِ هماهنگ‌کننده و ادغامِ
  بعدی‌اند. تا آن روز، ۶۰ ثانیه و فاصلهٔ ۵ ثانیه **پیشنهاد** می‌مانند، نه تصمیم.
