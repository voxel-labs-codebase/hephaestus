//! The Properties inspector, context-sensitive like Altium's: document options with nothing
//! selected, tool options while a placement tool is active, and per-object editors otherwise.
//! Every edit is a validated command, so it is undoable and visible to agents.

use std::collections::BTreeMap;

use egui::{RichText, Ui};
use hephaestus_commands::{Command, ComponentRef};
use hephaestus_core::geometry::{Point, Unit, format_coord};
use hephaestus_core::schematic::PortDirection;
use hephaestus_core::{Layer, ObjectId, Orientation, PcbKind, PcbObject, PowerStyle, SchKind, SchObject, SheetSize, Side, TitleBlock};

pub use super::EditState;
use super::{length_row, readout, row, section, text_row};
use crate::actions::Action;
use crate::app::HephaestusApp;
use crate::documents::{DocKey, Tool};
use crate::theme::Theme;

fn header(ui: &mut Ui, theme: &Theme, kind: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(kind).strong().size(13.0).color(theme.text_strong));
        ui.label(RichText::new(detail).size(11.5).color(theme.text_dim));
    });
    ui.separator();
}

fn rotation_combo(ui: &mut Ui, id: &str, current: f64) -> Option<f64> {
    let mut out = None;
    egui::ComboBox::from_id_salt(id).width(110.0).selected_text(format!("{current:.0} Degrees")).show_ui(ui, |ui| {
        for r in [0.0, 90.0, 180.0, 270.0] {
            if ui.selectable_label(current == r, format!("{r:.0} Degrees")).clicked() {
                out = Some(r);
            }
        }
    });
    out
}

fn net_combo(ui: &mut Ui, id: &str, nets: &[String], current: Option<&str>) -> Option<Option<String>> {
    let mut out = None;
    egui::ComboBox::from_id_salt(id)
        .width(ui.available_width().max(80.0))
        .selected_text(current.unwrap_or("No Net"))
        .height(320.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "No Net").clicked() {
                out = Some(None);
            }
            for n in nets {
                if ui.selectable_label(current == Some(n.as_str()), n).clicked() {
                    out = Some(Some(n.clone()));
                }
            }
        });
    out
}

fn layer_combo(ui: &mut Ui, id: &str, layers: &[Layer], current: Layer) -> Option<Layer> {
    let mut out = None;
    egui::ComboBox::from_id_salt(id).width(130.0).selected_text(current.to_string()).show_ui(ui, |ui| {
        for l in layers {
            if ui.selectable_label(*l == current, l.to_string()).clicked() {
                out = Some(*l);
            }
        }
    });
    out
}

fn location(ui: &mut Ui, theme: &Theme, edit: &mut EditState, key: &str, p: Point, unit: Unit) -> Option<Point> {
    let x = length_row(ui, theme, edit, "X", &format!("{key}.x"), p.x, unit);
    let y = length_row(ui, theme, edit, "Y", &format!("{key}.y"), p.y, unit);
    match (x, y) {
        (Some(x), _) => Some(Point::new(x, p.y)),
        (None, Some(y)) => Some(Point::new(p.x, y)),
        _ => None,
    }
}

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let Some(view) = app.view() else { return };
    let key = view.key;
    let selection = view.selection.clone();
    let tool = view.tool.clone();
    let mut commands: Vec<Command> = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        if !matches!(tool, Tool::Select) {
            tool_options(app, ui, &theme);
            ui.add_space(6.0);
        }
        match selection.as_slice() {
            [] => document(app, ui, &theme, key, &mut commands),
            [one] => {
                let d = app.design();
                if let Some(o) = d.schematic.get(one).cloned() {
                    sch_object(app, ui, &theme, &o, &mut commands);
                } else if let Some(o) = d.pcb.get(one).cloned() {
                    pcb_object(app, ui, &theme, &o, &mut commands);
                }
            }
            many => multi(app, ui, &theme, many, &mut commands),
        }
    });
    for c in commands {
        app.run(c);
    }
}

