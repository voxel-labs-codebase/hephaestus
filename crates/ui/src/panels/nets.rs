//! The PCB panel: board nets with pad counts and routing state; click to highlight.

use egui::{RichText, Ui};

use crate::app::HephaestusApp;
use crate::documents::DocKey;
use crate::panels::{header_row, table_row};

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let total: usize = app.cache.pcb.nets.values().map(|n| n.connections).sum();
    let unrouted = app.cache.pcb.unrouted.len();
    let pct = if total == 0 { 100.0 } else { (total - unrouted.min(total)) as f64 / total as f64 * 100.0 };
    ui.label(
        RichText::new(format!(
            "Nets: {} · Routed {:.0}% ({} of {} connections) · Shorts: {}",
            app.cache.pcb.nets.len(),
            pct,
            total - unrouted.min(total),
            total,
            app.cache.pcb.shorts.len()
        ))
        .size(11.5)
        .color(theme.text),
    );
    let highlighted = app.docs.iter().find(|v| v.is_pcb()).and_then(|v| v.highlight_net.clone());
    ui.horizontal(|ui| {
        if ui.small_button("Clear Highlight").clicked() {
            for v in &mut app.docs {
                v.highlight_net = None;
            }
        }
        ui.checkbox(&mut app.prefs.show_ratsnest, "Connection lines");
    });
    header_row(ui, &theme, &[("Net", 120.0), ("Pads", 44.0), ("Unrouted", 70.0), ("Tracks", 60.0)]);
    let mut clicked = None;
    let d = app.design();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (name, r) in &app.cache.pcb.nets {
            let tracks = d
                .pcb
                .values()
                .filter(|o| matches!(&o.kind, hephaestus_core::PcbKind::Track(t) if t.net.as_deref() == Some(name.as_str())))
                .count();
            let color = if r.unrouted > 0 { theme.warning } else { theme.ok };
            let row = table_row(
                ui,
                &theme,
                highlighted.as_deref() == Some(name.as_str()),
                &[
                    (name.clone(), 120.0, theme.text_strong),
                    (r.pads.to_string(), 44.0, theme.text),
                    (r.unrouted.to_string(), 70.0, color),
                    (tracks.to_string(), 60.0, theme.text),
                ],
            );
            if row.clicked() {
                clicked = Some(name.clone());
            }
        }
    });
    if let Some(n) = clicked {
        let area = app
            .cache
            .pads
            .iter()
            .filter(|p| p.net.as_deref() == Some(n.as_str()))
            .filter_map(|p| p.shape.bounds())
            .reduce(|a, b| a.union(b));
        for v in &mut app.docs {
            v.highlight_net = Some(n.clone());
        }
        app.open_doc(DocKey::Pcb);
        if let Some(v) = app.view_mut() {
            v.pending_focus = area;
        }
    }
}
