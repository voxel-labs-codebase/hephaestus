//! Schematic netlist extraction.
//!
//! Connection rules, following Altium Designer:
//! - Wire vertices, pin hot spots, net labels, power ports, ports and junctions that coincide
//!   connect.
//! - A wire vertex, pin hot spot or label landing anywhere on a wire segment connects to it
//!   (a "T" connection, drawn with an automatic junction).
//! - Wires that merely cross do not connect unless a junction sits on the crossing.
//! - Net labels, power ports and ports are global: equal names connect across sheets.
//! - Unnamed nets are named `Net<designator>_<pin>` after their first pin.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use hephaestus_core::geometry::{GridIndex, Point, Rect, mil};
use hephaestus_core::{Design, ObjectId, PinType, SchKind};
use serde::Serialize;

use crate::union_find::UnionFind;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NetNode {
    pub component: ObjectId,
    pub designator: String,
    pub pin: String,
    pub pin_name: String,
    pub pin_type: PinType,
    pub symbol: ObjectId,
    pub sheet: ObjectId,
    pub position: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Net {
    pub name: String,
    /// Whether a label, power port or port named the net (otherwise the name is automatic).
    pub named: bool,
    /// Every distinct name attached; more than one is a naming conflict.
    pub names: Vec<String>,
    /// Pins, sorted by designator then pin number.
    pub nodes: Vec<NetNode>,
    /// Wires, labels, power ports, ports and junctions that make up the net.
    pub objects: Vec<ObjectId>,
    pub sheets: Vec<ObjectId>,
    /// The net has a power port (supply or ground).
    pub power: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Netlist {
    pub nets: Vec<Net>,
    /// Pins touching nothing (excluding pins marked with a no-connect).
    pub unconnected_pins: Vec<NetNode>,
    /// Pins carrying a no-connect marker, and whether something is connected anyway.
    pub no_connect_pins: Vec<(NetNode, bool)>,
    /// Net labels, power ports and ports that touch no wire or pin.
    pub floating_objects: Vec<ObjectId>,
    /// Wire ends touching nothing: (sheet, wire, point).
    pub dangling_wire_ends: Vec<(ObjectId, ObjectId, Point)>,
    /// Points that get an automatic junction dot, per sheet.
    pub junctions: BTreeMap<ObjectId, Vec<Point>>,
    #[serde(skip)]
    pin_index: HashMap<(ObjectId, String), usize>,
    #[serde(skip)]
    object_index: HashMap<ObjectId, usize>,
}

impl Netlist {
    pub fn net(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }

    /// The net of a component pin.
    pub fn net_of_pin(&self, component: ObjectId, pin: &str) -> Option<&Net> {
        self.pin_index.get(&(component, pin.to_string())).and_then(|&i| self.nets.get(i))
    }

    /// The net a wire, label or port belongs to.
    pub fn net_of_object(&self, object: ObjectId) -> Option<&Net> {
        self.object_index.get(&object).and_then(|&i| self.nets.get(i))
    }

    /// Pad number → net name for one component, using the part's pin map.
    pub fn pad_nets(&self, design: &Design, component: ObjectId) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let Some(c) = design.components.get(&component) else { return out };
        let Some(part) = design.part_of(c) else { return out };
        let Some(symbol) = design.library.symbols.get(&part.symbol) else { return out };
        for pin in &symbol.pins {
            if let Some(net) = self.net_of_pin(component, &pin.number) {
                for pad in part.pads_for_pin(&pin.number) {
                    out.insert(pad.to_string(), net.name.clone());
                }
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
enum ItemKind {
    Wire(ObjectId),
    Pin(NetNode),
    Label(ObjectId, String),
    Power(ObjectId, String),
    Port(ObjectId, String),
    Junction(ObjectId),
    NoConnect,
}

#[derive(Clone, Debug)]
struct Item {
    sheet: ObjectId,
    kind: ItemKind,
    /// Wire vertices, or the single hot spot.
    points: Vec<Point>,
}

fn on_segment(p: Point, a: Point, b: Point) -> bool {
    let cross = (b.x - a.x) as i128 * (p.y - a.y) as i128 - (b.y - a.y) as i128 * (p.x - a.x) as i128;
    cross == 0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

fn natural_key(designator: &str) -> (String, u64) {
    let split = designator.find(|c: char| c.is_ascii_digit()).unwrap_or(designator.len());
    let (prefix, number) = designator.split_at(split);
    (prefix.to_ascii_uppercase(), number.parse().unwrap_or(u64::MAX))
}

fn collect_items(design: &Design) -> Vec<Item> {
    let mut items = Vec::new();
    for object in design.schematic.values() {
        let sheet = object.sheet;
        let single = |kind, p| Item { sheet, kind, points: vec![p] };
        match &object.kind {
            SchKind::Wire(w) if w.points.len() >= 2 => {
                items.push(Item { sheet, kind: ItemKind::Wire(object.id), points: w.points.clone() })
            }
            SchKind::Symbol(_) => {
                let designator = |c: ObjectId| design.components.get(&c).map(|c| c.designator.clone()).unwrap_or_default();
                for pin in design.pins(object) {
                    let node = NetNode {
                        designator: designator(pin.component),
                        component: pin.component,
                        pin: pin.number,
                        pin_name: pin.name,
                        pin_type: pin.pin_type,
                        symbol: object.id,
                        sheet,
                        position: pin.position,
                    };
                    items.push(single(ItemKind::Pin(node), pin.position));
                }
            }
            SchKind::NetLabel(l) if !l.text.trim().is_empty() => {
                items.push(single(ItemKind::Label(object.id, l.text.trim().into()), l.position))
            }
            SchKind::PowerPort(p) if !p.net.trim().is_empty() => {
                items.push(single(ItemKind::Power(object.id, p.net.trim().into()), p.position))
            }
            SchKind::Port(p) if !p.name.trim().is_empty() => {
                items.push(single(ItemKind::Port(object.id, p.name.trim().into()), p.position))
            }
            SchKind::Junction(j) => items.push(single(ItemKind::Junction(object.id), j.position)),
            SchKind::NoConnect(n) => items.push(single(ItemKind::NoConnect, n.position)),
            SchKind::SheetSymbol(s) => {
                for entry in &s.entries {
                    items.push(single(ItemKind::Port(object.id, entry.name.trim().into()), s.position + entry.offset));
                }
            }
            _ => {}
        }
    }
    items
}

/// Compile every sheet into a netlist.
pub fn compile(design: &Design) -> Netlist {
    let items = collect_items(design);
    let mut uf = UnionFind::new(items.len());

    // Coincident points connect (no-connect markers are excluded: they mark, not join).
    let mut at_point: HashMap<(ObjectId, Point), Vec<usize>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        if matches!(item.kind, ItemKind::NoConnect) {
            continue;
        }
        for &p in &item.points {
            at_point.entry((item.sheet, p)).or_default().push(i);
        }
    }
    for members in at_point.values() {
        if let Some((&first, rest)) = members.split_first() {
            for &m in rest {
                uf.union(first, m);
            }
        }
    }

    // Anything landing on a wire segment connects to that wire.
    let mut segments: GridIndex<(usize, Point, Point)> = GridIndex::new(mil(500.0));
    for (i, item) in items.iter().enumerate() {
        if let ItemKind::Wire(_) = item.kind {
            for pair in item.points.windows(2) {
                if let [a, b] = pair {
                    segments.insert((i, *a, *b), Rect::from_corners(*a, *b));
                }
            }
        }
    }
    // (sheet, point) → how many wire ends, pass-throughs and pins meet there, for junction dots.
    let mut degree: HashMap<(ObjectId, Point), usize> = HashMap::new();
    let mut touched_mid_segment: HashSet<(usize, Point)> = HashSet::new();
    // Wires carrying a net label or port: a short stub ending at nothing is normal for them.
    let mut labelled_wires: HashSet<usize> = HashSet::new();
    for (i, item) in items.iter().enumerate() {
        if matches!(item.kind, ItemKind::NoConnect) {
            continue;
        }
        for (k, &p) in item.points.iter().enumerate() {
            let weight = match item.kind {
                ItemKind::Wire(_) if k == 0 || k + 1 == item.points.len() => 1,
                ItemKind::Wire(_) => 2,
                ItemKind::Pin(_) => 1,
                _ => 0,
            };
            *degree.entry((item.sheet, p)).or_default() += weight;
            for &(w, a, b) in segments.query_point(p, 0) {
                let Some(wire) = items.get(w) else { continue };
                if w == i || wire.sheet != item.sheet || !on_segment(p, a, b) {
                    continue;
                }
                uf.union(i, w);
                if matches!(item.kind, ItemKind::Label(..) | ItemKind::Port(..) | ItemKind::Power(..)) {
                    labelled_wires.insert(w);
                }
                if p != a && p != b && touched_mid_segment.insert((w, p)) {
                    *degree.entry((item.sheet, p)).or_default() += 2;
                }
            }
        }
    }

    // Global names: equal labels, power ports and ports connect across sheets.
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        if let ItemKind::Label(_, n) | ItemKind::Power(_, n) | ItemKind::Port(_, n) = &item.kind {
            match by_name.get(n) {
                Some(&first) => uf.union(first, i),
                None => {
                    by_name.insert(n.clone(), i);
                }
            }
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..items.len() {
        let root = uf.find(i);
        groups.entry(root).or_default().push(i);
    }

    let mut netlist = Netlist::default();
    for members in groups.values() {
        let mut nodes = Vec::new();
        let mut objects = BTreeSet::new();
        let mut sheets = BTreeSet::new();
        let mut power_names = BTreeSet::new();
        let mut label_names = BTreeSet::new();
        let mut port_names = BTreeSet::new();
        let mut wires = 0;
        for &m in members {
            let Some(item) = items.get(m) else { continue };
            sheets.insert(item.sheet);
            match &item.kind {
                ItemKind::Pin(node) => nodes.push(node.clone()),
                ItemKind::Wire(id) => {
                    wires += 1;
                    objects.insert(*id);
                }
                ItemKind::Junction(id) => {
                    objects.insert(*id);
                }
                ItemKind::NoConnect => {}
                ItemKind::Power(id, n) => {
                    objects.insert(*id);
                    power_names.insert(n.clone());
                }
                ItemKind::Label(id, n) => {
                    objects.insert(*id);
                    label_names.insert(n.clone());
                }
                ItemKind::Port(id, n) => {
                    objects.insert(*id);
                    port_names.insert(n.clone());
                }
            }
        }
        nodes.sort_by(|a, b| {
            natural_key(&a.designator).cmp(&natural_key(&b.designator)).then(natural_key(&a.pin).cmp(&natural_key(&b.pin)))
        });
        let named_objects = power_names.len() + label_names.len() + port_names.len();
        if nodes.is_empty() {
            if wires == 0 && named_objects > 0 && members.len() == 1 {
                netlist.floating_objects.extend(objects.iter().copied());
            }
            continue;
        }
        if nodes.len() == 1 && wires == 0 && named_objects == 0 {
            netlist.unconnected_pins.extend(nodes);
            continue;
        }
        let names: Vec<String> =
            power_names.iter().chain(&label_names).chain(&port_names).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        let explicit = power_names.iter().next().or(label_names.iter().next()).or(port_names.iter().next()).cloned();
        let named = explicit.is_some();
        let name = explicit.unwrap_or_else(|| nodes.first().map(|n| format!("Net{}_{}", n.designator, n.pin)).unwrap_or_default());
        netlist.nets.push(Net {
            name,
            named,
            names,
            nodes,
            objects: objects.into_iter().collect(),
            sheets: sheets.into_iter().collect(),
            power: !power_names.is_empty(),
        });
    }
    netlist.nets.sort_by(|a, b| a.name.cmp(&b.name));

    for (i, net) in netlist.nets.iter().enumerate() {
        for node in &net.nodes {
            netlist.pin_index.insert((node.component, node.pin.clone()), i);
        }
        for &o in &net.objects {
            netlist.object_index.insert(o, i);
        }
    }

    // No-connect markers on pins.
    for item in &items {
        let ItemKind::NoConnect = item.kind else { continue };
        let Some(&p) = item.points.first() else { continue };
        for other in &items {
            if let ItemKind::Pin(node) = &other.kind
                && other.sheet == item.sheet
                && other.points.first() == Some(&p)
            {
                let connected = netlist.net_of_pin(node.component, &node.pin).is_some();
                netlist.no_connect_pins.push((node.clone(), connected));
            }
        }
    }
    let marked: HashSet<(ObjectId, String)> = netlist.no_connect_pins.iter().map(|(n, _)| (n.component, n.pin.clone())).collect();
    netlist.unconnected_pins.retain(|n| !marked.contains(&(n.component, n.pin.clone())));

    // Dangling wire ends and junction dots.
    for (index, item) in items.iter().enumerate() {
        let ItemKind::Wire(id) = item.kind else { continue };
        if labelled_wires.contains(&index) {
            continue;
        }
        for p in [item.points.first(), item.points.last()].into_iter().flatten() {
            let others_here = at_point.get(&(item.sheet, *p)).map_or(0, |v| v.len());
            let on_other = segments.query_point(*p, 0).iter().any(|&&(w, a, b)| {
                items.get(w).is_some_and(|wire| !matches!(wire.kind, ItemKind::Wire(x) if x == id) && wire.sheet == item.sheet)
                    && on_segment(*p, a, b)
            });
            if others_here <= 1 && !on_other {
                netlist.dangling_wire_ends.push((item.sheet, id, *p));
            }
        }
    }
    for ((sheet, p), d) in degree {
        if d >= 3 {
            netlist.junctions.entry(sheet).or_default().push(p);
        }
    }
    for points in netlist.junctions.values_mut() {
        points.sort();
    }
    netlist
}

#[cfg(test)]
mod tests {
    use super::*;
    use hephaestus_core::schematic::{Junction, NetLabel, NoConnect, PowerPort, PowerStyle, SymbolInstance, Wire};
    use hephaestus_core::{Component, SchObject};

    struct Fixture {
        design: Design,
        sheet: ObjectId,
    }

    impl Fixture {
        fn new() -> Self {
            let mut design = Design::new("t");
            design.library = hephaestus_libraries::builtin();
            let sheet = *design.sheets.keys().next().unwrap();
            Self { design, sheet }
        }

        fn add(&mut self, kind: SchKind) -> ObjectId {
            let id = self.design.alloc_id();
            self.design.schematic.insert(id, SchObject { id, sheet: self.sheet, kind });
            id
        }

        /// A horizontal resistor at (x, y) mils: pin 1 at x-200, pin 2 at x+200.
        fn resistor(&mut self, designator: &str, x: f64, y: f64) -> ObjectId {
            let id = self.design.alloc_id();
            self.design.components.insert(
                id,
                Component {
                    id,
                    designator: designator.into(),
                    value: "1k".into(),
                    part: "RES".into(),
                    footprint: "0603".into(),
                    parameters: Default::default(),
                },
            );
            self.add(SchKind::Symbol(SymbolInstance {
                component: id,
                position: p(x, y),
                rotation: 0.0,
                mirror: false,
                designator_offset: None,
                value_offset: None,
            }));
            id
        }

        fn wire(&mut self, points: &[(f64, f64)]) -> ObjectId {
            self.add(SchKind::Wire(Wire { points: points.iter().map(|&(x, y)| p(x, y)).collect() }))
        }
    }

    fn p(x: f64, y: f64) -> Point {
        Point::new(mil(x), mil(y))
    }

    #[test]
    fn wire_between_pins_forms_auto_named_net() {
        let mut f = Fixture::new();
        let r1 = f.resistor("R1", 0.0, 0.0);
        let r2 = f.resistor("R2", 1000.0, 0.0);
        f.wire(&[(200.0, 0.0), (800.0, 0.0)]);
        let n = compile(&f.design);
        let net = n.net_of_pin(r1, "2").unwrap();
        assert_eq!(net.name, "NetR1_2");
        assert!(!net.named);
        assert_eq!(net.nodes.len(), 2);
        assert_eq!(n.net_of_pin(r2, "1").unwrap().name, "NetR1_2");
        assert_eq!(n.unconnected_pins.len(), 2);
        assert!(n.dangling_wire_ends.is_empty());
    }

    #[test]
    fn crossing_wires_need_a_junction() {
        let mut f = Fixture::new();
        f.resistor("R1", 0.0, 0.0);
        f.resistor("R2", 500.0, 500.0);
        f.wire(&[(200.0, 0.0), (1000.0, 0.0)]);
        // A vertical wire from R2 pin 2 (700,500) crossing the first wire at (700, 0).
        f.wire(&[(700.0, 500.0), (700.0, -500.0)]);
        let n = compile(&f.design);
        assert!(n.nets.iter().all(|net| net.nodes.len() == 1));
        f.add(SchKind::Junction(Junction { position: p(700.0, 0.0) }));
        let n = compile(&f.design);
        assert!(n.nets.iter().any(|net| net.nodes.len() == 2));
    }

    #[test]
    fn t_connection_gets_a_junction_dot() {
        let mut f = Fixture::new();
        f.resistor("R1", 0.0, 0.0);
        f.wire(&[(200.0, 0.0), (1000.0, 0.0)]);
        f.wire(&[(600.0, 0.0), (600.0, -500.0)]);
        let n = compile(&f.design);
        assert_eq!(n.junctions.get(&f.sheet), Some(&vec![p(600.0, 0.0)]));
        assert_eq!(n.nets.len(), 1);
        assert_eq!(n.nets[0].objects.len(), 2);
    }

    #[test]
    fn labels_and_power_ports_name_and_join_nets() {
        let mut f = Fixture::new();
        let r1 = f.resistor("R1", 0.0, 0.0);
        let r2 = f.resistor("R2", 0.0, 1000.0);
        f.wire(&[(200.0, 0.0), (500.0, 0.0)]);
        f.add(SchKind::NetLabel(NetLabel { position: p(400.0, 0.0), text: "SIG".into(), rotation: 0.0 }));
        f.wire(&[(200.0, 1000.0), (500.0, 1000.0)]);
        f.add(SchKind::NetLabel(NetLabel { position: p(500.0, 1000.0), text: "SIG".into(), rotation: 0.0 }));
        f.add(SchKind::PowerPort(PowerPort {
            position: p(-200.0, 0.0),
            net: "GND".into(),
            style: PowerStyle::PowerGround,
            rotation: 270.0,
        }));
        let n = compile(&f.design);
        let sig = n.net("SIG").unwrap();
        assert!(sig.named);
        assert_eq!(sig.nodes.len(), 2);
        assert_eq!(n.net_of_pin(r2, "2").unwrap().name, "SIG");
        let gnd = n.net_of_pin(r1, "1").unwrap();
        assert_eq!(gnd.name, "GND");
        assert!(gnd.power);
    }

    #[test]
    fn no_connect_markers_suppress_unconnected_pins() {
        let mut f = Fixture::new();
        f.resistor("R1", 0.0, 0.0);
        f.add(SchKind::NoConnect(NoConnect { position: p(-200.0, 0.0) }));
        let n = compile(&f.design);
        assert_eq!(n.unconnected_pins.len(), 1);
        assert_eq!(n.no_connect_pins.len(), 1);
        assert!(!n.no_connect_pins[0].1);
    }

    #[test]
    fn floating_label_and_dangling_wire_are_reported() {
        let mut f = Fixture::new();
        let label = f.add(SchKind::NetLabel(NetLabel { position: p(3000.0, 3000.0), text: "X".into(), rotation: 0.0 }));
        let wire = f.wire(&[(0.0, 2000.0), (500.0, 2000.0)]);
        let n = compile(&f.design);
        assert_eq!(n.floating_objects, vec![label]);
        assert_eq!(n.dangling_wire_ends.len(), 2);
        assert_eq!(n.dangling_wire_ends[0].1, wire);
    }

    #[test]
    fn pad_nets_follow_pin_map() {
        let mut f = Fixture::new();
        let id = f.design.alloc_id();
        f.design.components.insert(
            id,
            Component {
                id,
                designator: "U1".into(),
                value: "AMS1117".into(),
                part: "AMS1117-3.3".into(),
                footprint: "SOT-223".into(),
                parameters: Default::default(),
            },
        );
        f.add(SchKind::Symbol(SymbolInstance {
            component: id,
            position: p(0.0, 0.0),
            rotation: 0.0,
            mirror: false,
            designator_offset: None,
            value_offset: None,
        }));
        let pins = f.design.sheet_pins(f.sheet);
        let vout = pins.iter().find(|p| p.number == "2").unwrap().position;
        f.add(SchKind::PowerPort(PowerPort { position: vout, net: "3V3".into(), style: PowerStyle::Bar, rotation: 90.0 }));
        let n = compile(&f.design);
        let pads = n.pad_nets(&f.design, id);
        assert_eq!(pads.get("2").map(String::as_str), Some("3V3"));
        assert_eq!(pads.get("4").map(String::as_str), Some("3V3"));
    }
}
