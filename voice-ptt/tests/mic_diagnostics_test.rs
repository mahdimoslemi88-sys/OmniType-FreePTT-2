//! M1 phase-1 acceptance tests: level analysis, the test state machine and the
//! device-ownership contract, all driven by a synthetic source and injected
//! time.
//!
//! **No microphone is opened anywhere in this file.** Every frame is built
//! here, and every duration is a number the test chose.
//!
//! The module is pulled in by path rather than through the crate root on
//! purpose: this stage must not touch module registration, so the test proves
//! the logic without waiting for the connection request to be wired up.

#[path = "../src/audio/diagnostics.rs"]
mod diagnostics;

use diagnostics::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const RATE: u32 = 16_000;

fn budget() -> Duration {
    Duration::from_secs(5)
}

fn sine(freq: f32, amplitude: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin())
        .collect()
}

fn square(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect()
}

/// A stand-in for "the device": it counts how often a session was opened and
/// how often it was released, which is the only thing these tests assert about
/// resource use.
#[derive(Default)]
struct SyntheticSource {
    opened: AtomicUsize,
    released: AtomicUsize,
}

impl SyntheticSource {
    fn open(&self) {
        self.opened.fetch_add(1, Ordering::SeqCst);
    }

    fn release(&self) {
        self.released.fetch_add(1, Ordering::SeqCst);
    }

    fn opens(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }

    fn releases(&self) -> usize {
        self.released.load(Ordering::SeqCst)
    }
}

/// A gate that hands out permits only while the recorder is idle. This is the
/// shape of the central implementation; it lives in a test because the real one
/// belongs to a file this package does not own.
struct TestGate {
    recording_active: Arc<AtomicUsize>,
    handovers: Arc<AtomicUsize>,
    /// Forced refusal, so the other two answers of the contract can be tested
    /// without a second gate implementation.
    forced: Option<MicGateRefusal>,
}

impl MicUseGate for TestGate {
    fn try_begin_test(&self) -> Result<MicTestPermit, MicGateRefusal> {
        if let Some(forced) = self.forced {
            return Err(forced);
        }
        if self.recording_active.load(Ordering::SeqCst) > 0 {
            return Err(MicGateRefusal::RecordingActive);
        }
        self.handovers.fetch_add(1, Ordering::SeqCst);
        Ok(MicTestPermit::issue())
    }
}

fn gate() -> (TestGate, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let recording = Arc::new(AtomicUsize::new(0));
    let handovers = Arc::new(AtomicUsize::new(0));
    (
        TestGate {
            recording_active: recording.clone(),
            handovers: handovers.clone(),
            forced: None,
        },
        recording,
        handovers,
    )
}

#[test]
fn the_gate_can_refuse_for_three_distinct_reasons() {
    for refusal in [
        MicGateRefusal::RecordingActive,
        MicGateRefusal::TestAlreadyRunning,
        MicGateRefusal::ShuttingDown,
    ] {
        let gate = TestGate {
            recording_active: Arc::new(AtomicUsize::new(0)),
            handovers: Arc::new(AtomicUsize::new(0)),
            forced: Some(refusal),
        };
        assert_eq!(gate.try_begin_test().err(), Some(refusal));
    }
}

fn running(rate: u32) -> MicTestMachine {
    running_with(rate, budget())
}

fn running_with(rate: u32, budget: Duration) -> MicTestMachine {
    let mut m = MicTestMachine::new(MicTestThresholds::default());
    m.start(
        MicTestPermit::issue(),
        Some("synthetic".into()),
        rate,
        budget,
    )
    .expect("a valid rate starts a test");
    m
}

/// Duration comparison with an explicit tolerance, because "within a
/// millisecond" is a claim these tests make rather than an accident.
fn within(actual: Duration, expected: Duration, tolerance: Duration) -> bool {
    actual.abs_diff(expected) <= tolerance
}

/// Feeds frames on a real 16 kHz timeline: each 1024-sample chunk is 64 ms of
/// audio, and the machine is ticked after every chunk the way a caller with a
/// running stream would. Feeding 80 000 samples instantly and then ticking
/// would be a pattern no device ever produces, and it would let the no-data
/// rule fire on data that had just arrived.
fn feed(m: &mut MicTestMachine, frames: &[f32]) {
    let chunk_ms = 1_024 * 1_000 / RATE as u128;
    let mut now = m.elapsed();
    for chunk in frames.chunks(1_024) {
        now += Duration::from_millis(chunk_ms as u64);
        m.push_frame(chunk, now);
        let _ = m.tick(now);
    }
}

// ── the analyzer: numbers ──────────────────────────────────────────────────

#[test]
fn rms_of_a_quiet_constant_matches_the_sample_value() {
    let mut a = MicTestAnalyzer::new(RATE).expect("16 kHz is a real rate");
    a.push(&vec![0.1f32; RATE as usize]);
    assert!((a.rms() - 0.1).abs() < 1e-6, "rms was {}", a.rms());
    assert!((a.peak() - 0.1).abs() < 1e-6);
    assert!((a.rms_dbfs() - -20.0).abs() < 0.01);
    assert!(within(
        a.valid_duration(),
        Duration::from_secs(1),
        Duration::from_millis(1)
    ));
    assert_eq!(a.clipped_samples(), 0);
}

