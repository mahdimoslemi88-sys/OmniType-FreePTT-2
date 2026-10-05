//! آزمون‌های بستهٔ `O2` — سیاست بازگشت خودکار اورب به محل انتظار
//!
//! این آزمون فایلِ سیاست را مستقیم وارد می‌کند تا بدونِ تغییرِ `gui/mod.rs` یا
//! `Cargo.toml` کامپایل و آزموده شود — همان کاری که
//! [credentials_core_test.rs](../../voice-ptt/tests/credentials_core_test.rs) برای بستهٔ
//! `A1` می‌کند.
//!
//! ساعت **ساختگی** است: هیچ `sleep`ای در این فایل نیست و زمان فقط از `Instant` ساخته‌شده
//! می‌آید، پس آزمون به سرعت ماشین وابسته نیست.

#[path = "../src/gui/orb_idle_policy.rs"]
mod orb_idle_policy;

use std::time::{Duration, Instant};

use orb_idle_policy::{
    AckIgnored, Activity, Corner, Decision, IdlePolicy, IdleReturnSettings, MonitorChoice, MoveAck,
    MoveKind, MoveOutcome, MoveRequest, NoReturnReason, PhysicalPoint, ReturnBlocker, SpotTarget,
};

/// ساعتِ ساختگی: مبدأ ثابت و حرکت به جلو با گام‌های صریح.
struct Clock {
    origin: Instant,
}

impl Clock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// لحظهٔ `millis` میلی‌ثانیه بعد از مبدأ.
    fn at_ms(&self, millis: u64) -> Instant {
        self.origin + Duration::from_millis(millis)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

const TIMEOUT_MS: u64 = 60_000;
const GEOMETRY_V1: u64 = 7;

fn settings() -> IdleReturnSettings {
    IdleReturnSettings {
        enabled: true,
        timeout: Duration::from_millis(TIMEOUT_MS),
        retry_delay: Duration::from_secs(5),
        pinned: false,
        corner: Corner::TopRight,
        monitor: MonitorChoice::FollowOrb,
    }
}

fn target() -> SpotTarget {
    SpotTarget::new(1836, 84, GEOMETRY_V1)
}

/// فعالیتِ بی‌کار و بی‌تعامل.
fn idle() -> Activity {
    Activity::default()
}

// ---------------------------------------------------------------------------
// آماده‌سازی: نخستین evaluate هدف را «تازه» می‌بیند و یک دوره را آغاز می‌کند
// ---------------------------------------------------------------------------

/// سیاست تا وقتی یک‌بار با هدفِ واقعی صدا نزده، هیچ دوره‌ای را از نو شروع نکرده است.
/// بیشتر آزمون‌ها از این نقطه ادامه می‌دهند تا شمارشِ دوره‌ها خوانا بماند.
fn started(clock: &Clock, settings: IdleReturnSettings) -> IdlePolicy {
    let mut p = IdlePolicy::new(settings, clock.at_ms(0));
    let t = target();
    assert_eq!(
        p.evaluate(clock.at_ms(0), idle(), Some(&t)),
        Decision::No(NoReturnReason::TargetChanged),
        "the first sight of a target opens a fresh period"
    );
    p
}

fn move_request(clock: &Clock, p: &mut IdlePolicy) -> MoveRequest {
    let t = target();
    match p.evaluate(clock.at_ms(TIMEOUT_MS), idle(), Some(&t)) {
        Decision::Move(r) => r,
        other => panic!("expected a move at the deadline, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// ۱. مرزِ مهلت
// ---------------------------------------------------------------------------

#[test]
fn one_millisecond_before_the_deadline_nothing_is_requested() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let t = target();
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS - 1), idle(), Some(&t)),
        Decision::No(NoReturnReason::DeadlineNotReached)
    );
}

#[test]
fn exactly_at_the_deadline_the_move_is_requested() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let request = move_request(&c, &mut p);
    assert_eq!(request.kind, MoveKind::ToWaitingSpot);
    assert_eq!(request.to, PhysicalPoint { x: 1836, y: 84 });
}

#[test]
fn after_the_deadline_the_move_is_requested() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let t = target();
    assert!(matches!(
        p.evaluate(c.at_ms(TIMEOUT_MS + 1), idle(), Some(&t)),
        Decision::Move(_)
    ));
}

// ---------------------------------------------------------------------------
// ۲. تعاملِ نزدیکِ مهلت
// ---------------------------------------------------------------------------

