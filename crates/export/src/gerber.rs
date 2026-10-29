//! Gerber X2 output (Ucamco Gerber Layer Format Specification, revision 2023.08).
//!
//! Units are millimetres with format 4.6, so one coordinate unit is exactly one nanometre — the
//! design's own unit — and no rounding happens on export.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use hephaestus_core::geometry::{Coord, Point, Transform};
use hephaestus_core::{Design, Layer, PadShape, PcbKind, Side, SymbolGraphic};

use crate::{ExportError, ExportReport, write};

/// Solder mask opening beyond each pad (Altium's default expansion is 4 mil ≈ 0.1016 mm).
pub const MASK_EXPANSION: Coord = 101_600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Aperture {
    Circle(Coord),
    Rect(Coord, Coord),
    Obround(Coord, Coord),
}

/// One Gerber layer being built.
struct Writer {
    apertures: BTreeMap<Aperture, u32>,
    body: String,
    current: Option<u32>,
}

impl Writer {
    fn new() -> Self {
        Self { apertures: BTreeMap::new(), body: String::new(), current: None }
    }

    fn select(&mut self, a: Aperture) {
        let next = 10 + self.apertures.len() as u32;
        let code = *self.apertures.entry(a).or_insert(next);
        if self.current != Some(code) {
            let _ = writeln!(self.body, "D{code}*");
            self.current = Some(code);
        }
    }

    fn xy(p: Point) -> String {
        format!("X{}Y{}", p.x, p.y)
    }

    fn line(&mut self, a: Point, b: Point, width: Coord) {
        self.select(Aperture::Circle(width.max(1)));
        let _ = writeln!(self.body, "{}D02*", Self::xy(a));
        let _ = writeln!(self.body, "{}D01*", Self::xy(b));
    }

    fn polyline(&mut self, points: &[Point], width: Coord) {
        let Some(first) = points.first() else { return };
        self.select(Aperture::Circle(width.max(1)));
        let _ = writeln!(self.body, "{}D02*", Self::xy(*first));
        for p in points.iter().skip(1) {
            let _ = writeln!(self.body, "{}D01*", Self::xy(*p));
        }
    }

    fn flash(&mut self, a: Aperture, at: Point) {
        self.select(a);
        let _ = writeln!(self.body, "{}D03*", Self::xy(at));
    }

    fn region(&mut self, points: &[Point]) {
        let Some(first) = points.first() else { return };
        let _ = writeln!(self.body, "G36*");
        let _ = writeln!(self.body, "{}D02*", Self::xy(*first));
        for p in points.iter().skip(1) {
            let _ = writeln!(self.body, "{}D01*", Self::xy(*p));
        }
        let _ = writeln!(self.body, "{}D01*", Self::xy(*first));
        let _ = writeln!(self.body, "G37*");
    }

    fn finish(self, function: &str, polarity: &str, design: &Design) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "%TF.GenerationSoftware,Hephaestus,Hephaestus,{}*%", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(out, "%TF.ProjectId,{},00000000-0000-0000-0000-000000000000,1*%", design.name.replace([',', '*', '%'], "_"));
        let _ = writeln!(out, "%TF.FileFunction,{function}*%");
        let _ = writeln!(out, "%TF.FilePolarity,{polarity}*%");
        let _ = writeln!(out, "%FSLAX46Y46*%");
        let _ = writeln!(out, "%MOMM*%");
        let _ = writeln!(out, "%LPD*%");
        let _ = writeln!(out, "G01*");
        let mm = |v: Coord| format!("{:.6}", v as f64 / 1_000_000.0);
        for (a, code) in &self.apertures {
            let _ = match a {
                Aperture::Circle(d) => writeln!(out, "%ADD{code}C,{}*%", mm(*d)),
                Aperture::Rect(w, h) => writeln!(out, "%ADD{code}R,{}X{}*%", mm(*w), mm(*h)),
                Aperture::Obround(w, h) => writeln!(out, "%ADD{code}O,{}X{}*%", mm(*w), mm(*h)),
            };
        }
        out.push_str(&self.body);
        out.push_str("M02*\n");
        out
    }
}

