//! The main menu, context-sensitive to the active document like Altium's.

use egui::{Align, Layout, RichText, Ui};
use hephaestus_commands::{AlignMode, Axis};

use crate::actions::{Action, PcbToolKind, SchToolKind, shortcut};
use crate::app::HephaestusApp;
use crate::documents::DocKey;
use crate::icons::{self, Icon};
use crate::workspace::PanelId;

fn item(app: &mut HephaestusApp, ui: &mut Ui, label: &str, action: Action) {
    item_enabled(app, ui, label, action, true);
}

fn item_enabled(app: &mut HephaestusApp, ui: &mut Ui, label: &str, action: Action, enabled: bool) {
    let sc = shortcut(&action);
    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(sc)).clicked() {
        app.perform(action);
        ui.close();
    }
}

fn toggle(ui: &mut Ui, label: &str, value: &mut bool) {
    if ui.add(egui::Button::new(label).selected(*value)).clicked() {
        *value = !*value;
        ui.close();
    }
}

pub fn menu_bar(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.horizontal_centered(|ui| {
        // Quick-access icons, as on Altium's title bar.
        let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        icons::paint(ui.painter(), r, Icon::Pcb, theme.accent);
        ui.add_space(2.0);
        if icons::button(ui, Icon::Save, "Save (Ctrl+S)", false, 20.0, &theme).clicked() {
            app.perform(Action::Save);
        }
        if icons::button(ui, Icon::Open, "Open Project (Ctrl+O)", false, 20.0, &theme).clicked() {
            app.perform(Action::OpenProject);
        }
        ui.add_enabled_ui(app.session.editor.can_undo(), |ui| {
            let tip = format!("Undo {} (Ctrl+Z)", app.session.editor.undo_label().unwrap_or(""));
            if icons::button(ui, Icon::Undo, &tip, false, 20.0, &theme).clicked() {
                app.perform(Action::Undo);
            }
        });
        ui.add_enabled_ui(app.session.editor.can_redo(), |ui| {
            if icons::button(ui, Icon::Redo, "Redo (Ctrl+Y)", false, 20.0, &theme).clicked() {
                app.perform(Action::Redo);
            }
        });
        ui.add_space(6.0);
        egui::MenuBar::new().ui(ui, |ui| menus(app, ui));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let name = app.design().name.clone();
            let dirty = if app.session.editor.is_dirty() { " •" } else { "" };
            ui.label(RichText::new(format!("{name}{dirty}")).color(theme.text_dim).size(11.5));
            if let Some(prefix) = app.key_prefix {
                ui.label(RichText::new(format!("[{prefix}]")).color(theme.accent).strong());
            }
        });
    });
}