#[test]
fn peak_is_full_scale_while_rms_sits_three_db_below_it() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    a.push(&sine(400.0, 1.0, 8_000));
    assert!((a.peak_dbfs() - 0.0).abs() < 0.01, "peak {}", a.peak_dbfs());
    assert!((a.rms_dbfs() - -3.01).abs() < 0.1, "rms {}", a.rms_dbfs());
}

#[test]
fn silence_is_a_finite_number_and_still_counts_as_data() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    a.push(&vec![0.0f32; 4_000]);
    assert_eq!(a.valid_samples(), 4_000);
    assert_eq!(a.invalid_samples(), 0);
    assert!(a.has_data(), "zeros arrived, so data arrived");
    assert!(a.rms_dbfs().is_finite() && a.peak_dbfs().is_finite());
    assert!(
        a.peak_dbfs() <= -119.0,
        "silence is floored, got {}",
        a.peak_dbfs()
    );
}

#[test]
fn an_empty_frame_is_not_silence_and_moves_no_counter() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    assert_eq!(a.push(&[]), FrameOutcome::Empty);
    assert_eq!(a.frames(), 0);
    assert_eq!(a.valid_samples(), 0);
    assert_eq!(a.invalid_samples(), 0);
    assert_eq!(a.summary(), MicTestSummary::default());
    assert!(!a.has_data(), "nothing arrived, so nothing is measured");

    a.push(&sine(400.0, 0.5, 1_600));
    let rms_after = a.rms();
    a.push(&[]);
    assert_eq!(a.frames(), 1, "an empty frame is not a frame");
    assert!((a.rms() - rms_after).abs() < 1e-7);
}

#[test]
fn statistics_do_not_depend_on_how_frames_were_sliced() {
    let samples = sine(300.0, 0.4, 8_000);

    let mut whole = MicTestAnalyzer::new(RATE).unwrap();
    whole.push(&samples);

    let mut sliced = MicTestAnalyzer::new(RATE).unwrap();
    for chunk in samples.chunks(97) {
        sliced.push(chunk);
    }

    assert_eq!(whole.valid_samples(), sliced.valid_samples());
    assert_eq!(whole.peak(), sliced.peak(), "peak is a max: exactly stable");
    assert!(
        (whole.rms() - sliced.rms()).abs() < 1e-6,
        "rms {} vs {}",
        whole.rms(),
        sliced.rms()
    );
    assert!((whole.rms_dbfs() - sliced.rms_dbfs()).abs() < 0.01);
}

#[test]
fn the_live_window_is_numbers_only() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    a.push(&sine(400.0, 0.3, 1_024));
    let w: LevelWindow = a.window();
    assert_eq!(w.valid_samples, 1_024);
    assert!((w.rms_dbfs - a.rms_dbfs()).abs() < 1e-6);
    assert!((w.peak_dbfs - a.peak_dbfs()).abs() < 1e-6);
}

// ── the analyzer: invalid input ────────────────────────────────────────────

#[test]
fn nan_and_infinite_samples_are_counted_and_excluded() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    a.push(&[0.5, f32::NAN, 0.5, f32::INFINITY, 0.5, f32::NEG_INFINITY]);
    assert_eq!(a.valid_samples(), 3);
    assert_eq!(a.invalid_samples(), 3);
    assert!((a.invalid_fraction() - 0.5).abs() < 1e-12);
    assert!(
        a.rms().is_finite() && a.peak().is_finite(),
        "one NaN must not poison the aggregate"
    );
    assert!((a.rms() - 0.5).abs() < 1e-6);
}

#[test]
fn samples_outside_full_scale_are_invalid_but_full_scale_itself_is_not() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    a.push(&[1.0, -1.0, 1.0001, -1.5]);
    assert_eq!(
        a.valid_samples(),
        2,
        "±1.0 is what an i16 capture really produces"
    );
    assert_eq!(a.invalid_samples(), 2);
    assert_eq!(
        a.clipped_samples(),
        2,
        "full scale is clipped but still valid"
    );
    assert!((a.peak() - 1.0).abs() < 1e-6);
}

#[test]
fn a_frame_of_only_nonsense_counts_as_a_frame_with_no_usable_data() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    assert_eq!(
        a.push(&[f32::NAN; 100]),
        FrameOutcome::Accepted {
            valid: 0,
            invalid: 100
        }
    );
    assert_eq!(a.frames(), 1);
    assert_eq!(a.valid_samples(), 0);
    assert_eq!(a.invalid_fraction(), 1.0);
    assert!(!a.has_data());
    assert!(a.rms().is_finite(), "no valid sample means zero, never NaN");
    assert_eq!(a.invalid_fraction(), 1.0);
}

#[test]
fn a_sample_rate_of_zero_or_beyond_any_endpoint_is_rejected() {
    assert_eq!(
        MicTestAnalyzer::new(0).err(),
        Some(MicTestSetupError::InvalidSampleRate { rate: 0 })
    );
    assert_eq!(
        MicTestAnalyzer::new(MAX_SAMPLE_RATE + 1).err(),
        Some(MicTestSetupError::InvalidSampleRate {
            rate: MAX_SAMPLE_RATE + 1
        })
    );
    assert!(MicTestAnalyzer::new(MAX_SAMPLE_RATE).is_ok());
    assert!(MicTestAnalyzer::new(RATE).is_ok());
}

