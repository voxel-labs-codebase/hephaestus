//! Board copper connectivity: islands of touching copper, unrouted connections and shorts.
//!
//! Copper touches when two items share a copper layer and their shapes overlap. Zones count only
//! through their filled copper (`Zone::filled`); an unpoured zone connects nothing.

use std::collections::{BTreeMap, BTreeSet};

use hephaestus_core::geometry::{GridIndex, Point, Shape, mil};
use hephaestus_core::{Design, Layer, ObjectId, PcbKind};
use serde::Serialize;

use crate::union_find::UnionFind;

/// One piece of copper.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(tag = "type")]
pub enum CopperRef {
    Pad { footprint: ObjectId, number: String },
    Track { id: ObjectId },
    Via { id: ObjectId },
    Zone { id: ObjectId },
}

impl CopperRef {
    /// The PCB object that owns this copper.
    pub fn object(&self) -> ObjectId {
        match self {
            CopperRef::Pad { footprint, .. } => *footprint,
            CopperRef::Track { id } | CopperRef::Via { id } | CopperRef::Zone { id } => *id,
        }
    }
}

/// A missing connection between two islands of the same net (one ratsnest line).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ratsnest {
    pub net: String,
    pub from: Point,
    pub to: Point,
    pub from_item: CopperRef,
    pub to_item: CopperRef,
}

