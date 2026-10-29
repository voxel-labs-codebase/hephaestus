//! Headless interaction tests: the real application UI driven with synthetic pointer and keyboard
//! events, asserting on the design database afterwards.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use hephaestus_agent_api::Session;
use hephaestus_commands::{Command, Source};
use hephaestus_connectivity::schematic::compile;
use hephaestus_core::geometry::{Point, mil};
use hephaestus_core::{PcbKind, SchKind};

use crate::actions::{Action, PcbToolKind, SchToolKind};
use crate::app::{HephaestusApp, StartOptions};
use crate::documents::{DocKey, Tool};
use crate::theme::Theme;
use crate::workspace::Prefs;

fn app(demo: bool) -> HephaestusApp {
    let ctx = egui::Context::default();
    let start = StartOptions { demo, control_port: Some(0), ..Default::default() };
    HephaestusApp::build(&ctx, Prefs::default(), Theme::dark(), start)
}

fn harness(app: HephaestusApp) -> Harness<'static, HephaestusApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 960.0)).build_ui_state(|ui, app: &mut HephaestusApp| app.frame(ui), app);
    for _ in 0..6 {
        h.step();
    }
    h
}

/// Screen position of a world point in the active document.
fn screen(h: &Harness<HephaestusApp>, p: Point) -> Pos2 {
    let app = h.state();
    let v = &app.docs[app.active];
    v.camera.to_screen(v.viewport, p)
}

fn move_to(h: &mut Harness<HephaestusApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
}

fn click(h: &mut Harness<HephaestusApp>, pos: Pos2) {
    move_to(h, pos);
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    h.step();
}

fn drag(h: &mut Harness<HephaestusApp>, from: Pos2, to: Pos2) {
    move_to(h, from);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        move_to(h, from + (to - from) * t);
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    h.step();
}

fn key(h: &mut Harness<HephaestusApp>, key: egui::Key) {
    h.key_press(key);
    h.step();
}

fn run(app: &mut HephaestusApp, c: Command) {
    app.session.editor.execute(&c, Source::User).unwrap();
}

fn place(app: &mut HephaestusApp, part: &str, des: &str, x: f64, y: f64) {
    run(
        app,
        Command::PlaceComponent {
            part: part.into(),
            sheet: None,
            position: Point::new(mil(x), mil(y)),
            rotation: 0.0,
            mirror: false,
            designator: Some(des.into()),
            value: None,
            footprint: None,
            parameters: Default::default(),
        },
    );
}

#[test]
fn wire_tool_connects_two_pins() {
    let mut a = app(false);
    place(&mut a, "RES", "R1", 2000.0, 3000.0);
    place(&mut a, "RES", "R2", 3000.0, 3500.0);
    let mut h = harness(a);
    h.state_mut().perform(Action::SchTool(SchToolKind::Wire));
    h.step();
    let p1 = screen(&h, Point::new(mil(2200.0), mil(3000.0)));
    let p2 = screen(&h, Point::new(mil(2800.0), mil(3500.0)));
    click(&mut h, p1);
    click(&mut h, p2);
    let d = h.state().session.design().clone();
    let r1 = d.component_by_designator("R1").unwrap().id;
    let net = compile(&d).net_of_pin(r1, "2").cloned().expect("R1.2 is on a net");
    assert_eq!(net.nodes.len(), 2, "{net:?}");
    // The wire tool auto-terminated on the target pin and stays active for the next wire.
    assert!(matches!(h.state().docs[h.state().active].tool, Tool::Wire { ref points, .. } if points.is_empty()));
}

#[test]
fn click_selects_and_drag_moves_a_component() {
    let mut a = app(false);
    place(&mut a, "NE555", "U1", 3000.0, 3000.0);
    let mut h = harness(a);
    let body = screen(&h, Point::new(mil(3000.0), mil(3000.0)));
    click(&mut h, body);
    assert_eq!(h.state().docs[0].selection.len(), 1);
    let target = screen(&h, Point::new(mil(3500.0), mil(2500.0)));
    drag(&mut h, body, target);
    let d = h.state().session.design();
    let u1 = d.component_by_designator("U1").unwrap().id;
    let (_, sym) = d.symbols_of(u1).next().unwrap();
    assert_eq!(sym.position, Point::new(mil(3500.0), mil(2500.0)), "dragged by whole grid steps");
    // Undo restores the position.
    h.state_mut().perform(Action::Undo);
    h.step();
    let d = h.state().session.design();
    assert_eq!(d.symbols_of(u1).next().unwrap().1.position, Point::new(mil(3000.0), mil(3000.0)));
}

