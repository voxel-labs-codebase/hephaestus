//! Agents: the change history (who changed what), pending proposals awaiting approval, open
//! transactions and the agent connection.

use egui::{RichText, Ui};
use hephaestus_commands::{LogKind, Source};

use crate::app::HephaestusApp;
use crate::icons::{self, Icon};

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        icons::paint(ui.painter(), r, Icon::Agent, theme.agent);
        match &app.control {
            Some((addr, _)) => ui.label(
                RichText::new(format!("Listening on {addr} — connect with hephaestus-mcp or the Python SDK")).size(11.5).color(theme.text),
            ),
            None => ui.label(
                RichText::new(format!("Agent port unavailable ({})", app.control_error.clone().unwrap_or_default()))
                    .size(11.5)
                    .color(theme.text_dim),
            ),
        };
        ui.checkbox(&mut app.session.policy.approve_destructive, "Require approval for destructive agent changes");
    });
    let proposals: Vec<hephaestus_commands::Proposal> = app.session.editor.proposals().cloned().collect();
    if !proposals.is_empty() {
        ui.label(RichText::new(format!("Proposed changes ({})", proposals.len())).strong().color(theme.agent));
        for p in proposals {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("#{} {} — {}", p.id, p.source, p.description)).color(theme.text_strong));
                if ui.button("Accept").clicked() {
                    match app.session.editor.accept(p.id) {
                        Ok(o) => app.status = format!("Accepted: {}", o.description),
                        Err(e) => app.status = format!("Could not apply proposal: {e}"),
                    }
                }
                if ui.button("Reject").clicked() {
                    let _ = app.session.editor.reject(p.id);
                    app.status = format!("Rejected proposal {}", p.id);
                }
            })
            .response
            .on_hover_text(p.preview.join("\n"));
        }
        ui.separator();
    }
    if let Some(t) = app.session.editor.transaction_name() {
        ui.label(RichText::new(format!("Open transaction: {t} (changes appear live; undo as one step)")).color(theme.agent));
    }
    let log: Vec<hephaestus_commands::LogEntry> = app.session.editor.log_since(0).into_iter().rev().take(300).cloned().collect();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for e in log {
            let (who, color) = match &e.source {
                Source::User => ("User".to_string(), theme.text),
                Source::Script => ("Script".to_string(), theme.text_dim),
                Source::Agent(n) => (format!("Agent: {n}"), theme.agent),
            };
            let kind = match e.kind {
                LogKind::Do => "",
                LogKind::Undo => "Undo · ",
                LogKind::Redo => "Redo · ",
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{:>4}", e.seq)).family(egui::FontFamily::Monospace).size(11.0).color(theme.text_disabled));
                ui.label(RichText::new(who).size(11.5).color(color));
                ui.label(RichText::new(format!("{kind}{}", e.description)).size(11.5).color(theme.text));
                if !e.touched.is_empty() {
                    ui.label(RichText::new(format!("({} object(s))", e.touched.len())).size(10.5).color(theme.text_dim));
                }
            });
        }
    });
}