#[test]
fn a_zero_rate_never_reaches_a_duration() {
    // Belt and braces: no analyzer can exist at rate 0, so no division by it.
    assert!(MicTestAnalyzer::new(0).is_err());
}

// ── no retention ───────────────────────────────────────────────────────────

#[test]
fn only_statistics_remain_after_the_samples_are_gone() {
    let mut a = MicTestAnalyzer::new(RATE).unwrap();
    a.push(&sine(400.0, 0.7, 4_000));
    let summary = a.summary();
    assert!(
        std::mem::size_of::<MicTestSummary>() <= 48,
        "a summary is counters, nothing else"
    );
    assert_eq!(summary.valid_samples, 4_000);

    let report = MicTestReport {
        device: Some("synthetic".into()),
        requested: budget(),
        observed: Duration::from_millis(250),
        samples: summary.valid_samples,
        frames: summary.frames,
        peak_dbfs: a.peak_dbfs(),
        rms_dbfs: a.rms_dbfs(),
        clipped_samples: summary.clipped_samples,
        invalid_samples: summary.invalid_samples,
        verdict: LevelVerdict::Usable,
    };
    assert!(std::mem::size_of::<MicTestReport>() < 128);
    assert_eq!(report.samples, 4_000);
}

// ── the machine: phases ────────────────────────────────────────────────────

#[test]
fn a_fresh_machine_is_ready_and_has_no_report() {
    let mut m = MicTestMachine::new(MicTestThresholds::default());
    assert_eq!(*m.phase(), MicTestPhase::Ready);
    assert!(m.report().is_none(), "ready has no verdict");
    assert!(m.may_start_recording(), "nothing holds the device");
    assert_eq!(m.tick(Duration::from_secs(9)), TickOutcome::Idle);
}

#[test]
fn normal_speech_finishes_with_a_usable_report() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, RATE as usize * 5));

    let report = m.report().expect("the budget finished the test");
    assert_eq!(report.verdict, LevelVerdict::Usable);
    assert_eq!(report.device.as_deref(), Some("synthetic"));
    assert_eq!(
        report.samples, 80_000,
        "every sample that was fed was measured"
    );
    assert_eq!(report.frames, 79, "80 000 samples in 1024-sample chunks");
    assert!(within(
        report.observed,
        report.requested,
        Duration::from_millis(100)
    ));
    assert!(
        (report.peak_dbfs - -10.46).abs() < 0.1,
        "peak {}",
        report.peak_dbfs
    );
    assert!(
        (report.rms_dbfs - -13.5).abs() < 0.15,
        "rms {}",
        report.rms_dbfs
    );
    assert_eq!(report.invalid_samples, 0);
    assert!(m.analyzer().is_none(), "the measurement is closed");
    assert!(
        m.holds_device(),
        "the device is still ours until the owner says so"
    );
    assert!(!m.may_start_recording());
    assert!(m.outstanding_release().is_some(), "a release is owed");
}

#[test]
fn the_budget_is_not_reached_early() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, RATE as usize * 4));
    assert!(m.report().is_none(), "four seconds is not five");
    assert_eq!(
        m.tick(Duration::from_millis(4_999)),
        TickOutcome::StillRunning
    );
    assert!(m.report().is_none());
    assert_eq!(m.tick(budget()), TickOutcome::Finished);
    assert_eq!(m.report().unwrap().verdict, LevelVerdict::Usable);
}

#[test]
fn a_quiet_mic_is_reported_low_without_being_called_broken() {
    let mut m = running(RATE);
    feed(&mut m, &vec![0.01f32; RATE as usize * 5]);
    m.tick(budget());
    let report = m.report().unwrap();
    assert_eq!(report.verdict, LevelVerdict::TooLow);
    assert!(report.peak_dbfs.is_finite(), "a level, not a fault report");
    assert!(report.peak_dbfs < 0.0);
}

#[test]
fn clipping_wins_over_level() {
    let mut m = running(RATE);
    feed(&mut m, &square(RATE as usize * 5));
    m.tick(budget());
    let report = m.report().unwrap();
    assert_eq!(report.verdict, LevelVerdict::Clipped);
    assert_eq!(report.clipped_samples, 80_000);
    assert!(report.peak_dbfs > -0.01);
}

#[test]
fn loud_quiet_and_silent_runs_are_three_different_reports() {
    let verdict_for = |frames: Vec<f32>| -> LevelVerdict {
        let mut m = running(RATE);
        feed(&mut m, &frames);
        m.tick(budget());
        m.report().unwrap().verdict
    };
    assert_eq!(
        verdict_for(sine(400.0, 0.3, RATE as usize * 5)),
        LevelVerdict::Usable
    );
    assert_eq!(
        verdict_for(sine(400.0, 0.005, RATE as usize * 5)),
        LevelVerdict::TooLow
    );
    assert_eq!(
        verdict_for(vec![0.0f32; RATE as usize * 5]),
        LevelVerdict::NoSignal
    );
}

#[test]
fn a_run_too_short_to_judge_says_so_instead_of_guessing() {
    let mut m = running_with(RATE, Duration::from_millis(200));
    feed(&mut m, &sine(400.0, 0.3, 400));
    m.tick(Duration::from_millis(200));
    let report = m.report().expect("the budget was reached");
    assert_eq!(report.verdict, LevelVerdict::InsufficientData);
    assert!(
        report.observed < Duration::from_millis(30),
        "observed {:?}",
        report.observed
    );
    assert!(report.observed < m.thresholds().min_observation);
}

