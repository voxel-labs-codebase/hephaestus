//! Navigator: the compiled design's components and nets; click to cross-probe and zoom.

use egui::{RichText, Ui};
use hephaestus_core::ObjectId;

use crate::app::HephaestusApp;
use crate::documents::DocKey;
use crate::panels::{header_row, table_row};

fn natural(d: &str) -> (String, u64) {
    let split = d.find(|c: char| c.is_ascii_digit()).unwrap_or(d.len());
    let (a, b) = d.split_at(split);
    (a.to_string(), b.parse().unwrap_or(u64::MAX))
}

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let mut comps: Vec<(ObjectId, String, String, String, bool)> = app
        .design()
        .components
        .values()
        .map(|c| (c.id, c.designator.clone(), c.value.clone(), c.footprint.clone(), app.design().footprint_of(c.id).is_some()))
        .collect();
    comps.sort_by_key(|c| natural(&c.1));
    let selected = app.selected_components(app.active);
    let mut clicked_comp = None;
    let mut clicked_net = None;
    ui.label(RichText::new(format!("{} components · {} nets", comps.len(), app.cache.netlist.nets.len())).size(11.0).color(theme.text_dim));
    header_row(ui, &theme, &[("Designator", 74.0), ("Comment", 84.0), ("Footprint", 90.0)]);
    let half = (ui.available_height() * 0.5).max(100.0);
    egui::ScrollArea::vertical().id_salt("nav-comps").max_height(half).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (id, des, val, fp, placed) in &comps {
            let fp_text = if *placed { fp.clone() } else { format!("{fp} (not on PCB)") };
            let r = table_row(
                ui,
                &theme,
                selected.contains(id),
                &[
                    (des.clone(), 74.0, theme.text_strong),
                    (val.clone(), 84.0, theme.text),
                    (fp_text, 160.0, if *placed { theme.text_dim } else { theme.warning }),
                ],
            );
            if r.clicked() {
                clicked_comp = Some(*id);
            }
        }
    });
    ui.add_space(4.0);
    header_row(ui, &theme, &[("Net", 120.0), ("Pins", 50.0), ("Routing", 90.0)]);
    egui::ScrollArea::vertical().id_salt("nav-nets").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let active_net = app.view().and_then(|v| v.highlight_net.clone());
        for net in &app.cache.netlist.nets {
            let routing = app
                .cache
                .pcb
                .nets
                .get(&net.name)
                .map(|r| if r.unrouted == 0 { "Routed".to_string() } else { format!("{} unrouted", r.unrouted) })
                .unwrap_or_else(|| "–".into());
            let color = if net.power { theme.accent } else { theme.text_strong };
            let r = table_row(
                ui,
                &theme,
                active_net.as_deref() == Some(net.name.as_str()),
                &[(net.name.clone(), 120.0, color), (net.nodes.len().to_string(), 50.0, theme.text), (routing, 120.0, theme.text_dim)],
            );
            if r.clicked() {
                clicked_net = Some(net.name.clone());
            }
            r.on_hover_text(net.nodes.iter().map(|n| format!("{}.{}", n.designator, n.pin)).collect::<Vec<_>>().join(", "));
        }
    });
    if let Some(c) = clicked_comp {
        app.select_components(&[c]);
        let b = crate::commands::selection_bounds(app);
        if let Some(v) = app.view_mut() {
            v.pending_focus = b;
        }
    }
    if let Some(n) = clicked_net {
        for v in &mut app.docs {
            v.highlight_net = Some(n.clone());
        }
        // Zoom to the net's objects on the active document.
        let d = app.design();
        let area = match app.view().map(|v| v.key) {
            Some(DocKey::Sheet(s)) => app.cache.netlist.net(&n).and_then(|net| {
                net.objects
                    .iter()
                    .chain(net.nodes.iter().map(|x| &x.symbol))
                    .filter(|id| d.schematic.get(id).is_some_and(|o| o.sheet == s))
                    .filter_map(|id| app.cache.bounds.get(id).copied())
                    .reduce(|a, b| a.union(b))
            }),
            Some(DocKey::Pcb) => app
                .cache
                .pads
                .iter()
                .filter(|p| p.net.as_deref() == Some(n.as_str()))
                .filter_map(|p| p.shape.bounds())
                .reduce(|a, b| a.union(b)),
            None => None,
        };
        if let Some(v) = app.view_mut() {
            v.pending_focus = area;
        }
    }
}
