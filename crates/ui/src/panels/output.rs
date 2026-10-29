//! Output: the session log (saves, exports, agent activity, errors).

use egui::{RichText, Ui};

use crate::app::HephaestusApp;

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.horizontal(|ui| {
        if ui.small_button("Clear").clicked() {
            app.session.messages.clear();
        }
        ui.label(RichText::new(format!("{} message(s)", app.session.messages.len())).size(11.0).color(theme.text_dim));
    });
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        for m in &app.session.messages {
            let color = if m.contains("error") || m.contains("failed") {
                theme.error
            } else if m.starts_with("Agent") {
                theme.agent
            } else {
                theme.text
            };
            ui.label(RichText::new(m).family(egui::FontFamily::Monospace).size(11.5).color(color));
        }
    });
}
