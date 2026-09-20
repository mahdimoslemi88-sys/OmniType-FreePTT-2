//! Demonstrates the width cap added in this PR.
//!
//! On `main` (post 0.19), toast width is fully automatic: it is re-measured
//! from the rendered content every frame, so a long single-word caption
//! stretches the card to (at least) the full text width with no way to cap
//! it. This example shows the two new knobs, `Toasts::with_max_width`
//! (channel-wide default) and `Toast::max_width` (per-toast override).

use eframe::{
    egui::{Context, Ui},
    App, Frame, NativeOptions,
};
use egui_notify::Toasts;

struct ExampleApp {
    toasts: Toasts,
    caption: String,
}

impl App for ExampleApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.toasts.show(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        ui.heading("Toast width cap demo");
        ui.add_space(8.0);
        ui.text_edit_multiline(&mut self.caption);

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Auto width (default)").clicked() {
                self.toasts.basic(self.caption.clone());
            }
            if ui.button("Capped to 180 px").clicked() {
                // Per-toast override.
                self.toasts.basic(self.caption.clone()).max_width(180.);
            }
            if ui.button("Uncapped").clicked() {
                // Beats the channel-wide cap.
                self.toasts.basic(self.caption.clone()).max_width(None);
            }
        });

        ui.add_space(12.0);
        ui.label(
            "With a cap set, longer captions wrap and the card grows \
            vertically instead of stretching horizontally.",
        );
    }
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "toast-width-cap",
        NativeOptions::default(),
        Box::new(|_cc| {
            Ok(Box::new(ExampleApp {
                // Channel-wide cap: every toast wraps at 260 px unless it
                // sets its own `max_width`.
                toasts: Toasts::default().with_max_width(260.),
                caption: "A very long single-word caption without any spaces that would \
                    otherwise stretch its toast card across the whole screen"
                    .into(),
            }))
        }),
    )
}
