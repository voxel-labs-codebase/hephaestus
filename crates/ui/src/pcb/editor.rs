//! The PCB editor: selection and moving, interactive routing, vias, pours, keep-outs, board
//! shape and measurement.

use std::collections::BTreeSet;

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui};
use hephaestus_canvas::{GridLook, SelectionBox, draw_grid, nearest_hotspot, paint_cursor, snap_to_grid};
use hephaestus_commands::Command;
use hephaestus_core::geometry::{Coord, Point, Shape, mil, point_segment_distance};
use hephaestus_core::{Design, Layer, ObjectId, PcbKind, PcbObject};

use crate::actions::{Action, PcbToolKind};
use crate::app::HephaestusApp;
use crate::cache::Cache;
use crate::documents::{DocKey, Drag, RouteSession, Tool};
use crate::draw::Canvas;
use crate::pcb::render::{self, View};
use crate::workspace::PanelId;

/// The PCB object under a point, preferring the active layer, then pads and vias, then tracks.
pub fn hit(design: &Design, cache: &Cache, p: Point, tol: Coord, active: Layer) -> Option<ObjectId> {
    let mut best: Option<(i64, ObjectId)> = None;
    for id in cache.pcb_index.query_point(p, tol) {
        let Some(o) = design.pcb.get(id) else { continue };
        let score = match &o.kind {
            PcbKind::Via(v) => {
                if v.position.distance(p) > (v.diameter / 2 + tol) as f64 {
                    continue;
                }
                0
            }
            PcbKind::Track(t) => {
                if point_segment_distance(p, t.start, t.end) > (t.width / 2 + tol) as f64 {
                    continue;
                }
                if t.layer == active { 1 } else { 5 }
            }
            PcbKind::Footprint(_) => {
                let on_pad = cache.pads.iter().any(|pad| pad.footprint == *id && pad.shape.distance_to(p) <= tol as f64);
                let in_body = cache.bounds.get(id).is_some_and(|b| b.contains(p));
                if on_pad {
                    2
                } else if in_body {
                    6
                } else {
                    continue;
                }
            }
            PcbKind::Zone(z) => {
                if !hephaestus_core::geometry::polygon_contains(&z.outline, p) {
                    continue;
                }
                if z.layer == active { 8 } else { 9 }
            }
            PcbKind::Keepout(k) => {
                if !hephaestus_core::geometry::polygon_contains(&k.outline, p) {
                    continue;
                }
                9
            }
            PcbKind::Text(_) | PcbKind::Line(_) => 7,
        };
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, *id));
        }
    }
    best.map(|(_, id)| id)
}

/// The net of copper at a point on `layer` (pads first).
pub fn net_at(design: &Design, cache: &Cache, p: Point, layer: Layer) -> Option<String> {
    for pad in &cache.pads {
        if pad.layers.contains(&layer) && pad.net.is_some() && pad.shape.contains(p) {
            return pad.net.clone();
        }
    }
    for o in design.pcb.values() {
        match &o.kind {
            PcbKind::Track(t) if t.layer == layer && t.net.is_some() && Shape::capsule(t.start, t.end, t.width).contains(p) => {
                return t.net.clone();
            }
            PcbKind::Via(v) if v.net.is_some() && v.position.distance(p) <= v.diameter as f64 / 2.0 => return v.net.clone(),
            _ => {}
        }
    }
    None
}

/// Electrical hot spots on a layer: pad centres, track ends, vias.
fn hotspots(design: &Design, cache: &Cache, layer: Layer) -> Vec<Point> {
    let mut v: Vec<Point> = cache.pads.iter().filter(|p| p.layers.contains(&layer)).map(|p| p.center).collect();
    for o in design.pcb.values() {
        match &o.kind {
            PcbKind::Track(t) if t.layer == layer => {
                v.push(t.start);
                v.push(t.end);
            }
            PcbKind::Via(via) => v.push(via.position),
            _ => {}
        }
    }
    v
}

