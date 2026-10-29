//! PCB rendering in Altium's style: black board on a grey workspace, layers in their colours with
//! the active layer drawn on top, multi-layer pads and vias above copper, holes, overlay, keep-outs,
//! the board outline and ratsnest lines.

use std::collections::BTreeSet;

use egui::{Align2, Color32, Stroke};
use hephaestus_core::geometry::{Coord, Point, Rect, mil};
use hephaestus_core::{Design, Layer, ObjectId, PadGeometry, PadShape, PcbKind, PcbObject, Side};

use crate::cache::Cache;
use crate::draw::{Canvas, fill_polygon, pad_outline};
use crate::theme::PcbColors;
use crate::workspace::Prefs;

pub struct View<'a> {
    pub design: &'a Design,
    pub cache: &'a Cache,
    pub prefs: &'a Prefs,
    pub colors: &'a PcbColors,
    pub active: Layer,
    pub single_layer: bool,
    pub selection: &'a [ObjectId],
    pub hover: Option<ObjectId>,
    pub highlight_net: Option<&'a str>,
    /// Objects being dragged are drawn by the editor at their preview position.
    pub hidden: &'a BTreeSet<ObjectId>,
}

impl View<'_> {
    fn layer_color(&self, layer: Layer) -> Color32 {
        let c = self.prefs.layer_color(layer);
        if self.single_layer && layer != self.active && layer != Layer::MultiLayer {
            return Color32::from_gray(((c.r() as u16 + c.g() as u16 + c.b() as u16) / 9) as u8 + 20);
        }
        c
    }

    fn net_color(&self, base: Color32, net: Option<&str>) -> Color32 {
        match self.highlight_net {
            Some(h) if net == Some(h) => base,
            Some(_) => base.gamma_multiply(0.25),
            None => base,
        }
    }

    fn visible(&self, layer: Layer) -> bool {
        self.prefs.layer_visible(layer)
    }
}

/// Copper layers bottom-up with the active layer last, as Altium stacks them.
fn draw_order(design: &Design, active: Layer) -> Vec<Layer> {
    let mut layers: Vec<Layer> = design.board.stack.copper_layers();
    layers.reverse();
    layers.retain(|l| *l != active);
    if design.board.stack.contains(active) {
        layers.push(active);
    }
    layers
}

pub fn pad_shape(cv: &Canvas, pad: &PadGeometry, color: Color32, grow: Coord) {
    let (w, h) = (pad.width + 2 * grow, pad.height + 2 * grow);
    match pad.pad_shape {
        PadShape::Round => {
            cv.painter.circle_filled(cv.pos(pad.center), cv.len(w.min(h) / 2), color);
        }
        PadShape::Oval => {
            let pts = pad_outline(pad.center, w, h, w.min(h) / 2, pad.rotation);
            fill_polygon(cv.painter, &pts.iter().map(|p| cv.pos(*p)).collect::<Vec<_>>(), color);
        }
        PadShape::Rect => {
            let pts = pad_outline(pad.center, w, h, 0, pad.rotation);
            fill_polygon(cv.painter, &pts.iter().map(|p| cv.pos(*p)).collect::<Vec<_>>(), color);
        }
        PadShape::RoundRect { ratio } => {
            let r = (pad.width.min(pad.height) as f64 * ratio.clamp(0.0, 0.5)).round() as Coord + grow;
            let pts = pad_outline(pad.center, w, h, r, pad.rotation);
            fill_polygon(cv.painter, &pts.iter().map(|p| cv.pos(*p)).collect::<Vec<_>>(), color);
        }
    }
}

fn track_shape(cv: &Canvas, a: Point, b: Point, width: Coord, color: Color32) {
    let w = cv.len(width).max(1.0);
    cv.painter.line_segment([cv.pos(a), cv.pos(b)], Stroke::new(w, color));
    if w > 2.0 {
        cv.painter.circle_filled(cv.pos(a), w / 2.0, color);
        cv.painter.circle_filled(cv.pos(b), w / 2.0, color);
    }
}