#[test]
fn a_duplicate_start_is_refused_and_does_not_replace_the_run() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    let before = m.analyzer().unwrap().valid_samples();
    let phase_before = m.phase().clone();
    assert!(m.holds_device(), "a running test owns exactly one handover");

    assert_eq!(
        m.start(MicTestPermit::issue(), Some("other".into()), RATE, budget()),
        Err(MicStartRefusal::TestAlreadyRunning)
    );
    assert!(matches!(m.phase(), MicTestPhase::Running { .. }));
    assert!(m.holds_device(), "still exactly one handover, not two");
    assert_eq!(
        m.analyzer().unwrap().valid_samples(),
        before,
        "the running measurement keeps its samples"
    );
    assert_eq!(
        *m.phase(),
        phase_before,
        "a refused start changes nothing at all"
    );
    assert_eq!(
        phase_before,
        MicTestPhase::Running {
            device: Some("synthetic".into()),
            budget: budget(),
            elapsed: Duration::from_millis(128)
        }
    );
}

#[test]
fn every_way_out_of_a_test_owes_a_release_and_only_the_owner_closes_it() {
    // Finished by reaching the budget.
    let mut finished = running(RATE);
    feed(&mut finished, &sine(400.0, 0.3, RATE as usize * 5));
    assert!(matches!(finished.phase(), MicTestPhase::Finished(_)));
    assert!(finished.holds_device(), "the budget is not a release");
    let ticket = finished.outstanding_release().expect("a release is owed");
    assert!(!finished.may_start_recording());

    // Cancelled by closing the panel.
    let mut cancelled = running(RATE);
    cancelled.on_panel_closed();
    assert!(
        cancelled.holds_device(),
        "a closed panel does not free a device"
    );

    // Failed at the source.
    let mut failed = running(RATE);
    failed.fail_source(MicTestError::NoDevice);
    assert!(failed.holds_device(), "a failure is not a release");

    // Stopped by the user.
    let mut stopped = running(RATE);
    feed(&mut stopped, &sine(400.0, 0.3, 1_600));
    stopped.stop(MicTestCancel::UserPressedStop);
    assert!(stopped.holds_device());

    // The app exiting.
    let mut exiting = running(RATE);
    exiting.on_app_exit();
    assert!(exiting.holds_device());

    // A device nobody ever asked for.
    let idle = MicTestMachine::new(MicTestThresholds::default());
    assert!(!idle.holds_device());
    assert!(idle.may_start_recording());
    assert!(idle.outstanding_release().is_none());

    // Now the owner confirms one of them: the handover is really over.
    assert_eq!(
        finished.confirm_release(&MicReleaseAck::released(ticket.id())),
        Ok(())
    );
    assert!(!finished.holds_device());
    assert!(finished.may_start_recording());
    assert_eq!(
        finished.ownership(),
        DeviceOwnership::Released { request_id: ticket }
    );
    assert!(
        matches!(finished.phase(), MicTestPhase::Finished(_)),
        "the earned phase survives the late acknowledgement"
    );
}

#[test]
fn an_invalid_sample_rate_is_refused_before_anything_starts() {
    let mut m = MicTestMachine::new(MicTestThresholds::default());
    assert_eq!(
        m.start(MicTestPermit::issue(), None, 0, budget()),
        Err(MicStartRefusal::InvalidSampleRate { rate: 0 })
    );
    assert_eq!(*m.phase(), MicTestPhase::Ready, "a refusal changes nothing");
    assert!(m.analyzer().is_none());
    assert!(
        !m.holds_device(),
        "a rejected start hands the device straight back"
    );
}

// ── the machine: cancellation and failure ──────────────────────────────────

#[test]
fn cancelling_produces_no_verdict_even_with_good_data_collected() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.9, 8_000));
    let observed = m
        .stop(MicTestCancel::UserPressedStop)
        .expect("counts survive");

    assert_eq!(observed.valid_samples, 8_000);
    assert!(
        m.report().is_none(),
        "a cancelled test has counts, not a result"
    );
    match m.phase() {
        MicTestPhase::Cancelled {
            reason,
            observed: Some(s),
        } => {
            assert_eq!(*reason, MicTestCancel::UserPressedStop);
            assert_eq!(s.valid_samples, 8_000);
        }
        other => panic!("expected a cancelled phase, got {other:?}"),
    }
    assert!(m.analyzer().is_none());
    assert!(!m.may_start_recording(), "cancelling is not releasing");
    assert!(
        m.thresholds().low_level_rms > 0.0,
        "the diagnostic criterion is a positive floor"
    );
    assert_eq!(m.thresholds().no_signal_gap, Duration::from_millis(1_500));
}

#[test]
fn every_cancel_reason_names_itself() {
    let reasons = [
        MicTestCancel::UserPressedStop,
        MicTestCancel::RecordKeyTookOver,
        MicTestCancel::PanelClosed,
        MicTestCancel::AppExit,
    ];
    let names: Vec<&str> = reasons.iter().map(|r| r.as_str()).collect();
    for name in &names {
        assert!(!name.is_empty(), "every reason names itself");
    }
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        names.len(),
        "no two reasons share a name: {names:?}"
    );
}

