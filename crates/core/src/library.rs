//! Library definitions: schematic symbols, PCB footprints and the parts that tie them together.
//!
//! A [`PartDef`] is what a user places: it names a symbol, one or more compatible footprints, a
//! pin-to-pad map and supplier parameters. A project embeds a copy of every library item it uses
//! so it stays self-contained and reproducible.

use std::collections::BTreeMap;

use hephaestus_geometry::{Coord, Point, Rect};
use serde::{Deserialize, Serialize};

use crate::Layer;

/// The electrical type of a pin, used by the electrical rule check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PinType {
    Input,
    Output,
    #[serde(rename = "IO")]
    Bidirectional,
    #[default]
    Passive,
    Power,
    OpenCollector,
    OpenEmitter,
    HiZ,
}

impl PinType {
    pub const ALL: [PinType; 8] = [
        PinType::Input,
        PinType::Output,
        PinType::Bidirectional,
        PinType::Passive,
        PinType::Power,
        PinType::OpenCollector,
        PinType::OpenEmitter,
        PinType::HiZ,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PinType::Input => "Input",
            PinType::Output => "Output",
            PinType::Bidirectional => "I/O",
            PinType::Passive => "Passive",
            PinType::Power => "Power",
            PinType::OpenCollector => "Open Collector",
            PinType::OpenEmitter => "Open Emitter",
            PinType::HiZ => "HiZ",
        }
    }
}

