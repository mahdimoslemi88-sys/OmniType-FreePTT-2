//! Engines tab: register and switch between ASR providers.
//!
//! Owns [`EnginePanelState`] — the "add a provider" form — and reads the
//! shared [`AsrRouter`]. This is the only panel that writes `config.toml`,
//! which is why its `render` signature is wider than the others; see the note
//! on [`render`].

use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_phosphor::regular as ic;

use super::text::format_persian_display;
use super::theme::*;
use super::DashboardTab;
use crate::asr::engine::AsrHealth;
use crate::asr::router::AsrRouter;
use crate::config::settings::{CustomProvider, Settings};

/// The Engines tab's private UI state: the add-a-provider form and the
/// transient feedback line. The provider *registry* is not here — it lives in
/// the shared [`AsrRouter`], which the state machine and the tray also read.
pub(crate) struct EnginePanelState {
    /// The provider currently being added; `id` is empty until it is saved.
    pub new_id: String,
    pub new_name: String,
    /// Base URL of the OpenAI-compatible endpoint.
    pub new_url: String,
    /// API key. Never displayed in full — see `mask_secret`.
    pub new_key: String,
    /// Model name to request, e.g. `whisper-1`.
    pub new_model: String,
    /// BCP-47 language tag sent with each request.
    pub new_lang: String,
    /// Transient "provider saved / rejected" feedback, auto-expires.
    pub msg: Option<(String, Instant)>,
}

/// Hand-written rather than derived: the form opens with a real model name and
/// language, and `Default::default()` would have silently turned those into
/// empty strings the first time this state was constructed.
impl Default for EnginePanelState {
    fn default() -> Self {
        Self {
            new_id: String::new(),
            new_name: String::new(),
            new_url: String::new(),
            new_key: String::new(),
            new_model: "whisper-large-v3-turbo".to_string(),
            new_lang: "fa".to_string(),
            msg: None,
        }
    }
}

/// The slice of Settings-tab state the Engines tab is allowed to touch.
///
/// Measured, not assumed: this is the one tab that reaches outside itself.
/// Registering a provider writes `config.toml`, and afterwards it refreshes
/// the Settings tab's draft, clears that draft's error, and switches the
/// dashboard over to it.
///
/// Naming the exception as a type — rather than passing three loose `&mut`s —
/// keeps `render` under the argument-count lint and turns a private deal
/// between two tabs into something a reader can grep for. A future change
/// that needs a *fourth* Settings field has to extend this struct, which is
/// where the review happens.
pub(crate) struct SettingsHandoff<'a> {
    /// The unvalidated settings draft the Settings tab renders.
    pub draft: &'a mut Settings,
    /// That draft's validation error, cleared once the engine tab has saved.
    pub error: &'a mut Option<String>,
    /// Which tab the dashboard shows next.
    pub tab: &'a mut DashboardTab,
}

