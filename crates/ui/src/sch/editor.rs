//! The schematic editor: navigation, selection, moving, and the placement and wiring tools.

use std::collections::BTreeSet;

use egui::{Pos2, Rect, Sense, Stroke, Ui};
use hephaestus_canvas::{GridLook, GridStyle, SelectionBox, draw_grid, nearest_hotspot, paint_cursor};
use hephaestus_commands::Command;
use hephaestus_core::geometry::{Point, mil, point_segment_distance};
use hephaestus_core::schematic::{Junction, NetLabel, NoConnect, Port, PowerPort, TextNote};
use hephaestus_core::{Design, ObjectId, SchKind, SchObject, SymbolInstance, drag};

use crate::actions::Action;
use crate::app::HephaestusApp;
use crate::documents::{DocKey, Drag, Tool};
use crate::draw::Canvas;
use crate::sch::render::{self, Look};
use crate::workspace::PanelId;

/// Increment a trailing number (`D0` → `D1`, `NetLabel9` → `NetLabel10`), as Altium does when
/// placing labels repeatedly.
pub fn increment(text: &str) -> String {
    let digits = text.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return text.to_string();
    }
    let (head, tail) = text.split_at(text.len() - digits);
    match tail.parse::<u64>() {
        Ok(n) => format!("{head}{}", n + 1),
        Err(_) => text.to_string(),
    }
}

/// The object under a world point, preferring small objects over large ones.
pub fn hit(design: &Design, cache: &crate::cache::Cache, sheet: ObjectId, p: Point, tol: i64) -> Option<ObjectId> {
    let index = cache.sheet_index.get(&sheet)?;
    let mut best: Option<(i128, ObjectId)> = None;
    for id in index.query_point(p, tol) {
        let Some(o) = design.schematic.get(id) else { continue };
        let score: i128 = match &o.kind {
            SchKind::Wire(w) => {
                let d = w
                    .points
                    .windows(2)
                    .filter_map(|s| Some(point_segment_distance(p, *s.first()?, *s.get(1)?)))
                    .fold(f64::INFINITY, f64::min);
                if d > tol as f64 {
                    continue;
                }
                d as i128
            }
            _ => {
                let Some(b) = cache.bounds.get(id) else { continue };
                if !b.inflated(tol).contains(p) {
                    continue;
                }
                // Area ranks symbols below labels and junctions on top of them.
                (b.width() as i128 * b.height() as i128) / 1_000_000 + 1_000
            }
        };
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, *id));
        }
    }
    best.map(|(_, id)| id)
}

/// Hot spots for electrical snapping: pin ends, wire vertices, label and port anchors.
fn hotspots(design: &Design, sheet: ObjectId) -> Vec<Point> {
    let mut v: Vec<Point> = design.sheet_pins(sheet).into_iter().map(|p| p.position).collect();
    for o in design.sheet_objects(sheet) {
        match &o.kind {
            SchKind::Wire(w) => v.extend(w.points.iter().copied()),
            SchKind::PowerPort(p) => v.push(p.position),
            SchKind::Port(p) => v.push(p.position),
            SchKind::NetLabel(l) => v.push(l.position),
            _ => {}
        }
    }
    v
}

fn net_of(design: &Design, cache: &crate::cache::Cache, id: ObjectId, at: Point) -> Option<String> {
    if let Some(n) = cache.netlist.net_of_object(id) {
        return Some(n.name.clone());
    }
    let o = design.schematic.get(&id)?;
    let pins = design.pins(o);
    let pin = pins.iter().min_by(|a, b| a.position.distance(at).total_cmp(&b.position.distance(at)))?;
    cache.netlist.net_of_pin(pin.component, &pin.number).map(|n| n.name.clone())
}

