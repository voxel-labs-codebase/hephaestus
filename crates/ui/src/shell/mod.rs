//! The main window: menu bar with quick-access icons, standard toolbar, status bar and the dock.

mod document_area;
mod menus;

use craft_ui::docking::{Action as DockAction, DockArea, DockContent, DockStyle, PanelLimits};
use egui::{Align, Color32, Layout, RichText, Stroke, Ui, vec2};

use crate::actions::{Action, PcbToolKind};
use crate::app::HephaestusApp;
use crate::icons::{self, Icon};
use crate::theme::Theme;
use crate::workspace::PanelId;

pub use document_area::document_area;

pub fn show(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    egui::Panel::top("hephaestus-menubar")
        .exact_size(26.0)
        .frame(egui::Frame::new().fill(theme.chrome).inner_margin(egui::Margin::symmetric(4, 2)).stroke(Stroke::NONE))
        .show(ui, |ui| menus::menu_bar(app, ui));
    egui::Panel::top("hephaestus-toolbar")
        .exact_size(30.0)
        .frame(egui::Frame::new().fill(theme.chrome).inner_margin(egui::Margin::symmetric(6, 3)).stroke(Stroke::new(1.0, theme.divider)))
        .show(ui, |ui| toolbar(app, ui));
    egui::Panel::bottom("hephaestus-status")
        .exact_size(22.0)
        .frame(egui::Frame::new().fill(theme.chrome).inner_margin(egui::Margin::symmetric(8, 2)).stroke(Stroke::new(1.0, theme.divider)))
        .show(ui, |ui| status_bar(app, ui));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme.app_bg).inner_margin(egui::Margin::same(3)))
        .show(ui, |ui| dock(app, ui));
}

fn separator(ui: &mut Ui, theme: &Theme) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 20.0), egui::Sense::hover());
    ui.painter().line_segment([r.center_top() + vec2(0.0, 2.0), r.center_bottom() - vec2(0.0, 2.0)], Stroke::new(1.0, theme.border));
}

fn tool_button(app: &mut HephaestusApp, ui: &mut Ui, icon: Icon, tip: &str, action: Action) {
    let theme = app.theme.clone();
    let sc = crate::actions::shortcut(&action);
    let tip = if sc.is_empty() { tip.to_string() } else { format!("{tip}  ({sc})") };
    if icons::button(ui, icon, &tip, false, 24.0, &theme).clicked() {
        app.perform(action);
    }
}

