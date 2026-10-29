//! Action handlers and global keyboard shortcuts.

use std::path::{Path, PathBuf};

use craft_ui::docking::Action as DockAction;
use hephaestus_agent_api::Session;
use hephaestus_commands::{Command, Source};
use hephaestus_core::geometry::{Point, Rect, Unit, mil, mm};
use hephaestus_core::{ObjectId, PcbKind, SchKind};
use hephaestus_rules::{drc, erc};

use crate::actions::{Action, PcbToolKind, SchToolKind};
use crate::app::{Clipboard, HephaestusApp};
use crate::documents::{DocKey, Tool};
use crate::workspace::{PanelId, default_layout};

fn pick_folder(title: &str, start: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(title);
    if let Some(s) = start {
        d = d.set_directory(s);
    }
    d.pick_folder()
}

fn replace_session(app: &mut HephaestusApp, session: Session) {
    app.session = session;
    app.reset_documents();
}

fn open_path(app: &mut HephaestusApp, path: PathBuf) {
    let mut session = Session::new("PCB_Project");
    match session.open(&path) {
        Ok(()) => {
            app.dialogs.recovery = session.files.as_ref().and_then(|f| f.recovery_available());
            replace_session(app, session);
            app.prefs.remember_recent(path.clone());
            app.status = format!("Opened {}", path.display());
        }
        Err(e) => app.status = format!("Could not open {}: {e}", path.display()),
    }
}

fn save_as(app: &mut HephaestusApp) {
    let start = app.session.project_root().map(Path::to_path_buf);
    let Some(folder) = pick_folder("Save Project In Folder", start.as_deref()) else { return };
    match app.session.save(Some(&folder)) {
        Ok(r) => {
            if let Some(f) = &app.session.files {
                app.prefs.remember_recent(f.manifest_path());
            }
            app.status = format!("Saved ({} files written)", r.written.len());
        }
        Err(e) => app.status = format!("Save failed: {e}"),
    }
}

fn export(app: &mut HephaestusApp, what: &[&str]) {
    let start = app.session.project_root().map(|r| r.join("Outputs"));
    let Some(dir) = pick_folder("Output Folder", start.as_deref().filter(|p| p.exists()).or(app.session.project_root())) else { return };
    let d = app.session.design();
    let mut files = 0;
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    for w in what {
        let r = match *w {
            "gerber" => hephaestus_export::gerber::export(d, &dir),
            "drill" => hephaestus_export::drill::export(d, &dir),
            "bom" => hephaestus_export::bom(d, &dir),
            _ => hephaestus_export::pick_and_place(d, &dir),
        };
        match r {
            Ok(r) => {
                files += r.files.len();
                warnings.extend(r.warnings);
            }
            Err(e) => errors.push(e.to_string()),
        }
    }
    for w in &warnings {
        app.session.note(format!("Export warning: {w}"));
    }
    for e in &errors {
        app.session.note(format!("Export error: {e}"));
    }
    app.status = format!("Exported {files} file(s) to {} ({} warning(s), {} error(s))", dir.display(), warnings.len(), errors.len());
    app.session.note(app.status.clone());
    app.focus_panel = Some(PanelId::Output);
}

/// Bounds of the active selection (schematic or PCB).
pub fn selection_bounds(app: &HephaestusApp) -> Option<Rect> {
    let view = app.view()?;
    view.selection.iter().filter_map(|id| app.cache.bounds.get(id).copied()).reduce(Rect::union)
}

fn copy(app: &mut HephaestusApp) -> bool {
    let Some(view) = app.view() else { return false };
    if view.selection.is_empty() {
        return false;
    }
    let d = app.design();
    let sch: Vec<ObjectId> = view.selection.iter().copied().filter(|id| d.schematic.contains_key(id)).collect();
    let pcb: Vec<ObjectId> = view
        .selection
        .iter()
        .copied()
        .filter(|id| {
            matches!(d.pcb.get(id).map(|o| &o.kind), Some(PcbKind::Track(_) | PcbKind::Via(_) | PcbKind::Line(_) | PcbKind::Text(_)))
        })
        .collect();
    let anchor = selection_bounds(app).map(|b| b.min).unwrap_or_default();
    let count = sch.len() + pcb.len();
    app.clipboard = Some(Clipboard { design: app.design().clone(), sch, pcb, anchor });
    app.status = format!("Copied {count} object(s)");
    true
}