fn tool_options(app: &mut HephaestusApp, ui: &mut Ui, theme: &Theme) {
    let Some(view) = app.view_mut() else { return };
    let name = view.tool.name();
    header(ui, theme, name, "tool options");
    match &mut view.tool {
        Tool::NetLabel { text, .. } => {
            row(ui, theme, "Net Name", |ui| {
                ui.text_edit_singleline(text);
            });
        }
        Tool::Power { net, style, .. } => {
            row(ui, theme, "Name", |ui| {
                ui.text_edit_singleline(net);
            });
            row(ui, theme, "Style", |ui| {
                egui::ComboBox::from_id_salt("tool-power-style").selected_text(style.label()).show_ui(ui, |ui| {
                    for s in PowerStyle::ALL {
                        ui.selectable_value(style, s, s.label());
                    }
                });
            });
        }
        Tool::Port { name, direction, .. } => {
            row(ui, theme, "Name", |ui| {
                ui.text_edit_singleline(name);
            });
            row(ui, theme, "I/O Type", |ui| {
                egui::ComboBox::from_id_salt("tool-port-dir").selected_text(format!("{direction:?}")).show_ui(ui, |ui| {
                    for d in [PortDirection::Unspecified, PortDirection::Input, PortDirection::Output, PortDirection::Bidirectional] {
                        ui.selectable_value(direction, d, format!("{d:?}"));
                    }
                });
            });
        }
        Tool::Text { text } => {
            row(ui, theme, "Text", |ui| {
                ui.text_edit_singleline(text);
            });
        }
        Tool::PlacePart { part, rotation, mirror } => {
            readout(ui, theme, "Part", part.clone());
            readout(ui, theme, "Rotation", format!("{rotation:.0}° (Space)"));
            readout(ui, theme, "Mirrored", if *mirror { "Yes (X)" } else { "No (X)" });
        }
        Tool::Wire { mode, points } | Tool::Track { mode, points } => {
            readout(ui, theme, "Corner Mode", format!("{} (Space)", mode.label()));
            readout(ui, theme, "Vertices", points.len().to_string());
        }
        Tool::Route { mode, session } => {
            readout(ui, theme, "Corner Mode", format!("{} (Space)", mode.label()));
            if let Some(s) = session {
                readout(ui, theme, "Net", s.net.clone().unwrap_or_else(|| "No Net".into()));
                readout(ui, theme, "Width", format_coord(s.width, Unit::Mil));
                readout(ui, theme, "Vias", s.vias.len().to_string());
            } else {
                ui.label(RichText::new("Click a pad, via or track to start; change layer with * to insert a via; click the target pad to finish; Backspace undoes a vertex.").size(11.0).color(theme.text_dim));
            }
        }
        _ => {}
    }
}

