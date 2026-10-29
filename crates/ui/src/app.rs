//! The application: owns the session, documents, panels and the agent control port.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use hephaestus_agent_api::control::Request;
use hephaestus_agent_api::{Session, tools};
use hephaestus_commands::{Command, Outcome, Source};
use hephaestus_core::geometry::{Point, Rect, Unit, mil};
use hephaestus_core::{Design, Layer, ObjectId, ObjectRef, PcbKind, SchKind};
use hephaestus_rules::Report;
use serde_json::{Value, json};

use crate::actions::{Action, PcbToolKind, SchToolKind};
use crate::cache::Cache;
use crate::documents::{CornerMode, DocKey, DocView, Tool};
use crate::theme::Theme;
use crate::workspace::{PanelId, Prefs};

/// How the application starts (from the command line).
#[derive(Clone, Debug, Default)]
pub struct StartOptions {
    pub open: Option<PathBuf>,
    pub demo: bool,
    /// Save a screenshot of the window here once the UI has settled.
    pub screenshot: Option<PathBuf>,
    /// Exit after the screenshot.
    pub exit_after_screenshot: bool,
    /// `"pcb"` or a sheet name to show first.
    pub document: Option<String>,
    /// Control port override; 0 disables the agent port.
    pub control_port: Option<u16>,
    /// Select these designators before a screenshot (shows selection and Properties).
    pub select: Vec<String>,
    /// Activate this panel tab before a screenshot.
    pub panel: Option<String>,
    /// Show this library component in the Components panel: `NAME` or
    /// `NAME:schematic|footprint|3d`.
    pub component: Option<String>,
}

pub struct Screenshot {
    pub path: PathBuf,
    pub frames: u32,
    pub reply: Option<Request>,
    pub exit: bool,
    pub requested: bool,
}

#[derive(Default)]
pub struct Reports {
    pub erc: Option<Report>,
    pub drc: Option<Report>,
    pub selected: Option<(bool, usize)>,
}

/// Which view of a component the Components panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComponentTab {
    #[default]
    Schematic,
    Footprint,
    Model3d,
}

#[derive(Default)]
pub struct ComponentsState {
    pub query: String,
    pub category: Option<String>,
    pub selected: Option<String>,
    pub library: usize,
    pub tab: ComponentTab,
    /// Library status per design revision (hashing every component each frame is wasteful).
    pub status: Option<(u64, Vec<hephaestus_core::library_check::ComponentStatus>)>,
}

#[derive(Default)]
pub struct Dialogs {
    pub rules: Option<hephaestus_core::DesignRules>,
    pub stack: Option<u8>,
    pub preferences: bool,
    pub about: bool,
    pub shortcuts: bool,
    pub confirm: Option<(String, Action)>,
    pub recovery: Option<PathBuf>,
}

/// What was copied: schematic objects and their components, or PCB primitives.
#[derive(Clone)]
pub struct Clipboard {
    pub design: Design,
    pub sch: Vec<ObjectId>,
    pub pcb: Vec<ObjectId>,
    pub anchor: Point,
}

pub struct HephaestusApp {
    pub session: Session,
    pub theme: Theme,
    pub prefs: Prefs,
    pub cache: Cache,
    pub docs: Vec<DocView>,
    pub active: usize,
    pub status: String,
    pub reports: Reports,
    pub components: ComponentsState,
    pub dialogs: Dialogs,
    pub clipboard: Option<Clipboard>,
    pub control: Option<(SocketAddr, Receiver<Request>)>,
    pub control_error: Option<String>,
    pub screenshot: Option<Screenshot>,
    pub key_prefix: Option<char>,
    pub last_autosave: f64,
    pub cross_probe_from: Option<usize>,
    pub focus_panel: Option<PanelId>,
    pub agent_activity: Vec<(f64, String)>,
    pub start: StartOptions,
    pub frame: u64,
    pub properties_edit: crate::panels::properties::EditState,
    pub exit_requested: bool,
    last_title: String,
}

impl HephaestusApp {
    pub fn new(cc: &eframe::CreationContext<'_>, start: StartOptions) -> Self {
        let mut prefs: Prefs = cc.storage.and_then(|s| eframe::get_value(s, "hephaestus-prefs")).unwrap_or_default();
        prefs.sanitize();
        let theme = Theme::dark();
        theme.apply(&cc.egui_ctx);
        Self::build(&cc.egui_ctx, prefs, theme, start)
    }

