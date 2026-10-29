//! Electrical connectivity.
//!
//! [`schematic::compile`] turns sheets into a [`Netlist`] (Altium's "Compile Project"), and
//! [`pcb::analyze`] finds copper islands, unrouted connections (the ratsnest) and shorts on the
//! board. Both are pure functions of the design; callers cache them per design revision.

pub mod pcb;
pub mod schematic;
mod union_find;

pub use pcb::{CopperRef, PcbConnectivity, Ratsnest, Short};
pub use schematic::{Net, NetNode, Netlist};
