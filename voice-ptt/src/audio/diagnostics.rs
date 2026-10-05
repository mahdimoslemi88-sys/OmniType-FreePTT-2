//! Mic-test diagnostics: level analysis and the test state machine.
//!
//! **This stage is deliberately device-free.** There is no stream, no device
//! open, no panel and no clock read anywhere in this file — the analyzer takes
//! samples a caller hands it, and the state machine takes time a caller hands
//! it. That is what makes every rule below testable without a microphone.
//!
//! What this module deliberately cannot do, and does not even name: it cannot
//! reach a recognition engine, a text-insertion path, the filesystem or a
//! network, and it has no way to play anything back. Audio enters as `&[f32]`
//! and leaves as counters; **no audio is ever retained** — there is not a
//! sample buffer in any type here, and the type fields are the proof.
//!
//! Two rules that are easy to get wrong, so they are stated up front:
//!
//! * An **empty frame is not silence.** It changes nothing at all. Only samples
//!   that actually arrived count towards level and duration.
//! * **Silence and "no data" are not a hardware diagnosis.** They are
//!   observations. `LowLevel` is a *proposed* diagnostic criterion borrowed
//!   from one energy engine's threshold; it is not, and must not be read as,
//!   what every engine of this program considers speech.
//!
//! The device-ownership contract (`MicUseGate`, `MicTestPermit`,
//! `MicReleaseTicket`) is defined here as types only. The implementation that
//! consults the real recorder belongs to the central owner and is **not built
//! in this stage**.

/// dBFS floor, identical to the dictation path's, so a number printed by the
/// test and a number printed by the recorder mean the same thing.
pub const DBFS_FLOOR: f32 = 1e-6;

/// Sample rates outside this range are treated as a caller bug rather than a
/// device fact: `0` cannot divide, and nothing above 384 kHz is a capture
/// endpoint any supported driver exposes.
pub const MAX_SAMPLE_RATE: u32 = 384_000;

// ────────────────────────────────────────────────────────────── measurement ──

/// What one `push` did. `Empty` exists so "no frame arrived" stays
/// distinguishable from "a frame of zeros arrived".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// Nothing to measure; no counter moved.
    Empty,
    Accepted {
        valid: u64,
        invalid: u64,
    },
}

/// Counters for one mic test. Plain data, no collections, no audio.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MicTestSummary {
    pub frames: u64,
    pub valid_samples: u64,
    pub invalid_samples: u64,
    pub clipped_samples: u64,
    pub peak_bits: f32,
    pub rms: f32,
}

/// A rejected configuration, before any test exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicTestSetupError {
    InvalidSampleRate { rate: u32 },
}

/// Why a test could not start.
///
/// A live dictation is **not** in this list on purpose: refusing to take the
/// device away from a recording is the gate's job, and it says so with
/// [`MicGateRefusal::RecordingActive`] before a permit ever exists. Keeping the
/// two apart is what stops a caller from inventing its own answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicStartRefusal {
    TestAlreadyRunning,
    /// The previous test's handover has not been released yet. A new test may
    /// not open a device somebody still holds.
    DeviceNotReleased,
    InvalidSampleRate {
        rate: u32,
    },
}

/// Accumulates level statistics. Holds counters only.
#[derive(Debug, Clone)]
pub struct MicTestAnalyzer {
    sample_rate: u32,
    frames: u64,
    valid_samples: u64,
    invalid_samples: u64,
    clipped_samples: u64,
    peak: f32,
    /// `f64` so a long test does not lose headroom, and so the RMS of the same
    /// samples is stable (to within floating-point rounding) however the caller
    /// chose to slice them into frames.
    sum_squares: f64,
}

/// A sample is *invalid* — counted, reported, and excluded from every level
/// statistic — when it is not finite or falls outside full scale.
#[inline]
fn is_valid_sample(x: f32) -> bool {
    x.is_finite() && x.abs() <= 1.0
}

