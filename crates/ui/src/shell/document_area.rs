//! Document tabs, the Active Bar and the canvas host.

use egui::{Align2, Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::actions::{Action, PcbToolKind, SchToolKind};
use crate::app::HephaestusApp;
use crate::documents::{DocKey, Tool};
use crate::icons::{self, Icon};
use crate::theme::Theme;

fn doc_name(app: &HephaestusApp, key: DocKey) -> String {
    match key {
        DocKey::Sheet(s) => app.design().sheets.get(&s).map(|s| format!("{}.SchDoc", s.name)).unwrap_or_else(|| "Sheet".into()),
        DocKey::Pcb => format!("{}.PcbDoc", app.design().name),
    }
}

fn tabs(app: &mut HephaestusApp, ui: &mut Ui) {
    let theme = app.theme.clone();
    let height = 25.0;
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(strip, 0.0, theme.chrome);
    ui.painter().line_segment([strip.left_bottom(), strip.right_bottom()], Stroke::new(1.0, theme.border));
    let font = Theme::font(12.0);
    let names: Vec<String> = app.docs.iter().map(|d| doc_name(app, d.key)).collect();
    let natural: Vec<f32> = names
        .iter()
        .map(|n| {
            ui.fonts_mut(|f| f.layout_no_wrap(n.clone(), font.clone(), theme.text).size().x)
                + 46.0
                + if app.session.editor.is_dirty() { 8.0 } else { 0.0 }
        })
        .collect();
    let fit = craft_ui::tabs::fit(&natural, app.active, strip.width() - 4.0, 90.0, 26.0);
    let mut x = strip.left() + 2.0;
    let mut clicked = None;
    for (i, w) in fit.shown.iter().copied() {
        let rect = Rect::from_min_size(pos2(x, strip.top() + 2.0), vec2(w, height - 2.0));
        x += w + 1.0;
        let active = i == app.active;
        let key = app.docs.get(i).map(|d| d.key);
        let name = names.get(i).cloned().unwrap_or_default();
        let resp = craft_ui::tabs::Tab::new(egui::Id::new(("doc-tab", i)), &name, active).show_at(ui, rect, |ui, r| {
            let fill = if active {
                theme.app_bg
            } else if r.hovered() {
                theme.hover
            } else {
                theme.chrome
            };
            ui.painter().rect_filled(rect, egui::CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, fill);
            if active {
                ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, theme.accent));
            }
            let icon = if matches!(key, Some(DocKey::Pcb)) { Icon::Pcb } else { Icon::Schematic };
            let ir = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(14.0, 14.0));
            icons::paint(
                ui.painter(),
                ir,
                icon,
                if matches!(key, Some(DocKey::Pcb)) { Color32::from_rgb(90, 190, 90) } else { Color32::from_rgb(230, 190, 80) },
            );
            let color = if active { theme.text_strong } else { theme.text };
            let text_rect = Rect::from_min_max(pos2(rect.left() + 24.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
            ui.painter().with_clip_rect(text_rect).text(
                pos2(text_rect.left(), rect.center().y),
                Align2::LEFT_CENTER,
                &name,
                font.clone(),
                color,
            );
        });
        if resp.clicked() {
            clicked = Some(i);
        }
        resp.on_hover_text(&name);
    }
    if !fit.overflow.is_empty() {
        let r = Rect::from_min_size(pos2(x, strip.top() + 2.0), vec2(fit.overflow_width.max(20.0), height - 4.0));
        let resp = ui.interact(r, egui::Id::new("doc-tab-overflow"), Sense::click());
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "»", font.clone(), theme.text);
        egui::Popup::menu(&resp).show(|ui| {
            for i in &fit.overflow {
                if let Some(n) = names.get(*i)
                    && ui.button(n).clicked()
                {
                    clicked = Some(*i);
                }
            }
        });
    }
    if let Some(i) = clicked {
        app.active = i;
    }
}

