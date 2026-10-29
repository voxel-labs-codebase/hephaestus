use serde_json::json;

use super::*;

fn call(s: &mut Session, tool: &str, args: Value) -> Value {
    dispatch(s, tool, &args, Some("tester")).unwrap_or_else(|e| panic!("{tool}: {e}"))
}

#[test]
fn every_tool_in_the_specification_exists() {
    let names: Vec<&str> = catalog().iter().map(|t| t.name).collect();
    for required in [
        "get_project_info",
        "list_documents",
        "inspect_document",
        "save_project",
        "get_schematic",
        "get_components",
        "get_nets",
        "add_component",
        "remove_component",
        "connect_pins",
        "create_net",
        "move_symbol",
        "modify_component_properties",
        "get_pcb",
        "get_footprints",
        "get_tracks",
        "get_vias",
        "place_component",
        "move_component",
        "rotate_component",
        "route_net",
        "create_via",
        "modify_track",
        "create_copper_zone",
        "run_erc",
        "run_drc",
        "get_violations",
        "inspect_violation",
        "export_gerber",
        "export_drill_files",
        "export_bom",
        "export_pick_and_place",
    ] {
        assert!(names.contains(&required), "missing {required}");
    }
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "duplicate tool names");
}

#[test]
fn agent_builds_and_validates_a_circuit() {
    let mut s = Session::new("agent");
    let r = call(&mut s, "add_component", json!({"part": "RES", "position": {"x": "1000mil", "y": "1000mil"}}));
    assert_eq!(r["created"][0]["designator"], "R1");
    call(&mut s, "add_component", json!({"part": "LED", "position": ["2000mil", "1000mil"]}));
    let c = call(&mut s, "connect_pins", json!({"from": {"component": "R1", "pin": "2"}, "to": {"component": "D1", "pin": "A"}}));
    assert_eq!(c["status"], "committed");
    let nets = call(&mut s, "get_nets", json!({}));
    assert_eq!(nets["nets"][0]["pins"], json!(["D1.2", "R1.2"]));

    call(&mut s, "update_pcb", json!({}));
    call(&mut s, "place_component", json!({"component": "R1", "position": [10, 10]}));
    call(&mut s, "place_component", json!({"component": "D1", "position": [20, 10], "rotation": 180}));
    let pcb = call(&mut s, "get_pcb", json!({}));
    assert_eq!(pcb["routing"]["unrouted"], 1);
    call(&mut s, "route_net", json!({"net": "NetD1_2"}));
    assert_eq!(call(&mut s, "get_pcb", json!({}))["routing"]["completion_percent"], 100.0);
    let drc = call(&mut s, "run_drc", json!({}));
    assert_eq!(drc["errors"], 0, "{drc:#}");
    let erc = call(&mut s, "run_erc", json!({}));
    assert!(erc["warnings"].as_u64().unwrap() > 0);
    let detail = call(&mut s, "inspect_violation", json!({"check": "erc", "index": 0}));
    assert!(detail["objects"].as_array().is_some_and(|o| !o.is_empty()));
}

#[test]
fn dry_run_and_proposals_do_not_change_the_design() {
    let mut s = Session::new("agent");
    call(&mut s, "add_component", json!({"part": "RES", "position": [25.4, 25.4]}));
    let rev = s.editor.revision();
    let dry = call(&mut s, "add_component", json!({"part": "CAP", "position": [50.8, 25.4], "dry_run": true}));
    assert_eq!(dry["status"], "dry_run");
    assert_eq!(s.editor.revision(), rev);
    // Deletions by agents need approval.
    let pending = call(&mut s, "remove_component", json!({"component": "R1"}));
    assert_eq!(pending["status"], "pending_approval");
    assert_eq!(s.design().components.len(), 1);
    // The application user can delete directly.
    let direct = dispatch(&mut s, "remove_component", &json!({"component": "R1"}), None).unwrap();
    assert_eq!(direct["status"], "committed");
}

