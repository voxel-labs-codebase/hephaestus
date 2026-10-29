//! Footprint generators with IPC-7351 nominal land patterns.
//!
//! Conventions: overlay outlines on Top Overlay, assembly body on Mechanical 13, courtyard on
//! Mechanical 15 (Altium's common layer assignment). Pad 1 of polarized and multi-pin parts is
//! rectangular.

use hephaestus_core::geometry::{Coord, Point, Rect, mm};
use hephaestus_core::library::{FootprintGraphic, Model3d};
use hephaestus_core::{FootprintDef, Layer, PadDef, PadKind, PadShape, SymbolGraphic};

const SILK: Coord = 150_000;

fn p(x: f64, y: f64) -> Point {
    Point::new(mm(x), mm(y))
}

fn smd(number: impl Into<String>, x: f64, y: f64, w: f64, h: f64, shape: PadShape) -> PadDef {
    PadDef { number: number.into(), position: p(x, y), width: mm(w), height: mm(h), shape, rotation: 0.0, kind: PadKind::Smd }
}

fn tht(number: impl Into<String>, x: f64, y: f64, size: f64, drill: f64, shape: PadShape) -> PadDef {
    PadDef {
        number: number.into(),
        position: p(x, y),
        width: mm(size),
        height: mm(size),
        shape,
        rotation: 0.0,
        kind: PadKind::ThroughHole { drill: mm(drill) },
    }
}

fn rr() -> PadShape {
    PadShape::RoundRect { ratio: 0.25 }
}

fn rect_outline(layer: Layer, half_w: f64, half_h: f64, width: Coord) -> FootprintGraphic {
    FootprintGraphic { layer, shape: SymbolGraphic::Rect { min: p(-half_w, -half_h), max: p(half_w, half_h), width, filled: false } }
}

fn silk_line(points: &[(f64, f64)]) -> FootprintGraphic {
    FootprintGraphic {
        layer: Layer::TopOverlay,
        shape: SymbolGraphic::Line { points: points.iter().map(|&(x, y)| p(x, y)).collect(), width: SILK },
    }
}

fn finish(
    name: &str,
    description: &str,
    pads: Vec<PadDef>,
    body: (f64, f64),
    height: f64,
    mut graphics: Vec<FootprintGraphic>,
) -> FootprintDef {
    let (bw, bh) = body;
    let pad_bounds = pads
        .iter()
        .map(|pad| Rect::from_center(pad.position, pad.width / 2, pad.height / 2))
        .reduce(Rect::union)
        .unwrap_or(Rect::from_center(Point::ZERO, 0, 0));
    let courtyard = pad_bounds.union(Rect::from_center(Point::ZERO, mm(bw / 2.0), mm(bh / 2.0))).inflated(mm(0.25));
    graphics.push(rect_outline(Layer::Mechanical(13), bw / 2.0, bh / 2.0, mm(0.1)));
    graphics.push(FootprintGraphic {
        layer: Layer::Mechanical(15),
        shape: SymbolGraphic::Rect { min: courtyard.min, max: courtyard.max, width: mm(0.05), filled: false },
    });
    FootprintDef {
        name: name.into(),
        description: description.into(),
        pads,
        graphics,
        courtyard,
        model: Some(Model3d { source: format!("generated:box {bw}x{bh}x{height}mm"), height: mm(height) }),
    }
}

/// Two-terminal chip (resistor, capacitor, inductor).
pub fn chip(name: &str, pitch: f64, pad: (f64, f64), body: (f64, f64)) -> FootprintDef {
    let x = pitch / 2.0;
    let pads = vec![smd("1", -x, 0.0, pad.0, pad.1, rr()), smd("2", x, 0.0, pad.0, pad.1, rr())];
    let gap = x - pad.0 / 2.0 - 0.15;
    let y = body.1 / 2.0 + 0.1;
    let silk = if gap > 0.1 { vec![silk_line(&[(-gap, y), (gap, y)]), silk_line(&[(-gap, -y), (gap, -y)])] } else { Vec::new() };
    finish(name, &format!("Chip {name}"), pads, body, body.1.min(0.8), silk)
}

/// A polarized two-terminal SMD (diode, LED) with a cathode bar on pad 1.
pub fn polarized_chip(name: &str, pitch: f64, pad: (f64, f64), body: (f64, f64)) -> FootprintDef {
    let x = pitch / 2.0;
    let pads = vec![smd("1", -x, 0.0, pad.0, pad.1, PadShape::Rect), smd("2", x, 0.0, pad.0, pad.1, rr())];
    let y = (body.1 / 2.0 + 0.15).max(pad.1 / 2.0 + 0.2);
    let left = x + pad.0 / 2.0 + 0.2;
    let silk = vec![silk_line(&[(x, y), (-left, y), (-left, -y), (x, -y)])];
    finish(name, &format!("Polarized {name}"), pads, body, 1.0, silk)
}

