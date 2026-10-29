use std::path::{Path, PathBuf};

use hephaestus_commands::{Editor, Source};
use hephaestus_core::{Design, Library};
use hephaestus_project::ProjectFiles;
use hephaestus_rules::Report;

use crate::ApiError;

/// How agent changes are admitted.
#[derive(Clone, Debug)]
pub struct AgentPolicy {
    /// Destructive agent commands (deletions, board outline, layer stack, rules) become proposals
    /// that a person must accept in the application.
    pub approve_destructive: bool,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self { approve_destructive: true }
    }
}

/// One open project and everything agents and the UI share about it.
pub struct Session {
    pub editor: Editor,
    pub files: Option<ProjectFiles>,
    pub last_erc: Option<(u64, Report)>,
    pub last_drc: Option<(u64, Report)>,
    pub policy: AgentPolicy,
    /// Status lines for the Messages panel (agent activity, saves, errors).
    pub messages: Vec<String>,
}

impl Session {
    /// A session with an empty project and the built-in library available.
    pub fn new(name: &str) -> Self {
        Self::with_design(Design::new(name))
    }

    pub fn with_design(design: Design) -> Self {
        Self {
            editor: Editor::new(design, vec![hephaestus_libraries::builtin()]),
            files: None,
            last_erc: None,
            last_drc: None,
            policy: AgentPolicy::default(),
            messages: Vec::new(),
        }
    }

    pub fn design(&self) -> &Design {
        self.editor.design()
    }

    pub fn libraries(&self) -> &[Library] {
        self.editor.sources()
    }

    /// Open a project manifest (`*.pcbprj`) or a single-file bundle (`*.hephaestus.json`).
    pub fn open(&mut self, path: &Path) -> Result<(), ApiError> {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // `.pcbcraft.json` is the bundle extension from before the rename.
        if name.ends_with(".hephaestus.json") || name.ends_with(".pcbcraft.json") {
            let design = hephaestus_project::load_bundle(path)?;
            self.editor.replace_design(design);
            self.files = None;
        } else {
            let (files, design) = ProjectFiles::open(path)?;
            self.editor.replace_design(design);
            self.files = Some(files);
        }
        self.last_erc = None;
        self.last_drc = None;
        self.note(format!("Opened {}", path.display()));
        Ok(())
    }

    /// Save to the project folder (or create one at `folder`).
    pub fn save(&mut self, folder: Option<&Path>) -> Result<hephaestus_project::SaveReport, ApiError> {
        if let Some(folder) = folder {
            self.files = Some(ProjectFiles::new(folder, &self.design().name));
        }
        let design = self.editor.design().clone();
        let Some(files) = self.files.as_mut() else {
            return Err(ApiError::new("no_location", "the project has never been saved; pass a folder"));
        };
        let report = files.save(&design)?;
        files.clear_autosave();
        let message =
            format!("Saved {} ({} file(s) written, {} unchanged)", files.manifest_path().display(), report.written.len(), report.unchanged);
        self.editor.mark_saved();
        self.note(message);
        Ok(report)
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.files.as_ref().map(|f| f.root.as_path())
    }

    /// Resolve an output path: relative paths are inside the project folder.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, ApiError> {
        let p = PathBuf::from(path);
        if p.is_absolute() {
            return Ok(p);
        }
        match self.project_root() {
            Some(root) => Ok(root.join(p)),
            None => Err(ApiError::invalid("relative paths need a saved project; pass an absolute directory")),
        }
    }

    pub fn note(&mut self, message: String) {
        log::info!("{message}");
        self.messages.push(message);
        if self.messages.len() > 500 {
            self.messages.remove(0);
        }
    }

    /// The source tag for a caller name (`"user"` for the UI).
    pub fn source(agent: Option<&str>) -> Source {
        match agent {
            None => Source::User,
            Some(name) if name.eq_ignore_ascii_case("user") => Source::User,
            Some(name) if name.eq_ignore_ascii_case("script") => Source::Script,
            Some(name) => Source::Agent(name.chars().take(64).collect()),
        }
    }
}
