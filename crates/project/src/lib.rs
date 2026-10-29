//! Project persistence.
//!
//! A project is a folder of pretty-printed JSON documents, one per engineering document, so it
//! diffs well in version control and agents can read it directly:
//!
//! ```text
//! MyBoard/
//!   MyBoard.pcbprj               manifest: format version, name, id counter, document list
//!   components.json              logical components (shared by schematic and PCB)
//!   library/                     the project component library (see [`library_store`])
//!   board.pcbdoc.json            board settings and PCB objects
//!   sheets/Sheet1.schdoc.json    one file per schematic sheet
//!   .autosave/                   crash-recovery snapshot (not for version control)
//! ```
//!
//! Every document carries `"format"` and `"version"`; older versions are migrated on load and
//! newer ones are refused rather than misread. Saves are incremental (unchanged documents are
//! not rewritten) and atomic per file (write to a temporary file, then rename).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub mod library_store;

use hephaestus_core::{BoardSettings, Component, Design, Library, ObjectId, PcbObject, SchObject, Sheet};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The format version this build writes.
pub const FORMAT_VERSION: u32 = 2;
pub const MANIFEST_EXTENSION: &str = "pcbprj";
/// Single-file bundle of a whole design (exchange with agents and tests).
pub const BUNDLE_EXTENSION: &str = "hephaestus.json";

/// Largest document read (256 MiB) — larger files are refused instead of exhausting memory.
const MAX_FILE: u64 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {message}")]
    Format { path: PathBuf, message: String },
    #[error("{path} was written by a newer Hephaestus (format version {found}, this build reads up to {FORMAT_VERSION})")]
    TooNew { path: PathBuf, found: u32 },
    #[error("the project is inconsistent: {0}")]
    Integrity(String),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io { path: path.to_path_buf(), source }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SheetRef {
    id: ObjectId,
    file: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    name: String,
    next_id: u64,
    components: String,
    library: String,
    board: String,
    sheets: Vec<SheetRef>,
}

#[derive(Serialize, Deserialize)]
struct SheetDoc {
    format: String,
    version: u32,
    sheet: Sheet,
    objects: Vec<SchObject>,
}

#[derive(Serialize, Deserialize)]
struct BoardDoc {
    format: String,
    version: u32,
    board: BoardSettings,
    objects: Vec<PcbObject>,
}

#[derive(Serialize, Deserialize)]
struct ComponentsDoc {
    format: String,
    version: u32,
    components: Vec<Component>,
}

#[derive(Serialize, Deserialize)]
struct LibraryDoc {
    format: String,
    version: u32,
    library: Library,
}

#[derive(Serialize, Deserialize)]
struct Bundle {
    format: String,
    version: u32,
    design: Design,
}

/// Bring a document up to [`FORMAT_VERSION`]. Each future format change adds one step here.
fn migrate(mut doc: Value, path: &Path, expected_format: &str) -> Result<Value, ProjectError> {
    let format = doc.get("format").and_then(Value::as_str).unwrap_or_default().to_string();
    // Files written before the application was renamed from PCB Craft carry `pcbcraft-*` tags.
    let legacy = expected_format.strip_prefix("hephaestus-").map(|kind| format!("pcbcraft-{kind}"));
    if format != expected_format && Some(&format) != legacy.as_ref() {
        return Err(ProjectError::Format {
            path: path.to_path_buf(),
            message: format!("expected format {expected_format}, found {format:?}"),
        });
    }
    let version = doc.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
    if version > FORMAT_VERSION {
        return Err(ProjectError::TooNew { path: path.to_path_buf(), found: version });
    }
    if version == 0 {
        return Err(ProjectError::Format { path: path.to_path_buf(), message: "missing format version".into() });
    }
    // Version 2 moved the project library from `library.pcblib.json` into the `library/`
    // folder; documents themselves only gained fields with defaults, and the library itself is
    // converted in `ProjectFiles::open`. A step that rewrites fields looks like:
    // if version == 2 { rename fields in `doc`; version = 3; }
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("version".into(), Value::from(FORMAT_VERSION));
    }
    Ok(doc)
}