#[test]
fn interaction_one_millisecond_before_the_deadline_restarts_the_clock() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let t = target();

    let interacting = Activity {
        interacting: true,
        ..idle()
    };
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS - 1), interacting, Some(&t)),
        Decision::No(NoReturnReason::Interacting)
    );

    // تعامل تمام شد، ولی ساعت از لحظهٔ تعامل است، پس در مهلتِ قبلی چیزی صادر نمی‌شود.
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS), idle(), Some(&t)),
        Decision::No(NoReturnReason::DeadlineNotReached)
    );
    assert!(matches!(
        p.evaluate(c.at_ms(2 * TIMEOUT_MS - 1), idle(), Some(&t)),
        Decision::Move(_)
    ));
}

// ---------------------------------------------------------------------------
// ۳. رفعِ مانع، یک مهلتِ کاملِ تازه
// ---------------------------------------------------------------------------

#[test]
fn clearing_a_blocker_starts_a_full_new_timeout_not_the_remainder() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let t = target();

    let recording = Activity {
        recording: true,
        ..idle()
    };
    assert_eq!(
        p.evaluate(c.at_ms(5 * TIMEOUT_MS), recording, Some(&t)),
        Decision::No(NoReturnReason::Blocked(ReturnBlocker::Recording))
    );

    // رفعِ مانع درست در همان لحظه‌ای که مهلتِ قبلی گذشته بود.
    assert_eq!(
        p.evaluate(c.at_ms(5 * TIMEOUT_MS), idle(), Some(&t)),
        Decision::No(NoReturnReason::DeadlineNotReached),
        "the old deadline must not survive the blocker"
    );
    assert_eq!(
        p.evaluate(c.at_ms(5 * TIMEOUT_MS + TIMEOUT_MS - 1), idle(), Some(&t)),
        Decision::No(NoReturnReason::DeadlineNotReached)
    );
    assert!(matches!(
        p.evaluate(c.at_ms(6 * TIMEOUT_MS), idle(), Some(&t)),
        Decision::Move(_)
    ));
}

#[test]
fn every_blocker_reports_its_own_reason() {
    let c = Clock::new();
    let cases = [
        (
            Activity {
                recording: true,
                ..idle()
            },
            ReturnBlocker::Recording,
        ),
        (
            Activity {
                processing: true,
                ..idle()
            },
            ReturnBlocker::Processing,
        ),
        (
            Activity {
                inserting: true,
                ..idle()
            },
            ReturnBlocker::Inserting,
        ),
        (
            Activity {
                dragging: true,
                ..idle()
            },
            ReturnBlocker::Dragging,
        ),
        (
            Activity {
                pending_text: true,
                ..idle()
            },
            ReturnBlocker::PendingText,
        ),
    ];
    for (activity, expected) in cases {
        let mut p = started(&c, settings());
        let t = target();
        assert_eq!(
            p.evaluate(c.at_ms(TIMEOUT_MS), activity, Some(&t)),
            Decision::No(NoReturnReason::Blocked(expected))
        );
    }
}

// ---------------------------------------------------------------------------
// ۴. درخواست در برابر نتیجه، و دو tick متوالی
// ---------------------------------------------------------------------------

#[test]
fn no_second_request_while_a_result_is_outstanding() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let request = move_request(&c, &mut p);
    let t = target();

    // حتی بسیار بعد از مهلت، تا وقتی نتیجه نیامده درخواستِ دوم صادر نمی‌شود.
    for step in 1..=5 {
        assert_eq!(
            p.evaluate(c.at_ms(TIMEOUT_MS * (1 + step)), idle(), Some(&t)),
            Decision::No(NoReturnReason::AwaitingResult)
        );
    }
    assert_eq!(p.snapshot().outstanding, Some(request.id));
}

#[test]
fn a_successful_result_consumes_the_period_and_two_later_ticks_do_nothing() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let request = move_request(&c, &mut p);
    let t = target();

    assert_eq!(
        p.report_move_result(c.at_ms(TIMEOUT_MS + 10), request.id, MoveOutcome::Success),
        MoveAck::Accepted {
            id: request.id,
            outcome: MoveOutcome::Success,
            consumed_this_period: true
        }
    );

    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS + 20), idle(), Some(&t)),
        Decision::No(NoReturnReason::AlreadyReturnedThisPeriod)
    );
    assert_eq!(
        p.evaluate(c.at_ms(10 * TIMEOUT_MS), idle(), Some(&t)),
        Decision::No(NoReturnReason::AlreadyReturnedThisPeriod)
    );
}

