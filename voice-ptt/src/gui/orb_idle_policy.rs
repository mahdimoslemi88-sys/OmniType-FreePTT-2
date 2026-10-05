//! سیاست بازگشت خودکار اورب به محل انتظار — بستهٔ `O2`، مرحلهٔ نخست.
//!
//! این ماژول **هیچ پنجره‌ای را حرکت نمی‌دهد** و هیچ API ویندوزی صدا نمی‌زند. کار آن فقط
//! تصمیم است: «آیا درخواستِ جابه‌جایی بده؟» و «نتیجهٔ جابه‌جاییِ قبلی چه شد؟».
//!
//! سه قرارداد که این فایل نگه می‌دارد و عمداً در تست قفل شده‌اند:
//!
//! 1. **زمان تزریقی.** هیچ‌جای این فایل `Instant::now()` نیست. زمان از راه پارامتر می‌آید،
//!    پس آزمون با ساعتِ ساختگی و بدون `sleep` کار می‌کند.
//! 2. **هندسه از بیرون.** نقطهٔ مقصد، نمایشگر و نسخهٔ هندسه را *مالکِ هندسه* می‌سازد و
//!    بریده‌شده تحویل می‌دهد ([`SpotTarget`]). این فایل نه `clamp` دارد، نه شعاع، نه حاشیهٔ
//!    لبه، نه فرمولی؛ تنها کاری که با مختصات می‌کند این است که همان عدد را برمی‌گرداند.
//! 3. **درخواست با نتیجه یکی نیست.** «درخواست دادم» و «حرکت انجام شد» دو لحظهٔ جدا هستند.
//!    تا نتیجه نیامده درخواستِ تکراری تولید نمی‌شود، و **فقط** تأییدِ موفق، بازگشتِ آن دوره
//!    را مصرف می‌کند.
//!
//! این فایل در `gui/mod.rs` ثبت **نشده** است: اتصال، کارِ هماهنگ‌کننده است. تا آن زمان،
//! تنها راهِ کامپایل‌شدنش آزمونِ `tests/orb_idle_policy_test.rs` است که آن را با
//! `#[path]` مستقیم وارد می‌کند.

use std::time::{Duration, Instant};

/// پیشنهاد اولیه برای مهلتِ بی‌کاری. **پیشنهاد است، نه تصمیمِ ثبت‌شدهٔ کاربر**: در
/// [O2-DESIGN.md](../../docs/execution/O2-DESIGN.md) بند ۰ توضیح داده شده که عدد ۶۰ ثانیه از
/// پرامپتِ بسته آمده، نه از تصمیمی. برای تغییرِ آن فقط همین ثابت عوض می‌شود.
pub const PROPOSED_TIMEOUT: Duration = Duration::from_secs(60);

/// فاصلهٔ پیشنهادی تا تلاشِ دوباره پس از شکستِ حرکت. بدون آن، هر tick یک درخواست تازه
/// تولید می‌شد.
pub const PROPOSED_RETRY_DELAY: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// واحد مختصات
// ---------------------------------------------------------------------------

/// مختصات فیزیکی روی دسکتاپ، همان واحدی که `Orb::home_position` و
/// `GuiSettings::orb_position_x/y` از آن استفاده می‌کنند.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// «خودکار» یعنی نمایشگری که اورب همین حالا روی آن است. **انتخابِ نمایشگر کارِ فراخوان است**
/// (سیاست فهرست نمایشگرها را نمی‌بیند)، پس این فقط ورودیِ پنل است.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MonitorChoice {
    FollowOrb,
    Primary,
    Fixed(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MonitorRef {
    pub index: u8,
    pub is_primary: bool,
}

/// مقصدی که **مالکِ هندسه** ساخته و بریده است.
///
/// `geometry_version` مهرِ روی مختصات است: هر وقت تعریفِ هندسهٔ `O1` عوض شد، این عدد عوض
/// می‌شود و سیاست می‌فهمد درخواستِ قبلی دربارهٔ دنیای قدیمی بوده است.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpotTarget {
    /// از پیش داخلِ ناحیهٔ کاری و با فاصلهٔ لبهٔ لازم — سیاست این را **دوباره حساب
    /// نمی‌کند**.
    pub point: PhysicalPoint,
    pub monitor: MonitorRef,
    pub geometry_version: u64,
}

