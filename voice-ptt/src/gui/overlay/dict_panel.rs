//! Dictionary tab: the user's technical-jargon find/replace rules.
//!
//! Owns [`DictPanelState`] and nothing else. The rule *texts* live in the
//! shared [`Dictionary`] because the tray menu adds entries without opening
//! this window; what this module owns is the form and the inline editor.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_phosphor::regular as ic;

use super::text::{format_persian_display, persian_text_edit_layouter};
use super::theme::*;
use crate::processing::Dictionary;

/// The Dictionary tab's private UI state.
///
/// Grouped so the tab owns a single field on [`OverlayApp`] instead of nine,
/// and so a reviewer can see at a glance that nothing here overlaps another
/// tab's state. The rule texts themselves are *not* here — they live in the
/// shared [`Dictionary`], because the tray menu mutates those too.
#[derive(Default)]
pub(crate) struct DictPanelState {
    /// The "from" side of the new-rule form.
    pub new_from: String,
    /// The "to" side of the new-rule form.
    pub new_to: String,
    /// Category of the new rule, used to group it in the list.
    pub new_cat: String,
    /// Live filter over the rule list.
    pub search_query: String,
    /// Transient "rule saved / rejected" feedback, auto-expires.
    pub msg: Option<(String, Instant)>,
    /// Index of the rule being edited inline; `None` closes the editor.
    pub edit_index: Option<usize>,
    /// Inline editor buffers, kept separate from the live rule so an
    /// in-progress edit never mutates the dictionary.
    pub edit_from: String,
    pub edit_to: String,
    pub edit_cat: String,
}