pub(crate) fn read_doc<T: DeserializeOwned>(path: &Path, format: &str) -> Result<T, ProjectError> {
    let meta = fs::metadata(path).map_err(io(path))?;
    if meta.len() > MAX_FILE {
        return Err(ProjectError::Format { path: path.to_path_buf(), message: "file is larger than 256 MiB".into() });
    }
    let text = fs::read_to_string(path).map_err(io(path))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| ProjectError::Format { path: path.to_path_buf(), message: e.to_string() })?;
    let value = migrate(value, path, format)?;
    serde_json::from_value(value).map_err(|e| ProjectError::Format { path: path.to_path_buf(), message: e.to_string() })
}

pub(crate) fn to_json<T: Serialize>(value: &T) -> Result<String, ProjectError> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|e| ProjectError::Format { path: PathBuf::new(), message: e.to_string() })?;
    text.push('\n');
    Ok(text)
}

/// Write via a temporary sibling and rename, so a crash never leaves a half-written document.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), ProjectError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    fs::write(&tmp, contents).map_err(io(&tmp))?;
    fs::rename(&tmp, path).map_err(io(path))
}

fn safe_file_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect();
    let trimmed = cleaned.trim().to_string();
    if trimmed.is_empty() { "Sheet".into() } else { trimmed }
}

/// Where a project lives and what was last written, for incremental saves.
#[derive(Clone, Debug, Default)]
pub struct ProjectFiles {
    /// The folder holding the manifest.
    pub root: PathBuf,
    /// The manifest file name.
    pub manifest: String,
    written: HashMap<PathBuf, String>,
    /// A format 1 `library.pcblib.json` to set aside (as `.bak`) after the first save.
    legacy_library: Option<PathBuf>,
}

/// What a save did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveReport {
    pub written: Vec<PathBuf>,
    pub unchanged: usize,
    pub removed: Vec<PathBuf>,
}

impl ProjectFiles {
    /// A new project at `root` with manifest `<name>.pcbprj`.
    pub fn new(root: impl Into<PathBuf>, name: &str) -> Self {
        Self {
            root: root.into(),
            manifest: format!("{}.{MANIFEST_EXTENSION}", safe_file_name(name)),
            written: HashMap::new(),
            legacy_library: None,
        }
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(&self.manifest)
    }

    pub fn autosave_dir(&self) -> PathBuf {
        self.root.join(".autosave")
    }