fn document(app: &mut HephaestusApp, ui: &mut Ui, theme: &Theme, key: DocKey, commands: &mut Vec<Command>) {
    let unit = app.units();
    match key {
        DocKey::Sheet(s) => {
            let Some(sheet) = app.design().sheets.get(&s).cloned() else { return };
            header(ui, theme, "Document Options", &format!("{}.SchDoc", sheet.name));
            // An UpdateSheet that changes nothing; each field below fills in one part.
            let update = || Command::UpdateSheet {
                sheet: s,
                name: None,
                size: None,
                title: None,
                revision: None,
                orientation: None,
                title_block: None,
                show_zones: None,
            };
            section(ui, theme, "General", |ui| {
                if let Some(n) = text_row(ui, theme, &mut app.properties_edit, "Sheet Name", &format!("{s}.name"), &sheet.name) {
                    commands.push(with(update(), |c| {
                        if let Command::UpdateSheet { name, .. } = c {
                            *name = Some(n)
                        }
                    }));
                }
                if let Some(t) = text_row(ui, theme, &mut app.properties_edit, "Title", &format!("{s}.title"), &sheet.title) {
                    commands.push(with(update(), |c| {
                        if let Command::UpdateSheet { title, .. } = c {
                            *title = Some(t)
                        }
                    }));
                }
                if let Some(r) = text_row(ui, theme, &mut app.properties_edit, "Revision", &format!("{s}.rev"), &sheet.revision) {
                    commands.push(with(update(), |c| {
                        if let Command::UpdateSheet { revision, .. } = c {
                            *revision = Some(r)
                        }
                    }));
                }
            });
            section(ui, theme, "Page Options", |ui| {
                row(ui, theme, "Sheet Size", |ui| {
                    egui::ComboBox::from_id_salt("sheet-size").selected_text(sheet.size.label()).show_ui(ui, |ui| {
                        for z in SheetSize::ALL {
                            let (w, h) = z.mils();
                            if ui.selectable_label(z == sheet.size, format!("{}  ({}×{} mil)", z.label(), w, h)).clicked() {
                                commands.push(with(update(), |c| {
                                    if let Command::UpdateSheet { size, .. } = c {
                                        *size = Some(z)
                                    }
                                }));
                            }
                        }
                    });
                });
                row(ui, theme, "Orientation", |ui| {
                    egui::ComboBox::from_id_salt("sheet-orientation").selected_text(sheet.orientation.label()).show_ui(ui, |ui| {
                        for o in Orientation::ALL {
                            if ui.selectable_label(o == sheet.orientation, o.label()).clicked() {
                                commands.push(with(update(), |c| {
                                    if let Command::UpdateSheet { orientation, .. } = c {
                                        *orientation = Some(o)
                                    }
                                }));
                            }
                        }
                    });
                });
                row(ui, theme, "Title Block", |ui| {
                    egui::ComboBox::from_id_salt("sheet-title-block").selected_text(sheet.title_block.label()).show_ui(ui, |ui| {
                        for t in TitleBlock::ALL {
                            if ui.selectable_label(t == sheet.title_block, t.label()).clicked() {
                                commands.push(with(update(), |c| {
                                    if let Command::UpdateSheet { title_block, .. } = c {
                                        *title_block = Some(t)
                                    }
                                }));
                            }
                        }
                    });
                });
                row(ui, theme, "Zones", |ui| {
                    let mut z = sheet.show_zones;
                    if ui.checkbox(&mut z, "").changed() {
                        commands.push(with(update(), |c| {
                            if let Command::UpdateSheet { show_zones, .. } = c {
                                *show_zones = Some(z)
                            }
                        }));
                    }
                });
                let r = sheet.rect();
                readout(ui, theme, "Paper", format!("{} × {}", format_coord(r.width(), unit), format_coord(r.height(), unit)));
            });
            section(ui, theme, "Grid & Units", |ui| {
                readout(ui, theme, "Snap Grid", format_coord(app.prefs.sch_grid, unit));
                row(ui, theme, "Visible Grid", |ui| {
                    ui.checkbox(&mut app.prefs.sch_grid_visible, "");
                });
                readout(ui, theme, "Units", unit.suffix());
            });
            let netlist = &app.cache.netlist;
            section(ui, theme, "Design", |ui| {
                readout(ui, theme, "Objects", app.design().sheet_objects(s).count().to_string());
                readout(ui, theme, "Components", app.design().components.len().to_string());
                readout(ui, theme, "Nets", netlist.nets.len().to_string());
                readout(ui, theme, "Unconnected Pins", netlist.unconnected_pins.len().to_string());
            });
        }
        DocKey::Pcb => {
            let board = app.design().board.clone();
            header(ui, theme, "Board", &format!("{}.PcbDoc", app.design().name));
            section(ui, theme, "Board Information", |ui| {
                if let Some(b) = board.outline_bounds() {
                    readout(ui, theme, "Size", format!("{} × {}", format_coord(b.width(), unit), format_coord(b.height(), unit)));
                }
                readout(
                    ui,
                    theme,
                    "Layers",
                    format!("{} copper ({})", board.stack.copper_count(), format_coord(board.stack.total_thickness(), Unit::Mm)),
                );
                readout(
                    ui,
                    theme,
                    "Components",
                    app.design().pcb.values().filter(|o| matches!(o.kind, PcbKind::Footprint(_))).count().to_string(),
                );
                let total: usize = app.cache.pcb.nets.values().map(|n| n.connections).sum();
                readout(ui, theme, "Routing", format!("{} of {} connections", total.saturating_sub(app.cache.pcb.unrouted.len()), total));
                row(ui, theme, "", |ui| {
                    if ui.button("Layer Stack…").clicked() {
                        app.dialogs.stack = Some(board.stack.copper_count());
                    }
                    if ui.button("Rules…").clicked() {
                        app.dialogs.rules = Some(board.rules.clone());
                    }
                });
            });
            section(ui, theme, "Routing Rules", |ui| {
                let r = &board.rules;
                let mut next = r.clone();
                let mut changed = false;
                let e = &mut app.properties_edit;
                for (label, key, value, slot) in [
                    ("Clearance", "rules.clearance", r.clearance, &mut next.clearance as &mut i64),
                    ("Track Width", "rules.width", r.preferred_track_width, &mut next.preferred_track_width),
                    ("Min Width", "rules.minwidth", r.min_track_width, &mut next.min_track_width),
                    ("Via Diameter", "rules.via", r.preferred_via_diameter, &mut next.preferred_via_diameter),
                    ("Via Hole", "rules.drill", r.preferred_via_drill, &mut next.preferred_via_drill),
                    ("Edge Clearance", "rules.edge", r.board_edge_clearance, &mut next.board_edge_clearance),
                ] {
                    if let Some(v) = length_row(ui, theme, e, label, key, value, unit) {
                        *slot = v;
                        changed = true;
                    }
                }
                if changed {
                    commands.push(Command::SetDesignRules { rules: next });
                }
            });
            section(ui, theme, "Grid & Units", |ui| {
                readout(ui, theme, "Snap Grid", format_coord(app.prefs.pcb_grid, unit));
                readout(ui, theme, "Units", unit.suffix());
                if let Some(o) = location(ui, theme, &mut app.properties_edit, "origin", board.origin, unit) {
                    commands.push(Command::SetOrigin { origin: o });
                }
            });
        }
    }
}

