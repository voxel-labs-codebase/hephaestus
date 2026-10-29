//! Schematic rendering: sheet, border and title block, symbols, pins and wiring objects.

use egui::{Align2, Color32, Stroke};
use hephaestus_core::geometry::{Coord, Point, Rect, Transform, mil};
use hephaestus_core::schematic::{Port, PortDirection, PowerPort, Sheet, TitleBlock};
use hephaestus_core::{Component, Design, PowerStyle, SchKind, SchObject, SymbolDef};

use crate::draw::Canvas;
use crate::theme::SchColors;

const WIRE: Coord = 10 * 25_400;
const TEXT: Coord = 80 * 25_400;

/// Per-object emphasis.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Look {
    pub selected: bool,
    pub hovered: bool,
    /// Drawn faded (another net is highlighted).
    pub dim: bool,
    /// Part of the highlighted net.
    pub highlight: bool,
}

fn fade(c: Color32, look: Look) -> Color32 {
    if look.dim { c.gamma_multiply(0.25) } else { c }
}

/// The blank sheet paper.
pub fn paper(cv: &Canvas, sheet: &Sheet, c: &SchColors) {
    cv.painter.rect_filled(cv.screen_rect(&sheet.rect()), 0.0, c.sheet);
}

/// The zone border and title block, drawn over the grid.
pub fn frame(cv: &Canvas, sheet: &Sheet, design_name: &str, c: &SchColors) {
    let r = sheet.rect();
    let margin = mil(200.0);
    let inner = Rect::from_corners(Point::new(r.min.x + margin, r.min.y + margin), Point::new(r.max.x - margin, r.max.y - margin));
    let thin = 1.0;
    let border = Stroke::new(thin, c.border);
    cv.painter.rect_stroke(cv.screen_rect(&inner), 0.0, border, egui::StrokeKind::Middle);
    if sheet.show_zones {
        cv.painter.rect_stroke(cv.screen_rect(&r), 0.0, border, egui::StrokeKind::Middle);
        // Zone references: numbers along top and bottom, letters down the sides (A at the top),
        // about one zone per 2.6 by 1.9 inches like Altium's templates.
        let cols = ((r.width() as f64 / mil(2600.0) as f64).round() as i64).max(2);
        let rows = ((r.height() as f64 / mil(1900.0) as f64).round() as i64).clamp(2, 26);
        let zone_w = r.width() / cols;
        let zone_h = r.height() / rows;
        for i in 0..cols {
            let x = r.min.x + zone_w * i;
            if i > 0 {
                cv.line(Point::new(x, r.max.y), Point::new(x, inner.max.y), thin, c.border);
                cv.line(Point::new(x, r.min.y), Point::new(x, inner.min.y), thin, c.border);
            }
            let label = (i + 1).to_string();
            let cx = x + zone_w / 2;
            cv.text(Point::new(cx, r.max.y - margin / 2), &label, mil(100.0), 0.0, Align2::CENTER_CENTER, c.border_text);
            cv.text(Point::new(cx, r.min.y + margin / 2), &label, mil(100.0), 0.0, Align2::CENTER_CENTER, c.border_text);
        }
        for j in 0..rows {
            let y = r.max.y - zone_h * j;
            if j > 0 {
                cv.line(Point::new(r.min.x, y), Point::new(inner.min.x, y), thin, c.border);
                cv.line(Point::new(r.max.x, y), Point::new(inner.max.x, y), thin, c.border);
            }
            let label = ((b'A' + j as u8) as char).to_string();
            let cy = y - zone_h / 2;
            cv.text(Point::new(r.min.x + margin / 2, cy), &label, mil(100.0), 0.0, Align2::CENTER_CENTER, c.border_text);
            cv.text(Point::new(r.max.x - margin / 2, cy), &label, mil(100.0), 0.0, Align2::CENTER_CENTER, c.border_text);
        }
    }
    let title = if sheet.title.is_empty() { &sheet.name } else { &sheet.title };
    let file = format!("{design_name} / {}.SchDoc", sheet.name);
    match sheet.title_block {
        TitleBlock::Standard => standard_title_block(cv, &inner, sheet, title, &file, c),
        TitleBlock::Ansi => ansi_title_block(cv, &inner, sheet, title, design_name, c),
        TitleBlock::None => {}
    }
}