/// Small-outline transistor: SOT-23 (3 pins) or SOT-23-5.
pub fn sot23(pins: usize) -> FootprintDef {
    let pad = (0.6, 1.1);
    let pads = if pins == 5 {
        vec![
            smd("1", -0.95, -1.1, pad.0, pad.1, rr()),
            smd("2", 0.0, -1.1, pad.0, pad.1, rr()),
            smd("3", 0.95, -1.1, pad.0, pad.1, rr()),
            smd("4", 0.95, 1.1, pad.0, pad.1, rr()),
            smd("5", -0.95, 1.1, pad.0, pad.1, rr()),
        ]
    } else {
        vec![smd("1", -0.95, -1.1, pad.0, pad.1, rr()), smd("2", 0.95, -1.1, pad.0, pad.1, rr()), smd("3", 0.0, 1.1, pad.0, pad.1, rr())]
    };
    let silk = vec![silk_line(&[(-1.55, 0.35), (-1.55, -0.35)]), silk_line(&[(1.55, 0.35), (1.55, -0.35)])];
    let name = if pins == 5 { "SOT-23-5" } else { "SOT-23" };
    finish(name, &format!("{name} small-outline transistor"), pads, (2.9, 1.6), 1.1, silk)
}

pub fn sot223() -> FootprintDef {
    let pads = vec![
        smd("1", -2.3, -3.15, 0.95, 2.0, PadShape::Rect),
        smd("2", 0.0, -3.15, 0.95, 2.0, rr()),
        smd("3", 2.3, -3.15, 0.95, 2.0, rr()),
        smd("4", 0.0, 3.15, 3.25, 2.0, rr()),
    ];
    let silk = vec![silk_line(&[(-3.4, -1.9), (-3.4, 1.9)]), silk_line(&[(3.4, -1.9), (3.4, 1.9)])];
    finish("SOT-223", "SOT-223 power transistor package", pads, (6.5, 3.5), 1.8, silk)
}

/// Dual-row gull-wing package (SOIC/SSOP), pins counter-clockwise from top-left.
pub fn soic(pins: usize, pitch: f64, row: f64, pad: (f64, f64), body: (f64, f64)) -> FootprintDef {
    let per_side = pins / 2;
    let top = (per_side as f64 - 1.0) * pitch / 2.0;
    let mut pads = Vec::new();
    for i in 0..per_side {
        let shape = if i == 0 { PadShape::Rect } else { rr() };
        pads.push(smd((i + 1).to_string(), -row / 2.0, top - i as f64 * pitch, pad.0, pad.1, shape));
    }
    for i in 0..per_side {
        pads.push(smd((per_side + i + 1).to_string(), row / 2.0, -top + i as f64 * pitch, pad.0, pad.1, rr()));
    }
    let (hw, hh) = (body.0 / 2.0, body.1 / 2.0 + 0.1);
    let silk = vec![
        silk_line(&[(-hw, hh), (hw, hh)]),
        silk_line(&[(-hw, -hh), (hw, -hh)]),
        silk_line(&[(-row / 2.0 - pad.0 / 2.0, hh + 0.05), (-hw, hh + 0.05)]),
    ];
    let name = format!("SOIC-{pins}");
    finish(&name, &format!("{pins}-pin small-outline IC, {pitch} mm pitch"), pads, body, 1.5, silk)
}

/// Quad flat package, square, pins counter-clockwise from the top of the left side.
pub fn qfp(pins: usize, pitch: f64, span: f64, pad: (f64, f64), body: f64) -> FootprintDef {
    let per_side = pins / 4;
    let start = (per_side as f64 - 1.0) * pitch / 2.0;
    let r = span / 2.0;
    let mut pads = Vec::new();
    for i in 0..pins {
        let side = i / per_side;
        let k = (i % per_side) as f64;
        let (x, y, w, h) = match side {
            0 => (-r, start - k * pitch, pad.0, pad.1),
            1 => (-start + k * pitch, -r, pad.1, pad.0),
            2 => (r, -start + k * pitch, pad.0, pad.1),
            _ => (start - k * pitch, r, pad.1, pad.0),
        };
        pads.push(smd((i + 1).to_string(), x, y, w, h, if i == 0 { PadShape::Rect } else { rr() }));
    }
    let h = body / 2.0 + 0.1;
    let c = start + pitch / 2.0 + 0.1;
    let silk = vec![
        silk_line(&[(-c, h), (-h, h), (-h, c)]),
        silk_line(&[(c, h), (h, h), (h, c)]),
        silk_line(&[(-c, -h), (-h, -h), (-h, -c)]),
        silk_line(&[(c, -h), (h, -h), (h, -c)]),
        silk_line(&[(-h, c), (-r - pad.0 / 2.0, c)]),
    ];
    let name = format!("TQFP-{pins}");
    finish(&name, &format!("{pins}-pin thin quad flat package, {body} mm body"), pads, (body, body), 1.2, silk)
}