/// Outline of a rotated or rounded pad as a polygon (arcs as 8 segments per quarter).
fn pad_polygon(center: Point, w: Coord, h: Coord, corner: Coord, rotation: f64) -> Vec<Point> {
    let (hw, hh) = (w / 2, h / 2);
    let r = corner.clamp(0, hw.min(hh));
    let mut pts = Vec::new();
    let corners = [(hw - r, hh - r, 0.0), (-(hw - r), hh - r, 90.0), (-(hw - r), -(hh - r), 180.0), (hw - r, -(hh - r), 270.0)];
    for (cx, cy, start) in corners {
        if r == 0 {
            pts.push(Point::new(cx, cy));
            continue;
        }
        for k in 0..=8 {
            let a = (start + 90.0 * k as f64 / 8.0).to_radians();
            pts.push(Point::new(cx + (r as f64 * a.cos()).round() as Coord, cy + (r as f64 * a.sin()).round() as Coord));
        }
    }
    pts.into_iter().map(|p| p.rotated(rotation) + center).collect()
}

fn right_angle(rotation: f64) -> Option<bool> {
    let r = rotation.rem_euclid(180.0);
    if r == 0.0 {
        Some(false)
    } else if r == 90.0 {
        Some(true)
    } else {
        None
    }
}

/// Add one pad to a layer, grown by `grow` on every side.
fn pad(w: &mut Writer, p: &hephaestus_core::PadGeometry, grow: Coord) {
    let (pw, ph) = (p.width + 2 * grow, p.height + 2 * grow);
    match p.pad_shape {
        PadShape::Round => w.flash(Aperture::Circle(pw.min(ph)), p.center),
        PadShape::Rect => match right_angle(p.rotation) {
            Some(swap) => w.flash(if swap { Aperture::Rect(ph, pw) } else { Aperture::Rect(pw, ph) }, p.center),
            None => w.region(&pad_polygon(p.center, pw, ph, 0, p.rotation)),
        },
        PadShape::Oval => match right_angle(p.rotation) {
            Some(swap) => w.flash(if swap { Aperture::Obround(ph, pw) } else { Aperture::Obround(pw, ph) }, p.center),
            None => w.region(&pad_polygon(p.center, pw, ph, pw.min(ph) / 2, p.rotation)),
        },
        PadShape::RoundRect { ratio } => {
            let r = (p.width.min(p.height) as f64 * ratio.clamp(0.0, 0.5)).round() as Coord + grow;
            w.region(&pad_polygon(p.center, pw, ph, r, p.rotation));
        }
    }
}

fn graphic(w: &mut Writer, g: &SymbolGraphic, t: &Transform, min_width: Coord) {
    let width = |v: Coord| v.max(min_width);
    match g {
        SymbolGraphic::Line { points, width: lw } => w.polyline(&points.iter().map(|p| t.apply(*p)).collect::<Vec<_>>(), width(*lw)),
        SymbolGraphic::Polygon { points, width: lw, .. } => {
            let mut pts: Vec<Point> = points.iter().map(|p| t.apply(*p)).collect();
            if let Some(f) = pts.first().copied() {
                pts.push(f);
            }
            w.polyline(&pts, width(*lw));
        }
        SymbolGraphic::Rect { min, max, width: lw, .. } => {
            let c = [*min, Point::new(max.x, min.y), *max, Point::new(min.x, max.y), *min];
            w.polyline(&c.iter().map(|p| t.apply(*p)).collect::<Vec<_>>(), width(*lw));
        }
        SymbolGraphic::Circle { center, radius, width: lw, .. } => {
            let pts: Vec<Point> = (0..=48)
                .map(|k| {
                    let a = (k as f64 * 7.5).to_radians();
                    t.apply(*center + Point::new((*radius as f64 * a.cos()).round() as Coord, (*radius as f64 * a.sin()).round() as Coord))
                })
                .collect();
            w.polyline(&pts, width(*lw));
        }
        SymbolGraphic::Arc { center, radius, start, end, width: lw } => {
            let sweep = (end - start).rem_euclid(360.0);
            let sweep = if sweep == 0.0 { 360.0 } else { sweep };
            let steps = ((sweep / 7.5).ceil() as usize).max(2);
            let pts: Vec<Point> = (0..=steps)
                .map(|k| {
                    let a = (start + sweep * k as f64 / steps as f64).to_radians();
                    t.apply(*center + Point::new((*radius as f64 * a.cos()).round() as Coord, (*radius as f64 * a.sin()).round() as Coord))
                })
                .collect();
            w.polyline(&pts, width(*lw));
        }
        SymbolGraphic::Text { .. } => {}
    }
}