/// A title block cell grid: lines are given in mils from the block's bottom-left corner.
struct Block<'a> {
    cv: &'a Canvas<'a>,
    tb: Rect,
    color: Color32,
}

impl Block<'_> {
    fn at(&self, x: f64, y: f64) -> Point {
        Point::new(self.tb.min.x + mil(x), self.tb.min.y + mil(y))
    }

    fn hline(&self, y: f64, x0: f64, x1: f64) {
        self.cv.line(self.at(x0, y), self.at(x1, y), 1.0, self.color);
    }

    fn vline(&self, x: f64, y0: f64, y1: f64) {
        self.cv.line(self.at(x, y0), self.at(x, y1), 1.0, self.color);
    }

    /// A small caption in the top-left corner of the cell whose top-left is (x, y).
    fn caption(&self, x: f64, y: f64, t: &str) {
        self.cv.text(self.at(x + 30.0, y - 30.0), t, mil(55.0), 0.0, Align2::LEFT_TOP, self.color);
    }

    /// A value along the bottom of the cell whose bottom-left is (x, y).
    fn value(&self, x: f64, y: f64, t: &str, size: f64) {
        self.cv.text(self.at(x + 60.0, y + 40.0), t, mil(size), 0.0, Align2::LEFT_BOTTOM, self.color);
    }
}

/// Altium's "Standard" block: 3500 × 1000 mil.
fn standard_title_block(cv: &Canvas, inner: &Rect, sheet: &Sheet, title: &str, file: &str, c: &SchColors) {
    let w = mil(3500.0).min(inner.width());
    let tb = Rect::from_corners(Point::new(inner.max.x - w, inner.min.y), Point::new(inner.max.x, inner.min.y + mil(1000.0)));
    cv.painter.rect_stroke(cv.screen_rect(&tb), 0.0, Stroke::new(1.0, c.border), egui::StrokeKind::Middle);
    let b = Block { cv, tb, color: c.border_text };
    for y in [400.0, 600.0, 800.0] {
        b.hline(y, 0.0, 3500.0);
    }
    b.vline(600.0, 400.0, 800.0);
    b.vline(2700.0, 400.0, 800.0);
    b.vline(2400.0, 0.0, 400.0);
    b.caption(0.0, 1000.0, "Title");
    b.caption(0.0, 800.0, "Size");
    b.caption(600.0, 800.0, "Number");
    b.caption(2700.0, 800.0, "Revision");
    b.caption(0.0, 600.0, "Date:");
    b.caption(0.0, 400.0, "File:");
    b.caption(2400.0, 400.0, "Sheet");
    b.caption(2400.0, 200.0, "Drawn By:");
    b.value(0.0, 800.0, title, 110.0);
    b.value(0.0, 600.0, sheet.size.label(), 90.0);
    b.value(2700.0, 600.0, &sheet.revision, 90.0);
    b.value(600.0, 0.0, file, 70.0);
    b.value(2400.0, 0.0, "Hephaestus", 70.0);
}

/// An ANSI Y14.1-style block: 6200 × 1500 mil with size, FSCM, drawing number and revision.
fn ansi_title_block(cv: &Canvas, inner: &Rect, sheet: &Sheet, title: &str, design_name: &str, c: &SchColors) {
    let w = mil(6200.0).min(inner.width());
    let tb = Rect::from_corners(Point::new(inner.max.x - w, inner.min.y), Point::new(inner.max.x, inner.min.y + mil(1500.0)));
    cv.painter.rect_stroke(cv.screen_rect(&tb), 0.0, Stroke::new(1.0, c.border), egui::StrokeKind::Middle);
    let b = Block { cv, tb, color: c.border_text };
    b.hline(900.0, 0.0, 6200.0);
    b.hline(250.0, 0.0, 6200.0);
    b.vline(700.0, 250.0, 900.0);
    b.vline(2100.0, 250.0, 900.0);
    b.vline(5400.0, 250.0, 900.0);
    b.vline(3100.0, 0.0, 250.0);
    b.caption(0.0, 1500.0, "Title");
    b.value(0.0, 900.0, title, 150.0);
    b.caption(0.0, 900.0, "Size");
    b.value(0.0, 250.0, sheet.size.label(), 150.0);
    b.caption(700.0, 900.0, "FSCM No.");
    b.caption(2100.0, 900.0, "Dwg No.");
    b.value(2100.0, 250.0, design_name, 110.0);
    b.caption(5400.0, 900.0, "Rev");
    b.value(5400.0, 250.0, &sheet.revision, 150.0);
    b.caption(0.0, 250.0, "Scale  1:1");
    b.caption(3100.0, 250.0, &format!("Sheet {}", sheet.order + 1));
}