fn outline(cv: &Canvas, pts: &[Point], color: Color32, width: f32) {
    let mut v: Vec<Point> = pts.to_vec();
    if let Some(f) = pts.first() {
        v.push(*f);
    }
    cv.polyline(&v, width, color);
}

/// Draw the whole board.
/// The board area, drawn under the grid so the grid stays visible on the board.
pub fn board_fill(cv: &Canvas, d: &Design, c: &PcbColors) {
    let outline_pts: Vec<egui::Pos2> = d.board.outline.iter().map(|p| cv.pos(*p)).collect();
    fill_polygon(cv.painter, &outline_pts, c.board);
}

pub fn board(cv: &Canvas, v: &View) {
    let d = v.design;
    let c = v.colors;

    let world = cv.cam.visible(cv.vp).inflated(mil(50.0));
    let in_view = |id: &ObjectId| v.cache.bounds.get(id).is_none_or(|b| b.intersects(&world));
    let objects: Vec<&PcbObject> = d.pcb.values().filter(|o| in_view(&o.id) && !v.hidden.contains(&o.id)).collect();
    let pads: Vec<&PadGeometry> =
        v.cache.pads.iter().filter(|p| !v.hidden.contains(&p.footprint) && p.shape.bounds().is_none_or(|b| b.intersects(&world))).collect();

    // Mechanical and keep-out artwork below copper.
    for o in &objects {
        match &o.kind {
            PcbKind::Line(l)
                if !l.layer.is_copper() && !matches!(l.layer, Layer::TopOverlay | Layer::BottomOverlay) && v.visible(l.layer) =>
            {
                cv.line(l.start, l.end, cv.width(l.width), v.layer_color(l.layer));
            }
            PcbKind::Keepout(k) if v.visible(Layer::KeepOut) => {
                let col = v.layer_color(Layer::KeepOut);
                outline(cv, &k.outline, col, 1.5);
                let pts: Vec<egui::Pos2> = k.outline.iter().map(|p| cv.pos(*p)).collect();
                fill_polygon(cv.painter, &pts, col.gamma_multiply(0.12));
            }
            _ => {}
        }
    }
    for fp in objects.iter().filter_map(|o| match &o.kind {
        PcbKind::Footprint(f) => Some(f),
        _ => None,
    }) {
        let Some(def) = d.footprint_def(fp) else { continue };
        let t = Design::footprint_transform(fp);
        for g in &def.graphics {
            let layer = if fp.side == Side::Bottom { g.layer.flipped() } else { g.layer };
            if matches!(layer, Layer::Mechanical(_)) && v.visible(layer) {
                let col = v.layer_color(layer);
                cv.graphic(&g.shape, &t, col, Color32::TRANSPARENT, col);
            }
        }
    }

    // Copper, layer by layer.
    for layer in draw_order(d, v.active) {
        if !v.visible(layer) {
            continue;
        }
        let base = v.layer_color(layer);
        for o in &objects {
            match &o.kind {
                PcbKind::Zone(z) if z.layer == layer => {
                    let col = v.net_color(base, z.net.as_deref());
                    if z.filled.is_empty() {
                        let pts: Vec<egui::Pos2> = z.outline.iter().map(|p| cv.pos(*p)).collect();
                        fill_polygon(cv.painter, &pts, col.gamma_multiply(0.18));
                        cv.polygon(&z.outline, Color32::TRANSPARENT, Stroke::new(1.5, col));
                    } else {
                        for island in &z.filled {
                            let pts: Vec<egui::Pos2> = island.iter().map(|p| cv.pos(*p)).collect();
                            fill_polygon(cv.painter, &pts, col.gamma_multiply(0.8));
                        }
                    }
                }
                PcbKind::Track(t) if t.layer == layer => track_shape(cv, t.start, t.end, t.width, v.net_color(base, t.net.as_deref())),
                _ => {}
            }
        }
        for pad in pads.iter().filter(|p| p.drill.is_none() && p.layers.contains(&layer)) {
            pad_shape(cv, pad, v.net_color(base, pad.net.as_deref()), 0);
        }
    }

    // Multi-layer: through-hole pads and vias, then holes.
    let multi = v.layer_color(Layer::MultiLayer);
    if v.visible(Layer::MultiLayer) {
        for pad in pads.iter().filter(|p| p.drill.is_some() && p.plated) {
            pad_shape(cv, pad, v.net_color(multi, pad.net.as_deref()), 0);
        }
        for o in &objects {
            if let PcbKind::Via(via) = &o.kind {
                cv.painter.circle_filled(cv.pos(via.position), cv.len(via.diameter / 2), v.net_color(multi, via.net.as_deref()));
            }
        }
    }
    for pad in pads.iter().filter(|p| p.drill.is_some()) {
        let r = cv.len(pad.drill.unwrap_or(0) / 2);
        cv.painter.circle_filled(cv.pos(pad.center), r, c.hole);
        if !pad.plated {
            cv.painter.circle_stroke(cv.pos(pad.center), r, Stroke::new(1.0, v.layer_color(Layer::MultiLayer)));
        }
    }
    for o in &objects {
        if let PcbKind::Via(via) = &o.kind {
            cv.painter.circle_filled(cv.pos(via.position), cv.len(via.drill / 2), c.hole);
        }
    }

    // Overlay: footprint artwork, designators, free lines and text.
    for o in &objects {
        match &o.kind {
            PcbKind::Footprint(fp) => {
                let overlay = if fp.side == Side::Top { Layer::TopOverlay } else { Layer::BottomOverlay };
                if !v.visible(overlay) {
                    continue;
                }
                let col = v.layer_color(overlay);
                if let Some(def) = d.footprint_def(fp) {
                    let t = Design::footprint_transform(fp);
                    for g in &def.graphics {
                        let layer = if fp.side == Side::Bottom { g.layer.flipped() } else { g.layer };
                        if layer == overlay {
                            cv.graphic(&g.shape, &t, col, Color32::TRANSPARENT, col);
                        }
                    }
                }
                if let Some(comp) = fp.component.and_then(|id| d.components.get(&id)) {
                    let b = v.cache.bounds.get(&o.id).copied().unwrap_or(Rect::from_center(fp.position, 0, 0));
                    cv.text(Point::new(b.min.x, b.max.y + mil(15.0)), &comp.designator, mil(40.0), 0.0, Align2::LEFT_BOTTOM, col);
                }
            }
            PcbKind::Line(l) if matches!(l.layer, Layer::TopOverlay | Layer::BottomOverlay) && v.visible(l.layer) => {
                cv.line(l.start, l.end, cv.width(l.width), v.layer_color(l.layer));
            }
            PcbKind::Text(t) if v.visible(t.layer) => {
                cv.text(t.position, &t.text, t.height, t.rotation, Align2::LEFT_BOTTOM, v.layer_color(t.layer));
            }
            _ => {}
        }
    }

    // Board shape outline.
    outline(cv, &d.board.outline, c.outline.gamma_multiply(0.9), 1.5);

    // Pad numbers and net names when large enough.
    if v.prefs.show_pad_numbers {
        for pad in &pads {
            let size = pad.width.min(pad.height);
            if cv.len(size) < 14.0 {
                continue;
            }
            let text_size = (size as f64 * 0.35) as Coord;
            let net = if v.prefs.show_net_names { pad.net.as_deref() } else { None };
            match net {
                Some(n) if cv.len(size) > 28.0 => {
                    cv.text(pad.center + Point::new(0, text_size / 2), &pad.number, text_size, 0.0, Align2::CENTER_CENTER, c.pad_text);
                    cv.text(
                        pad.center - Point::new(0, text_size / 2),
                        n,
                        text_size * 7 / 10,
                        0.0,
                        Align2::CENTER_CENTER,
                        c.pad_text.gamma_multiply(0.8),
                    );
                }
                _ => {
                    cv.text(pad.center, &pad.number, text_size, 0.0, Align2::CENTER_CENTER, c.pad_text);
                }
            }
        }
    }

    // Ratsnest (connection lines).
    if v.prefs.show_ratsnest {
        for r in &v.cache.pcb.unrouted {
            let col = v.net_color(c.ratsnest, Some(&r.net));
            cv.line(r.from, r.to, 1.0, col);
        }
    }
}

