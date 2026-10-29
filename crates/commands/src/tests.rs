use hephaestus_connectivity::{pcb as pcb_conn, schematic as sch_conn};
use hephaestus_core::geometry::{Point, mil};
use hephaestus_core::{Design, Layer, PcbKind, SchKind};

use crate::*;

fn p(x: f64, y: f64) -> Point {
    Point::new(mil(x), mil(y))
}

fn editor() -> Editor {
    Editor::new(Design::new("test"), vec![hephaestus_libraries::builtin()])
}

fn place(e: &mut Editor, part: &str, x: f64, y: f64) -> hephaestus_core::ObjectId {
    let out = e
        .execute(
            &Command::PlaceComponent {
                part: part.into(),
                sheet: None,
                position: p(x, y),
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
    out.created[0]
}

#[test]
fn placing_imports_the_part_and_assigns_designators() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    let r2 = place(&mut e, "RES", 2000.0, 1000.0);
    let d = e.design();
    assert_eq!(d.components[&r1].designator, "R1");
    assert_eq!(d.components[&r2].designator, "R2");
    // The generic family becomes one component per value and footprint.
    assert_eq!(d.components[&r1].part, "RES_10k_0603");
    assert!(!d.library.parts.contains_key("RES"));
    let part = &d.library.parts["RES_10k_0603"];
    assert_eq!((part.family.as_deref(), part.default_value.as_str()), (Some("RES"), "10k"));
    assert_eq!(part.origin.as_ref().map(|o| o.item.as_str()), Some("RES"));
    assert!(d.library.footprints.contains_key("0603"));
    assert!(d.integrity_problems().is_empty());
}

#[test]
fn changing_a_generic_value_or_footprint_repoints_and_prunes() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    let r2 = place(&mut e, "RES", 2000.0, 1000.0);
    let modify = |value: Option<&str>, footprint: Option<&str>, id| Command::ModifyComponent {
        component: ComponentRef::Id(id),
        designator: None,
        value: value.map(Into::into),
        footprint: footprint.map(Into::into),
        parameters: Default::default(),
    };
    e.execute(&modify(Some("4.7k"), None, r1), Source::User).unwrap();
    assert_eq!(e.design().components[&r1].part, "RES_4.7k_0603");
    // R2 still uses the 10k component, so it stays.
    assert!(e.design().library.parts.contains_key("RES_10k_0603"));
    e.execute(&modify(None, Some("0805"), r2), Source::User).unwrap();
    assert_eq!(e.design().components[&r2].part, "RES_10k_0805");
    assert!(!e.design().library.parts.contains_key("RES_10k_0603"), "unused auto-made component is pruned");
    // Undo brings the old component back.
    e.undo(Source::User).unwrap();
    assert_eq!(e.design().components[&r2].part, "RES_10k_0603");
    assert!(e.design().library.parts.contains_key("RES_10k_0603"));
}

#[test]
fn sourcing_edits_do_not_mark_a_component_modified() {
    let mut e = editor();
    place(&mut e, "RES", 1000.0, 1000.0);
    let approved =
        vec![hephaestus_core::ApprovedPart { manufacturer: "YAGEO".into(), mpn: "RC0603FR-0710KL".into(), ..Default::default() }];
    let update = |approved, description: Option<&str>| Command::UpdateLibraryComponent {
        part: "RES_10k_0603".into(),
        description: description.map(Into::into),
        manufacturer: None,
        mpn: None,
        parameters: Default::default(),
        approved,
    };
    e.execute(&update(Some(approved), None), Source::User).unwrap();
    let part = &e.design().library.parts["RES_10k_0603"];
    assert_eq!(part.sourcing(), Some(("YAGEO", "RC0603FR-0710KL")));
    assert!(!part.modified);
    e.execute(&update(None, Some("Precision resistor")), Source::User).unwrap();
    assert!(e.design().library.parts["RES_10k_0603"].modified);
    // A component with part numbers is kept when unused.
    let r1 = e.design().component_by_designator("R1").unwrap().id;
    e.execute(&Command::RemoveComponent { component: ComponentRef::Id(r1) }, Source::User).unwrap();
    assert!(e.design().library.parts.contains_key("RES_10k_0603"));
}

#[test]
fn assign_component_switches_parts_with_matching_pins() {
    let mut e = editor();
    let u1 = place(&mut e, "NE555", 3000.0, 3000.0);
    e.execute(&Command::AssignComponent { components: vec![ComponentRef::Id(u1)], part: "NE555P".into() }, Source::User).unwrap();
    let c = &e.design().components[&u1];
    assert_eq!((c.part.as_str(), c.footprint.as_str()), ("NE555P", "DIP-8"));
    let err = e.execute(&Command::AssignComponent { components: vec![ComponentRef::Id(u1)], part: "RES".into() }, Source::User);
    assert!(err.is_err(), "a resistor cannot replace a timer");
}