/// Text placement for designator and comment, kept upright like Altium.
fn label_positions(def: &SymbolDef, inst_pos: Point, rotation: f64, mirror: bool, bounds: Rect) -> ((Point, Align2), (Point, Align2)) {
    let r = rotation.rem_euclid(360.0);
    if r == 90.0 || r == 270.0 {
        let x = bounds.max.x + mil(40.0);
        let cy = bounds.center().y;
        ((Point::new(x, cy + mil(10.0)), Align2::LEFT_BOTTOM), (Point::new(x, cy - mil(10.0)), Align2::LEFT_TOP))
    } else {
        let flip = |p: Point| if mirror { Point::new(-p.x, p.y) } else { p };
        let d = flip(def.designator_position);
        let v = flip(def.value_position);
        ((inst_pos + d, Align2::LEFT_BOTTOM), (inst_pos + v, Align2::LEFT_BOTTOM))
    }
}

/// A symbol body with pins (also used for placement ghosts and library previews).
pub fn symbol(cv: &Canvas, def: &SymbolDef, t: &Transform, c: &SchColors, look: Look, alpha: f32) {
    let k = |col: Color32| fade(col, look).gamma_multiply(alpha);
    for g in &def.graphics {
        cv.graphic(g, t, k(c.graphic), k(c.body_fill), k(c.body_line));
    }
    let pin_text = mil(60.0);
    for pin in &def.pins {
        let a = t.apply(pin.position);
        let body = t.apply(pin.body_end());
        cv.line(a, body, 1.0f32.max(cv.len(mil(4.0))), k(c.pin));
        let angle = t.apply_angle(pin.direction.degrees());
        let horizontal = angle == 0.0 || angle == 180.0;
        let dir = Point::new(mil(1.0), 0).rotated(angle);
        let text_angle = if horizontal { 0.0 } else { 90.0 };
        if pin.show_name && !def.hide_pin_names && !pin.name.is_empty() {
            let inward = Point::new(-dir.x * 50, -dir.y * 50);
            let along = if horizontal { inward.x } else { inward.y };
            let anchor = if along > 0 { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER };
            cv.text(body + inward, &pin.name, pin_text, text_angle, anchor, k(c.pin_text));
        }
        if pin.show_number {
            let mid = Point::new((a.x + body.x) / 2, (a.y + body.y) / 2);
            let offset = if horizontal { Point::new(0, mil(15.0)) } else { Point::new(-mil(15.0), 0) };
            cv.text(mid + offset, &pin.number, pin_text, text_angle, Align2::CENTER_BOTTOM, k(c.pin_text));
        }
    }
}

