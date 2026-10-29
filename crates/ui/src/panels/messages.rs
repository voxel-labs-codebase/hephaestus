//! Messages: ERC and DRC violations; double-click navigates to the objects.

use egui::{RichText, Ui};
use hephaestus_rules::{Severity, Violation};

use crate::actions::Action;
use crate::app::HephaestusApp;
use crate::documents::DocKey;
use crate::icons::{self, Icon};
use crate::panels::header_row;

fn navigate(app: &mut HephaestusApp, v: &Violation) {
    let d = app.design();
    let pcb = v.check == hephaestus_rules::Check::Drc;
    let ids: Vec<hephaestus_core::ObjectId> =
        v.objects.iter().copied().filter(|id| if pcb { d.pcb.contains_key(id) } else { d.schematic.contains_key(id) }).collect();
    let mut area = ids.iter().filter_map(|id| app.cache.bounds.get(id).copied()).reduce(|a, b| a.union(b));
    if let Some(p) = v.location {
        let at =
            hephaestus_core::geometry::Rect::from_center(p, hephaestus_core::geometry::mil(100.0), hephaestus_core::geometry::mil(100.0));
        area = Some(area.map_or(at, |a| a.union(at)));
    }
    let key =
        if pcb { DocKey::Pcb } else { v.sheet.map(DocKey::Sheet).unwrap_or_else(|| app.view().map(|x| x.key).unwrap_or(DocKey::Pcb)) };
    app.focus(key, ids, area);
}

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let mut run = None;
    ui.horizontal(|ui| {
        if ui.small_button("Run ERC").clicked() {
            run = Some(Action::RunErc);
        }
        if ui.small_button("Run DRC").clicked() {
            run = Some(Action::RunDrc);
        }
        let erc = app
            .reports
            .erc
            .as_ref()
            .map(|r| format!("ERC: {} errors, {} warnings", r.errors, r.warnings))
            .unwrap_or_else(|| "ERC not run".into());
        let drc = app
            .reports
            .drc
            .as_ref()
            .map(|r| format!("DRC: {} errors, {} warnings", r.errors, r.warnings))
            .unwrap_or_else(|| "DRC not run".into());
        ui.label(RichText::new(format!("{erc}   ·   {drc}")).size(11.5).color(theme.text_dim));
        let stale = app.reports.erc.is_some() || app.reports.drc.is_some();
        if stale {
            ui.label(RichText::new("(re-run after changes)").size(10.5).color(theme.text_disabled));
        }
    });
    header_row(ui, &theme, &[("", 22.0), ("Class", 70.0), ("Rule", 170.0), ("Message", 600.0)]);
    let mut entries: Vec<(bool, usize, Violation)> = Vec::new();
    if let Some(r) = &app.reports.erc {
        entries.extend(r.violations.iter().cloned().enumerate().map(|(i, v)| (false, i, v)));
    }
    if let Some(r) = &app.reports.drc {
        entries.extend(r.violations.iter().cloned().enumerate().map(|(i, v)| (true, i, v)));
    }
    let mut go = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if entries.is_empty() {
            ui.label(
                RichText::new("No messages. Compile (C, C) runs the electrical rule check; Tools › Design Rule Check validates the board.")
                    .color(theme.text_dim),
            );
        }
        for (drc, i, v) in entries {
            let selected = app.reports.selected == Some((drc, i));
            let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 19.0), egui::Sense::click());
            let fill = if selected {
                theme.selection
            } else if resp.hovered() {
                theme.hover
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(r, 0.0, fill);
            let icon = match v.severity {
                Severity::Error => (Icon::Error, theme.error),
                Severity::Warning => (Icon::Warning, theme.warning),
                Severity::Info => (Icon::Info, theme.text_dim),
            };
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(egui::pos2(r.left() + 11.0, r.center().y), egui::vec2(13.0, 13.0)),
                icon.0,
                icon.1,
            );
            let font = crate::theme::Theme::font(12.0);
            let text = if selected { theme.selection_text } else { theme.text };
            ui.painter().text(
                egui::pos2(r.left() + 26.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                if drc { "DRC" } else { "ERC" },
                font.clone(),
                theme.text_dim,
            );
            ui.painter().text(egui::pos2(r.left() + 96.0, r.center().y), egui::Align2::LEFT_CENTER, &v.rule, font.clone(), text);
            ui.painter().with_clip_rect(r).text(
                egui::pos2(r.left() + 266.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                &v.message,
                font,
                text,
            );
            if resp.clicked() {
                app.reports.selected = Some((drc, i));
            }
            if resp.double_clicked() {
                go = Some(v.clone());
            }
            resp.on_hover_text("Double-click to jump to the objects");
        }
    });
    if let Some(v) = go {
        navigate(app, &v);
    }
    if let Some(a) = run {
        app.perform(a);
    }
}