/// Which way a pin points, from the symbol body out to its electrical end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PinDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PinDirection {
    /// The angle of the outward direction, degrees counter-clockwise from +X.
    pub fn degrees(self) -> f64 {
        match self {
            PinDirection::Right => 0.0,
            PinDirection::Up => 90.0,
            PinDirection::Left => 180.0,
            PinDirection::Down => 270.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PinDef {
    pub number: String,
    pub name: String,
    #[serde(default)]
    pub pin_type: PinType,
    /// The electrical end (hot spot) in symbol coordinates.
    pub position: Point,
    pub direction: PinDirection,
    pub length: Coord,
    #[serde(default = "yes")]
    pub show_name: bool,
    #[serde(default = "yes")]
    pub show_number: bool,
}

fn yes() -> bool {
    true
}

impl PinDef {
    /// The end that touches the symbol body.
    pub fn body_end(&self) -> Point {
        let back = Point::new(-self.length, 0).rotated(self.direction.degrees());
        self.position + back
    }
}

/// Symbol and footprint artwork, in the owner's local coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SymbolGraphic {
    Line {
        points: Vec<Point>,
        width: Coord,
    },
    Rect {
        min: Point,
        max: Point,
        width: Coord,
        filled: bool,
    },
    Circle {
        center: Point,
        radius: Coord,
        width: Coord,
        filled: bool,
    },
    /// Counter-clockwise from `start` to `end` degrees.
    Arc {
        center: Point,
        radius: Coord,
        start: f64,
        end: f64,
        width: Coord,
    },
    Polygon {
        points: Vec<Point>,
        width: Coord,
        filled: bool,
    },
    Text {
        position: Point,
        text: String,
        size: Coord,
    },
}

impl SymbolGraphic {
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            SymbolGraphic::Line { points, width } | SymbolGraphic::Polygon { points, width, .. } => {
                Rect::bounding(points.iter().copied()).map(|r| r.inflated(width / 2))
            }
            SymbolGraphic::Rect { min, max, width, .. } => Some(Rect::from_corners(*min, *max).inflated(width / 2)),
            SymbolGraphic::Circle { center, radius, width, .. } | SymbolGraphic::Arc { center, radius, width, .. } => {
                Some(Rect::from_center(*center, radius + width / 2, radius + width / 2))
            }
            SymbolGraphic::Text { position, text, size } => {
                Some(Rect::from_corners(*position, *position + Point::new(size * 6 / 10 * text.chars().count() as Coord, *size)))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub pins: Vec<PinDef>,
    pub graphics: Vec<SymbolGraphic>,
    /// Where the designator and value are drawn by default, in symbol coordinates.
    pub designator_position: Point,
    pub value_position: Point,
    /// Power symbols such as a regulator's GND pin hide nothing; this is for net-tie style parts.
    #[serde(default)]
    pub hide_pin_names: bool,
}

impl SymbolDef {
    pub fn pin(&self, number: &str) -> Option<&PinDef> {
        self.pins.iter().find(|p| p.number == number)
    }

    /// The local bounds of graphics and pins.
    pub fn bounds(&self) -> Rect {
        let graphic = self.graphics.iter().filter_map(SymbolGraphic::bounds);
        let pins = self.pins.iter().map(|p| Rect::from_corners(p.position, p.body_end()));
        graphic.chain(pins).reduce(Rect::union).unwrap_or(Rect::from_center(Point::ZERO, 0, 0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PadShape {
    Round,
    Rect,
    /// Corner radius as a fraction (0..=0.5) of the shorter side.
    RoundRect {
        ratio: f64,
    },
    Oval,
}

impl PadShape {
    pub fn label(&self) -> &'static str {
        match self {
            PadShape::Round => "Round",
            PadShape::Rect => "Rectangular",
            PadShape::RoundRect { .. } => "Rounded Rectangle",
            PadShape::Oval => "Oval",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PadKind {
    /// A surface-mount pad on the footprint's own side.
    Smd,
    /// A plated hole present on every copper layer.
    ThroughHole { drill: Coord },
    /// A non-plated mounting hole.
    NonPlated { drill: Coord },
}

impl PadKind {
    pub fn drill(&self) -> Option<Coord> {
        match self {
            PadKind::Smd => None,
            PadKind::ThroughHole { drill } | PadKind::NonPlated { drill } => Some(*drill),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadDef {
    pub number: String,
    pub position: Point,
    pub width: Coord,
    pub height: Coord,
    pub shape: PadShape,
    #[serde(default)]
    pub rotation: f64,
    pub kind: PadKind,
}

/// Footprint artwork on a non-copper layer (overlay, assembly or courtyard mechanical layers).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FootprintGraphic {
    /// The layer for a top-side placement; flipped for bottom placement.
    pub layer: Layer,
    pub shape: SymbolGraphic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model3d {
    /// A STEP file path relative to the library, or a generated body description.
    pub source: String,
    pub height: Coord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FootprintDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub pads: Vec<PadDef>,
    #[serde(default)]
    pub graphics: Vec<FootprintGraphic>,
    /// Placement keep-out (courtyard), local coordinates.
    pub courtyard: Rect,
    #[serde(default)]
    pub model: Option<Model3d>,
}

impl FootprintDef {
    pub fn pad(&self, number: &str) -> Option<&PadDef> {
        self.pads.iter().find(|p| p.number == number)
    }
}

/// Whether a library component is one exact part or a specification many parts satisfy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// One exact manufacturer part (ICs, connectors, most discretes).
    #[default]
    Specific,
    /// A specification ("10k 1 % 0603") that any approved manufacturer part satisfies; agents
    /// choose the part numbers. Placing a generic family creates one component per value and
    /// footprint in the project library.
    Generic,
}

/// A manufacturer part approved for a component, with cached supplier part numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApprovedPart {
    pub manufacturer: String,
    pub mpn: String,
    /// Supplier → supplier part number (`digikey`, `mouser`, `lcsc` …).
    #[serde(default)]
    pub suppliers: BTreeMap<String, String>,
    /// JLCPCB assembly "Basic" part (no setup fee), when known.
    #[serde(default)]
    pub jlc_basic: Option<bool>,
    /// Date the supplier data was last checked (YYYY-MM-DD).
    #[serde(default)]
    pub checked: String,
}

/// Where a library component came from, for update detection (like a lockfile entry).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Origin {
    /// The source library's name, e.g. `"Hephaestus Basic"`.
    pub source: String,
    /// The component (or generic family) name in the source.
    pub item: String,
    #[serde(default)]
    pub version: String,
    /// [`Library::component_hash`] of the item in the source when it was imported.
    pub hash: String,
}

/// A placeable library component: the entry users and agents search for. In the UI this is a
/// "component" with a schematic symbol, a footprint and a 3D model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartDef {
    /// Unique library reference, e.g. `"RES_0603"`.
    pub name: String,
    pub description: String,
    /// Category path for the Components panel, e.g. `"Passives/Resistors"`.
    pub category: String,
    pub symbol: String,
    /// Compatible footprints; the first is the default.
    pub footprints: Vec<String>,
    /// Designator prefix such as `R`, `C`, `U`.
    pub designator_prefix: String,
    #[serde(default)]
    pub default_value: String,
    /// Symbol pin number to footprint pad numbers. Pins absent here map to the pad with the same
    /// number.
    #[serde(default)]
    pub pin_map: BTreeMap<String, Vec<String>>,
    /// Manufacturer, Manufacturer Part Number, Datasheet, Supplier … free-form parameters.
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default = "first_revision")]
    pub revision: u32,
    #[serde(default)]
    pub kind: PartKind,
    /// For a component made from a generic family (`RES` → `RES_10k_0603`), the family name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// The exact part of a specific component.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub manufacturer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mpn: String,
    /// Manufacturer parts approved for this component, preferred first (alternates of a
    /// specific part, or the choices for a generic one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved: Vec<ApprovedPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Edited in this project since it was imported or created.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub modified: bool,
}

fn first_revision() -> u32 {
    1
}

/// A file- and id-safe fragment of a value or footprint name (`4.7k` stays, `10uF 16V` →
/// `10uF_16V`).
pub fn id_fragment(text: &str) -> String {
    let s: String =
        text.trim().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '%') { c } else { '_' }).collect();
    if s.is_empty() { "_".into() } else { s }
}

impl PartDef {
    /// A new specific component with no parameters (fill the rest with struct update syntax).
    pub fn new(name: &str, symbol: &str, footprint: &str, prefix: &str) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            category: String::new(),
            symbol: symbol.into(),
            footprints: vec![footprint.into()],
            designator_prefix: prefix.into(),
            default_value: String::new(),
            pin_map: BTreeMap::new(),
            parameters: BTreeMap::new(),
            revision: 1,
            kind: PartKind::Specific,
            family: None,
            manufacturer: String::new(),
            mpn: String::new(),
            approved: Vec::new(),
            origin: None,
            modified: false,
        }
    }

    /// The name shown to users: `R 10k 0603` for a component made from a generic family,
    /// otherwise the library name.
    pub fn title(&self) -> String {
        match (&self.family, self.footprints.first()) {
            (Some(_), Some(fp)) => format!("{} {} {}", self.designator_prefix, self.default_value, fp),
            _ => self.name.clone(),
        }
    }

    /// The family a generic component belongs to (itself, for a family).
    pub fn family_name(&self) -> &str {
        self.family.as_deref().unwrap_or(&self.name)
    }

    /// The library name of this generic family's component for `value` on `footprint`.
    pub fn derived_name(&self, value: &str, footprint: &str) -> String {
        format!("{}_{}_{}", self.family_name(), id_fragment(value), id_fragment(footprint))
    }

    /// A generic family (or a component of one) specialised to one value and footprint. Sourcing
    /// data depends on the value, so it is not carried over; `origin` is set by the caller.
    pub fn derived(&self, value: &str, footprint: &str) -> PartDef {
        let mut d = self.clone();
        d.name = self.derived_name(value, footprint);
        d.family = Some(self.family_name().to_string());
        d.default_value = value.to_string();
        d.footprints = vec![footprint.to_string()];
        d.manufacturer.clear();
        d.mpn.clear();
        d.approved.clear();
        d.modified = false;
        d
    }

    /// The manufacturer part this component is bought as: its own part number, or the
    /// preferred approved part.
    pub fn sourcing(&self) -> Option<(&str, &str)> {
        if !self.mpn.is_empty() {
            return Some((self.manufacturer.as_str(), self.mpn.as_str()));
        }
        self.approved.first().map(|a| (a.manufacturer.as_str(), a.mpn.as_str()))
    }

    /// Footprint pads bound to a symbol pin.
    pub fn pads_for_pin<'a>(&'a self, pin: &'a str) -> Vec<&'a str> {
        match self.pin_map.get(pin) {
            Some(pads) => pads.iter().map(String::as_str).collect(),
            None => vec![pin],
        }
    }

    /// The symbol pin bound to a footprint pad, if any.
    pub fn pin_for_pad<'a>(&'a self, symbol: &'a SymbolDef, pad: &str) -> Option<&'a str> {
        symbol.pins.iter().map(|p| p.number.as_str()).find(|pin| self.pads_for_pin(pin).contains(&pad))
    }
}

/// The name `item` takes when added next to `existing` (and items already staged in `staged`):
/// its own name when free or already holding identical content, otherwise `name_2`, `name_3` …
/// New items are added to `staged`.
fn place<T: Clone + PartialEq>(
    existing: &BTreeMap<String, T>,
    staged: &mut BTreeMap<String, T>,
    name: &str,
    item: &T,
    rename: impl Fn(&mut T, &str),
) -> String {
    for n in 1.. {
        let candidate = if n == 1 { name.to_string() } else { format!("{name}_{n}") };
        let mut renamed = item.clone();
        rename(&mut renamed, &candidate);
        match existing.get(&candidate).or(staged.get(&candidate)) {
            Some(held) if *held == renamed => return candidate,
            Some(_) => continue,
            None => {
                staged.insert(candidate.clone(), renamed);
                return candidate;
            }
        }
    }
    name.to_string()
}

/// A named collection of symbols, footprints and parts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub version: u32,
    pub symbols: BTreeMap<String, SymbolDef>,
    pub footprints: BTreeMap<String, FootprintDef>,
    pub parts: BTreeMap<String, PartDef>,
}