    pub fn build(ctx: &egui::Context, prefs: Prefs, theme: Theme, start: StartOptions) -> Self {
        let mut status = String::from("Ready");
        let mut session = if start.demo {
            hephaestus_agent_api::demo::flasher().unwrap_or_else(|e| {
                status = format!("Could not build the demo: {e}");
                Session::new("PCB_Project")
            })
        } else {
            Session::new("PCB_Project")
        };
        let mut recovery = None;
        if let Some(path) = &start.open {
            match session.open(path) {
                Ok(()) => {
                    status = format!("Opened {}", path.display());
                    recovery = session.files.as_ref().and_then(|f| f.recovery_available());
                }
                Err(e) => status = format!("Could not open {}: {e}", path.display()),
            }
        }
        let port = start.control_port.unwrap_or(prefs.control_port);
        let (control, control_error) = if port == 0 {
            (None, Some("disabled".to_string()))
        } else {
            let wake_ctx = ctx.clone();
            match hephaestus_agent_api::control::serve(port, Arc::new(move || wake_ctx.request_repaint())) {
                Ok(c) => (Some(c), None),
                Err(e) => (None, Some(format!("port {port}: {e}"))),
            }
        };
        let screenshot = start.screenshot.clone().map(|path| Screenshot {
            path,
            frames: 0,
            reply: None,
            exit: start.exit_after_screenshot,
            requested: false,
        });
        let mut app = Self {
            session,
            theme,
            prefs,
            cache: Cache::empty(),
            docs: Vec::new(),
            active: 0,
            status,
            reports: Reports::default(),
            components: ComponentsState::default(),
            dialogs: Dialogs { recovery, ..Default::default() },
            clipboard: None,
            control,
            control_error,
            screenshot,
            key_prefix: None,
            last_autosave: 0.0,
            cross_probe_from: None,
            focus_panel: None,
            agent_activity: Vec::new(),
            start,
            frame: 0,
            properties_edit: Default::default(),
            exit_requested: false,
            last_title: String::new(),
        };
        app.reset_documents();
        if let Some(doc) = app.start.document.clone() {
            app.open_named_document(&doc);
        }
        if let Some(panel) = app.start.panel.clone()
            && let Some(p) =
                PanelId::ALL.into_iter().find(|p| p.title().eq_ignore_ascii_case(&panel) || format!("{p:?}").eq_ignore_ascii_case(&panel))
        {
            app.focus_panel = Some(p);
        }
        if let Some(spec) = app.start.component.clone() {
            let (name, view) = spec.split_once(':').unwrap_or((spec.as_str(), "schematic"));
            app.components.selected = Some(name.to_string());
            app.components.tab = match view.to_ascii_lowercase().as_str() {
                "footprint" | "pcb" => ComponentTab::Footprint,
                "3d" | "model" => ComponentTab::Model3d,
                _ => ComponentTab::Schematic,
            };
            app.focus_panel = Some(PanelId::Components);
        }
        let select = app.start.select.clone();
        if !select.is_empty() {
            let ids: Vec<ObjectId> = select.iter().filter_map(|d| app.design().component_by_designator(d).map(|c| c.id)).collect();
            app.select_components(&ids);
        }
        app
    }

    pub fn design(&self) -> &Design {
        self.session.design()
    }

    /// Open one tab per sheet plus the PCB.
    pub fn reset_documents(&mut self) {
        let mut docs: Vec<DocView> = self.design().sheets_in_order().iter().map(|s| DocView::new(DocKey::Sheet(s.id))).collect();
        docs.push(DocView::new(DocKey::Pcb));
        self.docs = docs;
        self.active = 0;
        self.reports = Reports::default();
        self.cache = Cache::empty();
    }

    pub fn open_named_document(&mut self, name: &str) {
        let key = if name.eq_ignore_ascii_case("pcb") {
            Some(DocKey::Pcb)
        } else {
            self.design().sheets.values().find(|s| s.name.eq_ignore_ascii_case(name)).map(|s| DocKey::Sheet(s.id)).or_else(|| {
                (name.eq_ignore_ascii_case("sch") || name.eq_ignore_ascii_case("schematic"))
                    .then(|| self.design().sheets_in_order().first().map(|s| DocKey::Sheet(s.id)))
                    .flatten()
            })
        };
        if let Some(k) = key {
            self.open_doc(k);
        }
    }