/// Paste at the cursor (or offset from the original), as new objects with new designators.
fn paste(app: &mut HephaestusApp) {
    let Some(clip) = app.clipboard.clone() else { return };
    let Some(view) = app.view() else { return };
    let target = view.cursor.map(|c| c.snapped(app.grid())).unwrap_or(clip.anchor + Point::new(mil(500.0), -mil(500.0)));
    let delta = target - clip.anchor;
    let mut commands = Vec::new();
    match view.key {
        DocKey::Sheet(sheet) => {
            for id in &clip.sch {
                let Some(o) = clip.design.schematic.get(id) else { continue };
                let mut moved = o.clone();
                moved.translate(delta);
                let cmd = match moved.kind {
                    SchKind::Symbol(s) => {
                        let Some(c) = clip.design.components.get(&s.component) else { continue };
                        Command::PlaceComponent {
                            part: c.part.clone(),
                            sheet: Some(sheet),
                            position: s.position,
                            rotation: s.rotation,
                            mirror: s.mirror,
                            designator: None,
                            value: Some(c.value.clone()),
                            footprint: Some(c.footprint.clone()),
                            parameters: c.parameters.clone(),
                        }
                    }
                    SchKind::Wire(w) => Command::AddWire { sheet: Some(sheet), points: w.points },
                    SchKind::NetLabel(l) => {
                        Command::AddNetLabel { sheet: Some(sheet), position: l.position, text: l.text, rotation: l.rotation }
                    }
                    SchKind::PowerPort(p) => Command::AddPowerPort {
                        sheet: Some(sheet),
                        position: p.position,
                        net: p.net,
                        style: Some(p.style),
                        rotation: Some(p.rotation),
                    },
                    SchKind::Port(p) => Command::AddPort {
                        sheet: Some(sheet),
                        position: p.position,
                        name: p.name,
                        direction: p.direction,
                        rotation: p.rotation,
                    },
                    SchKind::Junction(j) => Command::AddJunction { sheet: Some(sheet), position: j.position },
                    SchKind::NoConnect(n) => Command::AddNoConnect { sheet: Some(sheet), position: n.position },
                    SchKind::Text(t) => Command::AddText { sheet: Some(sheet), position: t.position, text: t.text, size: Some(t.size) },
                    _ => continue,
                };
                commands.push(cmd);
            }
        }
        DocKey::Pcb => {
            for id in &clip.pcb {
                let Some(o) = clip.design.pcb.get(id) else { continue };
                let mut moved = o.clone();
                moved.translate(delta);
                let cmd = match moved.kind {
                    PcbKind::Track(t) => Command::AddTrack { start: t.start, end: t.end, layer: t.layer, width: Some(t.width), net: t.net },
                    PcbKind::Via(v) => Command::AddVia {
                        position: v.position,
                        diameter: Some(v.diameter),
                        drill: Some(v.drill),
                        net: v.net,
                        from: Some(v.from),
                        to: Some(v.to),
                    },
                    PcbKind::Line(l) => Command::AddBoardLine { layer: l.layer, start: l.start, end: l.end, width: l.width },
                    PcbKind::Text(t) => Command::AddBoardText { layer: t.layer, position: t.position, text: t.text, height: t.height },
                    _ => continue,
                };
                commands.push(cmd);
            }
        }
    }
    if commands.is_empty() {
        app.status = "Nothing on the clipboard can be pasted here".into();
        return;
    }
    if let Some(out) = app.run(Command::Batch { commands }) {
        let created: Vec<ObjectId> =
            out.created.iter().copied().filter(|id| app.design().schematic.contains_key(id) || app.design().pcb.contains_key(id)).collect();
        if let Some(v) = app.view_mut() {
            v.selection = created;
        }
        app.status = "Pasted".into();
    }
}

fn delete(app: &mut HephaestusApp) {
    let Some(view) = app.view() else { return };
    if view.selection.is_empty() {
        return;
    }
    let ids = view.selection.clone();
    if app.run(Command::DeleteObjects { ids }).is_some()
        && let Some(v) = app.view_mut()
    {
        v.selection.clear();
    }
}

fn grid_steps(pcb: bool, unit: Unit) -> Vec<i64> {
    match (pcb, unit) {
        (false, _) => vec![mil(50.0), mil(100.0)],
        (true, Unit::Mm) => vec![mm(0.025), mm(0.05), mm(0.1), mm(0.25), mm(0.5), mm(1.0)],
        (true, _) => vec![mil(1.0), mil(5.0), mil(10.0), mil(20.0), mil(25.0), mil(50.0), mil(100.0)],
    }
}