#[test]
fn a_failed_result_does_not_consume_the_period_and_holds_off_the_next_attempt() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let first = move_request(&c, &mut p);
    let t = target();

    assert_eq!(
        p.report_move_result(c.at_ms(TIMEOUT_MS + 10), first.id, MoveOutcome::Failure),
        MoveAck::Accepted {
            id: first.id,
            outcome: MoveOutcome::Failure,
            consumed_this_period: false
        }
    );

    // هر tick تا پایان فاصله، فقط «در فاصلهٔ تلاشِ دوباره» — نه درخواستِ تازه.
    for step in 1..=20 {
        let at = c.at_ms(TIMEOUT_MS + 10 + step * 200);
        assert_eq!(
            p.evaluate(at, idle(), Some(&t)),
            Decision::No(NoReturnReason::RetryBackoff),
            "a failed move must not request again on the very next tick"
        );
    }

    // پس از فاصله، تلاشِ بعدی روشن و با شناسهٔ تازه است.
    let retry_at = c.at_ms(TIMEOUT_MS + 10 + 5_000);
    match p.evaluate(retry_at, idle(), Some(&t)) {
        Decision::Move(second) => {
            assert_ne!(second.id, first.id, "the retry is a new request");
            assert_eq!(second.kind, MoveKind::ToWaitingSpot);
        }
        other => panic!("expected the retry to be requested, got {other:?}"),
    }
}

#[test]
fn a_failure_then_a_success_consumes_the_period_exactly_once() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let t = target();

    let first = move_request(&c, &mut p);
    p.report_move_result(c.at_ms(TIMEOUT_MS + 10), first.id, MoveOutcome::Failure);
    let second = match p.evaluate(c.at_ms(TIMEOUT_MS + 5_010), idle(), Some(&t)) {
        Decision::Move(r) => r,
        other => panic!("expected the retry, got {other:?}"),
    };

    assert_eq!(
        p.report_move_result(c.at_ms(TIMEOUT_MS + 5_100), second.id, MoveOutcome::Success),
        MoveAck::Accepted {
            id: second.id,
            outcome: MoveOutcome::Success,
            consumed_this_period: true
        }
    );
    assert!(p.snapshot().returned_this_period);
    assert_eq!(
        p.evaluate(c.at_ms(20 * TIMEOUT_MS), idle(), Some(&t)),
        Decision::No(NoReturnReason::AlreadyReturnedThisPeriod)
    );
}

// ---------------------------------------------------------------------------
// ۵. تأییدِ دیررس و بی‌اعتبارسازی
// ---------------------------------------------------------------------------

#[test]
fn a_late_ack_after_interaction_cannot_close_the_new_period() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let t = target();
    let old = move_request(&c, &mut p);

    // کاربر تعامل می‌کند: درخواستِ باز باطل و دوره تازه باز می‌شود.
    let interacting = Activity {
        interacting: true,
        ..idle()
    };
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS + 100), interacting, Some(&t)),
        Decision::No(NoReturnReason::Interacting)
    );

    // نتیجهٔ دیررسِ درخواستِ باطل‌شده نادیده گرفته می‌شود و دورهٔ تازه را نمی‌بندد.
    assert_eq!(
        p.report_move_result(c.at_ms(TIMEOUT_MS + 200), old.id, MoveOutcome::Success),
        MoveAck::Ignored {
            id: old.id,
            reason: AckIgnored::NoOutstanding
        }
    );
    assert!(
        !p.snapshot().returned_this_period,
        "a stale success must not consume the new period"
    );

    // دورهٔ تازه سرِ جایش باقی است و در مهلتِ خودش درخواست می‌دهد.
    let fresh = match p.evaluate(c.at_ms(2 * TIMEOUT_MS + 100), idle(), Some(&t)) {
        Decision::Move(r) => r,
        other => panic!("the new period must still be able to request, got {other:?}"),
    };
    assert_ne!(fresh.id, old.id);

    // و تأییدِ درست، همان دوره را مصرف می‌کند.
    assert_eq!(
        p.report_move_result(
            c.at_ms(2 * TIMEOUT_MS + 200),
            fresh.id,
            MoveOutcome::Success
        ),
        MoveAck::Accepted {
            id: fresh.id,
            outcome: MoveOutcome::Success,
            consumed_this_period: true
        }
    );
}