    /// Serialize every document; write only those whose content changed since the last save or
    /// load, and remove sheet files of deleted sheets.
    pub fn save(&mut self, design: &Design) -> Result<SaveReport, ProjectError> {
        if let Some(first) = design.integrity_problems().first() {
            return Err(ProjectError::Integrity(first.clone()));
        }
        // `render` lists the manifest last, so a crash mid-save leaves the previous manifest
        // pointing at complete documents.
        let docs = self.render(design)?;
        let mut report = SaveReport::default();
        for (path, text) in &docs {
            if self.written.get(path) == Some(text) && path.exists() {
                report.unchanged += 1;
                continue;
            }
            write_atomic(path, text)?;
            report.written.push(path.clone());
        }
        let current: HashMap<PathBuf, String> = docs.into_iter().collect();
        let sheets_dir = self.root.join("sheets");
        let library_dir = self.root.join(library_store::LIBRARY_DIR);
        let mut stale: Vec<PathBuf> = self
            .written
            .keys()
            .filter(|p| !current.contains_key(*p) && (p.starts_with(&sheets_dir) || p.starts_with(&library_dir)))
            .cloned()
            .collect();
        // Library files nobody references any more (a removed component, a shape no component
        // uses) are removed even if this session never wrote them.
        for sub in ["components", "symbols", "footprints"] {
            if let Ok(entries) = fs::read_dir(library_dir.join(sub)) {
                stale.extend(
                    entries
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.extension().is_some_and(|e| e == "json") && !current.contains_key(p)),
                );
            }
        }
        stale.sort();
        stale.dedup();
        for path in stale {
            if path.exists() {
                fs::remove_file(&path).map_err(io(&path))?;
                report.removed.push(path);
            }
        }
        if let Some(legacy) = self.legacy_library.take()
            && legacy.exists()
        {
            let bak = legacy.with_extension("json.bak");
            fs::rename(&legacy, &bak).map_err(io(&legacy))?;
            report.removed.push(legacy);
        }
        self.written = current;
        Ok(report)
    }

    /// Open the project whose manifest is `manifest`.
    pub fn open(manifest: &Path) -> Result<(Self, Design), ProjectError> {
        let root = manifest.parent().map(Path::to_path_buf).unwrap_or_default();
        let m: Manifest = read_doc(manifest, "hephaestus-project")?;
        let mut files = ProjectFiles {
            root: root.clone(),
            manifest: manifest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            written: HashMap::new(),
            legacy_library: None,
        };
        let mut design = Design::new(m.name.clone());
        design.sheets.clear();
        design.next_id = m.next_id;
        let inside = |rel: &str| -> Result<PathBuf, ProjectError> {
            let p = Path::new(rel);
            if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                return Err(ProjectError::Format {
                    path: manifest.to_path_buf(),
                    message: format!("document path {rel} leaves the project folder"),
                });
            }
            Ok(root.join(p))
        };
        let legacy = m.library.ends_with(".json");
        if legacy {
            let path = inside(&m.library)?;
            let lib: LibraryDoc = read_doc(&path, "hephaestus-library")?;
            design.library = lib.library;
            files.legacy_library = Some(path);
        } else {
            inside(&m.library)?;
            design.library = library_store::load(&root, "Project Library")?;
        }
        let comps: ComponentsDoc = read_doc(&inside(&m.components)?, "hephaestus-components")?;
        design.components = comps.components.into_iter().map(|c| (c.id, c)).collect();
        if legacy {
            library_store::upgrade_legacy(&mut design, &hephaestus_libraries::builtin());
        }
        let board: BoardDoc = read_doc(&inside(&m.board)?, "hephaestus-board")?;
        design.board = board.board;
        design.pcb = board.objects.into_iter().map(|o| (o.id, o)).collect();
        for sheet_ref in &m.sheets {
            let doc: SheetDoc = read_doc(&inside(&sheet_ref.file)?, "hephaestus-sheet")?;
            if doc.sheet.id != sheet_ref.id {
                return Err(ProjectError::Format {
                    path: inside(&sheet_ref.file)?,
                    message: "sheet id does not match the manifest".into(),
                });
            }
            for o in doc.objects {
                if o.sheet != doc.sheet.id {
                    return Err(ProjectError::Format {
                        path: inside(&sheet_ref.file)?,
                        message: format!("object {} belongs to another sheet", o.id),
                    });
                }
                design.schematic.insert(o.id, o);
            }
            design.sheets.insert(doc.sheet.id, doc.sheet);
        }
        if let Some(first) = design.integrity_problems().first() {
            return Err(ProjectError::Integrity(first.clone()));
        }
        // Remember documents whose disk content already matches, so the first save only
        // rewrites what changes.
        files.written = files
            .render(&design)?
            .into_iter()
            .filter(|(path, text)| fs::read_to_string(path).ok().as_deref() == Some(text.as_str()))
            .collect();
        Ok((files, design))
    }

    /// Every document's path and serialized content.
    fn render(&self, design: &Design) -> Result<Vec<(PathBuf, String)>, ProjectError> {
        let mut out = Vec::new();
        let mut used_names: BTreeMap<String, usize> = BTreeMap::new();
        let mut sheet_refs = Vec::new();
        for sheet in design.sheets_in_order() {
            let base = safe_file_name(&sheet.name);
            let n = used_names.entry(base.to_ascii_lowercase()).or_default();
            *n += 1;
            let file = if *n == 1 { format!("sheets/{base}.schdoc.json") } else { format!("sheets/{base}-{n}.schdoc.json") };
            let mut objects: Vec<SchObject> = design.sheet_objects(sheet.id).cloned().collect();
            objects.sort_by_key(|o| o.id);
            out.push((
                self.root.join(&file),
                to_json(&SheetDoc { format: "hephaestus-sheet".into(), version: FORMAT_VERSION, sheet: sheet.clone(), objects })?,
            ));
            sheet_refs.push(SheetRef { id: sheet.id, file });
        }
        out.push((
            self.root.join("board.pcbdoc.json"),
            to_json(&BoardDoc {
                format: "hephaestus-board".into(),
                version: FORMAT_VERSION,
                board: design.board.clone(),
                objects: design.pcb.values().cloned().collect(),
            })?,
        ));
        out.push((
            self.root.join("components.json"),
            to_json(&ComponentsDoc {
                format: "hephaestus-components".into(),
                version: FORMAT_VERSION,
                components: design.components.values().cloned().collect(),
            })?,
        ));
        out.extend(library_store::render(&self.root, &design.library)?);
        let manifest = Manifest {
            format: "hephaestus-project".into(),
            version: FORMAT_VERSION,
            name: design.name.clone(),
            next_id: design.next_id,
            components: "components.json".into(),
            library: library_store::LIBRARY_DIR.into(),
            board: "board.pcbdoc.json".into(),
            sheets: sheet_refs,
        };
        out.push((self.manifest_path(), to_json(&manifest)?));
        Ok(out)
    }

    /// Write a crash-recovery snapshot of the whole design.
    pub fn autosave(&self, design: &Design) -> Result<PathBuf, ProjectError> {
        let path = self.autosave_dir().join("autosave.hephaestus.json");
        save_bundle(design, &path)?;
        Ok(path)
    }

    /// A snapshot newer than the saved manifest, if one exists (the previous session ended
    /// without saving).
    pub fn recovery_available(&self) -> Option<PathBuf> {
        let snapshot = self.autosave_dir().join("autosave.hephaestus.json");
        let modified = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
        let snap_time = modified(&snapshot)?;
        match modified(&self.manifest_path()) {
            Some(saved) if saved >= snap_time => None,
            _ => Some(snapshot),
        }
    }

    pub fn clear_autosave(&self) {
        // Missing snapshots are fine: there is nothing to recover.
        let _ = fs::remove_file(self.autosave_dir().join("autosave.hephaestus.json"));
    }
}