#[test]
fn imported_packs_rename_clashing_content() {
    let mut e = editor();
    place(&mut e, "NE555", 3000.0, 3000.0);
    let mut pack = e.design().library.extract(["NE555"]);
    if let Some(fp) = pack.footprints.get_mut("SOIC-8") {
        fp.description = "edited elsewhere".into();
    }
    if let Some(p) = pack.parts.get_mut("NE555") {
        p.description = "from another project".into();
    }
    e.execute(&Command::ImportLibrary { library: pack }, Source::User).unwrap();
    let lib = &e.design().library;
    assert_eq!(lib.parts["NE555_2"].footprints, vec!["SOIC-8_2".to_string()]);
    assert_eq!(lib.parts["NE555_2"].symbol, "NE555", "identical symbol is shared");
    assert_ne!(lib.footprints["SOIC-8"].description, "edited elsewhere", "existing footprint untouched");
    assert!(lib.check_part(&lib.parts["NE555_2"]).is_empty());
}

#[test]
fn undo_and_redo_restore_exact_state() {
    let mut e = editor();
    let empty = e.design().clone();
    place(&mut e, "NE555", 3000.0, 3000.0);
    let placed = e.design().clone();
    e.undo(Source::User).unwrap();
    // Ids are never reused, so only the counter differs.
    let mut expected = empty.clone();
    expected.next_id = e.design().next_id;
    assert_eq!(e.design(), &expected);
    e.redo(Source::User).unwrap();
    assert_eq!(e.design(), &placed);
    assert!(matches!(e.redo(Source::User), Err(CommandError::NothingToRedo)));
}

#[test]
fn invalid_commands_leave_the_design_untouched() {
    let mut e = editor();
    let before = e.design().clone();
    let err = e.execute(&Command::AddWire { sheet: None, points: vec![p(0.0, 0.0)] }, Source::User).unwrap_err();
    assert_eq!(err.code(), "invalid");
    let batch = Command::Batch {
        commands: vec![
            Command::AddJunction { sheet: None, position: p(0.0, 0.0) },
            Command::AddNetLabel { sheet: None, position: p(0.0, 0.0), text: "  ".into(), rotation: 0.0 },
        ],
    };
    assert!(matches!(e.execute(&batch, Source::User), Err(CommandError::Batch { index: 1, .. })));
    assert_eq!(e.design(), &before);
}

#[test]
fn connect_pins_with_wire_creates_a_net() {
    let mut e = editor();
    place(&mut e, "RES", 1000.0, 1000.0);
    place(&mut e, "RES", 2000.0, 1500.0);
    let cmd = Command::ConnectPins {
        from: PinRef { component: ComponentRef::Designator("R1".into()), pin: "2".into() },
        to: PinRef { component: ComponentRef::Designator("R2".into()), pin: "1".into() },
        net: None,
        style: ConnectStyle::Auto,
    };
    e.execute(&cmd, Source::Agent("test".into())).unwrap();
    let netlist = sch_conn::compile(e.design());
    let r1 = e.design().component_by_designator("R1").unwrap().id;
    let net = netlist.net_of_pin(r1, "2").unwrap();
    assert_eq!(net.nodes.len(), 2);
    assert!(e.design().schematic.values().any(|o| matches!(&o.kind, SchKind::Wire(w) if w.points.len() == 3)));
}

#[test]
fn create_net_labels_every_pin() {
    let mut e = editor();
    place(&mut e, "RES", 1000.0, 1000.0);
    place(&mut e, "CAP", 1000.0, 2000.0);
    place(&mut e, "NE555", 4000.0, 2000.0);
    let pins = vec![
        PinRef { component: ComponentRef::Designator("R1".into()), pin: "1".into() },
        PinRef { component: ComponentRef::Designator("C1".into()), pin: "2".into() },
        PinRef { component: ComponentRef::Designator("U1".into()), pin: "VCC".into() },
    ];
    e.execute(&Command::CreateNet { name: "VBUS".into(), pins }, Source::User).unwrap();
    let net = sch_conn::compile(e.design()).net("VBUS").cloned().unwrap();
    assert_eq!(net.nodes.len(), 3);
    assert!(net.named);
}