#[test]
fn structured_errors() {
    let mut s = Session::new("agent");
    let e = dispatch(&mut s, "add_component", &json!({"part": "NOPE", "position": [0, 0]}), Some("a")).unwrap_err();
    assert_eq!(e.code, "invalid");
    let e = dispatch(&mut s, "add_component", &json!({"part": "RES"}), Some("a")).unwrap_err();
    assert_eq!(e.code, "invalid_argument");
    let e = dispatch(&mut s, "move_component", &json!({"component": "R9", "offset": [1, 1]}), Some("a")).unwrap_err();
    assert_eq!(e.code, "invalid");
}

#[test]
fn require_clean_blocks_new_drc_errors() {
    let mut s = crate::demo::flasher().unwrap();
    let e = dispatch(&mut s, "create_via", &json!({"position": [-5, -5], "net": "GND", "require_clean": true}), Some("a")).unwrap_err();
    assert_eq!(e.code, "validation_failed");
}

#[test]
fn agents_see_user_changes() {
    let mut s = Session::new("agent");
    let seq = call(&mut s, "get_changes", json!({}))["seq"].as_u64().unwrap();
    dispatch(&mut s, "add_component", &json!({"part": "RES", "position": [0, 0]}), None).unwrap();
    let changes = call(&mut s, "get_changes", json!({"since": seq}));
    assert_eq!(changes["changes"][0]["source"]["kind"], "user");
}

#[test]
fn exports_write_files() {
    let mut s = crate::demo::flasher().unwrap();
    let dir = std::env::temp_dir().join(format!("hephaestus-agent-export-{}", std::process::id()));
    let r = call(&mut s, "export_gerber", json!({"directory": dir.to_string_lossy()}));
    assert!(r["files"].as_array().unwrap().len() >= 9);
    let bom = call(&mut s, "export_bom", json!({}));
    assert!(bom["rows"].as_array().unwrap().len() >= 6);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn agents_manage_the_project_component_library() {
    let mut s = Session::new("agent");
    call(&mut s, "add_component", json!({"part": "RES", "value": "4.7k", "position": ["1000mil", "1000mil"]}));
    call(&mut s, "add_component", json!({"part": "NE555", "position": ["3000mil", "3000mil"]}));
    let list = call(&mut s, "list_library_components", json!({}));
    let res = list.as_array().unwrap().iter().find(|c| c["part"] == "RES_4.7k_0603").cloned().unwrap();
    assert_eq!(res["title"], "R 4.7k 0603");
    assert_eq!(res["used_by"], json!(["R1"]));
    assert!(res["badges"].as_array().unwrap().contains(&json!("no part number")));

    call(
        &mut s,
        "update_library_component",
        json!({"part": "RES_4.7k_0603", "approved": [{"manufacturer": "YAGEO", "mpn": "RC0603FR-074K7L", "suppliers": {"lcsc": "C23162"}, "jlc_basic": true}]}),
    );
    let check = call(&mut s, "check_library", json!({}));
    assert!(
        !check["needs_attention"].as_array().unwrap().iter().any(|c| c["name"] == "RES_4.7k_0603" && c["no_part_number"] == true),
        "{check}"
    );
    let bom = call(&mut s, "export_bom", json!({}));
    assert!(bom["rows"].as_array().unwrap().iter().any(|r| r["Manufacturer Part Number"] == "RC0603FR-074K7L"));

    let dir = std::env::temp_dir().join(format!("hephaestus-pack-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("res.pcbcomp.json");
    let out = call(&mut s, "export_library_components", json!({"parts": ["RES_4.7k_0603"], "path": path}));
    assert_eq!(out["components"], 1);
    let mut other = Session::new("other");
    call(&mut other, "import_library_components", json!({"path": path}));
    assert_eq!(other.design().library.parts["RES_4.7k_0603"].approved[0].mpn, "RC0603FR-074K7L");
    std::fs::remove_dir_all(dir).unwrap();

    call(&mut s, "assign_component", json!({"components": ["U1"], "part": "NE555P"}));
    assert_eq!(s.design().component_by_designator("U1").unwrap().footprint, "DIP-8");
}