pub fn perform(app: &mut HephaestusApp, action: Action) {
    match action {
        Action::NewProject => {
            replace_session(app, Session::new("PCB_Project"));
            app.status = "New project".into();
        }
        Action::OpenProject => {
            let picked = rfd::FileDialog::new().set_title("Open Project").add_filter("Hephaestus project", &["pcbprj", "json"]).pick_file();
            if let Some(path) = picked {
                open_path(app, path);
            }
        }
        Action::OpenRecent(path) => open_path(app, path),
        Action::OpenDemo => match hephaestus_agent_api::demo::flasher() {
            Ok(s) => {
                replace_session(app, s);
                app.status = "Opened the LED flasher example".into();
            }
            Err(e) => app.status = format!("Demo failed: {e}"),
        },
        Action::Save => {
            if app.session.files.is_some() {
                match app.session.save(None) {
                    Ok(r) => app.status = format!("Saved ({} written, {} unchanged)", r.written.len(), r.unchanged),
                    Err(e) => app.status = format!("Save failed: {e}"),
                }
            } else {
                save_as(app);
            }
        }
        Action::SaveAs => save_as(app),
        Action::Exit => app.exit_requested = true,
        Action::Undo => match app.session.editor.undo(Source::User) {
            Ok(d) => app.status = format!("Undo: {d}"),
            Err(e) => app.status = e.to_string(),
        },
        Action::Redo => match app.session.editor.redo(Source::User) {
            Ok(d) => app.status = format!("Redo: {d}"),
            Err(e) => app.status = e.to_string(),
        },
        Action::Copy => {
            copy(app);
        }
        Action::Cut => {
            if copy(app) {
                delete(app);
            }
        }
        Action::Paste => paste(app),
        Action::Delete => delete(app),
        Action::SelectAll => {
            let d = app.session.design();
            if let Some(v) = app.docs.get_mut(app.active) {
                v.selection = match v.key {
                    DocKey::Sheet(s) => d.sheet_objects(s).map(|o| o.id).collect(),
                    DocKey::Pcb => d.pcb.keys().copied().collect(),
                };
            }
        }
        Action::ClearSelection => {
            if let Some(v) = app.view_mut() {
                v.selection.clear();
            }
        }
        Action::Rotate | Action::MirrorX => {
            let rotate = action == Action::Rotate;
            let Some(view) = app.view_mut() else { return };
            match &mut view.tool {
                Tool::PlacePart { rotation, mirror, .. } => {
                    if rotate {
                        *rotation = (*rotation + 90.0) % 360.0
                    } else {
                        *mirror = !*mirror
                    }
                }
                Tool::NetLabel { rotation, .. } | Tool::Port { rotation, .. } if rotate => {
                    *rotation = if *rotation == 0.0 { 90.0 } else { 0.0 }
                }
                Tool::Power { rotation, .. } if rotate => *rotation = (*rotation + 90.0) % 360.0,
                Tool::Wire { mode, .. } | Tool::Track { mode, .. } | Tool::Route { mode, .. } if rotate => {
                    *mode = mode.next(view.key == DocKey::Pcb)
                }
                _ => {
                    let ids = view.selection.clone();
                    if !ids.is_empty() {
                        let cmd = if rotate {
                            Command::RotateObjects { ids, degrees: 90.0, pivot: None }
                        } else {
                            Command::MirrorObjects { ids, pivot: None }
                        };
                        app.run(cmd);
                    }
                }
            }
        }
        Action::FlipSide => {
            let Some(view) = app.view() else { return };
            let ids: Vec<ObjectId> = view
                .selection
                .iter()
                .copied()
                .filter(|id| matches!(app.design().pcb.get(id).map(|o| &o.kind), Some(PcbKind::Footprint(_))))
                .collect();
            if !ids.is_empty() {
                app.run(Command::MirrorObjects { ids, pivot: None });
            }
        }
        Action::Fit => {
            if let Some(v) = app.view_mut() {
                v.needs_fit = true;
            }
        }
        Action::FitDocument => {
            if let Some(v) = app.view_mut() {
                if v.is_pcb() { v.needs_fit = true } else { v.fit_document = true }
            }
        }
        Action::FitSelection => {
            let b = selection_bounds(app);
            if let Some(v) = app.view_mut() {
                v.pending_focus = b;
            }
        }
        Action::ZoomIn | Action::ZoomOut => {
            if let Some(v) = app.view_mut() {
                let f = if action == Action::ZoomIn { 1.5 } else { 1.0 / 1.5 };
                let vp = v.viewport;
                v.camera.zoom_about(vp, vp.center(), f);
            }
        }
        Action::ToggleUnits => {
            if app.is_pcb() {
                app.prefs.pcb_unit = app.prefs.pcb_unit.toggled();
                app.prefs.pcb_grid = if app.prefs.pcb_unit == Unit::Mm { mm(0.1) } else { mil(5.0) };
            } else {
                app.prefs.sch_unit = app.prefs.sch_unit.toggled();
            }
            app.status = format!("Units: {}", app.units().suffix());
        }
        Action::CycleGrid => {
            let pcb = app.is_pcb();
            let steps = grid_steps(pcb, app.units());
            let current = app.grid();
            let next = steps.iter().copied().find(|s| *s > current).or(steps.first().copied()).unwrap_or(current);
            if pcb {
                app.prefs.pcb_grid = next
            } else {
                app.prefs.sch_grid = next
            }
            app.status = format!("Snap grid {}", app.fmt(next));
        }
        Action::ToggleGridVisible => app.prefs.sch_grid_visible = !app.prefs.sch_grid_visible,
        Action::Compile | Action::RunErc => {
            let report = erc::check(app.design());
            app.status = format!(
                "Compiled: {} net(s); ERC {} error(s), {} warning(s)",
                app.cache.netlist.nets.len(),
                report.errors,
                report.warnings
            );
            app.reports.erc = Some(report);
            app.focus_panel = Some(PanelId::Messages);
        }
        Action::RunDrc => {
            let report = drc::check(app.design());
            app.status = format!("DRC: {} error(s), {} warning(s)", report.errors, report.warnings);
            app.reports.drc = Some(report);
            app.focus_panel = Some(PanelId::Messages);
        }
        Action::UpdatePcb => {
            if let Some(out) = app.run(Command::UpdatePcb {}) {
                app.status = format!(
                    "Update PCB: {} footprint(s) added, {} updated",
                    out.created.len(),
                    out.touched.len().saturating_sub(out.created.len())
                );
                app.open_doc(DocKey::Pcb);
            }
        }
        Action::AddSheet => {
            let n = app.design().sheets.len() + 1;
            let mut name = format!("Sheet{n}");
            let mut k = n;
            while app.design().sheets.values().any(|s| s.name == name) {
                k += 1;
                name = format!("Sheet{k}");
            }
            if let Some(out) = app.run(Command::AddSheet { name, size: None })
                && let Some(id) = out.created.first()
            {
                app.open_doc(DocKey::Sheet(*id));
            }
        }
        Action::RemoveSheet => {
            if let Some(DocKey::Sheet(s)) = app.view().map(|v| v.key) {
                app.run(Command::RemoveSheet { sheet: s });
            }
        }
        Action::OpenDoc(key) => app.open_doc(key),
        Action::SchTool(kind) => {
            if app.is_pcb()
                && let Some(s) = app.design().sheets_in_order().first().map(|s| s.id)
            {
                app.open_doc(DocKey::Sheet(s));
            }
            if let Some(tool) = app.tool_for(Some(kind), None)
                && let Some(v) = app.view_mut()
            {
                v.tool = tool;
                v.selection.clear();
            }
            if kind == SchToolKind::Part && app.components.selected.is_none() {
                app.focus_panel = Some(PanelId::Components);
            }
        }
        Action::PcbTool(kind) => {
            app.open_doc(DocKey::Pcb);
            if let Some(tool) = app.tool_for(None, Some(kind))
                && let Some(v) = app.view_mut()
            {
                v.tool = tool;
                if kind != PcbToolKind::Select {
                    v.selection.clear();
                }
            }
        }
        Action::NextLayer | Action::PrevLayer => {
            let layers = app.copper_layers();
            if let Some(v) = app.docs.iter_mut().find(|v| v.is_pcb()) {
                let i = layers.iter().position(|l| *l == v.active_layer).unwrap_or(0);
                let n = layers.len().max(1);
                let j = if action == Action::NextLayer { (i + 1) % n } else { (i + n - 1) % n };
                if let Some(l) = layers.get(j) {
                    v.active_layer = *l;
                    app.status = format!("Current layer: {l}");
                }
            }
        }
        Action::ToggleSingleLayer => {
            if let Some(v) = app.docs.iter_mut().find(|v| v.is_pcb()) {
                v.single_layer = !v.single_layer;
            }
        }
        Action::LayerStack => app.dialogs.stack = Some(app.design().board.stack.copper_count()),
        Action::DesignRules => app.dialogs.rules = Some(app.design().board.rules.clone()),
        Action::ExportGerber => export(app, &["gerber"]),
        Action::ExportDrill => export(app, &["drill"]),
        Action::ExportBom => export(app, &["bom"]),
        Action::ExportPickPlace => export(app, &["pnp"]),
        Action::ExportAll => export(app, &["gerber", "drill", "bom", "pnp"]),
        Action::Align(mode) => {
            if let Some(v) = app.view() {
                let ids = v.selection.clone();
                if ids.len() > 1 {
                    app.run(Command::AlignObjects { ids, mode });
                }
            }
        }
        Action::Distribute(axis) => {
            if let Some(v) = app.view() {
                let ids = v.selection.clone();
                if ids.len() > 2 {
                    app.run(Command::DistributeObjects { ids, axis });
                }
            }
        }
        Action::Lock(locked) => {
            if let Some(v) = app.view() {
                let ids: Vec<ObjectId> = v.selection.iter().copied().filter(|id| app.design().pcb.contains_key(id)).collect();
                if !ids.is_empty() {
                    app.run(Command::LockObjects { ids, locked });
                }
            }
        }
        Action::CrossProbe => {
            let comps = app.selected_components(app.active);
            if comps.is_empty() {
                app.status = "Select a component to cross-probe".into();
                return;
            }
            let target = if app.is_pcb() {
                app.design().symbols_of(comps[0]).next().map(|(o, _)| DocKey::Sheet(o.sheet))
            } else {
                Some(DocKey::Pcb)
            };
            if let Some(key) = target {
                app.select_components(&comps);
                app.open_doc(key);
                let b = selection_bounds(app);
                if let Some(v) = app.view_mut() {
                    v.pending_focus = b.map(|b| b.inflated(mil(300.0)));
                }
            }
        }
        Action::ClearHighlight => {
            for v in &mut app.docs {
                v.highlight_net = None;
            }
        }
        Action::TogglePanel(p) => {
            let layout = &mut app.prefs.layout;
            let result = if layout.contains(&p) {
                layout.apply(DockAction::Close { panel: p })
            } else {
                layout
                    .apply(DockAction::Open { panel: p, anchor: Some(PanelId::Properties) })
                    .or_else(|_| layout.apply(DockAction::Open { panel: p, anchor: None }))
            };
            if let Err(e) = result {
                app.status = e.to_string();
            }
        }
        Action::ResetLayout => app.prefs.layout = default_layout(),
        Action::Preferences => app.dialogs.preferences = true,
        Action::About => app.dialogs.about = true,
        Action::ShortcutHelp => app.dialogs.shortcuts = true,
    }
}

