//! View Configuration: layer colours and visibility, the active layer, and object display.

use egui::{RichText, Ui, vec2};
use hephaestus_core::{Layer, LayerKind};

use crate::app::HephaestusApp;
use crate::icons::{self, Icon};
use crate::panels::section;

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let copper = app.design().board.stack.copper_count();
    let active = app.docs.iter().find(|v| v.is_pcb()).map(|v| v.active_layer).unwrap_or(Layer::TopCopper);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        type Group = (&'static str, fn(LayerKind) -> bool);
        let groups: [Group; 4] = [
            ("Signal Layers", |k| k == LayerKind::Signal),
            ("Component Layers", |k| matches!(k, LayerKind::Overlay | LayerKind::Paste | LayerKind::Solder)),
            ("Mechanical Layers", |k| k == LayerKind::Mechanical),
            ("Other Layers", |k| matches!(k, LayerKind::KeepOut | LayerKind::Multi | LayerKind::Drill)),
        ];
        for (title, pred) in groups {
            section(ui, &theme, title, |ui| {
                for l in Layer::display_order(copper).into_iter().filter(|l| pred(l.kind())) {
                    ui.horizontal(|ui| {
                        let visible = app.prefs.layer_visible(l);
                        if icons::button(ui, if visible { Icon::Eye } else { Icon::EyeOff }, "Show/hide layer", false, 20.0, &theme)
                            .clicked()
                        {
                            if visible {
                                app.prefs.hidden_layers.insert(l);
                            } else {
                                app.prefs.hidden_layers.remove(&l);
                            }
                        }
                        let mut c = app.prefs.layer_color(l);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            app.prefs.set_layer_color(l, c);
                        }
                        let label = RichText::new(l.to_string()).color(if l == active {
                            theme.accent
                        } else if visible {
                            theme.text
                        } else {
                            theme.text_disabled
                        });
                        let resp = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
                        if resp.clicked() && l.is_copper() {
                            app.set_active_layer(l);
                        }
                        if l == active {
                            ui.label(RichText::new("current").size(10.5).color(theme.text_dim));
                        }
                    });
                }
            });
        }
        section(ui, &theme, "Object Display", |ui| {
            ui.checkbox(&mut app.prefs.show_ratsnest, "Connection lines (ratsnest)");
            ui.checkbox(&mut app.prefs.show_pad_numbers, "Pad numbers");
            ui.checkbox(&mut app.prefs.show_net_names, "Net names on pads");
            let mut lines = app.prefs.pcb_grid_style == hephaestus_canvas::GridStyle::Lines;
            if ui.checkbox(&mut lines, "Grid as lines (otherwise dots)").changed() {
                app.prefs.pcb_grid_style = if lines { hephaestus_canvas::GridStyle::Lines } else { hephaestus_canvas::GridStyle::Dots };
            }
            if ui.button("Restore Default Colours").clicked() {
                app.prefs.layer_colors.clear();
            }
        });
        ui.add_space(vec2(0.0, 4.0).y);
        ui.label(
            RichText::new("Shortcuts: L opens this panel · * / + / − change the current layer · Shift+S single-layer mode")
                .size(10.5)
                .color(theme.text_dim),
        );
    });
}