impl MicTestAnalyzer {
    pub fn new(sample_rate: u32) -> Result<Self, MicTestSetupError> {
        if sample_rate == 0 || sample_rate > MAX_SAMPLE_RATE {
            return Err(MicTestSetupError::InvalidSampleRate { rate: sample_rate });
        }
        Ok(Self {
            sample_rate,
            frames: 0,
            valid_samples: 0,
            invalid_samples: 0,
            clipped_samples: 0,
            peak: 0.0,
            sum_squares: 0.0,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Folds one frame in and returns only counters — the frame is never kept.
    ///
    /// Invalid samples (`NaN`, infinite, outside `-1.0..=1.0`) are counted and
    /// skipped: letting one `NaN` into the sum would poison every later RMS.
    /// An empty frame is a strict no-op.
    pub fn push(&mut self, frame: &[f32]) -> FrameOutcome {
        if frame.is_empty() {
            return FrameOutcome::Empty;
        }
        let mut valid = 0u64;
        let mut invalid = 0u64;
        for &x in frame {
            if !is_valid_sample(x) {
                invalid += 1;
                continue;
            }
            valid += 1;
            let magnitude = x.abs();
            if magnitude > self.peak {
                self.peak = magnitude;
            }
            if magnitude >= CLIP_SAMPLE {
                self.clipped_samples += 1;
            }
            // `f64` arithmetic from an `f32` sample: the cast is exact, so the
            // sum does not depend on the sample type's precision.
            self.sum_squares += f64::from(x) * f64::from(x);
        }
        self.frames += 1;
        self.valid_samples += valid;
        self.invalid_samples += invalid;
        FrameOutcome::Accepted { valid, invalid }
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn valid_samples(&self) -> u64 {
        self.valid_samples
    }

    pub fn invalid_samples(&self) -> u64 {
        self.invalid_samples
    }

    pub fn clipped_samples(&self) -> u64 {
        self.clipped_samples
    }

    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Root mean square of the **valid** samples, or `0.0` when there were
    /// none. Never `NaN`.
    pub fn rms(&self) -> f32 {
        if self.valid_samples == 0 {
            return 0.0;
        }
        (self.sum_squares / self.valid_samples as f64).sqrt() as f32
    }

    pub fn peak_dbfs(&self) -> f32 {
        dbfs(self.peak)
    }

    pub fn rms_dbfs(&self) -> f32 {
        dbfs(self.rms())
    }

    /// Duration of the data that actually arrived, at the device's rate.
    pub fn valid_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(self.valid_samples as f64 / f64::from(self.sample_rate))
    }

    /// Share of received samples that were unusable. Zero when nothing arrived.
    pub fn invalid_fraction(&self) -> f64 {
        let total = self.valid_samples + self.invalid_samples;
        if total == 0 {
            return 0.0;
        }
        self.invalid_samples as f64 / total as f64
    }

    /// Whether any usable sample arrived at all. Silence still counts as data:
    /// zeros are valid samples.
    pub fn has_data(&self) -> bool {
        self.valid_samples > 0
    }

    pub fn summary(&self) -> MicTestSummary {
        MicTestSummary {
            frames: self.frames,
            valid_samples: self.valid_samples,
            invalid_samples: self.invalid_samples,
            clipped_samples: self.clipped_samples,
            peak_bits: self.peak,
            rms: self.rms(),
        }
    }
}

/// One live window for a level meter. Numbers only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelWindow {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    pub valid_samples: u64,
}

impl MicTestAnalyzer {
    /// A window covering everything folded in so far — cheap enough to call per
    /// UI frame and stateless, so the meter needs no extra buffer either.
    pub fn window(&self) -> LevelWindow {
        LevelWindow {
            peak_dbfs: self.peak_dbfs(),
            rms_dbfs: self.rms_dbfs(),
            valid_samples: self.valid_samples,
        }
    }
}

fn dbfs(value: f32) -> f32 {
    20.0 * value.max(DBFS_FLOOR).log10()
}

// ─────────────────────────────────────────────────────────────── thresholds ──

/// |x| at or above this counts as one clipped sample. Full scale for `f32` is
/// `1.0`, and the `i16` conversion really does produce exactly `-1.0`, so the
/// test is `>= 0.999` rather than `>= 1.0`.
pub const CLIP_SAMPLE: f32 = 0.999;

/// Diagnostic criteria. `low_level_rms` is **a proposal**, not a law: it is
/// borrowed from the RMS fallback engine's threshold so the number is at least
/// traceable to something in the codebase, and it is explicitly *not* a claim
/// about what the Silero engine or any other engine calls speech.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MicTestThresholds {
    pub low_level_rms: f32,
    pub clip_sample: f32,
    pub clip_fraction: f64,
    pub min_observation: std::time::Duration,
    pub max_invalid_fraction: f64,
    pub no_signal_gap: std::time::Duration,
}

impl Default for MicTestThresholds {
    fn default() -> Self {
        Self {
            low_level_rms: 0.012,
            clip_sample: CLIP_SAMPLE,
            clip_fraction: 0.01,
            min_observation: std::time::Duration::from_millis(500),
            max_invalid_fraction: 0.05,
            no_signal_gap: std::time::Duration::from_millis(1_500),
        }
    }
}

/// What a completed test observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelVerdict {
    /// Enough signal to be worth dictating with.
    Usable,
    /// Signal arrived, but below this module's proposed criterion.
    TooLow,
    /// Enough of the signal was clipped to call the level unsafe.
    Clipped,
    /// The test ran and delivered no usable sample. **Not** a fault verdict.
    NoSignal,
    /// Too little data arrived to classify at all. Says nothing about level.
    InsufficientData,
}