fn sch_object(app: &mut HephaestusApp, ui: &mut Ui, theme: &Theme, o: &SchObject, commands: &mut Vec<Command>) {
    let unit = app.prefs.sch_unit;
    let id = o.id;
    let update = |kind: SchKind| Command::UpdateSchObject { object: SchObject { id: o.id, sheet: o.sheet, kind } };
    match &o.kind {
        SchKind::Symbol(s) => {
            let d = app.design();
            let Some(c) = d.components.get(&s.component).cloned() else { return };
            let part = d.library.parts.get(&c.part).cloned();
            let pins = d.pins(o);
            header(ui, theme, "Component", &c.designator);
            let cref = ComponentRef::Id(c.id);
            section(ui, theme, "Properties", |ui| {
                let e = &mut app.properties_edit;
                if let Some(v) = text_row(ui, theme, e, "Designator", &format!("{id}.des"), &c.designator) {
                    commands.push(Command::ModifyComponent {
                        component: cref.clone(),
                        designator: Some(v),
                        value: None,
                        footprint: None,
                        parameters: BTreeMap::new(),
                    });
                }
                if let Some(v) = text_row(ui, theme, e, "Comment", &format!("{id}.val"), &c.value) {
                    commands.push(Command::ModifyComponent {
                        component: cref.clone(),
                        designator: None,
                        value: Some(v),
                        footprint: None,
                        parameters: BTreeMap::new(),
                    });
                }
                readout(ui, theme, "Design Item", part.as_ref().map(|p| p.title()).unwrap_or_else(|| c.part.clone()));
                if let Some((m, mpn)) = part.as_ref().and_then(|p| p.sourcing()) {
                    readout(ui, theme, "Part Number", format!("{m} {mpn}"));
                }
                readout(ui, theme, "Description", part.as_ref().map(|p| p.description.clone()).unwrap_or_default());
                row(ui, theme, "Footprint", |ui| {
                    egui::ComboBox::from_id_salt(("fp", id)).width(140.0).selected_text(&c.footprint).show_ui(ui, |ui| {
                        for f in footprint_options(app, part.as_ref()) {
                            if ui.selectable_label(f == c.footprint, &f).clicked() {
                                commands.push(Command::ModifyComponent {
                                    component: cref.clone(),
                                    designator: None,
                                    value: None,
                                    footprint: Some(f),
                                    parameters: BTreeMap::new(),
                                });
                            }
                        }
                    });
                });
            });
            section(ui, theme, "Location", |ui| {
                if let Some(p) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), s.position, unit) {
                    commands.push(Command::MoveObjects { ids: vec![id], dx: p.x - s.position.x, dy: p.y - s.position.y, drag_wires: true });
                }
                row(ui, theme, "Rotation", |ui| {
                    if let Some(r) = rotation_combo(ui, &format!("{id}.rot"), s.rotation) {
                        commands.push(update(SchKind::Symbol(hephaestus_core::SymbolInstance { rotation: r, ..s.clone() })));
                    }
                });
                row(ui, theme, "Mirrored", |ui| {
                    let mut m = s.mirror;
                    if ui.checkbox(&mut m, "").changed() {
                        commands.push(update(SchKind::Symbol(hephaestus_core::SymbolInstance { mirror: m, ..s.clone() })));
                    }
                });
            });
            section(ui, theme, "Parameters", |ui| {
                let mut all: BTreeMap<String, (String, bool)> =
                    part.as_ref().map(|p| p.parameters.iter().map(|(k, v)| (k.clone(), (v.clone(), false))).collect()).unwrap_or_default();
                for (k, v) in &c.parameters {
                    all.insert(k.clone(), (v.clone(), true));
                }
                for (k, (v, _own)) in all {
                    let e = &mut app.properties_edit;
                    if let Some(nv) = text_row(ui, theme, e, &k, &format!("{id}.p.{k}"), &v) {
                        // Clearing a value removes the instance parameter (the part's default shows again).
                        let value = if nv.trim().is_empty() { None } else { Some(nv) };
                        commands.push(Command::ModifyComponent {
                            component: cref.clone(),
                            designator: None,
                            value: None,
                            footprint: None,
                            parameters: BTreeMap::from([(k.clone(), value)]),
                        });
                    }
                }
                let key = format!("{id}.newparam");
                let buf = app.properties_edit.buffers.entry(key.clone()).or_default();
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(buf).hint_text("New parameter name").desired_width(150.0));
                    if ui.button("Add").clicked() && !buf.trim().is_empty() {
                        commands.push(Command::ModifyComponent {
                            component: cref.clone(),
                            designator: None,
                            value: None,
                            footprint: None,
                            parameters: BTreeMap::from([(buf.trim().to_string(), Some("?".to_string()))]),
                        });
                        buf.clear();
                    }
                });
            });
            section(ui, theme, "Pins", |ui| {
                super::header_row(ui, theme, &[("Pin", 40.0), ("Name", 90.0), ("Type", 70.0), ("Net", 100.0)]);
                for p in &pins {
                    let net = app.cache.netlist.net_of_pin(p.component, &p.number).map(|n| n.name.clone()).unwrap_or_else(|| "—".into());
                    super::table_row(
                        ui,
                        theme,
                        false,
                        &[
                            (p.number.clone(), 40.0, theme.text_strong),
                            (p.name.clone(), 90.0, theme.text),
                            (p.pin_type.label().into(), 70.0, theme.text_dim),
                            (net, 140.0, theme.text),
                        ],
                    );
                }
            });
            if let Some((fo, f)) = app.design().footprint_of(c.id) {
                section(ui, theme, "PCB", |ui| {
                    readout(
                        ui,
                        theme,
                        "Placed",
                        format!("{} at {}, {}", fo.id, format_coord(f.position.x, unit), format_coord(f.position.y, unit)),
                    );
                });
            }
        }
        SchKind::Wire(w) => {
            let net = app.cache.netlist.net_of_object(id).map(|n| n.name.clone()).unwrap_or_else(|| "(unconnected)".into());
            header(ui, theme, "Wire", &id.to_string());
            section(ui, theme, "Properties", |ui| {
                readout(ui, theme, "Net", net.clone());
                readout(ui, theme, "Vertices", w.points.len().to_string());
                let len: f64 = w.points.windows(2).map(|s| s[0].distance(s[1])).sum();
                readout(ui, theme, "Length", format_coord(len.round() as i64, unit));
                row(ui, theme, "", |ui| {
                    if ui.button("Highlight Net").clicked() {
                        for v in &mut app.docs {
                            v.highlight_net = Some(net.clone());
                        }
                    }
                });
            });
            section(ui, theme, "Vertices", |ui| {
                for (i, p) in w.points.iter().enumerate() {
                    readout(ui, theme, &format!("Vertex {}", i + 1), format!("{}, {}", format_coord(p.x, unit), format_coord(p.y, unit)));
                }
            });
        }
        SchKind::NetLabel(l) => {
            header(ui, theme, "Net Label", &l.text);
            section(ui, theme, "Properties", |ui| {
                if let Some(t) = text_row(ui, theme, &mut app.properties_edit, "Net Name", &format!("{id}.text"), &l.text) {
                    commands.push(update(SchKind::NetLabel(hephaestus_core::NetLabel { text: t, ..l.clone() })));
                }
                if let Some(p) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), l.position, unit) {
                    commands.push(update(SchKind::NetLabel(hephaestus_core::NetLabel { position: p, ..l.clone() })));
                }
                row(ui, theme, "Rotation", |ui| {
                    if let Some(r) = rotation_combo(ui, &format!("{id}.rot"), l.rotation) {
                        commands.push(update(SchKind::NetLabel(hephaestus_core::NetLabel { rotation: r, ..l.clone() })));
                    }
                });
            });
        }
        SchKind::PowerPort(p) => {
            header(ui, theme, "Power Port", &p.net);
            section(ui, theme, "Properties", |ui| {
                if let Some(t) = text_row(ui, theme, &mut app.properties_edit, "Name", &format!("{id}.net"), &p.net) {
                    commands.push(update(SchKind::PowerPort(hephaestus_core::PowerPort { net: t, ..p.clone() })));
                }
                row(ui, theme, "Style", |ui| {
                    egui::ComboBox::from_id_salt(("pstyle", id)).selected_text(p.style.label()).show_ui(ui, |ui| {
                        for s in PowerStyle::ALL {
                            if ui.selectable_label(s == p.style, s.label()).clicked() {
                                commands.push(update(SchKind::PowerPort(hephaestus_core::PowerPort { style: s, ..p.clone() })));
                            }
                        }
                    });
                });
                row(ui, theme, "Rotation", |ui| {
                    if let Some(r) = rotation_combo(ui, &format!("{id}.rot"), p.rotation) {
                        commands.push(update(SchKind::PowerPort(hephaestus_core::PowerPort { rotation: r, ..p.clone() })));
                    }
                });
                if let Some(pos) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), p.position, unit) {
                    commands.push(update(SchKind::PowerPort(hephaestus_core::PowerPort { position: pos, ..p.clone() })));
                }
            });
        }
        SchKind::Port(p) => {
            header(ui, theme, "Port", &p.name);
            section(ui, theme, "Properties", |ui| {
                if let Some(t) = text_row(ui, theme, &mut app.properties_edit, "Name", &format!("{id}.name"), &p.name) {
                    commands.push(update(SchKind::Port(hephaestus_core::Port { name: t, ..p.clone() })));
                }
                row(ui, theme, "I/O Type", |ui| {
                    egui::ComboBox::from_id_salt(("pdir", id)).selected_text(format!("{:?}", p.direction)).show_ui(ui, |ui| {
                        for d in [PortDirection::Unspecified, PortDirection::Input, PortDirection::Output, PortDirection::Bidirectional] {
                            if ui.selectable_label(d == p.direction, format!("{d:?}")).clicked() {
                                commands.push(update(SchKind::Port(hephaestus_core::Port { direction: d, ..p.clone() })));
                            }
                        }
                    });
                });
                if let Some(w) = length_row(ui, theme, &mut app.properties_edit, "Width", &format!("{id}.w"), p.width, unit) {
                    commands.push(update(SchKind::Port(hephaestus_core::Port { width: w, ..p.clone() })));
                }
                if let Some(pos) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), p.position, unit) {
                    commands.push(update(SchKind::Port(hephaestus_core::Port { position: pos, ..p.clone() })));
                }
            });
        }
        SchKind::Text(t) => {
            header(ui, theme, "Text", "");
            section(ui, theme, "Properties", |ui| {
                if let Some(v) = text_row(ui, theme, &mut app.properties_edit, "Text", &format!("{id}.text"), &t.text) {
                    commands.push(update(SchKind::Text(hephaestus_core::TextNote { text: v, ..t.clone() })));
                }
                if let Some(v) = length_row(ui, theme, &mut app.properties_edit, "Size", &format!("{id}.size"), t.size, unit) {
                    commands.push(update(SchKind::Text(hephaestus_core::TextNote { size: v, ..t.clone() })));
                }
                if let Some(pos) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), t.position, unit) {
                    commands.push(update(SchKind::Text(hephaestus_core::TextNote { position: pos, ..t.clone() })));
                }
            });
        }
        other => {
            header(ui, theme, other.type_name(), &id.to_string());
            section(ui, theme, "Location", |ui| {
                let a = o.anchor();
                if let Some(p) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), a, unit) {
                    commands.push(Command::MoveObjects { ids: vec![id], dx: p.x - a.x, dy: p.y - a.y, drag_wires: false });
                }
            });
        }
    }
}