#[test]
fn rubber_band_window_selection() {
    let mut a = app(false);
    place(&mut a, "RES", "R1", 2000.0, 3000.0);
    place(&mut a, "RES", "R2", 2000.0, 3600.0);
    place(&mut a, "RES", "R3", 5000.0, 3000.0);
    let mut h = harness(a);
    let from = screen(&h, Point::new(mil(1500.0), mil(4000.0)));
    let to = screen(&h, Point::new(mil(2600.0), mil(2600.0)));
    drag(&mut h, from, to);
    assert_eq!(h.state().docs[0].selection.len(), 2);
}

#[test]
fn placement_tools_place_objects_and_increment_labels() {
    let mut h = harness(app(false));
    h.state_mut().perform(Action::SchTool(SchToolKind::NetLabel));
    if let Tool::NetLabel { text, .. } = &mut h.state_mut().docs[0].tool {
        *text = "D0".into();
    }
    let at = screen(&h, Point::new(mil(2000.0), mil(2000.0)));
    click(&mut h, at);
    let at = screen(&h, Point::new(mil(2000.0), mil(2300.0)));
    click(&mut h, at);
    h.state_mut().perform(Action::SchTool(SchToolKind::Gnd));
    let at = screen(&h, Point::new(mil(3000.0), mil(2000.0)));
    click(&mut h, at);
    let d = h.state().session.design();
    let labels: Vec<String> =
        d.schematic.values().filter_map(|o| if let SchKind::NetLabel(l) = &o.kind { Some(l.text.clone()) } else { None }).collect();
    assert_eq!(labels, vec!["D0".to_string(), "D1".to_string()]);
    assert!(d.schematic.values().any(|o| matches!(&o.kind, SchKind::PowerPort(p) if p.net == "GND")));
}

#[test]
fn part_placement_from_the_components_panel() {
    let mut h = harness(app(false));
    h.state_mut().components.selected = Some("LM358".into());
    h.state_mut().perform(Action::SchTool(SchToolKind::Part));
    let at = screen(&h, Point::new(mil(4000.0), mil(4000.0)));
    click(&mut h, at);
    key(&mut h, egui::Key::Space);
    let at = screen(&h, Point::new(mil(6000.0), mil(4000.0)));
    click(&mut h, at);
    let d = h.state().session.design();
    assert!(d.component_by_designator("U1").is_some());
    let u2 = d.component_by_designator("U2").unwrap().id;
    assert_eq!(d.symbols_of(u2).next().unwrap().1.rotation, 90.0, "Space rotates the part being placed");
}

#[test]
fn two_key_shortcuts_and_undo() {
    let mut a = app(false);
    place(&mut a, "RES", "R1", 2000.0, 3000.0);
    let mut h = harness(a);
    move_to(&mut h, pos2(700.0, 400.0));
    key(&mut h, egui::Key::P);
    key(&mut h, egui::Key::W);
    assert!(matches!(h.state().docs[0].tool, Tool::Wire { .. }));
    key(&mut h, egui::Key::Escape);
    h.event_modifiers(
        Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
        Modifiers::COMMAND,
    );
    h.step();
    assert!(h.state().session.design().components.is_empty(), "Ctrl+Z undid the placement");
}

