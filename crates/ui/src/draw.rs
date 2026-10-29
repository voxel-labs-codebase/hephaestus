//! World-space drawing helpers shared by the schematic and PCB renderers.

use egui::{Align2, Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};
use hephaestus_canvas::Camera;
use hephaestus_core::SymbolGraphic;
use hephaestus_core::geometry::{Coord, Point, Transform};

/// A painter bound to a camera and viewport.
pub struct Canvas<'a> {
    pub painter: &'a Painter,
    pub cam: &'a Camera,
    pub vp: Rect,
}

impl Canvas<'_> {
    pub fn pos(&self, p: Point) -> Pos2 {
        self.cam.to_screen(self.vp, p)
    }

    pub fn len(&self, nm: Coord) -> f32 {
        self.cam.len(nm)
    }

    /// Stroke width for a world width, never thinner than one pixel.
    pub fn width(&self, nm: Coord) -> f32 {
        self.len(nm).max(1.0)
    }

    pub fn line(&self, a: Point, b: Point, width: f32, color: Color32) {
        self.painter.line_segment([self.pos(a), self.pos(b)], Stroke::new(width, color));
    }

    pub fn polyline(&self, pts: &[Point], width: f32, color: Color32) {
        if pts.len() >= 2 {
            self.painter.add(Shape::line(pts.iter().map(|p| self.pos(*p)).collect(), Stroke::new(width, color)));
        }
    }

    pub fn polygon(&self, pts: &[Point], fill: Color32, stroke: Stroke) {
        if pts.len() >= 3 {
            let v: Vec<Pos2> = pts.iter().map(|p| self.pos(*p)).collect();
            if fill != Color32::TRANSPARENT {
                fill_polygon(self.painter, &v, fill);
            }
            if stroke.width > 0.0 {
                self.painter.add(Shape::closed_line(v, stroke));
            }
        }
    }

    pub fn dashed(&self, pts: &[Pos2], color: Color32) {
        self.painter.extend(Shape::dashed_line(pts, Stroke::new(1.0, color), 4.0, 3.0));
    }

    pub fn dashed_rect(&self, r: Rect, color: Color32) {
        self.dashed(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], color);
    }

    pub fn screen_rect(&self, r: &hephaestus_core::geometry::Rect) -> Rect {
        Rect::from_two_pos(self.pos(r.min), self.pos(r.max))
    }

    /// Text of world height `size` at a world point, rotated `degrees` counter-clockwise, with
    /// `anchor` interpreted in the text's own (rotated) frame. Skipped when too small to read.
    pub fn text(&self, at: Point, text: &str, size: Coord, degrees: f64, anchor: Align2, color: Color32) -> Option<Rect> {
        let px = self.len(size);
        if px < 3.5 || text.is_empty() {
            return None;
        }
        let px = (px * 2.0).round() / 2.0;
        let font = egui::FontId::proportional(px.min(400.0));
        let galley = self.painter.layout_no_wrap(text.to_string(), font, color);
        let size = galley.size();
        let (ax, ay) = (anchor.x().to_factor(), anchor.y().to_factor());
        let angle = -(degrees.to_radians() as f32);
        let (s, c) = angle.sin_cos();
        let off = vec2(ax * size.x, ay * size.y);
        let rotated = vec2(off.x * c - off.y * s, off.x * s + off.y * c);
        let pos = self.pos(at) - rotated;
        let rect = Rect::from_min_size(pos, size);
        self.painter.add(Shape::Text(egui::epaint::TextShape::new(pos, galley, color).with_angle(angle)));
        Some(rect)
    }

    pub fn arc(&self, center: Point, radius: Coord, start: f64, end: f64, width: f32, color: Color32) {
        let sweep = (end - start).rem_euclid(360.0);
        let sweep = if sweep == 0.0 { 360.0 } else { sweep };
        let steps = ((sweep / 6.0).ceil() as usize).clamp(4, 120);
        let r = radius as f64;
        let pts: Vec<Point> = (0..=steps)
            .map(|i| {
                let a = (start + sweep * i as f64 / steps as f64).to_radians();
                center + Point::new((r * a.cos()).round() as Coord, (r * a.sin()).round() as Coord)
            })
            .collect();
        self.polyline(&pts, width, color);
    }

    /// Draw library artwork placed with `t`.
    pub fn graphic(&self, g: &SymbolGraphic, t: &Transform, line: Color32, fill: Color32, rect_line: Color32) {
        match g {
            SymbolGraphic::Line { points, width } => {
                let pts: Vec<Point> = points.iter().map(|p| t.apply(*p)).collect();
                self.polyline(&pts, self.width(*width), line);
            }
            SymbolGraphic::Rect { min, max, width, filled } => {
                let c = [*min, Point::new(max.x, min.y), *max, Point::new(min.x, max.y)];
                let pts: Vec<Point> = c.iter().map(|p| t.apply(*p)).collect();
                self.polygon(&pts, if *filled { fill } else { Color32::TRANSPARENT }, Stroke::new(self.width(*width), rect_line));
            }
            SymbolGraphic::Circle { center, radius, width, filled } => {
                let c = self.pos(t.apply(*center));
                let r = self.len(*radius);
                if *filled {
                    self.painter.circle_filled(c, r, fill);
                }
                self.painter.circle_stroke(c, r, Stroke::new(self.width(*width), line));
            }
            SymbolGraphic::Arc { center, radius, start, end, width } => {
                let (a, b) = (t.apply_angle(*start), t.apply_angle(*end));
                let (a, b) = if t.mirror { (b, a) } else { (a, b) };
                self.arc(t.apply(*center), *radius, a, b, self.width(*width), line);
            }
            SymbolGraphic::Polygon { points, width, filled } => {
                let pts: Vec<Point> = points.iter().map(|p| t.apply(*p)).collect();
                self.polygon(&pts, if *filled { line } else { Color32::TRANSPARENT }, Stroke::new(self.width(*width), line));
            }
            SymbolGraphic::Text { position, text, size } => {
                self.text(t.apply(*position), text, *size, 0.0, Align2::LEFT_BOTTOM, line);
            }
        }
    }
}

