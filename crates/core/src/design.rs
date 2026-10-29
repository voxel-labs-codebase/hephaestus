use std::collections::BTreeMap;

use hephaestus_geometry::{Coord, Point, Rect, Shape, Transform, mil};
use serde::{Deserialize, Serialize};

use crate::schematic::Sheet;
use crate::{
    BoardSettings, Component, Footprint, FootprintDef, Layer, Library, ObjectId, PadKind, PadShape, PartDef, PcbKind, PcbObject, PinType,
    SchKind, SchObject, SheetSize, Side, SymbolDef, SymbolInstance,
};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DesignError {
    #[error("object {0} does not exist")]
    UnknownObject(ObjectId),
    #[error("{0}")]
    Invalid(String),
}

/// The whole engineering database of one project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Design {
    pub name: String,
    /// The next unallocated [`ObjectId`].
    pub next_id: u64,
    /// The project library: every part, symbol and footprint the design uses.
    pub library: Library,
    pub components: BTreeMap<ObjectId, Component>,
    pub sheets: BTreeMap<ObjectId, Sheet>,
    pub schematic: BTreeMap<ObjectId, SchObject>,
    pub board: BoardSettings,
    pub pcb: BTreeMap<ObjectId, PcbObject>,
}

/// A borrowed view of any object by id.
#[derive(Clone, Copy, Debug)]
pub enum ObjectRef<'a> {
    Component(&'a Component),
    Sheet(&'a Sheet),
    Sch(&'a SchObject),
    Pcb(&'a PcbObject),
}

/// A pin of a placed symbol, in sheet coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PinGeometry {
    pub symbol: ObjectId,
    pub component: ObjectId,
    pub number: String,
    pub name: String,
    pub pin_type: PinType,
    /// Electrical end (hot spot).
    pub position: Point,
    /// End touching the symbol body.
    pub body: Point,
    /// Outward direction, degrees.
    pub angle: f64,
}

/// A pad of a placed footprint, in board coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PadGeometry {
    pub footprint: ObjectId,
    pub component: Option<ObjectId>,
    pub number: String,
    pub center: Point,
    pub width: Coord,
    pub height: Coord,
    pub rotation: f64,
    pub pad_shape: PadShape,
    pub shape: Shape,
    /// The copper layers the pad occupies.
    pub layers: Vec<Layer>,
    pub drill: Option<Coord>,
    pub plated: bool,
    pub net: Option<String>,
}

impl Design {
    /// An empty project with one A3 sheet and a 3000 × 2000 mil two-layer board.
    pub fn new(name: impl Into<String>) -> Self {
        let mut design = Design {
            name: name.into(),
            next_id: 1,
            library: Library::new("Project Library"),
            components: BTreeMap::new(),
            sheets: BTreeMap::new(),
            schematic: BTreeMap::new(),
            board: BoardSettings::default(),
            pcb: BTreeMap::new(),
        };
        let id = design.alloc_id();
        design.sheets.insert(id, Sheet::new(id, "Sheet1", SheetSize::A3, 0));
        design
    }

    pub fn alloc_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        id
    }

    pub fn object(&self, id: ObjectId) -> Option<ObjectRef<'_>> {
        if let Some(c) = self.components.get(&id) {
            return Some(ObjectRef::Component(c));
        }
        if let Some(o) = self.schematic.get(&id) {
            return Some(ObjectRef::Sch(o));
        }
        if let Some(o) = self.pcb.get(&id) {
            return Some(ObjectRef::Pcb(o));
        }
        self.sheets.get(&id).map(ObjectRef::Sheet)
    }

    pub fn sheets_in_order(&self) -> Vec<&Sheet> {
        let mut sheets: Vec<&Sheet> = self.sheets.values().collect();
        sheets.sort_by_key(|s| (s.order, s.id));
        sheets
    }

    pub fn sheet_objects(&self, sheet: ObjectId) -> impl Iterator<Item = &SchObject> {
        self.schematic.values().filter(move |o| o.sheet == sheet)
    }

    pub fn component_by_designator(&self, designator: &str) -> Option<&Component> {
        self.components.values().find(|c| c.designator.eq_ignore_ascii_case(designator))
    }

    /// The symbols placed for a component (several for multi-part components).
    pub fn symbols_of(&self, component: ObjectId) -> impl Iterator<Item = (&SchObject, &SymbolInstance)> {
        self.schematic.values().filter_map(move |o| match &o.kind {
            SchKind::Symbol(s) if s.component == component => Some((o, s)),
            _ => None,
        })
    }

    pub fn footprint_of(&self, component: ObjectId) -> Option<(&PcbObject, &Footprint)> {
        self.pcb.values().find_map(|o| match &o.kind {
            PcbKind::Footprint(f) if f.component == Some(component) => Some((o, f)),
            _ => None,
        })
    }

    pub fn part_of(&self, component: &Component) -> Option<&PartDef> {
        self.library.parts.get(&component.part)
    }

    pub fn symbol_def_of(&self, component: &Component) -> Option<&SymbolDef> {
        self.part_of(component).and_then(|p| self.library.symbols.get(&p.symbol))
    }

    pub fn footprint_def(&self, footprint: &Footprint) -> Option<&FootprintDef> {
        self.library.footprints.get(&footprint.footprint)
    }

    /// The next free designator with `prefix`, e.g. `R4` after R1..R3.
    pub fn next_designator(&self, prefix: &str) -> String {
        let used = self
            .components
            .values()
            .filter_map(|c| c.designator.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("{prefix}{}", used + 1)
    }

    pub fn symbol_transform(instance: &SymbolInstance) -> Transform {
        Transform::new(instance.position, instance.rotation, instance.mirror)
    }

    pub fn footprint_transform(footprint: &Footprint) -> Transform {
        Transform::new(footprint.position, footprint.rotation, footprint.side == Side::Bottom)
    }

    /// Pins of a placed symbol object (empty for other objects or unresolved library refs).
    pub fn pins(&self, object: &SchObject) -> Vec<PinGeometry> {
        let SchKind::Symbol(instance) = &object.kind else { return Vec::new() };
        let Some(component) = self.components.get(&instance.component) else { return Vec::new() };
        let Some(symbol) = self.symbol_def_of(component) else { return Vec::new() };
        let t = Self::symbol_transform(instance);
        symbol
            .pins
            .iter()
            .map(|pin| PinGeometry {
                symbol: object.id,
                component: component.id,
                number: pin.number.clone(),
                name: pin.name.clone(),
                pin_type: pin.pin_type,
                position: t.apply(pin.position),
                body: t.apply(pin.body_end()),
                angle: t.apply_angle(pin.direction.degrees()),
            })
            .collect()
    }

    pub fn sheet_pins(&self, sheet: ObjectId) -> Vec<PinGeometry> {
        self.sheet_objects(sheet).flat_map(|o| self.pins(o)).collect()
    }

    /// Pads of a placed footprint (empty for other objects or unresolved library refs).
    pub fn pads(&self, object: &PcbObject) -> Vec<PadGeometry> {
        let PcbKind::Footprint(footprint) = &object.kind else { return Vec::new() };
        let Some(def) = self.footprint_def(footprint) else { return Vec::new() };
        let t = Self::footprint_transform(footprint);
        let copper = self.board.stack.copper_layers();
        def.pads
            .iter()
            .map(|pad| {
                let center = t.apply(pad.position);
                let rotation =
                    if footprint.side == Side::Bottom { -pad.rotation + footprint.rotation } else { pad.rotation + footprint.rotation };
                let shape = match pad.shape {
                    PadShape::Round => Shape::circle(center, pad.width.min(pad.height)),
                    PadShape::Rect => Shape::rect(center, pad.width, pad.height, rotation),
                    PadShape::RoundRect { ratio } => {
                        let r = (pad.width.min(pad.height) as f64 * ratio.clamp(0.0, 0.5)).round() as Coord;
                        Shape::rounded_rect(center, pad.width, pad.height, r, rotation)
                    }
                    PadShape::Oval => Shape::obround(center, pad.width, pad.height, rotation),
                };
                let layers = match pad.kind {
                    PadKind::Smd => vec![if footprint.side == Side::Top { Layer::TopCopper } else { Layer::BottomCopper }],
                    PadKind::ThroughHole { .. } | PadKind::NonPlated { .. } => copper.clone(),
                };
                PadGeometry {
                    footprint: object.id,
                    component: footprint.component,
                    number: pad.number.clone(),
                    center,
                    width: pad.width,
                    height: pad.height,
                    rotation,
                    pad_shape: pad.shape,
                    shape,
                    layers,
                    drill: pad.kind.drill(),
                    plated: !matches!(pad.kind, PadKind::NonPlated { .. }),
                    net: footprint.pad_nets.get(&pad.number).cloned(),
                }
            })
            .collect()
    }

    pub fn all_pads(&self) -> Vec<PadGeometry> {
        self.pcb.values().flat_map(|o| self.pads(o)).collect()
    }

    /// Sheet-space bounds of a schematic object, for selection and hit testing.
    pub fn sch_bounds(&self, object: &SchObject) -> Rect {
        let small = mil(50.0);
        match &object.kind {
            SchKind::Symbol(instance) => {
                let local = self
                    .components
                    .get(&instance.component)
                    .and_then(|c| self.symbol_def_of(c))
                    .map(|s| s.bounds())
                    .unwrap_or(Rect::from_center(Point::ZERO, small * 2, small * 2));
                let t = Self::symbol_transform(instance);
                let corners = [local.min, local.max, Point::new(local.min.x, local.max.y), Point::new(local.max.x, local.min.y)];
                Rect::bounding(corners.into_iter().map(|p| t.apply(p))).unwrap_or(local)
            }
            SchKind::Wire(w) => Rect::bounding(w.points.iter().copied()).unwrap_or(Rect::from_center(object.anchor(), 0, 0)),
            SchKind::Bus(b) => Rect::bounding(b.points.iter().copied()).unwrap_or(Rect::from_center(object.anchor(), 0, 0)),
            SchKind::BusEntry(e) => Rect::from_corners(e.start, e.end),
            SchKind::Junction(j) => Rect::from_center(j.position, mil(25.0), mil(25.0)),
            SchKind::NoConnect(n) => Rect::from_center(n.position, mil(40.0), mil(40.0)),
            SchKind::NetLabel(l) => text_bounds(l.position, &l.text, mil(80.0), l.rotation),
            SchKind::PowerPort(p) => Rect::from_center(p.position, mil(150.0), mil(150.0)).union(Rect::from_center(
                p.position + Point::new(0, mil(120.0)).rotated(p.rotation - 90.0),
                mil(200.0),
                mil(100.0),
            )),
            SchKind::Port(p) => {
                let end = p.position + Point::new(p.width, 0).rotated(p.rotation);
                Rect::from_corners(p.position, end).inflated(mil(50.0))
            }
            SchKind::Text(t) => text_bounds(t.position, &t.text, t.size, t.rotation),
            SchKind::SheetSymbol(s) => Rect::from_corners(s.position, s.position + Point::new(s.width, -s.height)),
            SchKind::Graphic(g) => g.bounds().unwrap_or(Rect::from_center(object.anchor(), small, small)),
        }
    }

    /// Board-space bounds of a PCB object.
    pub fn pcb_bounds(&self, object: &PcbObject) -> Rect {
        match &object.kind {
            PcbKind::Footprint(f) => {
                let pads = self.pads(object).into_iter().filter_map(|p| p.shape.bounds());
                let court = self.footprint_def(f).map(|d| {
                    let t = Self::footprint_transform(f);
                    let c = d.courtyard;
                    Rect::bounding(
                        [c.min, c.max, Point::new(c.min.x, c.max.y), Point::new(c.max.x, c.min.y)].into_iter().map(|p| t.apply(p)),
                    )
                    .unwrap_or(c)
                });
                pads.chain(court).reduce(Rect::union).unwrap_or(Rect::from_center(f.position, mil(20.0), mil(20.0)))
            }
            PcbKind::Track(t) => Rect::from_corners(t.start, t.end).inflated(t.width / 2),
            PcbKind::Via(v) => Rect::from_center(v.position, v.diameter / 2, v.diameter / 2),
            PcbKind::Zone(z) => Rect::bounding(z.outline.iter().copied()).unwrap_or(Rect::from_center(Point::ZERO, 0, 0)),
            PcbKind::Keepout(k) => Rect::bounding(k.outline.iter().copied()).unwrap_or(Rect::from_center(Point::ZERO, 0, 0)),
            PcbKind::Text(t) => text_bounds(t.position, &t.text, t.height, t.rotation),
            PcbKind::Line(l) => Rect::from_corners(l.start, l.end).inflated(l.width / 2),
        }
    }

    /// Structural problems: references to objects or library items that don't exist, duplicate
    /// designators. Empty for a consistent design. Commands refuse to commit a transaction that
    /// adds problems.
    pub fn integrity_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut designators: BTreeMap<String, ObjectId> = BTreeMap::new();
        for c in self.components.values() {
            if let Some(other) = designators.insert(c.designator.to_ascii_uppercase(), c.id) {
                problems.push(format!("duplicate designator {} ({} and {})", c.designator, other, c.id));
            }
            match self.library.parts.get(&c.part) {
                None => problems.push(format!("{} uses part {} missing from the project library", c.designator, c.part)),
                Some(part) => {
                    if !self.library.footprints.contains_key(&c.footprint) {
                        problems.push(format!("{} uses footprint {} missing from the project library", c.designator, c.footprint));
                    }
                    if !self.library.symbols.contains_key(&part.symbol) {
                        problems.push(format!("part {} uses missing symbol {}", part.name, part.symbol));
                    }
                }
            }
        }
        for o in self.schematic.values() {
            if !self.sheets.contains_key(&o.sheet) {
                problems.push(format!("{} is on missing sheet {}", o.id, o.sheet));
            }
            match &o.kind {
                SchKind::Symbol(s) if !self.components.contains_key(&s.component) => {
                    problems.push(format!("symbol {} references missing component {}", o.id, s.component));
                }
                SchKind::SheetSymbol(s) if !self.sheets.contains_key(&s.child) => {
                    problems.push(format!("sheet symbol {} references missing sheet {}", o.id, s.child));
                }
                _ => {}
            }
        }
        let mut placed: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
        for o in self.pcb.values() {
            if let PcbKind::Footprint(f) = &o.kind {
                if !self.library.footprints.contains_key(&f.footprint) {
                    problems.push(format!("footprint {} uses {} missing from the project library", o.id, f.footprint));
                }
                if let Some(c) = f.component {
                    if !self.components.contains_key(&c) {
                        problems.push(format!("footprint {} references missing component {}", o.id, c));
                    } else if let Some(other) = placed.insert(c, o.id) {
                        problems.push(format!("component {c} has two footprints ({other} and {})", o.id));
                    }
                }
            }
            for layer in object_copper_layers(o) {
                if !self.board.stack.contains(layer) {
                    problems.push(format!("{} is on {layer}, which is not in the layer stack", o.id));
                }
            }
        }
        if self.board.outline.len() < 3 {
            problems.push("board outline needs at least three vertices".into());
        }
        let max_id = self.components.keys().chain(self.sheets.keys()).chain(self.schematic.keys()).chain(self.pcb.keys()).max();
        if let Some(max) = max_id
            && max.0 >= self.next_id
        {
            problems.push(format!("id counter {} is not above the largest id {max}", self.next_id));
        }
        problems
    }
}