/// The numbers a finished test reports. No samples, ever.
#[derive(Debug, Clone, PartialEq)]
pub struct MicTestReport {
    pub device: Option<String>,
    pub requested: std::time::Duration,
    pub observed: std::time::Duration,
    pub samples: u64,
    pub frames: u64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    pub clipped_samples: u64,
    pub invalid_samples: u64,
    pub verdict: LevelVerdict,
}

/// Classifies a finished measurement.
///
/// `None` means "no verdict is honest yet": not enough data arrived to say
/// anything, which is different from saying the level was low.
pub fn classify(
    summary: &MicTestSummary,
    sample_rate: u32,
    thresholds: &MicTestThresholds,
) -> Option<LevelVerdict> {
    let observed = std::time::Duration::from_secs_f64(
        summary.valid_samples as f64 / f64::from(sample_rate.max(1)),
    );
    if observed < thresholds.min_observation {
        return None;
    }
    if summary.invalid_samples > 0
        && summary.invalid_samples as f64 / (summary.valid_samples + summary.invalid_samples) as f64
            > thresholds.max_invalid_fraction
    {
        return None;
    }
    // Digital silence across the whole observation is reported as "nothing
    // came in", not as "the level is low". Zeros are valid samples, so this is
    // a statement about the source, and it is deliberately weaker than any
    // statement about the hardware.
    if summary.valid_samples == 0 || summary.rms == 0.0 {
        return Some(LevelVerdict::NoSignal);
    }
    let clipped_share = summary.clipped_samples as f64 / summary.valid_samples as f64;
    if clipped_share >= thresholds.clip_fraction {
        return Some(LevelVerdict::Clipped);
    }
    if summary.rms < thresholds.low_level_rms {
        return Some(LevelVerdict::TooLow);
    }
    Some(LevelVerdict::Usable)
}

// ────────────────────────────────────────────────────── device ownership ──

/// Why the gate would not hand the device over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicGateRefusal {
    /// A dictation is live. It is not interrupted for a test.
    RecordingActive,
    /// A test already holds the device.
    TestAlreadyRunning,
    /// The program is on its way down.
    ShuttingDown,
}

/// Proof that a gate allowed a test to open the device.
///
/// Issued by a gate implementation, consumed by [`MicTestMachine::start`].
/// Deliberately neither `Copy` nor `Clone`: it is a single-use token, so
/// handing the device over twice is not expressible. Without a permit there is
/// no way in, which is the whole point — a panel that forgot to ask cannot open
/// a microphone.
#[derive(Debug, PartialEq, Eq)]
pub struct MicTestPermit {
    _issued_by_gate: (),
}

