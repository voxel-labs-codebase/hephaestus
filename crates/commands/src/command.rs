//! The command vocabulary shared by the UI, the agent API and scripts.
//!
//! Commands are serializable requests (`{"command": "add_wire", ...}`) that the planner turns into
//! a validated [`crate::ChangeSet`]. Lengths and coordinates are integer nanometres; the agent API
//! converts user units before building commands.

use std::collections::BTreeMap;
use std::fmt;

use hephaestus_core::geometry::{Coord, Point};
use hephaestus_core::{
    ApprovedPart, DesignRules, Layer, Library, ObjectId, Orientation, PcbObject, PortDirection, PowerStyle, SchObject, SheetSize, Side,
    TitleBlock,
};
use serde::{Deserialize, Serialize};

/// A component by object id or by designator (`5` or `"R1"`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComponentRef {
    Id(ObjectId),
    Designator(String),
}

impl fmt::Display for ComponentRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComponentRef::Id(id) => write!(f, "{id}"),
            ComponentRef::Designator(d) => f.write_str(d),
        }
    }
}

/// A component pin by number or by name (`{"component": "U1", "pin": "8"}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRef {
    pub component: ComponentRef,
    pub pin: String,
}

impl fmt::Display for PinRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.component, self.pin)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStyle {
    /// A wire when both pins are on one sheet and a clean orthogonal route exists, otherwise
    /// net labels.
    #[default]
    Auto,
    Wire,
    Label,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignMode {
    Left,
    Right,
    Top,
    Bottom,
    CenterHorizontal,
    CenterVertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}

fn zero() -> f64 {
    0.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    // Project structure
    AddSheet {
        name: String,
        #[serde(default)]
        size: Option<SheetSize>,
    },
    UpdateSheet {
        sheet: ObjectId,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        size: Option<SheetSize>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        revision: Option<String>,
        #[serde(default)]
        orientation: Option<Orientation>,
        #[serde(default)]
        title_block: Option<TitleBlock>,
        #[serde(default)]
        show_zones: Option<bool>,
    },
    /// Remove a sheet and everything on it (components whose only symbols are there go too).
    RemoveSheet {
        sheet: ObjectId,
    },
    /// Copy a part, its symbol and footprints from an available library into the project.
    ImportPart {
        part: String,
    },
    /// Add the components, symbols and footprints of a component pack (an exported library) to
    /// the project library. Names taken by different content get a `_2` … suffix.
    ImportLibrary {
        library: Library,
    },
    /// Edit a project library component's description, part number, parameters or approved
    /// manufacturer parts.
    UpdateLibraryComponent {
        part: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        manufacturer: Option<String>,
        #[serde(default)]
        mpn: Option<String>,
        /// `null` removes a parameter.
        #[serde(default)]
        parameters: BTreeMap<String, Option<String>>,
        /// Replaces the approved manufacturer parts when given.
        #[serde(default)]
        approved: Option<Vec<ApprovedPart>>,
    },
    /// Use another library component for placed components (same symbol pin numbers required).
    /// A generic family keeps each component's value and, when the family allows it, footprint.
    AssignComponent {
        components: Vec<ComponentRef>,
        part: String,
    },

    // Schematic
    PlaceComponent {
        part: String,
        #[serde(default)]
        sheet: Option<ObjectId>,
        position: Point,
        #[serde(default = "zero")]
        rotation: f64,
        #[serde(default)]
        mirror: bool,
        #[serde(default)]
        designator: Option<String>,
        #[serde(default)]
        value: Option<String>,
        #[serde(default)]
        footprint: Option<String>,
        #[serde(default)]
        parameters: BTreeMap<String, String>,
    },
    /// Remove a component with all its symbols and its footprint.
    RemoveComponent {
        component: ComponentRef,
    },
    ModifyComponent {
        component: ComponentRef,
        #[serde(default)]
        designator: Option<String>,
        #[serde(default)]
        value: Option<String>,
        #[serde(default)]
        footprint: Option<String>,
        /// `null` removes a parameter.
        #[serde(default)]
        parameters: BTreeMap<String, Option<String>>,
    },
    AddWire {
        #[serde(default)]
        sheet: Option<ObjectId>,
        points: Vec<Point>,
    },
    AddNetLabel {
        #[serde(default)]
        sheet: Option<ObjectId>,
        position: Point,
        text: String,
        #[serde(default = "zero")]
        rotation: f64,
    },
    AddPowerPort {
        #[serde(default)]
        sheet: Option<ObjectId>,
        position: Point,
        net: String,
        #[serde(default)]
        style: Option<PowerStyle>,
        #[serde(default)]
        rotation: Option<f64>,
    },
    AddPort {
        #[serde(default)]
        sheet: Option<ObjectId>,
        position: Point,
        name: String,
        #[serde(default)]
        direction: PortDirection,
        #[serde(default = "zero")]
        rotation: f64,
    },
    AddJunction {
        #[serde(default)]
        sheet: Option<ObjectId>,
        position: Point,
    },
    AddNoConnect {
        #[serde(default)]
        sheet: Option<ObjectId>,
        position: Point,
    },
    AddText {
        #[serde(default)]
        sheet: Option<ObjectId>,
        position: Point,
        text: String,
        #[serde(default)]
        size: Option<Coord>,
    },
    /// Electrically connect two pins with a wire or matching net labels.
    ConnectPins {
        from: PinRef,
        to: PinRef,
        #[serde(default)]
        net: Option<String>,
        #[serde(default)]
        style: ConnectStyle,
    },
    /// Put a list of pins on a named net (net-label stubs on each pin).
    CreateNet {
        name: String,
        pins: Vec<PinRef>,
    },

    // Generic object editing (schematic or PCB ids)
    MoveObjects {
        ids: Vec<ObjectId>,
        dx: Coord,
        dy: Coord,
        /// Schematic: drag attached wire ends along with moved pins.
        #[serde(default)]
        drag_wires: bool,
    },
    RotateObjects {
        ids: Vec<ObjectId>,
        degrees: f64,
        #[serde(default)]
        pivot: Option<Point>,
    },
    /// Schematic: mirror about a vertical axis. PCB footprints: flip to the other side.
    MirrorObjects {
        ids: Vec<ObjectId>,
        #[serde(default)]
        pivot: Option<Point>,
    },
    DeleteObjects {
        ids: Vec<ObjectId>,
    },
    /// Replace a schematic object with an edited copy (Properties panel edits).
    UpdateSchObject {
        object: SchObject,
    },
    /// Replace a PCB object with an edited copy (Properties panel edits).
    UpdatePcbObject {
        object: PcbObject,
    },

    // PCB
    /// Altium's Design › Update PCB Document: create missing footprints, swap changed footprints
    /// and assign pad nets from the compiled schematic.
    UpdatePcb {},
    PlaceFootprint {
        component: ComponentRef,
        position: Point,
        #[serde(default)]
        rotation: Option<f64>,
        #[serde(default)]
        side: Option<Side>,
    },
    AddTrack {
        start: Point,
        end: Point,
        layer: Layer,
        #[serde(default)]
        width: Option<Coord>,
        #[serde(default)]
        net: Option<String>,
    },
    /// A connected run of track segments through `points`.
    AddTracks {
        points: Vec<Point>,
        layer: Layer,
        #[serde(default)]
        width: Option<Coord>,
        #[serde(default)]
        net: Option<String>,
    },
    /// Route every unrouted connection of a net with a direct 45° track run on one layer, using
    /// whichever of the two 45° shapes keeps clearance to other nets' copper. This is not an
    /// obstacle-avoiding autorouter: a connection with no clean direct route is refused.
    RouteNet {
        net: String,
        #[serde(default)]
        layer: Option<Layer>,
        #[serde(default)]
        width: Option<Coord>,
    },
    AddVia {
        position: Point,
        #[serde(default)]
        diameter: Option<Coord>,
        #[serde(default)]
        drill: Option<Coord>,
        #[serde(default)]
        net: Option<String>,
        #[serde(default)]
        from: Option<Layer>,
        #[serde(default)]
        to: Option<Layer>,
    },
    ModifyTrack {
        track: ObjectId,
        #[serde(default)]
        start: Option<Point>,
        #[serde(default)]
        end: Option<Point>,
        #[serde(default)]
        width: Option<Coord>,
        #[serde(default)]
        layer: Option<Layer>,
        #[serde(default)]
        net: Option<String>,
    },
    AddZone {
        layer: Layer,
        #[serde(default)]
        net: Option<String>,
        outline: Vec<Point>,
        #[serde(default)]
        priority: Option<u32>,
    },
    AddKeepout {
        #[serde(default)]
        layers: Vec<Layer>,
        outline: Vec<Point>,
    },
    AddBoardLine {
        layer: Layer,
        start: Point,
        end: Point,
        width: Coord,
    },
    AddBoardText {
        layer: Layer,
        position: Point,
        text: String,
        height: Coord,
    },
    SetBoardOutline {
        outline: Vec<Point>,
    },
    SetDesignRules {
        rules: DesignRules,
    },
    SetLayerStack {
        copper_layers: u8,
    },
    SetOrigin {
        origin: Point,
    },
    AlignObjects {
        ids: Vec<ObjectId>,
        mode: AlignMode,
    },
    DistributeObjects {
        ids: Vec<ObjectId>,
        axis: Axis,
    },
    LockObjects {
        ids: Vec<ObjectId>,
        locked: bool,
    },

    /// Several commands as one undoable change.
    Batch {
        commands: Vec<Command>,
    },
}

impl Command {
    /// A short past-tense-free label for history and the Undo menu.
    pub fn label(&self) -> String {
        match self {
            Command::AddSheet { name, .. } => format!("Add sheet {name}"),
            Command::UpdateSheet { .. } => "Edit sheet".into(),
            Command::RemoveSheet { .. } => "Remove sheet".into(),
            Command::ImportPart { part } => format!("Import part {part}"),
            Command::ImportLibrary { library } => format!("Import {} component(s)", library.parts.len()),
            Command::UpdateLibraryComponent { part, .. } => format!("Edit library component {part}"),
            Command::AssignComponent { components, part } => format!("Assign {part} to {} component(s)", components.len()),
            Command::PlaceComponent { part, .. } => format!("Place {part}"),
            Command::RemoveComponent { component } => format!("Remove {component}"),
            Command::ModifyComponent { component, .. } => format!("Modify {component}"),
            Command::AddWire { .. } => "Place wire".into(),
            Command::AddNetLabel { text, .. } => format!("Place net label {text}"),
            Command::AddPowerPort { net, .. } => format!("Place power port {net}"),
            Command::AddPort { name, .. } => format!("Place port {name}"),
            Command::AddJunction { .. } => "Place junction".into(),
            Command::AddNoConnect { .. } => "Place no ERC".into(),
            Command::AddText { .. } => "Place text".into(),
            Command::ConnectPins { from, to, .. } => format!("Connect {from} to {to}"),
            Command::CreateNet { name, .. } => format!("Create net {name}"),
            Command::MoveObjects { ids, .. } => format!("Move {} object(s)", ids.len()),
            Command::RotateObjects { ids, .. } => format!("Rotate {} object(s)", ids.len()),
            Command::MirrorObjects { ids, .. } => format!("Mirror {} object(s)", ids.len()),
            Command::DeleteObjects { ids } => format!("Delete {} object(s)", ids.len()),
            Command::UpdateSchObject { object } => format!("Edit {}", object.kind.type_name()),
            Command::UpdatePcbObject { object } => format!("Edit {}", object.kind.type_name()),
            Command::UpdatePcb {} => "Update PCB".into(),
            Command::PlaceFootprint { component, .. } => format!("Place footprint of {component}"),
            Command::AddTrack { .. } | Command::AddTracks { .. } => "Place track".into(),
            Command::RouteNet { net, .. } => format!("Route net {net}"),
            Command::AddVia { .. } => "Place via".into(),
            Command::ModifyTrack { .. } => "Modify track".into(),
            Command::AddZone { .. } => "Place polygon pour".into(),
            Command::AddKeepout { .. } => "Place keepout".into(),
            Command::AddBoardLine { .. } => "Place line".into(),
            Command::AddBoardText { .. } => "Place text".into(),
            Command::SetBoardOutline { .. } => "Redefine board shape".into(),
            Command::SetDesignRules { .. } => "Edit design rules".into(),
            Command::SetLayerStack { copper_layers } => format!("Set {copper_layers}-layer stack"),
            Command::SetOrigin { .. } => "Set origin".into(),
            Command::AlignObjects { .. } => "Align".into(),
            Command::DistributeObjects { .. } => "Distribute".into(),
            Command::LockObjects { locked, .. } => {
                if *locked {
                    "Lock".into()
                } else {
                    "Unlock".into()
                }
            }
            Command::Batch { commands } => format!("Batch of {} commands", commands.len()),
        }
    }

    /// Whether the command can remove or overwrite existing work. Agents need approval for these
    /// when the application requires it.
    pub fn is_destructive(&self) -> bool {
        match self {
            Command::RemoveSheet { .. }
            | Command::RemoveComponent { .. }
            | Command::DeleteObjects { .. }
            | Command::SetLayerStack { .. }
            | Command::SetBoardOutline { .. }
            | Command::SetDesignRules { .. } => true,
            Command::Batch { commands } => commands.iter().any(Command::is_destructive),
            _ => false,
        }
    }
}