/// Altium's Active Bar: a compact floating toolbar centred over the canvas.
fn active_bar(app: &mut HephaestusApp, ui: &mut Ui, canvas: Rect) {
    let theme = app.theme.clone();
    let pcb = app.is_pcb();
    let current = app.view().map(|v| v.tool.clone()).unwrap_or(Tool::Select);
    type Entry = (Icon, &'static str, Action, fn(&Tool) -> bool);
    let sch: [Entry; 10] = [
        (Icon::Select, "Select", Action::SchTool(SchToolKind::Select), |t| matches!(t, Tool::Select)),
        (Icon::Part, "Place Part (P, P)", Action::SchTool(SchToolKind::Part), |t| matches!(t, Tool::PlacePart { .. })),
        (Icon::Wire, "Place Wire (P, W)", Action::SchTool(SchToolKind::Wire), |t| matches!(t, Tool::Wire { .. })),
        (Icon::NetLabel, "Place Net Label (P, N)", Action::SchTool(SchToolKind::NetLabel), |t| matches!(t, Tool::NetLabel { .. })),
        (
            Icon::PowerVcc,
            "Place VCC Power Port (P, O)",
            Action::SchTool(SchToolKind::Vcc),
            |t| matches!(t, Tool::Power { net, .. } if net != "GND"),
        ),
        (
            Icon::PowerGnd,
            "Place GND Power Port (P, G)",
            Action::SchTool(SchToolKind::Gnd),
            |t| matches!(t, Tool::Power { net, .. } if net == "GND"),
        ),
        (Icon::Port, "Place Port (P, R)", Action::SchTool(SchToolKind::Port), |t| matches!(t, Tool::Port { .. })),
        (Icon::Junction, "Place Junction (P, J)", Action::SchTool(SchToolKind::Junction), |t| matches!(t, Tool::Junction)),
        (Icon::NoErc, "Place No ERC (P, V)", Action::SchTool(SchToolKind::NoErc), |t| matches!(t, Tool::NoErc)),
        (Icon::Text, "Place Text (P, T)", Action::SchTool(SchToolKind::Text), |t| matches!(t, Tool::Text { .. })),
    ];
    let pcb_tools: [Entry; 8] = [
        (Icon::Select, "Select", Action::PcbTool(PcbToolKind::Select), |t| matches!(t, Tool::Select)),
        (Icon::Route, "Interactive Routing (Ctrl+W)", Action::PcbTool(PcbToolKind::Route), |t| matches!(t, Tool::Route { .. })),
        (Icon::Track, "Place Line (P, L)", Action::PcbTool(PcbToolKind::Track), |t| matches!(t, Tool::Track { .. })),
        (Icon::Via, "Place Via (P, V)", Action::PcbTool(PcbToolKind::Via), |t| matches!(t, Tool::Via)),
        (Icon::Polygon, "Place Polygon Pour (P, G)", Action::PcbTool(PcbToolKind::Zone), |t| matches!(t, Tool::Zone { .. })),
        (Icon::Keepout, "Place Keepout (P, K)", Action::PcbTool(PcbToolKind::Keepout), |t| matches!(t, Tool::Keepout { .. })),
        (Icon::BoardShape, "Redefine Board Shape", Action::PcbTool(PcbToolKind::BoardShape), |t| matches!(t, Tool::BoardShape { .. })),
        (Icon::Measure, "Measure Distance (Ctrl+M)", Action::PcbTool(PcbToolKind::Measure), |t| matches!(t, Tool::Measure { .. })),
    ];
    let entries: &[Entry] = if pcb { &pcb_tools } else { &sch };
    let size = 26.0;
    let extra = if pcb { 2 } else { 1 };
    let width = (entries.len() + extra) as f32 * (size + 1.0) + 12.0;
    let rect = Rect::from_center_size(pos2(canvas.center().x, canvas.top() + 20.0), vec2(width, size + 6.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.painter().rect(rect, 4.0, theme.chrome.gamma_multiply(0.96), Stroke::new(1.0, theme.border), egui::StrokeKind::Inside);
    child.add_space(5.0);
    child.spacing_mut().item_spacing.x = 1.0;
    for (icon, tip, action, is) in entries {
        if icons::button(&mut child, *icon, tip, is(&current), size, &theme).clicked() {
            app.perform(action.clone());
        }
    }
    let (r, _) = child.allocate_exact_size(vec2(5.0, size), Sense::hover());
    child.painter().line_segment([r.center_top() + vec2(0.0, 4.0), r.center_bottom() - vec2(0.0, 4.0)], Stroke::new(1.0, theme.border));
    if pcb {
        if icons::button(&mut child, Icon::Check, "Design Rule Check (T, D)", false, size, &theme).clicked() {
            app.perform(Action::RunDrc);
        }
        if icons::button(&mut child, Icon::Highlight, "Clear Net Highlight (Shift+C)", false, size, &theme).clicked() {
            app.perform(Action::ClearHighlight);
        }
    } else if icons::button(&mut child, Icon::Compile, "Compile (C, C)", false, size, &theme).clicked() {
        app.perform(Action::Compile);
    }
}

/// Altium's colour-coded layer tabs along the bottom of the PCB editor.
fn layer_tabs(app: &mut HephaestusApp, ui: &mut Ui, rect: Rect) {
    let theme = app.theme.clone();
    ui.painter().rect_filled(rect, 0.0, theme.chrome);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, theme.border));
    let copper = app.design().board.stack.copper_count();
    let layers: Vec<hephaestus_core::Layer> =
        hephaestus_core::Layer::display_order(copper).into_iter().filter(|l| app.prefs.layer_visible(*l)).collect();
    let (active, single) = app
        .docs
        .iter()
        .find(|v| v.is_pcb())
        .map(|v| (v.active_layer, v.single_layer))
        .unwrap_or((hephaestus_core::Layer::TopCopper, false));
    let font = Theme::font(11.5);
    let mut x = rect.left() + 4.0;
    // "LS": the layer set / single-layer toggle at the left, like Altium.
    let ls = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(24.0, rect.height() - 6.0));
    let r = ui.interact(ls, egui::Id::new("layer-ls"), Sense::click()).on_hover_text("Single Layer Mode (Shift+S)");
    ui.painter().rect(ls, 2.0, if single { theme.pressed } else { theme.header }, Stroke::new(1.0, theme.border), egui::StrokeKind::Inside);
    ui.painter().text(ls.center(), Align2::CENTER_CENTER, "LS", font.clone(), if single { theme.accent } else { theme.text });
    if r.clicked() {
        app.perform(Action::ToggleSingleLayer);
    }
    x += 28.0;
    for l in layers {
        let name = l.to_string();
        let w = ui.fonts_mut(|f| f.layout_no_wrap(name.clone(), font.clone(), theme.text).size().x) + 24.0;
        if x + w > rect.right() - 4.0 {
            break;
        }
        let tab = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(w, rect.height() - 6.0));
        x += w + 2.0;
        let resp = ui.interact(tab, egui::Id::new(("layer-tab", name.clone())), Sense::click());
        let is_active = l == active;
        let fill = if is_active {
            theme.app_bg
        } else if resp.hovered() {
            theme.hover
        } else {
            theme.header
        };
        ui.painter().rect(
            tab,
            2.0,
            fill,
            Stroke::new(1.0, if is_active { theme.text_dim } else { theme.border }),
            egui::StrokeKind::Inside,
        );
        let sw = Rect::from_center_size(pos2(tab.left() + 9.0, tab.center().y), vec2(9.0, 9.0));
        ui.painter().rect_filled(sw, 1.0, app.prefs.layer_color(l));
        ui.painter().text(
            pos2(tab.left() + 17.0, tab.center().y),
            Align2::LEFT_CENTER,
            &name,
            font.clone(),
            if is_active { theme.text_strong } else { theme.text },
        );
        if resp.clicked() && l.is_copper() {
            app.set_active_layer(l);
        }
        resp.on_hover_text(if l.is_copper() {
            "Click to make this the current layer"
        } else {
            "Visible layer (toggle in View Configuration)"
        });
    }
}

pub fn document_area(app: &mut HephaestusApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    tabs(app, ui);
    let pcb = app.is_pcb();
    let avail = ui.available_rect_before_wrap();
    let layer_h = if pcb { 24.0 } else { 0.0 };
    let canvas = Rect::from_min_max(avail.min, pos2(avail.max.x, avail.max.y - layer_h));
    if canvas.width() < 50.0 || canvas.height() < 50.0 {
        return;
    }
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(canvas));
    if pcb {
        crate::pcb::editor::show(app, &mut cui, canvas);
    } else {
        crate::sch::editor::show(app, &mut cui, canvas);
    }
    active_bar(app, ui, canvas);
    if pcb {
        let lt = Rect::from_min_max(pos2(avail.min.x, canvas.max.y), avail.max);
        layer_tabs(app, ui, lt);
    }
    ui.allocate_rect(avail, Sense::hover());
    let _ = RichText::new("");
}