fn pcb_object(app: &mut HephaestusApp, ui: &mut Ui, theme: &Theme, o: &PcbObject, commands: &mut Vec<Command>) {
    let unit = app.prefs.pcb_unit;
    let id = o.id;
    let nets: Vec<String> = app
        .cache
        .pcb
        .nets
        .keys()
        .cloned()
        .chain(app.cache.netlist.nets.iter().map(|n| n.name.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let copper = app.design().board.stack.copper_layers();
    let update = |kind: PcbKind| Command::UpdatePcbObject { object: PcbObject { id, kind } };
    match &o.kind {
        PcbKind::Footprint(f) => {
            let comp = f.component.and_then(|c| app.design().components.get(&c)).cloned();
            let part = comp.as_ref().and_then(|c| app.design().library.parts.get(&c.part)).cloned();
            header(ui, theme, "Component", comp.as_ref().map(|c| c.designator.as_str()).unwrap_or("(mechanical)"));
            section(ui, theme, "Properties", |ui| {
                if let Some(c) = &comp {
                    let cref = ComponentRef::Id(c.id);
                    if let Some(v) = text_row(ui, theme, &mut app.properties_edit, "Designator", &format!("{id}.des"), &c.designator) {
                        commands.push(Command::ModifyComponent {
                            component: cref.clone(),
                            designator: Some(v),
                            value: None,
                            footprint: None,
                            parameters: BTreeMap::new(),
                        });
                    }
                    if let Some(v) = text_row(ui, theme, &mut app.properties_edit, "Comment", &format!("{id}.val"), &c.value) {
                        commands.push(Command::ModifyComponent {
                            component: cref.clone(),
                            designator: None,
                            value: Some(v),
                            footprint: None,
                            parameters: BTreeMap::new(),
                        });
                    }
                    row(ui, theme, "Footprint", |ui| {
                        egui::ComboBox::from_id_salt(("pfp", id)).width(140.0).selected_text(&f.footprint).show_ui(ui, |ui| {
                            for name in footprint_options(app, part.as_ref()) {
                                if ui.selectable_label(name == f.footprint, &name).clicked() {
                                    commands.push(Command::ModifyComponent {
                                        component: cref.clone(),
                                        designator: None,
                                        value: None,
                                        footprint: Some(name),
                                        parameters: BTreeMap::new(),
                                    });
                                }
                            }
                        });
                    });
                } else {
                    readout(ui, theme, "Footprint", f.footprint.clone());
                }
                row(ui, theme, "Layer", |ui| {
                    egui::ComboBox::from_id_salt(("side", id))
                        .selected_text(if f.side == Side::Top { "Top Layer" } else { "Bottom Layer" })
                        .show_ui(ui, |ui| {
                            for (s, label) in [(Side::Top, "Top Layer"), (Side::Bottom, "Bottom Layer")] {
                                if ui.selectable_label(s == f.side, label).clicked() && s != f.side {
                                    commands.push(Command::MirrorObjects { ids: vec![id], pivot: None });
                                }
                            }
                        });
                });
                row(ui, theme, "Locked", |ui| {
                    let mut l = f.locked;
                    if ui.checkbox(&mut l, "").changed() {
                        commands.push(Command::LockObjects { ids: vec![id], locked: l });
                    }
                });
            });
            section(ui, theme, "Location", |ui| {
                if let Some(p) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), f.position, unit) {
                    commands.push(update(PcbKind::Footprint(hephaestus_core::Footprint { position: p, ..f.clone() })));
                }
                let rot = format!("{:.2}", f.rotation);
                if let Some(v) = text_row(ui, theme, &mut app.properties_edit, "Rotation", &format!("{id}.rot"), &rot)
                    && let Ok(r) = v.trim().trim_end_matches('°').parse::<f64>()
                    && r.is_finite()
                {
                    commands.push(update(PcbKind::Footprint(hephaestus_core::Footprint { rotation: r.rem_euclid(360.0), ..f.clone() })));
                }
            });
            let pads: Vec<hephaestus_core::PadGeometry> = app.cache.pads.iter().filter(|p| p.footprint == id).cloned().collect();
            section(ui, theme, "Pads", |ui| {
                super::header_row(ui, theme, &[("Pad", 34.0), ("Net", 84.0), ("Shape", 62.0), ("Size", 100.0)]);
                let short = |v: i64| {
                    let s = format_coord(v, unit);
                    s.trim_end_matches(unit.suffix()).to_string()
                };
                for p in pads {
                    let shape = match p.pad_shape {
                        hephaestus_core::PadShape::Round => "Round",
                        hephaestus_core::PadShape::Rect => "Rect",
                        hephaestus_core::PadShape::RoundRect { .. } => "RndRect",
                        hephaestus_core::PadShape::Oval => "Oval",
                    };
                    let size =
                        format!("{}×{}{}", short(p.width), short(p.height), p.drill.map(|d| format!(" ⌀{}", short(d))).unwrap_or_default());
                    super::table_row(
                        ui,
                        theme,
                        false,
                        &[
                            (p.number.clone(), 34.0, theme.text_strong),
                            (p.net.clone().unwrap_or_else(|| "No Net".into()), 84.0, theme.text),
                            (shape.into(), 62.0, theme.text_dim),
                            (size, 160.0, theme.text),
                        ],
                    );
                }
                ui.label(RichText::new(format!("Sizes in {}", unit.suffix())).size(10.5).color(theme.text_dim));
            });
            if let Some(m) = app.design().footprint_def(f).and_then(|d| d.model.clone()) {
                section(ui, theme, "3D Body", |ui| {
                    readout(ui, theme, "Model", m.source);
                    readout(ui, theme, "Height", format_coord(m.height, unit));
                });
            }
        }
        PcbKind::Track(t) => {
            header(ui, theme, "Track", &id.to_string());
            section(ui, theme, "Properties", |ui| {
                row(ui, theme, "Net", |ui| {
                    if let Some(n) = net_combo(ui, &format!("{id}.net"), &nets, t.net.as_deref()) {
                        commands.push(Command::ModifyTrack {
                            track: id,
                            start: None,
                            end: None,
                            width: None,
                            layer: None,
                            net: Some(n.unwrap_or_default()),
                        });
                    }
                });
                row(ui, theme, "Layer", |ui| {
                    if let Some(l) = layer_combo(ui, &format!("{id}.layer"), &copper, t.layer) {
                        commands.push(Command::ModifyTrack { track: id, start: None, end: None, width: None, layer: Some(l), net: None });
                    }
                });
                if let Some(w) = length_row(ui, theme, &mut app.properties_edit, "Width", &format!("{id}.w"), t.width, unit) {
                    commands.push(Command::ModifyTrack { track: id, start: None, end: None, width: Some(w), layer: None, net: None });
                }
                readout(ui, theme, "Length", format_coord(t.start.distance(t.end).round() as i64, unit));
            });
            section(ui, theme, "Location", |ui| {
                if let Some(p) = location(ui, theme, &mut app.properties_edit, &format!("{id}.s"), t.start, unit) {
                    commands.push(Command::ModifyTrack { track: id, start: Some(p), end: None, width: None, layer: None, net: None });
                }
                if let Some(p) = location(ui, theme, &mut app.properties_edit, &format!("{id}.e"), t.end, unit) {
                    commands.push(Command::ModifyTrack { track: id, start: None, end: Some(p), width: None, layer: None, net: None });
                }
            });
        }
        PcbKind::Via(v) => {
            header(ui, theme, "Via", &id.to_string());
            section(ui, theme, "Properties", |ui| {
                row(ui, theme, "Net", |ui| {
                    if let Some(n) = net_combo(ui, &format!("{id}.net"), &nets, v.net.as_deref()) {
                        commands.push(update(PcbKind::Via(hephaestus_core::Via { net: n, ..v.clone() })));
                    }
                });
                if let Some(d) = length_row(ui, theme, &mut app.properties_edit, "Diameter", &format!("{id}.d"), v.diameter, unit) {
                    commands.push(update(PcbKind::Via(hephaestus_core::Via { diameter: d, ..v.clone() })));
                }
                if let Some(d) = length_row(ui, theme, &mut app.properties_edit, "Hole Size", &format!("{id}.h"), v.drill, unit) {
                    commands.push(update(PcbKind::Via(hephaestus_core::Via { drill: d, ..v.clone() })));
                }
                readout(ui, theme, "Span", format!("{} – {}", v.from, v.to));
                if let Some(p) = location(ui, theme, &mut app.properties_edit, &format!("{id}.loc"), v.position, unit) {
                    commands.push(update(PcbKind::Via(hephaestus_core::Via { position: p, ..v.clone() })));
                }
            });
        }
        PcbKind::Zone(z) => {
            header(ui, theme, "Polygon Pour", &id.to_string());
            section(ui, theme, "Properties", |ui| {
                row(ui, theme, "Net", |ui| {
                    if let Some(n) = net_combo(ui, &format!("{id}.net"), &nets, z.net.as_deref()) {
                        commands.push(update(PcbKind::Zone(hephaestus_core::Zone { net: n, ..z.clone() })));
                    }
                });
                row(ui, theme, "Layer", |ui| {
                    if let Some(l) = layer_combo(ui, &format!("{id}.layer"), &copper, z.layer) {
                        commands.push(update(PcbKind::Zone(hephaestus_core::Zone { layer: l, ..z.clone() })));
                    }
                });
                readout(ui, theme, "Priority", z.priority.to_string());
                readout(ui, theme, "Vertices", z.outline.len().to_string());
                readout(ui, theme, "Poured", if z.filled.is_empty() { "No — pour filling is not implemented yet" } else { "Yes" });
            });
        }
        PcbKind::Keepout(k) => {
            header(ui, theme, "Keepout Region", &id.to_string());
            section(ui, theme, "Properties", |ui| {
                readout(
                    ui,
                    theme,
                    "Layers",
                    if k.layers.is_empty() {
                        "All copper layers".into()
                    } else {
                        k.layers.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ")
                    },
                );
                readout(ui, theme, "Restricts", "Tracks, vias, pads, pours");
                readout(ui, theme, "Vertices", k.outline.len().to_string());
            });
        }
        PcbKind::Text(t) => {
            header(ui, theme, "Text", &t.text);
            section(ui, theme, "Properties", |ui| {
                if let Some(v) = text_row(ui, theme, &mut app.properties_edit, "Text", &format!("{id}.t"), &t.text) {
                    commands.push(update(PcbKind::Text(hephaestus_core::PcbText { text: v, ..t.clone() })));
                }
                if let Some(h) = length_row(ui, theme, &mut app.properties_edit, "Height", &format!("{id}.h"), t.height, unit) {
                    commands.push(update(PcbKind::Text(hephaestus_core::PcbText { height: h, ..t.clone() })));
                }
                readout(ui, theme, "Layer", t.layer.to_string());
            });
        }
        PcbKind::Line(l) => {
            header(ui, theme, "Line", &id.to_string());
            section(ui, theme, "Properties", |ui| {
                readout(ui, theme, "Layer", l.layer.to_string());
                readout(ui, theme, "Width", format_coord(l.width, unit));
            });
        }
    }
}

