//! Turns commands into change sets. Planning never mutates the caller's design: it works on a
//! scratch copy, validating each step, and records the primitive edits it made.

use std::collections::{BTreeMap, BTreeSet};

use hephaestus_connectivity::{pcb as pcb_conn, schematic as sch_conn};
use hephaestus_core::drag;
use hephaestus_core::geometry::{Coord, Point, Rect, mil, polygon_contains};
use hephaestus_core::schematic::{Junction, NetLabel, NoConnect, Port, PowerPort, PowerStyle, TextNote, Wire};
use hephaestus_core::{
    BoardSettings, Component, Design, Footprint, Keepout, Layer, LayerStack, Library, ObjectId, PartDef, PartKind, PcbKind, PcbLine,
    PcbObject, PcbText, PinGeometry, SchKind, SchObject, Sheet, SheetSize, Side, SymbolInstance, Track, Via, Zone,
};

use crate::CommandError;
use crate::command::{AlignMode, Axis, Command, ComponentRef, ConnectStyle, PinRef};
use crate::edit::{Change, ChangeSet, Edit};

/// Largest coordinate magnitude accepted (10 m).
const MAX_COORD: Coord = 10_000_000_000;
/// Largest number of points in one wire, track run or outline.
const MAX_POINTS: usize = 100_000;

type Result<T> = std::result::Result<T, CommandError>;

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(CommandError::Invalid(msg.into()))
}

fn check_point(p: Point) -> Result<Point> {
    if p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD {
        return invalid(format!("coordinate ({}, {}) is outside the 10 m design area", p.x, p.y));
    }
    Ok(p)
}

fn check_length(name: &str, v: Coord, min: Coord) -> Result<Coord> {
    if v < min || v > MAX_COORD {
        return invalid(format!("{name} {v} nm is out of range"));
    }
    Ok(v)
}

fn check_rotation(degrees: f64, right_angles: bool) -> Result<f64> {
    if !degrees.is_finite() {
        return invalid("rotation must be a finite number of degrees");
    }
    if right_angles && degrees.rem_euclid(90.0) != 0.0 {
        return invalid("schematic rotation must be a multiple of 90 degrees");
    }
    Ok(degrees.rem_euclid(360.0))
}

fn check_name(what: &str, s: &str) -> Result<String> {
    let t = s.trim();
    if t.is_empty() {
        return invalid(format!("{what} must not be empty"));
    }
    if t.len() > 256 {
        return invalid(format!("{what} is longer than 256 characters"));
    }
    Ok(t.to_string())
}

fn check_polygon(points: &[Point]) -> Result<Vec<Point>> {
    if points.len() < 3 || points.len() > MAX_POINTS {
        return invalid("an outline needs between 3 and 100000 vertices");
    }
    points.iter().map(|&p| check_point(p)).collect()
}

/// The 45° two-segment path Altium's interactive router previews: diagonal first, then straight.
pub fn route_45(a: Point, b: Point) -> Vec<Point> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d = dx.abs().min(dy.abs());
    let mid = Point::new(a.x + d * dx.signum(), a.y + d * dy.signum());
    if mid == a || mid == b { vec![a, b] } else { vec![a, mid, b] }
}

pub struct Planner<'a> {
    pub design: Design,
    sources: &'a [Library],
    edits: Vec<Edit>,
}

impl<'a> Planner<'a> {
    pub fn new(design: &Design, sources: &'a [Library]) -> Self {
        Self { design: design.clone(), sources, edits: Vec::new() }
    }

    pub fn finish(self, description: String) -> ChangeSet {
        ChangeSet { description, edits: self.edits, next_id: self.design.next_id }
    }

    fn record(&mut self, edit: Edit) -> Result<()> {
        edit.apply(&mut self.design)?;
        self.edits.push(edit);
        Ok(())
    }

    // --- primitive helpers --------------------------------------------------------------------

    fn insert_sch(&mut self, sheet: ObjectId, kind: SchKind) -> Result<ObjectId> {
        let id = self.design.alloc_id();
        self.record(Edit::Sch(Change::Insert(SchObject { id, sheet, kind })))?;
        Ok(id)
    }

    fn insert_pcb(&mut self, kind: PcbKind) -> Result<ObjectId> {
        let id = self.design.alloc_id();
        self.record(Edit::Pcb(Change::Insert(PcbObject { id, kind })))?;
        Ok(id)
    }

    fn modify_sch(&mut self, after: SchObject) -> Result<()> {
        let Some(before) = self.design.schematic.get(&after.id).cloned() else { return Err(CommandError::UnknownObject(after.id)) };
        if before != after {
            self.record(Edit::Sch(Change::Modify { before, after }))?;
        }
        Ok(())
    }

    fn modify_pcb(&mut self, after: PcbObject) -> Result<()> {
        let Some(before) = self.design.pcb.get(&after.id).cloned() else { return Err(CommandError::UnknownObject(after.id)) };
        if before != after {
            self.record(Edit::Pcb(Change::Modify { before, after }))?;
        }
        Ok(())
    }

    fn modify_component(&mut self, after: Component) -> Result<()> {
        let Some(before) = self.design.components.get(&after.id).cloned() else { return Err(CommandError::UnknownObject(after.id)) };
        if before != after {
            self.record(Edit::Component(Change::Modify { before, after }))?;
        }
        Ok(())
    }

    fn modify_board(&mut self, after: BoardSettings) -> Result<()> {
        let before = self.design.board.clone();
        if before != after {
            self.record(Edit::Board(Change::Modify { before, after }))?;
        }
        Ok(())
    }

    fn remove_sch(&mut self, id: ObjectId) -> Result<()> {
        if let Some(o) = self.design.schematic.get(&id).cloned() {
            self.record(Edit::Sch(Change::Remove(o)))?;
        }
        Ok(())
    }

    fn remove_pcb(&mut self, id: ObjectId) -> Result<()> {
        if let Some(o) = self.design.pcb.get(&id).cloned() {
            self.record(Edit::Pcb(Change::Remove(o)))?;
        }
        Ok(())
    }

    /// Remove a component, its symbols and its footprint.
    fn remove_component(&mut self, id: ObjectId) -> Result<()> {
        let symbols: Vec<ObjectId> = self.design.symbols_of(id).map(|(o, _)| o.id).collect();
        for s in symbols {
            self.remove_sch(s)?;
        }
        if let Some(f) = self.design.footprint_of(id).map(|(o, _)| o.id) {
            self.remove_pcb(f)?;
        }
        if let Some(c) = self.design.components.get(&id).cloned() {
            self.record(Edit::Component(Change::Remove(c.clone())))?;
            self.prune_part(&c.part)?;
        }
        Ok(())
    }

    /// Drop a library component made automatically from a generic family once nothing uses it
    /// and it carries nothing of its own (no part numbers, no local edits).
    fn prune_part(&mut self, name: &str) -> Result<()> {
        let Some(part) = self.design.library.parts.get(name) else { return Ok(()) };
        let pristine = part.family.is_some() && part.mpn.is_empty() && part.approved.is_empty() && !part.modified;
        if pristine && !self.design.components.values().any(|c| c.part == name) {
            let part = part.clone();
            self.record(Edit::LibraryPart(Box::new(Change::Remove(part))))?;
        }
        Ok(())
    }

    fn sheet(&self, sheet: Option<ObjectId>) -> Result<ObjectId> {
        match sheet {
            Some(id) if self.design.sheets.contains_key(&id) => Ok(id),
            Some(id) => invalid(format!("sheet {id} does not exist")),
            None => {
                self.design.sheets_in_order().first().map(|s| s.id).ok_or_else(|| CommandError::Invalid("the project has no sheets".into()))
            }
        }
    }