pub fn show(app: &mut HephaestusApp, ui: &mut Ui, canvas: Rect) {
    let idx = app.active;
    let Some(DocKey::Sheet(sheet)) = app.docs.get(idx).map(|v| v.key) else { return };
    let response = ui.interact(canvas, egui::Id::new(("sch-canvas", sheet)), Sense::click_and_drag());
    // The canvas never takes keyboard focus: egui would turn Space (rotate) into a click.
    // Editor keys apply while the pointer is over the canvas.
    let mut actions: Vec<Action> = Vec::new();
    let mut commands: Vec<Command> = Vec::new();
    {
        let HephaestusApp { session, docs, cache, prefs, theme, focus_panel, cross_probe_from, components, .. } = app;
        let design = session.design();
        let Some(view) = docs.get_mut(idx) else { return };
        let colors = &theme.sch;
        view.viewport = canvas;
        if view.settle > 0 {
            view.settle -= 1;
            if view.settle > 0 && view.pending_focus.is_none() && view.selection.is_empty() {
                view.needs_fit = true;
            }
        }
        let sheet_def = design.sheets.get(&sheet).cloned();
        let sheet_rect = sheet_def.as_ref().map(|s| s.rect()).unwrap_or(hephaestus_core::geometry::Rect::from_center(
            Point::ZERO,
            mil(5000.0),
            mil(4000.0),
        ));
        if view.needs_fit {
            // Altium's Fit All Objects (Ctrl+PgDn); an empty sheet shows the whole sheet.
            let content = design.sheet_objects(sheet).filter_map(|o| cache.bounds.get(&o.id).copied()).reduce(|a, b| a.union(b));
            match content {
                Some(c) => view.camera.fit(canvas, c.inflated(mil(300.0)), 0.04),
                None => view.camera.fit(canvas, sheet_rect, 0.02),
            }
            view.needs_fit = false;
        }
        if std::mem::take(&mut view.fit_document) {
            view.camera.fit(canvas, sheet_rect, 0.02);
        }
        if let Some(area) = view.pending_focus.take() {
            let min = mil(1500.0);
            let grow = ((min - area.width().min(area.height())) / 2).max(mil(200.0));
            view.camera.fit(canvas, area.inflated(grow), 0.05);
        }
        prefs.navigation.handle(ui, &response, &mut view.camera, canvas);

        let grid = prefs.sch_grid;
        let tol = (5.0 / view.camera.scale) as i64;
        let raw_cursor = response.hover_pos().map(|p| view.camera.to_world(canvas, p));
        let wants_hotspot = !matches!(view.tool, Tool::Select | Tool::PlacePart { .. } | Tool::Text { .. });
        let spots = if wants_hotspot { hotspots(design, sheet) } else { Vec::new() };
        let cursor = response.hover_pos().map(|pos| {
            let snapped = hephaestus_canvas::snap_to_grid(view.camera.to_world(canvas, pos), grid, Point::ZERO);
            if wants_hotspot { nearest_hotspot(&view.camera, canvas, pos, 8.0, spots.iter().copied()).unwrap_or(snapped) } else { snapped }
        });
        if response.hovered() {
            view.cursor = cursor;
        }

        let painter = ui.painter_at(canvas);
        let cv = Canvas { painter: &painter, cam: &view.camera, vp: canvas };
        painter.rect_filled(canvas, 0.0, colors.outside);
        if let Some(s) = &sheet_def {
            render::paper(&cv, s, colors);
            if prefs.sch_grid_visible {
                let clip = cv.screen_rect(&sheet_rect).intersect(canvas);
                let gp = painter.with_clip_rect(clip);
                draw_grid(
                    &gp,
                    &view.camera,
                    canvas,
                    clip,
                    grid,
                    Point::ZERO,
                    GridLook { style: GridStyle::Lines, minor: colors.grid, major: colors.grid_major, major_every: 10 },
                );
            }
            render::frame(&cv, s, &design.name, colors);
        }

        // Moving objects, and wires rubber-banding to them, are drawn at their preview position.
        let moving: BTreeSet<ObjectId> = match &view.drag {
            Some(Drag::Move { offset, drag_wires, .. }) => {
                let mut ids: BTreeSet<ObjectId> = view.selection.iter().copied().collect();
                if *drag_wires {
                    ids.extend(move_preview(design, sheet, &view.selection, *offset).1.iter().map(|w| w.id));
                }
                ids
            }
            _ => BTreeSet::new(),
        };
        let highlight = view.highlight_net.clone();
        let look_of = |o: &SchObject| -> Look {
            let in_net = highlight.as_ref().map(|h| cache.netlist.net_of_object(o.id).is_some_and(|n| &n.name == h));
            let symbol_in_net = highlight.as_ref().is_some_and(|h| {
                matches!(o.kind, SchKind::Symbol(_))
                    && cache.netlist.nets.iter().any(|n| &n.name == h && n.nodes.iter().any(|x| x.symbol == o.id))
            });
            Look {
                selected: view.selection.contains(&o.id),
                hovered: view.hover == Some(o.id),
                dim: in_net == Some(false) && !symbol_in_net,
                highlight: in_net == Some(true),
            }
        };
        let world_view = view.camera.visible(canvas).inflated(mil(200.0));
        let objects: Vec<&SchObject> = design
            .sheet_objects(sheet)
            .filter(|o| !moving.contains(&o.id) && cache.bounds.get(&o.id).is_none_or(|b| b.intersects(&world_view)))
            .collect();
        // Wires under symbols, then everything else.
        for o in objects.iter().filter(|o| matches!(o.kind, SchKind::Wire(_) | SchKind::Bus(_))) {
            render::object(&cv, design, o, colors, look_of(o));
        }
        for o in objects.iter().filter(|o| !matches!(o.kind, SchKind::Wire(_) | SchKind::Bus(_))) {
            render::object(&cv, design, o, colors, look_of(o));
        }
        if let Some(points) = cache.netlist.junctions.get(&sheet) {
            for p in points {
                painter.circle_filled(cv.pos(*p), cv.len(mil(20.0)).max(2.0), colors.junction);
            }
        }
        connection_marks(&cv, design, cache, sheet, &moving, colors);
        for o in &objects {
            let l = look_of(o);
            if let Some(b) = cache.bounds.get(&o.id) {
                render::decorate(&cv, o, b, colors, l);
            }
        }

        // ---- interaction ----
        let (shift, ctrl, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.ctrl || i.modifiers.command, i.modifiers.alt));
        let press_world = ui.input(|i| i.pointer.press_origin()).map(|p| view.camera.to_world(canvas, p));
        let keys = |k: egui::Key| response.hovered() && ui.input(|i| i.key_pressed(k));
        let escape = keys(egui::Key::Escape);
        let backspace = keys(egui::Key::Backspace);
        let enter = keys(egui::Key::Enter);

        if response.secondary_clicked() && !matches!(view.tool, Tool::Select) {
            if view.tool.in_progress() {
                if let Tool::Wire { points, .. } = &view.tool
                    && points.len() >= 2
                {
                    commands.push(Command::AddWire { sheet: Some(sheet), points: drag::simplify(points) });
                }
                view.tool.cancel_gesture();
            } else {
                view.tool = Tool::Select;
            }
        }

        let mut tool = std::mem::replace(&mut view.tool, Tool::Select);
        let mut exit_tool = false;
        match &mut tool {
            Tool::Select => {
                view.hover = raw_cursor.and_then(|c| hit(design, cache, sheet, c, tol));
                if escape {
                    if view.highlight_net.is_some() {
                        view.highlight_net = None;
                    } else {
                        view.selection.clear();
                    }
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    let start = press_world.or(raw_cursor);
                    let target = start.and_then(|s| hit(design, cache, sheet, s, tol));
                    match target {
                        Some(t) => {
                            if !view.selection.contains(&t) {
                                view.selection = vec![t];
                                *cross_probe_from = Some(idx);
                            }
                            let origin = start.map(|s| s.snapped(grid)).unwrap_or_default();
                            view.drag = Some(Drag::Move { origin, offset: Point::ZERO, drag_wires: !ctrl });
                        }
                        None => {
                            let p0 = ui.input(|i| i.pointer.press_origin()).unwrap_or(Pos2::ZERO);
                            view.drag = Some(Drag::Rubber(SelectionBox { start: p0, end: p0 }));
                        }
                    }
                }
                if let Some(drag) = &mut view.drag {
                    match drag {
                        Drag::Move { origin, offset, .. } => {
                            if let Some(c) = raw_cursor {
                                *offset = c.snapped(grid) - *origin;
                            }
                        }
                        Drag::Rubber(b) => {
                            if let Some(p) = response.interact_pointer_pos().or(response.hover_pos()) {
                                b.end = p;
                            }
                        }
                    }
                }
                if response.drag_stopped() {
                    match view.drag.take() {
                        Some(Drag::Move { offset, drag_wires, .. }) if offset != Point::ZERO => {
                            commands.push(Command::MoveObjects { ids: view.selection.clone(), dx: offset.x, dy: offset.y, drag_wires });
                        }
                        Some(Drag::Rubber(b)) => {
                            let picked: Vec<ObjectId> = design
                                .sheet_objects(sheet)
                                .filter(|o| cache.bounds.get(&o.id).is_some_and(|bb| b.selects(&view.camera, canvas, bb)))
                                .map(|o| o.id)
                                .collect();
                            if shift {
                                for id in picked {
                                    if !view.selection.contains(&id) {
                                        view.selection.push(id);
                                    }
                                }
                            } else {
                                view.selection = picked;
                            }
                            *cross_probe_from = Some(idx);
                        }
                        _ => {}
                    }
                }
                if response.clicked() {
                    match (view.hover, alt) {
                        (Some(h), true) => {
                            view.highlight_net = raw_cursor.and_then(|c| net_of(design, cache, h, c));
                        }
                        (Some(h), false) if shift => {
                            if let Some(i) = view.selection.iter().position(|x| *x == h) {
                                view.selection.remove(i);
                            } else {
                                view.selection.push(h);
                            }
                            *cross_probe_from = Some(idx);
                        }
                        (Some(h), false) => {
                            view.selection = vec![h];
                            *cross_probe_from = Some(idx);
                        }
                        (None, _) => {
                            if !shift {
                                view.selection.clear();
                            }
                            view.highlight_net = None;
                        }
                    }
                }
                if response.double_clicked()
                    && let Some(h) = view.hover
                {
                    view.selection = vec![h];
                    *focus_panel = Some(PanelId::Properties);
                }
                if response.secondary_clicked()
                    && let Some(h) = view.hover
                    && !view.selection.contains(&h)
                {
                    view.selection = vec![h];
                }
            }
            Tool::PlacePart { part, rotation, mirror } => {
                if escape {
                    exit_tool = true;
                } else if let Some(c) = cursor {
                    let lib_symbol =
                        design.library.parts.get(part.as_str()).and_then(|p| design.library.symbols.get(&p.symbol)).or_else(|| {
                            session.libraries().iter().find_map(|l| l.parts.get(part.as_str()).and_then(|p| l.symbols.get(&p.symbol)))
                        });
                    if let Some(sym) = lib_symbol {
                        let t = Design::symbol_transform(&SymbolInstance {
                            component: ObjectId(0),
                            position: c,
                            rotation: *rotation,
                            mirror: *mirror,
                            designator_offset: None,
                            value_offset: None,
                        });
                        render::symbol(&cv, sym, &t, colors, Look::default(), 0.65);
                    }
                    if response.clicked() {
                        commands.push(Command::PlaceComponent {
                            part: part.clone(),
                            sheet: Some(sheet),
                            position: c,
                            rotation: *rotation,
                            mirror: *mirror,
                            designator: None,
                            value: None,
                            footprint: None,
                            parameters: Default::default(),
                        });
                    }
                }
            }
            Tool::Wire { points, mode } => {
                if let Some(c) = cursor {
                    if let Some(&last) = points.last() {
                        let mut preview = points.clone();
                        preview.extend(mode.path(last, c).into_iter().skip(1));
                        cv.polyline(&preview, cv.width(mil(10.0)), colors.wire);
                    }
                    if spots.contains(&c) {
                        hotspot_cross(&painter, cv.pos(c), colors.hotspot);
                    }
                    if response.clicked() {
                        if let Some(&last) = points.last() {
                            points.extend(mode.path(last, c).into_iter().skip(1));
                            let on_wire = design.sheet_objects(sheet).any(|o| matches!(&o.kind, SchKind::Wire(w) if w.points.windows(2).any(|s| point_segment_distance(c, s[0], s[1]) < 1.0)));
                            if (spots.contains(&c) || on_wire) && points.first() != Some(&c) {
                                commands.push(Command::AddWire { sheet: Some(sheet), points: drag::simplify(points) });
                                points.clear();
                            }
                        } else {
                            points.push(c);
                        }
                    }
                }
                if (response.double_clicked() || enter || escape) && points.len() >= 2 {
                    commands.push(Command::AddWire { sheet: Some(sheet), points: drag::simplify(points) });
                    points.clear();
                } else if escape && points.len() < 2 {
                    if points.is_empty() {
                        exit_tool = true;
                    } else {
                        points.clear();
                    }
                }
                if backspace {
                    points.pop();
                }
            }
            Tool::NetLabel { text, rotation } => {
                if let Some(c) = cursor {
                    let ghost = SchObject {
                        id: ObjectId(0),
                        sheet,
                        kind: SchKind::NetLabel(NetLabel { position: c, text: text.clone(), rotation: *rotation }),
                    };
                    render::object(&cv, design, &ghost, colors, Look::default());
                    if response.clicked() {
                        commands.push(Command::AddNetLabel { sheet: Some(sheet), position: c, text: text.clone(), rotation: *rotation });
                        *text = increment(text);
                    }
                }
                if escape {
                    exit_tool = true;
                }
            }
            Tool::Power { net, style, rotation } => {
                if let Some(c) = cursor {
                    let ghost = SchObject {
                        id: ObjectId(0),
                        sheet,
                        kind: SchKind::PowerPort(PowerPort { position: c, net: net.clone(), style: *style, rotation: *rotation }),
                    };
                    render::object(&cv, design, &ghost, colors, Look::default());
                    if response.clicked() {
                        commands.push(Command::AddPowerPort {
                            sheet: Some(sheet),
                            position: c,
                            net: net.clone(),
                            style: Some(*style),
                            rotation: Some(*rotation),
                        });
                    }
                }
                if escape {
                    exit_tool = true;
                }
            }
            Tool::Port { name, direction, rotation } => {
                if let Some(c) = cursor {
                    let ghost = SchObject {
                        id: ObjectId(0),
                        sheet,
                        kind: SchKind::Port(Port {
                            position: c,
                            name: name.clone(),
                            direction: *direction,
                            rotation: *rotation,
                            width: mil(500.0),
                        }),
                    };
                    render::object(&cv, design, &ghost, colors, Look::default());
                    if response.clicked() {
                        commands.push(Command::AddPort {
                            sheet: Some(sheet),
                            position: c,
                            name: name.clone(),
                            direction: *direction,
                            rotation: *rotation,
                        });
                        *name = increment(name);
                    }
                }
                if escape {
                    exit_tool = true;
                }
            }
            Tool::Junction | Tool::NoErc => {
                let junction = matches!(tool, Tool::Junction);
                if let Some(c) = cursor {
                    let kind =
                        if junction { SchKind::Junction(Junction { position: c }) } else { SchKind::NoConnect(NoConnect { position: c }) };
                    render::object(&cv, design, &SchObject { id: ObjectId(0), sheet, kind }, colors, Look::default());
                    if response.clicked() {
                        commands.push(if junction {
                            Command::AddJunction { sheet: Some(sheet), position: c }
                        } else {
                            Command::AddNoConnect { sheet: Some(sheet), position: c }
                        });
                    }
                }
                if escape {
                    exit_tool = true;
                }
            }
            Tool::Text { text } => {
                if let Some(c) = cursor {
                    let ghost = SchObject {
                        id: ObjectId(0),
                        sheet,
                        kind: SchKind::Text(TextNote { position: c, text: text.clone(), size: mil(100.0), rotation: 0.0 }),
                    };
                    render::object(&cv, design, &ghost, colors, Look::default());
                    if response.clicked() {
                        commands.push(Command::AddText { sheet: Some(sheet), position: c, text: text.clone(), size: Some(mil(100.0)) });
                    }
                }
                if escape {
                    exit_tool = true;
                }
            }
            _ => exit_tool = true,
        }
        if exit_tool {
            tool = Tool::Select;
        }
        view.tool = tool;
        let _ = components;

        // Move preview: the selection at its new place with attached wires following it.
        if let Some(Drag::Move { offset, drag_wires, .. }) = &view.drag {
            let (ghosts, followers) = move_preview(design, sheet, &view.selection, *offset);
            for w in followers.iter().filter(|_| *drag_wires) {
                render::object(&cv, design, w, colors, Look::default());
            }
            let fixed: BTreeSet<Point> = if *offset == Point::ZERO {
                BTreeSet::new()
            } else {
                design
                    .sheet_objects(sheet)
                    .filter(|o| !moving.contains(&o.id))
                    .flat_map(|o| drag::connection_points(design, o))
                    .map(|(_, p)| p)
                    .collect()
            };
            for ghost in &ghosts {
                render::object(&cv, design, ghost, colors, Look { selected: true, ..Default::default() });
                let b = design.sch_bounds(ghost);
                render::decorate(&cv, ghost, &b, colors, Look { selected: true, ..Default::default() });
                // Red crosses where a moving pin lands on something it will connect to.
                for (_, p) in drag::connection_points(design, ghost) {
                    if fixed.contains(&p) {
                        hotspot_cross(&painter, cv.pos(p), colors.hotspot);
                    }
                }
            }
        }
        // What the pointer is over, electrically.
        if let (Tool::Select, None, Some(h), Some(c)) = (&view.tool, &view.drag, view.hover, raw_cursor)
            && let Some(text) = hover_net(design, cache, h, c, tol * 3)
            && let Some(pos) = response.hover_pos()
        {
            net_tip(&painter, pos, &text, theme);
        }
        if let Some(Drag::Rubber(b)) = &view.drag {
            b.paint(&painter, egui::Color32::from_rgb(40, 110, 220), egui::Color32::from_rgb(20, 160, 60));
        }
        if let (Some(c), true) = (cursor, response.hovered()) {
            paint_cursor(&painter, cv.pos(c), 9.0, egui::Color32::from_rgb(40, 40, 40));
        }

        // Context menu.
        let selection_empty = view.selection.is_empty();
        response.context_menu(|ui| {
            act(ui, &mut actions, "Cut", Action::Cut, !selection_empty);
            act(ui, &mut actions, "Copy", Action::Copy, !selection_empty);
            act(ui, &mut actions, "Paste", Action::Paste, true);
            act(ui, &mut actions, "Delete", Action::Delete, !selection_empty);
            ui.separator();
            act(ui, &mut actions, "Rotate Selection", Action::Rotate, !selection_empty);
            act(ui, &mut actions, "Mirror Selection", Action::MirrorX, !selection_empty);
            ui.menu_button("Align", |ui| {
                for (label, mode) in [
                    ("Align Left", hephaestus_commands::AlignMode::Left),
                    ("Align Right", hephaestus_commands::AlignMode::Right),
                    ("Align Top", hephaestus_commands::AlignMode::Top),
                    ("Align Bottom", hephaestus_commands::AlignMode::Bottom),
                ] {
                    if ui.button(label).clicked() {
                        actions.push(Action::Align(mode));
                        ui.close();
                    }
                }
            });
            ui.separator();
            ui.menu_button("Place", |ui| {
                for (label, kind) in [
                    ("Part", crate::actions::SchToolKind::Part),
                    ("Wire", crate::actions::SchToolKind::Wire),
                    ("Net Label", crate::actions::SchToolKind::NetLabel),
                    ("Power Port VCC", crate::actions::SchToolKind::Vcc),
                    ("Power Port GND", crate::actions::SchToolKind::Gnd),
                    ("Port", crate::actions::SchToolKind::Port),
                    ("No ERC", crate::actions::SchToolKind::NoErc),
                ] {
                    if ui.button(label).clicked() {
                        actions.push(Action::SchTool(kind));
                        ui.close();
                    }
                }
            });
            act(ui, &mut actions, "Cross Probe to PCB", Action::CrossProbe, !selection_empty);
            ui.separator();
            act(ui, &mut actions, "Compile", Action::Compile, true);
            act(ui, &mut actions, "Fit Document", Action::Fit, true);
        });
    }
    for c in commands {
        app.run(c);
    }
    for a in actions {
        app.perform(a);
    }
}

