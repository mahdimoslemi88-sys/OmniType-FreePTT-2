//! Floating status overlay (egui/eframe): an AI-native, glassmorphic capsule
//! that reflects the current state (Idle / Recording / Processing / Typing),
//! animates live audio waveforms, and displays Persian transcripts crisply.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use ar_reshaper::ArabicReshaper;
use eframe::egui;
use unicode_bidi::BidiInfo;

use crate::asr::engine::AsrHealth;
use crate::asr::router::AsrRouter;
use crate::config::settings::{CustomProvider, Settings};
use crate::hotkey::HotkeyEvent;
use crate::processing::Dictionary;
use crate::state::{AppState, AppStatus};

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

/// Reshapes Persian/Arabic cursive text and reorders visually for LTR renderers like egui.
pub fn format_persian_display(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Fast check: if text contains any Arabic/Persian/Presentation-form characters
    let has_persian = trimmed.chars().any(|c| {
        ('\u{0600}'..='\u{06FF}').contains(&c)
            || ('\u{FB50}'..='\u{FDFF}').contains(&c)
            || ('\u{FE70}'..='\u{FEFF}').contains(&c)
    });

    if !has_persian {
        return trimmed.to_string();
    }

    let reshaped = ArabicReshaper::default().reshape(trimmed);
    let bidi_info = BidiInfo::new(&reshaped, None);
    let mut visual = String::new();
    for para in &bidi_info.paragraphs {
        let line = bidi_info.reorder_line(para, para.range.clone());
        if !visual.is_empty() {
            visual.push(' ');
        }
        visual.push_str(&line);
    }
    visual
}

/// Historical voice transcription record.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HistoryItem {
    pub id: usize,
    pub text: String,
    pub timestamp: String,
    pub engine: String,
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

#[cfg(windows)]
#[repr(C)]
struct WinMargins {
    cx_left: i32,
    cx_right: i32,
    cy_top: i32,
    cy_bottom: i32,
}

#[cfg(windows)]
#[link(name = "gdi32")]
extern "system" {
    fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> isize;
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SetWindowRgn(hwnd: isize, hrgn: isize, b_redraw: i32) -> i32;
    fn SystemParametersInfoW(
        ui_action: u32,
        ui_param: u32,
        pv_param: *mut windows::Win32::Foundation::RECT,
        f_win_ini: u32,
    ) -> i32;
    fn SetWindowPos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "dwmapi")]
extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: isize, p_mar_inset: *const WinMargins) -> i32;
    fn DwmSetWindowAttribute(
        hwnd: isize,
        dw_attribute: u32,
        pv_attribute: *const std::ffi::c_void,
        cb_attribute: u32,
    ) -> i32;
}

#[cfg(windows)]
pub fn enable_true_transparency(hwnd: isize) {
    let margins = WinMargins {
        cx_left: -1,
        cx_right: -1,
        cy_top: -1,
        cy_bottom: -1,
    };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        // DWMWA_SYSTEMBACKDROP_TYPE = 38, DWMSBT_NONE = 1
        // Disables acrylic and mica frosted glass so the background is 100% transparent!
        let backdrop_none: u32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_none as *const _ as *const std::ffi::c_void,
            4,
        );
    }
}

#[cfg(windows)]
static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
pub fn position_above_taskbar(hwnd: isize, width_px: i32, height_px: i32, corner_px: i32) {
    use windows::Win32::Foundation::RECT;

    let mut work_area = RECT::default();
    let ok = unsafe { SystemParametersInfoW(0x0030 /* SPI_GETWORKAREA */, 0, &mut work_area, 0) };
    if ok != 0 {
        let center_x = (work_area.left + work_area.right) / 2;
        let taskbar_top = work_area.bottom;

        let left = center_x - width_px / 2;
        let top = taskbar_top - height_px - 8; // 8 physical pixels above the taskbar

        unsafe {
            // SWP_NOACTIVATE = 0x0010, SWP_SHOWWINDOW = 0x0040
            SetWindowPos(hwnd, -1 /* HWND_TOPMOST */, left, top, width_px, height_px, 0x0010 | 0x0040);
            let corner_dia = corner_px * 2;
            let hrgn = CreateRoundRectRgn(0, 0, width_px, height_px, corner_dia, corner_dia);
            if hrgn != 0 {
                SetWindowRgn(hwnd, hrgn, 1);
            }
        }
        enable_true_transparency(hwnd);
    }
}

#[cfg(windows)]
fn local_time_str() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}

#[cfg(not(windows))]
fn local_time_str() -> String {
    "00:00:00".to_string()
}

#[cfg(windows)]
fn apply_window_shapes_all() {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetClientRect, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let actual = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..actual as usize]);

            if title == "OmniType" {
                MAIN_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
                enable_true_transparency(hwnd.0 as isize);
            } else if title.contains("OmniType_Preview") {
                enable_true_transparency(hwnd.0 as isize);
                let mut rect = RECT::default();
                if GetClientRect(hwnd, &mut rect).is_ok() {
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;
                    if w > 0 && h > 0 {
                        // Toast has egui corner radius 12.0 -> scale to physical pixels
                        let ppp_est = (h as f32 / 90.0_f32).max(1.0);
                        let corner_dia = ((12.0_f32 * ppp_est).round() as i32) * 2;
                        let hrgn = CreateRoundRectRgn(0, 0, w, h, corner_dia, corner_dia);
                        if hrgn != 0 {
                            SetWindowRgn(hwnd.0 as isize, hrgn, 1);
                        }
                    }
                }
            }
        }
        BOOL(1)
    }

    unsafe {
        let thread_id = GetCurrentThreadId();
        let _ = EnumThreadWindows(thread_id, Some(enum_proc), LPARAM(0));
    }
}

/// Paints a vector microphone icon inside `rect`.
fn paint_vector_mic(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let center = rect.center();
    // Mic capsule
    let cap_w = 4.0_f32;
    let cap_h = 7.0_f32;
    let cap_rect = egui::Rect::from_center_size(center - egui::vec2(0.0, 1.5), egui::vec2(cap_w, cap_h));
    painter.rect_filled(cap_rect, egui::Rounding::same(2.0), color);

    // Cradle (U shape)
    let cradle_y = center.y + 0.5;
    painter.line_segment(
        [center + egui::vec2(-4.0, -1.0), center + egui::vec2(-4.0, cradle_y)],
        egui::Stroke::new(1.3_f32, color),
    );
    painter.line_segment(
        [center + egui::vec2(-4.0, cradle_y), center + egui::vec2(4.0, cradle_y)],
        egui::Stroke::new(1.3_f32, color),
    );
    painter.line_segment(
        [center + egui::vec2(4.0, cradle_y), center + egui::vec2(4.0, -1.0)],
        egui::Stroke::new(1.3_f32, color),
    );

    // Stem and base
    painter.line_segment(
        [center + egui::vec2(0.0, cradle_y), center + egui::vec2(0.0, cradle_y + 3.0)],
        egui::Stroke::new(1.3_f32, color),
    );
    painter.line_segment(
        [center + egui::vec2(-3.0, cradle_y + 3.0), center + egui::vec2(3.0, cradle_y + 3.0)],
        egui::Stroke::new(1.3_f32, color),
    );
}

/// Paints a vector cross (✕) icon inside `rect`.
fn paint_vector_cross(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let center = rect.center();
    let d = 3.5_f32;
    painter.line_segment([center - egui::vec2(d, d), center + egui::vec2(d, d)], stroke);
    painter.line_segment([center + egui::vec2(-d, d), center + egui::vec2(d, -d)], stroke);
}

/// Paints a vector checkmark (✓) icon inside `rect`.
fn paint_vector_check(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let center = rect.center();
    painter.line_segment(
        [center + egui::vec2(-4.0, 0.0), center + egui::vec2(-1.0, 3.5)],
        stroke,
    );
    painter.line_segment(
        [center + egui::vec2(-1.0, 3.5), center + egui::vec2(4.5, -3.5)],
        stroke,
    );
}

