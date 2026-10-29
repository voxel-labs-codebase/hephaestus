//! Projects panel: the project tree with source documents and libraries.

use egui::{RichText, Ui, vec2};

use crate::actions::Action;
use crate::app::HephaestusApp;
use crate::documents::DocKey;
use crate::icons::{self, Icon};

fn tree_item(
    ui: &mut Ui,
    app: &HephaestusApp,
    indent: f32,
    icon: Icon,
    icon_color: egui::Color32,
    label: &str,
    selected: bool,
) -> egui::Response {
    let theme = &app.theme;
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(r, 0.0, theme.selection);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, theme.hover);
    }
    let ir = egui::Rect::from_center_size(egui::pos2(r.left() + indent + 8.0, r.center().y), vec2(14.0, 14.0));
    icons::paint(ui.painter(), ir, icon, icon_color);
    ui.painter().text(
        egui::pos2(ir.right() + 5.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        crate::theme::Theme::font(12.0),
        if selected { theme.selection_text } else { theme.text },
    );
    resp
}

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.small_button("Compile").clicked() {
            action = Some(Action::Compile);
        }
        if ui.small_button("+ Sheet").on_hover_text("Add New Schematic Sheet").clicked() {
            action = Some(Action::AddSheet);
        }
    });
    ui.add_space(2.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let name = app.design().name.clone();
        let dirty = app.session.editor.is_dirty();
        tree_item(ui, app, 0.0, Icon::Project, theme.accent, &format!("{name}.PrjPcb{}", if dirty { " *" } else { "" }), false);
        tree_item(ui, app, 14.0, Icon::Folder, theme.text_dim, "Source Documents", false);
        let active = app.view().map(|v| v.key);
        let sheets: Vec<(hephaestus_core::ObjectId, String)> =
            app.design().sheets_in_order().iter().map(|s| (s.id, s.name.clone())).collect();
        for (id, sheet) in sheets {
            let key = DocKey::Sheet(id);
            let resp = tree_item(
                ui,
                app,
                28.0,
                Icon::Schematic,
                egui::Color32::from_rgb(230, 190, 80),
                &format!("{sheet}.SchDoc"),
                active == Some(key),
            );
            if resp.clicked() || resp.double_clicked() {
                action = Some(Action::OpenDoc(key));
            }
            resp.context_menu(|ui| {
                if ui.button("Open").clicked() {
                    action = Some(Action::OpenDoc(key));
                    ui.close();
                }
                if ui.button("Remove Sheet").clicked() {
                    app.perform(Action::OpenDoc(key));
                    action = Some(Action::RemoveSheet);
                    ui.close();
                }
            });
        }
        let pcb = format!("{name}.PcbDoc");
        let resp = tree_item(ui, app, 28.0, Icon::Pcb, egui::Color32::from_rgb(90, 190, 90), &pcb, active == Some(DocKey::Pcb));
        if resp.clicked() || resp.double_clicked() {
            action = Some(Action::OpenDoc(DocKey::Pcb));
        }
        tree_item(ui, app, 14.0, Icon::Folder, theme.text_dim, "Libraries", false);
        let lib = format!("{} ({} components)", app.design().library.name, app.design().library.parts.len());
        tree_item(ui, app, 28.0, Icon::Library, theme.text_dim, &lib, false);
        let sources: Vec<String> = app.session.libraries().iter().map(|l| format!("{} ({} components)", l.name, l.parts.len())).collect();
        for s in sources {
            tree_item(ui, app, 28.0, Icon::Library, theme.text_dim, &s, false);
        }
        ui.add_space(8.0);
        let location =
            app.session.files.as_ref().map(|f| f.manifest_path().display().to_string()).unwrap_or_else(|| "Not saved yet".into());
        ui.label(RichText::new(location).size(11.0).color(theme.text_dim));
    });
    if let Some(a) = action {
        app.perform(a);
    }
}