/// Copper of different nets touching.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Short {
    pub nets: Vec<String>,
    pub items: Vec<CopperRef>,
    pub location: Point,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NetRouting {
    pub pads: usize,
    /// Pad-to-pad connections a fully routed net has (pads − 1).
    pub connections: usize,
    pub unrouted: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PcbConnectivity {
    pub unrouted: Vec<Ratsnest>,
    pub shorts: Vec<Short>,
    pub nets: BTreeMap<String, NetRouting>,
    /// Copper items in each island; items of one island are electrically connected.
    pub islands: Vec<Vec<CopperRef>>,
}

impl PcbConnectivity {
    pub fn unrouted_count(&self) -> usize {
        self.unrouted.len()
    }
}

struct Copper {
    item: CopperRef,
    shape: Shape,
    layers: Vec<Layer>,
    net: Option<String>,
    /// Where ratsnest lines attach.
    anchor: Point,
}

fn collect(design: &Design) -> Vec<Copper> {
    let stack = design.board.stack.copper_layers();
    let span = |from: Layer, to: Layer| -> Vec<Layer> {
        let a = stack.iter().position(|l| *l == from);
        let b = stack.iter().position(|l| *l == to);
        match (a, b) {
            (Some(a), Some(b)) => stack.get(a.min(b)..=a.max(b)).map(<[Layer]>::to_vec).unwrap_or_default(),
            _ => stack.clone(),
        }
    };
    let mut out = Vec::new();
    for object in design.pcb.values() {
        match &object.kind {
            PcbKind::Footprint(_) => {
                for pad in design.pads(object) {
                    if !pad.plated && pad.drill.is_some() {
                        continue;
                    }
                    out.push(Copper {
                        item: CopperRef::Pad { footprint: pad.footprint, number: pad.number.clone() },
                        anchor: pad.center,
                        shape: pad.shape,
                        layers: pad.layers,
                        net: pad.net,
                    });
                }
            }
            PcbKind::Track(t) if t.layer.is_copper() => out.push(Copper {
                item: CopperRef::Track { id: object.id },
                shape: Shape::capsule(t.start, t.end, t.width),
                layers: vec![t.layer],
                net: t.net.clone(),
                anchor: t.start,
            }),
            PcbKind::Via(v) => out.push(Copper {
                item: CopperRef::Via { id: object.id },
                shape: Shape::circle(v.position, v.diameter),
                layers: span(v.from, v.to),
                net: v.net.clone(),
                anchor: v.position,
            }),
            PcbKind::Zone(z) if z.layer.is_copper() => {
                for island in z.filled.iter().filter(|i| i.len() >= 3) {
                    out.push(Copper {
                        item: CopperRef::Zone { id: object.id },
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
    out
}

/// Analyze board copper connectivity against the pads' assigned nets.
pub fn analyze(design: &Design) -> PcbConnectivity {
    let copper = collect(design);
    let mut index = GridIndex::new(mil(200.0));
    for (i, c) in copper.iter().enumerate() {
        if let Some(b) = c.shape.bounds() {
            index.insert(i, b);
        }
    }
    let mut uf = UnionFind::new(copper.len());
    let mut contacts: Vec<(usize, usize)> = Vec::new();
    for (i, c) in copper.iter().enumerate() {
        let Some(bounds) = c.shape.bounds() else { continue };
        for &j in index.query_unique(bounds) {
            if j <= i {
                continue;
            }
            let Some(other) = copper.get(j) else { continue };
            if !c.layers.iter().any(|l| other.layers.contains(l)) {
                continue;
            }
            if c.shape.gap(&other.shape) <= 0.0 {
                uf.union(i, j);
                contacts.push((i, j));
            }
        }
    }

    let mut islands: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..copper.len() {
        let root = uf.find(i);
        islands.entry(root).or_default().push(i);
    }

    let mut result = PcbConnectivity::default();
    // Shorts: an island carrying more than one net.
    for members in islands.values() {
        let nets: BTreeSet<&str> = members.iter().filter_map(|&m| copper.get(m)?.net.as_deref()).collect();
        if nets.len() > 1 {
            let location = contacts
                .iter()
                .find(|(a, b)| members.contains(a) && copper.get(*a).map(|c| &c.net) != copper.get(*b).map(|c| &c.net))
                .and_then(|(a, _)| copper.get(*a))
                .map_or(Point::ZERO, |c| c.anchor);
            result.shorts.push(Short {
                nets: nets.iter().map(|s| s.to_string()).collect(),
                items: members.iter().filter_map(|&m| copper.get(m).map(|c| c.item.clone())).collect(),
                location,
            });
        }
    }

    // Ratsnest: per net, connect its islands with a minimum spanning tree.
    let mut net_islands: BTreeMap<String, BTreeMap<usize, Vec<usize>>> = BTreeMap::new();
    for (root, members) in &islands {
        for &m in members {
            let Some(c) = copper.get(m) else { continue };
            if let Some(net) = &c.net {
                net_islands.entry(net.clone()).or_default().entry(*root).or_default().push(m);
            }
        }
    }
    for (net, groups) in &net_islands {
        let pads = groups.values().flatten().filter(|&&m| copper.get(m).is_some_and(|c| matches!(c.item, CopperRef::Pad { .. }))).count();
        let pad_groups: Vec<&Vec<usize>> = groups.values().collect();
        let n = pad_groups.len();
        let mut lines = Vec::new();
        if n > 1 {
            // Prim's algorithm on islands, edge weight = closest anchor pair.
            let closest = |a: &Vec<usize>, b: &Vec<usize>| -> Option<(f64, usize, usize)> {
                let mut best: Option<(f64, usize, usize)> = None;
                for &x in a {
                    for &y in b {
                        let (Some(cx), Some(cy)) = (copper.get(x), copper.get(y)) else { continue };
                        let d = cx.anchor.distance(cy.anchor);
                        if best.is_none_or(|(bd, _, _)| d < bd) {
                            best = Some((d, x, y));
                        }
                    }
                }
                best
            };
            let mut in_tree = vec![false; n];
            if let Some(first) = in_tree.first_mut() {
                *first = true;
            }
            for _ in 1..n {
                let mut best: Option<(f64, usize, usize, usize)> = None;
                for (_, group_a) in pad_groups.iter().enumerate().filter(|(a, _)| in_tree.get(*a).copied().unwrap_or(false)) {
                    for (b, group_b) in pad_groups.iter().enumerate().filter(|(b, _)| !in_tree.get(*b).copied().unwrap_or(true)) {
                        if let Some((d, x, y)) = closest(group_a, group_b)
                            && best.is_none_or(|(bd, ..)| d < bd)
                        {
                            best = Some((d, x, y, b));
                        }
                    }
                }
                let Some((_, x, y, b)) = best else { break };
                if let Some(slot) = in_tree.get_mut(b) {
                    *slot = true;
                }
                if let (Some(cx), Some(cy)) = (copper.get(x), copper.get(y)) {
                    lines.push(Ratsnest {
                        net: net.clone(),
                        from: cx.anchor,
                        to: cy.anchor,
                        from_item: cx.item.clone(),
                        to_item: cy.item.clone(),
                    });
                }
            }
        }
        result.nets.insert(net.clone(), NetRouting { pads, connections: pads.saturating_sub(1), unrouted: lines.len() });
        result.unrouted.extend(lines);
    }
    result.islands = islands.values().map(|m| m.iter().filter_map(|&i| copper.get(i).map(|c| c.item.clone())).collect()).collect();
    result
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use hephaestus_core::{Component, Footprint, PcbObject, Side, Track, Via};

    fn p(x: f64, y: f64) -> Point {
        Point::new(mil(x), mil(y))
    }

    fn board() -> Design {
        let mut d = Design::new("t");
        d.library = hephaestus_libraries::builtin();
        d
    }

    fn resistor(d: &mut Design, name: &str, at: Point, nets: [&str; 2]) -> ObjectId {
        let c = d.alloc_id();
        d.components.insert(
            c,
            Component {
                id: c,
                designator: name.into(),
                value: "1k".into(),
                part: "RES".into(),
                footprint: "0805".into(),
                parameters: Default::default(),
            },
        );
        let f = d.alloc_id();
        d.pcb.insert(
            f,
            PcbObject {
                id: f,
                kind: PcbKind::Footprint(Footprint {
                    component: Some(c),
                    footprint: "0805".into(),
                    position: at,
                    rotation: 0.0,
                    side: Side::Top,
                    locked: false,
                    pad_nets: BTreeMap::from([("1".into(), nets[0].into()), ("2".into(), nets[1].into())]),
                }),
            },
        );
        f
    }

    fn track(d: &mut Design, a: Point, b: Point, layer: Layer, net: &str) -> ObjectId {
        let id = d.alloc_id();
        d.pcb
            .insert(id, PcbObject { id, kind: PcbKind::Track(Track { start: a, end: b, width: mil(10.0), layer, net: Some(net.into()) }) });
        id
    }

    #[test]
    fn unrouted_net_produces_a_ratsnest_line_until_routed() {
        let mut d = board();
        resistor(&mut d, "R1", p(1000.0, 1000.0), ["A", "B"]);
        resistor(&mut d, "R2", p(2000.0, 1000.0), ["B", "C"]);
        let c = analyze(&d);
        assert_eq!(c.unrouted.len(), 1);
        assert_eq!(c.unrouted[0].net, "B");
        // 0805 pads sit at ±0.95 mm (≈37.4 mil).
        let r1_pad2 = Point::new(mil(1000.0) + 950_000, mil(1000.0));
        let r2_pad1 = Point::new(mil(2000.0) - 950_000, mil(1000.0));
        track(&mut d, r1_pad2, r2_pad1, Layer::TopCopper, "B");
        let c = analyze(&d);
        assert!(c.unrouted.is_empty());
        assert!(c.shorts.is_empty());
        assert_eq!(c.nets["B"].pads, 2);
    }

    #[test]
    fn tracks_on_different_layers_connect_only_through_vias() {
        let mut d = board();
        resistor(&mut d, "R1", p(1000.0, 1000.0), ["A", "N"]);
        resistor(&mut d, "R2", p(2000.0, 1000.0), ["N", "C"]);
        let r1_pad2 = Point::new(mil(1000.0) + 950_000, mil(1000.0));
        let r2_pad1 = Point::new(mil(2000.0) - 950_000, mil(1000.0));
        let mid = p(1500.0, 1000.0);
        track(&mut d, r1_pad2, mid, Layer::TopCopper, "N");
        track(&mut d, mid, r2_pad1, Layer::BottomCopper, "N");
        // The tracks meet on different layers and the bottom one ends on a top-only SMD pad:
        // three islands of net N, two ratsnest lines.
        assert_eq!(analyze(&d).unrouted.len(), 2);
        let v = d.alloc_id();
        d.pcb.insert(
            v,
            PcbObject {
                id: v,
                kind: PcbKind::Via(Via {
                    position: mid,
                    diameter: mil(24.0),
                    drill: mil(12.0),
                    net: Some("N".into()),
                    from: Layer::TopCopper,
                    to: Layer::BottomCopper,
                }),
            },
        );
        let near_r2 = r2_pad1 - p(100.0, 0.0);
        d.pcb.retain(|_, o| !matches!(&o.kind, PcbKind::Track(t) if t.layer == Layer::BottomCopper));
        track(&mut d, mid, near_r2, Layer::BottomCopper, "N");
        let v2 = d.alloc_id();
        d.pcb.insert(
            v2,
            PcbObject {
                id: v2,
                kind: PcbKind::Via(Via {
                    position: near_r2,
                    diameter: mil(24.0),
                    drill: mil(12.0),
                    net: Some("N".into()),
                    from: Layer::TopCopper,
                    to: Layer::BottomCopper,
                }),
            },
        );
        track(&mut d, near_r2, r2_pad1, Layer::TopCopper, "N");
        let c = analyze(&d);
        assert!(c.unrouted.is_empty(), "{:?}", c.unrouted);
    }

    #[test]
    fn touching_different_nets_is_a_short() {
        let mut d = board();
        resistor(&mut d, "R1", p(1000.0, 1000.0), ["A", "B"]);
        let pad1 = Point::new(mil(1000.0) - 950_000, mil(1000.0));
        let pad2 = Point::new(mil(1000.0) + 950_000, mil(1000.0));
        track(&mut d, pad1, pad2, Layer::TopCopper, "A");
        let c = analyze(&d);
        assert_eq!(c.shorts.len(), 1);
        assert_eq!(c.shorts[0].nets, vec!["A".to_string(), "B".to_string()]);
    }
}
