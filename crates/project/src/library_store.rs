//! The project component library on disk (format version 2):
//!
//! ```text
//! library/
//!   components/<name>.pcbcomp.json   one file per component, readable names
//!   symbols/<sha256>.json            shared schematic symbols, named by content hash
//!   footprints/<sha256>.json         shared footprints (with their 3D body), by content hash
//! ```
//!
//! In memory the library keeps symbols and footprints by name ([`Library`]); component files
//! refer to them by hash, so identical shapes are stored once and any change to one is visible
//! as a new hash. Each component file records its own hash; a file whose content no longer
//! matches it was edited by hand and loads as "modified".

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use hephaestus_core::hash::hex_of;
use hephaestus_core::{FootprintDef, Library, PartDef, PartKind, SymbolDef};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{FORMAT_VERSION, ProjectError, read_doc, to_json};

pub const LIBRARY_DIR: &str = "library";
const COMPONENT_EXT: &str = ".pcbcomp.json";

fn format_err(path: &Path, message: impl Into<String>) -> ProjectError {
    ProjectError::Format { path: path.to_path_buf(), message: message.into() }
}

/// A component's file name: its library name with characters unsafe in file names replaced.
fn component_file_name(name: &str) -> String {
    let s: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | '%' | ' ') { c } else { '_' }).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "component".into() } else { s }
}

fn shape_doc<T: Serialize>(format: &str, key: &str, hash: &str, shape: &T) -> Result<String, ProjectError> {
    let mut doc = Map::new();
    doc.insert("format".into(), format.into());
    doc.insert("version".into(), FORMAT_VERSION.into());
    doc.insert("hash".into(), hash.into());
    doc.insert(key.into(), serde_json::to_value(shape).map_err(|e| format_err(Path::new(key), e.to_string()))?);
    to_json(&Value::Object(doc))
}

/// Every library file under `root/library` with its content, for [`crate::ProjectFiles::save`].
/// Only shapes some component uses are written.
pub fn render(root: &Path, lib: &Library) -> Result<Vec<(PathBuf, String)>, ProjectError> {
    let dir = root.join(LIBRARY_DIR);
    let mut out = Vec::new();
    let mut symbol_hash: BTreeMap<&str, String> = BTreeMap::new();
    let mut footprint_hash: BTreeMap<&str, String> = BTreeMap::new();
    let mut used_files: BTreeSet<String> = BTreeSet::new();
    for part in lib.parts.values() {
        let Some(sym) = lib.symbols.get(&part.symbol) else {
            return Err(ProjectError::Integrity(format!("component {} references missing symbol {}", part.name, part.symbol)));
        };
        let sh = symbol_hash.entry(sym.name.as_str()).or_insert_with(|| Library::shape_hash(sym)).clone();
        let mut fps = Vec::new();
        for f in &part.footprints {
            let Some(fp) = lib.footprints.get(f) else {
                return Err(ProjectError::Integrity(format!("component {} references missing footprint {f}", part.name)));
            };
            fps.push(footprint_hash.entry(fp.name.as_str()).or_insert_with(|| Library::shape_hash(fp)).clone());
        }
        let mut v = serde_json::to_value(part).map_err(|e| format_err(&dir, e.to_string()))?;
        let Some(obj) = v.as_object_mut() else { continue };
        obj.insert("format".into(), "hephaestus-component".into());
        obj.insert("version".into(), FORMAT_VERSION.into());
        obj.insert("hash".into(), lib.component_hash(part).unwrap_or_default().into());
        obj.insert("symbol".into(), sh.into());
        obj.remove("footprints");
        let mut fps = fps.into_iter();
        obj.insert("footprint".into(), fps.next().unwrap_or_default().into());
        obj.insert("alt_footprints".into(), fps.collect::<Vec<_>>().into());
        // Case-insensitive file systems: two names differing only in case get a suffix.
        let base = component_file_name(&part.name);
        let mut file = base.clone();
        let mut n = 1;
        while !used_files.insert(file.to_ascii_lowercase()) {
            n += 1;
            file = format!("{base}-{n}");
        }
        out.push((dir.join("components").join(format!("{file}{COMPONENT_EXT}")), to_json(&v)?));
    }
    for (name, hash) in &symbol_hash {
        if let Some(sym) = lib.symbols.get(*name) {
            out.push((dir.join("symbols").join(format!("{}.json", hex_of(hash))), shape_doc("hephaestus-symbol", "symbol", hash, sym)?));
        }
    }
    for (name, hash) in &footprint_hash {
        if let Some(fp) = lib.footprints.get(*name) {
            out.push((
                dir.join("footprints").join(format!("{}.json", hex_of(hash))),
                shape_doc("hephaestus-footprint", "footprint", hash, fp)?,
            ));
        }
    }
    Ok(out)
}