impl Library {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    /// Copy one part with its symbol and footprints from another library. Returns false if the
    /// part or something it references is missing in `source`.
    pub fn import_part(&mut self, source: &Library, part: &str) -> bool {
        let Some(def) = source.parts.get(part) else { return false };
        let Some(symbol) = source.symbols.get(&def.symbol) else { return false };
        let footprints: Option<Vec<_>> = def.footprints.iter().map(|f| source.footprints.get(f)).collect();
        let Some(footprints) = footprints else { return false };
        self.symbols.insert(symbol.name.clone(), symbol.clone());
        for footprint in footprints {
            self.footprints.insert(footprint.name.clone(), footprint.clone());
        }
        self.parts.insert(def.name.clone(), def.clone());
        true
    }

    /// Content hash of a symbol, footprint or other library value.
    pub fn shape_hash<T: Serialize>(shape: &T) -> String {
        crate::hash::content_hash(shape)
    }

    /// Content hash of a component: its design data plus the hashes of the symbol and footprints
    /// it uses, so editing any of them changes it. Origin, the modified flag and approved
    /// manufacturer parts (sourcing data) are excluded. `None` when a reference is missing.
    pub fn component_hash(&self, part: &PartDef) -> Option<String> {
        let symbol = Self::shape_hash(self.symbols.get(&part.symbol)?);
        let footprints: Option<Vec<String>> = part.footprints.iter().map(|f| self.footprints.get(f).map(Self::shape_hash)).collect();
        let mut v = serde_json::to_value(part).ok()?;
        let obj = v.as_object_mut()?;
        for k in ["origin", "modified", "approved"] {
            obj.remove(k);
        }
        obj.insert("symbol".into(), symbol.into());
        obj.insert("footprints".into(), footprints?.into());
        Some(crate::hash::value_hash(&v))
    }