/// Selection and hover decorations for PCB objects.
pub fn decorate(cv: &Canvas, v: &View) {
    for (id, color, dashed) in v
        .selection
        .iter()
        .map(|id| (*id, v.colors.selection, true))
        .chain(v.hover.filter(|h| !v.selection.contains(h)).map(|h| (h, v.colors.hover, false)))
    {
        let Some(o) = v.design.pcb.get(&id) else { continue };
        match &o.kind {
            PcbKind::Track(t) => {
                let w = cv.len(t.width).max(1.0) + 3.0;
                cv.painter.line_segment([cv.pos(t.start), cv.pos(t.end)], Stroke::new(w, color.gamma_multiply(0.45)));
                if dashed {
                    for p in [t.start, t.end] {
                        cv.painter.rect_filled(egui::Rect::from_center_size(cv.pos(p), egui::vec2(6.0, 6.0)), 0.0, color);
                    }
                }
            }
            PcbKind::Via(via) => {
                cv.painter.circle_stroke(cv.pos(via.position), cv.len(via.diameter / 2) + 2.0, Stroke::new(1.5, color));
            }
            PcbKind::Footprint(_) => {
                for pad in v.cache.pads.iter().filter(|p| p.footprint == id) {
                    pad_shape(cv, pad, color.gamma_multiply(0.35), 0);
                }
                if let Some(b) = v.cache.bounds.get(&id) {
                    let r = cv.screen_rect(b);
                    if dashed {
                        cv.dashed_rect(r, color);
                    } else {
                        cv.painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Middle);
                    }
                }
            }
            _ => {
                if let Some(b) = v.cache.bounds.get(&id) {
                    let r = cv.screen_rect(b);
                    if dashed {
                        cv.dashed_rect(r, color);
                    } else {
                        cv.painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Middle);
                    }
                }
            }
        }
    }
}

