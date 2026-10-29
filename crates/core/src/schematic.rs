//! Schematic sheets and the objects placed on them.

use hephaestus_geometry::{Coord, Point, Rect, mil};
use serde::{Deserialize, Serialize};

use crate::{ObjectId, SymbolGraphic};

/// Standard sheet sizes (Altium's dimensions, landscape).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetSize {
    A4,
    #[default]
    A3,
    A2,
    A1,
    A0,
    A,
    B,
    C,
    D,
    E,
    Letter,
    Legal,
    Tabloid,
}

impl SheetSize {
    pub const ALL: [SheetSize; 13] = [
        SheetSize::A4,
        SheetSize::A3,
        SheetSize::A2,
        SheetSize::A1,
        SheetSize::A0,
        SheetSize::A,
        SheetSize::B,
        SheetSize::C,
        SheetSize::D,
        SheetSize::E,
        SheetSize::Letter,
        SheetSize::Legal,
        SheetSize::Tabloid,
    ];

    /// Width and height in mils, landscape.
    pub fn mils(self) -> (f64, f64) {
        match self {
            SheetSize::A4 => (11500.0, 7600.0),
            SheetSize::A3 => (15500.0, 11100.0),
            SheetSize::A2 => (22300.0, 15700.0),
            SheetSize::A1 => (31500.0, 22300.0),
            SheetSize::A0 => (44600.0, 31500.0),
            SheetSize::A => (9500.0, 7500.0),
            SheetSize::B => (15000.0, 9500.0),
            SheetSize::C => (20000.0, 15000.0),
            SheetSize::D => (32000.0, 20000.0),
            SheetSize::E => (42000.0, 32000.0),
            SheetSize::Letter => (11000.0, 8500.0),
            SheetSize::Legal => (14000.0, 8500.0),
            SheetSize::Tabloid => (17000.0, 11000.0),
        }
    }

    pub fn rect(self) -> Rect {
        let (w, h) = self.mils();
        Rect::from_corners(Point::ZERO, Point::new(mil(w), mil(h)))
    }

    pub fn label(self) -> &'static str {
        match self {
            SheetSize::A4 => "A4",
            SheetSize::A3 => "A3",
            SheetSize::A2 => "A2",
            SheetSize::A1 => "A1",
            SheetSize::A0 => "A0",
            SheetSize::A => "A",
            SheetSize::B => "B",
            SheetSize::C => "C",
            SheetSize::D => "D",
            SheetSize::E => "E",
            SheetSize::Letter => "Letter",
            SheetSize::Legal => "Legal",
            SheetSize::Tabloid => "Tabloid",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

impl Orientation {
    pub const ALL: [Orientation; 2] = [Orientation::Landscape, Orientation::Portrait];

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Landscape => "Landscape",
            Orientation::Portrait => "Portrait",
        }
    }
}

/// Title block drawn in the sheet's bottom-right corner (Altium's Standard and ANSI blocks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TitleBlock {
    #[default]
    Standard,
    Ansi,
    None,
}

impl TitleBlock {
    pub const ALL: [TitleBlock; 3] = [TitleBlock::Standard, TitleBlock::Ansi, TitleBlock::None];

    pub fn label(self) -> &'static str {
        match self {
            TitleBlock::Standard => "Standard",
            TitleBlock::Ansi => "ANSI",
            TitleBlock::None => "None",
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: ObjectId,
    pub name: String,
    pub size: SheetSize,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub title_block: TitleBlock,
    /// Zone references (1, 2, 3 … across, A, B, C … down) in the border.
    #[serde(default = "yes")]
    pub show_zones: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub revision: String,
    /// Sheet order in the project tree.
    #[serde(default)]
    pub order: u32,
}

impl Sheet {
    pub fn new(id: ObjectId, name: impl Into<String>, size: SheetSize, order: u32) -> Self {
        Self {
            id,
            name: name.into(),
            size,
            orientation: Orientation::Landscape,
            title_block: TitleBlock::Standard,
            show_zones: true,
            title: String::new(),
            revision: String::new(),
            order,
        }
    }

