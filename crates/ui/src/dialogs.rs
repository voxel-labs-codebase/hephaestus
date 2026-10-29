//! Modal-style windows: design rules, layer stack, preferences, shortcuts, about, confirmations.

use egui::{RichText, Ui};
use hephaestus_commands::Command;
use hephaestus_core::geometry::{Coord, Unit, format_coord, parse_length};
use hephaestus_core::{LayerStack, NetClass};

use crate::actions::{Action, shortcut};
use crate::app::HephaestusApp;

fn length_edit(ui: &mut Ui, label: &str, value: &mut Coord, unit: Unit) {
    ui.label(label);
    let mut text = format_coord(*value, unit);
    let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
    if resp.changed()
        && let Some(v) = parse_length(&text, unit)
    {
        *value = v;
    }
    ui.end_row();
}

pub fn show(app: &mut HephaestusApp, ctx: &egui::Context) {
    let theme = app.theme.clone();
    let unit = app.prefs.pcb_unit;

    if let Some(mut rules) = app.dialogs.rules.take() {
        let mut open = true;
        let mut apply = false;
        egui::Window::new("PCB Rules and Constraints Editor").open(&mut open).resizable(true).default_width(460.0).collapsible(false).show(
            ctx,
            |ui| {
                ui.label(RichText::new("Board-wide rules. Net classes override clearance and width for their nets.").color(theme.text_dim));
                egui::Grid::new("rules-grid").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new("Electrical").strong());
                    ui.end_row();
                    length_edit(ui, "Clearance", &mut rules.clearance, unit);
                    ui.label(RichText::new("Routing").strong());
                    ui.end_row();
                    length_edit(ui, "Min Width", &mut rules.min_track_width, unit);
                    length_edit(ui, "Preferred Width", &mut rules.preferred_track_width, unit);
                    length_edit(ui, "Max Width", &mut rules.max_track_width, unit);
                    length_edit(ui, "Via Diameter", &mut rules.preferred_via_diameter, unit);
                    length_edit(ui, "Via Hole", &mut rules.preferred_via_drill, unit);
                    length_edit(ui, "Min Via Diameter", &mut rules.min_via_diameter, unit);
                    length_edit(ui, "Min Via Hole", &mut rules.min_via_drill, unit);
                    ui.label(RichText::new("Manufacturing").strong());
                    ui.end_row();
                    length_edit(ui, "Min Annular Ring", &mut rules.min_annular_ring, unit);
                    length_edit(ui, "Min Hole Size", &mut rules.min_hole_size, unit);
                    length_edit(ui, "Max Hole Size", &mut rules.max_hole_size, unit);
                    length_edit(ui, "Board Edge Clearance", &mut rules.board_edge_clearance, unit);
                });
                ui.separator();
                ui.label(RichText::new("Net Classes").strong());
                let mut remove = None;
                for (i, c) in rules.net_classes.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(80.0));
                        let mut nets = c.nets.join(", ");
                        if ui.add(egui::TextEdit::singleline(&mut nets).hint_text("nets, comma separated").desired_width(150.0)).changed() {
                            c.nets = nets.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                        }
                        let mut w = format_coord(c.track_width, unit);
                        if ui.add(egui::TextEdit::singleline(&mut w).desired_width(60.0)).on_hover_text("Track width").changed()
                            && let Some(v) = parse_length(&w, unit)
                        {
                            c.track_width = v;
                        }
                        let mut cl = format_coord(c.clearance, unit);
                        if ui.add(egui::TextEdit::singleline(&mut cl).desired_width(60.0)).on_hover_text("Clearance").changed()
                            && let Some(v) = parse_length(&cl, unit)
                        {
                            c.clearance = v;
                        }
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    rules.net_classes.remove(i);
                }
                if ui.button("Add Net Class").clicked() {
                    let w = rules.preferred_track_width * 2;
                    let c = rules.clearance;
                    rules.net_classes.push(NetClass {
                        name: "Power".into(),
                        nets: vec!["VCC".into(), "GND".into()],
                        track_width: w,
                        clearance: c,
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                });
            },
        );
        if apply {
            app.run(Command::SetDesignRules { rules: rules.clone() });
        }
        if open && !apply {
            app.dialogs.rules = Some(rules);
        }
    }

    if let Some(mut count) = app.dialogs.stack.take() {
        let mut open = true;
        let mut apply = false;
        let preview = LayerStack::standard(count);
        egui::Window::new("Layer Stack Manager").open(&mut open).default_width(460.0).collapsible(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Copper layers");
                for n in [2u8, 4, 6, 8] {
                    ui.selectable_value(&mut count, n, n.to_string());
                }
            });
            crate::panels::header_row(ui, &theme, &[("#", 26.0), ("Name", 120.0), ("Material", 110.0), ("Thickness", 80.0), ("Dk", 40.0)]);
            for (i, l) in preview.layers.iter().enumerate() {
                crate::panels::table_row(
                    ui,
                    &theme,
                    false,
                    &[
                        ((i + 1).to_string(), 26.0, theme.text_dim),
                        (l.name.clone(), 120.0, theme.text_strong),
                        ("Copper".into(), 110.0, theme.text),
                        (format_coord(l.copper_thickness, Unit::Mm), 80.0, theme.text),
                        (String::new(), 40.0, theme.text),
                    ],
                );
                if let Some(d) = &l.dielectric {
                    crate::panels::table_row(
                        ui,
                        &theme,
                        false,
                        &[
                            (String::new(), 26.0, theme.text_dim),
                            ("Dielectric".into(), 120.0, theme.text_dim),
                            (d.material.clone(), 110.0, theme.text),
                            (format_coord(d.thickness, Unit::Mm), 80.0, theme.text),
                            (format!("{:.1}", d.constant), 40.0, theme.text),
                        ],
                    );
                }
            }
            ui.label(RichText::new(format!("Total thickness {}", format_coord(preview.total_thickness(), Unit::Mm))).color(theme.text_dim));
            ui.label(RichText::new("Removing layers is refused while tracks, pours or vias use them.").size(11.0).color(theme.text_dim));
            if ui.button("Apply").clicked() {
                apply = true;
            }
        });
        if apply && count != app.design().board.stack.copper_count() {
            app.run(Command::SetLayerStack { copper_layers: count });
        }
        if open && !apply {
            app.dialogs.stack = Some(count);
        }
    }

    if app.dialogs.preferences {
        let mut open = true;
        egui::Window::new("Preferences").open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
            egui::Grid::new("prefs").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                ui.label("Mouse wheel");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut app.prefs.navigation.wheel_zooms, false, "Scroll (Ctrl+wheel zooms) — Altium");
                    ui.radio_value(&mut app.prefs.navigation.wheel_zooms, true, "Zoom");
                });
                ui.end_row();
                ui.label("Cross select");
                ui.checkbox(&mut app.prefs.cross_select, "Selecting in one editor selects in the other");
                ui.end_row();
                ui.label("Autosave");
                ui.add(egui::Slider::new(&mut app.prefs.autosave_seconds, 0..=900).suffix(" s").text("0 = off"));
                ui.end_row();
                ui.label("Agent port");
                ui.add(egui::DragValue::new(&mut app.prefs.control_port).range(0..=65535))
                    .on_hover_text("Takes effect at next start; 0 disables");
                ui.end_row();
                ui.label("Schematic units");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut app.prefs.sch_unit, Unit::Mil, "mil");
                    ui.radio_value(&mut app.prefs.sch_unit, Unit::Mm, "mm");
                });
                ui.end_row();
                ui.label("PCB units");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut app.prefs.pcb_unit, Unit::Mil, "mil");
                    ui.radio_value(&mut app.prefs.pcb_unit, Unit::Mm, "mm");
                });
                ui.end_row();
            });
        });
        app.dialogs.preferences = open;
    }

    if app.dialogs.shortcuts {
        let mut open = true;
        egui::Window::new("Keyboard Shortcuts").open(&mut open).default_width(420.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                egui::Grid::new("shortcuts").striped(true).num_columns(2).show(ui, |ui| {
                    let list = [
                        ("Pan", "Right-drag / middle-drag / wheel"),
                        ("Zoom", "Ctrl+wheel, PgUp / PgDn"),
                        ("Fit document", "Ctrl+PgDn, V F"),
                        ("Select", "Click, Shift+click toggles, drag a box (left→right inside, right→left touching)"),
                        ("Highlight net", "Alt+click (schematic), Ctrl+click (PCB)"),
                        ("Rotate / corner mode", "Space"),
                        ("Mirror during placement", "X"),
                        ("Cancel / exit tool", "Esc or right-click"),
                        ("Remove last vertex", "Backspace"),
                        ("Next / previous layer", "* or + / −"),
                        ("View configuration", "L"),
                    ];
                    for (a, k) in list {
                        ui.label(a);
                        ui.label(RichText::new(k).color(theme.accent));
                        ui.end_row();
                    }
                    for a in [
                        Action::Save,
                        Action::Undo,
                        Action::Redo,
                        Action::Delete,
                        Action::ToggleUnits,
                        Action::CycleGrid,
                        Action::Compile,
                        Action::RunErc,
                        Action::RunDrc,
                        Action::UpdatePcb,
                        Action::SchTool(crate::actions::SchToolKind::Part),
                        Action::SchTool(crate::actions::SchToolKind::Wire),
                        Action::SchTool(crate::actions::SchToolKind::NetLabel),
                        Action::SchTool(crate::actions::SchToolKind::Vcc),
                        Action::SchTool(crate::actions::SchToolKind::Gnd),
                        Action::PcbTool(crate::actions::PcbToolKind::Route),
                        Action::PcbTool(crate::actions::PcbToolKind::Via),
                        Action::PcbTool(crate::actions::PcbToolKind::Zone),
                        Action::PcbTool(crate::actions::PcbToolKind::Measure),
                        Action::ToggleSingleLayer,
                    ] {
                        ui.label(format!("{a:?}"));
                        ui.label(RichText::new(shortcut(&a)).color(theme.accent));
                        ui.end_row();
                    }
                });
            });
        });
        app.dialogs.shortcuts = open;
    }

    if app.dialogs.about {
        let mut open = true;
        egui::Window::new("About Hephaestus").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.heading("Hephaestus");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label("Schematic capture and PCB design, with an MCP interface for engineering agents.");
            ui.label(RichText::new("Built with Rust, egui and craft-ui.").color(theme.text_dim));
        });
        app.dialogs.about = open;
    }

    if let Some((message, action)) = app.dialogs.confirm.clone() {
        let mut close = false;
        egui::Window::new("Unsaved Changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
            ctx,
            |ui| {
                ui.label(message);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        app.perform(Action::Save);
                        if !app.session.editor.is_dirty() {
                            crate::commands::perform(app, action.clone());
                        }
                        close = true;
                    }
                    if ui.button("Discard").clicked() {
                        crate::commands::perform(app, action.clone());
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            },
        );
        if close {
            app.dialogs.confirm = None;
        }
    }

    if let Some(path) = app.dialogs.recovery.clone() {
        let mut close = false;
        egui::Window::new("Recover Unsaved Work").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
            ctx,
            |ui| {
                ui.label("An autosave newer than the saved project was found. The previous session may have ended unexpectedly.");
                ui.horizontal(|ui| {
                    if ui.button("Recover").clicked() {
                        match hephaestus_project::load_bundle(&path) {
                            Ok(design) => {
                                app.session.editor.replace_design(design);
                                app.reset_documents();
                                app.status = "Recovered the autosaved design (save to keep it)".into();
                            }
                            Err(e) => app.status = format!("Recovery failed: {e}"),
                        }
                        close = true;
                    }
                    if ui.button("Discard Autosave").clicked() {
                        if let Some(f) = &app.session.files {
                            f.clear_autosave();
                        }
                        close = true;
                    }
                });
            },
        );
        if close {
            app.dialogs.recovery = None;
        }
    }
}