fn power_port(cv: &Canvas, p: &PowerPort, color: Color32, w: f32) {
    let t = Transform::new(p.position, p.rotation, false);
    let at = |x: f64, y: f64| t.apply(Point::new(mil(x), mil(y)));
    match p.style {
        PowerStyle::Bar => {
            cv.line(at(0.0, 0.0), at(100.0, 0.0), w, color);
            cv.line(at(100.0, -100.0), at(100.0, 100.0), w * 2.0, color);
        }
        PowerStyle::Arrow => {
            cv.line(at(0.0, 0.0), at(100.0, 0.0), w, color);
            cv.polygon(&[at(140.0, 0.0), at(90.0, -40.0), at(90.0, 40.0)], color, Stroke::NONE);
        }
        PowerStyle::Circle => {
            cv.line(at(0.0, 0.0), at(80.0, 0.0), w, color);
            cv.painter.circle_stroke(cv.pos(at(110.0, 0.0)), cv.len(mil(30.0)), Stroke::new(w, color));
        }
        PowerStyle::Wave => {
            cv.line(at(0.0, 0.0), at(80.0, 0.0), w, color);
            let pts: Vec<Point> = (0..=16)
                .map(|i| {
                    let s = i as f64 / 16.0;
                    at(110.0 + 20.0 * (s * std::f64::consts::TAU).sin(), -80.0 + 160.0 * s)
                })
                .collect();
            cv.polyline(&pts, w, color);
        }
        PowerStyle::PowerGround => {
            cv.line(at(0.0, 0.0), at(100.0, 0.0), w, color);
            cv.line(at(100.0, -100.0), at(100.0, 100.0), w, color);
            cv.line(at(130.0, -70.0), at(130.0, 70.0), w, color);
            cv.line(at(160.0, -40.0), at(160.0, 40.0), w, color);
            cv.line(at(190.0, -10.0), at(190.0, 10.0), w, color);
        }
        PowerStyle::SignalGround => {
            cv.line(at(0.0, 0.0), at(100.0, 0.0), w, color);
            cv.polygon(&[at(100.0, -100.0), at(100.0, 100.0), at(200.0, 0.0)], Color32::TRANSPARENT, Stroke::new(w, color));
        }
        PowerStyle::Earth => {
            cv.line(at(0.0, 0.0), at(100.0, 0.0), w, color);
            cv.line(at(100.0, -100.0), at(100.0, 100.0), w, color);
            for y in [-100.0, 0.0, 100.0] {
                cv.line(at(100.0, y), at(160.0, y - 50.0), w, color);
            }
        }
    }
    let ground = p.style.is_ground();
    let dist = if ground { 260.0 } else { 150.0 };
    let label_at = at(dist, 0.0);
    let r = p.rotation.rem_euclid(360.0);
    let anchor = if r == 90.0 {
        Align2::CENTER_BOTTOM
    } else if r == 270.0 {
        Align2::CENTER_TOP
    } else if r == 0.0 {
        Align2::LEFT_CENTER
    } else {
        Align2::RIGHT_CENTER
    };
    cv.text(label_at, &p.net, TEXT, 0.0, anchor, color);
}

fn port(cv: &Canvas, p: &Port, c: &SchColors, look: Look) {
    let t = Transform::new(p.position, p.rotation, false);
    let h = 50.0;
    let w = p.width as f64 / 25_400.0;
    let pts: Vec<Point> = match p.direction {
        PortDirection::Input => vec![(0.0, 0.0), (h, h), (w, h), (w, -h), (h, -h)],
        PortDirection::Output => vec![(0.0, h), (w - h, h), (w, 0.0), (w - h, -h), (0.0, -h)],
        PortDirection::Bidirectional => vec![(0.0, 0.0), (h, h), (w - h, h), (w, 0.0), (w - h, -h), (h, -h)],
        PortDirection::Unspecified => vec![(0.0, h), (w - h, h), (w, 0.0), (w - h, -h), (0.0, -h)],
    }
    .into_iter()
    .map(|(x, y)| t.apply(Point::new(mil(x), mil(y))))
    .collect();
    cv.polygon(&pts, fade(c.port_fill, look), Stroke::new(1.0, fade(c.port_line, look)));
    let center = t.apply(Point::new(p.width / 2, 0));
    let r = p.rotation.rem_euclid(360.0);
    cv.text(center, &p.name, mil(70.0), if r == 90.0 || r == 270.0 { 90.0 } else { 0.0 }, Align2::CENTER_CENTER, fade(c.text, look));
}