    /// The paper area with the sheet's orientation applied.
    pub fn rect(&self) -> Rect {
        let (w, h) = self.size.mils();
        let (w, h) = match self.orientation {
            Orientation::Landscape => (w, h),
            Orientation::Portrait => (h, w),
        };
        Rect::from_corners(Point::ZERO, Point::new(mil(w), mil(h)))
    }
}

/// A placed schematic symbol; its electrical identity is `component`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolInstance {
    pub component: ObjectId,
    pub position: Point,
    /// Degrees counter-clockwise, a multiple of 90.
    pub rotation: f64,
    pub mirror: bool,
    /// Designator/value text positions relative to `position`, unrotated. `None` uses the
    /// symbol's defaults.
    #[serde(default)]
    pub designator_offset: Option<Point>,
    #[serde(default)]
    pub value_offset: Option<Point>,
}

/// A polyline wire. Its endpoints and vertices connect to pins and other wires that touch them;
/// a wire endpoint landing on another wire's segment connects too (with an automatic junction).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bus {
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusEntry {
    pub start: Point,
    pub end: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Junction {
    pub position: Point,
}

/// Names the net of whatever wire or pin it touches. Net labels are global across sheets
/// (Altium's "Global" net identifier scope).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetLabel {
    pub position: Point,
    pub text: String,
    #[serde(default)]
    pub rotation: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerStyle {
    #[default]
    Bar,
    Arrow,
    Circle,
    Wave,
    PowerGround,
    SignalGround,
    Earth,
}

impl PowerStyle {
    pub const ALL: [PowerStyle; 7] = [
        PowerStyle::Bar,
        PowerStyle::Arrow,
        PowerStyle::Circle,
        PowerStyle::Wave,
        PowerStyle::PowerGround,
        PowerStyle::SignalGround,
        PowerStyle::Earth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PowerStyle::Bar => "Bar",
            PowerStyle::Arrow => "Arrow",
            PowerStyle::Circle => "Circle",
            PowerStyle::Wave => "Wave",
            PowerStyle::PowerGround => "Power Ground",
            PowerStyle::SignalGround => "Signal Ground",
            PowerStyle::Earth => "Earth",
        }
    }

    pub fn is_ground(self) -> bool {
        matches!(self, PowerStyle::PowerGround | PowerStyle::SignalGround | PowerStyle::Earth)
    }
}

/// A power port names a global net (VCC, GND …).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PowerPort {
    pub position: Point,
    pub net: String,
    pub style: PowerStyle,
    /// Direction the symbol points away from its hot spot: 90 = up (supplies), 270 = down
    /// (grounds).
    #[serde(default)]
    pub rotation: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortDirection {
    #[default]
    Unspecified,
    Input,
    Output,
    Bidirectional,
}

/// An off-sheet connector; ports with the same name connect across sheets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Port {
    pub position: Point,
    pub name: String,
    pub direction: PortDirection,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "default_port_width")]
    pub width: Coord,
}

