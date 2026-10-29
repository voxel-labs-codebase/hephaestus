use std::fmt;

use serde::{Deserialize, Serialize};

/// A project-wide unique, stable object identity.
///
/// Every component, sheet, schematic object and PCB object has one. IDs are allocated from the
/// design's counter, never reused, and serialized as plain numbers so external agents can refer
/// to objects directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}
