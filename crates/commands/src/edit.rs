//! Primitive, invertible edits. Every change to a [`Design`] is a list of these.

use hephaestus_core::{BoardSettings, Component, Design, FootprintDef, ObjectId, PartDef, PcbObject, SchObject, Sheet, SymbolDef};
use serde::{Deserialize, Serialize};

use crate::CommandError;

/// Insert, remove or modify one value. `Remove` and `Modify` carry the full prior state, so every
/// change can be checked against the design before applying and inverted exactly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", content = "value")]
pub enum Change<T> {
    Insert(T),
    Remove(T),
    Modify { before: T, after: T },
}

impl<T: Clone> Change<T> {
    pub fn inverse(&self) -> Change<T> {
        match self {
            Change::Insert(v) => Change::Remove(v.clone()),
            Change::Remove(v) => Change::Insert(v.clone()),
            Change::Modify { before, after } => Change::Modify { before: after.clone(), after: before.clone() },
        }
    }

    pub fn current(&self) -> Option<&T> {
        match self {
            Change::Insert(v) | Change::Modify { after: v, .. } => Some(v),
            Change::Remove(_) => None,
        }
    }

    pub fn previous(&self) -> Option<&T> {
        match self {
            Change::Remove(v) | Change::Modify { before: v, .. } => Some(v),
            Change::Insert(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "target")]
pub enum Edit {
    Component(Change<Component>),
    Sheet(Change<Sheet>),
    Sch(Change<SchObject>),
    Pcb(Change<PcbObject>),
    Board(Change<BoardSettings>),
    LibrarySymbol(Change<SymbolDef>),
    LibraryFootprint(Change<FootprintDef>),
    /// Boxed: a component record is several times larger than the other edits.
    LibraryPart(Box<Change<PartDef>>),
}

impl Edit {
    pub fn inverse(&self) -> Edit {
        match self {
            Edit::Component(c) => Edit::Component(c.inverse()),
            Edit::Sheet(c) => Edit::Sheet(c.inverse()),
            Edit::Sch(c) => Edit::Sch(c.inverse()),
            Edit::Pcb(c) => Edit::Pcb(c.inverse()),
            Edit::Board(c) => Edit::Board(c.inverse()),
            Edit::LibrarySymbol(c) => Edit::LibrarySymbol(c.inverse()),
            Edit::LibraryFootprint(c) => Edit::LibraryFootprint(c.inverse()),
            Edit::LibraryPart(c) => Edit::LibraryPart(Box::new(c.inverse())),
        }
    }

    /// The design object this edit touches, when it is one.
    pub fn object_id(&self) -> Option<ObjectId> {
        fn id<T: Clone>(c: &Change<T>, f: impl Fn(&T) -> ObjectId) -> Option<ObjectId> {
            c.current().or(c.previous()).map(f)
        }
        match self {
            Edit::Component(c) => id(c, |v| v.id),
            Edit::Sheet(c) => id(c, |v| v.id),
            Edit::Sch(c) => id(c, |v| v.id),
            Edit::Pcb(c) => id(c, |v| v.id),
            _ => None,
        }
    }

    /// `"insert"`, `"remove"` or `"modify"` and a short noun, for change summaries.
    pub fn describe(&self) -> String {
        fn verb<T: Clone>(c: &Change<T>) -> &'static str {
            match c {
                Change::Insert(_) => "add",
                Change::Remove(_) => "remove",
                Change::Modify { .. } => "modify",
            }
        }
        match self {
            Edit::Component(c) => format!("{} component {}", verb(c), c.current().or(c.previous()).map_or("", |v| v.designator.as_str())),
            Edit::Sheet(c) => format!("{} sheet {}", verb(c), c.current().or(c.previous()).map_or("", |v| v.name.as_str())),
            Edit::Sch(c) => format!(
                "{} {} {}",
                verb(c),
                c.current().or(c.previous()).map_or("object", |v| v.kind.type_name()),
                self.object_id().map(|i| i.to_string()).unwrap_or_default()
            ),
            Edit::Pcb(c) => format!(
                "{} {} {}",
                verb(c),
                c.current().or(c.previous()).map_or("object", |v| v.kind.type_name()),
                self.object_id().map(|i| i.to_string()).unwrap_or_default()
            ),
            Edit::Board(c) => format!("{} board settings", verb(c)),
            Edit::LibrarySymbol(c) => {
                format!("{} library symbol {}", verb(c), c.current().or(c.previous()).map_or("", |v| v.name.as_str()))
            }
            Edit::LibraryFootprint(c) => {
                format!("{} library footprint {}", verb(c), c.current().or(c.previous()).map_or("", |v| v.name.as_str()))
            }
            Edit::LibraryPart(c) => format!("{} library part {}", verb(c), c.current().or(c.previous()).map_or("", |v| v.name.as_str())),
        }
    }

    /// Apply to `design`, checking that the design is in the state the edit expects.
    pub fn apply(&self, design: &mut Design) -> Result<(), CommandError> {
        match self {
            Edit::Component(c) => apply_map(&mut design.components, c, |v| v.id),
            Edit::Sheet(c) => apply_map(&mut design.sheets, c, |v| v.id),
            Edit::Sch(c) => apply_map(&mut design.schematic, c, |v| v.id),
            Edit::Pcb(c) => apply_map(&mut design.pcb, c, |v| v.id),
            Edit::Board(c) => match c {
                Change::Modify { before, after } if &design.board == before => {
                    design.board = after.clone();
                    Ok(())
                }
                _ => Err(CommandError::Conflict("board settings changed underneath this edit".into())),
            },
            Edit::LibrarySymbol(c) => apply_map(&mut design.library.symbols, c, |v| v.name.clone()),
            Edit::LibraryFootprint(c) => apply_map(&mut design.library.footprints, c, |v| v.name.clone()),
            Edit::LibraryPart(c) => apply_map(&mut design.library.parts, c, |v| v.name.clone()),
        }
    }
}

fn apply_map<K: Ord + std::fmt::Debug, T: Clone + PartialEq>(
    map: &mut std::collections::BTreeMap<K, T>,
    change: &Change<T>,
    key: impl Fn(&T) -> K,
) -> Result<(), CommandError> {
    match change {
        Change::Insert(v) => {
            let k = key(v);
            if map.contains_key(&k) {
                return Err(CommandError::Conflict(format!("{k:?} already exists")));
            }
            map.insert(k, v.clone());
        }
        Change::Remove(v) => {
            let k = key(v);
            if map.get(&k) != Some(v) {
                return Err(CommandError::Conflict(format!("{k:?} changed or vanished before removal")));
            }
            map.remove(&k);
        }
        Change::Modify { before, after } => {
            let k = key(before);
            if key(after) != k {
                return Err(CommandError::Invalid("an edit cannot change an object's identity".into()));
            }
            match map.get_mut(&k) {
                Some(slot) if slot == before => *slot = after.clone(),
                _ => return Err(CommandError::Conflict(format!("{k:?} changed underneath this edit"))),
            }
        }
    }
    Ok(())
}

/// An ordered list of edits applied all-or-nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub description: String,
    pub edits: Vec<Edit>,
    /// The design's id counter after the change (ids are never reused, so undo keeps it).
    pub next_id: u64,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn inverse(&self) -> ChangeSet {
        ChangeSet {
            description: self.description.clone(),
            edits: self.edits.iter().rev().map(Edit::inverse).collect(),
            next_id: self.next_id,
        }
    }