#[test]
fn an_ack_for_a_different_id_is_ignored() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let real = move_request(&c, &mut p);
    let stranger = orb_idle_policy::MoveRequestId(9999);

    assert_eq!(
        p.report_move_result(c.at_ms(TIMEOUT_MS + 10), stranger, MoveOutcome::Success),
        MoveAck::Ignored {
            id: stranger,
            reason: AckIgnored::NotTheOutstandingRequest
        }
    );
    assert_eq!(p.snapshot().outstanding, Some(real.id));
    assert!(!p.snapshot().returned_this_period);
}

// ---------------------------------------------------------------------------
// ۶. تغییرِ مقصد و تغییرِ نسخهٔ هندسه
// ---------------------------------------------------------------------------

#[test]
fn a_new_geometry_version_invalidates_the_outstanding_request() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let old = move_request(&c, &mut p);

    let v2 = SpotTarget::new(1836, 84, GEOMETRY_V1 + 1);
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS + 100), idle(), Some(&v2)),
        Decision::No(NoReturnReason::TargetChanged),
        "a geometry version bump is a change even at identical coordinates"
    );
    assert_eq!(
        p.report_move_result(c.at_ms(TIMEOUT_MS + 200), old.id, MoveOutcome::Success),
        MoveAck::Ignored {
            id: old.id,
            reason: AckIgnored::NoOutstanding
        }
    );

    // ساعت عمداً از نو نشد، پس درخواستِ تازه بی‌درنگ می‌آید.
    let fresh = match p.evaluate(c.at_ms(TIMEOUT_MS + 300), idle(), Some(&v2)) {
        Decision::Move(r) => r,
        other => panic!("expected a move for the new geometry, got {other:?}"),
    };
    assert_eq!(
        fresh.context.map(|t| t.geometry_version),
        Some(GEOMETRY_V1 + 1),
        "the request must carry the geometry it was made for"
    );
}

#[test]
fn a_different_corner_is_a_target_change() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let _ = move_request(&c, &mut p);

    let other = SpotTarget::new(84, 84, GEOMETRY_V1);
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS + 100), idle(), Some(&other)),
        Decision::No(NoReturnReason::TargetChanged)
    );
}

#[test]
fn an_unavailable_target_is_reported_without_a_coordinate() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS), idle(), None),
        Decision::No(NoReturnReason::TargetUnavailable),
        "a missing target must never be turned into a guessed point, nor called a change"
    );
    assert_eq!(p.snapshot().outstanding, None);
}

#[test]
fn losing_the_target_invalidates_a_request_already_in_flight() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let in_flight = move_request(&c, &mut p);

    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS + 10), idle(), None),
        Decision::No(NoReturnReason::TargetUnavailable)
    );
    assert_eq!(
        p.report_move_result(c.at_ms(TIMEOUT_MS + 20), in_flight.id, MoveOutcome::Success),
        MoveAck::Ignored {
            id: in_flight.id,
            reason: AckIgnored::NoOutstanding
        }
    );
    assert!(!p.snapshot().returned_this_period);
}

// ---------------------------------------------------------------------------
// ۷. خاموش و ثابت، با دلیلِ جدا
// ---------------------------------------------------------------------------

#[test]
fn disabled_pinned_and_deadline_are_three_different_reasons() {
    let c = Clock::new();
    let t = target();

    // این دو را با `started` نمی‌سازیم: آن کمک‌تابع فرض می‌کند نخستین evaluate
    // «تغییرِ مقصد» بدهد، و وقتی قابلیت خاموش یا ثابت است، دلیلِ درست اصلاً آن است.
    let mut off = IdlePolicy::new(
        IdleReturnSettings {
            enabled: false,
            ..settings()
        },
        c.at_ms(0),
    );
    assert_eq!(
        off.evaluate(c.at_ms(TIMEOUT_MS), idle(), Some(&t)),
        Decision::No(NoReturnReason::Disabled)
    );

    let mut pinned = IdlePolicy::new(
        IdleReturnSettings {
            pinned: true,
            ..settings()
        },
        c.at_ms(0),
    );
    assert_eq!(
        pinned.evaluate(c.at_ms(TIMEOUT_MS), idle(), Some(&t)),
        Decision::No(NoReturnReason::Pinned)
    );

    let mut normal = started(&c, settings());
    assert_eq!(
        normal.evaluate(c.at_ms(TIMEOUT_MS - 1), idle(), Some(&t)),
        Decision::No(NoReturnReason::DeadlineNotReached)
    );
}