fn default_port_width() -> Coord {
    mil(500.0)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoConnect {
    pub position: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextNote {
    pub position: Point,
    pub text: String,
    pub size: Coord,
    #[serde(default)]
    pub rotation: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetEntry {
    pub name: String,
    /// Offset of the entry's connection point from the sheet symbol's top-left corner.
    pub offset: Point,
    pub direction: PortDirection,
}

/// A box on a parent sheet that stands for a child sheet in a hierarchical design.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetSymbol {
    /// Top-left corner.
    pub position: Point,
    pub width: Coord,
    pub height: Coord,
    pub child: ObjectId,
    pub entries: Vec<SheetEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SchKind {
    Symbol(SymbolInstance),
    Wire(Wire),
    Bus(Bus),
    BusEntry(BusEntry),
    Junction(Junction),
    NetLabel(NetLabel),
    PowerPort(PowerPort),
    Port(Port),
    NoConnect(NoConnect),
    Text(TextNote),
    SheetSymbol(SheetSymbol),
    /// Free drawing (lines, rectangles, arcs) with no electrical meaning.
    Graphic(SymbolGraphic),
}

impl SchKind {
    /// Human-readable object type, as shown in the Properties panel header.
    pub fn type_name(&self) -> &'static str {
        match self {
            SchKind::Symbol(_) => "Component",
            SchKind::Wire(_) => "Wire",
            SchKind::Bus(_) => "Bus",
            SchKind::BusEntry(_) => "Bus Entry",
            SchKind::Junction(_) => "Junction",
            SchKind::NetLabel(_) => "Net Label",
            SchKind::PowerPort(_) => "Power Port",
            SchKind::Port(_) => "Port",
            SchKind::NoConnect(_) => "No ERC",
            SchKind::Text(_) => "Text",
            SchKind::SheetSymbol(_) => "Sheet Symbol",
            SchKind::Graphic(_) => "Graphic",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchObject {
    pub id: ObjectId,
    pub sheet: ObjectId,
    #[serde(flatten)]
    pub kind: SchKind,
}

impl SchObject {
    /// Translate every coordinate of the object.
    pub fn translate(&mut self, by: Point) {
        match &mut self.kind {
            SchKind::Symbol(s) => s.position = s.position + by,
            SchKind::Wire(Wire { points }) | SchKind::Bus(Bus { points }) => points.iter_mut().for_each(|p| *p = *p + by),
            SchKind::BusEntry(e) => {
                e.start = e.start + by;
                e.end = e.end + by;
            }
            SchKind::Junction(Junction { position })
            | SchKind::NetLabel(NetLabel { position, .. })
            | SchKind::PowerPort(PowerPort { position, .. })
            | SchKind::Port(Port { position, .. })
            | SchKind::NoConnect(NoConnect { position })
            | SchKind::Text(TextNote { position, .. })
            | SchKind::SheetSymbol(SheetSymbol { position, .. }) => *position = *position + by,
            SchKind::Graphic(g) => translate_graphic(g, by),
        }
    }

    /// The object's anchor (used as the rotation pivot for a single object).
    pub fn anchor(&self) -> Point {
        match &self.kind {
            SchKind::Symbol(s) => s.position,
            SchKind::Wire(Wire { points }) | SchKind::Bus(Bus { points }) => points.first().copied().unwrap_or_default(),
            SchKind::BusEntry(e) => e.start,
            SchKind::Junction(Junction { position })
            | SchKind::NetLabel(NetLabel { position, .. })
            | SchKind::PowerPort(PowerPort { position, .. })
            | SchKind::Port(Port { position, .. })
            | SchKind::NoConnect(NoConnect { position })
            | SchKind::Text(TextNote { position, .. })
            | SchKind::SheetSymbol(SheetSymbol { position, .. }) => *position,
            SchKind::Graphic(g) => g.bounds().map(|b| b.center()).unwrap_or_default(),
        }
    }

    /// Rotate by `degrees` (a multiple of 90) about `pivot`.
    pub fn rotate_about(&mut self, pivot: Point, degrees: f64) {
        let turn = |p: Point| (p - pivot).rotated(degrees) + pivot;
        let spin = |r: &mut f64| *r = (*r + degrees).rem_euclid(360.0);
        match &mut self.kind {
            SchKind::Symbol(s) => {
                s.position = turn(s.position);
                spin(&mut s.rotation);
            }
            SchKind::Wire(Wire { points }) | SchKind::Bus(Bus { points }) => points.iter_mut().for_each(|p| *p = turn(*p)),
            SchKind::BusEntry(e) => {
                e.start = turn(e.start);
                e.end = turn(e.end);
            }
            SchKind::Junction(Junction { position }) | SchKind::NoConnect(NoConnect { position }) => *position = turn(*position),
            SchKind::NetLabel(NetLabel { position, rotation, .. })
            | SchKind::PowerPort(PowerPort { position, rotation, .. })
            | SchKind::Port(Port { position, rotation, .. })
            | SchKind::Text(TextNote { position, rotation, .. }) => {
                *position = turn(*position);
                spin(rotation);
            }
            SchKind::SheetSymbol(s) => s.position = turn(s.position),
            SchKind::Graphic(g) => rotate_graphic(g, pivot, degrees),
        }
    }

    /// Mirror horizontally about the vertical line through `pivot`.
    pub fn mirror_x(&mut self, pivot: Point) {
        let flip = |p: Point| Point::new(2 * pivot.x - p.x, p.y);
        match &mut self.kind {
            SchKind::Symbol(s) => {
                s.position = flip(s.position);
                s.mirror = !s.mirror;
            }
            SchKind::Wire(Wire { points }) | SchKind::Bus(Bus { points }) => points.iter_mut().for_each(|p| *p = flip(*p)),
            SchKind::BusEntry(e) => {
                e.start = flip(e.start);
                e.end = flip(e.end);
            }
            SchKind::PowerPort(PowerPort { position, rotation, .. }) | SchKind::Port(Port { position, rotation, .. }) => {
                *position = flip(*position);
                *rotation = (180.0 - *rotation).rem_euclid(360.0);
            }
            SchKind::Junction(Junction { position })
            | SchKind::NoConnect(NoConnect { position })
            | SchKind::NetLabel(NetLabel { position, .. })
            | SchKind::Text(TextNote { position, .. })
            | SchKind::SheetSymbol(SheetSymbol { position, .. }) => *position = flip(*position),
            SchKind::Graphic(g) => mirror_graphic(g, pivot),
        }
    }
}

pub(crate) fn translate_graphic(g: &mut SymbolGraphic, by: Point) {
    match g {
        SymbolGraphic::Line { points, .. } | SymbolGraphic::Polygon { points, .. } => points.iter_mut().for_each(|p| *p = *p + by),
        SymbolGraphic::Rect { min, max, .. } => {
            *min = *min + by;
            *max = *max + by;
        }
        SymbolGraphic::Circle { center, .. } | SymbolGraphic::Arc { center, .. } => *center = *center + by,
        SymbolGraphic::Text { position, .. } => *position = *position + by,
    }
}

fn rotate_graphic(g: &mut SymbolGraphic, pivot: Point, degrees: f64) {
    let turn = |p: Point| (p - pivot).rotated(degrees) + pivot;
    match g {
        SymbolGraphic::Line { points, .. } | SymbolGraphic::Polygon { points, .. } => points.iter_mut().for_each(|p| *p = turn(*p)),
        SymbolGraphic::Rect { min, max, .. } => {
            let r = Rect::from_corners(turn(*min), turn(*max));
            *min = r.min;
            *max = r.max;
        }
        SymbolGraphic::Circle { center, .. } => *center = turn(*center),
        SymbolGraphic::Arc { center, start, end, .. } => {
            *center = turn(*center);
            *start += degrees;
            *end += degrees;
        }
        SymbolGraphic::Text { position, .. } => *position = turn(*position),
    }
}

fn mirror_graphic(g: &mut SymbolGraphic, pivot: Point) {
    let flip = |p: Point| Point::new(2 * pivot.x - p.x, p.y);
    match g {
        SymbolGraphic::Line { points, .. } | SymbolGraphic::Polygon { points, .. } => points.iter_mut().for_each(|p| *p = flip(*p)),
        SymbolGraphic::Rect { min, max, .. } => {
            let r = Rect::from_corners(flip(*min), flip(*max));
            *min = r.min;
            *max = r.max;
        }
        SymbolGraphic::Circle { center, .. } => *center = flip(*center),
        SymbolGraphic::Arc { center, start, end, .. } => {
            *center = flip(*center);
            let (s, e) = (180.0 - *end, 180.0 - *start);
            *start = s;
            *end = e;
        }
        SymbolGraphic::Text { position, .. } => *position = flip(*position),
    }
}
