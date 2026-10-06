//! Floating status overlay (egui/eframe): an AI-native, glassmorphic capsule
//! that reflects the current state (Idle / Recording / Processing / Typing),
//! animates live audio waveforms, and displays Persian transcripts crisply.
//!
//! # Layout
//!
//! This file is the *shell*: the [`OverlayApp`] state, the `eframe::App`
//! update loop, and the manager-window frame that hosts the four dashboard
//! panels. The parts that can be reasoned about on their own live in
//! submodules:
//!
//! - `text` — Persian shaping and caption wrapping; no app state.
//! - `theme` — the two compiled palettes and the shared widgets; no app
//!   state.
//! - `*_panel` — one module per dashboard tab, each owning exactly the
//!   fields its UI mutates.
//! - `toast` — the transcript card and its countdown bookkeeping.
//!
//! The split follows a measured rule, not taste: a module may only be lifted
//! out when the state it touches is disjoint from its neighbours'. That is
//! what keeps a panel change from being able to alter a different tab.

mod dict_fix_panel;
mod dict_panel;
mod engine_panel;
mod history_panel;
mod mic_test_panel;
mod profiles_panel;
mod review_panel;
mod settings_panel;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod testutil;
// `pub(crate)` for one caller outside this module: the orb reshapes the
// application-profile name it draws under itself with the same function the
// panels use, so a Persian name cannot be the one place that skips shaping.
pub(crate) mod text;
mod theme;
mod toast;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_phosphor::regular as ic;

use history_panel::HistoryItem;
use theme::*;

pub use text::format_persian_display;

use crate::asr::router::AsrRouter;
use crate::config::settings::Settings;
use crate::gui::flags::{DashboardFlags, Toggle};
use crate::gui::orb_idle_adapter;
use crate::gui::orb_idle_policy;
use crate::gui::orb::{Orb, OrbMode};
use crate::gui::preview_window;
use crate::gui::window_shape::{
    enforce_frameless_window, local_time_str, position_in_screen, register_main_hwnd,
    true_screen_size_px, MAIN_HWND,
};
use crate::hotkey::binding::HotkeyBinding;
use crate::hotkey::{HotkeyControl, HotkeyEvent};
use crate::processing::Dictionary;
use crate::state::{AppState, AppStatus};

/// Reads the idle-return settings out of the persisted config.
///
/// `0` is treated as "not chosen" and falls back to the policy's proposed
/// default rather than becoming a zero-length timeout. The two are different
/// things — a user who has never touched the setting and a user who set "return
/// instantly" are not the same request, and only the latter has no sensible
/// meaning, so it gets the default rather than a value that fights the pointer
/// every frame.
pub(crate) fn idle_settings_from(gui: &crate::config::settings::GuiSettings) -> orb_idle_policy::IdleReturnSettings {
    orb_idle_policy::IdleReturnSettings {
        enabled: gui.orb_return_enabled,
        timeout: Duration::from_secs(match gui.orb_return_after_idle_secs {
            0 => orb_idle_policy::PROPOSED_TIMEOUT.as_secs(),
            secs => secs,
        }),
        // The retry gap is not a user setting: it exists to avoid hammering a
        // move that just failed, and a user-visible knob for it would only
        // offer them a way to make the orb give up sooner.
        retry_delay: orb_idle_policy::IdleReturnSettings::default().retry_delay,
        pinned: gui.orb_pinned,
        // The corner is the user's, not ours. It used to be hardcoded to
        // top-right, so on a machine whose taskbar is at the bottom the orb
        // always travelled *up* to a corner that is not the out-of-the-way one
        // — and there was no setting to say otherwise.
        corner: gui.orb_return_corner_value(),
        monitor: orb_idle_policy::MonitorChoice::FollowOrb,
    }
}

impl From<&AppState> for OrbMode {    fn from(state: &AppState) -> Self {
        match state {
            AppState::Idle => OrbMode::Idle,
            AppState::Recording => OrbMode::Recording,
            AppState::Processing => OrbMode::Processing,
            AppState::Typing => OrbMode::Complete,
            AppState::Error(_) => OrbMode::Error,
        }
    }
}

/// Sync-readable wrapper around the Tokio watch channel for the GUI thread.
pub struct StatusClient {
    rx: tokio::sync::watch::Receiver<AppStatus>,
}

impl StatusClient {
    pub fn new(rx: tokio::sync::watch::Receiver<AppStatus>) -> Self {
        Self { rx }
    }

    pub fn get(&self) -> AppStatus {
        self.rx.borrow().clone()
    }
}

/// The screen a secondary window is placed on, in logical points.
///
/// `true_screen_size_px` first: `ctx.screen_rect()` is the *capsule's own* rect
/// with its origin at `(0,0)` — egui-winit builds it from `window.inner_size()`
/// (`egui-winit-0.28.1/src/lib.rs:38-41`, `:239-241`) — so deriving a position
/// from it pinned the review and consent windows to the top-left of the desktop,
/// partly off-screen, wherever the orb was. The dashboard
/// (`render_dashboard`) and the transcript bubble already use the true screen;
/// found again by Q1-1 on two windows that had not been converted.
///
/// The `None` branch is the non-Windows build, where there is nothing else to
/// ask and the root window's rect is all egui has.
fn screen_size_pt(ctx: &egui::Context) -> (f32, f32) {
    match true_screen_size_px() {
        Some((w, h)) => {
            let ppp = ctx.pixels_per_point().max(1.0);
            (w as f32 / ppp, h as f32 / ppp)
        }
        None => {
            let rect = ctx.screen_rect();
            (rect.width(), rect.height())
        }
    }
}