/// Renders the Dictionary tab body into the dashboard's current panel.
///
/// `dictionary` is the shared store (other panels and the tray can edit it);
/// everything else is this tab's private UI state.
pub(crate) fn render(
    ui: &mut egui::Ui,
    state: &mut DictPanelState,
    dictionary: &Arc<RwLock<Dictionary>>,
) {
    // egui has no Arabic shaping, so every TextEdit here
    // lays its text out through the Persian reshaper via a
    // shared closure, keeping letters connected.
    let mut persian_layouter =
        |ui: &egui::Ui, text: &str, w: f32| persian_text_edit_layouter(ui, text, w);

    // ── Header & Title ──
    let total_rules = dictionary.read().map(|d| d.len()).unwrap_or(0);
    manager_header(
        ui,
        "مدیریت دیکشنری کلمات تخصصی",
        Some((
            &format!("{total_rules} قانون فعال"),
            palette::HEADER_PILL_BG,
            palette::ACCENT_SOFT,
        )),
    );

    manager_subtitle(ui, "تعریف و تصحیح خودکار واژگان فنی، مهندسی و گفتاری");

    // Info callout explaining dictionary behavior
    callout(
        ui,
        CalloutKind::Info,
        "قوانین دیکشنری قبل از تایپ نهایی روی متن خروجی اعمال می‌شوند.",
    );

    ui.add_space(6.0);

    // Feedback message if active
    if let Some((ref msg, timestamp)) = state.msg {
        if timestamp.elapsed() < Duration::from_secs(4) {
            success_banner(ui, msg);
        }
    }

    ui.add_space(10.0);

    // ── Card 1: Add New Word ──
    manager_card(palette::CARD_BG_ALT, palette::STROKE).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            egui::RichText::new(format!(
                "{}  {}",
                format_persian_display("افزودن یا ویرایش کلمه جدید:"),
                ic::PLUS
            ))
            .size(12.0)
            .strong()
            .color(palette::TEXT_SECTION),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format_persian_display("کلمه شنیده شده (از):"))
                    .size(11.0)
                    .color(palette::TEXT_LABEL),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.new_from)
                    .hint_text(format_persian_display("پاتون / سی ان سی"))
                    .desired_width(140.0)
                    .layouter(&mut persian_layouter),
            );

            ui.label(
                egui::RichText::new(format_persian_display("معادل صحیح (به):"))
                    .size(11.0)
                    .color(palette::TEXT_LABEL),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.new_to)
                    .hint_text(format_persian_display("پایتون / CNC"))
                    .desired_width(140.0)
                    .layouter(&mut persian_layouter),
            );
        });

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format_persian_display("دسته‌بندی (اختیاری):"))
                    .size(11.0)
                    .color(palette::TEXT_LABEL),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.new_cat)
                    .hint_text(format_persian_display("برنامه‌نویسی / مکانیک"))
                    .desired_width(130.0)
                    .layouter(&mut persian_layouter),
            );

            ui.add_space(8.0);
            let add_btn = ui.button(
                egui::RichText::new(format_persian_display("+ ثبت در دیکشنری"))
                    .size(11.5)
                    .color(egui::Color32::WHITE),
            );
            if add_btn.clicked() {
                let from = state.new_from.trim().to_string();
                let to = state.new_to.trim().to_string();
                let cat = if state.new_cat.trim().is_empty() {
                    None
                } else {
                    Some(state.new_cat.trim().to_string())
                };

                if !from.is_empty() && !to.is_empty() && from != to {
                    if let Ok(mut dict) = dictionary.write() {
                        dict.add_rule(from, to, cat);
                        let _ = dict.save_to_file();
                        state.new_from.clear();
                        state.new_to.clear();
                        state.new_cat.clear();
                        // Raw, not shaped: `success_banner` is the shaping
                        // boundary (it formats its own text). A pre-formatted
                        // string here was shaped a second time and drawn
                        // backwards.
                        state.msg = Some((
                            "قاعده با موفقیت ثبت و ذخیره شد!".to_string(),
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
            egui::RichText::new(format!(
                "{}  {}",
                format_persian_display("جستجو:"),
                ic::MAGNIFYING_GLASS
            ))
            .size(11.5)
            .color(palette::TEXT_LABEL),
        );
        ui.add(
            egui::TextEdit::singleline(&mut state.search_query)
                .hint_text(format_persian_display("جستجو در بین کلمات..."))
                .desired_width((ui.available_width() - 10.0).max(120.0))
                .layouter(&mut persian_layouter),
        );
    });

    ui.add_space(6.0);

    // ── Scrollable RTL Table of Rules ──
    // Uses explicit RTL horizontal rows instead of `egui::Grid` because
    // `Grid` in egui 0.28 sets `cursor.min.x = -INFINITY` on row 0 inside
    // RTL parent layouts and forces LTR left-alignment.
    let mut rule_to_remove: Option<String> = None;
    let mut rule_to_edit: Option<usize> = None;
    let query = state.search_query.trim().to_lowercase();

    if let Ok(dict) = dictionary.read() {
        let rules = dict.rules();
        // Header band (Right-to-Left: از → ← → به → دسته → عملیات)
        egui::Frame::none()
            .fill(palette::TABLE_HEADER_BG)
            .rounding(egui::Rounding::same(5.0))
            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    rtl_table_cell(
                        ui,
                        150.0,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display("کلمه گفتاری (از)"))
                                    .strong()
                                    .size(10.5)
                                    .color(palette::TEXT_LABEL),
                            );
                        },
                    );
                    rtl_table_cell(
                        ui,
                        28.0,
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.label(
                                egui::RichText::new("←")
                                    .size(10.5)
                                    .color(palette::TEXT_FAINT),
                            );
                        },
                    );
                    rtl_table_cell(
                        ui,
                        150.0,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display("معادل صحیح (به)"))
                                    .strong()
                                    .size(10.5)
                                    .color(palette::TEXT_LABEL),
                            );
                        },
                    );
                    rtl_table_cell(
                        ui,
                        110.0,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(
                                egui::RichText::new(format_persian_display("دسته"))
                                    .strong()
                                    .size(10.5)
                                    .color(palette::TEXT_LABEL),
                            );
                        },
                    );
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format_persian_display("عملیات"))
                                .strong()
                                .size(10.5)
                                .color(palette::TEXT_LABEL),
                        );
                    });
                });
            });

        ui.add_space(3.0);

        egui::ScrollArea::vertical()
            .id_source("dict_rules_scroll")
            .max_height(210.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (idx, r) in rules.iter().enumerate() {
                    if !query.is_empty() {
                        let matches_from = r.from.to_lowercase().contains(&query);
                        let matches_to = r.to.to_lowercase().contains(&query);
                        let matches_cat = r
                            .category
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query);
                        if !matches_from && !matches_to && !matches_cat {
                            continue;
                        }
                    }

                    let row_bg = if idx % 2 == 1 {
                        palette::TABLE_ROW_ALT
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    egui::Frame::none()
                        .fill(row_bg)
                        .rounding(egui::Rounding::same(4.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.push_id(idx, |ui| {
                                ui.horizontal(|ui| {
                                    if Some(idx) == state.edit_index {
                                        rtl_table_cell(
                                            ui,
                                            150.0,
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut state.edit_from,
                                                    )
                                                    .desired_width(140.0)
                                                    .layouter(&mut persian_layouter),
                                                );
                                            },
                                        );
                                        rtl_table_cell(
                                            ui,
                                            28.0,
                                            egui::Layout::centered_and_justified(
                                                egui::Direction::LeftToRight,
                                            ),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new("←")
                                                        .size(11.0)
                                                        .color(palette::TEXT_FAINT),
                                                );
                                            },
                                        );
                                        rtl_table_cell(
                                            ui,
                                            150.0,
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut state.edit_to)
                                                        .desired_width(140.0)
                                                        .layouter(&mut persian_layouter),
                                                );
                                            },
                                        );
                                        rtl_table_cell(
                                            ui,
                                            110.0,
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut state.edit_cat)
                                                        .desired_width(100.0)
                                                        .layouter(&mut persian_layouter),
                                                );
                                            },
                                        );
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button(egui::RichText::new(ic::X).size(10.5))
                                                    .clicked()
                                                {
                                                    state.edit_index = None;
                                                }
                                                if ui
                                                    .button(
                                                        egui::RichText::new(ic::CHECK).size(10.5),
                                                    )
                                                    .clicked()
                                                {
                                                    let from = state.edit_from.trim().to_string();
                                                    let to = state.edit_to.trim().to_string();
                                                    let cat = if state.edit_cat.trim().is_empty() {
                                                        None
                                                    } else {
                                                        Some(state.edit_cat.trim().to_string())
                                                    };
                                                    if !from.is_empty()
                                                        && !to.is_empty()
                                                        && from != to
                                                    {
                                                        if let Ok(mut dict) = dictionary.write() {
                                                            dict.remove_rule(idx);
                                                            dict.add_rule(from, to, cat);
                                                            let _ = dict.save_to_file();
                                                            state.msg = Some((
                                                                "قاعده به‌روزرسانی شد".to_string(),
                                                                Instant::now(),
                                                            ));
                                                        }
                                                        state.edit_index = None;
                                                    }
                                                }
                                            },
                                        );
                                    } else {
                                        rtl_table_cell(
                                            ui,
                                            150.0,
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(
                                                        &r.from,
                                                    ))
                                                    .size(11.0)
                                                    .color(palette::TEXT_TABLE),
                                                );
                                            },
                                        );
                                        rtl_table_cell(
                                            ui,
                                            28.0,
                                            egui::Layout::centered_and_justified(
                                                egui::Direction::LeftToRight,
                                            ),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new("←")
                                                        .size(11.0)
                                                        .color(palette::TEXT_FAINT),
                                                );
                                            },
                                        );
                                        rtl_table_cell(
                                            ui,
                                            150.0,
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(
                                                        &r.to,
                                                    ))
                                                    .size(11.0)
                                                    .strong()
                                                    .color(palette::ACCENT),
                                                );
                                            },
                                        );
                                        let cat_str = r.category.as_deref().unwrap_or("-");
                                        rtl_table_cell(
                                            ui,
                                            110.0,
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format_persian_display(
                                                        cat_str,
                                                    ))
                                                    .size(9.5)
                                                    .color(palette::TEXT_MUTED),
                                                );
                                            },
                                        );
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button(
                                                        egui::RichText::new(ic::TRASH).size(10.5),
                                                    )
                                                    .clicked()
                                                {
                                                    rule_to_remove = Some(r.from.clone());
                                                }
                                                if ui
                                                    .button(
                                                        egui::RichText::new(ic::NOTE_PENCIL)
                                                            .size(10.5),
                                                    )
                                                    .clicked()
                                                {
                                                    rule_to_edit = Some(idx);
                                                }
                                            },
                                        );
                                    }
                                });
                            });
                        });
                }
            });
    }

    // Open the inline editor for the requested row
    if let Some(idx) = rule_to_edit {
        if let Ok(dict) = dictionary.read() {
            if let Some(r) = dict.rules().get(idx) {
                state.edit_index = Some(idx);
                state.edit_from = r.from.clone();
                state.edit_to = r.to.clone();
                state.edit_cat = r.category.clone().unwrap_or_default();
            }
        }
    }

    // Apply pending removal if clicked
    if let Some(target_from) = rule_to_remove {
        if let Ok(mut dict) = dictionary.write() {
            dict.remove_by_from(&target_from);
            let _ = dict.save_to_file();
            state.msg = Some(("کلمه از دیکشنری حذف شد".to_string(), Instant::now()));
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    // ── Bottom Action Buttons ──
    ui.horizontal(|ui| {
        if ui
            .button(
                egui::RichText::new(format!(
                    "{}  {}",
                    format_persian_display("ذخیره در فایل"),
                    ic::FLOPPY_DISK
                ))
                .size(11.0),
            )
            .clicked()
        {
            if let Ok(dict) = dictionary.read() {
                if dict.save_to_file().is_ok() {
                    state.msg = Some(("دیکشنری با موفقیت ذخیره شد".to_string(), Instant::now()));
                }
            }
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
            if let Ok(mut dict) = dictionary.write() {
                if dict.reload_from_file().is_ok() {
                    state.msg = Some(("دیکشنری از دیسک بازخوانی شد".to_string(), Instant::now()));
                }
            }
        }
    });
}