    pub fn open_doc(&mut self, key: DocKey) {
        if let Some(i) = self.docs.iter().position(|d| d.key == key) {
            self.active = i;
        } else {
            self.docs.push(DocView::new(key));
            self.active = self.docs.len() - 1;
        }
    }

    pub fn view(&self) -> Option<&DocView> {
        self.docs.get(self.active)
    }

    pub fn view_mut(&mut self) -> Option<&mut DocView> {
        self.docs.get_mut(self.active)
    }

    pub fn is_pcb(&self) -> bool {
        self.view().is_some_and(|v| v.is_pcb())
    }

    /// Execute a command as the user, reporting failures in the status bar.
    pub fn run(&mut self, command: Command) -> Option<Outcome> {
        match self.session.editor.execute(&command, Source::User) {
            Ok(o) => {
                self.status = o.description.clone();
                Some(o)
            }
            Err(e) => {
                self.status = format!("{}: {e}", command.label());
                self.session.note(self.status.clone());
                None
            }
        }
    }

    pub fn units(&self) -> Unit {
        if self.is_pcb() { self.prefs.pcb_unit } else { self.prefs.sch_unit }
    }

    pub fn fmt(&self, v: i64) -> String {
        hephaestus_core::geometry::format_coord(v, self.units())
    }

    /// Select components (by id) in every open document, as Altium's cross-select does.
    pub fn select_components(&mut self, components: &[ObjectId]) {
        let design = self.session.design();
        let mut per_doc: Vec<Vec<ObjectId>> = Vec::new();
        for view in &self.docs {
            let ids: Vec<ObjectId> = match view.key {
                DocKey::Sheet(sheet) => {
                    components.iter().flat_map(|c| design.symbols_of(*c).filter(|(o, _)| o.sheet == sheet).map(|(o, _)| o.id)).collect()
                }
                DocKey::Pcb => components.iter().filter_map(|c| design.footprint_of(*c).map(|(o, _)| o.id)).collect(),
            };
            per_doc.push(ids);
        }
        for (view, ids) in self.docs.iter_mut().zip(per_doc) {
            view.selection = ids;
        }
    }

    /// Components behind a document's selection.
    pub fn selected_components(&self, doc: usize) -> Vec<ObjectId> {
        let Some(view) = self.docs.get(doc) else { return Vec::new() };
        let d = self.design();
        view.selection
            .iter()
            .filter_map(|id| match d.object(*id) {
                Some(ObjectRef::Sch(o)) => match &o.kind {
                    SchKind::Symbol(s) => Some(s.component),
                    _ => None,
                },
                Some(ObjectRef::Pcb(o)) => match &o.kind {
                    PcbKind::Footprint(f) => f.component,
                    _ => None,
                },
                Some(ObjectRef::Component(c)) => Some(c.id),
                _ => None,
            })
            .collect()
    }

    /// Zoom the active document to `area` and select `ids` (violation navigation, Navigator).
    pub fn focus(&mut self, key: DocKey, ids: Vec<ObjectId>, area: Option<Rect>) {
        self.open_doc(key);
        if let Some(view) = self.view_mut() {
            view.selection = ids;
            view.pending_focus = area;
        }
    }

    fn drop_stale_selection(&mut self) {
        let d = self.session.design();
        for view in &mut self.docs {
            view.selection.retain(|id| d.schematic.contains_key(id) || d.pcb.contains_key(id));
            if view.hover.is_some_and(|h| !d.schematic.contains_key(&h) && !d.pcb.contains_key(&h)) {
                view.hover = None;
            }
        }
        let sheets: Vec<ObjectId> = d.sheets.keys().copied().collect();
        self.docs.retain(|v| match v.key {
            DocKey::Sheet(s) => sheets.contains(&s),
            DocKey::Pcb => true,
        });
        for s in sheets {
            if !self.docs.iter().any(|v| v.key == DocKey::Sheet(s)) {
                let pos = self.docs.iter().position(|v| v.key == DocKey::Pcb).unwrap_or(self.docs.len());
                self.docs.insert(pos, DocView::new(DocKey::Sheet(s)));
            }
        }
        self.active = self.active.min(self.docs.len().saturating_sub(1));
    }

    // --- agent control --------------------------------------------------------------------