/// Draw a footprint outline (pads and overlay) at a temporary position for drag previews.
pub fn ghost_object(cv: &Canvas, design: &Design, o: &PcbObject, prefs: &Prefs) {
    match &o.kind {
        PcbKind::Footprint(_) => {
            for pad in design.pads(o) {
                let layer = pad.layers.first().copied().unwrap_or(Layer::TopCopper);
                let col = if pad.drill.is_some() { prefs.layer_color(Layer::MultiLayer) } else { prefs.layer_color(layer) };
                pad_shape(cv, &pad, col.gamma_multiply(0.7), 0);
            }
            let b = design.pcb_bounds(o);
            cv.dashed_rect(cv.screen_rect(&b), Color32::WHITE);
        }
        PcbKind::Track(t) => track_shape(cv, t.start, t.end, t.width, prefs.layer_color(t.layer).gamma_multiply(0.7)),
        PcbKind::Via(v) => {
            cv.painter.circle_filled(cv.pos(v.position), cv.len(v.diameter / 2), prefs.layer_color(Layer::MultiLayer).gamma_multiply(0.7));
        }
        _ => {
            let b = design.pcb_bounds(o);
            cv.dashed_rect(cv.screen_rect(&b), Color32::WHITE);
        }
    }
}

/// A routing preview: segments with the clearance halo Altium draws around the track.
pub fn route_preview(cv: &Canvas, pts: &[Point], width: Coord, clearance: Coord, color: Color32, halo: Color32) {
    for s in pts.windows(2) {
        if let [a, b] = s {
            let wh = cv.len(width + 2 * clearance).max(1.0);
            cv.painter.line_segment([cv.pos(*a), cv.pos(*b)], Stroke::new(wh, halo.gamma_multiply(0.18)));
            track_shape(cv, *a, *b, width, color);
        }
    }
}