#[test]
fn pinning_or_disabling_while_a_result_is_pending_takes_effect_at_once() {
    let c = Clock::new();
    let t = target();

    let mut pinned = started(&c, settings());
    let _ = move_request(&c, &mut pinned);
    pinned.set_settings(IdleReturnSettings {
        pinned: true,
        ..settings()
    });
    assert_eq!(
        pinned.evaluate(c.at_ms(TIMEOUT_MS + 10), idle(), Some(&t)),
        Decision::No(NoReturnReason::Pinned),
        "pinned must outrank a pending result, and must not be reported as awaiting"
    );

    let mut off = started(&c, settings());
    let _ = move_request(&c, &mut off);
    off.set_settings(IdleReturnSettings {
        enabled: false,
        ..settings()
    });
    assert_eq!(
        off.evaluate(c.at_ms(TIMEOUT_MS + 10), idle(), Some(&t)),
        Decision::No(NoReturnReason::Disabled)
    );
}

// ---------------------------------------------------------------------------
// ۸. استقلالِ محلِ دستی از محلِ انتظار
// ---------------------------------------------------------------------------

#[test]
fn the_auto_return_passes_the_supplied_point_through_untouched() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    // نقطه‌ای عجیب و بیرون از هر فرمولی: سیاست باید همان را برگرداند، نه چیز دیگری.
    // اگر جایی فرمولی روی مختصات اعمال می‌شد، این آزمون عوضش می‌کرد.
    let odd = SpotTarget::new(-40, 7_000, GEOMETRY_V1);
    assert_eq!(
        p.evaluate(c.at_ms(TIMEOUT_MS), idle(), Some(&odd)),
        Decision::No(NoReturnReason::TargetChanged),
        "a different point is a target change"
    );
    let request = match p.evaluate(c.at_ms(TIMEOUT_MS + 1), idle(), Some(&odd)) {
        Decision::Move(r) => r,
        other => panic!("expected a move, got {other:?}"),
    };
    assert_eq!(request.to, PhysicalPoint { x: -40, y: 7_000 });
}

#[test]
fn going_to_the_corner_and_going_back_to_the_manual_spot_are_two_operations() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    let manual = PhysicalPoint { x: 1113, y: 107 };

    // تا وقتی کاربر جایی را دستی تعیین نکرده، عملِ دستی معنا ندارد.
    assert_eq!(
        p.ask_return_to_manual(c.at_ms(0)),
        Decision::No(NoReturnReason::NoManualSpot)
    );
    // و «رفتن به گوشه» که پیش از آن در همان دوره صادر و موفق شده، جدا از آن است.
    let to_corner_first = match p.evaluate(c.at_ms(TIMEOUT_MS), idle(), Some(&target())) {
        Decision::Move(r) => r,
        other => panic!("expected the corner return, got {other:?}"),
    };
    assert_eq!(to_corner_first.kind, MoveKind::ToWaitingSpot);
    assert_eq!(to_corner_first.to, PhysicalPoint { x: 1836, y: 84 });
    p.report_move_result(
        c.at_ms(TIMEOUT_MS + 5),
        to_corner_first.id,
        MoveOutcome::Success,
    );

    p.note_manual_move(manual);
    assert_eq!(p.manual_spot(), Some(manual));

    let to_manual = match p.ask_return_to_manual(c.at_ms(1_000)) {
        Decision::Move(r) => r,
        other => panic!("expected the manual return, got {other:?}"),
    };
    assert_eq!(to_manual.kind, MoveKind::ToManualSpot);
    assert_eq!(to_manual.to, manual);
    p.report_move_result(c.at_ms(1_100), to_manual.id, MoveOutcome::Success);

    // جابه‌جاییِ دستیِ موفق هم بودجهٔ یک‌بارِ آن دوره را مصرف می‌کند؛ پس بازگشتِ
    // خودکارِ همان دوره تکرار نمی‌شود. این یک تصمیمِ عمدی است، نه اتفاق.
    assert_eq!(
        p.evaluate(c.at_ms(2 * TIMEOUT_MS), idle(), Some(&target())),
        Decision::No(NoReturnReason::AlreadyReturnedThisPeriod)
    );

    // بازگشتِ خودکار، محلِ دستی را جابه‌جا نکرد.
    assert_eq!(p.manual_spot(), Some(manual));
}