/// Write every fabrication layer into `dir` with file names prefixed by the project name.
pub fn export(design: &Design, dir: &Path) -> Result<ExportReport, ExportError> {
    let mut report = ExportReport::default();
    let base = design.name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "_");
    let copper = design.board.stack.copper_layers();
    let count = copper.len();
    let pads = design.all_pads();

    let mut text_skipped = 0;
    for (index, layer) in copper.iter().enumerate() {
        let mut w = Writer::new();
        for p in pads.iter().filter(|p| p.layers.contains(layer) && (p.plated || p.drill.is_none())) {
            pad(&mut w, p, 0);
        }
        for o in design.pcb.values() {
            match &o.kind {
                PcbKind::Track(t) if t.layer == *layer => w.line(t.start, t.end, t.width),
                PcbKind::Via(v) => {
                    let a = copper.iter().position(|l| *l == v.from).unwrap_or(0);
                    let b = copper.iter().position(|l| *l == v.to).unwrap_or(count.saturating_sub(1));
                    if (a.min(b)..=a.max(b)).contains(&index) {
                        w.flash(Aperture::Circle(v.diameter), v.position);
                    }
                }
                PcbKind::Zone(z) if z.layer == *layer => {
                    if z.filled.is_empty() {
                        report.warnings.push(format!("polygon {} on {layer} is not poured and was not exported", o.id));
                    }
                    for island in &z.filled {
                        w.region(island);
                    }
                }
                PcbKind::Text(t) if t.layer == *layer => text_skipped += 1,
                _ => {}
            }
        }
        let side = match layer {
            Layer::TopCopper => "Top",
            Layer::BottomCopper => "Bot",
            _ => "Inr",
        };
        let function = format!("Copper,L{},{side}", index + 1);
        let ext = match layer {
            Layer::TopCopper => "GTL".to_string(),
            Layer::BottomCopper => "GBL".to_string(),
            Layer::Inner(n) => format!("G{n}"),
            _ => "GBR".to_string(),
        };
        write(dir, &format!("{base}.{ext}"), &w.finish(&function, "Positive", design), &mut report)?;
    }

    for (layer, function, ext) in [(Layer::TopOverlay, "Legend,Top", "GTO"), (Layer::BottomOverlay, "Legend,Bot", "GBO")] {
        let mut w = Writer::new();
        for o in design.pcb.values() {
            match &o.kind {
                PcbKind::Footprint(f) => {
                    let Some(def) = design.footprint_def(f) else { continue };
                    let t = Design::footprint_transform(f);
                    for g in &def.graphics {
                        let on = if f.side == Side::Bottom { g.layer.flipped() } else { g.layer };
                        if on == layer {
                            graphic(&mut w, &g.shape, &t, 100_000);
                        }
                    }
                }
                PcbKind::Line(l) if l.layer == layer => w.line(l.start, l.end, l.width),
                PcbKind::Text(t) if t.layer == layer => text_skipped += 1,
                _ => {}
            }
        }
        write(dir, &format!("{base}.{ext}"), &w.finish(function, "Positive", design), &mut report)?;
    }

    for (side, function, ext, paste) in [
        (Layer::TopCopper, "Soldermask,Top", "GTS", false),
        (Layer::BottomCopper, "Soldermask,Bot", "GBS", false),
        (Layer::TopCopper, "Paste,Top", "GTP", true),
        (Layer::BottomCopper, "Paste,Bot", "GBP", true),
    ] {
        let mut w = Writer::new();
        for p in pads.iter().filter(|p| p.layers.contains(&side)) {
            if paste && p.drill.is_some() {
                continue;
            }
            pad(&mut w, p, if paste { 0 } else { MASK_EXPANSION });
        }
        write(dir, &format!("{base}.{ext}"), &w.finish(function, if paste { "Positive" } else { "Negative" }, design), &mut report)?;
    }

    let mut w = Writer::new();
    let mut outline = design.board.outline.clone();
    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }
    w.polyline(&outline, 100_000);
    write(dir, &format!("{base}.GKO"), &w.finish("Profile,NP", "Positive", design), &mut report)?;

    if text_skipped > 0 {
        report.warnings.push(format!("{text_skipped} free text object(s) were not exported: text stroking is not implemented yet"));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_polygon_closes_corners() {
        let poly = pad_polygon(Point::ZERO, 2000, 1000, 0, 0.0);
        assert_eq!(poly, vec![Point::new(1000, 500), Point::new(-1000, 500), Point::new(-1000, -500), Point::new(1000, -500)]);
        assert_eq!(pad_polygon(Point::ZERO, 2000, 1000, 200, 0.0).len(), 36);
    }
}