#[test]
fn closing_the_panel_and_exiting_both_cancel_without_a_result() {
    let frames = sine(400.0, 0.9, 8_000);

    let mut panel = running(RATE);
    feed(&mut panel, &frames);
    panel.on_panel_closed();
    assert!(panel.report().is_none());
    assert!(matches!(
        panel.phase(),
        MicTestPhase::Cancelled {
            reason: MicTestCancel::PanelClosed,
            ..
        }
    ));

    let mut exiting = running(RATE);
    feed(&mut exiting, &frames);
    exiting.on_app_exit();
    assert!(exiting.report().is_none());
    assert!(matches!(
        exiting.phase(),
        MicTestPhase::Cancelled {
            reason: MicTestCancel::AppExit,
            ..
        }
    ));

    let mut quiet = running(RATE);
    assert!(
        quiet.on_panel_closed().is_none(),
        "no frame ever arrived, so there is no observation to report"
    );
    assert!(matches!(
        quiet.phase(),
        MicTestPhase::Cancelled {
            reason: MicTestCancel::PanelClosed,
            observed: None
        }
    ));
    assert!(
        quiet.holds_device(),
        "a closed panel does not free the device; the owner still owes a release"
    );
}

#[test]
fn source_failures_are_reported_verbatim_and_release_the_device() {
    for error in [
        MicTestError::NoDevice,
        MicTestError::SourceError {
            detail: "device busy".into(),
        },
    ] {
        let mut m = running(RATE);
        feed(&mut m, &sine(400.0, 0.3, 1_600));
        assert_eq!(
            m.fail_source(error.clone()),
            TickOutcome::Failed(error.clone())
        );
        assert_eq!(*m.phase(), MicTestPhase::Failed(error));
        assert!(m.report().is_none());
        assert!(m.analyzer().is_none());
        assert!(
            !m.may_start_recording(),
            "a source failure is a phase, not a release"
        );
    }
    assert_eq!(MicTestError::NoDevice.as_str(), "no input device");
}

#[test]
fn silence_is_not_a_fault_but_a_quiet_gap_is() {
    // Silence keeps arriving: the test reaches its budget and reports NoSignal.
    let mut silent = running(RATE);
    feed(&mut silent, &vec![0.0f32; RATE as usize * 5]);
    assert_eq!(silent.report().unwrap().verdict, LevelVerdict::NoSignal);
    assert_eq!(
        silent.report().unwrap().samples,
        80_000,
        "zeros are samples"
    );

    // The source goes quiet while the test is still inside its budget.
    let mut lost = running(RATE);
    feed(&mut lost, &sine(400.0, 0.3, RATE as usize));
    let elapsed = lost.elapsed();
    assert_eq!(
        lost.tick(elapsed + Duration::from_millis(1_499)),
        TickOutcome::StillRunning
    );
    assert_eq!(
        lost.tick(elapsed + Duration::from_millis(1_500)),
        TickOutcome::Failed(MicTestError::DeviceLost)
    );
    assert!(lost.report().is_none());
    assert!(
        lost.holds_device(),
        "a lost device still owes its owner a release"
    );
}

#[test]
fn mostly_invalid_data_fails_instead_of_producing_a_level() {
    let mut m = running(RATE);
    let mut frame = vec![0.4f32; 1_000];
    for x in frame.iter_mut().take(200) {
        *x = f32::NAN;
    }
    assert_eq!(
        m.push_frame(&frame, Duration::ZERO),
        FrameOutcome::Accepted {
            valid: 800,
            invalid: 200
        }
    );
    assert_eq!(
        m.tick(Duration::from_millis(50)),
        TickOutcome::Failed(MicTestError::UnusableInput {
            invalid_samples: 200
        })
    );
    assert!(m.report().is_none(), "garbage in, no measurement out");
}

#[test]
fn a_little_invalid_data_does_not_poison_the_result() {
    let mut m = running(RATE);
    let mut frame = sine(400.0, 0.3, RATE as usize * 5);
    frame[0] = f32::NAN;
    feed(&mut m, &frame);
    let report = m.report().expect("the budget finished the test");
    assert_eq!(report.invalid_samples, 1);
    assert_eq!(report.verdict, LevelVerdict::Usable);
}

// ── device ownership ───────────────────────────────────────────────────────

#[test]
fn a_start_during_recording_is_refused_by_the_gate() {
    let (gate, recording, handovers) = gate();
    assert!(gate.try_begin_test().is_ok());
    assert_eq!(handovers.load(Ordering::SeqCst), 1);

    recording.store(1, Ordering::SeqCst);
    assert_eq!(
        gate.try_begin_test().err(),
        Some(MicGateRefusal::RecordingActive)
    );
    assert_eq!(
        handovers.load(Ordering::SeqCst),
        1,
        "nothing was handed over"
    );

    // A refused start leaves a ready machine ready.
    let mut m = MicTestMachine::new(MicTestThresholds::default());
    if let Ok(permit) = gate.try_begin_test() {
        m.start(permit, None, RATE, budget()).unwrap();
    }
    assert_eq!(*m.phase(), MicTestPhase::Ready);
    assert!(m.analyzer().is_none());
}