    fn serve_agents(&mut self, ctx: &egui::Context) {
        let Some((_, rx)) = &self.control else { return };
        let requests: Vec<Request> = rx.try_iter().take(64).collect();
        for request in requests {
            let agent = request.agent.clone().unwrap_or_else(|| "agent".into());
            let now = ctx.input(|i| i.time);
            match request.tool.as_str() {
                "ui.screenshot" => {
                    let path = request.args.get("path").and_then(Value::as_str).map(PathBuf::from);
                    match path {
                        Some(p) if p.is_absolute() => {
                            self.screenshot = Some(Screenshot { path: p, frames: 0, reply: Some(request), exit: false, requested: false })
                        }
                        _ => request.respond(Err(hephaestus_agent_api::ApiError::invalid("path must be absolute"))),
                    }
                }
                "ui.open_document" => {
                    let name = request.args.get("document").and_then(Value::as_str).unwrap_or("pcb").to_string();
                    self.open_named_document(&name);
                    request.respond(Ok(json!({"active": name})));
                }
                "ui.select" => {
                    let ids: Vec<ObjectId> = request
                        .args
                        .get("components")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| {
                                    v.as_str()
                                        .and_then(|d| self.design().component_by_designator(d).map(|c| c.id))
                                        .or_else(|| v.as_u64().map(ObjectId))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.select_components(&ids);
                    request.respond(Ok(json!({"selected": ids.len()})));
                }
                _ => {
                    let before = self.session.editor.revision();
                    let result = tools::dispatch(&mut self.session, &request.tool, &request.args, Some(&agent));
                    if self.session.editor.revision() != before {
                        self.agent_activity.push((now, format!("{agent}: {}", request.tool)));
                        if self.agent_activity.len() > 200 {
                            self.agent_activity.remove(0);
                        }
                    }
                    if request.tool == "open_project" && result.is_ok() {
                        self.reset_documents();
                    }
                    if request.tool == "run_erc" {
                        self.reports.erc = self.session.last_erc.as_ref().map(|(_, r)| r.clone());
                    }
                    if request.tool == "run_drc" {
                        self.reports.drc = self.session.last_drc.as_ref().map(|(_, r)| r.clone());
                    }
                    request.respond(result);
                }
            }
        }
    }

    // --- screenshots ----------------------------------------------------------------------

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let mut saved: Option<Result<PathBuf, String>> = None;
        ctx.input(|i| {
            for e in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = e
                    && let Some(shot) = &self.screenshot
                {
                    let [w, h] = image.size;
                    let mut bytes = Vec::with_capacity(w * h * 4);
                    for p in &image.pixels {
                        bytes.extend_from_slice(&p.to_srgba_unmultiplied());
                    }
                    let result = image::RgbaImage::from_raw(w as u32, h as u32, bytes)
                        .ok_or_else(|| "bad screenshot buffer".to_string())
                        .and_then(|img| img.save(&shot.path).map_err(|e| e.to_string()))
                        .map(|_| shot.path.clone());
                    saved = Some(result);
                }
            }
        });
        if let Some(result) = saved
            && let Some(shot) = self.screenshot.take()
        {
            match &result {
                Ok(p) => self.status = format!("Screenshot saved to {}", p.display()),
                Err(e) => self.status = format!("Screenshot failed: {e}"),
            }
            if let Some(reply) = shot.reply {
                reply.respond(result.map(|p| json!({"path": p})).map_err(|e| hephaestus_agent_api::ApiError::new("screenshot_failed", e)));
            }
            if shot.exit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        if let Some(shot) = &mut self.screenshot {
            shot.frames += 1;
            ctx.request_repaint();
            if shot.frames >= 12 && !shot.requested {
                shot.requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }
    }

    fn autosave(&mut self, now: f64) {
        if self.prefs.autosave_seconds == 0 || now - self.last_autosave < self.prefs.autosave_seconds as f64 {
            return;
        }
        self.last_autosave = now;
        if self.session.editor.is_dirty()
            && let Some(files) = &self.session.files
            && let Err(e) = files.autosave(self.session.design())
        {
            self.status = format!("Autosave failed: {e}");
        }
    }

    // --- actions --------------------------------------------------------------------------

    pub fn perform(&mut self, action: Action) {
        match action {
            Action::NewProject | Action::OpenProject | Action::OpenDemo | Action::OpenRecent(_)
                if self.session.editor.is_dirty() && self.dialogs.confirm.is_none() =>
            {
                self.dialogs.confirm = Some(("The project has unsaved changes. Discard them?".into(), action));
            }
            Action::Exit if self.session.editor.is_dirty() && self.dialogs.confirm.is_none() => {
                self.dialogs.confirm = Some(("The project has unsaved changes. Exit without saving?".into(), action));
            }
            other => crate::commands::perform(self, other),
        }
    }

    pub fn tool_for(&self, sch: Option<SchToolKind>, pcb: Option<PcbToolKind>) -> Option<Tool> {
        if let Some(k) = sch {
            return Some(match k {
                SchToolKind::Select => Tool::Select,
                SchToolKind::Part => {
                    Tool::PlacePart { part: self.components.selected.clone().unwrap_or_else(|| "RES".into()), rotation: 0.0, mirror: false }
                }
                SchToolKind::Wire => Tool::Wire { points: Vec::new(), mode: CornerMode::HorizontalFirst },
                SchToolKind::NetLabel => Tool::NetLabel { text: "NetLabel1".into(), rotation: 0.0 },
                SchToolKind::Vcc => Tool::Power { net: "VCC".into(), style: hephaestus_core::PowerStyle::Bar, rotation: 90.0 },
                SchToolKind::Gnd => Tool::Power { net: "GND".into(), style: hephaestus_core::PowerStyle::PowerGround, rotation: 270.0 },
                SchToolKind::Port => {
                    Tool::Port { name: "Port1".into(), direction: hephaestus_core::PortDirection::Unspecified, rotation: 0.0 }
                }
                SchToolKind::Junction => Tool::Junction,
                SchToolKind::NoErc => Tool::NoErc,
                SchToolKind::Text => Tool::Text { text: "Text".into() },
            });
        }
        pcb.map(|k| match k {
            PcbToolKind::Select => Tool::Select,
            PcbToolKind::Route => Tool::Route { session: None, mode: CornerMode::Diagonal },
            PcbToolKind::Track => Tool::Track { points: Vec::new(), mode: CornerMode::Diagonal },
            PcbToolKind::Via => Tool::Via,
            PcbToolKind::Zone => Tool::Zone { points: Vec::new() },
            PcbToolKind::Keepout => Tool::Keepout { points: Vec::new() },
            PcbToolKind::BoardShape => Tool::BoardShape { points: Vec::new() },
            PcbToolKind::Measure => Tool::Measure { a: None, b: None },
        })
    }

    pub fn copper_layers(&self) -> Vec<Layer> {
        self.design().board.stack.copper_layers()
    }

    pub fn set_active_layer(&mut self, layer: Layer) {
        if let Some(v) = self.docs.iter_mut().find(|v| v.is_pcb()) {
            v.active_layer = layer;
        }
    }

    pub fn grid(&self) -> i64 {
        if self.is_pcb() { self.prefs.pcb_grid } else { self.prefs.sch_grid }
    }

    pub fn default_sheet_area(&self, sheet: ObjectId) -> Rect {
        self.design().sheets.get(&sheet).map(|s| s.rect()).unwrap_or(Rect::from_center(Point::ZERO, mil(5000.0), mil(4000.0)))
    }
}

impl HephaestusApp {
    /// One frame of the whole application (eframe calls this; tests drive it headlessly).
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        if self.frame == 1 {
            self.theme.apply(&ctx);
        }
        self.serve_agents(&ctx);
        let revision = self.session.editor.revision();
        if revision != self.cache.revision {
            self.cache.refresh(self.session.design(), revision);
            self.drop_stale_selection();
        }
        crate::commands::shortcuts(self, &ctx);
        crate::shell::show(self, ui);
        crate::dialogs::show(self, &ctx);
        if let Some(from) = self.cross_probe_from.take()
            && self.prefs.cross_select
        {
            let comps = self.selected_components(from);
            if !comps.is_empty() {
                let keep = self.docs.get(from).map(|v| v.selection.clone()).unwrap_or_default();
                self.select_components(&comps);
                if let Some(v) = self.docs.get_mut(from) {
                    v.selection = keep;
                }
            }
        }
        self.handle_screenshot(&ctx);
        self.autosave(ctx.input(|i| i.time));
        let title = format!("{}{} - Hephaestus", self.design().name, if self.session.editor.is_dirty() { " *" } else { "" });
        if title != self.last_title {
            self.last_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        if std::mem::take(&mut self.exit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for HephaestusApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "hephaestus-prefs", &self.prefs);
    }
}
