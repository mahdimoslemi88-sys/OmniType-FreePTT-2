//! The microphone test, as the user actually meets it.
//!
//! [`crate::audio::diagnostics`] decides *what* a level means and
//! [`crate::audio::gate`] decides *whether the device may be opened at all*.
//! Neither of them can run a capture, so this module owns the three things
//! that turn a measurement into a feature:
//!
//! * the **capture thread**, which opens its own [`AudioCapture`] on the chosen
//!   device and ships frames across a channel;
//! * the **pump**, which drains that channel into the state machine with real
//!   elapsed time, so a frame that arrives late is still accounted for;
//! * the **render**, which shows the device, the level and the verdict.
//!
//! Three promises this panel keeps, because the roadmap makes them acceptance
//! criteria rather than nice-to-haves:
//!
//! * **Nothing is written to disk.** There is no sample file, no "save" button
//!   and no default location, so there is nothing for a user to opt into by
//!   accident.
//! * **Nothing is typed anywhere.** The panel talks to the analyzer and to the
//!   gate; it never touches a text sink and never sends a transcription result.
//! * **Nothing is kept.** No sample vector, no summary beyond the numbers shown,
//!   and the capture thread is joined before the panel reports a phase.
//!
//! The panel deliberately reuses the *existing* `[audio]` settings for device
//! choice rather than inventing a second place to configure input: a diagnostic
//! that tests a different device than the app records from would be a test of
//! nothing.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use super::text::format_persian_display;
use super::theme::{callout, manager_card, palette, rtl_form_row, CalloutKind};

use crate::audio::diagnostics::{
    LevelVerdict, MicReleaseAck, MicStartRefusal, MicTestCancel, MicTestError, MicTestMachine,
    MicTestPhase, MicTestThresholds, MicUseGate,
};
use crate::audio::gate::LiveMicGate;
use crate::audio::{AudioCapture, CaptureConfig, InputDeviceInfo};
use crate::config::settings::Settings;

/// How long a test runs before it reports what it saw.
///
/// Three seconds: long enough for someone to say a word or blow on the mic,
/// short enough that nobody wonders whether the app hung. Deliberately a
/// constant rather than a setting — a diagnostic whose own controls need
/// calibrating is not a diagnostic.
pub const TEST_BUDGET: Duration = Duration::from_secs(3);

/// How often the capture thread wakes to move samples out of the device.
const POLL: Duration = Duration::from_millis(20);

/// Frames drained per rendered frame.
///
/// A bound, not a batch size: on a slow frame the channel could otherwise hold
/// enough audio to make the level reading describe the past rather than now.
/// Dropping the excess is the right direction to be wrong in — a slightly
/// stale level, never a stalled UI.
const MAX_FRAMES_PER_PUMP: usize = 32;