/// Renders the Engines tab body into the dashboard's current panel.
pub(crate) fn render(
    ui: &mut egui::Ui,
    state: &mut EnginePanelState,
    router: &AsrRouter,
    settings: &Arc<RwLock<Settings>>,
    config_path: &Path,
    handoff: &mut SettingsHandoff<'_>,
) {
    // ── Header & Title ──
    let active = router.active_engine();
    let badge_text = if active == "auto" {
        "حالت فعال: خودکار (Auto)".to_string()
    } else {
        format!("فعال: {active}")
    };
    manager_header(
        ui,
        "مدیریت مدل‌های صوتی و API",
        Some((&badge_text, palette::ENGINE_PILL_BG, palette::ACCENT)),
    );

    manager_subtitle(
        ui,
        "انتخاب موتور پیش‌فرض یا افزودن سرور و مدل‌های اختصاصی (سازگار با OpenAI)",
    );

    // Info callout about engine routing
    callout(
                            ui,
                            CalloutKind::Info,
                            "در حالت «خودکار»، اولین موتور آماده انتخاب می‌شود و در صورت خطا، موتور بعدی جایگزین می‌گردد.",
                        );

    ui.add_space(6.0);

    // Feedback message
    if let Some((ref msg, timestamp)) = state.msg {
        if timestamp.elapsed() < Duration::from_secs(4) {
            success_banner(ui, msg);
        }
    }

    ui.add_space(10.0);

    // ── Available Engines List ──
    ui.label(
        egui::RichText::new(format_persian_display("موتورهای گفتار به متن ثبت‌شده:"))
            .size(12.5)
            .strong()
            .color(palette::TEXT_STRONG_SOFT),
    );
    ui.add_space(4.0);

    let engines = router.list_engines();
    let current_active = router.active_engine();

    egui::ScrollArea::vertical()
        .id_source("engines_list_scroll")
        .max_height(240.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Auto Fallback Option
            let is_auto = current_active == "auto";
            manager_card(
                if is_auto {
                    palette::AUTO_BG
                } else {
                    palette::CARD_BG
                },
                if is_auto {
                    palette::SELECT_STROKE
                } else {
                    palette::CARD_STROKE
                },
            )
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                // `ui.horizontal` inside `top_down(Align::RIGHT)` is already
                // `right_to_left`. Place right-side items first so the
                // `left_to_right` status chip at the end only claims the
                // remaining left space instead of collapsing `cursor.max.x`.
                ui.horizontal(|ui| {
                    if ui.radio(is_auto, "").clicked() && !is_auto {
                        router.set_active_engine("auto");
                        if let Ok(mut s) = settings.write() {
                            s.active_engine = "auto".to_string();
                            let _ = s.save(config_path);
                        }
                        state.msg = Some((
                            format_persian_display("حالت خودکار هوشمند (Auto) فعال شد."),
                            Instant::now(),
                        ));
                    }

                    ui.add_space(4.0);

                    ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                        ui.label(
                            egui::RichText::new(format_persian_display(
                                "حالت خودکار هوشمند (Auto Fallback)",
                            ))
                            .strong()
                            .size(12.0)
                            .color(palette::TEXT_PRIMARY),
                        );
                        ui.label(
                            egui::RichText::new(format_persian_display(
                                "اولویت‌بندی خودکار بین ابری و محلی؛ سوییچ بدون وقفه در قطعی شبکه",
                            ))
                            .size(10.0)
                            .color(palette::TEXT_MUTED),
                        );
                    });

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if is_auto {
                            status_chip(
                                ui,
                                "انتخاب‌شده",
                                palette::SUCCESS_PILL,
                                palette::SUCCESS_OK,
                                9.5,
                                ChipFamily::Tiny,
                            );
                        }
                    });
                });
            });

            ui.add_space(5.0);

            let mut to_delete: Option<String> = None;
            for (id, display_name, kind, health, _is_selected) in &engines {
                let is_active = current_active == *id;
                manager_card(
                    if is_active {
                        palette::SELECTED_BG
                    } else {
                        palette::CARD_BG
                    },
                    if is_active {
                        palette::SELECT_STROKE
                    } else {
                        palette::CARD_STROKE
                    },
                )
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.push_id(id, |ui| {
                        ui.horizontal(|ui| {
                            // Right side placed first in the RTL horizontal row
                            if ui.radio(is_active, "").clicked() && !is_active {
                                router.set_active_engine(id);
                                if let Ok(mut s) = settings.write() {
                                    s.active_engine = id.clone();
                                    let _ = s.save(config_path);
                                }
                                state.msg = Some((
                                    format_persian_display(&format!(
                                        "موتور {} فعال شد.",
                                        display_name
                                    )),
                                    Instant::now(),
                                ));
                            }

                            // Health indicator dot
                            let (dot_color, health_desc) = match health {
                                AsrHealth::Ready => (palette::SUCCESS_DOT, "آماده کار".to_string()),
                                AsrHealth::NoModel => {
                                    (palette::DOT_INACTIVE, "مدل دانلود نشده".to_string())
                                }
                                AsrHealth::Cooldown { reason, .. } => {
                                    (palette::WARNING, format!("در حال بازیابی: {reason}"))
                                }
                                AsrHealth::Failed { reason } => {
                                    (palette::DANGER_TEXT, format!("غیرفعال: {reason}"))
                                }
                            };

                            let (resp, painter) =
                                ui.allocate_painter(egui::vec2(10.0, 10.0), egui::Sense::hover());
                            painter.circle_filled(resp.rect.center(), 3.5, dot_color);
                            resp.on_hover_text(format_persian_display(&health_desc));

                            ui.add_space(4.0);

                            ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(display_name)
                                            .strong()
                                            .size(12.0)
                                            .color(palette::TEXT_PRIMARY),
                                    );

                                    status_chip(
                                        ui,
                                        kind,
                                        palette::CHIP_BG,
                                        palette::ACCENT_BADGE,
                                        9.0,
                                        ChipFamily::Small,
                                    );
                                });

                                ui.label(
                                    egui::RichText::new(format!("ID: {id}"))
                                        .size(9.5)
                                        .color(palette::TEXT_FAINT),
                                );
                            });

                            // Left side: Delete button (if custom) and Active status chip
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    if *kind == "Cloud (Custom)" {
                                        let del_btn = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(ic::TRASH)
                                                    .size(11.0)
                                                    .color(palette::DANGER),
                                            )
                                            .fill(palette::DANGER_FILL)
                                            .rounding(egui::Rounding::same(4.0)),
                                        );
                                        if del_btn.clicked() {
                                            to_delete = Some(id.clone());
                                        }
                                    }

                                    if is_active {
                                        status_chip(
                                            ui,
                                            "فعال",
                                            palette::SUCCESS_PILL,
                                            palette::SUCCESS_OK,
                                            9.5,
                                            ChipFamily::Tiny,
                                        );
                                    }
                                },
                            );
                        });
                    });
                });
                ui.add_space(4.0);
            }

            if let Some(del_id) = to_delete {
                router.remove_engine(&del_id);
                if let Ok(mut s) = settings.write() {
                    s.remove_provider(&del_id);
                    if s.active_engine == del_id {
                        s.active_engine = "auto".to_string();
                        router.set_active_engine("auto");
                    }
                    let _ = s.save(config_path);
                }
                state.msg = Some((
                    format_persian_display("مدل اختصاصی حذف گردید."),
                    Instant::now(),
                ));
            }
        });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);

    // ── Add New API Provider Form ──
    manager_card(palette::CARD_BG_ALT, palette::STROKE)
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display("افزودن API / سرور دلخواه (سازگار با OpenAI):"),
                                            ic::KEY
                                        ))
                                        .size(12.5)
                                        .strong()
                                        .color(palette::TEXT_SECTION),
                                    );
                                });
                                ui.add_space(3.0);
                                ui.label(
                                    egui::RichText::new(format_persian_display("اتصال به سرورهای محلی، Ollama، vLLM یا ارائه‌دهندگان ابری (OpenAI، Groq، Together و ...)"))
                                        .size(10.0)
                                        .color(palette::TEXT_MUTED),
                                );
                                ui.add_space(8.0);

                                // 2-column RTL form layout instead of `egui::Grid` so all 6 fields
                                // sit balanced across the card and never trigger `Grid`'s `-INFINITY`
                                // row-0 cursor bug inside RTL layouts.
                                ui.columns(2, |cols| {
                                    cols[1].push_id("api_form_right_col", |ui| {
                                        ui.with_layout(
                                            egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                                            |ui| {
                                                let field_w = (ui.available_width() - 108.0).max(120.0);
                                                rtl_form_row(ui, "شناسه یکتا (ID):", 98.0, |ui| {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut state.new_id)
                                                            .hint_text("e.g. custom_openai")
                                                            .desired_width(field_w),
                                                    );
                                                });
                                                ui.add_space(5.0);
                                                rtl_form_row(ui, "نام نمایشی:", 98.0, |ui| {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut state.new_name)
                                                            .hint_text("e.g. OpenAI Whisper Large")
                                                            .desired_width(field_w),
                                                    );
                                                });
                                                ui.add_space(5.0);
                                                rtl_form_row(ui, "آدرس Base URL:", 98.0, |ui| {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut state.new_url)
                                                            .hint_text("e.g. https://api.openai.com/v1")
                                                            .desired_width(field_w),
                                                    );
                                                });
                                            },
                                        );
                                    });

                                    cols[0].push_id("api_form_left_col", |ui| {
                                        ui.with_layout(
                                            egui::Layout::top_down(egui::Align::RIGHT).with_cross_justify(true),
                                            |ui| {
                                                let field_w = (ui.available_width() - 108.0).max(120.0);
                                                rtl_form_row(ui, "کلید API:", 98.0, |ui| {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut state.new_key)
                                                            .password(true)
                                                            .hint_text("sk-...")
                                                            .desired_width((field_w - 85.0).max(90.0)),
                                                    );
                                                    let key_status = if state.new_key.trim().is_empty() {
                                                        format_persian_display("وارد نشده")
                                                    } else {
                                                        format_persian_display("ماسک‌شده")
                                                    };
                                                    ui.label(
                                                        egui::RichText::new(key_status)
                                                            .size(9.5)
                                                            .color(palette::TEXT_MUTED),
                                                    );
                                                });
                                                ui.add_space(5.0);
                                                rtl_form_row(ui, "نام مدل:", 98.0, |ui| {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut state.new_model)
                                                            .hint_text("whisper-1 / whisper-large-v3-turbo")
                                                            .desired_width(field_w),
                                                    );
                                                });
                                                ui.add_space(5.0);
                                                rtl_form_row(ui, "زبان (Language):", 98.0, |ui| {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut state.new_lang)
                                                            .hint_text("fa")
                                                            .desired_width(field_w),
                                                    );
                                                });
                                            },
                                        );
                                    });
                                });

                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    let add_btn = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(format!(
                                                "{}  {}",
                                                format_persian_display("ثبت و فعال‌سازی این موتور"),
                                                ic::PLUS
                                            ))
                                            .strong()
                                            .size(11.5)
                                            .color(palette::WHITE),
                                        )
                                        .fill(palette::ACCENT_ACTION)
                                        .rounding(egui::Rounding::same(6.0))
                                        .min_size(egui::vec2(160.0, 26.0)),
                                    );

                                    if add_btn.clicked() {
                                        let id = state.new_id.trim().to_lowercase();
                                        let url = state.new_url.trim().to_string();
                                        if id.is_empty() || url.is_empty() {
                                            state.msg = Some((
                                                format_persian_display("خطا: شناسه (ID) و آدرس URL الزامی هستند."),
                                                Instant::now(),
                                            ));
                                        } else {
                                            let name = if state.new_name.trim().is_empty() {
                                                id.clone()
                                            } else {
                                                state.new_name.trim().to_string()
                                            };
                                            let model = if state.new_model.trim().is_empty() {
                                                "whisper-large-v3-turbo".to_string()
                                            } else {
                                                state.new_model.trim().to_string()
                                            };
                                            let lang = if state.new_lang.trim().is_empty() {
                                                "fa".to_string()
                                            } else {
                                                state.new_lang.trim().to_string()
                                            };
                                            let provider = CustomProvider {
                                                id: id.clone(),
                                                name,
                                                base_url: url,
                                                api_key: state.new_key.trim().to_string(),
                                                model,
                                                language: lang,
                                                timeout_secs: 20,
                                            };

                                            let usage_path = crate::paths::resolve_usage_path();
                                            let engine = Arc::new(crate::asr::CloudEngine::new_custom(&provider, usage_path));
                                            router.register_engine(engine);
                                            router.set_active_engine(&id);

                                            if let Ok(mut s) = settings.write() {
                                                s.active_engine = id.clone();
                                                s.add_or_update_provider(provider);
                                                let _ = s.save(config_path);
                                            }

                                            state.msg = Some((
                                                format_persian_display("مدل جدید ثبت و به عنوان موتور فعال انتخاب شد."),
                                                Instant::now(),
                                            ));
                                            state.new_id.clear();
                                            state.new_name.clear();
                                            state.new_url.clear();
                                            state.new_key.clear();
                                        }
                                    }

                                    ui.add_space(8.0);
                                    if ui.button(egui::RichText::new(format!("{}  {}", format_persian_display("تنظیمات"), ic::GEAR)).size(11.0)).clicked() {
                                        if let Ok(s) = settings.read() {
                                            *handoff.draft = s.clone();
                                        }
                                        *handoff.error = None;
                                        *handoff.tab = DashboardTab::Settings;
                                    }
                                });
                            });
}