impl MicTestPermit {
    /// For gate implementations and tests only. UI code asks the gate.
    pub(crate) fn issue() -> Self {
        Self {
            _issued_by_gate: (),
        }
    }
}

/// The gate the central owner must implement. Not implemented in this stage:
/// the real decision needs the recorder's liveness, which lives in a file this
/// package does not own.
pub trait MicUseGate {
    /// `Ok(permit)` only when the device is free. Must not open anything.
    fn try_begin_test(&self) -> Result<MicTestPermit, MicGateRefusal>;
}

/// A request to stop the test and hand the device back.
///
/// The id is monotonic and never reused, which is what makes a late
/// acknowledgement from an older test impossible to confuse with a current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MicReleaseTicket {
    request_id: u64,
}

impl MicReleaseTicket {
    pub fn id(&self) -> u64 {
        self.request_id
    }
}

/// The central owner's confirmation that the device is actually free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicReleaseAck {
    request_id: u64,
    released: bool,
}

impl MicReleaseAck {
    /// A positive confirmation. `released: false` is representable on purpose:
    /// a failed release must not be able to unlock recording.
    pub fn released(request_id: u64) -> Self {
        Self {
            request_id,
            released: true,
        }
    }

    pub fn failed(request_id: u64) -> Self {
        Self {
            request_id,
            released: false,
        }
    }

    pub fn is_released(&self) -> bool {
        self.released
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipError {
    /// A release was confirmed while nothing is being held.
    NothingToRelease,
    /// The acknowledgement belongs to a different or older request.
    StaleTicket,
    /// The owner reported the device is not free.
    ReleaseFailed,
}

// ───────────────────────────────────────────────── device ownership state ──

/// Who holds the device, as a state of its own.
///
/// This is deliberately **not** the test's phase. A test can finish, be
/// cancelled or fail while the device is still open, because releasing it is
/// the central owner's job and only the owner's confirmation ends the
/// handover. Collapsing the two is what let a cancelled test hand the device to
/// the recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceOwnership {
    /// Nobody has asked for the device.
    Free,
    /// A test holds it. Nobody has been asked to release it yet.
    Held {
        request_id: Option<MicReleaseTicket>,
    },
    /// Release asked for, not yet confirmed. Recording is shut.
    Releasing { request_id: MicReleaseTicket },
    /// The owner confirmed the release. The handover is over.
    Released { request_id: MicReleaseTicket },
}

impl DeviceOwnership {
    /// Whether the recorder may open the device right now.
    pub fn allows_recording(self) -> bool {
        matches!(self, Self::Free | Self::Released { .. })
    }

    /// Whether somebody still owes the owner a release.
    pub fn holds_device(self) -> bool {
        matches!(self, Self::Held { .. } | Self::Releasing { .. })
    }

    /// The outstanding request, if one exists.
    pub fn outstanding_request(self) -> Option<MicReleaseTicket> {
        match self {
            Self::Held { request_id } => request_id,
            Self::Releasing { request_id } => Some(request_id),
            Self::Free | Self::Released { .. } => None,
        }
    }
}

/// What pressing record does right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordPressDecision {
    /// Nothing of ours holds the device.
    StartRecording,
    /// Stop the test and release first. Recording waits for the confirmation.
    StopTestThenConfirm { ticket: MicReleaseTicket },
}

// ────────────────────────────────────────────────────────────── state model ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicTestCancel {
    UserPressedStop,
    RecordKeyTookOver,
    PanelClosed,
    AppExit,
}