/// The selection translated by `offset`, and the other wires reshaped to stay attached to it.
fn move_preview(design: &Design, sheet: ObjectId, selection: &[ObjectId], offset: Point) -> (Vec<SchObject>, Vec<SchObject>) {
    let before: Vec<SchObject> = selection.iter().filter_map(|id| design.schematic.get(id).cloned()).collect();
    let after: Vec<SchObject> = before
        .iter()
        .map(|o| {
            let mut g = o.clone();
            g.translate(offset);
            g
        })
        .collect();
    if offset == Point::ZERO {
        return (after, Vec::new());
    }
    let moved: BTreeSet<ObjectId> = selection.iter().copied().collect();
    let moves = drag::point_moves(design, &before, &after);
    let followers = drag::follow_wires(design, sheet, &moves, &moved);
    (after, followers)
}

/// Altium's red cross on an electrical hot spot.
fn hotspot_cross(painter: &egui::Painter, s: Pos2, color: egui::Color32) {
    let stroke = Stroke::new(2.0, color);
    painter.line_segment([s + egui::vec2(-6.0, -6.0), s + egui::vec2(6.0, 6.0)], stroke);
    painter.line_segment([s + egui::vec2(-6.0, 6.0), s + egui::vec2(6.0, -6.0)], stroke);
}