/// Draw one schematic object.
pub fn object(cv: &Canvas, design: &Design, o: &SchObject, c: &SchColors, look: Look) {
    let wire_w = cv.width(WIRE);
    let base = |col: Color32| if look.highlight { c.highlight } else { fade(col, look) };
    match &o.kind {
        SchKind::Symbol(inst) => {
            let Some(comp): Option<&Component> = design.components.get(&inst.component) else { return };
            let t = Design::symbol_transform(inst);
            match design.symbol_def_of(comp) {
                Some(def) => {
                    symbol(cv, def, &t, c, look, 1.0);
                    let bounds = design.sch_bounds(o);
                    let ((dp, da), (vp, va)) = label_positions(def, inst.position, inst.rotation, inst.mirror, bounds);
                    let dp = inst.designator_offset.map(|off| inst.position + off).unwrap_or(dp);
                    let vp = inst.value_offset.map(|off| inst.position + off).unwrap_or(vp);
                    cv.text(dp, &comp.designator, TEXT, 0.0, da, fade(c.designator, look));
                    cv.text(vp, &comp.value, TEXT, 0.0, va, fade(c.value, look));
                }
                None => {
                    let r = Rect::from_center(inst.position, mil(200.0), mil(200.0));
                    cv.painter.rect_stroke(cv.screen_rect(&r), 0.0, Stroke::new(1.0, c.error_mark), egui::StrokeKind::Middle);
                    cv.text(
                        inst.position,
                        &format!("{} (missing {})", comp.designator, comp.part),
                        TEXT,
                        0.0,
                        Align2::CENTER_CENTER,
                        c.error_mark,
                    );
                }
            }
        }
        SchKind::Wire(w) => cv.polyline(&w.points, if look.highlight { wire_w * 2.0 } else { wire_w }, base(c.wire)),
        SchKind::Bus(b) => cv.polyline(&b.points, cv.width(mil(30.0)), base(c.bus)),
        SchKind::BusEntry(e) => cv.line(e.start, e.end, wire_w, base(c.bus)),
        SchKind::Junction(j) => {
            cv.painter.circle_filled(cv.pos(j.position), cv.len(mil(20.0)).max(2.0), base(c.junction));
        }
        SchKind::NetLabel(l) => {
            let r = l.rotation.rem_euclid(360.0);
            cv.text(
                l.position + Point::new(0, if r == 0.0 { mil(10.0) } else { 0 }),
                &l.text,
                TEXT,
                r,
                Align2::LEFT_BOTTOM,
                base(c.net_label),
            );
        }
        SchKind::PowerPort(p) => power_port(cv, p, base(c.power), wire_w),
        SchKind::Port(p) => port(cv, p, c, look),
        SchKind::NoConnect(n) => {
            let s = mil(40.0);
            let col = fade(c.no_erc, look);
            cv.line(n.position + Point::new(-s, -s), n.position + Point::new(s, s), wire_w, col);
            cv.line(n.position + Point::new(-s, s), n.position + Point::new(s, -s), wire_w, col);
        }
        SchKind::Text(t) => {
            cv.text(t.position, &t.text, t.size, t.rotation, Align2::LEFT_BOTTOM, fade(c.text, look));
        }
        SchKind::SheetSymbol(s) => {
            let r = Rect::from_corners(s.position, s.position + Point::new(s.width, -s.height));
            cv.painter.rect(
                cv.screen_rect(&r),
                0.0,
                fade(c.port_fill, look),
                Stroke::new(1.0, fade(c.port_line, look)),
                egui::StrokeKind::Middle,
            );
            let name = design.sheets.get(&s.child).map(|x| x.name.clone()).unwrap_or_else(|| "?".into());
            cv.text(s.position + Point::new(0, mil(20.0)), &name, TEXT, 0.0, Align2::LEFT_BOTTOM, fade(c.text, look));
            for e in &s.entries {
                cv.text(s.position + e.offset + Point::new(mil(40.0), 0), &e.name, mil(60.0), 0.0, Align2::LEFT_CENTER, fade(c.text, look));
            }
        }
        SchKind::Graphic(g) => cv.graphic(g, &Transform::default(), fade(c.graphic, look), fade(c.body_fill, look), fade(c.graphic, look)),
    }
}

/// Selection and hover decorations, drawn above all objects.
pub fn decorate(cv: &Canvas, o: &SchObject, bounds: &Rect, c: &SchColors, look: Look) {
    if !(look.selected || look.hovered) {
        return;
    }
    let color = if look.selected { c.selection } else { c.hover };
    match &o.kind {
        SchKind::Wire(w) => {
            cv.polyline(&w.points, cv.width(WIRE) + 2.0, color.gamma_multiply(0.6));
            if look.selected {
                for p in &w.points {
                    let s = cv.pos(*p);
                    cv.painter.rect_filled(egui::Rect::from_center_size(s, egui::vec2(5.0, 5.0)), 0.0, color);
                }
            }
        }
        _ => {
            let r = cv.screen_rect(&bounds.inflated(mil(20.0)));
            if look.selected {
                cv.dashed_rect(r, color);
            } else {
                cv.painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Middle);
            }
        }
    }
}
