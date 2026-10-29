//! Library status: the badges shown on each project library component (modified locally,
//! update available, no part number, no 3D model, unused) and reported to agents.

use serde::Serialize;

use crate::{Design, Library};

/// What needs attention on one project library component.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ComponentStatus {
    pub name: String,
    /// Designators of the placed components using it.
    pub used_by: Vec<String>,
    /// Edited in this project since it was imported or created.
    pub modified: bool,
    /// Its origin now has different content than when it was imported.
    pub update_available: bool,
    /// It names an origin that is not among the available libraries.
    pub origin_missing: bool,
    /// No manufacturer part is chosen (no part number, no approved part).
    pub no_part_number: bool,
    pub no_3d_model: bool,
    /// Broken references (missing symbol, footprint or mapped pad).
    pub problems: Vec<String>,
}

impl ComponentStatus {
    /// Short labels for the UI, most important first.
    pub fn badges(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !self.problems.is_empty() {
            out.push("broken");
        }
        if self.update_available {
            out.push("update available");
        }
        if self.modified {
            out.push("modified locally");
        }
        if self.no_part_number {
            out.push("no part number");
        }
        if self.no_3d_model {
            out.push("no 3D model");
        }
        if self.used_by.is_empty() {
            out.push("unused");
        }
        out
    }
}

/// Status of every component in the design's library, checked against the libraries it may
/// have come from.
pub fn check_library(design: &Design, sources: &[Library]) -> Vec<ComponentStatus> {
    let lib = &design.library;
    lib.parts
        .values()
        .map(|part| {
            let mut used_by: Vec<String> =
                design.components.values().filter(|c| c.part == part.name).map(|c| c.designator.clone()).collect();
            used_by.sort();
            let (update_available, origin_missing) = match &part.origin {
                None => (false, false),
                Some(o) => match sources.iter().find(|s| s.name == o.source).and_then(|s| s.parts.get(&o.item).map(|p| (s, p))) {
                    Some((source, upstream)) => (source.component_hash(upstream).is_some_and(|h| h != o.hash), false),
                    None => (false, true),
                },
            };
            ComponentStatus {
                name: part.name.clone(),
                used_by,
                modified: part.modified,
                update_available,
                origin_missing,
                no_part_number: part.sourcing().is_none(),
                no_3d_model: !part.footprints.iter().filter_map(|f| lib.footprints.get(f)).any(|f| f.model.is_some()),
                problems: lib.check_part(part),
            }
        })
        .collect()
}