#[test]
fn record_during_a_test_asks_for_a_confirmed_release_first() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));

    let ticket = match m.on_record_pressed() {
        RecordPressDecision::StopTestThenConfirm { ticket } => ticket,
        other => panic!("a running test must not hand the device over freely: {other:?}"),
    };
    assert!(
        !m.may_start_recording(),
        "recording stays shut until the owner confirms"
    );
    assert!(
        matches!(m.phase(), MicTestPhase::Running { .. }),
        "the test is still running"
    );
    assert!(m.report().is_none());

    assert_eq!(
        m.on_record_pressed(),
        RecordPressDecision::StopTestThenConfirm { ticket },
        "a second press must not stack a second request"
    );
    assert_eq!(
        m.confirm_release(&MicReleaseAck::failed(ticket.id())),
        Err(OwnershipError::ReleaseFailed)
    );
    assert!(
        !MicReleaseAck::failed(ticket.id()).is_released(),
        "a refusal is not a release"
    );
    assert!(MicReleaseAck::released(ticket.id()).is_released());
    assert!(!m.may_start_recording(), "a failed release unlocks nothing");
    assert!(matches!(m.phase(), MicTestPhase::Running { .. }));

    assert_eq!(
        m.confirm_release(&MicReleaseAck::released(ticket.id() + 99)),
        Err(OwnershipError::StaleTicket)
    );
    assert!(
        !m.may_start_recording(),
        "an ack for another request unlocks nothing"
    );

    assert_eq!(
        m.confirm_release(&MicReleaseAck::released(ticket.id())),
        Ok(())
    );
    assert!(
        m.may_start_recording(),
        "only now may the recorder open the device"
    );
    assert!(m.report().is_none(), "the taken-over test has no result");
    match m.phase() {
        MicTestPhase::Cancelled {
            reason,
            observed: Some(s),
        } => {
            assert_eq!(*reason, MicTestCancel::RecordKeyTookOver);
            assert_eq!(reason.as_str(), "record key took the device over");
            assert_eq!(s.valid_samples, 1_600, "counts survive, verdicts do not");
        }
        other => panic!("expected a cancelled phase, got {other:?}"),
    }
}

#[test]
fn recording_is_free_when_no_test_owns_the_device() {
    let mut m = MicTestMachine::new(MicTestThresholds::default());
    assert_eq!(m.on_record_pressed(), RecordPressDecision::StartRecording);
    assert_eq!(
        m.confirm_release(&MicReleaseAck::released(1)).err(),
        Some(OwnershipError::NothingToRelease),
        "confirming a release nobody asked for is an error, not a shortcut"
    );

    let mut finished = running(RATE);
    feed(&mut finished, &sine(400.0, 0.3, RATE as usize * 5));
    assert!(matches!(finished.phase(), MicTestPhase::Finished(_)));
    // Still ours: pressing record now asks for the release instead of taking
    // the device out from under a test that has not given it back.
    let ticket = match finished.on_record_pressed() {
        RecordPressDecision::StopTestThenConfirm { ticket } => ticket,
        other => panic!("a finished-but-unreleased test still holds the device: {other:?}"),
    };
    assert!(!finished.may_start_recording());
    assert_eq!(
        finished.confirm_release(&MicReleaseAck::released(ticket.id())),
        Ok(())
    );
    assert_eq!(
        finished.on_record_pressed(),
        RecordPressDecision::StartRecording
    );
    assert!(finished.may_start_recording());
}

#[test]
fn a_finished_test_can_be_repeated_and_does_not_inherit_numbers() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.9, 8_000));
    m.tick(budget());
    let first = m.report().unwrap().samples;
    assert_eq!(first, 8_000);

    // The repeat is only allowed once the owner has given the device back.
    assert_eq!(
        m.start(
            MicTestPermit::issue(),
            Some("synthetic".into()),
            RATE,
            budget()
        ),
        Err(MicStartRefusal::DeviceNotReleased)
    );
    let ticket = m.outstanding_release().expect("a release is owed");
    m.confirm_release(&MicReleaseAck::released(ticket.id()))
        .expect("the owner released it");

    m.start(
        MicTestPermit::issue(),
        Some("synthetic".into()),
        RATE,
        budget(),
    )
    .unwrap();
    assert_eq!(
        m.analyzer().unwrap().valid_samples(),
        0,
        "a repeat starts from zero"
    );
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    m.tick(budget());
    let second = m.report().unwrap();
    assert_eq!(second.samples, 1_600);
    assert_ne!(second.samples, first);
}

// ── the synthetic source ───────────────────────────────────────────────────

#[test]
fn the_synthetic_source_opens_and_closes_explicitly() {
    let source = SyntheticSource::default();
    assert_eq!(source.opens(), 0);
    source.open();
    assert_eq!(source.opens(), 1);
    assert_eq!(source.releases(), 0);
    source.release();
    assert_eq!(
        (source.opens(), source.releases()),
        (1, 1),
        "one open, one release"
    );
}

/// No recognition engine, no text insertion, no audio file, no playback: the
/// module cannot even name one. Checked against the source text, so the rule
/// survives an edit that would otherwise slip in unnoticed.
#[test]
fn the_module_has_no_path_to_recognition_insertion_files_or_playback() {
    let source = include_str!("../src/audio/diagnostics.rs");
    for forbidden in [
        "Vec<f32>",
        "Vec<u8>",
        "cpal",
        "AudioCapture",
        "AsrRouter",
        "Injection",
        "injector",
        "inject_text",
        "std::fs",
        "std::io",
        "std::net",
        "playback",
        "wav",
        "encode",
    ] {
        assert!(
            !source.contains(forbidden),
            "diagnostics.rs must not mention `{forbidden}`"
        );
    }
}