#[test]
fn move_with_drag_wires_keeps_connections() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    place(&mut e, "RES", 2000.0, 1000.0);
    e.execute(&Command::AddWire { sheet: None, points: vec![p(1200.0, 1000.0), p(1800.0, 1000.0)] }, Source::User).unwrap();
    let symbol = e.design().symbols_of(r1).next().unwrap().0.id;
    e.execute(&Command::MoveObjects { ids: vec![symbol], dx: 0, dy: mil(300.0), drag_wires: true }, Source::User).unwrap();
    let netlist = sch_conn::compile(e.design());
    assert_eq!(netlist.net_of_pin(r1, "2").unwrap().nodes.len(), 2);
}

#[test]
fn rotate_and_mirror_keep_wires_attached() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    place(&mut e, "RES", 2000.0, 1000.0);
    e.execute(&Command::AddWire { sheet: None, points: vec![p(1200.0, 1000.0), p(1800.0, 1000.0)] }, Source::User).unwrap();
    let symbol = e.design().symbols_of(r1).next().unwrap().0.id;
    e.execute(&Command::RotateObjects { ids: vec![symbol], degrees: 90.0, pivot: None }, Source::User).unwrap();
    assert_eq!(sch_conn::compile(e.design()).net_of_pin(r1, "2").unwrap().nodes.len(), 2);
    e.execute(&Command::MirrorObjects { ids: vec![symbol], pivot: None }, Source::User).unwrap();
    assert_eq!(sch_conn::compile(e.design()).net_of_pin(r1, "2").unwrap().nodes.len(), 2);
    // Every wire stays horizontal or vertical.
    for o in e.design().schematic.values() {
        if let SchKind::Wire(w) = &o.kind {
            assert!(w.points.windows(2).all(|s| s[0].x == s[1].x || s[0].y == s[1].y), "{:?}", w.points);
        }
    }
}

#[test]
fn moving_a_footprint_drags_its_tracks() {
    let mut e = editor();
    place(&mut e, "RES", 1000.0, 1000.0);
    place(&mut e, "RES", 2000.0, 1000.0);
    e.execute(&Command::AddWire { sheet: None, points: vec![p(1200.0, 1000.0), p(1800.0, 1000.0)] }, Source::User).unwrap();
    e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
    for (des, x) in [("R1", 1000.0), ("R2", 1500.0)] {
        let cmd =
            Command::PlaceFootprint { component: ComponentRef::Designator(des.into()), position: p(x, 1000.0), rotation: None, side: None };
        e.execute(&cmd, Source::User).unwrap();
    }
    e.execute(&Command::RouteNet { net: "NetR1_2".into(), layer: None, width: None }, Source::User).unwrap();
    assert!(pcb_conn::analyze(e.design()).unrouted.is_empty());
    let r1 = e.design().component_by_designator("R1").unwrap().id;
    let fp = e.design().footprint_of(r1).unwrap().0.id;
    e.execute(&Command::MoveObjects { ids: vec![fp], dx: 0, dy: mil(200.0), drag_wires: true }, Source::User).unwrap();
    assert!(pcb_conn::analyze(e.design()).unrouted.is_empty());
}

#[test]
fn update_pcb_creates_footprints_with_nets_and_route_net_completes_them() {
    let mut e = editor();
    place(&mut e, "RES", 1000.0, 1000.0);
    place(&mut e, "RES", 2000.0, 1000.0);
    e.execute(&Command::AddWire { sheet: None, points: vec![p(1200.0, 1000.0), p(1800.0, 1000.0)] }, Source::User).unwrap();
    let out = e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
    assert_eq!(out.created.len(), 2);
    let r1 = e.design().component_by_designator("R1").unwrap().id;
    let (_, fp) = e.design().footprint_of(r1).unwrap();
    assert_eq!(fp.pad_nets.get("2").map(String::as_str), Some("NetR1_2"));
    // Place them on the board and route.
    e.execute(
        &Command::PlaceFootprint {
            component: ComponentRef::Designator("R1".into()),
            position: p(1000.0, 1000.0),
            rotation: None,
            side: None,
        },
        Source::User,
    )
    .unwrap();
    e.execute(
        &Command::PlaceFootprint {
            component: ComponentRef::Designator("R2".into()),
            position: p(1500.0, 1200.0),
            rotation: None,
            side: None,
        },
        Source::User,
    )
    .unwrap();
    assert_eq!(pcb_conn::analyze(e.design()).unrouted.len(), 1);
    e.execute(&Command::RouteNet { net: "NetR1_2".into(), layer: None, width: None }, Source::User).unwrap();
    let conn = pcb_conn::analyze(e.design());
    assert!(conn.unrouted.is_empty());
    assert!(conn.shorts.is_empty());
    assert!(e.design().pcb.values().any(|o| matches!(&o.kind, PcbKind::Track(t) if t.layer == Layer::TopCopper)));
}