#[derive(serde::Deserialize)]
struct SymbolDoc {
    symbol: SymbolDef,
}

#[derive(serde::Deserialize)]
struct FootprintDoc {
    footprint: FootprintDef,
}

/// Shapes loaded by hash and placed in the in-memory library under a unique name.
struct Shapes<'a, T> {
    dir: PathBuf,
    format: &'static str,
    by_hash: BTreeMap<String, String>,
    names: &'a mut BTreeMap<String, T>,
}

impl<T: DeserializeOwned + Clone + PartialEq> Shapes<'_, T> {
    /// The in-memory name of the shape stored under `hash`, loading it on first use.
    fn resolve(
        &mut self,
        hash: &str,
        read: impl Fn(&Path, &str) -> Result<T, ProjectError>,
        name_of: impl Fn(&mut T) -> &mut String,
    ) -> Result<String, ProjectError> {
        if let Some(n) = self.by_hash.get(hash) {
            return Ok(n.clone());
        }
        let hex = hex_of(hash);
        if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format_err(&self.dir, format!("invalid shape reference {hash:?}")));
        }
        let mut shape = read(&self.dir.join(format!("{hex}.json")), self.format)?;
        // Two different shapes may carry the same name; the later one is renamed.
        let base = name_of(&mut shape).clone();
        let mut name = base.clone();
        let mut n = 1;
        while self.names.get(&name).is_some_and(|held| *held != shape) {
            n += 1;
            name = format!("{base}_{n}");
            *name_of(&mut shape) = name.clone();
        }
        self.names.insert(name.clone(), shape);
        self.by_hash.insert(hash.to_string(), name.clone());
        Ok(name)
    }
}

/// Read the library folder of a version 2 project.
pub fn load(root: &Path, name: &str) -> Result<Library, ProjectError> {
    let dir = root.join(LIBRARY_DIR);
    let mut lib = Library::new(name);
    let comp_dir = dir.join("components");
    let mut files: Vec<PathBuf> = match fs::read_dir(&comp_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(COMPONENT_EXT)))
            .collect(),
        // A project without components has no folder yet.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(source) => return Err(ProjectError::Io { path: comp_dir, source }),
    };
    files.sort();
    let mut parts: Vec<(PathBuf, PartDef, String)> = Vec::new();
    {
        let mut symbols =
            Shapes { dir: dir.join("symbols"), format: "hephaestus-symbol", by_hash: BTreeMap::new(), names: &mut lib.symbols };
        let mut footprints =
            Shapes { dir: dir.join("footprints"), format: "hephaestus-footprint", by_hash: BTreeMap::new(), names: &mut lib.footprints };
        for path in files {
            let mut v: Value = read_doc(&path, "hephaestus-component")?;
            let Some(obj) = v.as_object_mut() else { return Err(format_err(&path, "expected an object")) };
            let stored_hash = obj.remove("hash").and_then(|h| h.as_str().map(String::from)).unwrap_or_default();
            obj.remove("format");
            obj.remove("version");
            let sym_hash = obj.get("symbol").and_then(Value::as_str).unwrap_or_default().to_string();
            let sym = symbols.resolve(&sym_hash, |p, f| read_doc::<SymbolDoc>(p, f).map(|d| d.symbol), |s| &mut s.name)?;
            let mut refs: Vec<String> = obj.remove("footprint").and_then(|f| f.as_str().map(String::from)).into_iter().collect();
            if let Some(Value::Array(alts)) = obj.remove("alt_footprints") {
                refs.extend(alts.into_iter().filter_map(|a| a.as_str().map(String::from)));
            }
            let mut names = Vec::new();
            for r in refs {
                names.push(footprints.resolve(&r, |p, f| read_doc::<FootprintDoc>(p, f).map(|d| d.footprint), |s| &mut s.name)?);
            }
            obj.insert("symbol".into(), sym.into());
            obj.insert("footprints".into(), names.into());
            let mut part: PartDef = serde_json::from_value(v).map_err(|e| format_err(&path, e.to_string()))?;
            // The built-in library was called "PCB Craft Basic" before the rename.
            if let Some(o) = part.origin.as_mut().filter(|o| o.source == "PCB Craft Basic") {
                o.source = "Hephaestus Basic".into();
            }
            parts.push((path, part, stored_hash));
        }
    }
    for (path, mut part, stored_hash) in parts {
        if lib.component_hash(&part).is_some_and(|h| h != stored_hash) {
            part.modified = true;
        }
        if lib.parts.contains_key(&part.name) {
            return Err(format_err(&path, format!("a second component is named {}", part.name)));
        }
        lib.parts.insert(part.name.clone(), part);
    }
    Ok(lib)
}