    /// Apply every edit or none: on failure the design is restored.
    pub fn apply(&self, design: &mut Design) -> Result<(), CommandError> {
        for (i, edit) in self.edits.iter().enumerate() {
            if let Err(e) = edit.apply(design) {
                for done in self.edits.iter().take(i).rev() {
                    // The inverse of an edit that just applied cannot conflict.
                    let _ = done.inverse().apply(design);
                }
                return Err(e);
            }
        }
        design.next_id = design.next_id.max(self.next_id);
        Ok(())
    }

    /// Objects touched, deduplicated, in order of first appearance.
    pub fn touched(&self) -> Vec<ObjectId> {
        let mut out = Vec::new();
        for id in self.edits.iter().filter_map(Edit::object_id) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    /// Objects created by this change.
    pub fn created(&self) -> Vec<ObjectId> {
        self.edits
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Edit::Component(Change::Insert(_))
                        | Edit::Sch(Change::Insert(_))
                        | Edit::Pcb(Change::Insert(_))
                        | Edit::Sheet(Change::Insert(_))
                )
            })
            .filter_map(Edit::object_id)
            .collect()
    }

    /// Objects removed by this change.
    pub fn removed(&self) -> Vec<ObjectId> {
        self.edits
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Edit::Component(Change::Remove(_))
                        | Edit::Sch(Change::Remove(_))
                        | Edit::Pcb(Change::Remove(_))
                        | Edit::Sheet(Change::Remove(_))
                )
            })
            .filter_map(Edit::object_id)
            .collect()
    }

    pub fn summary(&self) -> Vec<String> {
        self.edits.iter().map(Edit::describe).collect()
    }

    pub fn append(&mut self, other: ChangeSet) {
        self.edits.extend(other.edits);
        self.next_id = self.next_id.max(other.next_id);
    }
}