#[test]
fn changing_footprint_preserves_identity_and_placement() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
    let before = e.design().footprint_of(r1).map(|(o, f)| (o.id, f.position)).unwrap();
    e.execute(
        &Command::ModifyComponent {
            component: ComponentRef::Id(r1),
            designator: None,
            value: None,
            footprint: Some("0805".into()),
            parameters: Default::default(),
        },
        Source::User,
    )
    .unwrap();
    let (obj, fp) = e.design().footprint_of(r1).unwrap();
    assert_eq!((obj.id, fp.position), before);
    assert_eq!(fp.footprint, "0805");
}

#[test]
fn locked_footprints_refuse_to_move() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
    let fp = e.design().footprint_of(r1).unwrap().0.id;
    e.execute(&Command::LockObjects { ids: vec![fp], locked: true }, Source::User).unwrap();
    let err = e.execute(&Command::MoveObjects { ids: vec![fp], dx: 100, dy: 0, drag_wires: false }, Source::User).unwrap_err();
    assert_eq!(err, CommandError::Locked(fp));
}

#[test]
fn transactions_undo_as_one_step_and_roll_back() {
    let mut e = editor();
    e.begin("Agent work", Source::Agent("a".into())).unwrap();
    place(&mut e, "RES", 1000.0, 1000.0);
    place(&mut e, "CAP", 2000.0, 1000.0);
    assert!(!e.can_undo());
    e.commit().unwrap();
    assert_eq!(e.history().len(), 1);
    e.undo(Source::User).unwrap();
    assert!(e.design().components.is_empty());

    e.begin("Discarded", Source::User).unwrap();
    place(&mut e, "RES", 1000.0, 1000.0);
    e.rollback().unwrap();
    assert!(e.design().components.is_empty());
}

#[test]
fn proposals_apply_only_when_accepted() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    let id = e.propose(Command::RemoveComponent { component: ComponentRef::Id(r1) }, Source::Agent("bot".into())).unwrap();
    assert!(e.design().components.contains_key(&r1));
    assert_eq!(e.proposals().count(), 1);
    e.accept(id).unwrap();
    assert!(!e.design().components.contains_key(&r1));
    assert!(matches!(e.accept(id), Err(CommandError::UnknownProposal(_))));
}

#[test]
fn deleting_the_last_symbol_removes_component_and_footprint() {
    let mut e = editor();
    let r1 = place(&mut e, "RES", 1000.0, 1000.0);
    e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
    let symbol = e.design().symbols_of(r1).next().unwrap().0.id;
    e.execute(&Command::DeleteObjects { ids: vec![symbol] }, Source::User).unwrap();
    assert!(e.design().components.is_empty());
    assert!(e.design().pcb.is_empty());
    assert!(e.design().integrity_problems().is_empty());
}

#[test]
fn log_reports_changes_since_a_sequence_number() {
    let mut e = editor();
    place(&mut e, "RES", 1000.0, 1000.0);
    let seen = e.seq();
    place(&mut e, "CAP", 1000.0, 2000.0);
    e.undo(Source::User).unwrap();
    let log = e.log_since(seen);
    assert_eq!(log.len(), 2);
    assert_eq!(log[1].kind, LogKind::Undo);
}

#[test]
fn commands_deserialize_from_agent_json() {
    let json = r#"{"command":"connect_pins","from":{"component":"U1","pin":"8"},"to":{"component":3,"pin":"1"}}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    assert!(matches!(
        cmd,
        Command::ConnectPins {
            from: PinRef { component: ComponentRef::Designator(_), .. },
            to: PinRef { component: ComponentRef::Id(_), .. },
            ..
        }
    ));
    let bad = r#"{"command":"add_junction","position":{"x":0,"y":0},"bogus":1}"#;
    assert!(serde_json::from_str::<Command>(bad).is_err());
}

#[test]
fn layer_stack_change_refuses_to_orphan_tracks() {
    let mut e = editor();
    e.execute(&Command::SetLayerStack { copper_layers: 4 }, Source::User).unwrap();
    e.execute(&Command::AddTrack { start: p(0.0, 0.0), end: p(100.0, 0.0), layer: Layer::Inner(1), width: None, net: None }, Source::User)
        .unwrap();
    assert!(e.execute(&Command::SetLayerStack { copper_layers: 2 }, Source::User).is_err());
}