/// Copper of other nets that a proposed track would violate clearance with.
fn conflicts(design: &Design, cache: &Cache, pts: &[Point], width: Coord, layer: Layer, net: Option<&str>) -> Vec<Shape> {
    let rules = &design.board.rules;
    let segs: Vec<Shape> = pts.windows(2).filter_map(|s| Some(Shape::capsule(*s.first()?, *s.get(1)?, width))).collect();
    let mut out = Vec::new();
    let mut check = |shape: Shape, other: Option<&str>| {
        if other.is_some() && other == net {
            return;
        }
        let need = rules.clearance_between(net, other) as f64;
        if segs.iter().any(|s| s.gap(&shape) < need) {
            out.push(shape);
        }
    };
    for pad in cache.pads.iter().filter(|p| p.layers.contains(&layer)) {
        check(pad.shape.clone(), pad.net.as_deref());
    }
    for o in design.pcb.values() {
        match &o.kind {
            PcbKind::Track(t) if t.layer == layer => check(Shape::capsule(t.start, t.end, t.width), t.net.as_deref()),
            PcbKind::Via(v) => check(Shape::circle(v.position, v.diameter), v.net.as_deref()),
            _ => {}
        }
    }
    out
}

fn finish_route(design: &Design, s: &RouteSession) -> Option<Command> {
    let rules = &design.board.rules;
    let mut cmds = Vec::new();
    for (layer, pts) in &s.runs {
        if pts.len() >= 2 {
            cmds.push(Command::AddTracks { points: pts.clone(), layer: *layer, width: Some(s.width), net: s.net.clone() });
        }
    }
    for v in &s.vias {
        cmds.push(Command::AddVia {
            position: *v,
            diameter: Some(rules.preferred_via_diameter),
            drill: Some(rules.preferred_via_drill),
            net: s.net.clone(),
            from: None,
            to: None,
        });
    }
    (!cmds.is_empty()).then_some(Command::Batch { commands: cmds })
}