impl MicTestCancel {
    /// A cancelled run is not a result, whatever the reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserPressedStop => "user stopped the test",
            Self::RecordKeyTookOver => "record key took the device over",
            Self::PanelClosed => "panel closed",
            Self::AppExit => "application exiting",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicTestError {
    /// No device, or the chosen one is gone.
    NoDevice,
    /// The source refused to start (open failed, busy, unsupported format).
    SourceError { detail: String },
    /// The source went quiet for longer than the allowed gap.
    DeviceLost,
    /// Too much of the received data was unusable to say anything about level.
    UnusableInput { invalid_samples: u64 },
}

impl MicTestError {
    pub fn as_str(&self) -> &str {
        match self {
            Self::NoDevice => "no input device",
            Self::SourceError { detail } => detail,
            Self::DeviceLost => "no data arrived",
            Self::UnusableInput { .. } => "input data unusable",
        }
    }
}

/// The five phases. Only `Finished` carries a verdict — a cancelled run leaves
/// counts behind, never a conclusion.
#[derive(Debug, Clone, PartialEq)]
pub enum MicTestPhase {
    Ready,
    Running {
        device: Option<String>,
        budget: std::time::Duration,
        elapsed: std::time::Duration,
    },
    Finished(MicTestReport),
    Cancelled {
        reason: MicTestCancel,
        observed: Option<MicTestSummary>,
    },
    Failed(MicTestError),
}

/// The mic test, as a state machine over injected time.
///
/// Not `Clone`, on purpose: it may be holding a single-use handover from the
/// gate, and a copied machine would be a second owner of one device.
#[derive(Debug)]
pub struct MicTestMachine {
    phase: MicTestPhase,
    analyzer: Option<MicTestAnalyzer>,
    /// The handover we were given. Kept until the owner confirms the release,
    /// because the gate handed the device over and only the gate may say it is
    /// back.
    permit: Option<MicTestPermit>,
    ownership: DeviceOwnership,
    thresholds: MicTestThresholds,
    /// Elapsed time at which usable data last arrived; `None` until the first
    /// sample, so the gap is measured from the start of a silent test too.
    last_data_at: Option<std::time::Duration>,
    next_request_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOutcome {
    /// Nothing to do: no test is running.
    Idle,
    StillRunning,
    Finished,
    Failed(MicTestError),
}

impl MicTestMachine {
    pub fn new(thresholds: MicTestThresholds) -> Self {
        Self {
            phase: MicTestPhase::Ready,
            analyzer: None,
            permit: None,
            ownership: DeviceOwnership::Free,
            thresholds,
            last_data_at: None,
            next_request_id: 1,
        }
    }

    pub fn phase(&self) -> &MicTestPhase {
        &self.phase
    }

    pub fn analyzer(&self) -> Option<&MicTestAnalyzer> {
        self.analyzer.as_ref()
    }

    pub fn thresholds(&self) -> &MicTestThresholds {
        &self.thresholds
    }

    /// Time already fed into the running test, as the caller supplied it.
    pub fn elapsed(&self) -> std::time::Duration {
        match &self.phase {
            MicTestPhase::Running { elapsed, .. } => *elapsed,
            _ => std::time::Duration::ZERO,
        }
    }

    /// The verdict of a finished test. `None` for every other phase — in
    /// particular `None` after a cancellation, by design.
    pub fn report(&self) -> Option<&MicTestReport> {
        match &self.phase {
            MicTestPhase::Finished(report) => Some(report),
            _ => None,
        }
    }

    /// Whether we are currently holding a handover from the gate.
    pub fn holds_device(&self) -> bool {
        self.ownership.holds_device()
    }

    /// The device-ownership state, which is **not** the test's phase.
    pub fn ownership(&self) -> DeviceOwnership {
        self.ownership
    }

    /// The release the owner still owes us, if any.
    pub fn outstanding_release(&self) -> Option<MicReleaseTicket> {
        self.ownership.outstanding_request()
    }

    /// Starts a test. The permit is the only way in; a refusal leaves the
    /// machine exactly as it was.
    ///
    /// The permit is taken by value and kept for the run. A duplicate start or
    /// a rejected rate returns before it is stored, so the handover goes
    /// straight back to the gate instead of being held by a test that never
    /// began.
    pub fn start(
        &mut self,
        permit: MicTestPermit,
        device: Option<String>,
        sample_rate: u32,
        budget: std::time::Duration,
    ) -> Result<(), MicStartRefusal> {
        if matches!(self.phase, MicTestPhase::Running { .. }) {
            return Err(MicStartRefusal::TestAlreadyRunning);
        }
        if self.ownership.holds_device() {
            return Err(MicStartRefusal::DeviceNotReleased);
        }
        let analyzer = match MicTestAnalyzer::new(sample_rate) {
            Ok(analyzer) => analyzer,
            Err(MicTestSetupError::InvalidSampleRate { rate }) => {
                return Err(MicStartRefusal::InvalidSampleRate { rate })
            }
        };

        self.analyzer = Some(analyzer);
        self.permit = Some(permit);
        self.ownership = DeviceOwnership::Held { request_id: None };
        self.last_data_at = None;
        self.phase = MicTestPhase::Running {
            device,
            budget,
            elapsed: std::time::Duration::ZERO,
        };
        Ok(())
    }

