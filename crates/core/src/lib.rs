//! The Hephaestus design database.
//!
//! Everything a project contains is structured engineering data: logical [`Component`]s, a
//! [`Library`] of symbols, footprints and parts, schematic [`Sheet`]s holding [`SchObject`]s, and
//! a board ([`BoardSettings`] plus [`PcbObject`]s). A component is one electrical identity shared
//! by its schematic symbol and its PCB footprint, so changing either view never loses the link.
//!
//! This crate has no UI dependency and no mutation policy: edits go through `hephaestus-commands`,
//! which validates them and records undo history.

mod component;
mod design;
pub mod drag;
pub mod hash;
mod id;
mod layer;
pub mod library;
pub mod library_check;
pub mod pcb;
pub mod schematic;

pub use component::Component;
pub use design::{Design, DesignError, ObjectRef, PadGeometry, PinGeometry};
pub use id::ObjectId;
pub use layer::{Layer, LayerKind};
pub use library::{
    ApprovedPart, FootprintDef, Library, Origin, PadDef, PadKind, PadShape, PartDef, PartKind, PinDef, PinDirection, PinType, SymbolDef,
    SymbolGraphic,
};
pub use pcb::{
    BoardSettings, DesignRules, Dielectric, Footprint, Keepout, LayerStack, NetClass, PcbKind, PcbLine, PcbObject, PcbText, Side,
    StackLayer, Track, Via, Zone, ZoneFillMode,
};
pub use schematic::{
    Bus, BusEntry, Junction, NetLabel, NoConnect, Orientation, Port, PortDirection, PowerPort, PowerStyle, SchKind, SchObject, Sheet,
    SheetEntry, SheetSize, SheetSymbol, SymbolInstance, TextNote, TitleBlock, Wire,
};

pub use hephaestus_geometry as geometry;
