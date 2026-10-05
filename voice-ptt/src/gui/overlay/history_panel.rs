//! History tab: the persistent list of past transcriptions.
//!
//! The only panel that is already a free function rather than a method on
//! [`OverlayApp`], because it is the only one whose state is small and fully
//! disjoint: the list, the search box, and the transient "copied" message.
//! Nothing here can reach the dictionary, the engine registry or the settings
//! draft, which is the property the other panels are being refactored towards.

use std::time::{Duration, Instant};

use eframe::egui;
use egui_phosphor::regular as ic;

use super::text::format_persian_display;
use super::theme::{callout, manager_card, manager_header, palette, CalloutKind};

/// Historical voice transcription record.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct HistoryItem {
    pub id: usize,
    pub text: String,
    pub timestamp: String,
    pub engine: String,
}

/// A request to fix a word in one of the transcripts.
///
/// The tab's *output*, not an action: History does not open the dictionary, does
/// not know the fix panel exists, and does not guess which word was wrong. It
/// reports that the user picked a sentence, and the overlay seeds the fix card
/// with it. This is the same hand-off shape the settings tab's orb controls use,
/// and it is what keeps a change in one tab from being able to reach another.
pub(crate) struct FixRequest {
    /// The word to correct. Empty: the user has not pointed at one yet, and
    /// guessing would be the unauthorised learning the roadmap forbids.
    pub word: String,
    /// The whole sentence, which becomes the preview's subject.
    pub sentence: String,
}

/// Renders the History tab body into the dashboard's current panel.
///
/// Takes the three fields it mutates as arguments instead of `&mut self`:
/// that is what lets the panel be a free function, and it makes the coupling
/// visible in the signature — if a future edit needs a fourth field, the
/// compiler will say so here instead of silently coupling two tabs.
pub(crate) fn render(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    history: &mut Vec<HistoryItem>,
    search: &mut String,
    copy_msg: &mut Option<(String, Instant)>,
    fix_request: &mut Option<FixRequest>,
) {
    let now = Instant::now();

    // ── Header & Title ──
    let count = history.len();
    manager_header(
        ui,
        "تاریخچه گفتار و رونوشت‌های صوتی",
        Some((
            &format!("{count} مورد ثبت‌شده"),
            palette::HEADER_PILL_BG,
            palette::ACCENT_SOFT,
        )),
    );

    ui.add_space(8.0);

    // ── Quick Actions Row (Copy All, Clear, Search) ──
    ui.horizontal(|ui| {
        let search_placeholder = format_persian_display("جستجو در متن‌ها...");
        ui.add(
            egui::TextEdit::singleline(search)
                .hint_text(search_placeholder)
                .desired_width(200.0),
        );

        if !history.is_empty() {
            if ui
                .button(
                    egui::RichText::new(format!(
                        "{}  {}",
                        format_persian_display("کپی همه متن‌ها"),
                        ic::CLIPBOARD_TEXT
                    ))
                    .size(11.5),
                )
                .clicked()
            {
                let all_texts = history
                    .iter()
                    .map(|h| h.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                ctx.copy_text(all_texts);
                *copy_msg = Some(("تمامی متن‌ها در کلیپ‌بورد کپی شدند!".into(), now));
            }

            if ui
                .button(
                    egui::RichText::new(format!(
                        "{}  {}",
                        format_persian_display("پاکسازی"),
                        ic::TRASH_SIMPLE
                    ))
                    .size(11.5)
                    .color(palette::DANGER_SOFT),
                )
                .clicked()
            {
                history.clear();
                *copy_msg = Some(("تاریخچه با موفقیت پاک شد.".into(), now));
            }
        }
    });

    // Notification / Feedback message
    if let Some((ref msg, t)) = *copy_msg {
        if now.duration_since(t) < Duration::from_secs(3) {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format_persian_display(msg))
                    .size(11.5)
                    .color(palette::SUCCESS_CHIPTXT),
            );
        }
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // Info callout about history retention
    callout(
        ui,
        CalloutKind::Info,
        "تاریخچه فقط روی همین کامپیوتر ذخیره می‌شود و هیچ‌گاه ارسال نمی‌شود.",
    );

    ui.add_space(6.0);

    // ── Items List ──
    let query = search.trim().to_lowercase();
    let filtered_indices: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|(_, h)| {
            query.is_empty()
                || h.text.to_lowercase().contains(&query)
                || h.engine.to_lowercase().contains(&query)
        })
        .map(|(i, _)| i)
        .collect();

    if filtered_indices.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                egui::RichText::new(format_persian_display("موردی در تاریخچه یافت نشد."))
                    .size(13.0)
                    .color(palette::TEXT_MUTED),
            );
            ui.label(
                egui::RichText::new(format_persian_display(
                    "هر صحبتی که انجام دهید به طور خودکار در این بخش نگهداری می‌شود.",
                ))
                .size(11.0)
                .color(palette::TEXT_FAINT),
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .id_source("history_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut delete_idx = None;
                for &idx in &filtered_indices {
                    let item = &history[idx];
                    manager_card(palette::CARD_TRANSLUCENT, palette::HAIRLINE).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        // Meta row: Time & Engine on the right, Actions on the left
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "⏱ {}",
                                    format_persian_display(&item.timestamp)
                                ))
                                .size(10.5)
                                .color(palette::TEXT_MUTED),
                            );

                            ui.label(
                                egui::RichText::new(format!(
                                    "⚡ {}",
                                    format_persian_display(&item.engine)
                                ))
                                .size(10.0)
                                .color(palette::ACCENT_SOFT),
                            );

                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(egui::RichText::new(ic::TRASH).size(11.0))
                                        .on_hover_text(format_persian_display("حذف این مورد"))
                                        .clicked()
                                    {
                                        delete_idx = Some(idx);
                                    }

                                    // The way into the dictionary fix: this
                                    // sentence, whose wrong word the user is
                                    // about to point at.
                                    let fix_btn = ui.button(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display("اصلاح واژه"),
                                            ic::MAGIC_WAND
                                        ))
                                        .size(11.5),
                                    );
                                    if fix_btn.clicked() {
                                        *fix_request = Some(FixRequest {
                                            word: String::new(),
                                            sentence: item.text.clone(),
                                        });
                                    }
                                    fix_btn.on_hover_text(format_persian_display(
                                        "این جمله را در اصلاح سریع واژه باز کن تا واژهٔ غلط را انتخاب کنی",
                                    ));

                                    let copy_btn = ui.button(
                                        egui::RichText::new(format!(
                                            "{}  {}",
                                            format_persian_display("کپی متن"),
                                            ic::COPY
                                        ))
                                        .size(11.5),
                                    );
                                    if copy_btn.clicked() {
                                        ctx.copy_text(item.text.clone());
                                        *copy_msg = Some(("متن در کلیپ‌بورد کپی شد!".into(), now));
                                    }
                                    copy_btn.on_hover_text(format_persian_display(
                                        "کپی کردن این رونوشت صوتی در کلیپ‌بورد",
                                    ));
                                },
                            );
                        });

                        ui.add_space(4.0);

                        // Persian text display
                        let display = format_persian_display(&item.text);
                        ui.label(
                            egui::RichText::new(display)
                                .size(12.5)
                                .color(palette::TEXT_PRIMARY),
                        );
                    });
                    ui.add_space(6.0);
                }

                if let Some(d_idx) = delete_idx {
                    history.remove(d_idx);
                }
            });
    }
}