/// Validates a settings draft before it is written to `config.toml`.
/// Returns a Persian error string for the first failing field, so the save
/// button can stay disabled and the reason can be shown inline.
fn validate_settings(s: &Settings) -> Result<(), String> {
    if s.audio.sample_rate == 0 {
        return Err(format_persian_display("نرخ نمونه‌برداری نمی‌تواند صفر باشد"));
    }
    if s.audio.channels == 0 {
        return Err(format_persian_display("تعداد کانال‌ها نمی‌تواند صفر باشد"));
    }
    if !(0.0..=1.0).contains(&s.vad.threshold) {
        return Err(format_persian_display("حساسیت VAD باید بین ۰ و ۱ باشد"));
    }
    if s.vad.silence_timeout_ms == 0 {
        return Err(format_persian_display("مدت سکوت باید بزرگتر از صفر باشد"));
    }
    // Hotkey strings are parsed into virtual-key codes at startup; a string
    // that cannot be parse is rejected here so the user sees the problem in the
    // settings UI instead of silently falling back to the default.
    if let Err(e) = HotkeyBinding::parse(&s.hotkey.record) {
        return Err(format_persian_display(&format!(
            "کلید ضبط نامعتبر است: {e}"
        )));
    }
    if let Err(e) = HotkeyBinding::parse(&s.hotkey.toggle_overlay) {
        return Err(format_persian_display(&format!(
            "کلید نمایش/مخفی نامعتبر است: {e}"
        )));
    }
    if let Err(e) = HotkeyBinding::parse(&s.hotkey.quit) {
        return Err(format_persian_display(&format!(
            "کلید خروج نامعتبر است: {e}"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualMode {
    /// State 1: Ultra-minimal subtle horizontal line above taskbar (36x5 px).
    IdleDormant,
    /// State 2: Mouse hovered: small mic pill button + 'Dictate CapsLock' tooltip.
    HoveredAwake,
    /// State 3: Active recording: sleek black stadium capsule with ✕, waveform bars, and ✓.
    RecordingActive,
    /// State 4: Processing / typing indicator.
    Processing,
}

/// Tabs of the unified OmniType Dashboard. Each variant reuses the body of
/// the corresponding legacy manager window (viewport wrapper removed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardTab {
    Engines,
    Dictionary,
    History,
    Settings,
    /// The gated microphone test. Its own tab rather than a corner of
    /// Settings, because it *acts* (it opens the device) where the rest of
    /// Settings only edits values — and a button that measures mixed in with
    /// fields that merely persist invites the wrong assumption about which is
    /// which.
    MicTest,
    /// Per-application rules. Its own tab rather than a card in Settings: it is
    /// a list with an editor, and the things it changes are the *text rules*
    /// that Settings' own `[text]` section owns only as a general default.
    Profiles,
}

impl DashboardTab {
    /// Persian label shown in the dashboard tab bar.
    fn label(self) -> &'static str {
        match self {
            Self::Engines => "موتورها",
            Self::Dictionary => "دیکشنری",
            Self::History => "تاریخچه",
            Self::Settings => "تنظیمات",
            Self::MicTest => "میکروفون",
            Self::Profiles => "پروفایل‌ها",
        }
    }

    /// Phosphor glyph for the tab bar.
    fn icon(self) -> &'static str {
        match self {
            Self::Engines => ic::BRAIN,
            Self::Dictionary => ic::BOOK_OPEN,
            Self::History => ic::CLOCK_COUNTER_CLOCKWISE,
            Self::Settings => ic::GEAR,
            Self::MicTest => ic::MICROPHONE,
            Self::Profiles => ic::APP_WINDOW,
        }
    }
}

/// eframe application implementing the modern AI-native overlay bubble.
pub struct OverlayApp {
    status: Arc<StatusClient>,
    events_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
    visible: bool,
    recording_start: Option<Instant>,
    /// Requests raised by the tray menu or a hotkey, waiting to be observed.
    flags: DashboardFlags,
    /// True for a few frames after the dashboard is asked to open: the OS
    /// window shape must be re-applied (it was sized like the capsule).
    dashboard_needs_shape: bool,
    /// Counts frames since the last dashboard open request (bounds the reshaping work).
    dashboard_shape_frames: u32,
    /// Shared technical dictionary for real-time rule management.
    dictionary: Arc<RwLock<Dictionary>>,
    /// Shared ASR router for dynamic model selection and registration.
    router: AsrRouter,
    /// Shared application settings.
    settings: Arc<RwLock<Settings>>,
    /// Path to config.toml.
    config_path: PathBuf,
    /// Persistent history of voice transcribed texts.
    pub(crate) history: Vec<HistoryItem>,
    next_history_id: usize,
    history_search: String,
    history_copy_msg: Option<(String, Instant)>,
    shape_frames_checked: u8,
    /// How many times the window style had to be re-stripped because the OS put
    /// the caption/frame back (winit re-applies window attributes on
    /// minimize/restore). Logged while small, then summarised.
    style_repairs: u32,
    /// Transcribed text for the 10-second secondary preview toast window
    /// Library-managed notification channel (egui-notify). Each transcript
    /// enqueues a toast that renders inside the preview viewport and manages
    /// its own 10 s lifetime, slide animation, and progress bar.
    /// and the id counter behind it.
    toast: toast::ToastState,
    pub last_seen_transcript: Option<String>,
    /// phase 3: history entry id that the current recording session keeps
    /// appending to, so a chunked dictation stays **one** history row instead of
    /// one row per chunk.
    session_history_id: Option<usize>,
    /// Text accumulated for that session (chunk texts joined in order).
    session_text: String,
    /// Wispr Flow interaction and dock mode
    pub visual_mode: VisualMode,
    pub is_hovered: bool,
    pub last_hover_time: Option<Instant>,
    #[allow(dead_code)]
    has_initial_positioned: bool,
    #[allow(dead_code)]
    current_width: f32,
    #[allow(dead_code)]
    current_height: f32,
    /// Dictionary tab: new-rule form, search box and inline editor.
    dict: dict_panel::DictPanelState,
    /// Settings tab: the unvalidated draft plus its UI state.
    settings_tab: settings_panel::SettingsPanelState,
    /// The profiles tab's own UI state. The profiles themselves live in the
    /// settings draft above; this is only which row is open and what is
    /// half-typed into the new-rule fields.
    profiles_tab: profiles_panel::ProfilesPanelState,
    /// The quick dictionary fix: one word, its replacement, the scope, and the
    /// phrase being previewed. Lives at the top of the Dictionary tab.
    dict_fix: dict_fix_panel::DictFixState,
    /// Microphone test tab: the gated short capture and its measurement.
    ///
    /// The gate is shared with the state machine on purpose: a panel that
    /// opened its own gate would be answering "is a dictation live?" from a
    /// second channel, and could disagree with the orb.
    mic_test: mic_test_panel::MicTestPanelState,
    mic_gate: Arc<crate::audio::gate::LiveMicGate>,
    /// The review/recovery window's state: the editable box and the draft it is
    /// showing.
    review_panel: review_panel::ReviewPanelState,
    /// The wire shared with the loop, so drafts raised there are visible here.
    review: Arc<crate::state::ReviewChannel>,
    /// True until the user picks an engine or downloads a local model.
    first_run_pending: bool,
    /// Cloud-consent gate: audio must not leave the machine until opt-in.
    cloud_consent_given: bool,
    /// Controls display of the one-time cloud-consent prompt viewport.
    show_consent_window: bool,
    /// Controls display of the unified OmniType Dashboard (tabbed manager).
    pub show_dashboard: bool,
    /// Active tab inside the unified dashboard.
    pub dashboard_tab: DashboardTab,
    /// Engines tab: the add-a-provider form and its feedback line.
    engine: engine_panel::EnginePanelState,
    pub update_state: crate::updates::SharedUpdateState,
    update_toast_notified: Option<String>,
    /// Runtime handle to the global hotkey listener: live re-bind plus the
    /// system-wide "press a key to bind it" capture.
    hotkey: Option<HotkeyControl>,
    /// The startup diagnosis, when it was not clean.
    ///
    /// Immutable for the process: the report is rewritten on every startup, not
    /// on every settings edit, so this is what was true at boot. The dashboard
    /// shows it as a callout because the tray badge is the only warning a user
    /// cannot miss — and only if they look at the notification area.
    boot_warning: Option<crate::gui::tray_warning::TrayWarning>,
    pub orb: Orb,
    /// Whether the orb should walk back to its corner after a spell of
    /// inactivity, and how long that spell is. Pure decision-making: this holds
    /// no window handle and moves nothing by itself.
    idle_return: orb_idle_policy::IdlePolicy,
    /// `pixels_per_point` as of the last rendered frame, so a command issued
    /// outside the frame loop converts points to pixels with the *current*
    /// scale rather than a stale one.
    last_ppp: f32,
}

impl OverlayApp {
    /// Reveals the dashboard on a given tab.
    ///
    /// All five panel flags did this same six-line dance; keeping it in one
    /// place is what makes the *difference* between them (only Settings
    /// refreshes its draft first) visible instead of buried in repetition.
    fn open_dashboard(&mut self, tab: DashboardTab, ctx: &egui::Context) {
        self.show_dashboard = true;
        self.dashboard_tab = tab;
        self.visible = true;
        self.dashboard_needs_shape = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: Arc<StatusClient>,
        events_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
        flags: DashboardFlags,
        dictionary: Arc<RwLock<Dictionary>>,
        router: AsrRouter,
        settings: Arc<RwLock<Settings>>,
        config_path: PathBuf,
        update_state: crate::updates::SharedUpdateState,
        hotkey: Option<HotkeyControl>,
        boot_warning: Option<crate::gui::tray_warning::TrayWarning>,
        mic_gate: Arc<crate::audio::gate::LiveMicGate>,
        review: Arc<crate::state::ReviewChannel>,
    ) -> Self {
        let initial_visible = settings.read().map(|s| s.gui.show_overlay).unwrap_or(true);

        let saved_orb_center = settings
            .read()
            .ok()
            .and_then(|s| s.gui.orb_position_x.zip(s.gui.orb_position_y));
        let orb = Orb::new("OmniType", saved_orb_center);
        let orb = {
            let mut orb = orb;
            // The user's size preference, applied before the first frame so the
            // very first painted orb is already the size they chose.
            if let Ok(s) = settings.read() {
                orb.set_user_scale(s.gui.orb_scale());
            }
            orb
        };

        Self {
            status,
            events_tx,
            visible: initial_visible,
            recording_start: None,
            flags,
            dashboard_needs_shape: false,
            dashboard_shape_frames: 0,
            dictionary,
            router,
            settings: settings.clone(),
            config_path,
            history: Vec::new(),
            next_history_id: 1,
            history_search: String::new(),
            history_copy_msg: None,
            shape_frames_checked: 0,
            style_repairs: 0,
            toast: toast::ToastState::default(),
            last_seen_transcript: None,
            session_history_id: None,
            session_text: String::new(),
            visual_mode: VisualMode::IdleDormant,
            is_hovered: false,
            last_hover_time: None,
            has_initial_positioned: false,
            current_width: 0.0,
            current_height: 0.0,
            dict: dict_panel::DictPanelState::default(),
            show_dashboard: false,
            dashboard_tab: DashboardTab::Engines,
            profiles_tab: profiles_panel::ProfilesPanelState::default(),
            dict_fix: dict_fix_panel::DictFixState::default(),
            settings_tab: settings_panel::SettingsPanelState::from_settings(
                &settings.read().map(|s| s.clone()).unwrap_or_default(),
            ),
            mic_test: mic_test_panel::MicTestPanelState::new(),
            mic_gate,
            review_panel: review_panel::ReviewPanelState::default(),
            review,
            first_run_pending: false,
            cloud_consent_given: false,
            show_consent_window: false,
            engine: engine_panel::EnginePanelState::default(),
            update_state,
            update_toast_notified: None,
            hotkey,
            boot_warning,
            orb,
            idle_return: orb_idle_policy::IdlePolicy::new(
                idle_settings_from(&settings.read().map(|s| s.gui.clone()).unwrap_or_default()),
                Instant::now(),
            ),
            
            last_ppp: 1.0,
        }
    }

    /// Persists the new orb center coordinates to Settings and writes to disk.
    fn persist_orb_position(&self, x: i32, y: i32) {
        let snapshot = match self.settings.write() {
            Ok(mut s) => {
                s.gui.orb_position_x = Some(x);
                s.gui.orb_position_y = Some(y);
                s.clone()
            }
            Err(_) => return,
        };
        let config_path = self.config_path.clone();
        std::thread::spawn(move || {
            if let Err(e) = snapshot.save(&config_path) {
                tracing::error!("[orb] failed to save position: {e:?}");
            }
        });
    }

    /// Toggles overlay visibility and persists the preference.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if let Ok(mut s) = self.settings.write() {
            s.gui.show_overlay = self.visible;
            let _ = s.save(&self.config_path);
        }
    }

    /// Delegates to [`dict_panel`], which owns the tab's nine form and
    /// inline-editor fields as a single [`DictPanelState`].
    fn render_dict_body(&mut self, ui: &mut egui::Ui) {
        // The quick fix comes first, and takes its corpus from the history:
        // the sentences the user actually dictated are the only evidence of
        // which *healthy words* a short rule would sit inside of.
        //
        // Built per frame rather than cached because it is bounded by the
        // history's own cap and is not consulted unless the fix card has a
        // word in it — see `dict_fix_panel`'s cache, which keeps the matcher
        // work off the redraw path.
        let corpus: Vec<String> = self.history.iter().map(|item| item.text.clone()).collect();
        dict_fix_panel::render(
            ui,
            &mut self.dict_fix,
            &mut self.settings_tab.draft,
            &self.settings,
            &self.config_path,
            &self.dictionary,
            &corpus,
        );
        ui.add_space(8.0);
        dict_panel::render(ui, &mut self.dict, &self.dictionary);
    }

    /// Delegates to [`engine_panel`], handing it the three Settings-tab
    /// fields it is allowed to write. Those borrows are disjoint, which is
    /// what lets the panel stay a free function instead of a method.
    fn render_engine_body(&mut self, ui: &mut egui::Ui) {
        let mut handoff = engine_panel::SettingsHandoff {
            draft: &mut self.settings_tab.draft,
            error: &mut self.settings_tab.error,
            tab: &mut self.dashboard_tab,
        };
        engine_panel::render(
            ui,
            &mut self.engine,
            &self.router,
            &self.settings,
            &self.config_path,
            &mut handoff,
        );
    }

    /// Delegates to [`history_panel`], which owns the three history
    /// fields; this is only the borrow-checked hand-off point.
    fn render_history_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // The History tab reports a fix request rather than acting on it: the
        // two panels do not know about each other, and the overlay is the one
        // place that can seed the fix card *and* move the user to it.
        let mut fix_request: Option<history_panel::FixRequest> = None;
        history_panel::render(
            ui,
            ctx,
            &mut self.history,
            &mut self.history_search,
            &mut self.history_copy_msg,
            &mut fix_request,
        );
        if let Some(request) = fix_request {
            self.dict_fix.seed(&request.word, &request.sentence);
            self.dashboard_tab = DashboardTab::Dictionary;
        }
    }
    /// Delegates to [`settings_panel`], which owns the draft and the UI
    /// state around it. The four borrows are disjoint fields of `self`,
    /// which is what keeps the tab a free function.
    fn render_settings_body(&mut self, ui: &mut egui::Ui) {
        // The controls are seeded from the policy, then written back after the
        // panel has had them. Doing it in that order means the policy is the
        // single source of truth for what the checkbox shows, and the panel
        // never has to know whether a value was rejected by the policy.
        let mut controls = settings_panel::OrbControls {
            return_enabled: self.idle_return_enabled(),
            return_after_idle_secs: self.idle_return.settings().timeout.as_secs(),
            pinned: self.orb_is_pinned(),
            // Seeded from the saved settings rather than from a default, so the
            // panel opens showing what is actually in force. A control that
            // opens on a value other than the live one overwrites it the moment
            // the panel is drawn.
            return_corner: self
                .settings
                .read()
                .map(|s| s.gui.orb_return_corner.clone())
                .unwrap_or_else(|_| "top_right".to_string()),
            scale_percent: self
                .settings
                .read()
                .map(|s| s.gui.orb_scale_percent)
                .unwrap_or(100),
            return_to_manual: false,
        };
        settings_panel::render(
            ui,
            &mut self.settings_tab,
            &self.settings,
            self.hotkey.as_ref(),
            &self.config_path,
            &self.update_state,
            Some(&mut controls),
        );
        if controls.return_enabled != self.idle_return_enabled() {
            self.set_idle_return_enabled(controls.return_enabled);
        }
        if controls.pinned != self.orb_is_pinned() {
            self.set_orb_pinned(controls.pinned);
        }
        if controls.return_after_idle_secs != self.idle_return.settings().timeout.as_secs() {
            self.set_idle_return_after_idle_secs(controls.return_after_idle_secs);
        }
        if std::mem::take(&mut controls.return_to_manual) {
            self.return_orb_to_manual_spot();
        }
        // Corner and size go through the same settings-and-save path as the rest,
        // so they survive a restart like every other preference.
        if controls.return_corner != self.settings.read().map(|s| s.gui.orb_return_corner.clone()).unwrap_or_default() {
            let corner = controls.return_corner.clone();
            self.update_gui_settings(|gui| gui.orb_return_corner = corner.clone());
            // Into the live policy too, not only on disk: the corner decides
            // where the *next* idle return goes, and the policy holds its own
            // copy of the settings.
            let mut s = *self.idle_return.settings();
            s.corner = self
                .settings
                .read()
                .map(|g| g.gui.orb_return_corner_value())
                .unwrap_or(orb_idle_policy::Corner::TopRight);
            self.idle_return.set_settings(s);
        }
        if controls.scale_percent != self.settings.read().map(|s| s.gui.orb_scale_percent).unwrap_or(100) {
            let pct = controls.scale_percent;
            self.update_gui_settings(|gui| gui.orb_scale_percent = pct);
            // Applied to the live orb as well as the saved value: a size the user
            // has to restart to see is not a setting, it is a surprise.
            if let Ok(s) = self.settings.read() {
                self.orb.set_user_scale(s.gui.orb_scale());
            }
        }
    }

    /// Delegates to [`profiles_panel`], which owns the row selection and edits
    /// the *settings draft* the Settings tab already holds.
    ///
    /// Nothing is written back out here, unlike the orb controls: a profile is
    /// a value the coordinator reads when the next dictation starts, so no live
    /// object has to be re-bound the moment it changes. The panel's own save
    /// button writes the draft through the same validator the Settings tab
    /// uses, which is what stops this tab from being the one place that can put
    /// an invalid `config.toml` on disk.
    fn render_profiles_body(&mut self, ui: &mut egui::Ui) {
        profiles_panel::render(
            ui,
            &mut self.profiles_tab,
            &mut self.settings_tab.draft,
            &self.settings,
            &self.config_path,
            // The live dictionary, so the preview's two lines are the pipeline
            // the coordinator would actually run rather than a reading of the
            // file at startup.
            &self.dictionary,
        );
    }

    /// Changes how long the orb waits before returning.
    ///
    /// A zero from the slider cannot reach here: the slider's range starts at
    /// ten seconds. The guard is kept anyway because `0` is the *stored*
    /// "not chosen" value and reading the slider back must never be able to
    /// write it.
    pub fn set_idle_return_after_idle_secs(&mut self, secs: u64) {
        if secs == 0 {
            return;
        }
        let mut settings = *self.idle_return.settings();
        settings.timeout = Duration::from_secs(secs);
        self.idle_return.set_settings(settings);
        self.save_idle_return_settings(settings);
    }

    /// Delegates to [`mic_test_panel`], handing it the two fields it may touch
    /// and a read-only clone of the settings it reads the device from. The
    /// clone is deliberate: a test that could *write* `[audio]` would let a
    /// diagnostic silently reconfigure the recorder.
    fn render_mic_test_body(&mut self, ui: &mut egui::Ui) {
        let settings = match self.settings.read() {
            Ok(s) => s.clone(),
            // A poisoned settings lock must not take the dashboard down with
            // it; the tab simply says it cannot read the configuration.
            Err(_) => {
                callout(
                    ui,
                    CalloutKind::Warning,
                    "تنظیمات قابل خواندن نیست؛ آزمون میکروفون اجرا نشد.",
                );
                return;
            }
        };
        mic_test_panel::render(ui, &mut self.mic_test, &settings, self.mic_gate.clone());
    }

    /// Releases the microphone when the dashboard closes.
    ///
    /// Called from the dashboard's own hide path rather than relying on `Drop`:
    /// a panel left holding a device is invisible to the user and only
    /// discoverable when their next dictation fails to start.
    pub fn release_mic_if_held(&mut self) {
        let gate = self.mic_gate.clone();
        self.mic_test.on_hidden(&gate);
    }

    /// Asks the idle-return policy whether the orb should go back to its corner,
    /// and carries out a "yes".
    ///
    /// The split is deliberate: [`orb_idle_policy::IdlePolicy`] decides *whether*
    /// and produces a request carrying an id, and this method is the only thing
    /// that turns a request into a `SetWindowPos`. A move that cannot be
    /// carried out is reported back as an outcome, because a policy that never
    /// hears how its requests went would keep retrying the same impossible move
    /// forever.
    fn tick_idle_return(&mut self, ctx: &egui::Context, status: &AppStatus) {
        let ppp = ctx.pixels_per_point();
        let center = self.orb.home_position();
        let orb_center = egui::Pos2::new(center.0 as f32, center.1 as f32);
        let target = orb_idle_adapter::waiting_spot(self.idle_return.settings(), orb_center, ppp);

        let activity = orb_idle_policy::Activity {
            recording: matches!(status.state, AppState::Recording),
            processing: matches!(status.state, AppState::Processing),
            inserting: matches!(status.state, AppState::Typing),
            dragging: self.orb.is_dragging(),
            // Text the user still has to act on blocks the return: moving the orb
            // away while a decision is pending hides the thing that needs it.
            //
            // Read from the shared store rather than a separate flag. There was
            // an atomic here that nothing ever wrote — the recovery UI did not
            // exist to write it — so this used to be permanently `false` and the
            // blocker below was decorative. Answering it from the drafts
            // themselves means the orb can never walk away from a pending text
            // it does not know about, because there is only one place that
            // decides what is pending.
            pending_text: !self.review.snapshot().is_empty(),
            // The dashboard being open is interaction by another name, and the
            // orb is not even drawn then.
            interacting: self.show_dashboard || self.is_hovered,
        };

        let now = Instant::now();
        let decision = self.idle_return.evaluate(now, activity, target.as_ref());
        let orb_idle_policy::Decision::Move(request) = decision else {
            return;
        };

        // A **glide**, not a jump. `move_home_to` puts the orb at its
        // destination inside one frame, which from the user's side is the orb
        // vanishing and reappearing somewhere else — it reads as a glitch, not
        // as the app tidying up. The spring that already carries the orb's hover
        // and scale excursion carries this too, so there is no second animation
        // that could drift out of sync with the first.
        let reached = self
            .orb
            .glide_home_to(egui::Pos2::new(request.to.x as f32, request.to.y as f32), ppp);
        // Keep painting until it arrives. Without this the loop can go idle after
        // the frame that started the glide, leaving the orb a few pixels short of
        // the corner rather than landing on it.
        if self.orb.is_gliding() {
            ctx.request_repaint();
        }
        let outcome = if reached {
            orb_idle_policy::MoveOutcome::Success
        } else {
            orb_idle_policy::MoveOutcome::Failure
        };
        let ack = self.idle_return.report_move_result(now, request.id, outcome);
        tracing::info!(
            kind = ?request.kind,
            to = ?request.to,
            reached,
            ack = ?ack,
            "orb idle return"
        );
    }

    /// "Take the orb back to where I put it", on demand.
    ///
    /// Separate from the automatic timeout on purpose: it is an explicit user
    /// command, so it runs even when the feature is switched off or the spot is
    /// pinned. Without a remembered spot there is nothing to go back to, and the
    /// policy says so rather than falling back to the corner.
    pub fn return_orb_to_manual_spot(&mut self) -> bool {
        let decision = self.idle_return.ask_return_to_manual(Instant::now());
        let orb_idle_policy::Decision::Move(request) = decision else {
            tracing::info!("orb: asked to return to its manual spot, but there is none");
            return false;
        };
        let ppp = self.last_ppp;
        let reached = self
            .orb
            .move_home_to(egui::Pos2::new(request.to.x as f32, request.to.y as f32), ppp);
        let outcome = if reached {
            orb_idle_policy::MoveOutcome::Success
        } else {
            orb_idle_policy::MoveOutcome::Failure
        };
        self.idle_return
            .report_move_result(Instant::now(), request.id, outcome);
        reached
    }

    /// Whether the automatic idle return is switched on, for the UI.
    pub fn idle_return_enabled(&self) -> bool {
        self.idle_return.settings().enabled
    }

    /// Turns the automatic idle return on or off.
    ///
    /// Applied through the policy rather than by poking the orb, so an
    /// in-flight request stays in flight and only *future* decisions use the
    /// new setting.
    pub fn set_idle_return_enabled(&mut self, enabled: bool) {
        let mut settings = *self.idle_return.settings();
        settings.enabled = enabled;
        self.idle_return.set_settings(settings);
        self.save_idle_return_settings(settings);
    }

    /// Whether the orb is pinned where the user put it, for the UI.
    pub fn orb_is_pinned(&self) -> bool {
        self.idle_return.settings().pinned
    }

    /// Pins or unpins the orb. An explicit "return to my spot" is still
    /// allowed while pinned — pinning suppresses the *automatic* move, not the
    /// user's own command.
    pub fn set_orb_pinned(&mut self, pinned: bool) {
        let mut settings = *self.idle_return.settings();
        settings.pinned = pinned;
        self.idle_return.set_settings(settings);
        self.save_idle_return_settings(settings);
    }

    fn save_idle_return_settings(&self, settings: orb_idle_policy::IdleReturnSettings) {
        let snapshot = match self.settings.write() {
            Ok(mut s) => {
                s.gui.orb_return_after_idle_secs = settings.timeout.as_secs();
                s.gui.orb_return_enabled = settings.enabled;
                s.gui.orb_pinned = settings.pinned;
                s.clone()
            }
            Err(_) => return,
        };
        let config_path = self.config_path.clone();
        std::thread::spawn(move || {
            if let Err(e) = snapshot.save(&config_path) {
                tracing::error!("[orb] failed to save idle-return settings: {e:?}");
            }
        });
    }

    /// Applies one change to the `gui` section and persists it off-thread.
    ///
    /// One place for "change it and save it", because the save is the half that
    /// is easy to forget: a preference that is applied to the live object but
    /// never written to disk is a preference that quietly reverts on the next
    /// launch, and the user has no way to tell that from a bug.
    fn update_gui_settings(&self, f: impl FnOnce(&mut crate::config::settings::GuiSettings)) {
        let snapshot = match self.settings.write() {
            Ok(mut s) => {
                f(&mut s.gui);
                s.clone()
            }
            Err(_) => return,
        };
        let config_path = self.config_path.clone();
        std::thread::spawn(move || {
            if let Err(e) = snapshot.save(&config_path) {
                tracing::error!("[gui] failed to save settings: {e:?}");
            }
        });
    }

    /// Renders the one-time cloud-consent prompt: audio must not leave the
    /// machine until the user explicitly opts in.
    /// Renders the unified OmniType Dashboard: a single decorated window with
    /// a tab bar (Engines / Dictionary / History / Settings) and a status bar.
    /// Each tab reuses the body of the former standalone manager window, so
    /// all chrome helpers and palette tokens stay the single source of truth.
    fn render_dashboard(&mut self, ctx: &egui::Context) {
        if !self.show_dashboard {
            return;
        }

        let (dash_w, dash_h) = (720.0, 640.0);
        // Center on the true monitor, not `ctx.screen_rect()`: from inside the
        // child viewport that returns the parent (capsule) rect.
        let (pos_x, pos_y) = if let Some((sw, sh)) = true_screen_size_px() {
            let ppp = ctx.pixels_per_point();
            (
                ((sw as f32 / ppp) / 2.0 - dash_w / 2.0).round(),
                ((sh as f32 / ppp) / 2.0 - dash_h / 2.0).round(),
            )
        } else {
            let screen = ctx.screen_rect();
            (
                (screen.center().x - dash_w / 2.0).round(),
                (screen.center().y - dash_h / 2.0).round(),
            )
        };

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("omnitype_dashboard_viewport"),
            egui::ViewportBuilder::default()
                .with_title("OmniType — داشبورد یکپارچه")
                .with_position([pos_x, pos_y])
                .with_inner_size([dash_w, dash_h])
                .with_min_inner_size([520.0, 440.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_transparent(false),
            |dash_ctx, _class| {
                if dash_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_dashboard = false;
                    // Closing the dashboard must not leave a capture armed: the
                    // global poller would otherwise keep swallowing hotkeys
                    // until its own timeout expires.
                    if let Some(hotkey) = self.hotkey.clone() {
                        hotkey.cancel_capture();
                    }
                    self.settings_tab.capturing_hotkey = [false; 3];
                    // Same reason as the hotkey capture: a test that outlived
                    // its window would hold the microphone with nothing on
                    // screen to explain why the next dictation cannot start.
                    self.release_mic_if_held();
                }
                apply_theme_visuals(dash_ctx);

                egui::CentralPanel::default()
                    .frame(manager_central_panel())
                    .show(dash_ctx, |ui| {
                        // Persian UI: lay out right-to-left. egui 0.28 has no
                        // global direction flag, so the whole panel body runs
                        // inside a top-down layout whose cross axis grows
                        // leftward — every `ui.horizontal` row and every grid
                        // column then mirrors automatically.
                        ui.with_layout(
                            egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                            |ui| {
                                // No manual width fiddling: the panel frame's
                                // 14 px symmetric margin already bounds the
                                // available width; subtracting again would
                                // shrink content and leave the right side
                                // visibly empty.
                                // ── Boot warning ──
                                // Above the tab bar and never hidden by it: a
                                // problem with the configuration is a problem
                                // with every tab, and burying it under the
                                // currently-open tab is how it goes unnoticed.
                                if let Some(w) = &self.boot_warning {
                                    ui.add_space(6.0);
                                    theme::callout(ui, theme::CalloutKind::Warning, &w.banner_body());
                                }
                                // ── Title row ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{} OmniType", ic::MICROPHONE))
                                    .size(15.0)
                                    .strong()
                                    .color(palette::TEXT_PRIMARY),
                            );
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                let active = self.router.active_engine();
                                let engine_label = if active == "auto" {
                                    "خودکار (Auto)".to_string()
                                } else {
                                    active.clone()
                                };
                                status_chip(
                                    ui,
                                    &engine_label,
                                    palette::ENGINE_PILL_BG,
                                    palette::ACCENT,
                                    10.0,
                                    ChipFamily::Small,
                                );
                            });
                        });

                        ui.add_space(6.0);

                        // ── Tab bar ──
                        ui.horizontal(|ui| {
                            for tab in [
                                DashboardTab::Engines,
                                DashboardTab::Dictionary,
                                DashboardTab::History,
                                DashboardTab::Settings,
                                DashboardTab::Profiles,
                                DashboardTab::MicTest,
                            ] {
                                let selected = self.dashboard_tab == tab;
                                let btn = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(format!("{}  {}", format_persian_display(tab.label()), tab.icon()))
                                            .size(11.5)
                                            .color(if selected {
                                                palette::WHITE
                                            } else {
                                                palette::TEXT_SECONDARY
                                            })
                                            .strong(),
                                    )
                                    .fill(if selected {
                                        palette::ACCENT_ACTION
                                    } else {
                                        palette::CHIP_BG
                                    })
                                    .rounding(egui::Rounding::same(6.0)),
                                );
                                if btn.clicked() {
                                    self.dashboard_tab = tab;
                                }
                            }
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // ── Update Notification Banner (if newer version available) ──
                        let update_info_opt = {
                            if let Ok(st) = self.update_state.read() {
                                if let crate::updates::UpdateState::Available(ref info) = *st {
                                    Some(info.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        };

                        if let Some(info) = update_info_opt {
                            manager_card(palette::CARD_BG_ALT, palette::ACCENT).show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display(&format!("نسخه جدید در دسترس است: v{}", info.latest_version)),
                                            ic::CLOUD_ARROW_DOWN
                                        ))
                                            .size(11.5)
                                            .strong()
                                            .color(palette::ACCENT),
                                    );
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        if let Some(ref installer_url) = info.installer_url {
                                            if ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new(format_persian_display("دانلود فایل نصب"))
                                                        .size(11.0)
                                                        .strong()
                                                        .color(palette::WHITE),
                                                )
                                                .fill(palette::ACCENT_ACTION)
                                                .rounding(egui::Rounding::same(5.0)),
                                            ).clicked() {
                                                crate::updates::open_url_in_browser(installer_url);
                                            }
                                        }
                                        if ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(format_persian_display("مشاهده در گیت‌هاب"))
                                                    .size(10.5)
                                                    .color(palette::TEXT_SECONDARY),
                                            )
                                            .fill(palette::CHIP_BG)
                                            .rounding(egui::Rounding::same(5.0)),
                                        ).clicked() {
                                            crate::updates::open_url_in_browser(&info.release_url);
                                        }
                                    });
                                });
                            });
                            ui.add_space(4.0);
                        }

                        // ── Active tab body (scrollable) ──
                        egui::ScrollArea::vertical()
                            .id_source("dashboard_main_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| match self.dashboard_tab {
                                DashboardTab::Engines => self.render_engine_body(ui),
                                DashboardTab::Dictionary => self.render_dict_body(ui),
                                DashboardTab::History => self.render_history_body(ui, ctx),
                                DashboardTab::Settings => self.render_settings_body(ui),
                                DashboardTab::Profiles => self.render_profiles_body(ui),
                                DashboardTab::MicTest => self.render_mic_test_body(ui),
                            });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // ── Status bar ──
                        ui.horizontal(|ui| {
                            let status = self.status.get();
                            let busy =
                                matches!(status.state, AppState::Processing | AppState::Typing);
                            // Snapshot the error before the state is moved below.
                            let error_text = match &status.state {
                                AppState::Error(err) => Some(err.clone()),
                                _ => None,
                            };
                            let (dot, label) = match status.state {
                                AppState::Recording => (palette::DANGER, "در حال ضبط"),
                                AppState::Processing => (palette::WARNING, "در حال پردازش"),
                                AppState::Typing => (palette::ACCENT_SOFT, "در حال تایپ"),
                                AppState::Error(_) => (palette::DANGER, "خطا — دوباره تلاش کنید"),
                                _ => (palette::SUCCESS_DOT, "آماده"),
                            };

                            if busy {
                                ui.add(egui::Spinner::new().size(12.0).color(palette::ACCENT));
                                // Streaming engines (Antigravity) publish live partials;
                                // showing the text forming beats a generic label.
                                let live = status
                                    .partial
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|text| !text.is_empty());
                                let (caption, color) = match live {
                                    Some(text) => {
                                        let mut shown: String = text.chars().take(64).collect();
                                        if text.chars().count() > 64 {
                                            shown.push('…');
                                        }
                                        (shown, palette::TEXT_SECONDARY)
                                    }
                                    None => (
                                        "در حال پردازش گفتار — کمی صبر کنید...".to_string(),
                                        palette::TEXT_MUTED,
                                    ),
                                };
                                ui.label(
                                    egui::RichText::new(format_persian_display(&caption))
                                        .size(10.5)
                                        .color(color),
                                );
                            } else if let Some(err) = error_text {
                                // A failed utterance must be visible, not a
                                // green "ready" dot pretending nothing happened.
                                let mut shown: String = err.chars().take(90).collect();
                                if err.chars().count() > 90 {
                                    shown.push('…');
                                }
                                ui.label(
                                    egui::RichText::new(format_persian_display(&format!(
                                        "خطا: {shown}"
                                    )))
                                    .size(10.5)
                                    .color(palette::DANGER),
                                )
                                .on_hover_text(format_persian_display(&err));
                            } else {
                                let (resp, painter) =
                                    ui.allocate_painter(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                painter.circle_filled(resp.rect.center(), 3.5, dot);
                                ui.label(
                                    egui::RichText::new(format_persian_display(label))
                                        .size(10.5)
                                        .color(palette::TEXT_MUTED),
                                );
                            }

                            if self.first_run_pending {
                                ui.label(
                                    egui::RichText::new(format_persian_display("مدلی بارگذاری نشده"))
                                        .size(10.5)
                                        .color(palette::WARNING),
                                );
                            }

                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format_persian_display("نسخه ۰.۱"))
                                        .size(10.0)
                                        .color(palette::TEXT_FAINT),
                                );
                            });
                        });
                            });
                    });
            },
        );
    }

    /// How long a draft may wait for an answer before the review window stops
    /// offering it — the same number the loop expires drafts with
    /// (`coordinator.draft_ttl`), read from the same settings so the window's
    /// countdown and the store's deadline cannot disagree.
    fn draft_ttl(&self) -> Duration {
        let secs = self
            .settings
            .read()
            .map(|s| s.gui.draft_ttl_secs())
            .unwrap_or(crate::state::review::DEFAULT_DRAFT_TTL.as_secs());
        Duration::from_secs(secs)
    }

    fn render_consent_window(&mut self, ctx: &egui::Context) {
        if !self.show_consent_window {
            return;
        }

        let (win_w, win_h) = (440.0, 260.0);
        // Centred on the **screen**, not on the capsule: see `screen_size_pt`.
        let (pos_x, pos_y) = position_in_screen(screen_size_pt(ctx), (win_w, win_h), None);

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("cloud_consent_viewport"),
            egui::ViewportBuilder::default()
                .with_title("اجازه ارسال صوت به ابر — OmniType")
                .with_position([pos_x, pos_y])
                .with_inner_size([win_w, win_h])
                .with_min_inner_size([360.0, 220.0])
                .with_decorations(true)
                .with_resizable(false)
                .with_transparent(false),
            |con_ctx, _class| {
                if con_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_consent_window = false;
                }
                apply_theme_visuals(con_ctx);

                egui::CentralPanel::default()
                    .frame(manager_central_panel())
                    .show(con_ctx, |ui| {
                        manager_header(ui, "ارسال صوت به ابر", None);
                        manager_subtitle(
                            ui,
                            "موتور ابری برای تشخیف گفتار انتخاب شده است. صوت شما برای پردازش به سرور خارجی ارسال می‌شود.",
                        );
                        ui.add_space(8.0);

                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display(
                                    "گزینه‌های آفلاین (ویسپر محلی یا گوگل رایگان) هیچ صوتی را ارسال نمی‌کنند. آیا اجازه می‌دهید صوت برای دقت بالاتر به ابر ارسال شود؟",
                                ))
                                .size(11.5)
                                .color(palette::TEXT_PRIMARY),
                            );
                        });

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button(format_persian_display("اجازه می‌دهم")).clicked() {
                                self.cloud_consent_given = true;
                                self.show_consent_window = false;
                            }
                            if ui.button(format_persian_display("خیر، آفلاین بمان")).clicked() {
                                self.cloud_consent_given = false;
                                self.show_consent_window = false;
                                if let Ok(mut s) = self.settings.write() {
                                    s.active_engine = "local_whisper".to_string();
                                    let _ = s.save(&self.config_path);
                                }
                            }
                        });
                    });
            },
        );
    }

    /// Whether the 30 fps repaint loop must keep running this frame.
    ///
    /// Running it unconditionally keeps a frame's worth of allocations and
    /// textures alive forever (and the GPU busy) while the app sits idle in
    /// the tray. Only these states genuinely need per-frame updates:
    fn needs_animation_frames(&mut self) -> bool {
        // Recording draws a live waveform from the audio ring buffer.
        matches!(self.status.get().state, AppState::Recording | AppState::Processing | AppState::Typing)
            // Live toasts carry a per-second countdown that must tick.
            || self.toast.has_visible_cards()
            // An open dashboard is fully interactive (hover, text edits,
            // scroll): it must not freeze after the last mouse move.
            || self.show_dashboard
            || self.show_consent_window
            // A pending draft is a live, interactive window the user has to be
            // able to click into and edit — and, crucially, one whose appearance
            // is driven by the loop's thread. Without this the review window
            // would only appear on the next frame something else happened to
            // repaint, which could be never.
            || !self.review.snapshot().is_empty()
            // Viewports still settling their OS window shape.
            || self.dashboard_needs_shape
    }

    /// Shows the transcript card, or hides it when there is no bubble.
    ///
    /// Two checks live here rather than in [`toast`] because both are app
    /// policy: whether the feature is on at all, and whether *this* user wants
    /// bubbles. The card's own drawing lives in the toast module.
    ///
    /// The OS window is owned by [`crate::gui::preview_window`], which creates it
    /// once and then only ever shows/hides it. This function decides *what* to
    /// show; the module decides *how* the window is kept alive. See that
    /// module's docs for why a per-bubble window was the bug.
    fn render_preview_toast_window(&mut self, ctx: &egui::Context) {
        if !toast::enabled() {
            // Drained rather than left to grow, so turning the card back on
            // cannot resurrect a backlog of stale bubbles.
            self.toast.clear();
            return;
        }
        let bubble_enabled = self
            .settings
            .read()
            .map(|s| s.gui.show_transcript_bubble)
            .unwrap_or(true);
        toast::render(ctx, &mut self.toast, bubble_enabled);
    }
}
impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // phase 1 — deterministic window ownership (see `register_main_hwnd`).
        //
        // The main window handle now comes straight from eframe's own raw window
        // handle instead of being guessed by title, so winit's internal
        // event-target window and the tray-icon message window can no longer be
        // mistaken for the orb and dragged onto it (the accumulating ghost boxes).
        // `shape_frames_checked` is now "frames spent waiting for the handle".
        #[cfg(windows)]
        {
            // Re-resolved every frame: cheap, and it is the only way to notice
            // that winit recreated the OS window (`register_main_hwnd` returns
            // early when the handle is unchanged).
            if register_main_hwnd(frame) {
                self.shape_frames_checked = self.shape_frames_checked.wrapping_add(1);
            }
            let hwnd = MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
            // Self-healing shape guard (see `enforce_frameless_window`): two
            // `GetWindowLongW` reads per frame, and a repair only when the
            // window really drifted back to a captioned frame — e.g. after
            // minimize/restore. Without this the orb showed a Windows title bar
            // and a frame again until the app was restarted.
            if enforce_frameless_window(hwnd) {
                self.style_repairs = self.style_repairs.saturating_add(1);
                if self.style_repairs <= 10 {
                    tracing::warn!(
                        repairs = self.style_repairs,
                        hwnd,
                        "window frame drifted back; re-stripped caption/border"
                    );
                }
            }
        }
        // On non-Windows builds the frame is not needed at all.
        #[cfg(not(windows))]
        let _ = &frame;

        // Consume external control flags.
        if self.flags.take(Toggle::Overlay) {
            // "Open OmniType" reveals the unified dashboard (skill section 4:
            // tray Open un-minimizes the single existing window).
            self.open_dashboard(DashboardTab::Engines, ctx);
        }
        if self.flags.take(Toggle::Dictionary) {
            self.open_dashboard(DashboardTab::Dictionary, ctx);
        }
        if self.flags.take(Toggle::Engine) {
            self.open_dashboard(DashboardTab::Engines, ctx);
        }
        if self.flags.take(Toggle::History) {
            self.open_dashboard(DashboardTab::History, ctx);
        }
        if self.flags.take(Toggle::Settings) {
            // Refresh the draft from disk so the tab never shows stale values.
            if let Ok(s) = self.settings.read() {
                self.settings_tab.draft = s.clone();
            }
            self.settings_tab.error = None;
            self.open_dashboard(DashboardTab::Settings, ctx);
        }

        // The dashboard is an immediate viewport: on first open the OS window
        // region/shape is stale (it was last shaped as the tiny capsule), so
        // the window looks half-rendered and swallows input until resized.
        // Re-apply the shape for a few frames after each open request, and
        // force a repaint so the content actually paints.
        if self.dashboard_needs_shape {
            self.dashboard_shape_frames += 1;
            if self.dashboard_shape_frames > 10 {
                self.dashboard_needs_shape = false;
                self.dashboard_shape_frames = 0;
            }
            // phase 1: the thread-wide shaping that used to run here is disabled —
            // re-resolving "the app window" on every dashboard open was one of the
            // paths that parked the wrong OS window on the orb. The dashboard is a
            // decorated window and never needed the transparency surgery.
            // (rollback: restore `apply_window_shapes_all();`)
            // #[cfg(windows)]
            // apply_window_shapes_all();
            ctx.request_repaint();
        }
        // `take` clears the flag where the old code used `load`. For Quit the
        // difference is unobservable — the window closes in this same frame —
        // and clearing is the safer of the two if the close is ever refused.
        //
        // The second condition is the important one. A state machine that ended
        // on its own — a panic, or an error out of its run loop — leaves this
        // window open with nothing behind it: the orb still paints, still takes
        // clicks, and the record key does nothing, because every send is going
        // into a channel whose reader is gone. `send` does not report that,
        // because the GUI is holding the other end. So the window checks the
        // one fact that is actually true and closes, rather than sitting there
        // looking alive and swallowing the user's dictations.
        if self.flags.take(Toggle::Quit) || !self.flags.machine_is_alive() {
            if !self.flags.machine_is_alive() {
                tracing::error!(
                    "closing window: the state machine is no longer running, so the record key would do nothing"
                );
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Trigger update notification toast if a newer release is detected
        let update_to_toast = {
            if let Ok(st) = self.update_state.read() {
                if let crate::updates::UpdateState::Available(ref info) = *st {
                    if self.update_toast_notified.as_deref() != Some(&info.latest_version) {
                        Some(info.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(info) = update_to_toast {
            self.update_toast_notified = Some(info.latest_version.clone());
            let msg = format!(
                "نسخه جدید {} منتشر شد\nبرای مشاهده و دریافت کلیک کنید",
                info.latest_version
            );
            // phase 2 — the egui-notify channel is never rendered anywhere (no
            // `toasts.show()` exists in the project), so every `add` queued a
            // Toast that could never expire. `needs_animation_frames()` sees a
            // non-empty channel, so the 30 fps repaint loop ran forever after the
            // first transcript (measured: ~2.5–5 % of a core while idle, plus a
            // slowly climbing working set). The banner in the dashboard still
            // reports new releases; this dead path is disabled.
            // (rollback: restore `ToastState::make_toast` + `toast.add`.)
            let _ = &msg;
            // let display = format_persian_display(&msg);
            // let mut toast = Toast::custom(
            //     display,
            //     ToastLevel::Custom(ic::BELL.to_string(), palette::ACCENT),
            // );
            // toast.set_duration(Some(Duration::from_secs(12)));
            // toast.set_closable(true);
            // toast.set_show_progress_bar(true);
            // toast.set_max_width(Some(TOAST_MAX_WIDTH));
            // state.toasts.add(toast);

            // phase 1: the update banner is not a window of its own, so the
            // thread-wide re-shaping that used to run here is disabled.
            // (rollback: restore `apply_window_shapes_all();`)
            // #[cfg(windows)]
            // apply_window_shapes_all();
        }

        // Always render secondary viewports if open
        self.render_dashboard(ctx);
        self.render_consent_window(ctx);
        self.render_preview_toast_window(ctx);
        // The review window reads the shared store rather than a flag, so it can
        // appear and disappear on the loop's schedule without the GUI having to
        // poll a boolean that could be a frame stale.
        // Hoisted so the shared settings are read before the panel is mutably
        // borrowed — one frame's worth of a `RwLock` read, and it keeps the
        // window's countdown on the same number the loop expires drafts with.
        let ttl = self.draft_ttl();
        review_panel::render(ctx, &self.review, &mut self.review_panel, ttl);

        // Repaint loop. A continuous 30 fps loop was applied unconditionally
        // here (a fresh shape/allocation pass every frame), which kept the GPU
        // busy and every transient buffer alive even while the app sat idle
        // in the tray. Now the animation loop runs only when something on
        // screen actually changes every frame: an active recording (waveform),
        // a live toast countdown, an open dashboard, or pending viewport work.
        if self.needs_animation_frames() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }

        if !self.visible {
            return;
        }

        let status = self.status.get();
        let now = Instant::now();

        // First-run gate: until an engine is chosen or a local model is
        // loaded, surface the "No Model Loaded" cue (skill state 1).
        let active_engine = self.router.active_engine();
        let local_ready = self
            .settings
            .read()
            .map(|s| !matches!(s.asr.model.as_str(), "" | "none"))
            .unwrap_or(false);
        self.first_run_pending = active_engine == "auto" && !local_ready;

        // Cloud-consent gate (skill state 5): switching to a cloud engine
        // without prior consent opens the opt-in prompt instead of sending
        // audio off-machine.
        if !self.cloud_consent_given
            && !self.show_consent_window
            && (active_engine == "groq" || active_engine == "cloud")
        {
            self.show_consent_window = true;
        }

        // Session bookkeeping.
        //
        // phase 3: with chunked streaming a *single* session alternates
        // Recording → Processing/Typing → Recording once per chunk, so a bubble
        // must only be dismissed when a **new** session starts (Idle/Error →
        // Recording) — clearing it on every Recording frame would wipe each
        // chunk's text before the user could read it.
        match status.state {
            AppState::Recording => {
                if self.recording_start.is_none() {
                    self.recording_start = Some(now);
                    self.last_seen_transcript = None; // next utterance gets its own bubble
                                                      // Dismiss the previous session's bubbles, and start a fresh
                                                      // history row for this session (chunks append to it).
                    self.toast.clear();
                    self.session_history_id = None;
                    self.session_text.clear();
                    self.toast.reset_channel();
                }
            }
            // Mid-session states: keep the session (and its bubbles) alive.
            AppState::Processing | AppState::Typing => {}
            _ => self.recording_start = None,
        }

        // When speech transcript arrives, pop up the 10-second toast preview and record in history (ONLY ONCE per utterance)
        if let Some(ref raw) = status.last_text {
            let trimmed = raw.trim();
            if !trimmed.is_empty() && self.last_seen_transcript.as_deref() != Some(trimmed) {
                self.last_seen_transcript = Some(trimmed.to_string());
                self.toast.enqueue(trimmed, now);

                // Dark OmniType card: near-white caption with a live 10 s
                // countdown footer, accent mic glyph, progress bar, ✕.
                // toast_caption shapes the raw text itself (per finished
                // line, so ligatures stay intact).
                // phase 2: same dead egui-notify path as above — the transcript
                // is shown by the dedicated bubble window below, not by
                // egui-notify. (rollback: restore `Self::make_toast` + add.)
                // let toast = Self::make_toast(
                //     trimmed.to_string(),
                //     TOAST_TOTAL_SECS,
                //     Duration::from_secs(TOAST_TOTAL_SECS),
                // );
                // state.toasts.add(toast);

                let time_now = local_time_str();
                let active_engine_str = self.router.active_engine();

                // phase 3: one history row per *session*. A chunked dictation
                // delivers one text per chunk, so the row is extended in place
                // instead of pushing a new entry every 20 s.
                match self.session_history_id {
                    Some(id) => {
                        if !self.session_text.is_empty() {
                            self.session_text.push(' ');
                        }
                        self.session_text.push_str(trimmed);
                        if let Some(item) = self.history.iter_mut().find(|h| h.id == id) {
                            item.text = self.session_text.clone();
                            item.timestamp = time_now;
                        }
                    }
                    None => {
                        self.session_text = trimmed.to_string();
                        let id = self.next_history_id;
                        self.session_history_id = Some(id);
                        self.history.insert(
                            0,
                            HistoryItem {
                                id,
                                text: self.session_text.clone(),
                                timestamp: time_now,
                                engine: active_engine_str,
                            },
                        );
                    }
                }
                self.next_history_id += 1;
                if self.history.len() > 100 {
                    self.history.truncate(100);
                }

                // phase 1: the main window is shaped exactly once at startup
                // (`register_main_hwnd`); re-shaping it after every transcript was
                // pure DWM churn and another chance to grab the wrong window.
                // (rollback: restore `apply_window_shapes_all();`)
                // #[cfg(windows)]
                // apply_window_shapes_all();
            }
        }

        // Render Floating AI Orb Assistant (Hidden when Dashboard is open to prevent visual collision)
        if !self.show_dashboard {
            #[cfg(windows)]
            {
                let hwnd = MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
                if hwnd != 0 {
                    self.orb.set_hwnd(hwnd);
                }
            }

            // The profile in force, straight off the status packet: the orb
            // draws it while a dictation is in flight and says nothing when the
            // status carries none.
            let orb_out = self.orb.show(
                ctx,
                OrbMode::from(&status.state),
                status.profile.as_deref(),
            );
            self.last_ppp = ctx.pixels_per_point();
            if let Some((x, y)) = orb_out.moved_to {
                self.persist_orb_position(x, y);
                // Only a *user* drag moves the remembered spot. An automatic
                // return must never rewrite it, or the orb would ping-pong
                // between the corner and wherever it had been parked.
                self.idle_return
                    .note_manual_move(orb_idle_policy::PhysicalPoint { x, y });
            }
            self.tick_idle_return(ctx, &status);
            if orb_out.clicked {
                // The other half of the orb's pointer log: the orb knows
                // *where* the click landed, this is the only place that knows
                // *what was done about it*. Without both lines, "the window
                // took the click" and "the app ignored it" look the same.
                let action = if matches!(status.state, AppState::Recording) {
                    let _ = self.events_tx.send(HotkeyEvent::RecordUp);
                    "record_up"
                } else {
                    let _ = self.events_tx.send(HotkeyEvent::RecordDown);
                    "record_down"
                };
                tracing::info!(
                    state = ?status.state,
                    action,
                    "orb click handled",
                );
            }
        }
    }
}
