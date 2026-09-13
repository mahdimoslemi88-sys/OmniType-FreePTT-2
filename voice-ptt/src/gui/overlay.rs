//! Floating status overlay (egui/eframe): a small always-on-top bubble that
//! reflects the current state (Idle / Recording / Processing / Typing) and
//! shows the last typed text briefly.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::state::{AppStatus, AppState};

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

/// eframe application implementing the overlay bubble.
pub struct OverlayApp {
    status: Arc<StatusClient>,
    visible: bool,
    last_text_time: Option<Instant>,
    /// Set externally (tray menu / hotkey) to request a visibility toggle.
    overlay_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Set externally (tray menu / hotkey) to request application quit.
    quit_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl OverlayApp {
    pub fn new(
        status: Arc<StatusClient>,
        overlay_flag: Arc<std::sync::atomic::AtomicBool>,
        quit_flag: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            status,
            visible: true,
            last_text_time: None,
            overlay_flag,
            quit_flag,
        }
    }

    /// Toggles overlay visibility.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }
}

impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Consume external control flags.
        if self
            .overlay_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.toggle_visible();
        }
        if self.quit_flag.load(std::sync::atomic::Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // ~15 fps is plenty for a status bubble and keeps CPU near zero.
        ctx.request_repaint_after(Duration::from_millis(66));

        if !self.visible {
            return;
        }

        let status = self.status.get();

        // Show the last typed text for a few seconds after injection.
        if status.last_text.is_some() && self.last_text_time.is_none() {
            self.last_text_time = Some(Instant::now());
        }
        if let Some(t) = self.last_text_time {
            if t.elapsed() > Duration::from_secs(4) {
                self.last_text_time = None;
            }
        }

        let (label, text_color, fill) = match &status.state {
            AppState::Idle => (
                "● idle",
                egui::Color32::from_gray(200),
                egui::Color32::from_rgba_unmultiplied(40, 40, 46, 200),
            ),
            AppState::Recording => (
                "● REC",
                egui::Color32::from_rgb(255, 120, 120),
                egui::Color32::from_rgba_unmultiplied(120, 30, 30, 220),
            ),
            AppState::Processing => (
                "● …",
                egui::Color32::from_rgb(250, 210, 120),
                egui::Color32::from_rgba_unmultiplied(110, 90, 20, 220),
            ),
            AppState::Typing => (
                "● type",
                egui::Color32::from_rgb(140, 240, 180),
                egui::Color32::from_rgba_unmultiplied(30, 100, 60, 220),
            ),
            AppState::Error(_) => (
                "⚠ error",
                egui::Color32::from_rgb(255, 160, 160),
                egui::Color32::from_rgba_unmultiplied(110, 30, 30, 220),
            ),
        };

        egui::Area::new(egui::Id::new("ptt_overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -64.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(fill)
                    .rounding(12.0)
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0))
                    .show(ui, |ui| {
                        ui.set_min_width(140.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(label)
                                    .strong()
                                    .size(15.0)
                                    .color(text_color),
                            );

                            if let (Some(text), Some(_)) =
                                (&status.last_text, self.last_text_time)
                            {
                                ui.separator();
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(text)
                                            .size(12.0)
                                            .color(egui::Color32::from_gray(220)),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Wrap),
                                );
                            }

                            if let AppState::Error(e) = &status.state {
                                ui.separator();
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(e)
                                            .size(11.0)
                                            .color(egui::Color32::from_rgb(255, 160, 160)),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Wrap),
                                );
                            }

                            ui.label(
                                egui::RichText::new(format!("VAD: {}", status.vad_engine))
                                    .size(9.0)
                                    .color(egui::Color32::from_gray(150)),
                            );
                        });
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_toggles_visibility() {
        let (_tx, rx) = tokio::sync::watch::channel(AppStatus {
            state: AppState::Idle,
            last_text: None,
            vad_engine: "rms",
        });
        let flags = (
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut app = OverlayApp::new(Arc::new(StatusClient::new(rx)), flags.0, flags.1);
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
            vad_engine: "rms",
        });
        let client = Arc::new(StatusClient::new(rx));
        assert_eq!(client.get().state, AppState::Idle);
        tx.send(AppStatus {
            state: AppState::Recording,
            last_text: None,
            vad_engine: "rms",
        })
        .unwrap();
        assert_eq!(client.get().state, AppState::Recording);
    }
}