    /// The origin record for importing `part` from this library.
    pub fn origin_of(&self, part: &PartDef) -> Option<Origin> {
        Some(Origin {
            source: self.name.clone(),
            item: part.name.clone(),
            version: self.version.to_string(),
            hash: self.component_hash(part)?,
        })
    }

    /// A library holding only `parts` and the symbols and footprints they use, e.g. to export
    /// them as a component pack. Missing names are skipped.
    pub fn extract<'a>(&self, parts: impl IntoIterator<Item = &'a str>) -> Library {
        let mut out = Library::new(self.name.clone());
        out.version = self.version;
        for name in parts {
            out.import_part(self, name);
        }
        out
    }

    /// What importing `incoming` into this library adds: the symbols, footprints and components
    /// it does not already hold. An item whose name is taken by different content is renamed
    /// (`name_2` …) and the components referring to it follow, so nothing already here changes.
    pub fn merge_plan(&self, incoming: &Library) -> Library {
        let mut out = Library::new(incoming.name.clone());
        let mut symbols: BTreeMap<String, String> = BTreeMap::new();
        for (name, sym) in &incoming.symbols {
            let n = place(&self.symbols, &mut out.symbols, name, sym, |s, n| s.name = n.into());
            symbols.insert(name.clone(), n);
        }
        let mut footprints: BTreeMap<String, String> = BTreeMap::new();
        for (name, fp) in &incoming.footprints {
            let n = place(&self.footprints, &mut out.footprints, name, fp, |f, n| f.name = n.into());
            footprints.insert(name.clone(), n);
        }
        for (name, part) in &incoming.parts {
            let mut p = part.clone();
            if let Some(n) = symbols.get(&p.symbol) {
                p.symbol = n.clone();
            }
            for f in &mut p.footprints {
                if let Some(n) = footprints.get(f) {
                    *f = n.clone();
                }
            }
            place(&self.parts, &mut out.parts, name, &p, |p, n| p.name = n.into());
        }
        out
    }

    /// Case-insensitive search across name, description, category, value and parameters.
    /// Every whitespace-separated term must match somewhere.
    pub fn search<'a>(&'a self, query: &str) -> Vec<&'a PartDef> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.parts
            .values()
            .filter(|part| {
                let haystack = format!(
                    "{} {} {} {} {} {} {} {}",
                    part.name,
                    part.manufacturer,
                    part.mpn,
                    part.description,
                    part.category,
                    part.default_value,
                    part.footprints.join(" "),
                    part.parameters.values().cloned().collect::<Vec<_>>().join(" ")
                )
                .to_lowercase();
                terms.iter().all(|t| haystack.contains(t.as_str()))
            })
            .collect()
    }

    /// Distinct categories in sorted order.
    pub fn categories(&self) -> Vec<&str> {
        let mut categories: Vec<&str> = self.parts.values().map(|p| p.category.as_str()).collect();
        categories.sort_unstable();
        categories.dedup();
        categories
    }

    /// Problems that would make a part unusable (missing symbol, footprint or mapped pad).
    pub fn check_part(&self, part: &PartDef) -> Vec<String> {
        let mut problems = Vec::new();
        let Some(symbol) = self.symbols.get(&part.symbol) else {
            return vec![format!("part {} references missing symbol {}", part.name, part.symbol)];
        };
        for name in &part.footprints {
            let Some(footprint) = self.footprints.get(name) else {
                problems.push(format!("part {} references missing footprint {name}", part.name));
                continue;
            };
            for pin in &symbol.pins {
                for pad in part.pads_for_pin(&pin.number) {
                    if footprint.pad(pad).is_none() {
                        problems.push(format!("part {} pin {} maps to pad {pad} absent from {name}", part.name, pin.number));
                    }
                }
            }
        }
        problems
    }
}
