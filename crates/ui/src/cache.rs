//! Derived data cached per design revision: the netlist, board connectivity and the spatial
//! indexes used for hit testing. Rebuilt only when the editor's revision changes, never per
//! frame.

use std::collections::HashMap;

use hephaestus_connectivity::{PcbConnectivity, pcb as pcb_conn, schematic as sch_conn};
use hephaestus_core::geometry::{GridIndex, Rect, mil};
use hephaestus_core::{Design, ObjectId, PadGeometry};

pub struct Cache {
    pub revision: u64,
    pub netlist: sch_conn::Netlist,
    pub pcb: PcbConnectivity,
    /// Per sheet: object bounds index for hit testing.
    pub sheet_index: HashMap<ObjectId, GridIndex<ObjectId>>,
    pub pcb_index: GridIndex<ObjectId>,
    pub pads: Vec<PadGeometry>,
    pub bounds: HashMap<ObjectId, Rect>,
}

impl Cache {
    pub fn empty() -> Self {
        Self {
            revision: 0,
            netlist: Default::default(),
            pcb: Default::default(),
            sheet_index: HashMap::new(),
            pcb_index: GridIndex::new(mil(100.0)),
            pads: Vec::new(),
            bounds: HashMap::new(),
        }
    }

    pub fn refresh(&mut self, design: &Design, revision: u64) {
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        self.netlist = sch_conn::compile(design);
        self.pcb = pcb_conn::analyze(design);
        self.pads = design.all_pads();
        self.bounds.clear();
        self.sheet_index.clear();
        for o in design.schematic.values() {
            let b = design.sch_bounds(o);
            self.bounds.insert(o.id, b);
            self.sheet_index.entry(o.sheet).or_insert_with(|| GridIndex::new(mil(500.0))).insert(o.id, b);
        }
        self.pcb_index = GridIndex::new(mil(100.0));
        for o in design.pcb.values() {
            let b = design.pcb_bounds(o);
            self.bounds.insert(o.id, b);
            self.pcb_index.insert(o.id, b);
        }
    }
}
