//! Design rule check on the board.

use std::collections::BTreeSet;

use hephaestus_connectivity::pcb::{CopperRef, PcbConnectivity, analyze};
use hephaestus_core::geometry::{Coord, GridIndex, Point, Shape, Unit, format_coord, mil, polygon_contains};
use hephaestus_core::{Design, Layer, ObjectId, PcbKind, ZoneFillMode};

use crate::{Check, Report, Severity, Violation};

fn v(rule: &str, severity: Severity, message: String, objects: Vec<ObjectId>, location: Option<Point>, layer: Option<Layer>) -> Violation {
    Violation { check: Check::Drc, rule: rule.into(), severity, message, objects, location, sheet: None, layer }
}

/// A piece of copper for clearance checking.
struct Item {
    object: ObjectId,
    /// Pads of one footprint don't check against each other (Altium's "ignore pad to pad
    /// clearances within a footprint").
    footprint: Option<ObjectId>,
    label: String,
    shape: Shape,
    layers: Vec<Layer>,
    net: Option<String>,
    anchor: Point,
}

fn copper_items(design: &Design) -> Vec<Item> {
    let stack = design.board.stack.copper_layers();
    let mut items = Vec::new();
    for o in design.pcb.values() {
        match &o.kind {
            PcbKind::Footprint(_) => {
                let designator = |c| design.components.get(&c).map(|c| c.designator.clone());
                for pad in design.pads(o) {
                    let owner = pad.component.and_then(designator).unwrap_or_else(|| o.id.to_string());
                    items.push(Item {
                        object: o.id,
                        footprint: Some(o.id),
                        label: format!("Pad {owner}-{}", pad.number),
                        anchor: pad.center,
                        shape: pad.shape,
                        layers: pad.layers,
                        net: pad.net,
                    });
                }
            }
            PcbKind::Track(t) if t.layer.is_copper() => items.push(Item {
                object: o.id,
                footprint: None,
                label: format!("Track {}", o.id),
                anchor: Point::new((t.start.x + t.end.x) / 2, (t.start.y + t.end.y) / 2),
                shape: Shape::capsule(t.start, t.end, t.width),
                layers: vec![t.layer],
                net: t.net.clone(),
            }),
            PcbKind::Via(via) => {
                let a = stack.iter().position(|l| *l == via.from).unwrap_or(0);
                let b = stack.iter().position(|l| *l == via.to).unwrap_or(stack.len().saturating_sub(1));
                items.push(Item {
                    object: o.id,
                    footprint: None,
                    label: format!("Via {}", o.id),
                    anchor: via.position,
                    shape: Shape::circle(via.position, via.diameter),
                    layers: stack.get(a.min(b)..=a.max(b)).map(<[Layer]>::to_vec).unwrap_or_default(),
                    net: via.net.clone(),
                });
            }
            PcbKind::Zone(z) if z.layer.is_copper() => {
                for island in z.filled.iter().filter(|i| i.len() >= 3) {
                    items.push(Item {
                        object: o.id,
                        footprint: None,
                        label: format!("Polygon {}", o.id),
                        anchor: island.first().copied().unwrap_or_default(),
                        shape: Shape::polygon(island.clone()),
                        layers: vec![z.layer],
                        net: z.net.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    items
}

fn fmt(v: Coord) -> String {
    format_coord(v, Unit::Mil)
}

/// Run every DRC check.
pub fn check(design: &Design) -> Report {
    check_with(design, &analyze(design))
}

pub fn check_with(design: &Design, connectivity: &PcbConnectivity) -> Report {
    let rules = &design.board.rules;
    let mut out = Vec::new();
    let items = copper_items(design);

    // Clearance between copper of different nets. Overlaps are reported as shorts below.
    let mut index = GridIndex::new(mil(200.0));
    for (i, item) in items.iter().enumerate() {
        if let Some(b) = item.shape.bounds() {
            index.insert(i, b);
        }
    }
    let max_clearance = rules.net_classes.iter().map(|c| c.clearance).fold(rules.clearance, Coord::max);
    let mut reported: BTreeSet<(ObjectId, ObjectId, Layer)> = BTreeSet::new();
    for (i, a) in items.iter().enumerate() {
        let Some(bounds) = a.shape.bounds() else { continue };
        for &j in index.query_unique(bounds.inflated(max_clearance)) {
            if j <= i {
                continue;
            }
            let Some(b) = items.get(j) else { continue };
            if a.object == b.object || (a.footprint.is_some() && a.footprint == b.footprint) {
                continue;
            }
            if a.net.is_some() && a.net == b.net {
                continue;
            }
            let Some(&layer) = a.layers.iter().find(|l| b.layers.contains(l)) else { continue };
            let required = rules.clearance_between(a.net.as_deref(), b.net.as_deref());
            let gap = a.shape.gap(&b.shape);
            if gap > 0.0 && gap < required as f64 {
                let key = (a.object.min(b.object), a.object.max(b.object), layer);
                if reported.insert(key) {
                    out.push(v(
                        "Clearance",
                        Severity::Error,
                        format!(
                            "{} and {} on {layer} are {} apart (minimum {})",
                            a.label,
                            b.label,
                            fmt(gap.round() as Coord),
                            fmt(required)
                        ),
                        vec![a.object, b.object],
                        Some(a.anchor),
                        Some(layer),
                    ));
                }
            } else if gap <= 0.0 && a.net.is_none() != b.net.is_none() {
                // Netless copper touching a net: not a short between nets, but still wrong.
                let key = (a.object.min(b.object), a.object.max(b.object), layer);
                if reported.insert(key) {
                    out.push(v(
                        "Clearance",
                        Severity::Error,
                        format!("{} and {} overlap on {layer}; one of them has no net", a.label, b.label),
                        vec![a.object, b.object],
                        Some(a.anchor),
                        Some(layer),
                    ));
                }
            }
        }
    }

    for short in &connectivity.shorts {
        let objects: Vec<ObjectId> = short.items.iter().map(CopperRef::object).collect::<BTreeSet<_>>().into_iter().collect();
        out.push(v(
            "Short-Circuit",
            Severity::Error,
            format!("Nets {} are shorted", short.nets.join(" and ")),
            objects,
            Some(short.location),
            None,
        ));
    }

    for r in &connectivity.unrouted {
        out.push(v(
            "Un-Routed Net",
            Severity::Error,
            format!("Net {} is not routed between {} and {}", r.net, describe(design, &r.from_item), describe(design, &r.to_item)),
            vec![r.from_item.object(), r.to_item.object()],
            Some(r.from),
            None,
        ));
    }

    // Track widths, via geometry and holes.
    for o in design.pcb.values() {
        match &o.kind {
            PcbKind::Track(t) if t.layer.is_copper() => {
                if t.width < rules.min_track_width || t.width > rules.max_track_width {
                    out.push(v(
                        "Width",
                        Severity::Error,
                        format!(
                            "Track {} is {} wide (allowed {} – {})",
                            o.id,
                            fmt(t.width),
                            fmt(rules.min_track_width),
                            fmt(rules.max_track_width)
                        ),
                        vec![o.id],
                        Some(t.start),
                        Some(t.layer),
                    ));
                }
            }
            PcbKind::Via(via) => {
                if via.diameter < rules.min_via_diameter {
                    out.push(v(
                        "Routing Via Style",
                        Severity::Error,
                        format!("Via {} diameter {} is below {}", o.id, fmt(via.diameter), fmt(rules.min_via_diameter)),
                        vec![o.id],
                        Some(via.position),
                        None,
                    ));
                }
                if via.drill < rules.min_via_drill || via.drill < rules.min_hole_size {
                    out.push(v(
                        "Hole Size",
                        Severity::Error,
                        format!("Via {} drill {} is below {}", o.id, fmt(via.drill), fmt(rules.min_via_drill.max(rules.min_hole_size))),
                        vec![o.id],
                        Some(via.position),
                        None,
                    ));
                }
                let ring = (via.diameter - via.drill) / 2;
                if ring < rules.min_annular_ring {
                    out.push(v(
                        "Minimum Annular Ring",
                        Severity::Error,
                        format!("Via {} annular ring {} is below {}", o.id, fmt(ring), fmt(rules.min_annular_ring)),
                        vec![o.id],
                        Some(via.position),
                        None,
                    ));
                }
            }
            PcbKind::Footprint(_) => {
                for pad in design.pads(o) {
                    let Some(drill) = pad.drill else { continue };
                    if drill < rules.min_hole_size || drill > rules.max_hole_size {
                        out.push(v(
                            "Hole Size",
                            Severity::Error,
                            format!(
                                "Pad {} hole {} is outside {} – {}",
                                pad.number,
                                fmt(drill),
                                fmt(rules.min_hole_size),
                                fmt(rules.max_hole_size)
                            ),
                            vec![o.id],
                            Some(pad.center),
                            None,
                        ));
                    }
                    let ring = (pad.width.min(pad.height) - drill) / 2;
                    if pad.plated && ring < rules.min_annular_ring {
                        out.push(v(
                            "Minimum Annular Ring",
                            Severity::Error,
                            format!("Pad {} annular ring {} is below {}", pad.number, fmt(ring), fmt(rules.min_annular_ring)),
                            vec![o.id],
                            Some(pad.center),
                            None,
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    // Board edge: copper must sit inside the outline with clearance to every edge.
    let outline = &design.board.outline;
    if outline.len() >= 3 {
        let edges: Vec<Shape> =
            (0..outline.len()).filter_map(|i| Some(Shape::capsule(*outline.get(i)?, *outline.get((i + 1) % outline.len())?, 0))).collect();
        let mut flagged: BTreeSet<ObjectId> = BTreeSet::new();
        for item in &items {
            if flagged.contains(&item.object) {
                continue;
            }
            let inside = polygon_contains(outline, item.anchor);
            let gap = edges.iter().map(|e| item.shape.gap(e)).fold(f64::INFINITY, f64::min);
            if !inside || gap < rules.board_edge_clearance as f64 {
                flagged.insert(item.object);
                let message = if inside {
                    format!(
                        "{} is {} from the board edge (minimum {})",
                        item.label,
                        fmt(gap.max(0.0).round() as Coord),
                        fmt(rules.board_edge_clearance)
                    )
                } else {
                    format!("{} is outside the board outline", item.label)
                };
                out.push(v(
                    "Board Outline Clearance",
                    Severity::Error,
                    message,
                    vec![item.object],
                    Some(item.anchor),
                    item.layers.first().copied(),
                ));
            }
        }
    }

    // Keep-outs.
    for k in design.pcb.values() {
        let PcbKind::Keepout(keepout) = &k.kind else { continue };
        let region = Shape::polygon(keepout.outline.clone());
        for item in &items {
            let forbidden = match design.pcb.get(&item.object).map(|o| &o.kind) {
                Some(PcbKind::Track(_)) => keepout.no_tracks,
                Some(PcbKind::Via(_)) => keepout.no_vias,
                Some(PcbKind::Footprint(_)) => keepout.no_pads,
                Some(PcbKind::Zone(_)) => keepout.no_pours,
                _ => false,
            };
            if forbidden && item.layers.iter().any(|l| keepout.applies_to(*l)) && item.shape.gap(&region) <= 0.0 {
                out.push(v(
                    "Keepout",
                    Severity::Error,
                    format!("{} intrudes into keepout {}", item.label, k.id),
                    vec![item.object, k.id],
                    Some(item.anchor),
                    item.layers.first().copied(),
                ));
            }
        }
    }

    // Placement: components off the board and overlapping courtyards.
    let footprints: Vec<(ObjectId, hephaestus_core::geometry::Rect, String)> = design
        .pcb
        .values()
        .filter_map(|o| match &o.kind {
            PcbKind::Footprint(f) => {
                let name = f.component.and_then(|c| design.components.get(&c)).map_or_else(|| o.id.to_string(), |c| c.designator.clone());
                Some((o.id, design.pcb_bounds(o), name))
            }
            _ => None,
        })
        .collect();
    for (id, bounds, name) in &footprints {
        let corners = [bounds.min, bounds.max, Point::new(bounds.min.x, bounds.max.y), Point::new(bounds.max.x, bounds.min.y)];
        if outline.len() >= 3 && !corners.iter().all(|c| polygon_contains(outline, *c)) {
            out.push(v(
                "Component Off Board",
                Severity::Warning,
                format!("{name} is not fully inside the board outline"),
                vec![*id],
                Some(bounds.center()),
                None,
            ));
        }
    }
    for (i, (a, ba, na)) in footprints.iter().enumerate() {
        for (b, bb, nb) in footprints.iter().skip(i + 1) {
            let sides = |id: &ObjectId| match design.pcb.get(id).map(|o| &o.kind) {
                Some(PcbKind::Footprint(f)) => Some(f.side),
                _ => None,
            };
            if ba.inflated(-mil(1.0)).intersects(&bb.inflated(-mil(1.0))) && sides(a) == sides(b) {
                out.push(v(
                    "Component Clearance",
                    Severity::Warning,
                    format!("{na} and {nb} courtyards overlap"),
                    vec![*a, *b],
                    Some(ba.center()),
                    None,
                ));
            }
        }
    }

    // Copper pours: unpoured outlines carry no copper yet.
    for o in design.pcb.values() {
        if let PcbKind::Zone(z) = &o.kind {
            if z.fill_mode != ZoneFillMode::None && z.filled.is_empty() {
                out.push(v(
                    "Unpoured Polygon",
                    Severity::Warning,
                    format!("Polygon {} on {} is not poured (zone filling is not implemented yet)", o.id, z.layer),
                    vec![o.id],
                    z.outline.first().copied(),
                    Some(z.layer),
                ));
            }
            if outline.len() >= 3 && !z.outline.iter().all(|p| polygon_contains(outline, *p)) {
                out.push(v(
                    "Polygon Off Board",
                    Severity::Warning,
                    format!("Polygon {} extends beyond the board outline", o.id),
                    vec![o.id],
                    z.outline.first().copied(),
                    Some(z.layer),
                ));
            }
        }
    }

    Report::new(out)
}

fn describe(design: &Design, item: &CopperRef) -> String {
    match item {
        CopperRef::Pad { footprint, number } => {
            let owner = design
                .pcb
                .get(footprint)
                .and_then(|o| match &o.kind {
                    PcbKind::Footprint(f) => f.component,
                    _ => None,
                })
                .and_then(|c| design.components.get(&c))
                .map_or_else(|| footprint.to_string(), |c| c.designator.clone());
            format!("pad {owner}-{number}")
        }
        CopperRef::Track { id } => format!("track {id}"),
        CopperRef::Via { id } => format!("via {id}"),
        CopperRef::Zone { id } => format!("polygon {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hephaestus_commands::{Command, ComponentRef, Editor, Source};
    use hephaestus_core::geometry::mil;

    fn p(x: f64, y: f64) -> Point {
        Point::new(mil(x), mil(y))
    }

    fn two_resistors() -> Editor {
        let mut e = Editor::new(hephaestus_core::Design::new("drc"), vec![hephaestus_libraries::builtin()]);
        for (x, d) in [(1000.0, "R1"), (2000.0, "R2")] {
            e.execute(
                &Command::PlaceComponent {
                    part: "RES".into(),
                    sheet: None,
                    position: p(x, 1000.0),
                    rotation: 0.0,
                    mirror: false,
                    designator: Some(d.into()),
                    value: None,
                    footprint: Some("0805".into()),
                    parameters: Default::default(),
                },
                Source::User,
            )
            .unwrap();
        }
        e.execute(&Command::AddWire { sheet: None, points: vec![p(1200.0, 1000.0), p(1800.0, 1000.0)] }, Source::User).unwrap();
        e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
        e.execute(
            &Command::PlaceFootprint {
                component: ComponentRef::Designator("R1".into()),
                position: p(1000.0, 1000.0),
                rotation: None,
                side: None,
            },
            Source::User,
        )
        .unwrap();
        e.execute(
            &Command::PlaceFootprint {
                component: ComponentRef::Designator("R2".into()),
                position: p(1600.0, 1000.0),
                rotation: None,
                side: None,
            },
            Source::User,
        )
        .unwrap();
        e
    }

    #[test]
    fn unrouted_then_clean_after_routing() {
        let mut e = two_resistors();
        let report = check(e.design());
        assert!(report.violations.iter().any(|v| v.rule == "Un-Routed Net"), "{:#?}", report.violations);
        e.execute(&Command::RouteNet { net: "NetR1_2".into(), layer: None, width: None }, Source::User).unwrap();
        let report = check(e.design());
        assert!(report.is_clean(), "{:#?}", report.violations);
    }

    #[test]
    fn track_too_close_to_foreign_pad_violates_clearance() {
        let mut e = two_resistors();
        // A GND track passing 2 mil from R1's pads.
        let r1_pad1_top = mil(1000.0) + 725_000; // pad half-height 0.725 mm
        let y = r1_pad1_top + mil(2.0) + mil(5.0);
        e.execute(
            &Command::AddTrack {
                start: Point::new(mil(800.0), y),
                end: Point::new(mil(1200.0), y),
                layer: Layer::TopCopper,
                width: Some(mil(10.0)),
                net: Some("GND".into()),
            },
            Source::User,
        )
        .unwrap();
        let report = check(e.design());
        assert!(report.violations.iter().any(|v| v.rule == "Clearance"), "{:#?}", report.violations);
    }

    #[test]
    fn narrow_track_small_via_and_edge_violations() {
        let mut e = two_resistors();
        e.execute(
            &Command::AddTrack {
                start: p(100.0, 100.0),
                end: p(300.0, 100.0),
                layer: Layer::TopCopper,
                width: Some(mil(4.0)),
                net: Some("X".into()),
            },
            Source::User,
        )
        .unwrap();
        e.execute(
            &Command::AddVia {
                position: p(500.0, 500.0),
                diameter: Some(mil(16.0)),
                drill: Some(mil(10.0)),
                net: Some("X".into()),
                from: None,
                to: None,
            },
            Source::User,
        )
        .unwrap();
        e.execute(
            &Command::AddTrack {
                start: p(-200.0, 600.0),
                end: p(200.0, 600.0),
                layer: Layer::TopCopper,
                width: Some(mil(10.0)),
                net: Some("Y".into()),
            },
            Source::User,
        )
        .unwrap();
        let rules: Vec<String> = check(e.design()).violations.into_iter().map(|v| v.rule).collect();
        for expected in ["Width", "Routing Via Style", "Minimum Annular Ring", "Board Outline Clearance"] {
            assert!(rules.iter().any(|r| r == expected), "missing {expected} in {rules:?}");
        }
    }

    #[test]
    fn erc_flags_floating_inputs_and_single_pin_nets() {
        let mut e = Editor::new(hephaestus_core::Design::new("erc"), vec![hephaestus_libraries::builtin()]);
        e.execute(
            &Command::PlaceComponent {
                part: "NE555".into(),
                sheet: None,
                position: p(3000.0, 3000.0),
                rotation: 0.0,
                mirror: false,
                designator: None,
                value: None,
                footprint: None,
                parameters: Default::default(),
            },
            Source::User,
        )
        .unwrap();
        let report = crate::erc::check(e.design());
        let rules: Vec<&str> = report.violations.iter().map(|v| v.rule.as_str()).collect();
        assert!(rules.contains(&"Floating input pin"));
        assert!(rules.contains(&"Unconnected power pin"));
        assert!(report.violations.iter().all(|v| v.location.is_some() && v.sheet.is_some()));
    }
}