// ── ownership is separate from the phase (defect reproduction) ─────────────

/// Reproduces the audited defect: a logical end of the test was treated as a
/// release of the device. The owner had been asked to stop and drop the real
/// `AudioCapture`, but never confirmed it, and recording was allowed anyway.
#[test]
fn a_cancelled_test_does_not_release_the_device_on_its_own() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    match m.on_record_pressed() {
        RecordPressDecision::StopTestThenConfirm { .. } => {}
        other => panic!("expected a release request first: {other:?}"),
    }
    m.on_panel_closed();
    assert!(
        !m.may_start_recording(),
        "a cancelled test is a phase, not a release: without an ack, recording stays shut"
    );
}

#[test]
fn a_finished_test_does_not_release_the_device_on_its_own() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, RATE as usize * 5));
    assert!(matches!(m.phase(), MicTestPhase::Finished(_)));
    assert!(
        !m.may_start_recording(),
        "reaching the budget does not release a device nobody has freed"
    );
}

#[test]
fn a_failed_test_does_not_release_the_device_on_its_own() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    m.fail_source(MicTestError::NoDevice);
    assert!(matches!(m.phase(), MicTestPhase::Failed(_)));
    assert!(
        !m.may_start_recording(),
        "a source failure is a phase, not a release"
    );
}

#[test]
fn exiting_the_app_does_not_release_the_device_on_its_own() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    m.on_app_exit();
    assert!(matches!(
        m.phase(),
        MicTestPhase::Cancelled {
            reason: MicTestCancel::AppExit,
            ..
        }
    ));
    assert!(!m.may_start_recording(), "exiting is not a release either");
}

#[test]
fn a_new_test_is_refused_before_the_previous_release_is_confirmed() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    m.on_panel_closed();
    assert!(
        m.start(
            MicTestPermit::issue(),
            Some("second".into()),
            RATE,
            budget()
        )
        .is_err(),
        "a second test must not open a device the first one still holds"
    );
}

/// A release request is identified and asked for once: asking again while one
/// is outstanding returns the same ticket instead of stacking a second one.
#[test]
fn a_release_request_is_identified_and_never_repeated() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));

    let first = m.outstanding_release();
    assert!(first.is_none(), "nobody has asked for a release yet");

    let ticket = match m.on_record_pressed() {
        RecordPressDecision::StopTestThenConfirm { ticket } => ticket,
        other => panic!("{other:?}"),
    };
    assert_eq!(m.outstanding_release(), Some(ticket));
    assert_eq!(
        m.on_record_pressed(),
        RecordPressDecision::StopTestThenConfirm { ticket }
    );
    assert_eq!(m.outstanding_release(), Some(ticket), "still one request");

    // Ending the test afterwards must not mint a second id either.
    m.on_panel_closed();
    assert_eq!(
        m.outstanding_release(),
        Some(ticket),
        "the outstanding request is reused, not replaced"
    );
}

/// The audited defect's second half: an old acknowledgement must not free a
/// device that a later test is holding.
#[test]
fn a_stale_acknowledgement_never_unlocks_a_new_test() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    m.on_panel_closed();
    let old_ticket = m.outstanding_release().expect("first test owes a release");

    // The owner frees it, so a second test may start.
    m.confirm_release(&MicReleaseAck::released(old_ticket.id()))
        .expect("released");
    m.start(
        MicTestPermit::issue(),
        Some("second".into()),
        RATE,
        budget(),
    )
    .expect("the device is free now");
    assert!(!m.may_start_recording(), "the second test holds it again");

    // The first owner sends one more acknowledgement, late. It must not match.
    let new_ticket = match m.on_record_pressed() {
        RecordPressDecision::StopTestThenConfirm { ticket } => ticket,
        other => panic!("{other:?}"),
    };
    assert!(
        new_ticket.id() > old_ticket.id(),
        "ids are monotonic and never reused"
    );
    assert_eq!(
        m.confirm_release(&MicReleaseAck::released(old_ticket.id())),
        Err(OwnershipError::StaleTicket)
    );
    assert!(
        !m.may_start_recording(),
        "a late acknowledgement from an older test must not free the new one"
    );
    assert_eq!(
        m.confirm_release(&MicReleaseAck::released(new_ticket.id())),
        Ok(())
    );
    assert!(m.may_start_recording());
}