fn object_copper_layers(object: &PcbObject) -> Vec<Layer> {
    match &object.kind {
        PcbKind::Track(t) if t.layer.is_copper() => vec![t.layer],
        PcbKind::Zone(z) if z.layer.is_copper() => vec![z.layer],
        PcbKind::Via(v) => vec![v.from, v.to],
        _ => Vec::new(),
    }
}

/// Approximate text extent: 0.6 em per character.
fn text_bounds(position: Point, text: &str, size: Coord, rotation: f64) -> Rect {
    let width = size * 6 / 10 * text.chars().count().max(1) as Coord;
    let corners = [Point::ZERO, Point::new(width, 0), Point::new(width, size), Point::new(0, size)];
    Rect::bounding(corners.into_iter().map(|c| c.rotated(rotation) + position)).unwrap_or(Rect::from_center(position, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{PadDef, PinDef, PinDirection};
    use crate::schematic::Wire;

    fn tiny_library() -> Library {
        let mut lib = Library::new("test");
        lib.symbols.insert(
            "R".into(),
            SymbolDef {
                name: "R".into(),
                description: String::new(),
                pins: vec![
                    PinDef {
                        number: "1".into(),
                        name: "1".into(),
                        pin_type: PinType::Passive,
                        position: Point::new(-mil(300.0), 0),
                        direction: PinDirection::Left,
                        length: mil(100.0),
                        show_name: false,
                        show_number: false,
                    },
                    PinDef {
                        number: "2".into(),
                        name: "2".into(),
                        pin_type: PinType::Passive,
                        position: Point::new(mil(300.0), 0),
                        direction: PinDirection::Right,
                        length: mil(100.0),
                        show_name: false,
                        show_number: false,
                    },
                ],
                graphics: vec![],
                designator_position: Point::new(0, mil(100.0)),
                value_position: Point::new(0, -mil(100.0)),
                hide_pin_names: true,
            },
        );
        let pad = |n: &str, x: f64| PadDef {
            number: n.into(),
            position: Point::new(mil(x), 0),
            width: mil(40.0),
            height: mil(40.0),
            shape: PadShape::Rect,
            rotation: 0.0,
            kind: PadKind::Smd,
        };
        lib.footprints.insert(
            "0603".into(),
            FootprintDef {
                name: "0603".into(),
                description: String::new(),
                pads: vec![pad("1", -30.0), pad("2", 30.0)],
                graphics: vec![],
                courtyard: Rect::from_center(Point::ZERO, mil(60.0), mil(30.0)),
                model: None,
            },
        );
        lib.parts.insert(
            "RES".into(),
            PartDef {
                description: "Resistor".into(),
                category: "Passives".into(),
                default_value: "10k".into(),
                ..PartDef::new("RES", "R", "0603", "R")
            },
        );
        lib
    }

    fn design_with_resistor() -> (Design, ObjectId, ObjectId, ObjectId) {
        let mut d = Design::new("t");
        d.library = tiny_library();
        let sheet = *d.sheets.keys().next().unwrap();
        let c = d.alloc_id();
        d.components.insert(
            c,
            Component {
                id: c,
                designator: "R1".into(),
                value: "10k".into(),
                part: "RES".into(),
                footprint: "0603".into(),
                parameters: BTreeMap::new(),
            },
        );
        let s = d.alloc_id();
        d.schematic.insert(
            s,
            SchObject {
                id: s,
                sheet,
                kind: SchKind::Symbol(SymbolInstance {
                    component: c,
                    position: Point::new(mil(1000.0), mil(1000.0)),
                    rotation: 90.0,
                    mirror: false,
                    designator_offset: None,
                    value_offset: None,
                }),
            },
        );
        let f = d.alloc_id();
        d.pcb.insert(
            f,
            PcbObject {
                id: f,
                kind: PcbKind::Footprint(Footprint {
                    component: Some(c),
                    footprint: "0603".into(),
                    position: Point::new(mil(500.0), mil(500.0)),
                    rotation: 0.0,
                    side: Side::Bottom,
                    locked: false,
                    pad_nets: BTreeMap::from([("1".to_string(), "VCC".to_string())]),
                }),
            },
        );
        (d, c, s, f)
    }

    #[test]
    fn pins_follow_symbol_rotation() {
        let (d, _, s, _) = design_with_resistor();
        let pins = d.pins(&d.schematic[&s]);
        assert_eq!(pins.len(), 2);
        assert_eq!(pins[0].position, Point::new(mil(1000.0), mil(700.0)));
        assert_eq!(pins[0].angle, 270.0);
        assert_eq!(pins[1].body, Point::new(mil(1000.0), mil(1200.0)));
    }

    #[test]
    fn bottom_side_pads_mirror_and_flip_layers() {
        let (d, _, _, f) = design_with_resistor();
        let pads = d.pads(&d.pcb[&f]);
        assert_eq!(pads[0].center, Point::new(mil(530.0), mil(500.0)));
        assert_eq!(pads[0].layers, vec![Layer::BottomCopper]);
        assert_eq!(pads[0].net.as_deref(), Some("VCC"));
        assert_eq!(pads[1].net, None);
    }

    #[test]
    fn integrity_detects_dangling_references_and_duplicates() {
        let (mut d, c, _, _) = design_with_resistor();
        assert!(d.integrity_problems().is_empty(), "{:?}", d.integrity_problems());
        let mut dup = d.components[&c].clone();
        dup.id = d.alloc_id();
        dup.designator = "r1".into();
        d.components.insert(dup.id, dup);
        let wire = d.alloc_id();
        d.schematic.insert(wire, SchObject { id: wire, sheet: ObjectId(999), kind: SchKind::Wire(Wire { points: vec![] }) });
        let problems = d.integrity_problems();
        assert!(problems.iter().any(|p| p.contains("duplicate designator")));
        assert!(problems.iter().any(|p| p.contains("missing sheet")));
    }

    #[test]
    fn design_round_trips_through_json() {
        let (d, ..) = design_with_resistor();
        let json = serde_json::to_string_pretty(&d).unwrap();
        let back: Design = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        assert!(json.contains("\"type\": \"Symbol\""));
    }

    #[test]
    fn next_designator_skips_used_numbers() {
        let (d, ..) = design_with_resistor();
        assert_eq!(d.next_designator("R"), "R2");
        assert_eq!(d.next_designator("C"), "C1");
    }
}