    /// Asks the owner to release the device, unless a request is already
    /// outstanding. The id is minted once and never reused, so an
    /// acknowledgement can be matched to this exact request and to no other.
    fn request_release(&mut self) -> MicReleaseTicket {
        match self.ownership {
            DeviceOwnership::Held {
                request_id: Some(ticket),
            } => ticket,
            DeviceOwnership::Releasing { request_id } => request_id,
            DeviceOwnership::Held { request_id: None } => {
                let ticket = MicReleaseTicket {
                    request_id: self.next_request_id,
                };
                self.next_request_id += 1;
                self.ownership = DeviceOwnership::Releasing { request_id: ticket };
                ticket
            }
            DeviceOwnership::Free | DeviceOwnership::Released { .. } => {
                // Nothing is held, so there is nothing to ask for. Rather than
                // invent a confirmation the model mints an id it will never
                // wait for: it is honest about refusing to confirm.
                MicReleaseTicket {
                    request_id: self.next_request_id,
                }
            }
        }
    }

    /// Feeds a frame to the running test. Outside `Running` it is ignored.
    pub fn push_frame(&mut self, frame: &[f32], now: std::time::Duration) -> FrameOutcome {
        let Some(analyzer) = self.analyzer.as_mut() else {
            return FrameOutcome::Empty;
        };
        let outcome = analyzer.push(frame);
        if matches!(outcome, FrameOutcome::Accepted { valid, .. } if valid > 0) {
            self.last_data_at = Some(now);
        }
        outcome
    }

    /// Advances the test with time supplied by the caller.
    pub fn tick(&mut self, now: std::time::Duration) -> TickOutcome {
        let MicTestPhase::Running {
            budget,
            elapsed,
            device,
        } = &self.phase
        else {
            return TickOutcome::Idle;
        };
        let budget = *budget;
        let device = device.clone();
        let now = now.max(*elapsed);

        // Unusable input is a failure even at the budget: a measurement of
        // garbage would be a measurement of nothing.
        if let Some(analyzer) = self.analyzer.as_ref() {
            if analyzer.invalid_fraction() > self.thresholds.max_invalid_fraction {
                let invalid = analyzer.invalid_samples();
                return self.fail(MicTestError::UnusableInput {
                    invalid_samples: invalid,
                });
            }
        }
        // Budget: a test that reaches its limit reports what it saw.
        if now >= budget {
            return self.finish(device, budget);
        }
        let since = match self.last_data_at {
            Some(at) => now.saturating_sub(at),
            None => now,
        };
        if since >= self.thresholds.no_signal_gap {
            return self.fail(MicTestError::DeviceLost);
        }

        if let MicTestPhase::Running { elapsed, .. } = &mut self.phase {
            *elapsed = now;
        }
        TickOutcome::StillRunning
    }

    fn finish(&mut self, device: Option<String>, requested: std::time::Duration) -> TickOutcome {
        let Some(analyzer) = self.analyzer.as_ref() else {
            return self.fail(MicTestError::DeviceLost);
        };
        let summary = analyzer.summary();
        let sample_rate = analyzer.sample_rate();
        // No verdict is invented when the observation is too thin to support
        // one; the report says so instead of guessing.
        let verdict = classify(&summary, sample_rate, &self.thresholds)
            .unwrap_or(LevelVerdict::InsufficientData);
        let observed = std::time::Duration::from_secs_f64(
            summary.valid_samples as f64 / f64::from(sample_rate),
        );
        self.phase = MicTestPhase::Finished(MicTestReport {
            device,
            requested,
            observed,
            samples: summary.valid_samples,
            frames: summary.frames,
            peak_dbfs: dbfs(summary.peak_bits),
            rms_dbfs: dbfs(summary.rms),
            clipped_samples: summary.clipped_samples,
            invalid_samples: summary.invalid_samples,
            verdict,
        });
        self.analyzer = None;
        self.last_data_at = None;
        // The phase is over; the device is not. The release is now owed, and
        // whoever ends the test must let the owner know.
        let _ = self.request_release();
        TickOutcome::Finished
    }