/// Every terminal path owes exactly one identified release, and each machine
/// answers a wrong id with `StaleTicket` rather than a release.
#[test]
fn each_terminal_path_owes_exactly_one_identified_release() {
    let mut owed: Vec<MicTestMachine> = Vec::new();

    let mut by_budget = running(RATE);
    feed(&mut by_budget, &sine(400.0, 0.3, RATE as usize * 5));
    assert!(matches!(by_budget.phase(), MicTestPhase::Finished(_)));
    owed.push(by_budget);

    let mut by_error = running(RATE);
    feed(&mut by_error, &sine(400.0, 0.3, 1_600));
    by_error.fail_source(MicTestError::SourceError {
        detail: "busy".into(),
    });
    owed.push(by_error);

    let mut by_stop = running(RATE);
    feed(&mut by_stop, &sine(400.0, 0.3, 1_600));
    by_stop.stop(MicTestCancel::UserPressedStop);
    owed.push(by_stop);

    let mut by_panel = running(RATE);
    by_panel.on_panel_closed();
    owed.push(by_panel);

    let mut by_exit = running(RATE);
    by_exit.on_app_exit();
    owed.push(by_exit);

    for machine in &mut owed {
        let ticket = machine
            .outstanding_release()
            .expect("one outstanding release");
        assert!(machine.holds_device());
        assert!(!machine.may_start_recording());
        assert_eq!(
            machine.confirm_release(&MicReleaseAck::released(ticket.id() + 7)),
            Err(OwnershipError::StaleTicket),
            "only the id that was handed out releases the device"
        );
        assert!(!machine.may_start_recording());
        assert_eq!(
            machine.confirm_release(&MicReleaseAck::released(ticket.id())),
            Ok(())
        );
        assert!(machine.may_start_recording());
        assert!(!machine.holds_device());
    }
}

/// Within one machine, successive tests never reuse a request id.
#[test]
fn request_ids_advance_across_successive_tests() {
    let mut m = running(RATE);
    let mut ids = Vec::new();
    for round in 0..3 {
        feed(&mut m, &sine(400.0, 0.3, RATE as usize * 5));
        let ticket = m.outstanding_release().expect("a release is owed");
        ids.push(ticket.id());
        m.confirm_release(&MicReleaseAck::released(ticket.id()))
            .expect("released");
        if round < 2 {
            m.start(
                MicTestPermit::issue(),
                Some("synthetic".into()),
                RATE,
                budget(),
            )
            .expect("free again");
        }
    }
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "ids are monotonic and never reused: {ids:?}"
    );
}

/// A negative answer is not a release, and a repeated negative answer neither.
#[test]
fn a_negative_acknowledgement_leaves_the_device_held() {
    let mut m = running(RATE);
    feed(&mut m, &sine(400.0, 0.3, 1_600));
    let ticket = match m.on_record_pressed() {
        RecordPressDecision::StopTestThenConfirm { ticket } => ticket,
        other => panic!("{other:?}"),
    };
    for _ in 0..3 {
        assert_eq!(
            m.confirm_release(&MicReleaseAck::failed(ticket.id())),
            Err(OwnershipError::ReleaseFailed)
        );
        assert!(m.holds_device());
        assert!(!m.may_start_recording());
        assert_eq!(
            m.outstanding_release(),
            Some(ticket),
            "the request is still open"
        );
    }
    assert_eq!(
        m.confirm_release(&MicReleaseAck::released(ticket.id())),
        Ok(())
    );
    assert!(m.may_start_recording());
}

/// The model can only receive an acknowledgement; it has no way to produce one.
#[test]
fn the_model_cannot_manufacture_an_acknowledgement() {
    let source = include_str!("../src/audio/diagnostics.rs");
    // Anything that could produce a positive acknowledgement without the owner
    // is the defect this test exists to prevent.
    // The constructors exist so the central owner can build one. The module
    // itself must never *call* them: only `confirm_release` consumes an ack,
    // and nothing in here may produce a positive one on its own.
    for call in [
        "MicReleaseAck::released(",
        "MicReleaseAck::failed(",
        "Self::released(",
        "Self::failed(",
    ] {
        assert!(
            !source.contains(call),
            "diagnostics.rs must not construct an acknowledgement via `{call}`"
        );
    }
    // A struct literal is the other way to build one. Every `MicReleaseAck {`
    // in the file must therefore be a declaration, never a value.
    for line in source.lines().filter(|l| l.contains("MicReleaseAck {")) {
        let is_declaration = line.trim_start().starts_with("pub struct ")
            || line.trim_start().starts_with("struct ")
            || line.trim_start().starts_with("impl ");
        assert!(
            is_declaration,
            "diagnostics.rs must not build an acknowledgement by struct literal: {line}"
        );
    }
}

#[test]
fn only_a_positive_acknowledgement_from_outside_opens_the_device() {
    // The behavioural half of the same rule: across every terminal path, the
    // device stays held until something that cannot come from inside this
    // module — a matching, positive ack — is handed in.
    let mut finished = running(16_000);
    feed(&mut finished, &sine(400.0, 0.3, 16_000));
    assert!(matches!(
        finished.tick(Duration::from_secs(5)),
        TickOutcome::Finished
    ));
    assert!(
        matches!(finished.ownership(), DeviceOwnership::Releasing { .. }),
        "finishing requests a release, it does not grant one"
    );

    let mut cancelled = running(16_000);
    cancelled.on_panel_closed();
    assert!(
        matches!(cancelled.ownership(), DeviceOwnership::Releasing { .. }),
        "closing the panel requests a release, it does not grant one"
    );

    let mut failed = running(16_000);
    failed.fail_source(MicTestError::NoDevice);
    assert!(
        matches!(failed.ownership(), DeviceOwnership::Releasing { .. }),
        "a source failure requests a release, it does not grant one"
    );

    // Only this — an ack the module did not create — changes the answer.
    let ticket = cancelled.outstanding_release().expect("a request is owed");
    assert!(!cancelled.may_start_recording());
    cancelled
        .confirm_release(&MicReleaseAck::released(ticket.id()))
        .expect("the owner confirms");
    assert!(cancelled.may_start_recording());
}