#[test]
fn a_manual_return_needs_no_deadline_and_outranks_the_auto_timeout() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    p.note_manual_move(PhysicalPoint { x: 100, y: 200 });
    // خیلی پیش از مهلت، و با قابلیتِ خاموش: دستورِ صریحِ کاربر کار می‌کند.
    p.set_settings(IdleReturnSettings {
        enabled: false,
        ..settings()
    });
    assert!(matches!(
        p.ask_return_to_manual(c.at_ms(10)),
        Decision::Move(_)
    ));
}

#[test]
fn a_manual_return_while_a_result_is_pending_is_refused() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    p.note_manual_move(PhysicalPoint { x: 100, y: 200 });
    let pending = match p.ask_return_to_manual(c.at_ms(1_000)) {
        Decision::Move(r) => r,
        other => panic!("expected the first manual return, got {other:?}"),
    };
    assert_eq!(
        p.ask_return_to_manual(c.at_ms(1_100)),
        Decision::No(NoReturnReason::AwaitingResult)
    );
    assert_eq!(p.snapshot().outstanding, Some(pending.id));
}

// ---------------------------------------------------------------------------
// ۹. تصویرِ وضعیت
// ---------------------------------------------------------------------------

#[test]
fn the_snapshot_reports_the_deadline_and_the_pending_request() {
    let c = Clock::new();
    let mut p = started(&c, settings());
    assert_eq!(p.snapshot().deadline, Some(c.at_ms(TIMEOUT_MS)));
    assert_eq!(p.snapshot().outstanding, None);

    let request = move_request(&c, &mut p);
    let snapshot = p.snapshot();
    assert_eq!(snapshot.outstanding, Some(request.id));
    assert!(!snapshot.returned_this_period);

    p.report_move_result(c.at_ms(TIMEOUT_MS + 5), request.id, MoveOutcome::Success);
    let snapshot = p.snapshot();
    assert_eq!(snapshot.outstanding, None);
    assert_eq!(
        snapshot.last_result.map(|r| r.outcome),
        Some(MoveOutcome::Success)
    );
}

#[test]
fn the_proposed_defaults_are_sixty_seconds_and_five() {
    let d = IdleReturnSettings::default();
    assert_eq!(d.timeout, Duration::from_secs(60));
    assert_eq!(d.retry_delay, Duration::from_secs(5));
    assert!(d.enabled && !d.pinned);
}

#[test]
fn is_busy_covers_both_a_blocker_and_interaction() {
    assert_eq!(idle().blocker(), None);
    assert!(!idle().is_busy());
    assert_eq!(
        Activity {
            pending_text: true,
            ..idle()
        }
        .blocker(),
        Some(ReturnBlocker::PendingText)
    );
    assert!(
        Activity {
            interacting: true,
            ..idle()
        }
        .is_busy(),
        "interaction alone is busy even with no blocker"
    );
}

#[test]
fn the_policy_is_indifferent_to_the_corner_and_the_monitor_preference() {
    // ساختنِ مختصه و انتخابِ نمایشگر کارِ فراخوان است، پس سیاست نباید به گوشه یا
    // نمایشگرِ انتخابی واکنشی نشان دهد: در هر دوازده ترکیب (۴ گوشه × ۳ نمایشگر)، همان
    // نقطهٔ تحویل‌شده را
    // درخواست می‌کند و تنظیمات را دست‌نخورده نگه می‌دارد.
    let c = Clock::new();
    let t = target();
    let corners = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];
    let monitors = [
        MonitorChoice::FollowOrb,
        MonitorChoice::Primary,
        MonitorChoice::Fixed(2),
    ];

    for corner in corners {
        for monitor in monitors {
            let mut p = IdlePolicy::new(
                IdleReturnSettings {
                    corner,
                    monitor,
                    ..settings()
                },
                c.at_ms(0),
            );
            assert_eq!(
                p.evaluate(c.at_ms(0), idle(), Some(&t)),
                Decision::No(NoReturnReason::TargetChanged)
            );
            match p.evaluate(c.at_ms(TIMEOUT_MS), idle(), Some(&t)) {
                Decision::Move(r) => assert_eq!(r.to, t.point, "{corner:?} / {monitor:?}"),
                other => panic!("expected a move for {corner:?} / {monitor:?}, got {other:?}"),
            }
            assert_eq!(p.settings().corner, corner);
            assert_eq!(p.settings().monitor, monitor);
        }
    }
}