impl SpotTarget {
    /// ساختِ مختصر برای فراخوان و آزمون.
    pub fn new(x: i32, y: i32, geometry_version: u64) -> Self {
        Self {
            point: PhysicalPoint { x, y },
            monitor: MonitorRef {
                index: 0,
                is_primary: true,
            },
            geometry_version,
        }
    }
}

// ---------------------------------------------------------------------------
// تنظیمات
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleReturnSettings {
    /// خاموش/روشنِ کل قابلیت.
    pub enabled: bool,
    /// مهلتِ بی‌کاری پیش از درخواستِ بازگشت.
    pub timeout: Duration,
    /// فاصله تا تلاشِ دوباره پس از شکستِ حرکت.
    pub retry_delay: Duration,
    /// ثابت‌کردنِ محل، بازگشتِ خودکار را مسدود می‌کند. دستورِ کاربرِ صریح (بازگشت به
    /// محل دستی) از این مستثنا است.
    pub pinned: bool,
    /// خوانده نمی‌شود؛ پنل و فراخوان از آن استفاده می‌کنند تا مقصد را بسازند.
    pub corner: Corner,
    /// خوانده نمی‌شود؛ انتخاب نمایشگر کارِ فراخوان است.
    pub monitor: MonitorChoice,
}

impl Default for IdleReturnSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout: PROPOSED_TIMEOUT,
            retry_delay: PROPOSED_RETRY_DELAY,
            pinned: false,
            corner: Corner::TopRight,
            monitor: MonitorChoice::FollowOrb,
        }
    }
}

// ---------------------------------------------------------------------------
// آنچه فراخوان دربارهٔ کارِ در جریان می‌داند
// ---------------------------------------------------------------------------

/// پنج واقعیتِ مستقل، نه یک حالتِ ترکیبی. سیاست خودش جمع نمی‌کند، چون جمع‌کردن یعنی
/// دوباره حدس‌زدن از روی `AppState` — و همان حدس‌زدنی که در [O2-DESIGN.md](../../docs/execution/O2-DESIGN.md)
/// بند ۲٫۲ نشان داده شد غلط از آب درمی‌آید.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub recording: bool,
    pub processing: bool,
    /// درجِ متن در جریان است (نتیجهٔ نوع‌دار و ناقص، هر دو).
    pub inserting: bool,
    pub dragging: bool,
    /// متنی هست که کاربر باید درباره‌اش تصمیم بگیرد.
    pub pending_text: bool,
    /// تعاملِ کاربر با اورب یا پنل.
    pub interacting: bool,
}

/// مانعِ بازگشت. هرکدام دلیلِ خودش را دارد و به چیز دیگری گزافه نمی‌شود.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnBlocker {
    Recording,
    Processing,
    Inserting,
    Dragging,
    PendingText,
}

impl Activity {
    /// اولین مانعِ فعال، با ترتیبِ ثابت تا گزارشِ دلیل **قابل پیش‌بینی** بماند.
    pub fn blocker(&self) -> Option<ReturnBlocker> {
        if self.recording {
            Some(ReturnBlocker::Recording)
        } else if self.processing {
            Some(ReturnBlocker::Processing)
        } else if self.inserting {
            Some(ReturnBlocker::Inserting)
        } else if self.dragging {
            Some(ReturnBlocker::Dragging)
        } else if self.pending_text {
            Some(ReturnBlocker::PendingText)
        } else {
            None
        }
    }

    pub fn is_busy(&self) -> bool {
        self.blocker().is_some() || self.interacting
    }
}

// ---------------------------------------------------------------------------
// درخواست و نتیجه
// ---------------------------------------------------------------------------

/// شناسهٔ یکتا و یک‌بارمصرفِ هر درخواست. نتیجهٔ دیررسِ یک شناسهٔ کهنه با شناسهٔ درخواستِ
/// جاری برابر نیست و نادیده گرفته می‌شود.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MoveRequestId(pub u64);