fn menus(app: &mut HephaestusApp, ui: &mut Ui) {
    let pcb = app.is_pcb();
    let has_sel = app.view().is_some_and(|v| !v.selection.is_empty());
    ui.menu_button("File", |ui| {
        item(app, ui, "New Project", Action::NewProject);
        item(app, ui, "Open Project…", Action::OpenProject);
        item(app, ui, "Open Example: LED Flasher", Action::OpenDemo);
        let recent = app.prefs.recent.clone();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Recent Projects", |ui| {
                for p in recent {
                    let label = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.button(label).on_hover_text(p.display().to_string()).clicked() {
                        app.perform(Action::OpenRecent(p));
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        item(app, ui, "Save Project", Action::Save);
        item(app, ui, "Save Project As…", Action::SaveAs);
        ui.separator();
        ui.menu_button("Fabrication Outputs", |ui| {
            item(app, ui, "Gerber Files…", Action::ExportGerber);
            item(app, ui, "NC Drill Files…", Action::ExportDrill);
            item(app, ui, "All Fabrication and Assembly Outputs…", Action::ExportAll);
        });
        ui.menu_button("Assembly Outputs", |ui| {
            item(app, ui, "Generates pick and place files…", Action::ExportPickPlace);
            item(app, ui, "Bill of Materials…", Action::ExportBom);
        });
        ui.separator();
        item(app, ui, "Exit", Action::Exit);
    });
    ui.menu_button("Edit", |ui| {
        let undo = app.session.editor.undo_label().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into());
        let redo = app.session.editor.redo_label().map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into());
        let (cu, cr) = (app.session.editor.can_undo(), app.session.editor.can_redo());
        item_enabled(app, ui, &undo, Action::Undo, cu);
        item_enabled(app, ui, &redo, Action::Redo, cr);
        ui.separator();
        item_enabled(app, ui, "Cut", Action::Cut, has_sel);
        item_enabled(app, ui, "Copy", Action::Copy, has_sel);
        let can_paste = app.clipboard.is_some();
        item_enabled(app, ui, "Paste", Action::Paste, can_paste);
        item_enabled(app, ui, "Delete", Action::Delete, has_sel);
        ui.separator();
        item(app, ui, "Select All", Action::SelectAll);
        item_enabled(app, ui, "Deselect All", Action::ClearSelection, has_sel);
        ui.separator();
        item_enabled(app, ui, "Rotate Selection", Action::Rotate, has_sel);
        if pcb {
            item_enabled(app, ui, "Flip Selection to Other Side", Action::FlipSide, has_sel);
        } else {
            item_enabled(app, ui, "Mirror Selection", Action::MirrorX, has_sel);
        }
        ui.add_enabled_ui(has_sel, |ui| {
            ui.menu_button("Align", |ui| {
                item(app, ui, "Align Left", Action::Align(AlignMode::Left));
                item(app, ui, "Align Right", Action::Align(AlignMode::Right));
                item(app, ui, "Align Top", Action::Align(AlignMode::Top));
                item(app, ui, "Align Bottom", Action::Align(AlignMode::Bottom));
                item(app, ui, "Align Horizontal Centers", Action::Align(AlignMode::CenterHorizontal));
                item(app, ui, "Align Vertical Centers", Action::Align(AlignMode::CenterVertical));
                ui.separator();
                item(app, ui, "Distribute Horizontally", Action::Distribute(Axis::Horizontal));
                item(app, ui, "Distribute Vertically", Action::Distribute(Axis::Vertical));
            });
        });
        if pcb {
            item_enabled(app, ui, "Lock Components", Action::Lock(true), has_sel);
            item_enabled(app, ui, "Unlock Components", Action::Lock(false), has_sel);
        }
    });
    ui.menu_button("View", |ui| {
        item(app, ui, "Fit All Objects", Action::Fit);
        item(app, ui, "Fit Document", Action::FitDocument);
        item_enabled(app, ui, "Fit Selected Objects", Action::FitSelection, has_sel);
        item(app, ui, "Zoom In", Action::ZoomIn);
        item(app, ui, "Zoom Out", Action::ZoomOut);
        ui.separator();
        item(app, ui, "Toggle Units (Imperial/Metric)", Action::ToggleUnits);
        item(app, ui, "Cycle Snap Grid", Action::CycleGrid);
        if pcb {
            item(app, ui, "Single Layer Mode", Action::ToggleSingleLayer);
            toggle(ui, "Connection Lines", &mut app.prefs.show_ratsnest);
            toggle(ui, "Pad Numbers", &mut app.prefs.show_pad_numbers);
            toggle(ui, "Net Names on Pads", &mut app.prefs.show_net_names);
        } else {
            toggle(ui, "Visible Grid", &mut app.prefs.sch_grid_visible);
        }
        ui.separator();
        ui.menu_button("Panels", |ui| {
            for p in PanelId::ALL {
                let open = app.prefs.layout.contains(&p);
                if ui.add(egui::Button::new(p.title()).selected(open)).clicked() {
                    app.perform(Action::TogglePanel(p));
                    ui.close();
                }
            }
        });
        item(app, ui, "Reset Workspace Layout", Action::ResetLayout);
    });
    ui.menu_button("Project", |ui| {
        item(app, ui, "Compile PCB Project", Action::Compile);
        ui.separator();
        item(app, ui, "Add New Schematic Sheet", Action::AddSheet);
        let sheet = !pcb && app.design().sheets.len() > 1;
        item_enabled(app, ui, "Remove Current Sheet", Action::RemoveSheet, sheet);
        ui.separator();
        if ui.button("Show Change History").clicked() {
            app.focus_panel = Some(PanelId::Agents);
            ui.close();
        }
    });
    ui.menu_button("Place", |ui| {
        if pcb {
            item(app, ui, "Interactive Routing", Action::PcbTool(PcbToolKind::Route));
            item(app, ui, "Line", Action::PcbTool(PcbToolKind::Track));
            item(app, ui, "Via", Action::PcbTool(PcbToolKind::Via));
            item(app, ui, "Polygon Pour", Action::PcbTool(PcbToolKind::Zone));
            item(app, ui, "Keepout Region", Action::PcbTool(PcbToolKind::Keepout));
        } else {
            item(app, ui, "Part…", Action::SchTool(SchToolKind::Part));
            item(app, ui, "Wire", Action::SchTool(SchToolKind::Wire));
            item(app, ui, "Net Label", Action::SchTool(SchToolKind::NetLabel));
            item(app, ui, "Power Port (VCC)", Action::SchTool(SchToolKind::Vcc));
            item(app, ui, "Power Port (GND)", Action::SchTool(SchToolKind::Gnd));
            item(app, ui, "Port", Action::SchTool(SchToolKind::Port));
            item(app, ui, "Junction", Action::SchTool(SchToolKind::Junction));
            item(app, ui, "Directives › No ERC", Action::SchTool(SchToolKind::NoErc));
            item(app, ui, "Text String", Action::SchTool(SchToolKind::Text));
        }
    });
    ui.menu_button("Design", |ui| {
        item(app, ui, "Update PCB Document", Action::UpdatePcb);
        ui.separator();
        item(app, ui, "Rules…", Action::DesignRules);
        item(app, ui, "Layer Stack Manager…", Action::LayerStack);
        item(app, ui, "Board Shape › Redefine Board Shape", Action::PcbTool(PcbToolKind::BoardShape));
    });
    ui.menu_button("Tools", |ui| {
        item(app, ui, "Electrical Rule Check", Action::RunErc);
        item(app, ui, "Design Rule Check…", Action::RunDrc);
        ui.separator();
        item_enabled(app, ui, "Cross Probe", Action::CrossProbe, has_sel);
        toggle(ui, "Cross Select Mode", &mut app.prefs.cross_select);
        item(app, ui, "Clear Net Highlight", Action::ClearHighlight);
        ui.separator();
        item(app, ui, "Preferences…", Action::Preferences);
    });
    ui.menu_button("Route", |ui| {
        item(app, ui, "Interactive Routing", Action::PcbTool(PcbToolKind::Route));
        item(app, ui, "Via", Action::PcbTool(PcbToolKind::Via));
        ui.separator();
        ui.label(RichText::new("Auto Route: direct routing of a whole net is available to agents (route_net).").size(11.0).weak());
    });
    ui.menu_button("Reports", |ui| {
        item(app, ui, "Bill of Materials…", Action::ExportBom);
        item(app, ui, "Measure Distance", Action::PcbTool(PcbToolKind::Measure));
        if ui.button("Netlist Status").clicked() {
            app.focus_panel = Some(PanelId::Navigator);
            ui.close();
        }
    });
    ui.menu_button("Window", |ui| {
        let docs: Vec<(DocKey, String)> = app
            .docs
            .iter()
            .map(|d| {
                (
                    d.key,
                    match d.key {
                        DocKey::Sheet(s) => app.design().sheets.get(&s).map(|s| format!("{}.SchDoc", s.name)).unwrap_or_default(),
                        DocKey::Pcb => format!("{}.PcbDoc", app.design().name),
                    },
                )
            })
            .collect();
        let active = app.view().map(|v| v.key);
        for (key, name) in docs {
            if ui.add(egui::Button::new(name).selected(Some(key) == active)).clicked() {
                app.perform(Action::OpenDoc(key));
                ui.close();
            }
        }
        ui.separator();
        item(app, ui, "Reset Workspace Layout", Action::ResetLayout);
    });
    ui.menu_button("Help", |ui| {
        item(app, ui, "Keyboard Shortcuts", Action::ShortcutHelp);
        item(app, ui, "About Hephaestus", Action::About);
    });
}