/// Fill a simple polygon (convex or not) by ear clipping. egui's path fill is only correct for
/// convex shapes, and board outlines and pours are often concave.
pub fn fill_polygon(painter: &Painter, pts: &[Pos2], color: Color32) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let mut idx: Vec<usize> = (0..n).collect();
    if area < 0.0 {
        idx.reverse();
    }
    let cross = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let inside = |p: Pos2, a: Pos2, b: Pos2, c: Pos2| cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0;
    let mut mesh = egui::Mesh::default();
    for p in pts {
        mesh.colored_vertex(*p, color);
    }
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross(a, b, c) <= 0.0 {
                continue;
            }
            if idx.iter().any(|&j| j != ia && j != ib && j != ic && inside(pts[j], a, b, c)) {
                continue;
            }
            mesh.add_triangle(ia as u32, ib as u32, ic as u32);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        mesh.add_triangle(idx[0] as u32, idx[1] as u32, idx[2] as u32);
    }
    painter.add(Shape::mesh(mesh));
}

/// The outline of a pad as a polygon (rounded corners as 6 segments per quarter).
pub fn pad_outline(center: Point, w: Coord, h: Coord, corner: Coord, rotation: f64) -> Vec<Point> {
    let (hw, hh) = (w / 2, h / 2);
    let r = corner.clamp(0, hw.min(hh));
    let mut out = Vec::new();
    for (cx, cy, start) in [(hw - r, hh - r, 0.0), (-(hw - r), hh - r, 90.0), (-(hw - r), -(hh - r), 180.0), (hw - r, -(hh - r), 270.0)] {
        if r == 0 {
            out.push(Point::new(cx, cy));
            continue;
        }
        for k in 0..=6 {
            let a = (start + 15.0 * k as f64).to_radians();
            out.push(Point::new(cx + (r as f64 * a.cos()).round() as Coord, cy + (r as f64 * a.sin()).round() as Coord));
        }
    }
    out.into_iter().map(|p| p.rotated(rotation) + center).collect()
}