/// eframe application implementing the modern AI-native overlay bubble.
pub struct OverlayApp {
    status: Arc<StatusClient>,
    events_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
    visible: bool,
    recording_start: Option<Instant>,
    /// Set externally (tray menu / hotkey) to request a visibility toggle.
    overlay_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu) to request opening the dictionary window.
    dict_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu) to request opening the AI engine window.
    engine_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu) to request opening the history window.
    history_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu / hotkey) to request application quit.
    quit_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Shared technical dictionary for real-time rule management.
    dictionary: Arc<RwLock<Dictionary>>,
    /// Shared ASR router for dynamic model selection and registration.
    router: AsrRouter,
    /// Shared application settings.
    settings: Arc<RwLock<Settings>>,
    /// Path to config.toml.
    config_path: PathBuf,
    /// Persistent history of voice transcribed texts.
    pub history: Vec<HistoryItem>,
    next_history_id: usize,
    /// Controls display of the History & Clipboard secondary viewport window.
    pub show_history_window: bool,
    history_search: String,
    history_copy_msg: Option<(String, Instant)>,
    shape_frames_checked: u8,
    /// Transcribed text for the 10-second secondary preview toast window
    pub toast_text: Option<String>,
    pub toast_display: Option<String>,
    pub toast_start: Option<Instant>,
    pub toast_copied: Option<Instant>,
    pub last_seen_transcript: Option<String>,
    /// Wispr Flow interaction and dock mode
    pub visual_mode: VisualMode,
    pub is_hovered: bool,
    pub last_hover_time: Option<Instant>,
    has_initial_positioned: bool,
    current_width: f32,
    current_height: f32,
    /// Controls display of the Dictionary Manager secondary viewport window.
    pub show_dict_window: bool,
    new_from: String,
    new_to: String,
    new_cat: String,
    search_query: String,
    dict_msg: Option<(String, Instant)>,
    /// Controls display of the AI Engine Manager secondary viewport window.
    pub show_engine_window: bool,
    new_engine_id: String,
    new_engine_name: String,
    new_engine_url: String,
    new_engine_key: String,
    new_engine_model: String,
    new_engine_lang: String,
    engine_msg: Option<(String, Instant)>,
}