pub fn show(app: &mut HephaestusApp, ui: &mut Ui, canvas: Rect) {
    let idx = app.active;
    if app.docs.get(idx).map(|v| v.key) != Some(DocKey::Pcb) {
        return;
    }
    let response = ui.interact(canvas, egui::Id::new("pcb-canvas"), Sense::click_and_drag());
    // The canvas never takes keyboard focus: egui would turn Space (rotate) into a click.
    // Editor keys apply while the pointer is over the canvas.
    let mut actions: Vec<Action> = Vec::new();
    let mut commands: Vec<Command> = Vec::new();
    let mut status: Option<String> = None;
    {
        let HephaestusApp { session, docs, cache, prefs, theme, focus_panel, cross_probe_from, .. } = app;
        let design = session.design();
        let Some(view) = docs.get_mut(idx) else { return };
        let colors = &theme.pcb;
        view.viewport = canvas;
        if view.settle > 0 {
            view.settle -= 1;
            if view.settle > 0 && view.pending_focus.is_none() && view.selection.is_empty() {
                view.needs_fit = true;
            }
        }
        let board =
            design.board.outline_bounds().unwrap_or(hephaestus_core::geometry::Rect::from_center(Point::ZERO, mil(1000.0), mil(1000.0)));
        if view.needs_fit {
            let all = design.pcb.keys().filter_map(|id| cache.bounds.get(id)).fold(board, |a, b| a.union(*b));
            view.camera.fit(canvas, all, 0.05);
            view.needs_fit = false;
        }
        if let Some(area) = view.pending_focus.take() {
            let grow = ((mil(600.0) - area.width().min(area.height())) / 2).max(mil(60.0));
            view.camera.fit(canvas, area.inflated(grow), 0.08);
        }
        prefs.navigation.handle(ui, &response, &mut view.camera, canvas);
        if !design.board.stack.contains(view.active_layer) {
            view.active_layer = Layer::TopCopper;
        }
        let layer = view.active_layer;
        let grid = prefs.pcb_grid;
        let origin = design.board.origin;
        let tol = (4.0 / view.camera.scale) as i64;
        let raw = response.hover_pos().map(|p| view.camera.to_world(canvas, p));
        let electrical = matches!(view.tool, Tool::Route { .. } | Tool::Via | Tool::Measure { .. });
        let spots = if electrical { hotspots(design, cache, layer) } else { Vec::new() };
        let cursor = response.hover_pos().map(|pos| {
            let snapped = snap_to_grid(view.camera.to_world(canvas, pos), grid, origin);
            if electrical { nearest_hotspot(&view.camera, canvas, pos, 8.0, spots.iter().copied()).unwrap_or(snapped) } else { snapped }
        });
        if response.hovered() {
            view.cursor = cursor;
        }

        let painter = ui.painter_at(canvas);
        let cv = Canvas { painter: &painter, cam: &view.camera, vp: canvas };
        painter.rect_filled(canvas, 0.0, colors.outside);
        render::board_fill(&cv, design, colors);
        draw_grid(
            &painter,
            &view.camera,
            canvas,
            canvas,
            grid,
            origin,
            GridLook { style: prefs.pcb_grid_style, minor: colors.grid, major: colors.grid_major, major_every: 10 },
        );
        let hidden: BTreeSet<ObjectId> = match &view.drag {
            Some(Drag::Move { offset, drag_wires, .. }) => {
                let mut ids: BTreeSet<ObjectId> = view.selection.iter().copied().collect();
                if *drag_wires {
                    ids.extend(track_followers(design, &view.selection, *offset).iter().map(|t| t.id));
                }
                ids
            }
            _ => BTreeSet::new(),
        };
        let v = View {
            design,
            cache,
            prefs,
            colors,
            active: layer,
            single_layer: view.single_layer,
            selection: &view.selection,
            hover: view.hover,
            highlight_net: view.highlight_net.as_deref(),
            hidden: &hidden,
        };
        render::board(&cv, &v);
        render::decorate(&cv, &v);

        let (shift, ctrl, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.ctrl || i.modifiers.command, i.modifiers.alt));
        let escape = response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape));
        let backspace = response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Backspace));
        let enter = response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let press_world = ui.input(|i| i.pointer.press_origin()).map(|p| view.camera.to_world(canvas, p));

        if response.secondary_clicked() && !matches!(view.tool, Tool::Select) {
            match &mut view.tool {
                Tool::Route { session: Some(s), .. } => {
                    if let Some(c) = finish_route(design, s) {
                        commands.push(c);
                    }
                    view.tool.cancel_gesture();
                }
                Tool::Track { points, .. } if points.len() >= 2 => {
                    commands.push(Command::AddTracks { points: points.clone(), layer, width: None, net: None });
                    view.tool.cancel_gesture();
                }
                Tool::Zone { points } | Tool::Keepout { points } | Tool::BoardShape { points } if points.len() >= 3 => {
                    let pts = points.clone();
                    commands.push(match &view.tool {
                        Tool::Zone { .. } => Command::AddZone {
                            layer,
                            net: view.highlight_net.clone().or_else(|| pts.first().and_then(|p| net_at(design, cache, *p, layer))),
                            outline: pts,
                            priority: None,
                        },
                        Tool::Keepout { .. } => Command::AddKeepout { layers: vec![layer], outline: pts },
                        _ => Command::SetBoardOutline { outline: pts },
                    });
                    view.tool.cancel_gesture();
                }
                t if t.in_progress() => t.cancel_gesture(),
                _ => view.tool = Tool::Select,
            }
        }

        let mut tool = std::mem::replace(&mut view.tool, Tool::Select);
        let mut exit_tool = false;
        // 0 = polygon pour, 1 = keep-out, 2 = board shape.
        let region_kind = match &tool {
            Tool::Zone { .. } => 0,
            Tool::Keepout { .. } => 1,
            _ => 2,
        };
        match &mut tool {
            Tool::Select => {
                view.hover = raw.and_then(|c| hit(design, cache, c, tol, layer));
                if escape {
                    if view.highlight_net.is_some() { view.highlight_net = None } else { view.selection.clear() }
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    let start = press_world.or(raw);
                    match start.and_then(|s| hit(design, cache, s, tol, layer)) {
                        Some(t) => {
                            if !view.selection.contains(&t) {
                                view.selection = vec![t];
                                *cross_probe_from = Some(idx);
                            }
                            let locked = view.selection.iter().any(|id| design.pcb.get(id).is_some_and(|o| o.is_locked()));
                            if locked {
                                status = Some("Locked components cannot be moved (Edit › Unlock)".into());
                            } else {
                                // Tracks stay on their pads; Ctrl+drag breaks them away like Altium's Move.
                                view.drag = Some(Drag::Move {
                                    origin: start.map(|s| snap_to_grid(s, grid, origin)).unwrap_or_default(),
                                    offset: Point::ZERO,
                                    drag_wires: !ctrl,
                                });
                            }
                        }
                        None => {
                            let p0 = ui.input(|i| i.pointer.press_origin()).unwrap_or(Pos2::ZERO);
                            view.drag = Some(Drag::Rubber(SelectionBox { start: p0, end: p0 }));
                        }
                    }
                }
                match &mut view.drag {
                    Some(Drag::Move { origin: o, offset, .. }) => {
                        if let Some(c) = raw {
                            *offset = snap_to_grid(c, grid, origin) - *o;
                        }
                    }
                    Some(Drag::Rubber(b)) => {
                        if let Some(p) = response.interact_pointer_pos().or(response.hover_pos()) {
                            b.end = p;
                        }
                    }
                    None => {}
                }
                if response.drag_stopped() {
                    match view.drag.take() {
                        Some(Drag::Move { offset, drag_wires, .. }) if offset != Point::ZERO => {
                            commands.push(Command::MoveObjects { ids: view.selection.clone(), dx: offset.x, dy: offset.y, drag_wires });
                        }
                        Some(Drag::Rubber(b)) => {
                            let picked: Vec<ObjectId> = design
                                .pcb
                                .values()
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
                    match view.hover {
                        Some(h) if ctrl || alt => {
                            let c = raw.unwrap_or_default();
                            view.highlight_net = net_at(design, cache, c, layer)
                                .or_else(|| design.pcb.get(&h).and_then(|o| o.kind.net().map(String::from)))
                                .or_else(|| {
                                    cache.pads.iter().find(|p| p.footprint == h && p.shape.contains(c)).and_then(|p| p.net.clone())
                                });
                        }
                        Some(h) if shift => {
                            if let Some(i) = view.selection.iter().position(|x| *x == h) {
                                view.selection.remove(i);
                            } else {
                                view.selection.push(h);
                            }
                            *cross_probe_from = Some(idx);
                        }
                        Some(h) => {
                            view.selection = vec![h];
                            *cross_probe_from = Some(idx);
                        }
                        None => {
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
            Tool::Route { session: route, mode } => {
                let mut finished = false;
                // A layer change mid-route drops a via and continues on the new layer.
                if let Some(s) = route.as_mut()
                    && let Some((run_layer, pts)) = s.runs.last()
                    && *run_layer != layer
                    && let Some(&at) = pts.last()
                {
                    s.vias.push(at);
                    s.runs.push((layer, vec![at]));
                }
                if let Some(c) = cursor {
                    match route.as_mut() {
                        None => {
                            if spots.contains(&c) {
                                painter.circle_stroke(cv.pos(c), 7.0, Stroke::new(1.5, Color32::WHITE));
                            }
                            if response.clicked() {
                                let net = net_at(design, cache, c, layer);
                                let width = design.board.rules.track_width_for(net.as_deref());
                                *route = Some(RouteSession { net: net.clone(), width, runs: vec![(layer, vec![c])], vias: Vec::new() });
                                status = Some(match net {
                                    Some(n) => format!("Routing net {n}"),
                                    None => "Routing without a net (start on a pad to pick up its net)".into(),
                                });
                            }
                        }
                        Some(s) => {
                            let last = s.runs.last().and_then(|(_, p)| p.last().copied()).unwrap_or(c);
                            let path = mode.path(last, c);
                            // Committed part of the route.
                            for (l, pts) in &s.runs {
                                render::route_preview(&cv, pts, s.width, 0, prefs.layer_color(*l), Color32::TRANSPARENT);
                            }
                            for via in &s.vias {
                                painter.circle_filled(
                                    cv.pos(*via),
                                    cv.len(design.board.rules.preferred_via_diameter / 2),
                                    prefs.layer_color(Layer::MultiLayer),
                                );
                            }
                            let clearance = design.board.rules.clearance_between(s.net.as_deref(), None);
                            render::route_preview(&cv, &path, s.width, clearance, prefs.layer_color(layer), colors.clearance);
                            for shape in conflicts(design, cache, &path, s.width, layer, s.net.as_deref()) {
                                if let Some(b) = shape.bounds() {
                                    painter.rect_stroke(
                                        cv.screen_rect(&b),
                                        0.0,
                                        Stroke::new(2.0, colors.violation),
                                        egui::StrokeKind::Middle,
                                    );
                                }
                            }
                            // Ratsnest of the routed net stays visible as guidance.
                            if let Some(n) = &s.net {
                                for r in cache.pcb.unrouted.iter().filter(|r| &r.net == n) {
                                    cv.line(r.from, r.to, 1.5, colors.highlight);
                                }
                            }
                            if response.clicked() {
                                if let Some((_, pts)) = s.runs.last_mut() {
                                    pts.extend(path.iter().skip(1).copied());
                                }
                                let reached = net_at(design, cache, c, layer);
                                let start = s.runs.first().and_then(|(_, p)| p.first().copied());
                                if reached.is_some() && reached == s.net && Some(c) != start {
                                    if let Some(cmd) = finish_route(design, s) {
                                        commands.push(cmd);
                                    }
                                    finished = true;
                                }
                            }
                            if backspace {
                                let pop_run = s.runs.last().is_some_and(|(_, p)| p.len() <= 1) && s.runs.len() > 1;
                                if pop_run {
                                    s.runs.pop();
                                    s.vias.pop();
                                } else if let Some((_, p)) = s.runs.last_mut()
                                    && p.len() > 1
                                {
                                    p.pop();
                                }
                            }
                        }
                    }
                }
                if finished {
                    *route = None;
                }
                if (escape || enter || response.double_clicked())
                    && let Some(s) = route.take()
                    && let Some(cmd) = finish_route(design, &s)
                {
                    commands.push(cmd);
                } else if escape && route.is_none() {
                    exit_tool = true;
                }
            }
            Tool::Track { points, mode } => {
                if let Some(c) = cursor {
                    if let Some(&last) = points.last() {
                        let mut pts = points.clone();
                        pts.extend(mode.path(last, c).into_iter().skip(1));
                        render::route_preview(
                            &cv,
                            &pts,
                            design.board.rules.preferred_track_width,
                            0,
                            prefs.layer_color(layer),
                            Color32::TRANSPARENT,
                        );
                    }
                    if response.clicked() {
                        match points.last().copied() {
                            Some(last) => points.extend(mode.path(last, c).into_iter().skip(1)),
                            None => points.push(c),
                        }
                    }
                }
                if (escape || enter || response.double_clicked()) && points.len() >= 2 {
                    commands.push(Command::AddTracks { points: std::mem::take(points), layer, width: None, net: None });
                } else if escape {
                    if points.is_empty() { exit_tool = true } else { points.clear() }
                }
                if backspace {
                    points.pop();
                }
            }
            Tool::Via => {
                if let Some(c) = cursor {
                    let r = &design.board.rules;
                    painter.circle_filled(
                        cv.pos(c),
                        cv.len(r.preferred_via_diameter / 2),
                        prefs.layer_color(Layer::MultiLayer).gamma_multiply(0.7),
                    );
                    painter.circle_filled(cv.pos(c), cv.len(r.preferred_via_drill / 2), colors.hole);
                    if response.clicked() {
                        commands.push(Command::AddVia { position: c, diameter: None, drill: None, net: None, from: None, to: None });
                    }
                }
                if escape {
                    exit_tool = true;
                }
            }
            Tool::Zone { points } | Tool::Keepout { points } | Tool::BoardShape { points } => {
                let color = match region_kind {
                    0 => prefs.layer_color(layer),
                    1 => prefs.layer_color(Layer::KeepOut),
                    _ => colors.outline,
                };
                if let Some(c) = cursor {
                    let mut pts = points.clone();
                    pts.push(c);
                    if pts.len() >= 2 {
                        cv.polygon(&pts, color.gamma_multiply(0.15), Stroke::new(1.5, color));
                    }
                    if response.clicked() {
                        points.push(c);
                    }
                }
                let done = (enter || response.double_clicked()) && points.len() >= 3;
                if done {
                    let pts = std::mem::take(points);
                    commands.push(match region_kind {
                        0 => Command::AddZone {
                            layer,
                            net: view.highlight_net.clone().or_else(|| pts.first().and_then(|p| net_at(design, cache, *p, layer))),
                            outline: pts,
                            priority: None,
                        },
                        1 => Command::AddKeepout { layers: vec![layer], outline: pts },
                        _ => Command::SetBoardOutline { outline: pts },
                    });
                    if region_kind == 2 {
                        exit_tool = true;
                    }
                } else if escape {
                    if points.is_empty() { exit_tool = true } else { points.clear() }
                } else if backspace {
                    points.pop();
                }
            }
            Tool::Measure { a, b } => {
                if response.clicked()
                    && let Some(c) = cursor
                {
                    if a.is_none() || b.is_some() {
                        *a = Some(c);
                        *b = None;
                    } else {
                        *b = Some(c);
                    }
                }
                if let (Some(pa), Some(pb)) = (*a, b.or(cursor)) {
                    cv.line(pa, pb, 1.5, Color32::from_rgb(255, 230, 0));
                    let d = pa.distance(pb).round() as Coord;
                    let unit = prefs.pcb_unit;
                    let f = |v: Coord| hephaestus_core::geometry::format_coord(v, unit);
                    let text = format!("Distance {}  ΔX {}  ΔY {}", f(d), f((pb.x - pa.x).abs()), f((pb.y - pa.y).abs()));
                    painter.text(
                        cv.pos(pb) + egui::vec2(12.0, -12.0),
                        Align2::LEFT_BOTTOM,
                        &text,
                        egui::FontId::proportional(12.0),
                        Color32::from_rgb(255, 230, 0),
                    );
                    if b.is_some() {
                        status = Some(text);
                    }
                }
                if escape {
                    if a.is_some() {
                        *a = None;
                        *b = None
                    } else {
                        exit_tool = true
                    }
                }
            }
            _ => exit_tool = true,
        }
        if exit_tool {
            tool = Tool::Select;
        }
        view.tool = tool;

        if let Some(Drag::Move { offset, drag_wires, .. }) = &view.drag {
            if *drag_wires {
                for track in track_followers(design, &view.selection, *offset) {
                    render::ghost_object(&cv, design, &track, prefs);
                }
            }
            for id in &view.selection {
                if let Some(o) = design.pcb.get(id) {
                    let mut ghost = o.clone();
                    ghost.translate(*offset);
                    render::ghost_object(&cv, design, &ghost, prefs);
                }
            }
        }
        if let Some(Drag::Rubber(b)) = &view.drag {
            b.paint(&painter, Color32::from_rgb(90, 150, 255), Color32::from_rgb(80, 220, 120));
        }
        if let (Some(c), true) = (cursor, response.hovered()) {
            paint_cursor(&painter, cv.pos(c), 10.0, Color32::from_rgb(220, 220, 220));
        }

        let sel_empty = view.selection.is_empty();
        let is_fp = view.selection.iter().any(|id| matches!(design.pcb.get(id).map(|o| &o.kind), Some(PcbKind::Footprint(_))));
        response.context_menu(|ui| {
            act(ui, &mut actions, "Interactive Routing", Action::PcbTool(PcbToolKind::Route), true);
            act(ui, &mut actions, "Place Via", Action::PcbTool(PcbToolKind::Via), true);
            ui.separator();
            act(ui, &mut actions, "Cut", Action::Cut, !sel_empty);
            act(ui, &mut actions, "Copy", Action::Copy, !sel_empty);
            act(ui, &mut actions, "Paste", Action::Paste, true);
            act(ui, &mut actions, "Delete", Action::Delete, !sel_empty);
            ui.separator();
            act(ui, &mut actions, "Rotate Selection", Action::Rotate, !sel_empty);
            act(ui, &mut actions, "Flip to Other Side", Action::FlipSide, is_fp);
            ui.add_enabled_ui(!sel_empty, |ui| {
                ui.menu_button("Align", |ui| {
                    for (label, a) in [
                        ("Align Left", Action::Align(hephaestus_commands::AlignMode::Left)),
                        ("Align Right", Action::Align(hephaestus_commands::AlignMode::Right)),
                        ("Align Top", Action::Align(hephaestus_commands::AlignMode::Top)),
                        ("Align Bottom", Action::Align(hephaestus_commands::AlignMode::Bottom)),
                        ("Distribute Horizontally", Action::Distribute(hephaestus_commands::Axis::Horizontal)),
                        ("Distribute Vertically", Action::Distribute(hephaestus_commands::Axis::Vertical)),
                    ] {
                        if ui.button(label).clicked() {
                            actions.push(a);
                            ui.close();
                        }
                    }
                });
            });
            act(ui, &mut actions, "Lock", Action::Lock(true), is_fp);
            act(ui, &mut actions, "Unlock", Action::Lock(false), is_fp);
            ui.separator();
            act(ui, &mut actions, "Cross Probe to Schematic", Action::CrossProbe, is_fp);
            act(ui, &mut actions, "Clear Net Highlight", Action::ClearHighlight, true);
            act(ui, &mut actions, "Design Rule Check", Action::RunDrc, true);
            act(ui, &mut actions, "Fit Document", Action::Fit, true);
        });
    }
    if let Some(s) = status {
        app.status = s;
    }
    for c in commands {
        app.run(c);
    }
    for a in actions {
        app.perform(a);
    }
}

fn act(ui: &mut Ui, actions: &mut Vec<Action>, label: &str, a: Action, enabled: bool) {
    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(crate::actions::shortcut(&a))).clicked() {
        actions.push(a);
        ui.close();
    }
}

/// Tracks attached to the pads of the moving footprints, stretched by `offset`.
fn track_followers(design: &Design, selection: &[ObjectId], offset: Point) -> Vec<PcbObject> {
    if offset == Point::ZERO {
        return Vec::new();
    }
    let before: Vec<PcbObject> = selection.iter().filter_map(|id| design.pcb.get(id).cloned()).collect();
    let after: Vec<PcbObject> = before
        .iter()
        .map(|o| {
            let mut g = o.clone();
            g.translate(offset);
            g
        })
        .collect();
    let moved: BTreeSet<ObjectId> = selection.iter().copied().collect();
    hephaestus_core::drag::follow_tracks(design, &hephaestus_core::drag::pad_moves(design, &before, &after), &moved)
}