/// The thread that owns the device while a test runs.
///
/// Dropped (and joined) whenever the panel stops drawing the test, so a closed
/// panel cannot leave a microphone open in the background.
struct CaptureThread {
    stop: Arc<std::sync::atomic::AtomicBool>,
    frames: Receiver<Vec<f32>>,
    /// Read after the join, so the main thread never touches the ring buffer.
    rate: u32,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl CaptureThread {
    /// Opens the device and starts shipping frames.
    ///
    /// Returns the channel only when the stream is genuinely running: a caller
    /// that gets an `Err` must not start a test, or the verdict would describe
    /// a device that never produced anything.
    fn start(device: Option<String>, sample_rate: u32, channels: u16, gain_db: f32) -> Result<Self, String> {
        let config = CaptureConfig {
            sample_rate,
            channels,
            device_name: device.clone(),
            gain_db,
            ..CaptureConfig::default()
        };
        // Opened here, on the GUI thread, so a failure is a refusal the user
        // reads immediately rather than a test that quietly measures silence.
        let capture = AudioCapture::new(&config).map_err(|e| format!("{e:#}"))?;
        capture.start().map_err(|e| format!("{e:#}"))?;
        let rate = capture.pipeline_sample_rate();

        let (tx, frames) = std::sync::mpsc::channel::<Vec<f32>>();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_thread = stop.clone();
        let handle = std::thread::Builder::new()
            .name("mic-test-capture".into())
            .spawn(move || {
                while !stop_thread.load(std::sync::atomic::Ordering::SeqCst) {
                    let frame = capture.take_audio();
                    if !frame.is_empty() && tx.send(frame).is_err() {
                        // The panel is gone; nothing left to measure for.
                        break;
                    }
                    std::thread::sleep(POLL);
                }
                // Leave the device as we found it, whatever ended the loop.
                let _ = capture.stop();
            })
            .map_err(|e| format!("{e}"))?;

        Ok(Self {
            stop,
            frames,
            rate,
            handle: Some(handle),
        })
    }

    /// Stops the thread and waits for it, so the device is released before the
    /// caller is told the test has ended.
    fn shutdown(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for CaptureThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Everything the mic-test tab owns.
pub struct MicTestPanelState {
    /// The measurement itself. Never cloned: it may hold a single-use handover
    /// from the gate, and a copy would be a second owner of one device.
    machine: MicTestMachine,
    thread: Option<CaptureThread>,
    /// Real time the current run began, so elapsed time is measured rather than
    /// counted by frames — a slow frame must not extend the test's budget.
    started: Option<Instant>,
    /// Input devices, refreshed when the panel opens.
    devices: Vec<InputDeviceInfo>,
    devices_loaded: bool,
    /// Why the last attempt to start was refused, in the user's words.
    start_error: Option<String>,
    /// A line to say once: what the verdict is and what it is not.
    notice: Option<String>,
}

impl Default for MicTestPanelState {
    fn default() -> Self {
        Self::new()
    }
}

impl MicTestPanelState {
    pub fn new() -> Self {
        Self {
            machine: MicTestMachine::new(MicTestThresholds::default()),
            thread: None,
            started: None,
            devices: Vec::new(),
            devices_loaded: false,
            start_error: None,
            notice: None,
        }
    }

    fn running(&self) -> bool {
        matches!(self.machine.phase(), MicTestPhase::Running { .. })
    }

    /// Releases the device and tells the gate it is free again.
    ///
    /// Both halves are required and neither implies the other: the machine
    /// needs the acknowledgement to drop its own handover, and the gate is the
    /// only thing allowed to say the device is available again.
    fn settle_release(&mut self, gate: &LiveMicGate) {
        if let Some(ticket) = self.machine.outstanding_release() {
            let ack = MicReleaseAck::released(ticket.id());
            if self.machine.confirm_release(&ack).is_ok() {
                gate.confirm_release();
            }
        }
        if let Some(mut thread) = self.thread.take() {
            thread.shutdown();
        }
        self.started = None;
    }

    /// Starts a test, or explains why it cannot start right now.
    ///
    /// Order matters and is the whole contract: ask the **gate** first, then
    /// open the **device**. Doing it the other way round would open a stream and
    /// then refuse to use it, which is exactly the "device busy" failure a user
    /// cannot do anything about.
    fn start(&mut self, settings: &Settings, gate: &LiveMicGate) {
        if self.running() {
            return;
        }
        self.start_error = None;
        self.notice = None;

        let permit = match gate.try_begin_test() {
            Ok(permit) => permit,
            Err(refusal) => {
                self.start_error = Some(match refusal {
                    crate::audio::diagnostics::MicGateRefusal::RecordingActive => {
                        "یک ضبط در جریان است. آزمون میکروفون در حین ضبط شروع نمی‌شود.".to_string()
                    }
                    crate::audio::diagnostics::MicGateRefusal::TestAlreadyRunning => {
                        "آزمون دیگری در حال اجراست.".to_string()
                    }
                    crate::audio::diagnostics::MicGateRefusal::ShuttingDown => {
                        "برنامه در حال بستن است.".to_string()
                    }
                });
                return;
            }
        };

        let device = chosen_device(&settings.audio.device);
        let thread = match CaptureThread::start(
            device.clone(),
            settings.audio.sample_rate,
            settings.audio.channels,
            settings.audio.gain_db,
        ) {
            Ok(thread) => thread,
            Err(detail) => {
                // The gate handed the device over and we could not open it, so
                // the handover goes straight back instead of being held by a
                // test that never began.
                gate.confirm_release();
                self.start_error = Some(format!("باز کردن دستگاه ورودی ممکن نشد: {detail}"));
                return;
            }
        };

        let rate = thread.rate;
        if let Err(refusal) = self.machine.start(
            permit,
            device,
            rate,
            TEST_BUDGET,
        ) {
            gate.confirm_release();
            self.start_error = Some(match refusal {
                MicStartRefusal::InvalidSampleRate { rate } => {
                    format!("نرخ نمونه‌برداری نامعتبر است: {rate} Hz")
                }
                MicStartRefusal::TestAlreadyRunning => "آزمون دیگری در حال اجراست.".to_string(),
                MicStartRefusal::DeviceNotReleased => {
                    "دستگاه هنوز آزاد نشده است.".to_string()
                }
            });
            return;
        }

        self.thread = Some(thread);
        self.started = Some(Instant::now());
    }

    /// Moves whatever the capture thread has produced into the measurement.
    ///
    /// Called once per rendered frame. Returns whether the panel needs a
    /// repaint, so a running test keeps updating without the user moving the
    /// mouse.
    fn pump(&mut self, gate: &LiveMicGate) -> bool {
        let Some(started) = self.started else {
            return false;
        };
        let mut moved = false;
        if let Some(thread) = self.thread.as_ref() {
            for _ in 0..MAX_FRAMES_PER_PUMP {
                match thread.frames.try_recv() {
                    Ok(frame) => {
                        self.machine.push_frame(&frame, started.elapsed());
                        moved = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }

        match self.machine.tick(started.elapsed()) {
            crate::audio::diagnostics::TickOutcome::Idle => {}
            crate::audio::diagnostics::TickOutcome::StillRunning => return true,
            crate::audio::diagnostics::TickOutcome::Finished => {
                self.settle_release(gate);
                return true;
            }
            crate::audio::diagnostics::TickOutcome::Failed(error) => {
                self.settle_release(gate);
                self.start_error = Some(describe_error(&error));
                return true;
            }
        }
        moved
    }

    /// Stops a running test on request. A cancelled run produces counts but no
    /// verdict, which is what the user asked for by pressing stop.
    fn stop(&mut self, gate: &LiveMicGate) {
        if !self.running() {
            return;
        }
        self.machine.stop(MicTestCancel::UserPressedStop);
        self.settle_release(gate);
        self.notice = Some("آزمون لغو شد؛ نتیجه‌ای ثبت نشد.".to_string());
    }

    /// Forgets a finished, failed or cancelled run so the panel is ready again.
    ///
    /// The thresholds carry over: they belong to the panel, not to one run, and
    /// resetting the verdict must not silently reset the criteria it was judged
    /// against.
    fn reset(&mut self) {
        self.notice = None;
        self.start_error = None;
        self.machine = MicTestMachine::new(*self.machine.thresholds());
    }

    /// Called when the panel goes away. Releases the device rather than leaking
    /// it: a user who closed the window must not find the microphone busy later.
    pub fn on_hidden(&mut self, gate: &LiveMicGate) {
        if self.running() {
            self.machine.on_panel_closed();
        }
        self.settle_release(gate);
    }
}

/// The device name the settings mean, with `"default"` translated to `None`.
///
/// Shared with [`crate::audio::CaptureConfig`]'s own convention so the test and
/// the recorder cannot end up on different devices because of a spelling
/// difference.
fn chosen_device(configured: &str) -> Option<String> {
    let trimmed = configured.trim();
    if trimmed.is_empty() || trimmed == "default" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn describe_error(error: &MicTestError) -> String {
    match error {
        MicTestError::NoDevice => "دستگاه ورودی در دسترس نیست.".to_string(),
        MicTestError::SourceError { detail } => format!("منبع صدا خطا داد: {detail}"),
        MicTestError::DeviceLost => {
            "برای مدتی هیچ صدایی نرسید. این به‌تنهایی یعنی میکروفون خراب نیست؛ ممکن است انتخاب دستگاه اشتباه باشد."
                .to_string()
        }
        MicTestError::UnusableInput { invalid_samples } => {
            format!("دادهٔ ورودی قابل استفاده نبود ({invalid_samples} نمونه).")
        }
    }
}

/// A short Persian line for a verdict, phrased as what it means for the user
/// rather than as a name for a branch in the code.
fn verdict_line(verdict: LevelVerdict) -> &'static str {
    match verdict {
        LevelVerdict::Usable => "سطح صدا مناسب است؛ دیکته کردن باید کار کند.",
        LevelVerdict::TooLow => "صدا می‌آید ولی ضعیف است.gain_db یا فاصلهٔ میکروفون را بررسی کنید.",
        LevelVerdict::Clipped => "بخش زیادی از صدا بریده شده است.gain_db را کم کنید.",
        LevelVerdict::NoSignal => "در این بازه صدایی نرسید. سکوت به‌تنهایی دلیل خرابی میکروفون نیست.",
        LevelVerdict::InsufficientData => "دادهٔ کافی نرسید تا دربارهٔ سطح صدا چیزی گفته شود.",
    }
}

fn verdict_kind(verdict: LevelVerdict) -> CalloutKind {
    match verdict {
        LevelVerdict::Usable => CalloutKind::Success,
        LevelVerdict::TooLow | LevelVerdict::Clipped => CalloutKind::Warning,
        LevelVerdict::NoSignal | LevelVerdict::InsufficientData => CalloutKind::Info,
    }
}

/// Renders the tab. A free function taking the disjoint fields it may touch, in
/// the same style as the other panels.
pub fn render(
    ui: &mut egui::Ui,
    state: &mut MicTestPanelState,
    settings: &Settings,
    gate: Arc<LiveMicGate>,
) {
    // Keep the measurement moving even when the user is not touching anything:
    // a level meter that only ticks on mouse movement is not a meter.
    if state.pump(&gate) {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }

    manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        ui.label(
            egui::RichText::new(format_persian_display("آزمون میکروفون"))
                .size(13.0)
                .strong()
                .color(palette::TEXT_SECTION),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format_persian_display(
                "چند ثانیه صحبت کنید تا سطح صدای دستگاه ورودی سنجیده شود. هیچ فایلی ذخیره نمی‌شود و هیچ متنی جایی تایپ نمی‌شود.",
            ))
            .size(11.0)
            .color(palette::TEXT_FAINT),
        );
        ui.add_space(8.0);

        // Device row: the device the test runs on is the device the app records
        // from, so it is shown rather than silently assumed.
        rtl_form_row(ui, "دستگاه ورودی:", 105.0, |ui| {
            if !state.devices_loaded {
                state.devices = crate::audio::list_input_devices().unwrap_or_default();
                state.devices_loaded = true;
            }
            let label = device_label(&settings.audio.device, &state.devices);
            ui.label(
                egui::RichText::new(format_persian_display(&label))
                    .size(11.5)
                    .color(palette::TEXT_PRIMARY),
            );
        });
        ui.add_space(8.0);

        // Buttons: exactly one of start/stop is live at a time, so the panel
        // cannot ask for a test it already has.
        ui.horizontal(|ui| {
            if state.running() {
                if ui
                    .button(egui::RichText::new(format_persian_display("توقف")).size(12.0))
                    .clicked()
                {
                    state.stop(&gate);
                }
                ui.add_space(6.0);
                render_level_meter(ui, state);
            } else {
                if ui
                    .button(
                        egui::RichText::new(format_persian_display("شروع آزمون"))
                            .size(12.0)
                            .strong(),
                    )
                    .clicked()
                {
                    state.start(settings, &gate);
                }
                match state.machine.phase() {
                    MicTestPhase::Finished(report) => {
                        ui.add_space(8.0);
                        render_report(ui, report);
                    }
                    MicTestPhase::Cancelled { reason, .. } => {
                        // A cancelled run has counts but no conclusion, and
                        // saying so is the whole point of cancelling.
                        ui.add_space(6.0);
                        callout(
                            ui,
                            CalloutKind::Info,
                            &format!("آزمون بدون نتیجه پایان یافت ({})", reason.as_str()),
                        );
                    }
                    MicTestPhase::Failed(error) => {
                        ui.add_space(6.0);
                        callout(ui, CalloutKind::Warning, &describe_error(error));
                    }
                    _ => {}
                }
                if !matches!(state.machine.phase(), MicTestPhase::Ready) {
                    ui.add_space(6.0);
                    if ui.button(egui::RichText::new("پاک کردن").size(11.0)).clicked() {
                        state.reset();
                    }
                }
            }
        });
        ui.add_space(8.0);

        if let Some(err) = &state.start_error {
            callout(ui, CalloutKind::Warning, err);
        } else if let Some(notice) = &state.notice {
            callout(ui, CalloutKind::Info, notice);
        }
    });
}

/// The configured device, resolved to a real name when one can be found.
///
/// A name that is not in the list is reported as **configured but not
/// present** rather than silently replaced by the default: a user who picked a
/// headset and unplugged it needs to be told that, not handed a microphone they
/// did not ask for.
fn device_label(configured: &str, devices: &[InputDeviceInfo]) -> String {
    match chosen_device(configured) {
        None => "پیش‌فرض سیستم".to_string(),
        Some(name) => {
            if devices.iter().any(|d| d.name == name) {
                name
            } else if devices.is_empty() {
                format!("{name} (فهرست دستگاه‌ها در دسترس نیست)")
            } else {
                format!("{name} (در حال حاضر متصل نیست)")
            }
        }
    }
}

/// A plain bar for the live level. Peak is the loudest recent sample and RMS is
/// the average, because "loud" and "usable" are different questions and one
/// number cannot answer both.
fn render_level_meter(ui: &mut egui::Ui, state: &MicTestPanelState) {
    let Some(analyzer) = state.machine.analyzer() else {
        return;
    };
    // dBFS floors at 1e-6; anything quieter is below what the display can
    // show, so the bar starts there rather than at a number that lies.
    let floor = -60.0_f32;
    let db_to_fraction = |db: f32| ((db - floor) / -floor).clamp(0.0, 1.0);
    ui.vertical(|ui| {
        ui.add(egui::ProgressBar::new(db_to_fraction(analyzer.peak_dbfs())).desired_width(120.0));
        ui.add(egui::ProgressBar::new(db_to_fraction(analyzer.rms_dbfs())).desired_width(120.0));
        ui.label(
            egui::RichText::new(format!(
                "بیشینه {:.0} dB · میانگین {:.0} dB",
                analyzer.peak_dbfs(),
                analyzer.rms_dbfs()
            ))
            .size(10.5)
            .color(palette::TEXT_FAINT),
        );
    });
}

/// The finished measurement: the verdict first, the numbers that produced it
/// underneath so a surprising verdict can be argued with.
fn render_report(ui: &mut egui::Ui, report: &crate::audio::diagnostics::MicTestReport) {
    callout(ui, verdict_kind(report.verdict), verdict_line(report.verdict));
    ui.add_space(4.0);
    let detail = format!(
        "دستگاه: {}\nنمونه: {} در {:.1} ثانیه\nبیشینه: {:.0} dB · میانگین: {:.0} dB\nبریده‌شده: {} · نامعتبر: {}",
        report.device.as_deref().unwrap_or("پیش‌فرض سیستم"),
        report.samples,
        report.observed.as_secs_f64(),
        report.peak_dbfs,
        report.rms_dbfs,
        report.clipped_samples,
        report.invalid_samples,
    );
    ui.label(
        egui::RichText::new(format_persian_display(&detail))
            .size(10.5)
            .color(palette::TEXT_FAINT),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with_device(device: &str) -> Settings {
        let mut settings = Settings::default();
        settings.audio.device = device.to_string();
        settings
    }

    /// The convention has to be shared with the recorder or the test measures a
    /// different device than the app records from — which would make a "usable"
    /// verdict a statement about hardware the user never chose.
    #[test]
    fn the_blank_and_default_names_mean_the_system_device() {
        assert_eq!(chosen_device("default"), None);
        assert_eq!(chosen_device(""), None);
        assert_eq!(chosen_device("   "), None);
        assert_eq!(
            chosen_device("Headset Microphone"),
            Some("Headset Microphone".to_string())
        );
    }

    #[test]
    fn the_label_reports_the_system_device_when_none_is_named() {
        assert_eq!(device_label("default", &[]), "پیش‌فرض سیستم");
    }

    /// The case the roadmap calls out: a removed device must produce an
    /// understandable error, which starts with naming the fact that the
    /// configured device is not currently present — not with quietly measuring
    /// the default microphone instead.
    #[test]
    fn a_configured_device_that_is_not_present_says_so() {
        let devices = vec![device("Built-in Microphone")];
        let label = device_label("Headset", &devices);
        assert!(label.contains("Headset"), "names the device: {label}");
        assert!(
            label.contains("متصل نیست"),
            "says it is absent: {label}"
        );
    }

    #[test]
    fn a_present_device_is_named_without_a_caveat() {
        let devices = vec![device("Headset")];
        assert_eq!(device_label("Headset", &devices), "Headset");
    }

    /// A minimal stand-in; only the name is read by the code under test.
    fn device(name: &str) -> InputDeviceInfo {
        InputDeviceInfo {
            name: name.to_string(),
            default_sample_rate: 48_000,
            max_channels: 2,
        }
    }

    /// Silence is not a verdict about the hardware. The wording has to keep
    /// those two apart, because the alternative is a user replacing a working
    /// microphone.
    #[test]
    fn no_signal_is_not_reported_as_a_broken_microphone() {
        let line = verdict_line(LevelVerdict::NoSignal);
        assert!(
            line.contains("دلیل خرابی میکروفون نیست"),
            "the line must disclaim a hardware fault: {line}"
        );
        assert!(
            !line.contains("خراب است"),
            "it must not assert one either: {line}"
        );
    }

    #[test]
    fn a_usable_level_is_shown_as_success_not_as_a_warning() {
        assert_eq!(verdict_kind(LevelVerdict::Usable), CalloutKind::Success);
        assert_eq!(verdict_kind(LevelVerdict::Clipped), CalloutKind::Warning);
        assert_eq!(verdict_kind(LevelVerdict::NoSignal), CalloutKind::Info);
    }

    #[test]
    fn a_missing_device_is_an_error_line_not_a_verdict() {
        let line = describe_error(&MicTestError::NoDevice);
        assert!(line.contains("در دسترس نیست"), "{line}");
        assert!(
            !line.contains("خراب"),
            "a missing device is not a broken device: {line}"
        );
    }

    /// The panel must never begin a test when the gate says a dictation owns
    /// the device — that refusal is the safety property the whole module is
    /// built around, and it is worth pinning at the panel level too.
    #[test]
    fn a_busy_gate_refuses_to_start_and_leaves_no_test_running() {
        use crate::state::{AppState, StatusChannel};

        let status = std::sync::Arc::new(StatusChannel::new("test"));
        let gate = crate::audio::gate::LiveMicGate::new(status.clone());
        status.set_state(AppState::Recording);

        let state = MicTestPanelState::new();
        // The real `start` would open a device; exercise the gate question on
        // its own so the test needs no microphone.
        assert!(
            gate.try_begin_test().is_err(),
            "a live dictation must block a test"
        );
        assert!(!state.running());
        assert!(!gate.test_is_running(), "a refusal reserves nothing");
    }

    /// A panel that was never opened holds nothing, asks for nothing, and
    /// closing it is not an error.
    #[test]
    fn a_fresh_panel_holds_no_device() {
        use crate::state::StatusChannel;

        let status = std::sync::Arc::new(StatusChannel::new("test"));
        let gate = crate::audio::gate::LiveMicGate::new(status);
        let mut state = MicTestPanelState::new();
        assert!(!state.running());
        assert!(!gate.test_is_running());

        state.on_hidden(&gate);
        assert!(
            state.machine.may_start_recording(),
            "closing an idle panel must not leave the device claimed"
        );
        assert!(!gate.test_is_running());
    }

    /// The settings the panel reads must be the ones the recorder reads, so a
    /// test can never end up on a device the app is not recording from.
    #[test]
    fn the_settings_object_is_usable_without_a_microphone() {
        let settings = settings_with_device("Headset");
        assert_eq!(settings.audio.device, "Headset");
        assert_eq!(chosen_device(&settings.audio.device).as_deref(), Some("Headset"));
    }
}