/// Save a whole design as one JSON file.
pub fn save_bundle(design: &Design, path: &Path) -> Result<(), ProjectError> {
    let bundle = Bundle { format: "hephaestus-bundle".into(), version: FORMAT_VERSION, design: design.clone() };
    write_atomic(path, &to_json(&bundle)?)
}

pub fn load_bundle(path: &Path) -> Result<Design, ProjectError> {
    let bundle: Bundle = read_doc(path, "hephaestus-bundle")?;
    if let Some(first) = bundle.design.integrity_problems().first() {
        return Err(ProjectError::Integrity(first.clone()));
    }
    Ok(bundle.design)
}

/// Modification time, for "file changed on disk" checks.
pub fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hephaestus_commands::{Command, Editor, Source};
    use hephaestus_core::geometry::{Point, mil};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hephaestus-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> Design {
        let mut e = Editor::new(Design::new("Sample"), vec![hephaestus_libraries::builtin()]);
        e.execute(
            &Command::PlaceComponent {
                part: "NE555".into(),
                sheet: None,
                position: Point::new(mil(3000.0), mil(3000.0)),
                rotation: 0.0,
                mirror: false,
                designator: None,
                value: None,
                footprint: None,
                parameters: Default::default(),
            },
            Source::User,
        )
        .unwrap();
        e.execute(&Command::AddSheet { name: "Power".into(), size: None }, Source::User).unwrap();
        e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
        e.design().clone()
    }

    #[test]
    fn save_and_open_round_trip() {
        let dir = temp("roundtrip");
        let design = sample();
        let mut files = ProjectFiles::new(&dir, &design.name);
        let report = files.save(&design).unwrap();
        // manifest, components, board, two sheets, and the NE555's component, symbol and footprint
        assert_eq!(report.written.len(), 8);
        assert_eq!(fs::read_dir(dir.join("library/components")).unwrap().count(), 1);
        assert!(dir.join("Sample.pcbprj").exists());
        assert!(dir.join("sheets/Power.schdoc.json").exists());
        let (_, back) = ProjectFiles::open(&files.manifest_path()).unwrap();
        assert_eq!(back, design);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saves_are_incremental() {
        let dir = temp("incremental");
        let mut design = sample();
        let mut files = ProjectFiles::new(&dir, &design.name);
        files.save(&design).unwrap();
        let again = files.save(&design).unwrap();
        assert!(again.written.is_empty());
        assert_eq!(again.unchanged, 8);
        let (mut reopened, _) = ProjectFiles::open(&files.manifest_path()).unwrap();
        assert!(reopened.save(&design).unwrap().written.is_empty());
        design.board.origin = Point::new(5, 5);
        let changed = files.save(&design).unwrap();
        assert_eq!(changed.written, vec![dir.join("board.pcbdoc.json")]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deleted_sheets_lose_their_files() {
        let dir = temp("stale");
        let mut design = sample();
        let mut files = ProjectFiles::new(&dir, &design.name);
        files.save(&design).unwrap();
        let power = design.sheets.values().find(|s| s.name == "Power").unwrap().id;
        design.sheets.remove(&power);
        let report = files.save(&design).unwrap();
        assert_eq!(report.removed, vec![dir.join("sheets/Power.schdoc.json")]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_versions_and_escaping_paths_are_refused() {
        let dir = temp("versions");
        let design = sample();
        let mut files = ProjectFiles::new(&dir, &design.name);
        files.save(&design).unwrap();
        let board = dir.join("board.pcbdoc.json");
        let text = fs::read_to_string(&board).unwrap().replacen("\"version\": 2", "\"version\": 99", 1);
        fs::write(&board, text).unwrap();
        assert!(matches!(ProjectFiles::open(&files.manifest_path()), Err(ProjectError::TooNew { found: 99, .. })));
        let manifest = fs::read_to_string(files.manifest_path()).unwrap().replace("\"board.pcbdoc.json\"", "\"../board.pcbdoc.json\"");
        fs::write(files.manifest_path(), manifest).unwrap();
        assert!(matches!(ProjectFiles::open(&files.manifest_path()), Err(ProjectError::Format { .. })));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn library_files_track_components_and_hand_edits() {
        let dir = temp("library");
        let mut design = sample();
        let mut files = ProjectFiles::new(&dir, &design.name);
        files.save(&design).unwrap();
        let comp = dir.join("library/components/NE555.pcbcomp.json");
        let text = fs::read_to_string(&comp).unwrap();
        assert!(text.contains("\"mpn\": \"NE555DR\"") && text.contains("\"symbol\": \"sha256:"), "{text}");
        // A hand edit loads as a local modification.
        fs::write(&comp, text.replace("Precision timer", "Precision timer (hand edited)")).unwrap();
        let (_, back) = ProjectFiles::open(&files.manifest_path()).unwrap();
        assert!(back.library.parts["NE555"].modified);
        // Removing the component removes its files and the shapes nobody uses any more.
        let u1 = design.component_by_designator("U1").unwrap().id;
        design.components.remove(&u1);
        design.schematic.retain(|_, o| !matches!(&o.kind, hephaestus_core::SchKind::Symbol(s) if s.component == u1));
        design.pcb.retain(|_, o| !matches!(&o.kind, hephaestus_core::PcbKind::Footprint(f) if f.component == Some(u1)));
        design.library = Library::new("Project Library");
        let report = files.save(&design).unwrap();
        assert_eq!(report.removed.len(), 3, "{:?}", report.removed);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn format_1_libraries_are_converted() {
        let dir = temp("legacy");
        let mut e = Editor::new(Design::new("Old"), vec![hephaestus_libraries::builtin()]);
        e.execute(&Command::ImportPart { part: "RES".into() }, Source::User).unwrap();
        e.execute(&Command::ImportPart { part: "NE555".into() }, Source::User).unwrap();
        let mut design = e.design().clone();
        // A format 1 project placed the family itself, with part numbers in parameters.
        let id = design.alloc_id();
        design.components.insert(
            id,
            Component {
                id,
                designator: "R1".into(),
                value: "4.7k".into(),
                part: "RES".into(),
                footprint: "0805".into(),
                parameters: Default::default(),
            },
        );
        design.library.footprints.insert("0805".into(), hephaestus_libraries::builtin().footprints["0805"].clone());
        for p in design.library.parts.values_mut() {
            p.kind = Default::default();
            p.origin = None;
            if p.name == "NE555" {
                p.parameters.insert("Manufacturer Part Number".into(), std::mem::take(&mut p.mpn));
            }
        }
        let mut files = ProjectFiles::new(&dir, &design.name);
        files.save(&design).unwrap();
        let lib_doc = LibraryDoc { format: "hephaestus-library".into(), version: 1, library: design.library.clone() };
        write_atomic(&dir.join("library.pcblib.json"), &to_json(&lib_doc).unwrap()).unwrap();
        fs::remove_dir_all(dir.join("library")).unwrap();
        let manifest =
            fs::read_to_string(files.manifest_path()).unwrap().replace("\"library\": \"library\"", "\"library\": \"library.pcblib.json\"");
        fs::write(files.manifest_path(), manifest.replace("\"version\": 2", "\"version\": 1")).unwrap();

        let (mut reopened, back) = ProjectFiles::open(&files.manifest_path()).unwrap();
        assert_eq!(back.components[&id].part, "RES_4.7k_0805");
        assert!(!back.library.parts.contains_key("RES"), "the unused family is dropped");
        assert_eq!(back.library.parts["NE555"].mpn, "NE555DR");
        let status = hephaestus_core::library_check::check_library(&back, &[hephaestus_libraries::builtin()]);
        assert!(status.iter().all(|s| !s.update_available && !s.modified), "{status:?}");
        reopened.save(&back).unwrap();
        assert!(dir.join("library.pcblib.json.bak").exists() && !dir.join("library.pcblib.json").exists());
        assert!(dir.join("library/components/RES_4.7k_0805.pcbcomp.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn projects_saved_before_the_rename_still_open() {
        let dir = temp("rename");
        let design = sample();
        let mut files = ProjectFiles::new(&dir, &design.name);
        files.save(&design).unwrap();
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            for entry in fs::read_dir(&d).unwrap().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let text = fs::read_to_string(&p).unwrap();
                    fs::write(&p, text.replace("\"hephaestus-", "\"pcbcraft-").replace("Hephaestus Basic", "PCB Craft Basic")).unwrap();
                }
            }
        }
        let (_, back) = ProjectFiles::open(&files.manifest_path()).unwrap();
        assert_eq!(back, design);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn component_packs_round_trip() {
        let dir = temp("pack");
        let design = sample();
        let path = dir.join("timer.pcbcomp.json");
        assert_eq!(library_store::save_pack(&design.library, ["NE555"], &path).unwrap(), 1);
        let pack = library_store::load_pack(&path).unwrap();
        assert_eq!(pack.parts["NE555"], design.library.parts["NE555"]);
        assert_eq!(pack.footprints.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn autosave_offers_recovery_until_saved() {
        let dir = temp("autosave");
        let design = sample();
        let mut files = ProjectFiles::new(&dir, &design.name);
        files.save(&design).unwrap();
        assert!(files.recovery_available().is_none());
        std::thread::sleep(std::time::Duration::from_millis(20));
        let snap = files.autosave(&design).unwrap();
        assert_eq!(files.recovery_available(), Some(snap.clone()));
        assert_eq!(load_bundle(&snap).unwrap(), design);
        files.clear_autosave();
        assert!(files.recovery_available().is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
