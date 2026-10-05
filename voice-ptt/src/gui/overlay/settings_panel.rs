//! Settings tab: hotkeys, update policy and the cloud-consent switch.
//!
//! Owns [`SettingsPanelState`] — the draft and its UI state — and reads the
//! live [`Settings`]. This is the only tab that can write `config.toml`, and
//! it can only do so from a draft that [`super::validate_settings`]
//! has accepted.

use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use eframe::egui;
use egui_phosphor::regular as ic;

use super::text::{egui_key_to_hotkey_token, format_persian_display};
use super::theme::*;
use crate::config::settings::Settings;
use crate::hotkey::binding::HotkeyBinding;
use crate::hotkey::{CaptureOutcome, HotkeyConfig, HotkeyControl};
use crate::updates::SharedUpdateState;

/// The Settings tab's state: the unvalidated draft plus the UI around it.
///
/// The draft is a full copy of [`Settings`] rather than a diff. That is a
/// deliberate trade: it costs a clone on every save and buys the guarantee
/// that a half-typed hotkey or an empty API key can never reach `config.toml`
/// — the draft is only written after [`super::validate_settings`]
/// passes.
pub(crate) struct SettingsPanelState {
    /// Pending edits, applied to the real settings only on a validated save.
    pub draft: Settings,
    /// Per-field hotkey capture state: when true, the next key press is
    /// recorded into the corresponding setting instead of being typed.
    pub capturing_hotkey: [bool; 3],
    /// True while the draft was validated and saved since the last disk read.
    pub saved: bool,
    /// Inline validation error for the current draft (empty = valid).
    pub error: Option<String>,
    /// Transient capture feedback (saved / rejected reason), auto-expires.
    pub msg: Option<(String, Instant)>,
}

impl SettingsPanelState {
    /// Starts from the settings on disk, so the form always opens on
    /// something valid.
    pub(crate) fn from_settings(settings: &Settings) -> Self {
        Self {
            draft: settings.clone(),
            capturing_hotkey: [false; 3],
            saved: false,
            error: None,
            msg: None,
        }
    }

    /// Pushes the draft hotkeys into the *running* listener and persists them.
    ///
    /// Called right after a key is captured so the new shortcut is live
    /// immediately (the poll thread re-reads its bindings every 10 ms); the
    /// save button calls it too, which is idempotent.
    pub(crate) fn apply_hotkeys_live(
        &mut self,
        hotkey: Option<&HotkeyControl>,
        settings: &Arc<RwLock<Settings>>,
        config_path: &Path,
    ) {
        let hotkey_settings = self.draft.hotkey.clone();
        if let Some(hotkey) = hotkey {
            hotkey.set_config(HotkeyConfig::from_settings(&hotkey_settings));
        }
        if let Ok(mut s) = settings.write() {
            s.hotkey = hotkey_settings;
            let _ = s.save(config_path);
        }
        self.saved = true;
    }
}

/// Renders the Settings tab body into the dashboard's current panel.
///
/// The dependencies are the ones the tab genuinely does not own: the live
/// settings (the draft is a copy), the running hotkey listener it re-binds,
/// where `config.toml` lives, and the shared update state it reports on.
/// The three orb controls the settings card edits, plus one command button.
///
/// A plain struct rather than a callback per field: the panel edits values, and
/// the overlay decides what a changed value means. Keeping them in one place is
/// what stops the panel from reaching into the orb — the whole reason
/// [`crate::gui::orb_idle_policy`] is a separate decision-making type.
#[derive(Debug, Clone)]
pub struct OrbControls {
    pub return_enabled: bool,
    pub return_after_idle_secs: u64,
    pub pinned: bool,
    /// Which corner the orb walks back to. The user's, because the sensible
    /// corner depends on where their taskbar is — a bottom-right corner is the
    /// out-of-the-way one on a machine whose taskbar is at the bottom.
    pub return_corner: String,
    /// Size as a percentage of the shipped size.
    pub scale_percent: u32,
    /// Set by the button, read and cleared by the overlay. A flag rather than a
    /// closure so the panel never needs a handle on the app.
    pub return_to_manual: bool,
}