/// Dual in-line through-hole package, 2.54 mm pitch, 7.62 mm row spacing.
pub fn dip(pins: usize) -> FootprintDef {
    let per_side = pins / 2;
    let top = (per_side as f64 - 1.0) * 2.54 / 2.0;
    let mut pads = Vec::new();
    for i in 0..per_side {
        let shape = if i == 0 { PadShape::Rect } else { PadShape::Round };
        pads.push(tht((i + 1).to_string(), -3.81, top - i as f64 * 2.54, 1.6, 0.8, shape));
    }
    for i in 0..per_side {
        pads.push(tht((per_side + i + 1).to_string(), 3.81, -top + i as f64 * 2.54, 1.6, 0.8, PadShape::Round));
    }
    let hh = top + 1.4;
    let silk = vec![
        silk_line(&[(-1.0, hh), (-2.6, hh), (-2.6, -hh), (2.6, -hh), (2.6, hh), (1.0, hh)]),
        FootprintGraphic {
            layer: Layer::TopOverlay,
            shape: SymbolGraphic::Arc { center: p(0.0, hh), radius: mm(1.0), start: 180.0, end: 360.0, width: SILK },
        },
    ];
    let name = format!("DIP-{pins}");
    finish(&name, &format!("{pins}-pin dual in-line package, 300 mil"), pads, (6.35, hh * 2.0), 3.3, silk)
}

/// Single-row 2.54 mm pin header, pin 1 at the top.
pub fn header(pins: usize) -> FootprintDef {
    let top = (pins as f64 - 1.0) * 2.54 / 2.0;
    let pads = (0..pins)
        .map(|i| tht((i + 1).to_string(), 0.0, top - i as f64 * 2.54, 1.7, 1.0, if i == 0 { PadShape::Rect } else { PadShape::Round }))
        .collect();
    let hh = top + 1.27;
    let silk = vec![silk_line(&[(-1.27, hh), (1.27, hh), (1.27, -hh), (-1.27, -hh), (-1.27, hh)])];
    let name = format!("HDR-1x{pins}");
    finish(&name, &format!("1x{pins} pin header, 2.54 mm pitch"), pads, (2.54, hh * 2.0), 8.5, silk)
}

/// Two-lead through-hole part with `pitch` mm lead spacing.
pub fn axial(name: &str, pitch: f64, body: (f64, f64), pad: f64, drill: f64, polarized: bool) -> FootprintDef {
    let x = pitch / 2.0;
    let first = if polarized { PadShape::Rect } else { PadShape::Round };
    let pads = vec![tht("1", -x, 0.0, pad, drill, first), tht("2", x, 0.0, pad, drill, PadShape::Round)];
    let (hw, hh) = (body.0 / 2.0, body.1 / 2.0);
    let silk = vec![silk_line(&[(-hw, hh), (hw, hh), (hw, -hh), (-hw, -hh), (-hw, hh)])];
    finish(name, &format!("Through-hole, {pitch} mm lead spacing"), pads, body, body.1, silk)
}

pub fn radial_led() -> FootprintDef {
    let pads = vec![tht("1", -1.27, 0.0, 1.8, 0.9, PadShape::Rect), tht("2", 1.27, 0.0, 1.8, 0.9, PadShape::Round)];
    let silk = vec![FootprintGraphic {
        layer: Layer::TopOverlay,
        shape: SymbolGraphic::Arc { center: Point::ZERO, radius: mm(2.9), start: -150.0, end: 150.0, width: SILK },
    }];
    finish("LED-5MM", "5 mm radial LED", pads, (5.8, 5.8), 8.6, silk)
}

pub fn mounting_hole() -> FootprintDef {
    let pads = vec![PadDef {
        number: "".into(),
        position: Point::ZERO,
        width: mm(3.2),
        height: mm(3.2),
        shape: PadShape::Round,
        rotation: 0.0,
        kind: PadKind::NonPlated { drill: mm(3.2) },
    }];
    finish("MH-M3", "M3 mounting hole (non-plated)", pads, (6.0, 6.0), 0.0, Vec::new())
}

pub fn test_point() -> FootprintDef {
    finish("TP-1.5MM", "1.5 mm round SMD test point", vec![smd("1", 0.0, 0.0, 1.5, 1.5, PadShape::Round)], (1.5, 1.5), 0.1, Vec::new())
}