    pub fn component(&self, r: &ComponentRef) -> Result<ObjectId> {
        match r {
            ComponentRef::Id(id) if self.design.components.contains_key(id) => Ok(*id),
            ComponentRef::Id(id) => {
                // Accept a symbol or footprint id too: resolve it to its component.
                match self.design.object(*id) {
                    Some(hephaestus_core::ObjectRef::Sch(SchObject { kind: SchKind::Symbol(s), .. })) => Ok(s.component),
                    Some(hephaestus_core::ObjectRef::Pcb(PcbObject {
                        kind: PcbKind::Footprint(Footprint { component: Some(c), .. }),
                        ..
                    })) => Ok(*c),
                    _ => Err(CommandError::UnknownObject(*id)),
                }
            }
            ComponentRef::Designator(d) => self
                .design
                .component_by_designator(d)
                .map(|c| c.id)
                .ok_or_else(|| CommandError::Invalid(format!("no component with designator {d}"))),
        }
    }

    pub fn pin(&self, r: &PinRef) -> Result<PinGeometry> {
        let component = self.component(&r.component)?;
        let pins: Vec<PinGeometry> = self.design.symbols_of(component).flat_map(|(o, _)| self.design.pins(o)).collect();
        if pins.is_empty() {
            return invalid(format!("{} has no placed symbol", r.component));
        }
        if let Some(p) = pins.iter().find(|p| p.number == r.pin) {
            return Ok(p.clone());
        }
        let by_name: Vec<&PinGeometry> = pins.iter().filter(|p| p.name.eq_ignore_ascii_case(&r.pin)).collect();
        match by_name.as_slice() {
            [one] => Ok((*one).clone()),
            [] => invalid(format!("{} has no pin {}", r.component, r.pin)),
            _ => invalid(format!("pin name {} is ambiguous on {}; use the pin number", r.pin, r.component)),
        }
    }

    fn ensure_part(&mut self, part: &str) -> Result<()> {
        if self.design.library.parts.contains_key(part) {
            return Ok(());
        }
        let Some(source) = self.sources.iter().find(|l| l.parts.contains_key(part)) else {
            return invalid(format!("part {part} is not in the project library or any available library"));
        };
        let mut staged = Library::default();
        if !staged.import_part(source, part) {
            return invalid(format!("part {part} references symbols or footprints missing from {}", source.name));
        }
        for symbol in staged.symbols.into_values() {
            if !self.design.library.symbols.contains_key(&symbol.name) {
                self.record(Edit::LibrarySymbol(Change::Insert(symbol)))?;
            }
        }
        for fp in staged.footprints.into_values() {
            if !self.design.library.footprints.contains_key(&fp.name) {
                self.record(Edit::LibraryFootprint(Change::Insert(fp)))?;
            }
        }
        for mut def in staged.parts.into_values() {
            def.origin = source.origin_of(&def);
            self.record(Edit::LibraryPart(Box::new(Change::Insert(def))))?;
        }
        Ok(())
    }

    fn ensure_symbol(&mut self, name: &str) -> Result<()> {
        if self.design.library.symbols.contains_key(name) {
            return Ok(());
        }
        let Some(sym) = self.sources.iter().find_map(|l| l.symbols.get(name)).cloned() else {
            return invalid(format!("symbol {name} is not in any library"));
        };
        self.record(Edit::LibrarySymbol(Change::Insert(sym)))
    }

    /// A component from the project library, else from the first available library that has
    /// it, with the origin it would be imported with.
    fn lookup_part(&self, part: &str) -> Result<(PartDef, Option<hephaestus_core::Origin>)> {
        if let Some(p) = self.design.library.parts.get(part) {
            return Ok((p.clone(), p.origin.clone()));
        }
        match self.sources.iter().find_map(|l| l.parts.get(part).map(|p| (l, p))) {
            Some((lib, p)) => Ok((p.clone(), lib.origin_of(p))),
            None => invalid(format!("part {part} is not in the project library or any available library")),
        }
    }

    /// The project library component to use for `part` with `value` on `footprint`. Specific
    /// parts are imported as they are. A generic family, or a component made from one, gives the
    /// family's component for that value and footprint (`RES` + 10k + 0603 → `RES_10k_0603`),
    /// created on first use, so each value has its own part numbers.
    fn concrete_part(&mut self, part: &str, value: &str, footprint: Option<&str>) -> Result<String> {
        let (base, origin) = self.lookup_part(part)?;
        if base.kind != PartKind::Generic {
            self.ensure_part(part)?;
            return Ok(part.to_string());
        }
        let Some(footprint) = footprint.or(base.footprints.first().map(String::as_str)) else {
            return invalid(format!("part {part} has no footprint"));
        };
        let value = check_name("value", value)?;
        let name = base.derived_name(&value, footprint);
        if self.design.library.parts.contains_key(&name) {
            return Ok(name);
        }
        self.ensure_symbol(&base.symbol)?;
        self.ensure_footprint(footprint)?;
        let mut derived = base.derived(&value, footprint);
        derived.origin = origin;
        if let Some(problem) = self.design.library.check_part(&derived).into_iter().next() {
            return invalid(problem);
        }
        self.record(Edit::LibraryPart(Box::new(Change::Insert(derived))))?;
        Ok(name)
    }

    /// Footprints are looked up in the project library first, then in the available libraries.
    fn ensure_footprint(&mut self, name: &str) -> Result<()> {
        if self.design.library.footprints.contains_key(name) {
            return Ok(());
        }
        let Some(fp) = self.sources.iter().find_map(|l| l.footprints.get(name)).cloned() else {
            return invalid(format!("footprint {name} is not in any library"));
        };
        self.record(Edit::LibraryFootprint(Change::Insert(fp)))
    }

    // --- command dispatch ---------------------------------------------------------------------