/// Connection feedback: open red circles on pins that touch nothing, red squares on wire ends
/// that touch nothing, and red dots on labels and ports that are not attached to a wire.
fn connection_marks(
    cv: &Canvas,
    design: &Design,
    cache: &crate::cache::Cache,
    sheet: ObjectId,
    hidden: &BTreeSet<ObjectId>,
    colors: &crate::theme::SchColors,
) {
    let r = cv.len(mil(14.0)).clamp(2.5, 7.0);
    let stroke = Stroke::new(1.5, colors.unconnected);
    for n in cache.netlist.unconnected_pins.iter().filter(|n| n.sheet == sheet && !hidden.contains(&n.symbol)) {
        cv.painter.circle_stroke(cv.pos(n.position), r, stroke);
    }
    for (_, _, p) in cache.netlist.dangling_wire_ends.iter().filter(|(s, w, _)| *s == sheet && !hidden.contains(w)) {
        cv.painter.rect_stroke(Rect::from_center_size(cv.pos(*p), egui::vec2(2.0 * r, 2.0 * r)), 0.0, stroke, egui::StrokeKind::Middle);
    }
    for id in cache.netlist.floating_objects.iter().filter(|id| !hidden.contains(id)) {
        if let Some(o) = design.schematic.get(id).filter(|o| o.sheet == sheet) {
            cv.painter.circle_filled(cv.pos(o.anchor()), r * 0.8, colors.unconnected);
        }
    }
}