fn multi(app: &mut HephaestusApp, ui: &mut Ui, theme: &Theme, ids: &[ObjectId], commands: &mut Vec<Command>) {
    let d = app.design();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for id in ids {
        let name =
            d.schematic.get(id).map(|o| o.kind.type_name()).or_else(|| d.pcb.get(id).map(|o| o.kind.type_name())).unwrap_or("Object");
        *kinds.entry(name).or_default() += 1;
    }
    header(ui, theme, &format!("{} objects", ids.len()), "selected");
    section(ui, theme, "Selection", |ui| {
        for (k, n) in &kinds {
            readout(ui, theme, k, n.to_string());
        }
    });
    let tracks: Vec<ObjectId> =
        ids.iter().copied().filter(|id| matches!(d.pcb.get(id).map(|o| &o.kind), Some(PcbKind::Track(_)))).collect();
    if !tracks.is_empty() {
        let unit = app.prefs.pcb_unit;
        let first = d.pcb.get(&tracks[0]).and_then(|o| if let PcbKind::Track(t) = &o.kind { Some(t.width) } else { None }).unwrap_or(0);
        section(ui, theme, "Tracks", |ui| {
            if let Some(w) = length_row(ui, theme, &mut app.properties_edit, "Width", "multi.width", first, unit) {
                commands.push(Command::Batch {
                    commands: tracks
                        .iter()
                        .map(|t| Command::ModifyTrack { track: *t, start: None, end: None, width: Some(w), layer: None, net: None })
                        .collect(),
                });
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.button("Align Left").clicked() {
            app.perform(Action::Align(hephaestus_commands::AlignMode::Left));
        }
        if ui.button("Align Top").clicked() {
            app.perform(Action::Align(hephaestus_commands::AlignMode::Top));
        }
        if ui.button("Distribute H").clicked() {
            app.perform(Action::Distribute(hephaestus_commands::Axis::Horizontal));
        }
    });
}

/// `value` after `f` edits it, for building a command in one expression.
fn with<T>(mut value: T, f: impl FnOnce(&mut T)) -> T {
    f(&mut value);
    value
}

/// Footprints a placed component may switch to: a generic component offers its family's
/// packages (the matching library component is created on the change), others their own list.
fn footprint_options(app: &HephaestusApp, part: Option<&hephaestus_core::PartDef>) -> Vec<String> {
    let Some(part) = part else { return Vec::new() };
    let family = part.family.as_ref().and_then(|f| {
        std::iter::once(&app.design().library).chain(app.session.libraries()).find_map(|l| l.parts.get(f)).map(|p| p.footprints.clone())
    });
    let mut out = family.unwrap_or_else(|| part.footprints.clone());
    for f in &part.footprints {
        if !out.contains(f) {
            out.insert(0, f.clone());
        }
    }
    out
}