/// دو عملِ جدا. «رفتن به گوشه» هرگز به «برگشتن به جایی که کاربر گذاشته» تبدیل نمی‌شود.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    ToWaitingSpot,
    ToManualSpot,
}

/// یک درخواستِ جابه‌جایی. **این یک دستورِ ذخیره نیست** — نوعِ داده هیچ حالتی برای
/// نوشتنِ محلِ دستی ندارد، پس سیاست نمی‌تواند چنین دستوری تولید کند.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveRequest {
    pub id: MoveRequestId,
    /// دورهٔ بی‌کاری‌ای که این درخواست در آن صادر شد.
    pub period: u64,
    pub kind: MoveKind,
    pub to: PhysicalPoint,
    /// مهرِ هندسه‌ای که با آن مختصات ساخته شده. برای «رفتن به گوشه» همیشه `Some` است؛
    /// برای «برگشت به محل دستی» آخرین مهرِ دیده‌شده است یا `None`.
    pub context: Option<SpotTarget>,
    pub issued_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveOutcome {
    Success,
    Failure,
}

/// چرا یک تأییدِ دیررس نادیده گرفته شد.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckIgnored {
    /// درخواستِ بازی در جریان نیست — یا هرگز نبوده، یا با تعامل/تغییر مقصد باطل شده.
    NoOutstanding,
    /// درخواستی در جریان هست، ولی این تأیید مربوط به آن نیست.
    NotTheOutstandingRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveAck {
    Accepted {
        id: MoveRequestId,
        outcome: MoveOutcome,
        /// فقط **تأییدِ موفق** آن را `true` است: بازگشتِ همین دوره مصرف شد.
        consumed_this_period: bool,
    },
    Ignored {
        id: MoveRequestId,
        reason: AckIgnored,
    },
}

/// دلیلِ «اقدامی نشد». خاموش، ثابت، نرسیدنِ مهلت، و هر مانع، دلیلِ جدا دارند.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    No(NoReturnReason),
    Move(MoveRequest),
}