/// "Net VCC · 5 pins" for a wire, label or the pin nearest the pointer; pins on no net say so.
fn hover_net(design: &Design, cache: &crate::cache::Cache, id: ObjectId, at: Point, reach: i64) -> Option<String> {
    let o = design.schematic.get(&id)?;
    let net = if matches!(o.kind, SchKind::Symbol(_)) {
        let pins = design.pins(o);
        let pin = pins
            .iter()
            .filter(|p| point_segment_distance(at, p.position, p.body) <= reach as f64)
            .min_by(|a, b| point_segment_distance(at, a.position, a.body).total_cmp(&point_segment_distance(at, b.position, b.body)))?;
        match cache.netlist.net_of_pin(pin.component, &pin.number) {
            Some(n) => n,
            None => return Some(format!("Pin {} {} · not connected", pin.number, pin.name)),
        }
    } else {
        match cache.netlist.net_of_object(id) {
            Some(n) => n,
            None if matches!(o.kind, SchKind::Wire(_) | SchKind::NetLabel(_) | SchKind::PowerPort(_) | SchKind::Port(_)) => {
                return Some("Not connected to any pin".into());
            }
            None => return None,
        }
    };
    let pins = net.nodes.len();
    Some(format!("Net {} · {} pin{}", net.name, pins, if pins == 1 { "" } else { "s" }))
}

fn net_tip(painter: &egui::Painter, at: Pos2, text: &str, theme: &crate::theme::Theme) {
    let galley = painter.layout_no_wrap(text.to_string(), crate::theme::Theme::font(11.5), theme.text_strong);
    let rect = Rect::from_min_size(at + egui::vec2(16.0, 18.0), galley.size() + egui::vec2(10.0, 6.0));
    painter.rect(rect, 2.0, theme.panel_alt.gamma_multiply(0.95), Stroke::new(1.0, theme.border), egui::StrokeKind::Inside);
    painter.galley(rect.min + egui::vec2(5.0, 3.0), galley, theme.text_strong);
}

fn act(ui: &mut Ui, actions: &mut Vec<Action>, label: &str, a: Action, enabled: bool) {
    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(crate::actions::shortcut(&a))).clicked() {
        actions.push(a);
        ui.close();
    }
}