/// Global keyboard handling (only while no text field has focus).
pub fn shortcuts(app: &mut HephaestusApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let mut actions = Vec::new();
    let (events, mods) = ctx.input(|i| (i.events.clone(), i.modifiers));
    for e in events {
        match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                use egui::Key as K;
                if let Some(prefix) = app.key_prefix.take() {
                    let pcb = app.is_pcb();
                    let a = match (prefix, key, pcb) {
                        ('P', K::W, false) => Some(Action::SchTool(SchToolKind::Wire)),
                        ('P', K::N, false) => Some(Action::SchTool(SchToolKind::NetLabel)),
                        ('P', K::O, false) => Some(Action::SchTool(SchToolKind::Vcc)),
                        ('P', K::G, false) => Some(Action::SchTool(SchToolKind::Gnd)),
                        ('P', K::R, false) => Some(Action::SchTool(SchToolKind::Port)),
                        ('P', K::J, false) => Some(Action::SchTool(SchToolKind::Junction)),
                        ('P', K::V, false) => Some(Action::SchTool(SchToolKind::NoErc)),
                        ('P', K::T, false) => Some(Action::SchTool(SchToolKind::Text)),
                        ('P', K::P, false) => Some(Action::SchTool(SchToolKind::Part)),
                        ('P', K::T, true) => Some(Action::PcbTool(PcbToolKind::Route)),
                        ('P', K::L, true) => Some(Action::PcbTool(PcbToolKind::Track)),
                        ('P', K::V, true) => Some(Action::PcbTool(PcbToolKind::Via)),
                        ('P', K::G, true) => Some(Action::PcbTool(PcbToolKind::Zone)),
                        ('P', K::K, true) => Some(Action::PcbTool(PcbToolKind::Keepout)),
                        ('D', K::U, _) => Some(Action::UpdatePcb),
                        ('D', K::R, _) => Some(Action::DesignRules),
                        ('D', K::K, _) => Some(Action::LayerStack),
                        ('D', K::E, _) => Some(Action::RunErc),
                        ('T', K::D, _) => Some(Action::RunDrc),
                        ('T', K::P, _) => Some(Action::Preferences),
                        ('C', K::C, _) => Some(Action::Compile),
                        ('V', K::F, _) | ('Z', K::A, _) => Some(Action::Fit),
                        ('V', K::D, _) => Some(Action::FitDocument),
                        ('Z', K::S, _) => Some(Action::FitSelection),
                        ('X', K::A, _) => Some(Action::ClearSelection),
                        _ => None,
                    };
                    match a {
                        Some(a) => actions.push(a),
                        None => app.status = "Shortcut cancelled".into(),
                    }
                    continue;
                }
                let ctrl = modifiers.command || modifiers.ctrl;
                let a = match key {
                    K::N if ctrl => Some(Action::NewProject),
                    K::O if ctrl => Some(Action::OpenProject),
                    K::S if ctrl && modifiers.shift => Some(Action::SaveAs),
                    K::S if ctrl => Some(Action::Save),
                    K::Z if ctrl && modifiers.shift => Some(Action::Redo),
                    K::Z if ctrl => Some(Action::Undo),
                    K::Y if ctrl => Some(Action::Redo),
                    K::X if ctrl => Some(Action::Cut),
                    K::C if ctrl => Some(Action::Copy),
                    K::V if ctrl => Some(Action::Paste),
                    K::A if ctrl => Some(Action::SelectAll),
                    K::W if ctrl => Some(Action::PcbTool(PcbToolKind::Route)),
                    K::M if ctrl => Some(Action::PcbTool(PcbToolKind::Measure)),
                    K::PageDown if ctrl => Some(Action::Fit),
                    K::Delete => Some(Action::Delete),
                    K::F1 => Some(Action::ShortcutHelp),
                    K::Q if !ctrl => Some(Action::ToggleUnits),
                    K::G if !ctrl && !modifiers.shift => Some(Action::CycleGrid),
                    K::S if modifiers.shift && !ctrl => Some(Action::ToggleSingleLayer),
                    K::C if modifiers.shift && !ctrl => Some(Action::ClearHighlight),
                    K::Space if !modifiers.shift && !ctrl => Some(Action::Rotate),
                    K::X | K::Y if !ctrl && app.view().is_some_and(|v| matches!(v.tool, Tool::PlacePart { .. })) => Some(Action::MirrorX),
                    K::L if !ctrl && app.is_pcb() => {
                        if app.view().is_some_and(|v| !v.selection.is_empty()) && mods.alt {
                            Some(Action::FlipSide)
                        } else {
                            app.focus_panel = Some(PanelId::Layers);
                            None
                        }
                    }
                    K::P | K::D | K::T | K::C | K::V | K::Z | K::X if !ctrl && !modifiers.shift && !modifiers.alt => {
                        let c = match key {
                            K::P => 'P',
                            K::D => 'D',
                            K::T => 'T',
                            K::C => 'C',
                            K::V => 'V',
                            K::Z => 'Z',
                            _ => 'X',
                        };
                        app.key_prefix = Some(c);
                        app.status = match c {
                            'P' if app.is_pcb() => "Place: T Interactive Routing · L Line · V Via · G Polygon Pour · K Keepout",
                            'P' => {
                                "Place: P Part · W Wire · N Net Label · O Power (VCC) · G Ground · R Port · J Junction · V No ERC · T Text"
                            }
                            'D' => "Design: U Update PCB · R Rules · K Layer Stack · E Electrical Rule Check",
                            'T' => "Tools: D Design Rule Check · P Preferences",
                            'C' => "Project: C Compile",
                            'V' => "View: F Fit Document",
                            'Z' => "Zoom: A Fit All · S Fit Selection",
                            _ => "Deselect: A All",
                        }
                        .into();
                        None
                    }
                    _ => None,
                };
                if let Some(a) = a {
                    actions.push(a);
                }
            }
            egui::Event::Text(t) if app.is_pcb() => match t.as_str() {
                "*" | "+" => actions.push(Action::NextLayer),
                "-" => actions.push(Action::PrevLayer),
                _ => {}
            },
            _ => {}
        }
    }
    for a in actions {
        app.perform(a);
    }
}