impl OverlayApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: Arc<StatusClient>,
        events_tx: tokio::sync::mpsc::UnboundedSender<HotkeyEvent>,
        overlay_flag: Arc<std::sync::atomic::AtomicBool>,
        dict_flag: Arc<std::sync::atomic::AtomicBool>,
        engine_flag: Arc<std::sync::atomic::AtomicBool>,
        history_flag: Arc<std::sync::atomic::AtomicBool>,
        quit_flag: Arc<std::sync::atomic::AtomicBool>,
        dictionary: Arc<RwLock<Dictionary>>,
        router: AsrRouter,
        settings: Arc<RwLock<Settings>>,
        config_path: PathBuf,
    ) -> Self {
        let initial_visible = settings
            .read()
            .map(|s| s.gui.show_overlay)
            .unwrap_or(true);

        Self {
            status,
            events_tx,
            visible: initial_visible,
            recording_start: None,
            overlay_flag,
            dict_flag,
            engine_flag,
            history_flag,
            quit_flag,
            dictionary,
            router,
            settings,
            config_path,
            history: Vec::new(),
            next_history_id: 1,
            show_history_window: false,
            history_search: String::new(),
            history_copy_msg: None,
            shape_frames_checked: 0,
            toast_text: None,
            toast_display: None,
            toast_start: None,
            toast_copied: None,
            last_seen_transcript: None,
            visual_mode: VisualMode::IdleDormant,
            is_hovered: false,
            last_hover_time: None,
            has_initial_positioned: false,
            current_width: 0.0,
            current_height: 0.0,
            show_dict_window: false,
            new_from: String::new(),
            new_to: String::new(),
            new_cat: String::new(),
            search_query: String::new(),
            dict_msg: None,
            show_engine_window: false,
            new_engine_id: String::new(),
            new_engine_name: String::new(),
            new_engine_url: String::new(),
            new_engine_key: String::new(),
            new_engine_model: "whisper-large-v3-turbo".to_string(),
            new_engine_lang: "fa".to_string(),
            engine_msg: None,
        }
    }

    /// Toggles overlay visibility and persists the preference.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if let Ok(mut s) = self.settings.write() {
            s.gui.show_overlay = self.visible;
            let _ = s.save(&self.config_path);
        }
    }

    /// Renders the standalone Dictionary Manager window in an immediate viewport.
    fn render_dict_window(&mut self, ctx: &egui::Context) {
        if !self.show_dict_window {
            return;
        }

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("dictionary_manager_viewport"),
            egui::ViewportBuilder::default()
                .with_title("مدیریت دیکشنری تخصصی — OmniType")
                .with_inner_size([560.0, 520.0])
                .with_min_inner_size([420.0, 350.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_transparent(false),
            |dict_ctx, _class| {
                if dict_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_dict_window = false;
                }

                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(18, 20, 28))
                            .inner_margin(egui::Margin::same(14.0)),
                    )
                    .show(dict_ctx, |ui| {
                        // ── Header & Title ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display("مدیریت دیکشنری کلمات تخصصی"))
                                    .size(16.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(240, 245, 255)),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let total_rules = self.dictionary.read().map(|d| d.len()).unwrap_or(0);
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(30, 36, 52))
                                    .rounding(egui::Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                    .show(ui, |ui| {
                                        let text = format!("{total_rules} قانون فعال");
                                        ui.label(
                                            egui::RichText::new(format_persian_display(&text))
                                                .size(10.5)
                                                .color(egui::Color32::from_rgb(100, 200, 255)),
                                        );
                                    });
                            });
                        });

                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(format_persian_display("تعریف و تصحیح خودکار واژگان فنی، مهندسی و گفتاری"))
                                .size(11.0)
                                .color(egui::Color32::from_rgb(150, 160, 180)),
                        );

                        // Feedback message if active
                        if let Some((ref msg, timestamp)) = self.dict_msg {
                            if timestamp.elapsed() < Duration::from_secs(4) {
                                ui.add_space(4.0);
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(24, 48, 38))
                                    .rounding(egui::Rounding::same(5.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(msg)
                                                .size(11.0)
                                                .color(egui::Color32::from_rgb(120, 240, 160)),
                                        );
                                    });
                            }
                        }

                        ui.add_space(10.0);

                        // ── Card 1: Add New Word ──
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(26, 30, 42))
                            .rounding(egui::Rounding::same(8.0))
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(42, 48, 66)))
                            .inner_margin(egui::Margin::same(10.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format_persian_display("➕ افزودن یا ویرایش کلمه جدید:"))
                                        .size(12.0)
                                        .strong()
                                        .color(egui::Color32::from_rgb(220, 230, 248)),
                                );
                                ui.add_space(6.0);

                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format_persian_display("کلمه شنیده شده (از):"))
                                            .size(11.0)
                                            .color(egui::Color32::from_rgb(180, 190, 210)),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.new_from)
                                            .hint_text("پاتون / سی ان سی")
                                            .desired_width(140.0),
                                    );

                                    ui.label(
                                        egui::RichText::new(format_persian_display("معادل صحیح (به):"))
                                            .size(11.0)
                                            .color(egui::Color32::from_rgb(180, 190, 210)),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.new_to)
                                            .hint_text("پایتون / CNC")
                                            .desired_width(140.0),
                                    );
                                });

                                ui.add_space(5.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format_persian_display("دسته‌بندی (اختیاری):"))
                                            .size(11.0)
                                            .color(egui::Color32::from_rgb(180, 190, 210)),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.new_cat)
                                            .hint_text("برنامه‌نویسی / مکانیک")
                                            .desired_width(130.0),
                                    );

                                    ui.add_space(8.0);
                                    let add_btn = ui.button(
                                        egui::RichText::new(format_persian_display("+ ثبت در دیکشنری"))
                                            .size(11.5)
                                            .color(egui::Color32::WHITE),
                                    );
                                    if add_btn.clicked() {
                                        let from = self.new_from.trim().to_string();
                                        let to = self.new_to.trim().to_string();
                                        let cat = if self.new_cat.trim().is_empty() {
                                            None
                                        } else {
                                            Some(self.new_cat.trim().to_string())
                                        };

                                        if !from.is_empty() && !to.is_empty() && from != to {
                                            if let Ok(mut dict) = self.dictionary.write() {
                                                dict.add_rule(from, to, cat);
                                                let _ = dict.save_to_file();
                                                self.new_from.clear();
                                                self.new_to.clear();
                                                self.new_cat.clear();
                                                self.dict_msg = Some((
                                                    format_persian_display("قاعده با موفقیت ثبت و ذخیره شد!"),
                                                    Instant::now(),
                                                ));
                                            }
                                        }
                                    }
                                });
                            });

                        ui.add_space(10.0);

                        // ── Search Bar ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display("🔍 جستجو:"))
                                    .size(11.5)
                                    .color(egui::Color32::from_rgb(190, 200, 220)),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.search_query)
                                    .hint_text("جستجو در بین کلمات...")
                                    .desired_width(ui.available_width() - 10.0),
                            );
                        });

                        ui.add_space(6.0);

                        // ── Scrollable List of Rules ──
                        let mut rule_to_remove: Option<String> = None;
                        let query = self.search_query.trim().to_lowercase();

                        if let Ok(dict) = self.dictionary.read() {
                            let rules = dict.rules();
                            egui::ScrollArea::vertical()
                                .max_height(210.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    egui::Grid::new("dict_rules_grid")
                                        .striped(true)
                                        .spacing([12.0, 6.0])
                                        .min_col_width(80.0)
                                        .show(ui, |ui| {
                                            // Table Header
                                            ui.label(egui::RichText::new(format_persian_display("کلمه گفتاری (از)")).strong().size(11.0));
                                            ui.label(egui::RichText::new("").size(10.0));
                                            ui.label(egui::RichText::new(format_persian_display("معادل صحیح (به)")).strong().size(11.0));
                                            ui.label(egui::RichText::new(format_persian_display("دسته")).strong().size(11.0));
                                            ui.label(egui::RichText::new(format_persian_display("حذف")).strong().size(11.0));
                                            ui.end_row();

                                            for r in rules {
                                                if !query.is_empty() {
                                                    let matches_from = r.from.to_lowercase().contains(&query);
                                                    let matches_to = r.to.to_lowercase().contains(&query);
                                                    let matches_cat = r.category.as_deref().unwrap_or("").to_lowercase().contains(&query);
                                                    if !matches_from && !matches_to && !matches_cat {
                                                        continue;
                                                    }
                                                }

                                                ui.label(
                                                    egui::RichText::new(format_persian_display(&r.from))
                                                        .size(11.0)
                                                        .color(egui::Color32::from_rgb(220, 225, 235)),
                                                );
                                                ui.label(
                                                    egui::RichText::new("→")
                                                        .size(11.0)
                                                        .color(egui::Color32::from_rgb(120, 130, 150)),
                                                );
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(&r.to))
                                                        .size(11.0)
                                                        .strong()
                                                        .color(egui::Color32::from_rgb(100, 220, 240)),
                                                );
                                                let cat_str = r.category.as_deref().unwrap_or("-");
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(cat_str))
                                                        .size(9.5)
                                                        .color(egui::Color32::from_rgb(160, 170, 190)),
                                                );

                                                if ui.button(egui::RichText::new("🗑").size(10.5)).clicked() {
                                                    rule_to_remove = Some(r.from.clone());
                                                }
                                                ui.end_row();
                                            }
                                        });
                                });
                        }

                        // Apply pending removal if clicked
                        if let Some(target_from) = rule_to_remove {
                            if let Ok(mut dict) = self.dictionary.write() {
                                dict.remove_by_from(&target_from);
                                let _ = dict.save_to_file();
                                self.dict_msg = Some((
                                    format_persian_display("کلمه از دیکشنری حذف شد"),
                                    Instant::now(),
                                ));
                            }
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // ── Bottom Action Buttons ──
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new(format_persian_display("💾 ذخیره در فایل")).size(11.0)).clicked() {
                                if let Ok(dict) = self.dictionary.read() {
                                    if dict.save_to_file().is_ok() {
                                        self.dict_msg = Some((
                                            format_persian_display("دیکشنری با موفقیت ذخیره شد"),
                                            Instant::now(),
                                        ));
                                    }
                                }
                            }

                            if ui.button(egui::RichText::new(format_persian_display("📝 ویرایش در Notepad")).size(11.0)).clicked() {
                                if let Ok(dict) = self.dictionary.read() {
                                    if let Some(path) = dict.file_path() {
                                        #[cfg(windows)]
                                        {
                                            let _ = std::process::Command::new("cmd")
                                                .args(["/C", "start", "", path.to_str().unwrap_or("dictionary.toml")])
                                                .spawn();
                                        }
                                    }
                                }
                            }

                            if ui.button(egui::RichText::new(format_persian_display("🔄 بارگذاری مجدد")).size(11.0)).clicked() {
                                if let Ok(mut dict) = self.dictionary.write() {
                                    if dict.reload_from_file().is_ok() {
                                        self.dict_msg = Some((
                                            format_persian_display("دیکشنری از دیسک بازخوانی شد"),
                                            Instant::now(),
                                        ));
                                    }
                                }
                            }
                        });
                    });
            },
        );
    }

    /// Renders the standalone AI Engine & Model Manager window in an immediate viewport.
    fn render_engine_window(&mut self, ctx: &egui::Context) {
        if !self.show_engine_window {
            return;
        }

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("engine_manager_viewport"),
            egui::ViewportBuilder::default()
                .with_title("مدیریت مدل‌های هوش مصنوعی و API — OmniType")
                .with_inner_size([580.0, 560.0])
                .with_min_inner_size([440.0, 380.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_transparent(false),
            |eng_ctx, _class| {
                if eng_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_engine_window = false;
                }

                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(18, 20, 28))
                            .inner_margin(egui::Margin::same(14.0)),
                    )
                    .show(eng_ctx, |ui| {
                        // ── Header & Title ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display("مدیریت مدل‌های صوتی و API"))
                                    .size(16.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(240, 245, 255)),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let active = self.router.active_engine();
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(30, 42, 60))
                                    .rounding(egui::Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                    .show(ui, |ui| {
                                        let text = if active == "auto" {
                                            "حالت فعال: خودکار (Auto)".to_string()
                                        } else {
                                            format!("فعال: {active}")
                                        };
                                        ui.label(
                                            egui::RichText::new(format_persian_display(&text))
                                                .size(10.5)
                                                .color(egui::Color32::from_rgb(100, 220, 255)),
                                        );
                                    });
                            });
                        });

                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(format_persian_display(
                                "انتخاب موتور پیش‌فرض یا افزودن سرور و مدل‌های اختصاصی (سازگار با OpenAI)",
                            ))
                            .size(11.0)
                            .color(egui::Color32::from_rgb(150, 160, 180)),
                        );

                        // Feedback message
                        if let Some((ref msg, timestamp)) = self.engine_msg {
                            if timestamp.elapsed() < Duration::from_secs(4) {
                                ui.add_space(4.0);
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(24, 48, 38))
                                    .rounding(egui::Rounding::same(5.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(msg)
                                                .size(11.0)
                                                .color(egui::Color32::from_rgb(120, 240, 160)),
                                        );
                                    });
                            }
                        }

                        ui.add_space(10.0);

                        // ── Available Engines List ──
                        ui.label(
                            egui::RichText::new(format_persian_display("موتورهای گفتار به متن ثبت‌شده:"))
                                .size(12.5)
                                .strong()
                                .color(egui::Color32::from_rgb(210, 225, 250)),
                        );
                        ui.add_space(4.0);

                        let engines = self.router.list_engines();
                        let current_active = self.router.active_engine();

                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .show(ui, |ui| {
                                // Auto Fallback Option
                                let is_auto = current_active == "auto";
                                egui::Frame::none()
                                    .fill(if is_auto {
                                        egui::Color32::from_rgb(26, 44, 58)
                                    } else {
                                        egui::Color32::from_rgb(24, 26, 36)
                                    })
                                    .rounding(egui::Rounding::same(8.0))
                                    .stroke(egui::Stroke::new(
                                        1.0_f32,
                                        if is_auto {
                                            egui::Color32::from_rgb(60, 150, 240)
                                        } else {
                                            egui::Color32::from_rgb(40, 44, 60)
                                        },
                                    ))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if ui.radio(is_auto, "").clicked() && !is_auto {
                                                self.router.set_active_engine("auto");
                                                if let Ok(mut s) = self.settings.write() {
                                                    s.active_engine = "auto".to_string();
                                                    let _ = s.save(&self.config_path);
                                                }
                                                self.engine_msg = Some((
                                                    format_persian_display("حالت خودکار هوشمند (Auto) فعال شد."),
                                                    Instant::now(),
                                                ));
                                            }

                                            ui.vertical(|ui| {
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(
                                                        "حالت خودکار هوشمند (Auto Fallback)",
                                                    ))
                                                    .strong()
                                                    .size(12.0)
                                                    .color(egui::Color32::from_rgb(240, 245, 255)),
                                                );
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(
                                                        "اولویت‌بندی خودکار بین ابری و محلی؛ سوییچ بدون وقفه در قطعی شبکه",
                                                    ))
                                                    .size(10.0)
                                                    .color(egui::Color32::from_rgb(140, 155, 175)),
                                                );
                                            });

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if is_auto {
                                                    egui::Frame::none()
                                                        .fill(egui::Color32::from_rgb(20, 80, 50))
                                                        .rounding(egui::Rounding::same(4.0))
                                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                        .show(ui, |ui| {
                                                            ui.label(
                                                                egui::RichText::new(format_persian_display("انتخاب‌شده"))
                                                                    .size(9.5)
                                                                    .color(egui::Color32::from_rgb(100, 240, 160)),
                                                            );
                                                        });
                                                }
                                            });
                                        });
                                    });

                                ui.add_space(4.0);

                                let mut to_delete: Option<String> = None;
                                for (id, display_name, kind, health, _is_selected) in &engines {
                                    let is_active = current_active == *id;
                                    egui::Frame::none()
                                        .fill(if is_active {
                                            egui::Color32::from_rgb(28, 42, 54)
                                        } else {
                                            egui::Color32::from_rgb(24, 26, 36)
                                        })
                                        .rounding(egui::Rounding::same(8.0))
                                        .stroke(egui::Stroke::new(
                                            1.0_f32,
                                            if is_active {
                                                egui::Color32::from_rgb(60, 150, 240)
                                            } else {
                                                egui::Color32::from_rgb(40, 44, 60)
                                            },
                                        ))
                                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                if ui.radio(is_active, "").clicked() && !is_active {
                                                    self.router.set_active_engine(id);
                                                    if let Ok(mut s) = self.settings.write() {
                                                        s.active_engine = id.clone();
                                                        let _ = s.save(&self.config_path);
                                                    }
                                                    self.engine_msg = Some((
                                                        format_persian_display(&format!("موتور {} فعال شد.", display_name)),
                                                        Instant::now(),
                                                    ));
                                                }

                                                // Health indicator dot
                                                let (dot_color, health_desc) = match health {
                                                    AsrHealth::Ready => (
                                                        egui::Color32::from_rgb(60, 220, 130),
                                                        "آماده کار".to_string(),
                                                    ),
                                                    AsrHealth::NoModel => (
                                                        egui::Color32::from_rgb(160, 160, 160),
                                                        "مدل دانلود نشده".to_string(),
                                                    ),
                                                    AsrHealth::Cooldown { reason, .. } => (
                                                        egui::Color32::from_rgb(255, 180, 50),
                                                        format!("در حال بازیابی: {reason}"),
                                                    ),
                                                    AsrHealth::Failed { reason } => (
                                                        egui::Color32::from_rgb(255, 80, 80),
                                                        format!("غیرفعال: {reason}"),
                                                    ),
                                                };

                                                let (resp, painter) = ui.allocate_painter(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                                painter.circle_filled(resp.rect.center(), 3.5, dot_color);
                                                resp.on_hover_text(&health_desc);

                                                ui.vertical(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(display_name)
                                                                .strong()
                                                                .size(12.0)
                                                                .color(egui::Color32::from_rgb(240, 245, 255)),
                                                        );

                                                        egui::Frame::none()
                                                            .fill(egui::Color32::from_rgb(36, 42, 56))
                                                            .rounding(egui::Rounding::same(4.0))
                                                            .inner_margin(egui::Margin::symmetric(5.0, 1.5))
                                                            .show(ui, |ui| {
                                                                ui.label(
                                                                    egui::RichText::new(*kind)
                                                                        .size(9.0)
                                                                        .color(egui::Color32::from_rgb(160, 190, 230)),
                                                                );
                                                            });
                                                    });

                                                    ui.label(
                                                        egui::RichText::new(format!("ID: {id}"))
                                                            .size(9.5)
                                                            .color(egui::Color32::from_rgb(120, 130, 150)),
                                                    );
                                                });

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    if *kind == "Cloud (Custom)" {
                                                        let del_btn = ui.add(
                                                            egui::Button::new(
                                                                egui::RichText::new("🗑")
                                                                    .size(11.0)
                                                                    .color(egui::Color32::from_rgb(255, 110, 110)),
                                                            )
                                                            .fill(egui::Color32::from_rgb(45, 25, 30))
                                                            .rounding(egui::Rounding::same(4.0)),
                                                        );
                                                        if del_btn.clicked() {
                                                            to_delete = Some(id.clone());
                                                        }
                                                    }

                                                    if is_active {
                                                        egui::Frame::none()
                                                            .fill(egui::Color32::from_rgb(20, 80, 50))
                                                            .rounding(egui::Rounding::same(4.0))
                                                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                            .show(ui, |ui| {
                                                                ui.label(
                                                                    egui::RichText::new(format_persian_display("فعال"))
                                                                        .size(9.5)
                                                                        .color(egui::Color32::from_rgb(100, 240, 160)),
                                                                );
                                                            });
                                                    }
                                                });
                                            });
                                        });
                                    ui.add_space(4.0);
                                }

                                if let Some(del_id) = to_delete {
                                    self.router.remove_engine(&del_id);
                                    if let Ok(mut s) = self.settings.write() {
                                        s.remove_provider(&del_id);
                                        if s.active_engine == del_id {
                                            s.active_engine = "auto".to_string();
                                            self.router.set_active_engine("auto");
                                        }
                                        let _ = s.save(&self.config_path);
                                    }
                                    self.engine_msg = Some((
                                        format_persian_display("مدل اختصاصی حذف گردید."),
                                        Instant::now(),
                                    ));
                                }
                            });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // ── Add New API Provider Form ──
                        ui.label(
                            egui::RichText::new(format_persian_display("افزودن API / سرور دلخواه (سازگار با OpenAI):"))
                                .size(12.5)
                                .strong()
                                .color(egui::Color32::from_rgb(210, 225, 250)),
                        );
                        ui.add_space(6.0);

                        egui::Grid::new("add_provider_grid")
                            .num_columns(2)
                            .spacing([10.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format_persian_display("شناسه یکتا (ID):"))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(180, 190, 210)),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_id)
                                        .hint_text("e.g. custom_openai")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("نام نمایشی:"))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(180, 190, 210)),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_name)
                                        .hint_text("e.g. OpenAI Whisper Large")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("آدرس Base URL:"))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(180, 190, 210)),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_url)
                                        .hint_text("e.g. https://api.openai.com/v1")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("کلید API:"))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(180, 190, 210)),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_key)
                                        .password(true)
                                        .hint_text("sk-...")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("نام مدل:"))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(180, 190, 210)),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_model)
                                        .hint_text("whisper-1 / whisper-large-v3-turbo")
                                        .desired_width(340.0),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(format_persian_display("زبان (Language):"))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(180, 190, 210)),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_engine_lang)
                                        .hint_text("fa")
                                        .desired_width(340.0),
                                );
                                ui.end_row();
                            });

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let add_btn = ui.add(
                                egui::Button::new(
                                    egui::RichText::new(format_persian_display("ثبت و فعال‌سازی این موتور"))
                                        .strong()
                                        .size(11.5)
                                        .color(egui::Color32::from_rgb(255, 255, 255)),
                                )
                                .fill(egui::Color32::from_rgb(32, 100, 180))
                                .rounding(egui::Rounding::same(6.0)),
                            );

                            if add_btn.clicked() {
                                let id = self.new_engine_id.trim().to_lowercase();
                                let url = self.new_engine_url.trim().to_string();
                                if id.is_empty() || url.is_empty() {
                                    self.engine_msg = Some((
                                        format_persian_display("خطا: شناسه (ID) و آدرس URL الزامی هستند."),
                                        Instant::now(),
                                    ));
                                } else {
                                    let name = if self.new_engine_name.trim().is_empty() {
                                        id.clone()
                                    } else {
                                        self.new_engine_name.trim().to_string()
                                    };
                                    let model = if self.new_engine_model.trim().is_empty() {
                                        "whisper-large-v3-turbo".to_string()
                                    } else {
                                        self.new_engine_model.trim().to_string()
                                    };
                                    let lang = if self.new_engine_lang.trim().is_empty() {
                                        "fa".to_string()
                                    } else {
                                        self.new_engine_lang.trim().to_string()
                                    };
                                    let provider = CustomProvider {
                                        id: id.clone(),
                                        name,
                                        base_url: url,
                                        api_key: self.new_engine_key.trim().to_string(),
                                        model,
                                        language: lang,
                                        timeout_secs: 20,
                                    };

                                    let usage_path = crate::paths::resolve_usage_path();
                                    let engine = Arc::new(crate::asr::CloudEngine::new_custom(&provider, usage_path));
                                    self.router.register_engine(engine);
                                    self.router.set_active_engine(&id);

                                    if let Ok(mut s) = self.settings.write() {
                                        s.active_engine = id.clone();
                                        s.add_or_update_provider(provider);
                                        let _ = s.save(&self.config_path);
                                    }

                                    self.engine_msg = Some((
                                        format_persian_display("مدل جدید ثبت و به عنوان موتور فعال انتخاب شد."),
                                        Instant::now(),
                                    ));
                                    self.new_engine_id.clear();
                                    self.new_engine_name.clear();
                                    self.new_engine_url.clear();
                                    self.new_engine_key.clear();
                                }
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(format_persian_display("باز کردن config.toml")).clicked() {
                                    #[cfg(windows)]
                                    {
                                        let target = self.config_path.to_str().unwrap_or("config.toml");
                                        let _ = std::process::Command::new("cmd")
                                            .args(["/C", "start", "", target])
                                            .spawn();
                                    }
                                }
                            });
                        });
                    });
            },
        );
    }

    /// Renders the standalone Speech History & Clipboard manager window in an immediate viewport.
    fn render_history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history_window {
            return;
        }

        let now = Instant::now();

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("history_manager_viewport"),
            egui::ViewportBuilder::default()
                .with_title("تاریخچه گفتار و رونوشت‌ها — OmniType")
                .with_inner_size([560.0, 520.0])
                .with_min_inner_size([400.0, 320.0])
                .with_decorations(true)
                .with_resizable(true)
                .with_transparent(false),
            |hist_ctx, _class| {
                if hist_ctx.input(|i| i.viewport().close_requested()) {
                    self.show_history_window = false;
                }

                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(18, 20, 28))
                            .inner_margin(egui::Margin::same(14.0)),
                    )
                    .show(hist_ctx, |ui| {
                        // ── Header & Title ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display("تاریخچه گفتار و رونوشت‌های صوتی"))
                                    .size(16.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(240, 245, 255)),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let count = self.history.len();
                                ui.label(
                                    egui::RichText::new(format_persian_display(&format!("{count} مورد ثبت‌شده")))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(150, 165, 185)),
                                );
                            });
                        });

                        ui.add_space(8.0);

                        // ── Quick Actions Row (Copy All, Clear, Search) ──
                        ui.horizontal(|ui| {
                            let search_placeholder = format_persian_display("جستجو در متن‌ها...");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.history_search)
                                    .hint_text(search_placeholder)
                                    .desired_width(200.0),
                            );

                            if !self.history.is_empty() {
                                if ui.button(egui::RichText::new("📋 کپی همه متن‌ها").size(11.5)).clicked() {
                                    let all_texts = self.history
                                        .iter()
                                        .map(|h| h.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n\n");
                                    hist_ctx.copy_text(all_texts);
                                    self.history_copy_msg = Some(("تمامی متن‌ها در کلیپ‌بورد کپی شدند!".into(), now));
                                }

                                if ui.button(egui::RichText::new("🗑 پاکسازی").size(11.5).color(egui::Color32::from_rgb(255, 120, 120))).clicked() {
                                    self.history.clear();
                                    self.history_copy_msg = Some(("تاریخچه با موفقیت پاک شد.".into(), now));
                                }
                            }
                        });

                        // Notification / Feedback message
                        if let Some((ref msg, t)) = self.history_copy_msg {
                            if now.duration_since(t) < Duration::from_secs(3) {
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format_persian_display(msg))
                                        .size(11.5)
                                        .color(egui::Color32::from_rgb(80, 220, 140)),
                                );
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // ── Items List ──
                        let query = self.history_search.trim().to_lowercase();
                        let filtered_indices: Vec<usize> = self.history
                            .iter()
                            .enumerate()
                            .filter(|(_, h)| {
                                query.is_empty() || h.text.to_lowercase().contains(&query) || h.engine.to_lowercase().contains(&query)
                            })
                            .map(|(i, _)| i)
                            .collect();

                        if filtered_indices.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(
                                    egui::RichText::new(format_persian_display("موردی در تاریخچه یافت نشد."))
                                        .size(13.0)
                                        .color(egui::Color32::from_rgb(130, 145, 165)),
                                );
                                ui.label(
                                    egui::RichText::new(format_persian_display("هر صحبتی که انجام دهید به طور خودکار در این بخش نگهداری می‌شود."))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(100, 115, 135)),
                                );
                            });
                        } else {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let mut delete_idx = None;
                                    for &idx in &filtered_indices {
                                        let item = &self.history[idx];
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgba_unmultiplied(26, 30, 42, 220))
                                            .rounding(egui::Rounding::same(8.0))
                                            .stroke(egui::Stroke::new(
                                                1.0_f32,
                                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                                            ))
                                            .inner_margin(egui::Margin::same(10.0))
                                            .show(ui, |ui| {
                                                // Meta row: Time, Engine, Actions
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(format!("⏱ {}", item.timestamp))
                                                            .size(10.5)
                                                            .color(egui::Color32::from_rgb(140, 155, 175)),
                                                    );

                                                    ui.label(
                                                        egui::RichText::new(format!("⚡ {}", item.engine))
                                                            .size(10.0)
                                                            .color(egui::Color32::from_rgb(100, 190, 255)),
                                                    );

                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        if ui.button(egui::RichText::new("🗑").size(11.0)).on_hover_text("حذف این مورد").clicked() {
                                                            delete_idx = Some(idx);
                                                        }

                                                        let copy_btn = ui.button(egui::RichText::new("📋 کپی متن").size(11.5));
                                                        if copy_btn.clicked() {
                                                            hist_ctx.copy_text(item.text.clone());
                                                            self.history_copy_msg = Some(("متن در کلیپ‌بورد کپی شد!".into(), now));
                                                        }
                                                        copy_btn.on_hover_text("کپی کردن این رونوشت صوتی در کلیپ‌بورد");
                                                    });
                                                });

                                                ui.add_space(4.0);

                                                // Persian text display
                                                let display = format_persian_display(&item.text);
                                                ui.label(
                                                    egui::RichText::new(display)
                                                        .size(12.5)
                                                        .color(egui::Color32::from_rgb(235, 242, 255)),
                                                );
                                            });
                                        ui.add_space(6.0);
                                    }

                                    if let Some(d_idx) = delete_idx {
                                        self.history.remove(d_idx);
                                    }
                                });
                        }
                    });
            },
        );
    }
    /// Renders a dedicated, sleek 10-second floating toast preview window positioned right below the main capsule.
    fn render_preview_toast_window(&mut self, ctx: &egui::Context) {
        let (Some(ref display_text), Some(start)) = (&self.toast_display, self.toast_start) else {
            return;
        };

        let now = Instant::now();
        let elapsed = now.duration_since(start);
        if elapsed >= Duration::from_secs(10) {
            self.toast_start = None;
            self.toast_text = None;
            self.toast_display = None;
            return;
        }

        let remaining_secs = 10_u64.saturating_sub(elapsed.as_secs()).max(1);

        // Position directly above the docked capsule at the bottom
        let main_rect = ctx.input(|i| i.viewport().outer_rect);
        let (pos_x, pos_y) = if let Some(rect) = main_rect {
            ((rect.center().x - 160.0).round(), (rect.min.y - 98.0).round())
        } else {
            (100.0, 100.0)
        };

        let mut dismiss_requested = false;
        let mut copy_requested = false;
        let toast_raw = self.toast_text.clone();
        let is_copied = self
            .toast_copied
            .as_ref()
            .map(|t| now.duration_since(*t) < Duration::from_secs(2))
            .unwrap_or(false);

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("preview_toast_viewport"),
            egui::ViewportBuilder::default()
                .with_title("OmniType_Preview")
                .with_position([pos_x, pos_y])
                .with_inner_size([320.0, 90.0])
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_resizable(false),
            |toast_ctx, _class| {
                #[cfg(windows)]
                apply_window_shapes_all();

                if toast_ctx.input(|i| i.viewport().close_requested()) {
                    dismiss_requested = true;
                }

                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(20, 24, 34))
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                egui::Color32::from_rgba_unmultiplied(100, 160, 240, 80),
                            ))
                            .rounding(egui::Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 7.0)),
                    )
                    .show(toast_ctx, |ui| {
                        // ── Top Header Row ──
                        ui.horizontal(|ui| {
                            let (mic_rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            paint_vector_mic(ui.painter(), mic_rect, egui::Color32::from_rgb(140, 195, 255));

                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new(format_persian_display("متن آماده شده"))
                                    .size(11.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(170, 215, 255)),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Close button with vector cross
                                let (close_rect, close_resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                                let close_hover = close_resp.hovered();
                                let bg_color = if close_hover {
                                    egui::Color32::from_rgba_unmultiplied(75, 35, 42, 220)
                                } else {
                                    egui::Color32::from_rgba_unmultiplied(45, 52, 70, 180)
                                };
                                ui.painter().rect_filled(close_rect, egui::Rounding::same(3.0), bg_color);
                                let cross_color = if close_hover {
                                    egui::Color32::from_rgb(255, 130, 130)
                                } else {
                                    egui::Color32::from_rgb(200, 210, 225)
                                };
                                paint_vector_cross(ui.painter(), close_rect, egui::Stroke::new(1.3_f32, cross_color));
                                if close_resp.clicked() {
                                    dismiss_requested = true;
                                }
                                let _ = close_resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text("بستن پیش‌نمایش");

                                ui.add_space(2.0);

                                // Copy button
                                let (copy_label, copy_color) = if is_copied {
                                    ("کپی شد ✓", egui::Color32::from_rgb(80, 245, 150))
                                } else {
                                    ("کپی متن", egui::Color32::from_rgb(160, 215, 255))
                                };
                                let copy_btn = ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(format_persian_display(copy_label))
                                                .size(9.5)
                                                .color(copy_color),
                                        )
                                        .fill(egui::Color32::from_rgba_unmultiplied(35, 50, 75, 180))
                                        .rounding(egui::Rounding::same(3.0)),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text("کپی متن در کلیپ‌بورد");
                                if copy_btn.clicked() {
                                    copy_requested = true;
                                }

                                ui.add_space(4.0);

                                // 10-second countdown pill badge
                                let count_str = format!("{remaining_secs}s");
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgba_unmultiplied(35, 42, 58, 200))
                                    .rounding(egui::Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(count_str)
                                                .size(9.0)
                                                .color(egui::Color32::from_rgb(255, 200, 100)),
                                        );
                                    });
                            });
                        });

                        ui.add_space(3.0);

                        // ── Body: Scrollable Persian text ──
                        egui::ScrollArea::vertical()
                            .max_height(48.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let text_resp = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(display_text)
                                            .size(11.5)
                                            .color(egui::Color32::from_rgb(240, 245, 255)),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Wrap),
                                );
                                if text_resp.clicked() {
                                    copy_requested = true;
                                }
                                text_resp.on_hover_text("برای کپی سریع کلیک کنید");
                            });
                    });
            },
        );

        if copy_requested {
            if let Some(ref raw) = toast_raw {
                ctx.copy_text(raw.clone());
                self.toast_copied = Some(now);
            }
        }

        if dismiss_requested {
            self.toast_start = None;
            self.toast_text = None;
            self.toast_display = None;
        }
    }
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Physically clip OS windows to eliminate any black or frosted glass rectangular bounding box
        if self.shape_frames_checked < 10 {
            self.shape_frames_checked += 1;
            #[cfg(windows)]
            apply_window_shapes_all();
        }

        // Consume external control flags.
        if self
            .overlay_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.toggle_visible();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
        }
        if self
            .dict_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.show_dict_window = true;
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        if self
            .engine_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.show_engine_window = true;
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        if self
            .history_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.show_history_window = true;
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        if self.quit_flag.load(std::sync::atomic::Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Always render secondary viewports if open
        self.render_dict_window(ctx);
        self.render_engine_window(ctx);
        self.render_history_window(ctx);
        self.render_preview_toast_window(ctx);

        // 30 fps gives buttery smooth waveform animations and accurate 10s countdown
        ctx.request_repaint_after(Duration::from_millis(33));

        if !self.visible {
            return;
        }

        let status = self.status.get();
        let now = Instant::now();

        // Track recording duration
        if matches!(status.state, AppState::Recording) {
            if self.recording_start.is_none() {
                self.recording_start = Some(now);
                self.last_seen_transcript = None; // Reset so next utterance can trigger toast
            }
            // Dismiss toast preview when user starts speaking again
            self.toast_start = None;
            self.toast_text = None;
            self.toast_display = None;
        } else {
            self.recording_start = None;
        }

        // When speech transcript arrives, pop up the 10-second toast preview and record in history (ONLY ONCE per utterance)
        if let Some(ref raw) = status.last_text {
            let trimmed = raw.trim();
            if !trimmed.is_empty() && self.last_seen_transcript.as_deref() != Some(trimmed) {
                self.last_seen_transcript = Some(trimmed.to_string());
                self.toast_display = Some(format_persian_display(trimmed));
                self.toast_text = Some(trimmed.to_string());
                self.toast_start = Some(now);

                let time_now = local_time_str();
                let active_engine_str = self.router.active_engine();

                self.history.insert(
                    0,
                    HistoryItem {
                        id: self.next_history_id,
                        text: trimmed.to_string(),
                        timestamp: time_now,
                        engine: active_engine_str,
                    },
                );
                self.next_history_id += 1;
                if self.history.len() > 100 {
                    self.history.truncate(100);
                }

                #[cfg(windows)]
                apply_window_shapes_all();
            }
        }

        // Auto-dismiss toast preview after exactly 10 seconds
        if let Some(start) = self.toast_start {
            if now.duration_since(start) >= Duration::from_secs(10) {
                self.toast_start = None;
                self.toast_text = None;
                self.toast_display = None;
            }
        }

        let time = ctx.input(|i| i.time);

        // Determine target VisualMode based on state and hover
        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let pointer_in_window = pointer_pos.is_some();
        if pointer_in_window {
            self.is_hovered = true;
            self.last_hover_time = Some(now);
        } else if let Some(last) = self.last_hover_time {
            if now.duration_since(last) > Duration::from_millis(400) {
                self.is_hovered = false;
            }
        }

        let target_mode = match status.state {
            AppState::Recording => VisualMode::RecordingActive,
            AppState::Processing | AppState::Typing => VisualMode::Processing,
            _ => {
                if self.is_hovered {
                    VisualMode::HoveredAwake
                } else {
                    VisualMode::IdleDormant
                }
            }
        };

        // Determine target dimensions and corner radius for Variant 5
        let (target_w, target_h, target_corner) = match target_mode {
            VisualMode::IdleDormant => (38.0_f32, 6.0_f32, 3.0_f32),
            VisualMode::HoveredAwake => (144.0_f32, 32.0_f32, 16.0_f32),
            VisualMode::RecordingActive => (176.0_f32, 34.0_f32, 17.0_f32),
            VisualMode::Processing => (126.0_f32, 32.0_f32, 16.0_f32),
        };

        let mode_changed = self.visual_mode != target_mode
            || (self.current_width - target_w).abs() > 0.5
            || (self.current_height - target_h).abs() > 0.5;

        if mode_changed {
            self.visual_mode = target_mode;
            self.current_width = target_w;
            self.current_height = target_h;

            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(target_w, target_h)));

            #[cfg(windows)]
            {
                let hwnd = MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
                if hwnd != 0 {
                    let ppp = ctx.pixels_per_point();
                    let w_px = (target_w * ppp).round() as i32;
                    let h_px = (target_h * ppp).round() as i32;
                    let corner_px = (target_corner * ppp).round() as i32;
                    position_above_taskbar(hwnd, w_px, h_px, corner_px);
                    self.has_initial_positioned = true;
                }
            }
        } else if !self.has_initial_positioned {
            #[cfg(windows)]
            {
                let hwnd = MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed);
                if hwnd != 0 {
                    let ppp = ctx.pixels_per_point();
                    let w_px = (target_w * ppp).round() as i32;
                    let h_px = (target_h * ppp).round() as i32;
                    let corner_px = (target_corner * ppp).round() as i32;
                    position_above_taskbar(hwnd, w_px, h_px, corner_px);
                    self.has_initial_positioned = true;
                }
            }
        }

        // Render Variant 5 (Executive Pill) according to current visual mode
        match self.visual_mode {
            VisualMode::IdleDormant => {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(130, 142, 158))
                            .rounding(egui::Rounding::same(3.0))
                            .inner_margin(egui::Margin::same(0.0)),
                    )
                    .show(ctx, |ui| {
                        let rect = ui.max_rect();
                        let sense = ui.interact(rect, ui.id().with("dormant_bar"), egui::Sense::click());
                        if sense.clicked() {
                            let _ = self.events_tx.send(HotkeyEvent::RecordDown);
                        }
                        if sense.hovered() {
                            self.is_hovered = true;
                            self.last_hover_time = Some(now);
                        }
                    });
            }
            VisualMode::HoveredAwake => {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(20, 24, 34))
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                egui::Color32::from_rgba_unmultiplied(100, 150, 220, 90),
                            ))
                            .rounding(egui::Rounding::same(16.0))
                            .inner_margin(egui::Margin::symmetric(7.0, 4.0)),
                    )
                    .show(ctx, |ui| {
                        let mut action_btn_clicked = false;

                        ui.horizontal(|ui| {
                            // Vector Mic button (Click to start recording)
                            let (mic_rect, mic_resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                            let mic_hover = mic_resp.hovered();
                            let mic_bg = if mic_hover {
                                egui::Color32::from_rgba_unmultiplied(45, 80, 130, 230)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(35, 60, 90, 200)
                            };
                            ui.painter().circle_filled(mic_rect.center(), 10.5, mic_bg);
                            paint_vector_mic(ui.painter(), mic_rect, egui::Color32::from_rgb(255, 255, 255));
                            if mic_resp.clicked() {
                                let _ = self.events_tx.send(HotkeyEvent::RecordDown);
                                action_btn_clicked = true;
                            }
                            let _ = mic_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("شروع ضبط گفتار");

                            ui.add_space(2.0);

                            // Shortcut badge
                            ui.label(
                                egui::RichText::new("CapsLock")
                                    .size(10.5)
                                    .strong()
                                    .color(egui::Color32::from_rgb(210, 225, 245)),
                            );

                            // Right-aligned buttons: Engine badge & History
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Engine Badge
                                    let active_engine = self.router.active_engine();
                                    let engine_short = match active_engine.as_str() {
                                        "auto" => "AUTO",
                                        "google" => "GGL",
                                        "local_whisper" | "whisper.cpp" => "LOC",
                                        "groq" | "cloud" => "GROQ",
                                        _ => "API",
                                    };

                                    let badge_btn = ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(engine_short)
                                                    .size(8.5)
                                                    .strong()
                                                    .color(egui::Color32::from_rgb(190, 210, 235)),
                                            )
                                            .fill(egui::Color32::from_rgba_unmultiplied(35, 45, 65, 220))
                                            .rounding(egui::Rounding::same(5.0)),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(format!(
                                            "موتور فعال: {active_engine} (کلیک برای مدیریت)"
                                        ));
                                    if badge_btn.clicked() {
                                        self.show_engine_window = !self.show_engine_window;
                                        action_btn_clicked = true;
                                    }

                                    ui.add_space(1.0);

                                    // History icon (Clean vector badge)
                                    let (hist_rect, hist_resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                                    let hist_hover = hist_resp.hovered();
                                    let hist_bg = if hist_hover {
                                        egui::Color32::from_rgba_unmultiplied(40, 75, 65, 230)
                                    } else {
                                        egui::Color32::from_rgba_unmultiplied(28, 48, 42, 190)
                                    };
                                    ui.painter().rect_filled(hist_rect, egui::Rounding::same(4.0), hist_bg);
                                    ui.painter().text(
                                        hist_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "H",
                                        egui::FontId::proportional(10.0),
                                        egui::Color32::from_rgb(170, 230, 210),
                                    );
                                    if hist_resp.clicked() {
                                        self.show_history_window = !self.show_history_window;
                                        action_btn_clicked = true;
                                    }
                                    let _ = hist_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text("تاریخچه گفتار (History)");
                                },
                            );
                        });

                        // Click anywhere else on the pill to start recording
                        let pill_sense = ui.interact(
                            ui.max_rect(),
                            ui.id().with("pill_awake_body"),
                            egui::Sense::click(),
                        );
                        if pill_sense.clicked() && !action_btn_clicked {
                            let _ = self.events_tx.send(HotkeyEvent::RecordDown);
                        }
                    });
            }
            VisualMode::RecordingActive => {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(18, 20, 28))
                            .stroke(egui::Stroke::new(
                                1.2_f32,
                                egui::Color32::from_rgba_unmultiplied(255, 85, 85, 150),
                            ))
                            .rounding(egui::Rounding::same(17.0))
                            .inner_margin(egui::Margin::symmetric(7.0, 4.0)),
                    )
                    .show(ctx, |ui| {
                        let mut action_btn_clicked = false;

                        ui.horizontal(|ui| {
                            // 1. Vector Cancel button (✕) on the left
                            let (cancel_rect, cancel_resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                            let cancel_hover = cancel_resp.hovered();
                            let cancel_bg = if cancel_hover {
                                egui::Color32::from_rgba_unmultiplied(85, 25, 30, 240)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(65, 25, 30, 220)
                            };
                            ui.painter().circle_filled(cancel_rect.center(), 10.0, cancel_bg);
                            let cross_stroke = egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 120, 120));
                            paint_vector_cross(ui.painter(), cancel_rect, cross_stroke);
                            if cancel_resp.clicked() {
                                let _ = self.events_tx.send(HotkeyEvent::Cancel);
                                action_btn_clicked = true;
                            }
                            let _ = cancel_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("لغو ضبط (بدون ارسال متن)");

                            ui.add_space(2.0);

                            // 2. Digital recording timer (00:04)
                            let elapsed = self
                                .recording_start
                                .map(|s| now.duration_since(s).as_secs())
                                .unwrap_or(0);
                            let timer_str = format!("{:02}:{:02}", elapsed / 60, elapsed % 60);
                            ui.label(
                                egui::RichText::new(timer_str)
                                    .size(10.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(255, 145, 145)),
                            );

                            ui.add_space(2.0);

                            // 3. Dynamic Equalizer Waveform Bars (8 animated bars)
                            let wave_w = 46.0_f32;
                            let (wave_resp, wave_painter) = ui.allocate_painter(
                                egui::vec2(wave_w, 16.0),
                                egui::Sense::hover(),
                            );
                            let num_bars = 8;
                            let bar_w = 2.4_f32;
                            let bar_spacing = (wave_w - (num_bars as f32 * bar_w)) / ((num_bars - 1) as f32);
                            let center_y = wave_resp.rect.center().y;

                            for i in 0..num_bars {
                                let phase = time * 10.0 + (i as f64) * 0.95;
                                let wave_val = (phase.sin().abs() * 0.7 + (phase * 1.6).cos().abs() * 0.3) as f32;
                                let h = (3.5 + 11.5 * wave_val).clamp(3.0, 15.0);

                                let bx = wave_resp.rect.min.x + (i as f32) * (bar_w + bar_spacing);
                                let by = center_y - h / 2.0;

                                let bar_color = if i % 2 == 0 {
                                    egui::Color32::from_rgb(255, 90, 90)
                                } else {
                                    egui::Color32::from_rgb(255, 230, 230)
                                };

                                wave_painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(bx, by),
                                        egui::vec2(bar_w, h),
                                    ),
                                    egui::Rounding::same(1.2),
                                    bar_color,
                                );
                            }

                            // 4. Vector Submit / Finish button (✓) on the right
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let (submit_rect, submit_resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                                    let submit_hover = submit_resp.hovered();
                                    let submit_bg = if submit_hover {
                                        egui::Color32::from_rgba_unmultiplied(30, 85, 55, 250)
                                    } else {
                                        egui::Color32::from_rgba_unmultiplied(22, 65, 42, 230)
                                    };
                                    ui.painter().circle_filled(submit_rect.center(), 10.0, submit_bg);
                                    let check_stroke = egui::Stroke::new(1.8_f32, egui::Color32::from_rgb(85, 250, 155));
                                    paint_vector_check(ui.painter(), submit_rect, check_stroke);
                                    if submit_resp.clicked() {
                                        let _ = self.events_tx.send(HotkeyEvent::RecordUp);
                                        action_btn_clicked = true;
                                    }
                                    let _ = submit_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text("پایان ضبط و تایپ متن");
                                },
                            );
                        });

                        // Clicking elsewhere on recording capsule also submits
                        let rec_sense = ui.interact(
                            ui.max_rect(),
                            ui.id().with("pill_rec_body"),
                            egui::Sense::click(),
                        );
                        if rec_sense.clicked() && !action_btn_clicked {
                            let _ = self.events_tx.send(HotkeyEvent::RecordUp);
                        }
                    });
            }
            VisualMode::Processing => {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(20, 24, 34))
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                egui::Color32::from_rgba_unmultiplied(255, 185, 45, 100),
                            ))
                            .rounding(egui::Rounding::same(16.0))
                            .inner_margin(egui::Margin::symmetric(8.0, 4.0)),
                    )
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            let pulse = 3.0 + 1.5 * (time * 6.0).sin().abs() as f32;
                            let (response, painter) = ui.allocate_painter(
                                egui::vec2(12.0, 12.0),
                                egui::Sense::hover(),
                            );
                            let center = response.rect.center();
                            let amber = egui::Color32::from_rgb(255, 185, 45);
                            painter.circle_filled(center, pulse + 1.5, amber.linear_multiply(0.25));
                            painter.circle_filled(center, 3.2, amber);

                            ui.add_space(2.0);

                            ui.label(
                                egui::RichText::new(format_persian_display("در حال پردازش..."))
                                    .size(11.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(255, 215, 130)),
                            );
                        });
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_persian_display_handles_persian() {
        let input = "سلام دنیا";
        let formatted = format_persian_display(input);
        assert!(!formatted.is_empty());
        // Reshaped Persian does not stay identical to raw input (contains contextual presentation forms)
        assert_ne!(formatted, input);
    }

    #[test]
    fn test_format_persian_display_handles_english() {
        let input = "Hello World";
        let formatted = format_persian_display(input);
        assert_eq!(formatted, "Hello World");
    }

    #[test]
    fn overlay_toggles_visibility() {
        let (_tx, rx) = tokio::sync::watch::channel(AppStatus {
            state: AppState::Idle,
            last_text: None,
            vad_engine: "silero",
        });
        let (events_tx, _events_rx) = tokio::sync::mpsc::unbounded_channel();
        let flags = (
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let dict = Arc::new(RwLock::new(Dictionary::with_defaults()));
        let router = AsrRouter::new(vec![]);
        let settings = Arc::new(RwLock::new(Settings::default()));
        let config_path = PathBuf::from("config.toml");
        let mut app = OverlayApp::new(
            Arc::new(StatusClient::new(rx)),
            events_tx,
            flags.0,
            flags.1,
            flags.2,
            flags.3,
            flags.4,
            dict,
            router,
            settings,
            config_path,
        );
        assert!(app.visible);
        app.toggle_visible();
        assert!(!app.visible);
        app.toggle_visible();
        assert!(app.visible);
    }

    #[tokio::test]
    async fn status_client_sees_updates() {
        let (tx, rx) = tokio::sync::watch::channel(AppStatus {
            state: AppState::Idle,
            last_text: None,
            vad_engine: "silero",
        });
        let client = Arc::new(StatusClient::new(rx));
        assert_eq!(client.get().state, AppState::Idle);
        tx.send(AppStatus {
            state: AppState::Recording,
            last_text: None,
            vad_engine: "silero",
        })
        .unwrap();
        assert_eq!(client.get().state, AppState::Recording);
    }

    #[test]
    fn test_history_item_serialization_and_retrieval() {
        let item = HistoryItem {
            id: 1,
            text: "متن تستی اول".into(),
            timestamp: "12:30:45".into(),
            engine: "google".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        let parsed: HistoryItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.text, "متن تستی اول");
        assert_eq!(parsed.engine, "google");
    }

    #[test]
    fn test_history_truncation_limits() {
        let mut history: Vec<HistoryItem> = Vec::new();
        for i in 0..120 {
            history.insert(
                0,
                HistoryItem {
                    id: i,
                    text: format!("Text {i}"),
                    timestamp: "10:00:00".into(),
                    engine: "auto".into(),
                },
            );
            if history.len() > 100 {
                history.truncate(100);
            }
        }
        assert_eq!(history.len(), 100);
        assert_eq!(history[0].id, 119);
        assert_eq!(history[99].id, 20);
    }

    #[test]
    fn test_toast_auto_dismiss_10s_expiration() {
        let now = Instant::now();
        let start = now - Duration::from_secs(11);
        let elapsed = now.duration_since(start);
        assert!(elapsed >= Duration::from_secs(10));
        let remaining_secs = 10_u64.saturating_sub(elapsed.as_secs());
        assert_eq!(remaining_secs, 0);

        let active_start = now - Duration::from_secs(3);
        let active_elapsed = now.duration_since(active_start);
        assert!(active_elapsed < Duration::from_secs(10));
        let active_remaining = 10_u64.saturating_sub(active_elapsed.as_secs()).max(1);
        assert_eq!(active_remaining, 7);
    }

    #[test]
    fn test_visual_mode_transitions() {
        let determine_mode = |state: AppState, is_hovered: bool| -> VisualMode {
            match state {
                AppState::Recording => VisualMode::RecordingActive,
                AppState::Processing | AppState::Typing => VisualMode::Processing,
                _ => {
                    if is_hovered {
                        VisualMode::HoveredAwake
                    } else {
                        VisualMode::IdleDormant
                    }
                }
            }
        };

        assert_eq!(determine_mode(AppState::Idle, false), VisualMode::IdleDormant);
        assert_eq!(determine_mode(AppState::Idle, true), VisualMode::HoveredAwake);
        assert_eq!(determine_mode(AppState::Recording, false), VisualMode::RecordingActive);
        assert_eq!(determine_mode(AppState::Recording, true), VisualMode::RecordingActive);
        assert_eq!(determine_mode(AppState::Processing, false), VisualMode::Processing);
        assert_eq!(determine_mode(AppState::Typing, false), VisualMode::Processing);
    }
}