    fn fail(&mut self, error: MicTestError) -> TickOutcome {
        self.phase = MicTestPhase::Failed(error.clone());
        self.analyzer = None;
        self.last_data_at = None;
        let _ = self.request_release();
        TickOutcome::Failed(error)
    }

    /// Reports that the source itself failed (no device, open failed, busy).
    pub fn fail_source(&mut self, error: MicTestError) -> TickOutcome {
        self.fail(error)
    }

    /// Stops a running test. No verdict is produced, whatever was collected.
    ///
    /// The counts come back only if a frame ever arrived: "we collected
    /// nothing" is reported as no observation at all, not as an empty one.
    pub fn stop(&mut self, reason: MicTestCancel) -> Option<MicTestSummary> {
        if !matches!(self.phase, MicTestPhase::Running { .. }) {
            return None;
        }
        let observed = self
            .analyzer
            .as_ref()
            .filter(|a| a.frames() > 0)
            .map(MicTestAnalyzer::summary);
        self.phase = MicTestPhase::Cancelled { reason, observed };
        self.analyzer = None;
        self.last_data_at = None;
        // Cancelling is not releasing. The handover survives until the owner
        // says the device is back.
        let _ = self.request_release();
        observed
    }

    /// The panel went away.
    pub fn on_panel_closed(&mut self) -> Option<MicTestSummary> {
        self.stop(MicTestCancel::PanelClosed)
    }

    /// The program is going down.
    pub fn on_app_exit(&mut self) -> Option<MicTestSummary> {
        self.stop(MicTestCancel::AppExit)
    }

    /// The record key was pressed.
    ///
    /// While a test holds the device this only *requests* a release and hands
    /// back a ticket. Recording is not allowed to start until the owner
    /// confirms with the matching acknowledgement — that is the whole contract.
    /// Pressing again while a request is outstanding returns the same ticket
    /// rather than stacking a second one.
    pub fn on_record_pressed(&mut self) -> RecordPressDecision {
        if self.ownership.allows_recording() {
            return RecordPressDecision::StartRecording;
        }
        RecordPressDecision::StopTestThenConfirm {
            ticket: self.request_release(),
        }
    }

    /// Applies the owner's answer. Only a matching, positive acknowledgement
    /// ends the handover and unlocks recording.
    ///
    /// The model never produces an acknowledgement of its own; it can only
    /// receive one. A test that is still running is cancelled by the takeover.
    /// A test that has already finished, failed or been cancelled **keeps the
    /// phase it earned**, because a late acknowledgement must not rewrite it.
    pub fn confirm_release(&mut self, ack: &MicReleaseAck) -> Result<(), OwnershipError> {
        let DeviceOwnership::Releasing { request_id } = self.ownership else {
            return Err(OwnershipError::NothingToRelease);
        };
        if ack.request_id != request_id.id() {
            return Err(OwnershipError::StaleTicket);
        }
        if !ack.released {
            return Err(OwnershipError::ReleaseFailed);
        }
        self.ownership = DeviceOwnership::Released { request_id };
        // The handover really is over, so the permit goes back to the gate.
        self.permit = None;
        if matches!(self.phase, MicTestPhase::Running { .. }) {
            self.stop(MicTestCancel::RecordKeyTookOver);
        }
        Ok(())
    }

    /// Whether the recorder may open the device right now. `false` from the
    /// moment a test takes the device until a matching acknowledgement arrives.
    pub fn may_start_recording(&self) -> bool {
        self.ownership.allows_recording()
    }
}