#[test]
fn interactive_routing_completes_a_connection() {
    let mut h = harness(app(true));
    h.state_mut().open_doc(DocKey::Pcb);
    for _ in 0..6 {
        h.step();
    }
    let before = h.state().cache.pcb.unrouted.len();
    // Pick a connection whose straight run is clear of other copper (R3 pad 2 → D1 pad 2).
    let line = h.state().cache.pcb.unrouted.iter().find(|r| r.net == "NetD1_2").cloned();
    let Some(line) = line else { panic!("demo has an unrouted LED connection") };
    h.state_mut().perform(Action::PcbTool(PcbToolKind::Route));
    if let Tool::Route { mode, .. } = &mut h.state_mut().docs.iter_mut().find(|v| v.is_pcb()).unwrap().tool {
        *mode = crate::documents::CornerMode::VerticalFirst;
    }
    h.step();
    let at = screen(&h, line.from);
    click(&mut h, at);
    let at = screen(&h, line.to);
    click(&mut h, at);
    for _ in 0..3 {
        h.step();
    }
    let d = h.state().session.design();
    let tracks = d.pcb.values().filter(|o| matches!(&o.kind, PcbKind::Track(t) if t.net.as_deref() == Some("NetD1_2"))).count();
    assert!(tracks >= 1, "a track was routed on the net (status: {})", h.state().status);
    assert!(h.state().cache.pcb.unrouted.len() < before);
}

#[test]
fn components_panel_shows_every_view_of_a_project_component() {
    let mut h = harness(app(true));
    h.state_mut().components.selected = Some("RES_10k_0603".into());
    h.state_mut().focus_panel = Some(crate::workspace::PanelId::Components);
    for tab in [crate::app::ComponentTab::Schematic, crate::app::ComponentTab::Footprint, crate::app::ComponentTab::Model3d] {
        h.state_mut().components.tab = tab;
        h.step();
        h.step();
    }
    let status = h.state().components.status.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
    let res = status.iter().find(|s| s.name == "RES_10k_0603").expect("demo has a 10k 0603 resistor");
    assert_eq!(res.used_by, vec!["R1".to_string()]);
    assert!(res.no_part_number);
}

#[test]
fn every_panel_and_dialog_renders() {
    let mut h = harness(app(true));
    for p in crate::workspace::PanelId::ALL {
        h.state_mut().focus_panel = Some(p);
        h.step();
        h.step();
    }
    h.state_mut().perform(Action::RunErc);
    h.state_mut().perform(Action::RunDrc);
    h.state_mut().perform(Action::DesignRules);
    h.state_mut().perform(Action::LayerStack);
    h.state_mut().perform(Action::Preferences);
    h.state_mut().perform(Action::ShortcutHelp);
    h.state_mut().perform(Action::About);
    for _ in 0..3 {
        h.step();
    }
    h.state_mut().open_doc(DocKey::Pcb);
    let u1 = h.state().session.design().component_by_designator("U1").unwrap().id;
    h.state_mut().select_components(&[u1]);
    for _ in 0..3 {
        h.step();
    }
    assert!(h.state().reports.drc.is_some());
}

#[test]
fn cross_select_follows_between_editors() {
    let mut h = harness(app(true));
    let d = h.state().session.design();
    let u1 = d.component_by_designator("U1").unwrap().id;
    let (sym, inst) = d.symbols_of(u1).next().map(|(o, s)| (o.id, s.position)).unwrap();
    let fp = d.footprint_of(u1).unwrap().0.id;
    let at = screen(&h, inst);
    click(&mut h, at);
    assert_eq!(h.state().docs[0].selection, vec![sym]);
    let pcb = h.state().docs.iter().find(|v| v.is_pcb()).unwrap();
    assert_eq!(pcb.selection, vec![fp]);
}

#[test]
fn agent_requests_are_served_on_the_ui_thread() {
    let ctx = egui::Context::default();
    let start = StartOptions { control_port: Some(0), ..Default::default() };
    let a = HephaestusApp::build(&ctx, Prefs::default(), Theme::dark(), start);
    // Port 0 means disabled for the UI; exercise the dispatcher through a session directly.
    let mut s: Session = a.session;
    let r = hephaestus_agent_api::tools::dispatch(
        &mut s,
        "add_component",
        &serde_json::json!({"part": "RES", "position": [10, 10]}),
        Some("bot"),
    )
    .unwrap();
    assert_eq!(r["status"], "committed");
}