    pub fn run(&mut self, command: &Command) -> Result<()> {
        match command {
            Command::AddSheet { name, size } => {
                let name = check_name("sheet name", name)?;
                if self.design.sheets.values().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                    return invalid(format!("a sheet named {name} already exists"));
                }
                let id = self.design.alloc_id();
                let order = self.design.sheets.values().map(|s| s.order + 1).max().unwrap_or(0);
                self.record(Edit::Sheet(Change::Insert(Sheet::new(id, name, size.unwrap_or(SheetSize::A3), order))))
            }
            Command::UpdateSheet { sheet, name, size, title, revision, orientation, title_block, show_zones } => {
                let Some(before) = self.design.sheets.get(sheet).cloned() else { return Err(CommandError::UnknownObject(*sheet)) };
                let mut after = before.clone();
                if let Some(n) = name {
                    after.name = check_name("sheet name", n)?;
                }
                if let Some(s) = size {
                    after.size = *s;
                }
                if let Some(t) = title {
                    after.title = t.clone();
                }
                if let Some(r) = revision {
                    after.revision = r.clone();
                }
                if let Some(o) = orientation {
                    after.orientation = *o;
                }
                if let Some(t) = title_block {
                    after.title_block = *t;
                }
                if let Some(z) = show_zones {
                    after.show_zones = *z;
                }
                if before != after {
                    self.record(Edit::Sheet(Change::Modify { before, after }))?;
                }
                Ok(())
            }
            Command::RemoveSheet { sheet } => {
                let Some(s) = self.design.sheets.get(sheet).cloned() else { return Err(CommandError::UnknownObject(*sheet)) };
                if self.design.sheets.len() == 1 {
                    return invalid("a project needs at least one sheet");
                }
                if self.design.schematic.values().any(|o| matches!(&o.kind, SchKind::SheetSymbol(ss) if ss.child == *sheet)) {
                    return invalid("the sheet is used by a sheet symbol; delete the sheet symbol first");
                }
                let objects: Vec<ObjectId> = self.design.sheet_objects(*sheet).map(|o| o.id).collect();
                let mut components = BTreeSet::new();
                for id in objects {
                    if let Some(SchObject { kind: SchKind::Symbol(sym), .. }) = self.design.schematic.get(&id) {
                        components.insert(sym.component);
                    }
                    self.remove_sch(id)?;
                }
                for c in components {
                    if self.design.symbols_of(c).next().is_none() {
                        self.remove_component(c)?;
                    }
                }
                self.record(Edit::Sheet(Change::Remove(s)))
            }
            Command::ImportPart { part } => self.ensure_part(part),
            Command::ImportLibrary { library } => self.import_library(library),
            Command::UpdateLibraryComponent { part, description, manufacturer, mpn, parameters, approved } => {
                let Some(before) = self.design.library.parts.get(part).cloned() else {
                    return invalid(format!("{part} is not in the project library"));
                };
                let mut after = before.clone();
                if let Some(d) = description {
                    after.description = d.clone();
                }
                if let Some(m) = manufacturer {
                    after.manufacturer = m.trim().to_string();
                }
                if let Some(m) = mpn {
                    after.mpn = m.trim().to_string();
                }
                for (k, v) in parameters {
                    match v {
                        Some(v) => after.parameters.insert(check_name("parameter name", k)?, v.clone()),
                        None => after.parameters.remove(k),
                    };
                }
                if let Some(a) = approved {
                    if a.iter().any(|x| x.mpn.trim().is_empty()) {
                        return invalid("approved parts need a manufacturer part number");
                    }
                    after.approved = a.clone();
                }
                // Approved parts are sourcing data; anything else is a local edit of the component.
                let mut sourcing_only = after.clone();
                sourcing_only.approved = before.approved.clone();
                if sourcing_only != before {
                    after.modified = true;
                }
                if after != before {
                    self.record(Edit::LibraryPart(Box::new(Change::Modify { before, after })))?;
                }
                Ok(())
            }
            Command::AssignComponent { components, part } => self.assign_component(components, part),
            Command::PlaceComponent { part, sheet, position, rotation, mirror, designator, value, footprint, parameters } => self
                .place_component(
                    part,
                    *sheet,
                    *position,
                    *rotation,
                    *mirror,
                    designator.as_deref(),
                    value.as_deref(),
                    footprint.as_deref(),
                    parameters,
                )
                .map(|_| ()),
            Command::RemoveComponent { component } => {
                let id = self.component(component)?;
                self.remove_component(id)
            }
            Command::ModifyComponent { component, designator, value, footprint, parameters } => {
                let id = self.component(component)?;
                let Some(mut c) = self.design.components.get(&id).cloned() else { return Err(CommandError::UnknownObject(id)) };
                if let Some(d) = designator {
                    let d = check_name("designator", d)?;
                    if self.design.components.values().any(|o| o.id != id && o.designator.eq_ignore_ascii_case(&d)) {
                        return invalid(format!("designator {d} is already used"));
                    }
                    c.designator = d;
                }
                if let Some(v) = value {
                    c.value = v.clone();
                }
                let part = self.design.library.parts.get(&c.part).cloned();
                let generic = part.as_ref().is_some_and(|p| p.kind == PartKind::Generic);
                if let Some(f) = footprint {
                    self.ensure_footprint(f)?;
                    if let Some(part) = part.filter(|_| !generic)
                        && !part.footprints.contains(f)
                    {
                        // Allowed, but the footprint must cover every mapped pad.
                        if let Some(fp) = self.design.library.footprints.get(f)
                            && let Some(sym) = self.design.library.symbols.get(&part.symbol)
                        {
                            for pin in &sym.pins {
                                for pad in part.pads_for_pin(&pin.number) {
                                    if fp.pad(pad).is_none() {
                                        return invalid(format!("footprint {f} has no pad {pad} for pin {}", pin.number));
                                    }
                                }
                            }
                        }
                    }
                    c.footprint = f.clone();
                }
                for (k, v) in parameters {
                    match v {
                        Some(v) => c.parameters.insert(k.clone(), v.clone()),
                        None => c.parameters.remove(k),
                    };
                }
                // A generic component's value and footprint pick its library component.
                let old_part = c.part.clone();
                if generic && (value.is_some() || footprint.is_some()) {
                    c.part = self.concrete_part(&old_part, &c.value, Some(&c.footprint))?;
                }
                let repointed = c.part != old_part;
                self.modify_component(c)?;
                if repointed {
                    self.prune_part(&old_part)?;
                }
                // A footprint change is applied to a placed footprint right away (position, side
                // and nets are kept: the component's identity is unchanged).
                if footprint.is_some()
                    && let Some((obj, fp)) = self.design.footprint_of(id).map(|(o, f)| (o.clone(), f.clone()))
                    && let Some(name) = footprint
                    && &fp.footprint != name
                {
                    let mut after = obj;
                    if let PcbKind::Footprint(f) = &mut after.kind {
                        f.footprint = name.clone();
                    }
                    self.modify_pcb(after)?;
                }
                Ok(())
            }
            Command::AddWire { sheet, points } => {
                let sheet = self.sheet(*sheet)?;
                if points.len() < 2 || points.len() > MAX_POINTS {
                    return invalid("a wire needs at least two points");
                }
                let mut pts: Vec<Point> = points.iter().map(|&p| check_point(p)).collect::<Result<_>>()?;
                pts.dedup();
                if pts.len() < 2 {
                    return invalid("a wire needs two distinct points");
                }
                self.insert_sch(sheet, SchKind::Wire(Wire { points: pts })).map(|_| ())
            }
            Command::AddNetLabel { sheet, position, text, rotation } => {
                let sheet = self.sheet(*sheet)?;
                let kind = SchKind::NetLabel(NetLabel {
                    position: check_point(*position)?,
                    text: check_name("net label", text)?,
                    rotation: check_rotation(*rotation, true)?,
                });
                self.insert_sch(sheet, kind).map(|_| ())
            }
            Command::AddPowerPort { sheet, position, net, style, rotation } => {
                let sheet = self.sheet(*sheet)?;
                let net = check_name("power net", net)?;
                let style =
                    style.unwrap_or(if net.to_ascii_uppercase().contains("GND") { PowerStyle::PowerGround } else { PowerStyle::Bar });
                let rotation = check_rotation(rotation.unwrap_or(if style.is_ground() { 270.0 } else { 90.0 }), true)?;
                self.insert_sch(sheet, SchKind::PowerPort(PowerPort { position: check_point(*position)?, net, style, rotation }))
                    .map(|_| ())
            }
            Command::AddPort { sheet, position, name, direction, rotation } => {
                let sheet = self.sheet(*sheet)?;
                let kind = SchKind::Port(Port {
                    position: check_point(*position)?,
                    name: check_name("port name", name)?,
                    direction: *direction,
                    rotation: check_rotation(*rotation, true)?,
                    width: mil(500.0),
                });
                self.insert_sch(sheet, kind).map(|_| ())
            }
            Command::AddJunction { sheet, position } => {
                let sheet = self.sheet(*sheet)?;
                self.insert_sch(sheet, SchKind::Junction(Junction { position: check_point(*position)? })).map(|_| ())
            }
            Command::AddNoConnect { sheet, position } => {
                let sheet = self.sheet(*sheet)?;
                self.insert_sch(sheet, SchKind::NoConnect(NoConnect { position: check_point(*position)? })).map(|_| ())
            }
            Command::AddText { sheet, position, text, size } => {
                let sheet = self.sheet(*sheet)?;
                if text.is_empty() {
                    return invalid("text must not be empty");
                }
                let size = check_length("text size", size.unwrap_or(mil(100.0)), mil(10.0))?;
                self.insert_sch(
                    sheet,
                    SchKind::Text(TextNote { position: check_point(*position)?, text: text.clone(), size, rotation: 0.0 }),
                )
                .map(|_| ())
            }
            Command::ConnectPins { from, to, net, style } => self.connect_pins(from, to, net.as_deref(), *style),
            Command::CreateNet { name, pins } => {
                let name = check_name("net name", name)?;
                if pins.is_empty() {
                    return invalid("create_net needs at least one pin");
                }
                for pin in pins {
                    let geometry = self.pin(pin)?;
                    self.label_pin(&geometry, &name)?;
                }
                Ok(())
            }
            Command::MoveObjects { ids, dx, dy, drag_wires } => self.move_objects(ids, Point::new(*dx, *dy), *drag_wires),
            Command::RotateObjects { ids, degrees, pivot } => self.rotate_objects(ids, *degrees, *pivot),
            Command::MirrorObjects { ids, pivot } => self.mirror_objects(ids, *pivot),
            Command::DeleteObjects { ids } => self.delete_objects(ids),
            Command::UpdateSchObject { object } => {
                let Some(before) = self.design.schematic.get(&object.id) else { return Err(CommandError::UnknownObject(object.id)) };
                if std::mem::discriminant(&before.kind) != std::mem::discriminant(&object.kind) {
                    return invalid("an object's type cannot be changed");
                }
                if !self.design.sheets.contains_key(&object.sheet) {
                    return invalid(format!("sheet {} does not exist", object.sheet));
                }
                if let (SchKind::Symbol(a), SchKind::Symbol(b)) = (&before.kind, &object.kind)
                    && a.component != b.component
                {
                    return invalid("a symbol's component cannot be changed; place a new component instead");
                }
                if let SchKind::Symbol(s) = &object.kind {
                    check_rotation(s.rotation, true)?;
                }
                self.modify_sch(object.clone())
            }
            Command::UpdatePcbObject { object } => {
                let Some(before) = self.design.pcb.get(&object.id) else { return Err(CommandError::UnknownObject(object.id)) };
                if std::mem::discriminant(&before.kind) != std::mem::discriminant(&object.kind) {
                    return invalid("an object's type cannot be changed");
                }
                match (&before.kind, &object.kind) {
                    (PcbKind::Footprint(a), PcbKind::Footprint(b)) if a.component != b.component => {
                        return invalid("a footprint's component cannot be changed");
                    }
                    (_, PcbKind::Footprint(b)) => {
                        let name = b.footprint.clone();
                        self.ensure_footprint(&name)?;
                    }
                    (_, PcbKind::Track(t)) => self.check_track(t)?,
                    (_, PcbKind::Via(v)) => self.check_via(v)?,
                    _ => {}
                }
                self.modify_pcb(object.clone())
            }
            Command::UpdatePcb {} => self.update_pcb(),
            Command::PlaceFootprint { component, position, rotation, side } => {
                let id = self.component(component)?;
                let rotation = rotation.map(|r| check_rotation(r, false)).transpose()?;
                let position = check_point(*position)?;
                match self.design.footprint_of(id).map(|(o, _)| o.clone()) {
                    Some(mut obj) => {
                        if obj.is_locked() {
                            return Err(CommandError::Locked(obj.id));
                        }
                        if let PcbKind::Footprint(f) = &mut obj.kind {
                            f.position = position;
                            if let Some(r) = rotation {
                                f.rotation = r;
                            }
                            if let Some(s) = side {
                                f.side = *s;
                            }
                        }
                        self.modify_pcb(obj)
                    }
                    None => {
                        let netlist = sch_conn::compile(&self.design);
                        let Some(c) = self.design.components.get(&id).cloned() else { return Err(CommandError::UnknownObject(id)) };
                        self.ensure_footprint(&c.footprint)?;
                        let pad_nets = netlist.pad_nets(&self.design, id);
                        self.insert_pcb(PcbKind::Footprint(Footprint {
                            component: Some(id),
                            footprint: c.footprint,
                            position,
                            rotation: rotation.unwrap_or(0.0),
                            side: side.unwrap_or_default(),
                            locked: false,
                            pad_nets,
                        }))
                        .map(|_| ())
                    }
                }
            }
            Command::AddTrack { start, end, layer, width, net } => {
                self.add_track_run(&[*start, *end], *layer, *width, net.clone()).map(|_| ())
            }
            Command::AddTracks { points, layer, width, net } => self.add_track_run(points, *layer, *width, net.clone()).map(|_| ()),
            Command::RouteNet { net, layer, width } => self.route_net(net, *layer, *width),
            Command::AddVia { position, diameter, drill, net, from, to } => {
                let rules = &self.design.board.rules;
                let copper = self.design.board.stack.copper_layers();
                let via = Via {
                    position: check_point(*position)?,
                    diameter: diameter.unwrap_or(rules.preferred_via_diameter),
                    drill: drill.unwrap_or(rules.preferred_via_drill),
                    net: net.clone().or_else(|| self.net_at(*position, None)),
                    from: from.or(copper.first().copied()).unwrap_or(Layer::TopCopper),
                    to: to.or(copper.last().copied()).unwrap_or(Layer::BottomCopper),
                };
                self.check_via(&via)?;
                self.insert_pcb(PcbKind::Via(via)).map(|_| ())
            }
            Command::ModifyTrack { track, start, end, width, layer, net } => {
                let Some(mut obj) = self.design.pcb.get(track).cloned() else { return Err(CommandError::UnknownObject(*track)) };
                let PcbKind::Track(t) = &mut obj.kind else { return invalid(format!("{track} is not a track")) };
                if let Some(s) = start {
                    t.start = check_point(*s)?;
                }
                if let Some(e) = end {
                    t.end = check_point(*e)?;
                }
                if let Some(w) = width {
                    t.width = *w;
                }
                if let Some(l) = layer {
                    t.layer = *l;
                }
                if let Some(n) = net {
                    t.net = if n.is_empty() { None } else { Some(n.clone()) };
                }
                let t = t.clone();
                self.check_track(&t)?;
                self.modify_pcb(obj)
            }
            Command::AddZone { layer, net, outline, priority } => {
                if !layer.is_copper() || !self.design.board.stack.contains(*layer) {
                    return invalid(format!("{layer} is not a copper layer of this board"));
                }
                let outline = check_polygon(outline)?;
                self.insert_pcb(PcbKind::Zone(Zone {
                    layer: *layer,
                    net: net.clone().filter(|n| !n.is_empty()),
                    outline,
                    priority: priority.unwrap_or(0),
                    clearance: None,
                    fill_mode: Default::default(),
                    filled: Vec::new(),
                }))
                .map(|_| ())
            }
            Command::AddKeepout { layers, outline } => {
                if let Some(bad) = layers.iter().find(|l| !l.is_copper()) {
                    return invalid(format!("{bad} is not a copper layer"));
                }
                let outline = check_polygon(outline)?;
                self.insert_pcb(PcbKind::Keepout(Keepout {
                    layers: layers.clone(),
                    outline,
                    no_tracks: true,
                    no_vias: true,
                    no_pads: true,
                    no_pours: true,
                }))
                .map(|_| ())
            }
            Command::AddBoardLine { layer, start, end, width } => {
                if layer.is_copper() {
                    return invalid("use add_track for copper");
                }
                let line = PcbLine {
                    layer: *layer,
                    start: check_point(*start)?,
                    end: check_point(*end)?,
                    width: check_length("line width", *width, 0)?,
                };
                self.insert_pcb(PcbKind::Line(line)).map(|_| ())
            }
            Command::AddBoardText { layer, position, text, height } => {
                if text.is_empty() {
                    return invalid("text must not be empty");
                }
                let t = PcbText {
                    layer: *layer,
                    position: check_point(*position)?,
                    text: text.clone(),
                    height: check_length("text height", *height, mil(10.0))?,
                    rotation: 0.0,
                };
                self.insert_pcb(PcbKind::Text(t)).map(|_| ())
            }
            Command::SetBoardOutline { outline } => {
                let mut board = self.design.board.clone();
                board.outline = check_polygon(outline)?;
                self.modify_board(board)
            }
            Command::SetDesignRules { rules } => {
                let lengths = [
                    rules.clearance,
                    rules.min_track_width,
                    rules.preferred_track_width,
                    rules.max_track_width,
                    rules.min_via_diameter,
                    rules.min_via_drill,
                    rules.preferred_via_diameter,
                    rules.preferred_via_drill,
                    rules.min_annular_ring,
                    rules.board_edge_clearance,
                    rules.min_hole_size,
                    rules.max_hole_size,
                ];
                if lengths.iter().any(|&l| !(0..=MAX_COORD).contains(&l)) {
                    return invalid("design rule values must be between 0 and 10 m");
                }
                if rules.min_track_width > rules.max_track_width {
                    return invalid("minimum track width exceeds the maximum");
                }
                if rules.preferred_via_drill >= rules.preferred_via_diameter {
                    return invalid("the preferred via drill must be smaller than its diameter");
                }
                let mut board = self.design.board.clone();
                board.rules = rules.clone();
                self.modify_board(board)
            }
            Command::SetLayerStack { copper_layers } => {
                if !(2..=32).contains(copper_layers) || copper_layers % 2 != 0 {
                    return invalid("the copper layer count must be even, from 2 to 32");
                }
                let stack = LayerStack::standard(*copper_layers);
                let kept = stack.copper_layers();
                let orphaned = self.design.pcb.values().find(|o| match &o.kind {
                    PcbKind::Track(t) => t.layer.is_copper() && !kept.contains(&t.layer),
                    PcbKind::Zone(z) => z.layer.is_copper() && !kept.contains(&z.layer),
                    PcbKind::Via(v) => !kept.contains(&v.from) || !kept.contains(&v.to),
                    _ => false,
                });
                if let Some(o) = orphaned {
                    return invalid(format!("{} uses a layer the new stack removes; move or delete it first", o.id));
                }
                let mut board = self.design.board.clone();
                board.stack = stack;
                self.modify_board(board)
            }
            Command::SetOrigin { origin } => {
                let mut board = self.design.board.clone();
                board.origin = check_point(*origin)?;
                self.modify_board(board)
            }
            Command::AlignObjects { ids, mode } => self.align(ids, *mode),
            Command::DistributeObjects { ids, axis } => self.distribute(ids, *axis),
            Command::LockObjects { ids, locked } => {
                for id in ids {
                    let Some(mut obj) = self.design.pcb.get(id).cloned() else { return Err(CommandError::UnknownObject(*id)) };
                    if let PcbKind::Footprint(f) = &mut obj.kind {
                        f.locked = *locked;
                        self.modify_pcb(obj)?;
                    }
                }
                Ok(())
            }
            Command::Batch { commands } => {
                if commands.len() > 10_000 {
                    return invalid("a batch is limited to 10000 commands");
                }
                for (i, c) in commands.iter().enumerate() {
                    self.run(c).map_err(|e| CommandError::Batch { index: i, error: Box::new(e) })?;
                }
                Ok(())
            }
        }
    }

    // --- schematic ------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn place_component(
        &mut self,
        part: &str,
        sheet: Option<ObjectId>,
        position: Point,
        rotation: f64,
        mirror: bool,
        designator: Option<&str>,
        value: Option<&str>,
        footprint: Option<&str>,
        parameters: &BTreeMap<String, String>,
    ) -> Result<ObjectId> {
        let sheet = self.sheet(sheet)?;
        let rotation = check_rotation(rotation, true)?;
        let position = check_point(position)?;
        let (base, _) = self.lookup_part(part)?;
        let value = value.map(str::to_string).unwrap_or(base.default_value.clone());
        let part = self.concrete_part(part, &value, footprint)?;
        let part = part.as_str();
        let Some(def) = self.design.library.parts.get(part).cloned() else { return invalid(format!("unknown part {part}")) };
        let footprint = if def.kind == PartKind::Generic { def.footprints.first().map(String::as_str) } else { footprint };
        let designator = match designator {
            Some(d) => {
                let d = check_name("designator", d)?;
                if self.design.component_by_designator(&d).is_some() {
                    return invalid(format!("designator {d} is already used"));
                }
                d
            }
            None => self.design.next_designator(&def.designator_prefix),
        };
        let footprint = match footprint {
            Some(f) => {
                self.ensure_footprint(f)?;
                f.to_string()
            }
            None => def.footprints.first().cloned().ok_or_else(|| CommandError::Invalid(format!("part {part} has no footprint")))?,
        };
        let id = self.design.alloc_id();
        self.record(Edit::Component(Change::Insert(Component {
            id,
            designator,
            value,
            part: part.to_string(),
            footprint,
            parameters: parameters.clone(),
        })))?;
        self.insert_sch(
            sheet,
            SchKind::Symbol(SymbolInstance { component: id, position, rotation, mirror, designator_offset: None, value_offset: None }),
        )?;
        Ok(id)
    }

    /// Points on a sheet where something electrical sits (pins, wire vertices, labels, ports).
    fn occupied_points(&self, sheet: ObjectId) -> BTreeSet<Point> {
        let mut points: BTreeSet<Point> = self.design.sheet_pins(sheet).into_iter().map(|p| p.position).collect();
        for o in self.design.sheet_objects(sheet) {
            match &o.kind {
                SchKind::Wire(w) => points.extend(w.points.iter().copied()),
                SchKind::NetLabel(l) => {
                    points.insert(l.position);
                }
                SchKind::PowerPort(p) => {
                    points.insert(p.position);
                }
                SchKind::Port(p) => {
                    points.insert(p.position);
                }
                _ => {}
            }
        }
        points
    }

    fn sheet_segments(&self, sheet: ObjectId) -> Vec<(Point, Point)> {
        self.design
            .sheet_objects(sheet)
            .filter_map(|o| {
                if let SchKind::Wire(w) = &o.kind {
                    Some(w.points.windows(2).filter_map(|s| Some((*s.first()?, *s.get(1)?))).collect::<Vec<_>>())
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    /// Whether a new wire path touches anything but its own endpoints.
    fn path_is_clean(&self, sheet: ObjectId, path: &[Point]) -> bool {
        let (Some(&start), Some(&end)) = (path.first(), path.last()) else { return false };
        let on = |p: Point, a: Point, b: Point| {
            let cross = (b.x - a.x) as i128 * (p.y - a.y) as i128 - (b.y - a.y) as i128 * (p.x - a.x) as i128;
            cross == 0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
        };
        let occupied = self.occupied_points(sheet);
        let segments = self.sheet_segments(sheet);
        for seg in path.windows(2) {
            let (Some(&a), Some(&b)) = (seg.first(), seg.get(1)) else { continue };
            if occupied.iter().any(|&p| p != start && p != end && on(p, a, b)) {
                return false;
            }
            // Collinear overlap with an existing wire would merge nets silently.
            for &(c, d) in &segments {
                let overlap = (on(c, a, b) || on(d, a, b) || on(a, c, d) || on(b, c, d))
                    && ((b.x - a.x) as i128 * (d.y - c.y) as i128 - (b.y - a.y) as i128 * (d.x - c.x) as i128) == 0;
                if overlap {
                    return false;
                }
            }
        }
        // The corner itself must be free as well.
        path.iter().skip(1).take(path.len().saturating_sub(2)).all(|p| !occupied.contains(p))
    }

    fn connect_pins(&mut self, from: &PinRef, to: &PinRef, net: Option<&str>, style: ConnectStyle) -> Result<()> {
        let a = self.pin(from)?;
        let b = self.pin(to)?;
        if a.component == b.component && a.number == b.number {
            return invalid("cannot connect a pin to itself");
        }
        let sheet_a = self.design.schematic.get(&a.symbol).map(|o| o.sheet);
        let sheet_b = self.design.schematic.get(&b.symbol).map(|o| o.sheet);
        if style != ConnectStyle::Label
            && sheet_a == sheet_b
            && let Some(sheet) = sheet_a
        {
            let candidates = [
                vec![a.position, Point::new(b.position.x, a.position.y), b.position],
                vec![a.position, Point::new(a.position.x, b.position.y), b.position],
            ];
            for mut path in candidates {
                path.dedup();
                if self.path_is_clean(sheet, &path) {
                    self.insert_sch(sheet, SchKind::Wire(Wire { points: path }))?;
                    if let Some(name) = net {
                        let name = check_name("net name", name)?;
                        self.label_pin(&a, &name)?;
                    }
                    return Ok(());
                }
            }
            if style == ConnectStyle::Wire {
                return invalid(format!("no clean orthogonal wire route from {from} to {to}; use style \"label\""));
            }
        } else if style == ConnectStyle::Wire {
            return invalid("pins on different sheets can only be connected with labels");
        }
        let netlist = sch_conn::compile(&self.design);
        let existing = |p: &PinGeometry| netlist.net_of_pin(p.component, &p.number).filter(|n| n.named).map(|n| n.name.clone());
        let name = match net {
            Some(n) => check_name("net name", n)?,
            None => existing(&a).or_else(|| existing(&b)).unwrap_or_else(|| {
                let des = self.design.components.get(&a.component).map(|c| c.designator.clone()).unwrap_or_default();
                format!("N_{des}_{}", a.number)
            }),
        };
        self.label_pin(&a, &name)?;
        self.label_pin(&b, &name)
    }

    /// Attach a short wire stub and a net label to a pin, unless the pin is already on that net.
    fn label_pin(&mut self, pin: &PinGeometry, name: &str) -> Result<()> {
        let netlist = sch_conn::compile(&self.design);
        if netlist.net_of_pin(pin.component, &pin.number).is_some_and(|n| n.names.iter().any(|x| x == name)) {
            return Ok(());
        }
        let Some(sheet) = self.design.schematic.get(&pin.symbol).map(|o| o.sheet) else {
            return Err(CommandError::UnknownObject(pin.symbol));
        };
        for length in [200.0, 300.0, 400.0, 600.0] {
            let end = pin.position + Point::new(mil(length), 0).rotated(pin.angle);
            if self.path_is_clean(sheet, &[pin.position, end]) {
                self.insert_sch(sheet, SchKind::Wire(Wire { points: vec![pin.position, end] }))?;
                // Label text runs left-to-right (or bottom-to-top) along the stub: for pins pointing
                // left or down it starts at the far end, otherwise just beyond the pin.
                let vertical = (pin.angle - 90.0).abs() < 1.0 || (pin.angle - 270.0).abs() < 1.0;
                let rotation = if vertical { 90.0 } else { 0.0 };
                let backwards = (pin.angle - 180.0).abs() < 1.0 || (pin.angle - 270.0).abs() < 1.0;
                let label_at = if backwards { end } else { pin.position + Point::new(mil(50.0), 0).rotated(pin.angle) };
                self.insert_sch(sheet, SchKind::NetLabel(NetLabel { position: label_at, text: name.to_string(), rotation }))?;
                return Ok(());
            }
        }
        // No stub fits: a label on the pin's own hot spot connects too (Altium allows it), as
        // long as nothing else sits there.
        let others_here = self.design.sheet_pins(sheet).iter().filter(|p| p.position == pin.position).count()
            + self.design.sheet_objects(sheet).filter(|o| matches!(&o.kind, SchKind::NetLabel(l) if l.position == pin.position)).count();
        if others_here == 1 {
            let rotation = if (pin.angle - 90.0).abs() < 1.0 || (pin.angle - 270.0).abs() < 1.0 { 90.0 } else { 0.0 };
            self.insert_sch(sheet, SchKind::NetLabel(NetLabel { position: pin.position, text: name.to_string(), rotation }))?;
            return Ok(());
        }
        invalid(format!("no free space to attach a net label to pin {}", pin.number))
    }

    fn import_library(&mut self, library: &Library) -> Result<()> {
        if let Some(problem) = library.parts.values().flat_map(|p| library.check_part(p)).next() {
            return invalid(problem);
        }
        let plan = self.design.library.merge_plan(library);
        for sym in plan.symbols.into_values() {
            self.record(Edit::LibrarySymbol(Change::Insert(sym)))?;
        }
        for fp in plan.footprints.into_values() {
            self.record(Edit::LibraryFootprint(Change::Insert(fp)))?;
        }
        for part in plan.parts.into_values() {
            check_name("component name", &part.name)?;
            self.record(Edit::LibraryPart(Box::new(Change::Insert(part))))?;
        }
        Ok(())
    }

    fn assign_component(&mut self, components: &[ComponentRef], part: &str) -> Result<()> {
        let (target, _) = self.lookup_part(part)?;
        let pins_of = |lib: &[&Library], p: &PartDef| -> Option<BTreeSet<String>> {
            lib.iter().find_map(|l| l.symbols.get(&p.symbol)).map(|s| s.pins.iter().map(|x| x.number.clone()).collect())
        };
        for r in components {
            let id = self.component(r)?;
            let Some(mut c) = self.design.components.get(&id).cloned() else { return Err(CommandError::UnknownObject(id)) };
            let old_part = c.part.clone();
            let libs: Vec<&Library> = std::iter::once(&self.design.library).chain(self.sources.iter()).collect();
            let current = self.design.library.parts.get(&old_part);
            if let Some(current) = current
                && pins_of(&libs, current) != pins_of(&libs, &target)
            {
                return invalid(format!("{part} has different pin numbers than {}'s {}", c.designator, current.name));
            }
            let (value, footprint) = if target.kind == PartKind::Generic {
                let keep = target.family.is_none() && target.footprints.contains(&c.footprint);
                (c.value.clone(), keep.then(|| c.footprint.clone()))
            } else {
                (if target.default_value.is_empty() { c.value.clone() } else { target.default_value.clone() }, None)
            };
            c.part = self.concrete_part(part, &value, footprint.as_deref())?;
            let Some(def) = self.design.library.parts.get(&c.part) else { return invalid(format!("unknown part {}", c.part)) };
            c.value = value;
            let new_fp = def.footprints.first().cloned().unwrap_or_else(|| c.footprint.clone());
            let fp_changed = new_fp != c.footprint;
            c.footprint = new_fp.clone();
            let new_part = c.part.clone();
            self.modify_component(c)?;
            if fp_changed && let Some(obj) = self.design.footprint_of(id).map(|(o, _)| o.clone()) {
                let mut after = obj;
                if let PcbKind::Footprint(f) = &mut after.kind {
                    f.footprint = new_fp;
                }
                self.modify_pcb(after)?;
            }
            if old_part != new_part {
                self.prune_part(&old_part)?;
            }
        }
        Ok(())
    }

    fn move_objects(&mut self, ids: &[ObjectId], delta: Point, drag_wires: bool) -> Result<()> {
        check_point(delta)?;
        if delta == Point::ZERO {
            return Ok(());
        }
        let ids = self.expand_components(ids)?;
        let before = self.snapshot(&ids);
        for id in &ids {
            if let Some(obj) = self.design.schematic.get(id).cloned() {
                let mut after = obj;
                after.translate(delta);
                self.modify_sch(after)?;
            } else if let Some(obj) = self.design.pcb.get(id).cloned() {
                if obj.is_locked() {
                    return Err(CommandError::Locked(*id));
                }
                let mut after = obj;
                after.translate(delta);
                self.modify_pcb(after)?;
            } else {
                return Err(CommandError::UnknownObject(*id));
            }
        }
        if drag_wires {
            self.rubber_band(&ids, before)?;
        }
        Ok(())
    }

    /// Copies of the schematic and board objects in `ids`, taken before they are transformed.
    fn snapshot(&self, ids: &[ObjectId]) -> (Vec<SchObject>, Vec<PcbObject>) {
        let sch = ids.iter().filter_map(|id| self.design.schematic.get(id).cloned()).collect();
        let pcb = ids.iter().filter_map(|id| self.design.pcb.get(id).cloned()).collect();
        (sch, pcb)
    }

    /// Keep wires on moved pins and labels, and tracks on moved pads, attached after `ids` were
    /// transformed from `before`.
    fn rubber_band(&mut self, ids: &[ObjectId], before: (Vec<SchObject>, Vec<PcbObject>)) -> Result<()> {
        let moved: BTreeSet<ObjectId> = ids.iter().copied().collect();
        let (sch_before, pcb_before) = before;
        let sch_after: Vec<SchObject> = sch_before.iter().filter_map(|o| self.design.schematic.get(&o.id).cloned()).collect();
        let moves = drag::point_moves(&self.design, &sch_before, &sch_after);
        let sheets: BTreeSet<ObjectId> = sch_before.iter().map(|o| o.sheet).collect();
        for sheet in sheets {
            for wire in drag::follow_wires(&self.design, sheet, &moves, &moved) {
                self.modify_sch(wire)?;
            }
        }
        let pcb_after: Vec<PcbObject> = pcb_before.iter().filter_map(|o| self.design.pcb.get(&o.id).cloned()).collect();
        let pad_moves = drag::pad_moves(&self.design, &pcb_before, &pcb_after);
        for track in drag::follow_tracks(&self.design, &pad_moves, &moved) {
            self.modify_pcb(track)?;
        }
        Ok(())
    }

    /// Component ids stand for their symbols and footprint.
    fn expand_components(&self, ids: &[ObjectId]) -> Result<Vec<ObjectId>> {
        let mut out = Vec::new();
        for id in ids {
            if self.design.components.contains_key(id) {
                out.extend(self.design.symbols_of(*id).map(|(o, _)| o.id));
                out.extend(self.design.footprint_of(*id).map(|(o, _)| o.id));
            } else if self.design.schematic.contains_key(id) || self.design.pcb.contains_key(id) {
                out.push(*id);
            } else {
                return Err(CommandError::UnknownObject(*id));
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn selection_center(&self, ids: &[ObjectId]) -> Option<Point> {
        let bounds = ids
            .iter()
            .filter_map(|id| {
                self.design
                    .schematic
                    .get(id)
                    .map(|o| self.design.sch_bounds(o))
                    .or_else(|| self.design.pcb.get(id).map(|o| self.design.pcb_bounds(o)))
            })
            .reduce(Rect::union)?;
        Some(bounds.center())
    }

    fn rotate_objects(&mut self, ids: &[ObjectId], degrees: f64, pivot: Option<Point>) -> Result<()> {
        let ids = self.expand_components(ids)?;
        let any_sch = ids.iter().any(|id| self.design.schematic.contains_key(id));
        let degrees = check_rotation(degrees, any_sch)?;
        let single = ids.len() == 1;
        let group_pivot = pivot.or_else(|| self.selection_center(&ids));
        let before = self.snapshot(&ids);
        for id in &ids {
            if let Some(obj) = self.design.schematic.get(id).cloned() {
                let mut pivot = if single { pivot.unwrap_or(obj.anchor()) } else { group_pivot.unwrap_or(obj.anchor()) };
                pivot = pivot.snapped(mil(10.0));
                let mut after = obj;
                after.rotate_about(pivot, degrees);
                self.modify_sch(after)?;
            } else if let Some(obj) = self.design.pcb.get(id).cloned() {
                if obj.is_locked() {
                    return Err(CommandError::Locked(*id));
                }
                let pivot = if single { pivot.unwrap_or(obj.anchor()) } else { group_pivot.unwrap_or(obj.anchor()) };
                let mut after = obj;
                after.rotate_about(pivot, degrees);
                self.modify_pcb(after)?;
            }
        }
        self.rubber_band(&ids, before)
    }

    fn mirror_objects(&mut self, ids: &[ObjectId], pivot: Option<Point>) -> Result<()> {
        let ids = self.expand_components(ids)?;
        let group_pivot = pivot.or_else(|| self.selection_center(&ids));
        let (sch_before, _) = self.snapshot(&ids);
        for id in &ids {
            if let Some(obj) = self.design.schematic.get(id).cloned() {
                let pivot = if ids.len() == 1 { pivot.unwrap_or(obj.anchor()) } else { group_pivot.unwrap_or(obj.anchor()) };
                let mut after = obj;
                after.mirror_x(pivot.snapped(mil(10.0)));
                self.modify_sch(after)?;
            } else if let Some(obj) = self.design.pcb.get(id).cloned() {
                if obj.is_locked() {
                    return Err(CommandError::Locked(*id));
                }
                let mut after = obj;
                if let PcbKind::Footprint(f) = &mut after.kind {
                    f.side = if f.side == Side::Top { Side::Bottom } else { Side::Top };
                    self.modify_pcb(after)?;
                }
            }
        }
        // Flipped footprints change copper side, so only schematic wiring follows a mirror.
        self.rubber_band(&ids, (sch_before, Vec::new()))
    }

    fn delete_objects(&mut self, ids: &[ObjectId]) -> Result<()> {
        for id in ids {
            if self.design.components.contains_key(id) {
                self.remove_component(*id)?;
            } else if let Some(obj) = self.design.schematic.get(id).cloned() {
                self.remove_sch(*id)?;
                if let SchKind::Symbol(s) = &obj.kind
                    && self.design.symbols_of(s.component).next().is_none()
                {
                    self.remove_component(s.component)?;
                }
            } else if let Some(obj) = self.design.pcb.get(id) {
                if obj.is_locked() {
                    return Err(CommandError::Locked(*id));
                }
                self.remove_pcb(*id)?;
            } else if self.design.sheets.contains_key(id) {
                return invalid("use remove_sheet to delete a sheet");
            }
            // Ids already removed (e.g. a footprint deleted with its component) are skipped.
        }
        Ok(())
    }

    fn align(&mut self, ids: &[ObjectId], mode: AlignMode) -> Result<()> {
        let ids = self.expand_components(ids)?;
        let bounds: Vec<(ObjectId, Rect)> = ids
            .iter()
            .filter_map(|id| {
                self.design
                    .schematic
                    .get(id)
                    .map(|o| (*id, self.design.sch_bounds(o)))
                    .or_else(|| self.design.pcb.get(id).map(|o| (*id, self.design.pcb_bounds(o))))
            })
            .collect();
        let Some(all) = bounds.iter().map(|(_, b)| *b).reduce(Rect::union) else { return Ok(()) };
        for (id, b) in bounds {
            let delta = match mode {
                AlignMode::Left => Point::new(all.min.x - b.min.x, 0),
                AlignMode::Right => Point::new(all.max.x - b.max.x, 0),
                AlignMode::Bottom => Point::new(0, all.min.y - b.min.y),
                AlignMode::Top => Point::new(0, all.max.y - b.max.y),
                AlignMode::CenterHorizontal => Point::new(all.center().x - b.center().x, 0),
                AlignMode::CenterVertical => Point::new(0, all.center().y - b.center().y),
            };
            let delta = if self.design.schematic.contains_key(&id) { delta.snapped(mil(10.0)) } else { delta };
            self.move_objects(&[id], delta, true)?;
        }
        Ok(())
    }

    fn distribute(&mut self, ids: &[ObjectId], axis: Axis) -> Result<()> {
        let ids = self.expand_components(ids)?;
        let mut centers: Vec<(ObjectId, Point)> = ids
            .iter()
            .filter_map(|id| {
                self.design
                    .schematic
                    .get(id)
                    .map(|o| (*id, self.design.sch_bounds(o).center()))
                    .or_else(|| self.design.pcb.get(id).map(|o| (*id, self.design.pcb_bounds(o).center())))
            })
            .collect();
        if centers.len() < 3 {
            return Ok(());
        }
        let key = |p: &Point| if axis == Axis::Horizontal { p.x } else { p.y };
        centers.sort_by_key(|(_, c)| key(c));
        let (Some(first), Some(last)) = (centers.first(), centers.last()) else { return Ok(()) };
        let (start, end) = (key(&first.1), key(&last.1));
        let step = (end - start) / (centers.len() as Coord - 1);
        let moves: Vec<(ObjectId, Point)> = centers
            .iter()
            .enumerate()
            .map(|(i, (id, c))| {
                let target = start + step * i as Coord;
                let d = target - key(c);
                (*id, if axis == Axis::Horizontal { Point::new(d, 0) } else { Point::new(0, d) })
            })
            .collect();
        for (id, delta) in moves {
            let delta = if self.design.schematic.contains_key(&id) { delta.snapped(mil(10.0)) } else { delta };
            self.move_objects(&[id], delta, true)?;
        }
        Ok(())
    }

    // --- PCB --------------------------------------------------------------------------------------

    fn check_track(&self, t: &Track) -> Result<()> {
        if !t.layer.is_copper() || !self.design.board.stack.contains(t.layer) {
            return invalid(format!("{} is not a copper layer of this board", t.layer));
        }
        check_length("track width", t.width, 1000)?;
        check_point(t.start)?;
        check_point(t.end)?;
        if t.start == t.end {
            return invalid("a track needs two distinct points");
        }
        Ok(())
    }

    fn check_via(&self, v: &Via) -> Result<()> {
        check_length("via diameter", v.diameter, 1000)?;
        check_length("via drill", v.drill, 1000)?;
        if v.drill >= v.diameter {
            return invalid("via drill must be smaller than its diameter");
        }
        for l in [v.from, v.to] {
            if !self.design.board.stack.contains(l) {
                return invalid(format!("{l} is not a copper layer of this board"));
            }
        }
        if v.from == v.to {
            return invalid("a via must span two different layers");
        }
        Ok(())
    }

    /// The net of copper touching `p` (on `layer` if given): pads first, then tracks and vias.
    pub fn net_at(&self, p: Point, layer: Option<Layer>) -> Option<String> {
        let on_layer = |l: &[Layer]| layer.is_none_or(|x| l.contains(&x));
        for pad in self.design.all_pads() {
            if pad.net.is_some() && on_layer(&pad.layers) && pad.shape.contains(p) {
                return pad.net;
            }
        }
        for o in self.design.pcb.values() {
            match &o.kind {
                PcbKind::Track(t) if t.net.is_some() && layer.is_none_or(|l| l == t.layer) => {
                    if hephaestus_core::geometry::Shape::capsule(t.start, t.end, t.width).contains(p) {
                        return t.net.clone();
                    }
                }
                PcbKind::Via(v) if v.net.is_some() && v.position.distance(p) <= v.diameter as f64 / 2.0 => return v.net.clone(),
                _ => {}
            }
        }
        None
    }

    fn add_track_run(&mut self, points: &[Point], layer: Layer, width: Option<Coord>, net: Option<String>) -> Result<Vec<ObjectId>> {
        if points.len() < 2 || points.len() > MAX_POINTS {
            return invalid("a track needs at least two points");
        }
        let mut pts: Vec<Point> = points.iter().map(|&p| check_point(p)).collect::<Result<_>>()?;
        pts.dedup();
        if pts.len() < 2 {
            return invalid("a track needs two distinct points");
        }
        let net = net.filter(|n| !n.is_empty()).or_else(|| {
            let first = pts.first().copied()?;
            let last = pts.last().copied()?;
            self.net_at(first, Some(layer)).or_else(|| self.net_at(last, Some(layer)))
        });
        let width = width.unwrap_or_else(|| self.design.board.rules.track_width_for(net.as_deref()));
        let mut ids = Vec::new();
        for seg in pts.windows(2) {
            let (Some(&start), Some(&end)) = (seg.first(), seg.get(1)) else { continue };
            let track = Track { start, end, width, layer, net: net.clone() };
            self.check_track(&track)?;
            ids.push(self.insert_pcb(PcbKind::Track(track))?);
        }
        Ok(ids)
    }

    fn route_net(&mut self, net: &str, layer: Option<Layer>, width: Option<Coord>) -> Result<()> {
        let conn = pcb_conn::analyze(&self.design);
        let lines: Vec<_> = conn.unrouted.iter().filter(|r| r.net == net).cloned().collect();
        if lines.is_empty() {
            if conn.nets.contains_key(net) {
                return Ok(());
            }
            return invalid(format!("net {net} has no pads on the board; run update_pcb first"));
        }
        let pads = self.design.all_pads();
        let layers_of = |item: &pcb_conn::CopperRef| -> Vec<Layer> {
            match item {
                pcb_conn::CopperRef::Pad { footprint, number } => {
                    pads.iter().find(|p| p.footprint == *footprint && &p.number == number).map(|p| p.layers.clone()).unwrap_or_default()
                }
                pcb_conn::CopperRef::Track { id } => match self.design.pcb.get(id).map(|o| &o.kind) {
                    Some(PcbKind::Track(t)) => vec![t.layer],
                    _ => Vec::new(),
                },
                pcb_conn::CopperRef::Via { .. } => self.design.board.stack.copper_layers(),
                pcb_conn::CopperRef::Zone { id } => match self.design.pcb.get(id).map(|o| &o.kind) {
                    Some(PcbKind::Zone(z)) => vec![z.layer],
                    _ => Vec::new(),
                },
            }
        };
        let planned: Vec<_> = lines.into_iter().map(|line| (layers_of(&line.from_item), layers_of(&line.to_item), line)).collect();
        for (a, b, line) in planned {
            let chosen = match layer {
                Some(l) if a.contains(&l) && b.contains(&l) => l,
                Some(l) => {
                    return invalid(format!(
                        "{} or {} is not on {l}; route this connection manually with a via",
                        line.from_item.object(),
                        line.to_item.object()
                    ));
                }
                None => match a.iter().find(|l| b.contains(l)) {
                    Some(l) => *l,
                    None => return invalid(format!("net {net} connects objects on different layers; route it manually with a via")),
                },
            };
            let w = width.unwrap_or_else(|| self.design.board.rules.track_width_for(Some(net)));
            let mut reversed = route_45(line.to, line.from);
            reversed.reverse();
            let candidates = [route_45(line.from, line.to), reversed];
            let Some(path) = candidates.iter().find(|path| self.path_clear(path, chosen, w, net)) else {
                return invalid(format!(
                    "no direct 45° route for net {net} from {} to {} clears other copper on {chosen}; route it interactively",
                    line.from_item.object(),
                    line.to_item.object()
                ));
            };
            self.add_track_run(path, chosen, Some(w), Some(net.to_string()))?;
        }
        Ok(())
    }

    /// Whether a track run along `path` keeps the clearance rule to copper of other nets.
    fn path_clear(&self, path: &[Point], layer: Layer, width: Coord, net: &str) -> bool {
        use hephaestus_core::geometry::Shape;
        let rules = &self.design.board.rules;
        let segments: Vec<Shape> = path.windows(2).filter_map(|s| Some(Shape::capsule(*s.first()?, *s.get(1)?, width))).collect();
        let clear = |shape: &Shape, other: Option<&str>| {
            let required = rules.clearance_between(Some(net), other) as f64;
            segments.iter().all(|seg| seg.gap(shape) >= required)
        };
        for pad in self.design.all_pads() {
            if pad.layers.contains(&layer) && pad.net.as_deref() != Some(net) && !clear(&pad.shape, pad.net.as_deref()) {
                return false;
            }
        }
        for o in self.design.pcb.values() {
            match &o.kind {
                PcbKind::Track(t)
                    if t.layer == layer
                        && t.net.as_deref() != Some(net)
                        && !clear(&Shape::capsule(t.start, t.end, t.width), t.net.as_deref()) =>
                {
                    return false;
                }
                PcbKind::Via(v) if v.net.as_deref() != Some(net) && !clear(&Shape::circle(v.position, v.diameter), v.net.as_deref()) => {
                    return false;
                }
                _ => {}
            }
        }
        true
    }

    /// Altium's Update PCB Document (engineering change order from the schematic).
    fn update_pcb(&mut self) -> Result<()> {
        let netlist = sch_conn::compile(&self.design);
        let outline = self.design.board.outline_bounds().unwrap_or(Rect::from_center(Point::ZERO, 0, 0));
        let mut slot = 0;
        let components: Vec<Component> = self.design.components.values().cloned().collect();
        for c in components {
            if self.design.symbols_of(c.id).next().is_none() {
                continue;
            }
            let pad_nets = netlist.pad_nets(&self.design, c.id);
            self.ensure_footprint(&c.footprint)?;
            match self.design.footprint_of(c.id).map(|(o, _)| o.clone()) {
                Some(mut obj) => {
                    if let PcbKind::Footprint(f) = &mut obj.kind {
                        f.footprint = c.footprint.clone();
                        f.pad_nets = pad_nets;
                    }
                    self.modify_pcb(obj)?;
                }
                None => {
                    // New footprints land in a grid to the right of the board, like Altium's
                    // component room placement.
                    let (col, row) = (slot % 6, slot / 6);
                    slot += 1;
                    let position = Point::new(outline.max.x + mil(500.0) + col * mil(400.0), outline.max.y - mil(200.0) - row * mil(400.0));
                    self.insert_pcb(PcbKind::Footprint(Footprint {
                        component: Some(c.id),
                        footprint: c.footprint.clone(),
                        position,
                        rotation: 0.0,
                        side: Side::Top,
                        locked: false,
                        pad_nets,
                    }))?;
                }
            }
        }
        Ok(())
    }

    /// Whether `p` is inside the board outline.
    pub fn on_board(&self, p: Point) -> bool {
        polygon_contains(&self.design.board.outline, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_45_goes_diagonal_then_straight() {
        let a = Point::new(0, 0);
        assert_eq!(route_45(a, Point::new(100, 30)), vec![a, Point::new(30, 30), Point::new(100, 30)]);
        assert_eq!(route_45(a, Point::new(0, 50)), vec![a, Point::new(0, 50)]);
        assert_eq!(route_45(a, Point::new(-40, -40)), vec![a, Point::new(-40, -40)]);
    }
}
