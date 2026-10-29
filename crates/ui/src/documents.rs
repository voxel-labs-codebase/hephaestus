//! Open documents and the per-document workspace state (camera, selection, active tool).

use hephaestus_canvas::{Camera, SelectionBox};
use hephaestus_core::geometry::Point;
use hephaestus_core::schematic::PortDirection;
use hephaestus_core::{Layer, ObjectId, PowerStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocKey {
    Sheet(ObjectId),
    Pcb,
}

/// Wire and track corner modes, cycled with Shift+Space as in Altium.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerMode {
    /// Horizontal first, then vertical (Altium "90 degree start").
    HorizontalFirst,
    VerticalFirst,
    /// 45° start (PCB) / 45° corner (schematic).
    Diagonal,
    Any,
}

impl CornerMode {
    pub fn next(self, pcb: bool) -> Self {
        match (self, pcb) {
            (CornerMode::HorizontalFirst, _) => CornerMode::VerticalFirst,
            (CornerMode::VerticalFirst, _) => CornerMode::Diagonal,
            (CornerMode::Diagonal, _) => CornerMode::Any,
            (CornerMode::Any, _) => CornerMode::HorizontalFirst,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CornerMode::HorizontalFirst => "90° Start (H)",
            CornerMode::VerticalFirst => "90° Start (V)",
            CornerMode::Diagonal => "45°",
            CornerMode::Any => "Any Angle",
        }
    }

    /// Intermediate corner(s) from `a` to `b`.
    pub fn path(self, a: Point, b: Point) -> Vec<Point> {
        let mut v = match self {
            CornerMode::HorizontalFirst => vec![a, Point::new(b.x, a.y), b],
            CornerMode::VerticalFirst => vec![a, Point::new(a.x, b.y), b],
            CornerMode::Diagonal => hephaestus_commands::route_45(a, b),
            CornerMode::Any => vec![a, b],
        };
        v.dedup();
        v
    }
}

/// An interactive routing session (PCB).
#[derive(Clone, Debug, PartialEq)]
pub struct RouteSession {
    pub net: Option<String>,
    pub width: i64,
    /// Committed vertices per layer run: (layer, points).
    pub runs: Vec<(Layer, Vec<Point>)>,
    /// Vias inserted at layer changes.
    pub vias: Vec<Point>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    Select,
    // Schematic
    PlacePart { part: String, rotation: f64, mirror: bool },
    Wire { points: Vec<Point>, mode: CornerMode },
    NetLabel { text: String, rotation: f64 },
    Power { net: String, style: PowerStyle, rotation: f64 },
    Port { name: String, direction: PortDirection, rotation: f64 },
    Junction,
    NoErc,
    Text { text: String },
    // PCB
    Route { session: Option<RouteSession>, mode: CornerMode },
    Track { points: Vec<Point>, mode: CornerMode },
    Via,
    Zone { points: Vec<Point> },
    Keepout { points: Vec<Point> },
    BoardShape { points: Vec<Point> },
    Measure { a: Option<Point>, b: Option<Point> },
}

impl Tool {
    pub fn name(&self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::PlacePart { .. } => "Place Part",
            Tool::Wire { .. } => "Place Wire",
            Tool::NetLabel { .. } => "Place Net Label",
            Tool::Power { .. } => "Place Power Port",
            Tool::Port { .. } => "Place Port",
            Tool::Junction => "Place Junction",
            Tool::NoErc => "Place No ERC",
            Tool::Text { .. } => "Place Text",
            Tool::Route { .. } => "Interactive Routing",
            Tool::Track { .. } => "Place Line",
            Tool::Via => "Place Via",
            Tool::Zone { .. } => "Place Polygon Pour",
            Tool::Keepout { .. } => "Place Keepout",
            Tool::BoardShape { .. } => "Redefine Board Shape",
            Tool::Measure { .. } => "Measure Distance",
        }
    }

    /// Whether the tool is mid-gesture (Esc then cancels the gesture, not the tool).
    pub fn in_progress(&self) -> bool {
        match self {
            Tool::Wire { points, .. } | Tool::Track { points, .. } => !points.is_empty(),
            Tool::Route { session, .. } => session.is_some(),
            Tool::Zone { points } | Tool::Keepout { points } | Tool::BoardShape { points } => !points.is_empty(),
            Tool::Measure { a, .. } => a.is_some(),
            _ => false,
        }
    }

    /// Abandon the current gesture, keeping the tool.
    pub fn cancel_gesture(&mut self) {
        match self {
            Tool::Wire { points, .. } | Tool::Track { points, .. } => points.clear(),
            Tool::Route { session, .. } => *session = None,
            Tool::Zone { points } | Tool::Keepout { points } | Tool::BoardShape { points } => points.clear(),
            Tool::Measure { a, b } => {
                *a = None;
                *b = None;
            }
            _ => {}
        }
    }
}

/// A drag in progress on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub enum Drag {
    /// Moving the selection: world offset so far.
    Move {
        origin: Point,
        offset: Point,
        drag_wires: bool,
    },
    Rubber(SelectionBox),
}

pub struct DocView {
    pub key: DocKey,
    pub camera: Camera,
    pub needs_fit: bool,
    /// View › Fit Document (the whole sheet) on the next frame.
    pub fit_document: bool,
    /// Frames left in which an initial fit is repeated while the dock layout settles.
    pub settle: u8,
    pub selection: Vec<ObjectId>,
    pub hover: Option<ObjectId>,
    pub tool: Tool,
    pub drag: Option<Drag>,
    pub cursor: Option<Point>,
    pub highlight_net: Option<String>,
    pub active_layer: Layer,
    pub single_layer: bool,
    /// Last place the viewport was centred on for navigation (zoom to violation etc).
    pub pending_focus: Option<hephaestus_core::geometry::Rect>,
    /// The canvas rectangle last frame (for keyboard zoom).
    pub viewport: egui::Rect,
}

impl DocView {
    pub fn new(key: DocKey) -> Self {
        Self {
            key,
            camera: Camera::default(),
            needs_fit: true,
            fit_document: false,
            settle: 4,
            selection: Vec::new(),
            hover: None,
            tool: Tool::Select,
            drag: None,
            cursor: None,
            highlight_net: None,
            active_layer: Layer::TopCopper,
            single_layer: false,
            pending_focus: None,
            viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
        }
    }

    pub fn is_pcb(&self) -> bool {
        self.key == DocKey::Pcb
    }
}
