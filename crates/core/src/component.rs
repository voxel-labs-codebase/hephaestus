use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ObjectId;

/// One logical component: the electrical identity a schematic symbol and a PCB footprint share.
///
/// The schematic [`crate::SymbolInstance`] and PCB [`crate::Footprint`] refer to it by `id`, so
/// changing the footprint, moving the symbol or renaming the designator keeps every connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: ObjectId,
    /// Reference designator, e.g. `R1`.
    pub designator: String,
    /// Comment/value shown on the schematic, e.g. `10k`.
    pub value: String,
    /// The part definition in the project library.
    pub part: String,
    /// The chosen footprint (one of the part's compatible footprints).
    pub footprint: String,
    /// Instance parameters; override the part's parameters with the same name.
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
}