/// The standard toolbar below the menus (Altium's legacy toolbars, condensed).
fn toolbar(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        tool_button(app, ui, Icon::New, "New Project", Action::NewProject);
        tool_button(app, ui, Icon::Open, "Open Project", Action::OpenProject);
        tool_button(app, ui, Icon::Save, "Save Project", Action::Save);
        separator(ui, &theme);
        ui.add_enabled_ui(app.session.editor.can_undo(), |ui| tool_button(app, ui, Icon::Undo, "Undo", Action::Undo));
        ui.add_enabled_ui(app.session.editor.can_redo(), |ui| tool_button(app, ui, Icon::Redo, "Redo", Action::Redo));
        separator(ui, &theme);
        tool_button(app, ui, Icon::ZoomFit, "Fit Document", Action::Fit);
        tool_button(app, ui, Icon::ZoomIn, "Zoom In", Action::ZoomIn);
        tool_button(app, ui, Icon::ZoomOut, "Zoom Out", Action::ZoomOut);
        separator(ui, &theme);
        let has_sel = app.view().is_some_and(|v| !v.selection.is_empty());
        ui.add_enabled_ui(has_sel, |ui| {
            tool_button(app, ui, Icon::Rotate, "Rotate Selection", Action::Rotate);
            if app.is_pcb() {
                tool_button(app, ui, Icon::Mirror, "Flip to Other Side", Action::FlipSide);
            } else {
                tool_button(app, ui, Icon::Mirror, "Mirror", Action::MirrorX);
            }
            tool_button(app, ui, Icon::Delete, "Delete", Action::Delete);
        });
        separator(ui, &theme);
        // Grid and units.
        ui.label(RichText::new("Grid").color(theme.text_dim).size(11.5));
        let pcb = app.is_pcb();
        let current = app.grid();
        let label = app.fmt(current);
        egui::ComboBox::from_id_salt("grid-combo").width(78.0).selected_text(label).show_ui(ui, |ui| {
            let steps: Vec<i64> = if pcb {
                if app.prefs.pcb_unit == hephaestus_core::geometry::Unit::Mm {
                    [0.025, 0.05, 0.1, 0.25, 0.5, 1.0].iter().map(|v| hephaestus_core::geometry::mm(*v)).collect()
                } else {
                    [1.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0].iter().map(|v| hephaestus_core::geometry::mil(*v)).collect()
                }
            } else {
                [10.0, 50.0, 100.0].iter().map(|v| hephaestus_core::geometry::mil(*v)).collect()
            };
            for s in steps {
                let text = app.fmt(s);
                if ui.selectable_label(s == current, text).clicked() {
                    if pcb { app.prefs.pcb_grid = s } else { app.prefs.sch_grid = s }
                }
            }
        });
        let unit = app.units();
        if ui
            .add(egui::Button::new(RichText::new(unit.suffix()).size(11.5)).min_size(vec2(36.0, 20.0)))
            .on_hover_text("Toggle units (Q)")
            .clicked()
        {
            app.perform(Action::ToggleUnits);
        }
        separator(ui, &theme);
        if pcb {
            let layers = app.copper_layers();
            let active = app.docs.iter().find(|v| v.is_pcb()).map(|v| v.active_layer).unwrap_or(hephaestus_core::Layer::TopCopper);
            let color = app.prefs.layer_color(active);
            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 1.0, color);
            egui::ComboBox::from_id_salt("layer-combo").width(110.0).selected_text(active.to_string()).show_ui(ui, |ui| {
                for l in layers {
                    if ui.selectable_label(l == active, l.to_string()).clicked() {
                        app.set_active_layer(l);
                    }
                }
            });
            let rules = &app.design().board.rules;
            let (w, vd, vh) = (rules.preferred_track_width, rules.preferred_via_diameter, rules.preferred_via_drill);
            ui.label(RichText::new(format!("Width {}  Via {}/{}", app.fmt(w), app.fmt(vd), app.fmt(vh))).color(theme.text_dim).size(11.5))
                .on_hover_text("Preferred routing sizes from Design › Rules");
            separator(ui, &theme);
            tool_button(app, ui, Icon::Route, "Interactive Routing", Action::PcbTool(PcbToolKind::Route));
            tool_button(app, ui, Icon::Check, "Design Rule Check", Action::RunDrc);
            let single = app.docs.iter().find(|v| v.is_pcb()).is_some_and(|v| v.single_layer);
            if icons::button(ui, Icon::Layers, "Single Layer Mode (Shift+S)", single, 24.0, &theme).clicked() {
                app.perform(Action::ToggleSingleLayer);
            }
            let rats = app.prefs.show_ratsnest;
            if icons::button(ui, Icon::Net, "Show Connection Lines", rats, 24.0, &theme).clicked() {
                app.prefs.show_ratsnest = !rats;
            }
        } else {
            tool_button(app, ui, Icon::Compile, "Compile PCB Project (C, C)", Action::Compile);
            tool_button(app, ui, Icon::Check, "Electrical Rule Check", Action::RunErc);
            if ui
                .add(egui::Button::new(RichText::new("Update PCB").size(11.5)).min_size(vec2(0.0, 20.0)))
                .on_hover_text("Design › Update PCB Document (D, U)")
                .clicked()
            {
                app.perform(Action::UpdatePcb);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(name) = app.session.editor.transaction_name() {
                ui.label(RichText::new(format!("Transaction: {name}")).color(theme.agent).size(11.5));
            }
            let pending = app.session.editor.proposals().count();
            if pending > 0
                && ui
                    .add(egui::Button::new(RichText::new(format!("{pending} proposal(s) awaiting approval")).color(theme.agent).size(11.5)))
                    .clicked()
            {
                app.focus_panel = Some(PanelId::Agents);
            }
        });
    });
}