// ---------------------------------------------------------------------------
// وضعیتِ قابل مشاهده
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveResultRecord {
    pub id: MoveRequestId,
    pub outcome: MoveOutcome,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PolicySnapshot {
    pub period: u64,
    pub returned_this_period: bool,
    pub outstanding: Option<MoveRequestId>,
    pub deadline: Option<Instant>,
    pub retry_after: Option<Instant>,
    pub manual_spot: Option<PhysicalPoint>,
    pub last_result: Option<MoveResultRecord>,
}

// ---------------------------------------------------------------------------
// سیاست
// ---------------------------------------------------------------------------

/// سیاستِ بازگشتِ خودکار. خالص، با وضعیتِ کوچک، و کاملاً آزمون‌پذیر با زمانِ تزریقی.
#[derive(Clone, Debug)]
pub struct IdlePolicy {
    settings: IdleReturnSettings,
    /// شمارهٔ دورهٔ بی‌کاریِ جاری. هر بی‌اعتبارسازی یا شروعِ دوباره، آن را بالا می‌برد.
    period: u64,
    /// لحظهٔ آغازِ دورهٔ جاری؛ مهلت از اینجا شمرده می‌شود.
    period_started: Instant,
    returned_this_period: bool,
    /// درخواستی که فرستاده شده و نتیجه‌اش هنوز نیامده.
    outstanding: Option<MoveRequest>,
    last_result: Option<MoveResultRecord>,
    /// تا این لحظه، درخواستِ تازه تولید نشود (بعد از شکستِ حرکت).
    retry_after: Option<Instant>,
    /// آخرین محلی که کاربر **دستی** گذاشته. بازگشتِ خودکار هرگز این را عوض نمی‌کند.
    last_manual: Option<PhysicalPoint>,
    /// آخرین `SpotTarget` دیده‌شده؛ برای تشخیصِ تغییرِ مقصد و مهرِ هندسه.
    known_target: Option<SpotTarget>,
    next_id: u64,
}

impl IdlePolicy {
    /// `now` فقط برای این است که سیاست از همان لحظه شروع کند و پیش از نخستین
    /// `evaluate` هم قابل استفاده باشد.
    pub fn new(settings: IdleReturnSettings, now: Instant) -> Self {
        Self {
            settings,
            period: 0,
            period_started: now,
            returned_this_period: false,
            outstanding: None,
            last_result: None,
            retry_after: None,
            last_manual: None,
            known_target: None,
            next_id: 1,
        }
    }

    pub fn settings(&self) -> &IdleReturnSettings {
        &self.settings
    }

    /// عوض‌کردن تنظیمات، وضعیتِ در جریان را **نمی‌شکند**: درخواستِ در انتظار سرِ جایش
    /// می‌ماند و فقط نتیجه‌گیری از این پس با تنظیماتِ جدید است.
    pub fn set_settings(&mut self, settings: IdleReturnSettings) {
        self.settings = settings;
    }

    /// فراخوان این را در پایانِ هر کشیدن صدا می‌زند. محلِ دستی فقط همین‌جا و از
    /// دستِ کاربر به‌روز می‌شود.
    pub fn note_manual_move(&mut self, to: PhysicalPoint) {
        self.last_manual = Some(to);
    }

    pub fn manual_spot(&self) -> Option<PhysicalPoint> {
        self.last_manual
    }

    pub fn snapshot(&self) -> PolicySnapshot {
        PolicySnapshot {
            period: self.period,
            returned_this_period: self.returned_this_period,
            outstanding: self.outstanding.map(|r| r.id),
            deadline: Some(self.period_started + self.settings.timeout),
            retry_after: self.retry_after,
            manual_spot: self.last_manual,
            last_result: self.last_result,
        }
    }

    /// تصمیمِ بازگشتِ خودکار. ترتیبِ شرط‌ها قرارداد است و در آزمون‌ها قفل شده:
    /// خاموش ← ثابت ← مانع ← تعامل ← تغییر مقصد/هندسه ← مقصدِ نامعتبر ← مصرف‌شدنِ دوره ←
    /// در انتظارِ نتیجه ← فاصلهٔ تلاشِ دوباره ← مهلت.
    pub fn evaluate(
        &mut self,
        now: Instant,
        activity: Activity,
        target: Option<&SpotTarget>,
    ) -> Decision {
        if !self.settings.enabled {
            return Decision::No(NoReturnReason::Disabled);
        }
        if self.settings.pinned {
            return Decision::No(NoReturnReason::Pinned);
        }
        if let Some(blocker) = activity.blocker() {
            // مانع، ساعت را از همین لحظه از نو می‌کند؛ پس بعد از رفعِ مانع یک مهلتِ
            // کامل و تازه از صفر شروع می‌شود.
            self.begin_period(now);
            return Decision::No(NoReturnReason::Blocked(blocker));
        }
        if activity.interacting {
            self.begin_period(now);
            return Decision::No(NoReturnReason::Interacting);
        }

        // ناموجودیِ مقصد، «تغییرِ مقصد» نیست و نباید چنین گزارش شود: نمایشگر رفته یا
        // ناحیهٔ کاری نامعتبر شده است. درخواستِ باز هم باطل می‌شود، چون دیگر مقصدی
        // برای رسیدن به آن نیست.
        let Some(target) = target else {
            if self.known_target.is_some() {
                self.invalidate();
                self.known_target = None;
            }
            return Decision::No(NoReturnReason::TargetUnavailable);
        };

        if self.known_target != Some(*target) {
            // مقصد یا نسخهٔ هندسه عوض شده: درخواستِ باز باطل می‌شود تا تأییدِ دیررسِ
            // آن نتواند دورهٔ تازه را ببندد. ساعت عمداً از نو نمی‌شود — کاربر منتظرِ
            // همان مهلتی است که از قبل می‌دید، نه چند ده ثانیه عقب‌تر.
            self.invalidate();
            self.known_target = Some(*target);
            return Decision::No(NoReturnReason::TargetChanged);
        }

        if self.returned_this_period {
            return Decision::No(NoReturnReason::AlreadyReturnedThisPeriod);
        }
        if self.outstanding.is_some() {
            return Decision::No(NoReturnReason::AwaitingResult);
        }
        if let Some(retry_after) = self.retry_after {
            if now < retry_after {
                return Decision::No(NoReturnReason::RetryBackoff);
            }
            self.retry_after = None;
        }
        if now < self.period_started + self.settings.timeout {
            return Decision::No(NoReturnReason::DeadlineNotReached);
        }

        let request = self.issue(now, MoveKind::ToWaitingSpot, target.point, Some(*target));
        Decision::Move(request)
    }

    /// «برگشتن به محل دستیِ قبلی» — عملی جدا از رفتن به گوشه، و بدونِ نیاز به مهلت.
    /// خاموش‌بودن و ثابت‌بودنِ قابلیتِ خودکار، این دستورِ صریحِ کاربر را نمی‌گیرند.
    ///
    /// توجه: این عمل هم مانند هر جابه‌جاییِ موفق، **بودجهٔ یک‌بارِ آن دوره را مصرف
    /// می‌کند**؛ پس پس از آن، بازگشتِ خودکارِ همان دوره تکرار نمی‌شود.
    pub fn ask_return_to_manual(&mut self, now: Instant) -> Decision {
        let Some(to) = self.last_manual else {
            return Decision::No(NoReturnReason::NoManualSpot);
        };
        if self.outstanding.is_some() {
            return Decision::No(NoReturnReason::AwaitingResult);
        }
        let request = self.issue(now, MoveKind::ToManualSpot, to, self.known_target);
        Decision::Move(request)
    }

    /// نتیجهٔ حرکت. تنها راهی که `returned_this_period` را روشن می‌کند.
    pub fn report_move_result(
        &mut self,
        now: Instant,
        id: MoveRequestId,
        outcome: MoveOutcome,
    ) -> MoveAck {
        match self.outstanding {
            None => MoveAck::Ignored {
                id,
                reason: AckIgnored::NoOutstanding,
            },
            Some(request) if request.id != id => MoveAck::Ignored {
                id,
                reason: AckIgnored::NotTheOutstandingRequest,
            },
            Some(_) => {
                self.outstanding = None;
                self.last_result = Some(MoveResultRecord {
                    id,
                    outcome,
                    at: now,
                });
                match outcome {
                    MoveOutcome::Success => {
                        self.returned_this_period = true;
                        self.retry_after = None;
                        MoveAck::Accepted {
                            id,
                            outcome,
                            consumed_this_period: true,
                        }
                    }
                    MoveOutcome::Failure => {
                        // شکست، دوره را مصرف نمی‌کند؛ فقط فاصلهٔ تلاشِ بعدی را تعیین
                        // می‌کند تا هر tick یک درخواست تازه تولید نشود.
                        self.retry_after = Some(now + self.settings.retry_delay);
                        MoveAck::Accepted {
                            id,
                            outcome,
                            consumed_this_period: false,
                        }
                    }
                }
            }
        }
    }

    /// دورهٔ تازه از همین لحظه: ساعت صفر می‌شود و هر درخواستِ باز باطل.
    fn begin_period(&mut self, now: Instant) {
        self.invalidate();
        self.period_started = now;
    }

    /// بی‌اعتبارسازیِ درخواستِ باز و بازکردنِ دورهٔ تازه، **بدون** دست‌زدن به ساعت.
    fn invalidate(&mut self) {
        self.period = self.period.wrapping_add(1);
        self.returned_this_period = false;
        self.outstanding = None;
        self.retry_after = None;
    }

    fn issue(
        &mut self,
        now: Instant,
        kind: MoveKind,
        to: PhysicalPoint,
        context: Option<SpotTarget>,
    ) -> MoveRequest {
        let request = MoveRequest {
            id: MoveRequestId(self.next_id),
            period: self.period,
            kind,
            to,
            context,
            issued_at: now,
        };
        self.next_id = self.next_id.wrapping_add(1);
        self.outstanding = Some(request);
        request
    }
}