/// Bring a format 1 project library (one `library.pcblib.json`) up to the component model:
/// part numbers move from parameters into fields, parts copied from the built-in library get
/// their origin and kind, and placed components of generic families get their own
/// value-and-footprint component (`RES` + 10k + 0603 → `RES_10k_0603`).
pub fn upgrade_legacy(design: &mut hephaestus_core::Design, builtin: &Library) {
    let names: Vec<String> = design.library.parts.keys().cloned().collect();
    for name in names {
        let Some(part) = design.library.parts.get_mut(&name) else { continue };
        if part.manufacturer.is_empty()
            && let Some(m) = part.parameters.remove("Manufacturer")
        {
            part.manufacturer = m;
        }
        if part.mpn.is_empty()
            && let Some(m) = part.parameters.remove("Manufacturer Part Number")
        {
            part.mpn = m;
        }
        if let Some(upstream) = builtin.parts.get(&name) {
            part.kind = upstream.kind;
            if part.mpn.is_empty() {
                part.manufacturer = upstream.manufacturer.clone();
                part.mpn = upstream.mpn.clone();
            }
            // These parts were copied from an earlier built-in library; treat them as current.
            part.origin = builtin.origin_of(upstream);
        }
    }
    let generic: Vec<PartDef> =
        design.library.parts.values().filter(|p| p.kind == PartKind::Generic && p.family.is_none()).cloned().collect();
    for family in generic {
        let users: Vec<_> = design.components.values().filter(|c| c.part == family.name).map(|c| c.id).collect();
        for id in users {
            let Some(c) = design.components.get_mut(&id) else { continue };
            if !design.library.footprints.contains_key(&c.footprint) {
                continue;
            }
            let mut derived = family.derived(&c.value, &c.footprint);
            derived.origin = family.origin.clone();
            c.part = derived.name.clone();
            design.library.parts.entry(derived.name.clone()).or_insert(derived);
        }
        if !design.components.values().any(|c| c.part == family.name) {
            design.library.parts.remove(&family.name);
        }
    }
}

/// File extension of a component pack: exported components with their symbols and footprints
/// in one file, like an Altium integrated library.
pub const PACK_EXTENSION: &str = "pcbcomp.json";

#[derive(Serialize, serde::Deserialize)]
struct PackDoc {
    format: String,
    version: u32,
    library: Library,
}

/// Write `parts` of `lib`, with everything they use, as a component pack.
pub fn save_pack<'a>(lib: &Library, parts: impl IntoIterator<Item = &'a str>, path: &Path) -> Result<usize, ProjectError> {
    let mut pack = lib.extract(parts);
    pack.name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim_end_matches(&format!(".{PACK_EXTENSION}")).to_string())
        .unwrap_or_default();
    let count = pack.parts.len();
    crate::write_atomic(path, &to_json(&PackDoc { format: "hephaestus-component-pack".into(), version: FORMAT_VERSION, library: pack })?)?;
    Ok(count)
}

/// Read a component pack; every component in it must resolve.
pub fn load_pack(path: &Path) -> Result<Library, ProjectError> {
    let doc: PackDoc = read_doc(path, "hephaestus-component-pack")?;
    if let Some(problem) = doc.library.parts.values().flat_map(|p| doc.library.check_part(p)).next() {
        return Err(format_err(path, problem));
    }
    Ok(doc.library)
}