fn status_bar(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let small = |t: String| RichText::new(t).size(11.5).color(theme.text);
        let dim = |t: String| RichText::new(t).size(11.5).color(theme.text_dim);
        if let Some(view) = app.view() {
            let cursor = view.cursor.map(|p| {
                let origin = if view.is_pcb() { app.design().board.origin } else { hephaestus_core::geometry::Point::ZERO };
                (p - origin, app.fmt(p.x - origin.x), app.fmt(p.y - origin.y))
            });
            match cursor {
                Some((_, x, y)) => ui.label(small(format!("X:{x}  Y:{y}"))),
                None => ui.label(dim("X:–  Y:–".into())),
            };
            ui.label(dim(format!("Grid: {}", app.fmt(app.grid()))));
            if view.is_pcb() {
                let l = view.active_layer;
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 1.0, app.prefs.layer_color(l));
                ui.label(small(l.to_string()));
                if let crate::documents::Tool::Route { mode, session } = &view.tool {
                    ui.label(small(format!(
                        "Routing: {}{}",
                        mode.label(),
                        session.as_ref().and_then(|s| s.net.clone()).map(|n| format!(" · Net {n}")).unwrap_or_default()
                    )));
                }
                let total: usize = app.cache.pcb.nets.values().map(|n| n.connections).sum();
                let unrouted = app.cache.pcb.unrouted.len();
                ui.label(dim(format!("Routed {}/{}", total.saturating_sub(unrouted), total)));
            } else if let crate::documents::DocKey::Sheet(s) = view.key {
                let name = app.design().sheets.get(&s).map(|s| s.name.clone()).unwrap_or_default();
                ui.label(dim(format!("Sheet: {name}")));
                // 100% is physical size on a 96 dpi display.
                ui.label(dim(format!("Zoom: {:.0}%", view.camera.scale / (96.0 / 25.4e6) * 100.0)));
            }
            ui.label(dim(view.tool.name().to_string()));
            if !view.selection.is_empty() {
                ui.label(small(format!("{} object(s) selected", view.selection.len())));
            }
        }
        ui.label(RichText::new(app.status.clone()).size(11.5).color(theme.text_dim));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.menu_button(RichText::new("Panels").size(11.5), |ui| {
                for p in PanelId::ALL {
                    let open = app.prefs.layout.contains(&p);
                    if ui.add(egui::Button::new(p.title()).selected(open)).clicked() {
                        app.perform(Action::TogglePanel(p));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Reset Layout").clicked() {
                    app.perform(Action::ResetLayout);
                    ui.close();
                }
            });
            let (text, color) = match (&app.control, &app.control_error) {
                (Some((addr, _)), _) => (format!("Agents: {addr}"), theme.ok),
                (None, Some(e)) => (format!("Agents: {e}"), theme.text_disabled),
                (None, None) => ("Agents: off".into(), theme.text_disabled),
            };
            let resp = ui.label(RichText::new(format!("● {text}")).size(11.5).color(color));
            resp.on_hover_text("Local control port for MCP agents and the Python SDK (newline-delimited JSON on 127.0.0.1)");
        });
    });
}

struct Content<'a> {
    app: &'a mut HephaestusApp,
}

impl DockContent<PanelId> for Content<'_> {
    fn body(&mut self, ui: &mut Ui, panel: &PanelId) {
        let fill = if *panel == PanelId::Documents { self.app.theme.app_bg } else { self.app.theme.panel };
        egui::Frame::new()
            .fill(fill)
            .inner_margin(if *panel == PanelId::Documents { egui::Margin::ZERO } else { egui::Margin::symmetric(6, 4) })
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                match panel {
                    PanelId::Documents => document_area(self.app, ui),
                    PanelId::Projects => crate::panels::projects::show(self.app, ui),
                    PanelId::Components => crate::panels::components::show(self.app, ui),
                    PanelId::Navigator => crate::panels::navigator::show(self.app, ui),
                    PanelId::Properties => crate::panels::properties::show(self.app, ui),
                    PanelId::Layers => crate::panels::layers::show(self.app, ui),
                    PanelId::Nets => crate::panels::nets::show(self.app, ui),
                    PanelId::Messages => crate::panels::messages::show(self.app, ui),
                    PanelId::Output => crate::panels::output::show(self.app, ui),
                    PanelId::Agents => crate::panels::agents::show(self.app, ui),
                }
            });
    }

    fn label(&mut self, _ui: &Ui, _panel: &PanelId, logical: &str, font: &egui::FontId, color: Color32) -> egui::WidgetText {
        RichText::new(logical).font(font.clone()).color(color).into()
    }
}

fn dock(app: &mut HephaestusApp, ui: &mut Ui) {
    if let Some(p) = app.focus_panel.take() {
        if !app.prefs.layout.contains(&p) {
            let _ = app.prefs.layout.apply(DockAction::Open { panel: p, anchor: Some(PanelId::Properties) });
        }
        let _ = app.prefs.layout.apply(DockAction::Activate { panel: p });
    }
    let theme = app.theme.clone();
    let mut style = DockStyle::from_ui(ui);
    style.tab_height = 24.0;
    style.gap = 3.0;
    style.min_pane = 60.0;
    style.background = theme.panel;
    style.tab_background = theme.chrome;
    style.active_background = theme.panel;
    style.text = theme.text_strong;
    style.inactive_text = theme.text_dim;
    style.border = Stroke::new(1.0, theme.border);
    style.accent = theme.accent;
    style.font = Theme::font(12.0);
    let layout = app.prefs.layout.clone();
    let mut content = Content { app };
    let output = DockArea::new(egui::Id::new("hephaestus-dock")).show_customized(
        ui,
        &layout,
        &style,
        |p| p.title().to_string(),
        |p| p.permissions(),
        |p| {
            if *p == PanelId::Documents {
                PanelLimits { min: vec2(300.0, 200.0), ..Default::default() }
            } else {
                PanelLimits { min: vec2(160.0, 70.0), ..Default::default() }
            }
        },
        &mut content,
    );
    for action in output.actions {
        if let Err(e) = app.prefs.layout.apply_with_permissions(action, |p| p.permissions()) {
            app.status = e.to_string();
        }
    }
    if let Some(e) = output.error {
        app.status = format!("Layout problem ({e}); the default layout was restored");
        app.prefs.layout = crate::workspace::default_layout();
    }
}