pub(crate) fn render(
    ui: &mut egui::Ui,
    state: &mut SettingsPanelState,
    settings: &Arc<RwLock<Settings>>,
    hotkey: Option<&HotkeyControl>,
    config_path: &Path,
    update_state: &SharedUpdateState,
    orb_controls: Option<&mut OrbControls>,
) {
    // Hotkey capture: if a field is armed, the next chord the user presses
    // becomes the new binding. The chord is read by the *global* keyboard
    // poller (`HotkeyControl`) because this window normally does not hold
    // keyboard focus, so egui cannot see the keys at all; the egui path
    // below is only a fallback for builds without a listener.
    if state.capturing_hotkey.iter().any(|c| *c) {
        let ctx = ui.ctx().clone();

        let outcome: Option<CaptureOutcome> = match &hotkey {
            Some(hotkey) => hotkey.take_capture(),
            None => {
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    Some(CaptureOutcome::Cancelled)
                } else {
                    egui_key_to_hotkey_token(&ctx).map(CaptureOutcome::Binding)
                }
            }
        };

        match outcome {
            Some(CaptureOutcome::Binding(token)) => {
                // Capture the armed slot index BEFORE clearing it so the new
                // binding lands in the field the user actually armed.
                let armed_idx = state.capturing_hotkey.iter().position(|c| *c);
                state.capturing_hotkey = [false; 3];
                // Only store chords the parser accepts, so a captured key
                // can never leave an unloadable config behind.
                if let (Some(idx), Ok(_)) = (armed_idx, HotkeyBinding::parse(&token)) {
                    match idx {
                        0 => state.draft.hotkey.record = token,
                        1 => state.draft.hotkey.toggle_overlay = token,
                        _ => state.draft.hotkey.quit = token,
                    }
                    state.apply_hotkeys_live(hotkey, settings, config_path);
                    state.msg = Some(("کلید جدید ذخیره شد".to_string(), Instant::now()));
                }
                ctx.request_repaint();
            }
            Some(CaptureOutcome::Rejected(reason)) => {
                state.capturing_hotkey = [false; 3];
                // A refused chord must never be silent: tell the user why.
                let msg = if reason.contains("Windows-key") {
                    "ترکیب با کلید ویندوز پشتیبانی نمی‌شود — کلید دیگری انتخاب کنید"
                } else {
                    "این کلید قابل انتساب نیست — کلید دیگری انتخاب کنید"
                };
                state.msg = Some((msg.to_string(), Instant::now()));
                ctx.request_repaint();
            }
            Some(CaptureOutcome::Cancelled) => {
                state.capturing_hotkey = [false; 3];
                ctx.request_repaint();
            }
            None => {
                // Still waiting for the chord: keep the "..." animating.
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
    }

    let engine_count = settings
        .read()
        .map(|s| s.custom_providers.len())
        .unwrap_or(0);
    manager_header(
        ui,
        "تنظیمات",
        Some((
            &format!("{} موتور سفارشی", engine_count),
            palette::HEADER_PILL_BG,
            palette::ACCENT_SOFT,
        )),
    );
    manager_subtitle(
        ui,
        "پیکربندی صوت، موتور تشخیص گفتار، تشخیص سکوت، کلیدها و دریافت به‌روزرسانی",
    );
    ui.add_space(8.0);

    // Pre-validate draft settings so save state and error notices are ready
    let validation = super::validate_settings(&state.draft);
    match &validation {
        Ok(()) => {
            state.error = None;
        }
        Err(msg) => {
            state.error = Some(msg.clone());
        }
    }

    // 2-Column Bento / Masonry Layout, RTL:
    // `ui.columns(2, ...)` resets each column's layout to LTR top_down_justified(Align::LEFT),
    // so we explicitly wrap each column in `push_id` and `Layout::top_down(Align::RIGHT)`
    // and avoid `egui::Grid` (which produces `-f32::INFINITY` cursor coordinates in RTL).
    ui.columns(2, |cols| {
            // ── Right Column: AI Core, Hotkeys, Updates (cols[1]) ──
            cols[1].push_id("settings_col_right", |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                    |ui| {
                        // ── Card 1: ASR Engine (موتور تشخیص گفتار) ──
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}",
                                    format_persian_display("موتور تشخیص گفتار"),
                                    ic::BRAIN
                                ))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_SECTION),
                            );
                            ui.add_space(6.0);

                            // 2x2 RTL rows for engine radio selection (without egui::Grid)
                            ui.horizontal(|ui| {
                                for (id, label) in [("auto", "خودکار"), ("google", "گوگل رایگان")] {
                                    if ui
                                        .radio(
                                            state.draft.active_engine == *id,
                                            format_persian_display(label),
                                        )
                                        .clicked()
                                    {
                                        state.draft.active_engine = (*id).to_string();
                                    }
                                    ui.add_space(8.0);
                                }
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                for (id, label) in [
                                    ("local_whisper", "ویسپر محلی"),
                                    ("groq", "ابر (Groq)"),
                                ] {
                                    if ui
                                        .radio(
                                            state.draft.active_engine == *id,
                                            format_persian_display(label),
                                        )
                                        .clicked()
                                    {
                                        state.draft.active_engine = (*id).to_string();
                                    }
                                    ui.add_space(8.0);
                                }
                            });

                            ui.add_space(6.0);
                            if state.draft.active_engine == "groq" {
                                callout(
                                    ui,
                                    CalloutKind::Warning,
                                    "با انتخاب موتور ابری، صوت شما برای پردازش به سرور خارجی ارسال می‌شود.",
                                );
                            } else if state.draft.active_engine == "google" {
                                callout(
                                    ui,
                                    CalloutKind::Info,
                                    "گوگل رایگان نیازی به کلید API ندارد، اما به اینترنت متصل می‌ماند.",
                                );
                            } else if state.draft.active_engine == "local_whisper" {
                                callout(
                                    ui,
                                    CalloutKind::Info,
                                    "ویسپر محلی کاملاً آفلاین است؛ هیچ صوتی ماشین شما را ترک نمی‌کند.",
                                );
                            }
                            ui.add_space(6.0);

                            let label_w = 98.0;
                            rtl_form_row(ui, "مدل محلی:", label_w, |ui| {
                                let _ = egui::ComboBox::from_id_source("settings_local_model")
                                    .width(140.0)
                                    .selected_text(state.draft.asr.model.clone())
                                    .show_ui(ui, |ui| {
                                        for m in [
                                            "auto",
                                            "tiny",
                                            "base",
                                            "small",
                                            "medium",
                                            "large-v3",
                                            "large-v3-turbo",
                                        ] {
                                            ui.selectable_value(
                                                &mut state.draft.asr.model,
                                                (*m).to_string(),
                                                m,
                                            );
                                        }
                                    });
                            });
                            ui.add_space(6.0);

                            rtl_form_row(ui, "زبان تشخیص:", label_w, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut state.draft.asr.language)
                                        .desired_width(90.0),
                                );
                            });
                            ui.add_space(6.0);

                            rtl_form_row(ui, "سقف روزانه ابر:", label_w, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut state.draft.cloud.daily_limit)
                                        .range(1..=10_000),
                                );
                            });
                        });

                        ui.add_space(8.0);

                        // ── Card 2: Hotkeys & GUI (کلیدهای میانبر و رابط کاربری) ──
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}",
                                    format_persian_display("کلیدهای میانبر و رابط کاربری"),
                                    ic::KEYBOARD
                                ))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_SECTION),
                            );
                            ui.add_space(6.0);

                            let hk_label_w = 115.0;
                            // Set by whichever button is clicked; consumed once
                            // the card has been laid out, so the capture arms
                            // exactly once per click (not once per frame).
                            let mut start_capture = false;
                            let render_hk_row = |ui: &mut egui::Ui,
                                                 label: &str,
                                                 value: &mut String,
                                                 capturing: &mut bool,
                                                 start_capture: &mut bool| {
                                rtl_form_row(ui, label, hk_label_w, |ui| {
                                    let btn_text = if *capturing {
                                        "...".to_string()
                                    } else {
                                        format_persian_display(value)
                                    };
                                    let btn = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(btn_text)
                                                .size(11.0)
                                                .color(if *capturing {
                                                    palette::ACCENT
                                                } else {
                                                    palette::TEXT_TABLE
                                                }),
                                        )
                                        .fill(if *capturing {
                                            palette::SELECTED_BG
                                        } else {
                                            palette::CHIP_BG
                                        })
                                        .rounding(egui::Rounding::same(5.0))
                                        .min_size(egui::vec2(130.0, 24.0)),
                                    );
                                    if btn.clicked() {
                                        *capturing = true;
                                        *start_capture = true;
                                    }
                                });
                            };

                            render_hk_row(
                                ui,
                                "کلید ضبط:",
                                &mut state.draft.hotkey.record,
                                &mut state.capturing_hotkey[0],
                                &mut start_capture,
                            );
                            ui.add_space(6.0);
                            render_hk_row(
                                ui,
                                "نمایش/مخفی کپسول:",
                                &mut state.draft.hotkey.toggle_overlay,
                                &mut state.capturing_hotkey[1],
                                &mut start_capture,
                            );
                            ui.add_space(6.0);
                            render_hk_row(
                                ui,
                                "کلید خروج:",
                                &mut state.draft.hotkey.quit,
                                &mut state.capturing_hotkey[2],
                                &mut start_capture,
                            );

                            // Arm the system-wide capture. Any single key or
                            // chord the user presses next becomes the binding.
                            if start_capture {
                                if let Some(hotkey) = hotkey {
                                    hotkey.begin_capture();
                                }
                            }

                            // Transient feedback for capture results so a
                            // rejected chord is never silently swallowed.
                            let expired = state.msg
                                .as_ref()
                                .is_some_and(|(_, at)| at.elapsed() > std::time::Duration::from_secs(4));
                            if expired {
                                state.msg = None;
                            }
                            if let Some((msg, _)) = &state.msg {
                                ui.label(
                                    egui::RichText::new(format_persian_display(msg))
                                        .size(10.5)
                                        .color(palette::WARNING),
                                );
                            }

                            ui.add_space(6.0);
                            ui.checkbox(
                                &mut state.draft.gui.show_overlay,
                                format_persian_display("نمایش کپسول شناور"),
                            );
                            ui.add_space(4.0);
                            // Off by default, and the tooltip says why: the burst
                            // form fails *visibly and early* when the platform
                            // refuses input, which pacing trades away.
                            ui.checkbox(
                                &mut state.draft.gui.type_progressively,
                                format_persian_display("تایپ تدریجی متن"),
                            )
                            .on_hover_text(
                                "type each dictation word by word instead of all at once.\nOff by default: one burst fails visibly if the platform refuses input,\nwhich pacing gives up.",
                            );
                            ui.checkbox(
                                &mut state.draft.gui.review_before_insert,
                                format_persian_display("بازبینی متن پیش از درج"),
                            )
                            .on_hover_text(
                                "show the finished text before it is typed, so it can be edited, inserted, copied or dropped.\nOff by default: direct typing is what makes a dictation feel like talking.\nThis does not affect recovery — text whose insert failed is always offered.",
                            );
                        });

                        ui.add_space(8.0);

                        // ── Card 3: Software Updates (به‌روزرسانی نرم‌افزار) ──
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}",
                                    format_persian_display("به‌روزرسانی نرم‌افزار"),
                                    ic::CLOUD_ARROW_DOWN
                                ))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_SECTION),
                            );
                            ui.add_space(6.0);

                            let current_ver = env!("CARGO_PKG_VERSION");
                            ui.label(
                                egui::RichText::new(format_persian_display(&format!(
                                    "نگارش فعلی: v{current_ver}"
                                )))
                                .size(11.0)
                                .color(palette::TEXT_LABEL),
                            );
                            ui.add_space(4.0);

                            let current_state = {
                                if let Ok(st) = update_state.read() {
                                    (*st).clone()
                                } else {
                                    crate::updates::UpdateState::Idle
                                }
                            };

                            match current_state {
                                crate::updates::UpdateState::Checking => {
                                    ui.horizontal(|ui| {
                                        ui.add(egui::Spinner::new().size(12.0).color(palette::ACCENT));
                                        ui.label(
                                            egui::RichText::new(format_persian_display(
                                                "در حال بررسی سرور...",
                                            ))
                                            .size(10.5)
                                            .color(palette::TEXT_MUTED),
                                        );
                                    });
                                }
                                crate::updates::UpdateState::Available(ref info) => {
                                    ui.horizontal(|ui| {
                                        status_chip(
                                            ui,
                                            &format!("نسخه جدید: v{}", info.latest_version),
                                            palette::SUCCESS_FILL,
                                            palette::SUCCESS,
                                            10.5,
                                            ChipFamily::Small,
                                        );
                                    });
                                    ui.add_space(3.0);
                                    ui.horizontal(|ui| {
                                        if let Some(ref installer_url) = info.installer_url {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new(format_persian_display("دانلود فایل نصب"))
                                                            .size(10.5)
                                                            .strong()
                                                            .color(palette::WHITE),
                                                    )
                                                    .fill(palette::ACCENT_ACTION)
                                                    .rounding(egui::Rounding::same(5.0)),
                                                )
                                                .clicked()
                                            {
                                                crate::updates::open_url_in_browser(installer_url);
                                            }
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new(format_persian_display("مشاهده در GitHub"))
                                                        .size(10.5)
                                                        .color(palette::TEXT_SECONDARY),
                                                )
                                                .fill(palette::CHIP_BG)
                                                .rounding(egui::Rounding::same(5.0)),
                                            )
                                            .clicked()
                                        {
                                            crate::updates::open_url_in_browser(&info.release_url);
                                        }
                                    });
                                }
                                crate::updates::UpdateState::UpToDate { ref checked_at } => {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display(&format!("نسخه شما به‌روز است ({})", checked_at)),
                                            ic::CHECK_CIRCLE
                                        ))
                                        .size(10.5)
                                        .color(palette::SUCCESS),
                                    );
                                }
                                crate::updates::UpdateState::Error(ref err) => {
                                    ui.label(
                                        egui::RichText::new(format!("{}  {}", format_persian_display(&format!("خطا: {err}")), ic::WARNING))
                                            .size(10.0)
                                            .color(palette::WARNING),
                                    );
                                }
                                crate::updates::UpdateState::Idle => {
                                    ui.label(
                                        egui::RichText::new(format_persian_display(
                                            "هنوز بررسی انجام نشده است",
                                        ))
                                        .size(10.5)
                                        .color(palette::TEXT_MUTED),
                                    );
                                }
                            }

                            ui.add_space(4.0);
                            if ui
                                .button(
                                    egui::RichText::new(format!(
                                        "{}  {}",
                                        format_persian_display("بررسی به‌روزرسانی اکنون"),
                                        ic::ARROWS_CLOCKWISE
                                    ))
                                    .size(10.5),
                                )
                                .clicked()
                            {
                                let st = update_state.clone();
                                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                                    handle.spawn(async move {
                                        crate::updates::perform_check(&st, env!("CARGO_PKG_VERSION")).await;
                                    });
                                } else {
                                    tracing::error!("no tokio runtime for manual update check");
                                }
                            }
                            ui.add_space(2.0);
                            ui.checkbox(
                                &mut state.draft.updates.check_on_startup,
                                format_persian_display("بررسی خودکار در شروع برنامه"),
                            );
                        });
                    },
                );
            });

            // ── Left Column: Audio, VAD, Save Hub (cols[0]) ──
            cols[0].push_id("settings_col_left", |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                    |ui| {
                        // ── Card 4: Audio (صوت) ──
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}",
                                    format_persian_display("صوت"),
                                    ic::MICROPHONE
                                ))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_SECTION),
                            );
                            ui.add_space(6.0);

                            let audio_label_w = 105.0;
                            rtl_form_row(ui, "دستگاه ورودی:", audio_label_w, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut state.draft.audio.device)
                                        .hint_text("default")
                                        .desired_width(140.0),
                                );
                            });
                            ui.add_space(6.0);

                            rtl_form_row(ui, "نرخ نمونه‌برداری:", audio_label_w, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut state.draft.audio.sample_rate)
                                        .range(8_000..=96_000)
                                        .suffix(" Hz"),
                                );
                            });
                            ui.add_space(6.0);

                            rtl_form_row(ui, "تقویت صدا:", audio_label_w, |ui| {
                                ui.add(
                                    egui::Slider::new(
                                        &mut state.draft.audio.gain_db,
                                        -12.0..=24.0,
                                    )
                                    .suffix(" dB")
                                    .fixed_decimals(1),
                                );
                            });
                            ui.add_space(6.0);

                            rtl_form_row(ui, "حافظه حلقه:", audio_label_w, |ui| {
                                ui.add(
                                    egui::Slider::new(
                                        &mut state.draft.audio.ring_seconds,
                                        5..=120,
                                    )
                                    .suffix(" s"),
                                );
                            });

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(6.0);

                            // ── Orb return (بازگشت اورب) ──
                            // Three controls and no more. Each one exists
                            // because the roadmap asks for it; anything beyond
                            // these (a corner picker, a monitor picker) would
                            // be a second place to configure the same policy,
                            // and two places to configure one thing means one
                            // of them is wrong.
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}",
                                    format_persian_display("بازگشت اورب به گوشه"),
                                    ic::ARROW_BEND_DOWN_RIGHT
                                ))
                                .size(12.0)
                                .strong()
                                .color(palette::TEXT_SECTION),
                            );
                            ui.add_space(6.0);

                            match orb_controls {
                                // No controls (a test without a real orb): say
                                // so rather than drawing rows that do nothing.
                                None => {
                                    ui.label(
                                        egui::RichText::new(format_persian_display(
                                            "کنترل اورب در این نشست فعال نیست.",
                                        ))
                                        .size(10.5)
                                        .color(palette::TEXT_FAINT),
                                    );
                                }
                                Some(controls) => {
                                    let orb_label_w = 105.0;
                                    rtl_form_row(
                                        ui,
                                        "بازگشت خودکار:",
                                        orb_label_w,
                                        |ui| {
                                            ui.checkbox(
                                                &mut controls.return_enabled,
                                                format_persian_display("پس از بی‌کاری به گوشه برگردد"),
                                            );
                                        },
                                    );
                                    ui.add_space(4.0);
                                    rtl_form_row(ui, "زمان بی‌کاری:", orb_label_w, |ui| {
                                        ui.add(
                                            egui::Slider::new(
                                                &mut controls.return_after_idle_secs,
                                                10..=600,
                                            )
                                            .suffix(" s")
                                            .fixed_decimals(0),
                                        )
                                        .on_hover_text(
                                            "how long the orb waits before going back to its corner",
                                        );
                                    });
                                    ui.add_space(4.0);
                                    rtl_form_row(ui, "ثابت کردن:", orb_label_w, |ui| {
                                        ui.checkbox(
                                            &mut controls.pinned,
                                            format_persian_display(
                                                "اورب همان‌جا که کاربر گذاشت بماند",
                                            ),
                                        );
                                    });
                                    ui.add_space(4.0);
                                    // The corner used to be fixed to top-right, so
                                    // the orb always walked *up* no matter where
                                    // the user had put it. On a machine whose
                                    // taskbar is at the bottom, bottom-right is the
                                    // corner that is actually out of the way.
                                    rtl_form_row(ui, "گوشهٔ بازگشت:", orb_label_w, |ui| {
                                        let labels = [
                                            ("top_right", "بالا-راست"),
                                            ("top_left", "بالا-چپ"),
                                            ("bottom_right", "پایین-راست"),
                                            ("bottom_left", "پایین-چپ"),
                                        ];
                                        let selected = labels
                                            .iter()
                            .find(|(value, _)| *value == controls.return_corner)
                            .map(|(_, label)| *label)
                            .unwrap_or(labels[0].1);
                        egui::ComboBox::from_id_source("orb_return_corner")
                            .selected_text(format_persian_display(selected))
                            .show_ui(ui, |ui| {
                                for (value, label) in labels {
                                    if ui
                                        .selectable_label(
                                            controls.return_corner == value,
                                            format_persian_display(label),
                                        )
                                        .clicked()
                                    {
                                        controls.return_corner = value.to_string();
                                    }
                                }
                            });
                    });
                    ui.add_space(4.0);
                    // One number for the whole orb: drawing, click target and
                    // window margin together, because a drawing whose click
                    // target stayed the old size is an orb you cannot reliably
                    // click.
                    rtl_form_row(ui, "اندازهٔ اورب:", orb_label_w, |ui| {
                        ui.add(
                            egui::Slider::new(&mut controls.scale_percent, 60..=200)
                                .suffix(" %")
                                .fixed_decimals(0),
                        )
                        .on_hover_text("how large the orb is drawn and how large a click it answers");
                    });
                    ui.add_space(6.0);
                                    if ui
                                        .button(egui::RichText::new(format_persian_display(
                                            "بازگرداندن به محل قبلی",
                                        )))
                                        .clicked()
                                    {
                                        controls.return_to_manual = true;
                                    }
                                }
                            }
                        });

                        ui.add_space(8.0);

                        // ── Card 5: VAD (تشخیص سکوت) ──
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}",
                                    format_persian_display("تشخیص سکوت (VAD)"),
                                    ic::WAVE_SINE
                                ))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_SECTION),
                            );
                            ui.add_space(6.0);

                            let vad_label_w = 120.0;
                            rtl_form_row(ui, "آستانه سکوت:", vad_label_w, |ui| {
                                ui.add(
                                    egui::Slider::new(
                                        &mut state.draft.vad.threshold,
                                        0.05..=0.95,
                                    )
                                    .fixed_decimals(2),
                                );
                            });
                            ui.add_space(6.0);

                            rtl_form_row(ui, "مدت سکوت برای توقف:", vad_label_w, |ui| {
                                ui.add(
                                    egui::DragValue::new(
                                        &mut state.draft.vad.silence_timeout_ms,
                                    )
                                    .range(200..=10_000)
                                    .suffix(" ms"),
                                );
                            });
                            ui.add_space(6.0);

                            rtl_form_row(ui, "حداقل مدت گفتار:", vad_label_w, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut state.draft.vad.min_speech_ms)
                                        .range(50..=2_000)
                                        .suffix(" ms"),
                                );
                            });

                            ui.add_space(6.0);
                            if ui
                                .checkbox(
                                    &mut state.draft.vad.cutoff_on_hold,
                                    format_persian_display("توقف ضبط با سکوت، حتی با نگه‌داشتن کلید"),
                                )
                                .changed()
                            {
                                state.error = None;
                            }
                        });

                        ui.add_space(8.0);

                        // ── Card 6: Save & Action Hub (ذخیره و مدیریت تنظیمات) ──
                        manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}",
                                    format_persian_display("ذخیره و اعمال تنظیمات"),
                                    ic::FLOPPY_DISK
                                ))
                                .size(12.5)
                                .strong()
                                .color(palette::TEXT_SECTION),
                            );
                            ui.add_space(6.0);

                            if let Some(ref err) = state.error {
                                callout(ui, CalloutKind::Warning, err);
                                ui.add_space(6.0);
                            }

                            ui.horizontal(|ui| {
                                let can_save = state.error.is_none();
                                let save = ui.add_enabled(
                                    can_save,
                                    egui::Button::new(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display("ذخیره تنظیمات"),
                                            ic::FLOPPY_DISK
                                        ))
                                        .size(11.5)
                                        .strong()
                                        .color(palette::WHITE),
                                    )
                                    .fill(if can_save {
                                        palette::ACCENT_ACTION
                                    } else {
                                        palette::CHIP_BG
                                    })
                                    .rounding(egui::Rounding::same(6.0)),
                                );
                                if save.clicked() {
                                    if let Ok(mut s) = settings.write() {
                                        *s = state.draft.clone();
                                        let _ = s.save(config_path);
                                    }
                                    // Push the (possibly hand-edited) bindings to
                                    // the running listener too.
                                    state.apply_hotkeys_live(hotkey, settings, config_path);
                                }

                                if ui
                                    .button(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display("بارگذاری مجدد"),
                                            ic::ARROWS_CLOCKWISE
                                        ))
                                        .size(11.0),
                                    )
                                    .clicked()
                                {
                                    if let Ok(loaded) = Settings::load_or_create(config_path) {
                                        if let Ok(mut s) = settings.write() {
                                            *s = loaded.clone();
                                        }
                                        state.draft = loaded;
                                        state.error = None;
                                        state.saved = false;
                                    }
                                }
                            });

                            if state.saved {
                                ui.add_space(6.0);
                                status_chip(
                                    ui,
                                    "تنظیمات با موفقیت ذخیره شد",
                                    palette::SUCCESS_FILL,
                                    palette::SUCCESS,
                                    10.5,
                                    ChipFamily::Tiny,
                                );
                            }
                        });
                    },
                );
            });
        });
}
