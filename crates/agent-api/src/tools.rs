//! The agent tool catalog and dispatcher.
//!
//! Every tool takes a JSON object and returns JSON. Lengths are millimetres (numbers) or strings
//! with units; coordinates in results are millimetres. Mutating tools accept:
//! - `dry_run`: plan and validate, report the edits and new ERC/DRC findings, change nothing;
//! - `propose`: park the change for a person to accept in the application;
//! - `require_clean`: refuse the change if it introduces new ERC or DRC errors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use hephaestus_commands::{AlignMode, Axis, Command, ComponentRef, ConnectStyle, PinRef, Source};
use hephaestus_connectivity::{pcb as pcb_conn, schematic as sch_conn};
use hephaestus_core::geometry::Point;
use hephaestus_core::library_check::check_library;
use hephaestus_core::schematic::{PortDirection, PowerStyle};
use hephaestus_core::{Component, Design, Layer, ObjectId, ObjectRef, PcbKind, PcbObject, SchKind, SchObject, Side, SymbolGraphic};
use hephaestus_rules::{Report, Violation, drc, erc};
use serde_json::{Value, json};

use crate::units::{mm, opt_length, point, points, pt};
use crate::{ApiError, Session};

/// Description of one tool for MCP `tools/list` and the control protocol's `list_tools`.
#[derive(Clone, Debug)]
pub struct ToolInfo {
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub mutates: bool,
    pub schema: Value,
}

fn len(desc: &str) -> Value {
    json!({"type": ["number", "string"], "description": format!("{desc} (mm, or a string with units such as \"100mil\")")})
}

fn pnt(desc: &str) -> Value {
    json!({"description": format!("{desc}: {{\"x\": mm, \"y\": mm}} or [x, y]"), "oneOf": [
        {"type": "object", "properties": {"x": {"type": ["number", "string"]}, "y": {"type": ["number", "string"]}}, "required": ["x", "y"]},
        {"type": "array", "minItems": 2, "maxItems": 2}
    ]})
}

fn pnts(desc: &str) -> Value {
    json!({"type": "array", "description": format!("{desc}: array of points"), "items": pnt("point")})
}

fn s(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn n(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}

fn b(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}

fn comp() -> Value {
    json!({"type": ["string", "integer"], "description": "Component designator (\"R1\") or object id"})
}

fn pin_ref() -> Value {
    json!({"type": "object", "properties": {"component": comp(), "pin": s("Pin number or unique pin name")}, "required": ["component", "pin"]})
}

fn obj(props: Vec<(&str, Value)>, required: &[&str], mutates: bool) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in props {
        map.insert(k.to_string(), v);
    }
    if mutates {
        map.insert("dry_run".into(), b("Plan and validate only; nothing changes"));
        map.insert("propose".into(), b("Create a proposal for a person to accept instead of applying"));
        map.insert("require_clean".into(), b("Refuse if the change introduces new ERC/DRC errors"));
    }
    json!({"type": "object", "properties": map, "required": required, "additionalProperties": false})
}

macro_rules! tool {
    ($name:literal, $cat:literal, $mutates:expr, $desc:literal, $props:expr, $req:expr) => {
        ToolInfo { name: $name, category: $cat, description: $desc, mutates: $mutates, schema: obj($props, $req, $mutates) }
    };
}

pub fn catalog() -> Vec<ToolInfo> {
    vec![
        // Project
        tool!(
            "get_project_info",
            "project",
            false,
            "Project name, location, documents, counts, revision, dirty state, open transaction and pending proposals.",
            vec![],
            &[]
        ),
        tool!("list_documents", "project", false, "Schematic sheets, the PCB document and the project library.", vec![], &[]),
        tool!(
            "inspect_document",
            "project",
            false,
            "Contents of one document: a sheet id, \"pcb\" or \"library\".",
            vec![("document", json!({"type": ["string", "integer"], "description": "Sheet id, \"pcb\" or \"library\""}))],
            &["document"]
        ),
        tool!(
            "save_project",
            "project",
            false,
            "Save the project. Pass folder to save a new project (or save as).",
            vec![("folder", s("Project folder (absolute)"))],
            &[]
        ),
        tool!(
            "open_project",
            "project",
            false,
            "Open a project manifest (*.pcbprj) or bundle (*.hephaestus.json). Unsaved changes are discarded.",
            vec![("path", s("Absolute path"))],
            &["path"]
        ),
        tool!(
            "get_changes",
            "project",
            false,
            "Change log entries after a sequence number, including manual edits by the user, undo and redo.",
            vec![("since", n("Sequence number from a previous call (0 for all)"))],
            &[]
        ),
        tool!("undo", "project", false, "Undo the last change.", vec![], &[]),
        tool!("redo", "project", false, "Redo the last undone change.", vec![], &[]),
        tool!(
            "begin_transaction",
            "project",
            false,
            "Group the following changes into one undo step that can be rolled back.",
            vec![("name", s("Transaction name"))],
            &["name"]
        ),
        tool!("commit_transaction", "project", false, "Commit the open transaction.", vec![], &[]),
        tool!("rollback_transaction", "project", false, "Discard every change of the open transaction.", vec![], &[]),
        tool!("list_proposals", "project", false, "Pending proposals awaiting approval in the application.", vec![], &[]),
        tool!(
            "execute_command",
            "project",
            true,
            "Run a raw core command (see AGENT_API.md; coordinates in nanometres).",
            vec![("command", json!({"type": "object", "description": "A command object with a \"command\" tag"}))],
            &["command"]
        ),
        // Library
        tool!(
            "search_library",
            "library",
            false,
            "Search available parts by keywords (all terms must match).",
            vec![("query", s("Keywords, e.g. \"resistor 0603\"")), ("category", s("Optional category prefix"))],
            &[]
        ),
        tool!(
            "get_part",
            "library",
            false,
            "Full library metadata of a part: symbol pins, footprints and pads, parameters.",
            vec![("part", s("Part name, e.g. NE555"))],
            &["part"]
        ),
        tool!(
            "list_library_components",
            "library",
            false,
            "Components in the project library (one per value and footprint for generic parts) with part numbers, users and status badges (update available, modified locally, no part number, no 3D model, unused).",
            vec![],
            &[]
        ),
        tool!(
            "check_library",
            "library",
            false,
            "Project library components that need attention: broken references, updates available from their origin, local edits, missing part numbers or 3D models, unused.",
            vec![],
            &[]
        ),
        tool!(
            "update_library_component",
            "library",
            true,
            "Set a project library component's description, manufacturer part number, parameters or approved manufacturer parts (preferred first; for a generic component these are the parts that may be bought for it).",
            vec![
                ("part", s("Project library component, e.g. RES_10k_0603")),
                ("description", s("New description")),
                ("manufacturer", s("Manufacturer of a specific part")),
                ("mpn", s("Manufacturer part number of a specific part")),
                (
                    "parameters",
                    json!({"type": "object", "description": "Parameters to set; null removes one", "additionalProperties": {"type": ["string", "null"]}})
                ),
                (
                    "approved",
                    json!({"type": "array", "description": "Replaces the approved parts", "items": {"type": "object", "properties": {
                    "manufacturer": s("Manufacturer"), "mpn": s("Manufacturer part number"),
                    "suppliers": json!({"type": "object", "description": "Supplier → supplier part number (digikey, mouser, lcsc)", "additionalProperties": {"type": "string"}}),
                    "jlc_basic": b("JLCPCB Basic part"), "checked": s("Date checked, YYYY-MM-DD")
                }, "required": ["mpn"]}})
                ),
            ],
            &["part"]
        ),
        tool!(
            "assign_component",
            "library",
            true,
            "Use another library component for placed components (pin numbers must match). Generic families keep each component's value.",
            vec![
                ("components", json!({"type": "array", "items": comp(), "description": "Designators or ids"})),
                ("part", s("Library component or family"))
            ],
            &["components", "part"]
        ),
        tool!(
            "export_library_components",
            "library",
            false,
            "Write project library components, with their symbols and footprints, to a component pack file (*.pcbcomp.json).",
            vec![
                ("parts", json!({"type": "array", "items": {"type": "string"}, "description": "Components to export; omit for all"})),
                ("path", s("Pack file path"))
            ],
            &["path"]
        ),
        tool!(
            "import_library_components",
            "library",
            true,
            "Add the components of a component pack (*.pcbcomp.json) to the project library; identical symbols and footprints are shared, clashing names get a suffix.",
            vec![("path", s("Pack file path"))],
            &["path"]
        ),
        // Schematic
        tool!(
            "get_schematic",
            "schematic",
            false,
            "Objects on a sheet (or all sheets) with positions in mm.",
            vec![("sheet", n("Sheet id; omit for all sheets"))],
            &[]
        ),
        tool!(
            "get_components",
            "schematic",
            false,
            "Components with designator, value, part, footprint, parameters, schematic and PCB placement.",
            vec![("filter", s("Optional designator/value/part substring"))],
            &[]
        ),
        tool!(
            "get_nets",
            "schematic",
            false,
            "Compiled netlist: every net with its pins, plus PCB routing status.",
            vec![("net", s("Optional net name"))],
            &[]
        ),
        tool!(
            "add_component",
            "schematic",
            true,
            "Place a part on a schematic sheet. Positions snap to the 100 mil grid unless snap is false.",
            vec![
                ("part", s("Part name from search_library")),
                ("sheet", n("Sheet id (default first sheet)")),
                ("position", pnt("Symbol origin")),
                ("rotation", n("0, 90, 180 or 270")),
                ("mirror", b("Mirror horizontally")),
                ("designator", s("Designator (default next free)")),
                ("value", s("Value/comment")),
                ("footprint", s("Footprint (default the part's first)")),
                ("parameters", json!({"type": "object"})),
                ("snap", b("Snap to 100 mil grid (default true)"))
            ],
            &["part", "position"]
        ),
        tool!(
            "remove_component",
            "schematic",
            true,
            "Remove a component, its symbols and footprint.",
            vec![("component", comp())],
            &["component"]
        ),
        tool!(
            "connect_pins",
            "schematic",
            true,
            "Connect two pins with a wire (when a clean route exists) or matching net labels.",
            vec![
                ("from", pin_ref()),
                ("to", pin_ref()),
                ("net", s("Net name for labels")),
                ("style", json!({"type": "string", "enum": ["auto", "wire", "label"]}))
            ],
            &["from", "to"]
        ),
        tool!(
            "create_net",
            "schematic",
            true,
            "Put pins on a named net using net-label stubs.",
            vec![("name", s("Net name")), ("pins", json!({"type": "array", "items": pin_ref()}))],
            &["name", "pins"]
        ),
        tool!(
            "move_symbol",
            "schematic",
            true,
            "Move a component's schematic symbol to an absolute position or by an offset; attached wire ends follow.",
            vec![
                ("component", comp()),
                ("position", pnt("New position")),
                ("offset", pnt("Relative move")),
                ("drag_wires", b("Default true"))
            ],
            &["component"]
        ),
        tool!(
            "modify_component_properties",
            "schematic",
            true,
            "Change designator, value, footprint or parameters (null removes a parameter). A footprint change keeps the PCB placement and nets.",
            vec![
                ("component", comp()),
                ("designator", s("")),
                ("value", s("")),
                ("footprint", s("")),
                ("parameters", json!({"type": "object"}))
            ],
            &["component"]
        ),
        tool!(
            "add_wire",
            "schematic",
            true,
            "Draw a wire through points.",
            vec![("sheet", n("Sheet id")), ("points", pnts("Vertices"))],
            &["points"]
        ),
        tool!(
            "add_net_label",
            "schematic",
            true,
            "Place a net label touching a wire or pin.",
            vec![("sheet", n("Sheet id")), ("position", pnt("Hot spot")), ("text", s("Net name")), ("rotation", n("0 or 90"))],
            &["position", "text"]
        ),
        tool!(
            "add_power_port",
            "schematic",
            true,
            "Place a power port (VCC, GND …) at a pin or wire end.",
            vec![
                ("sheet", n("Sheet id")),
                ("position", pnt("Hot spot")),
                ("net", s("Net name")),
                ("style", json!({"type": "string", "enum": ["Bar", "Arrow", "Circle", "Wave", "PowerGround", "SignalGround", "Earth"]})),
                ("rotation", n("Degrees"))
            ],
            &["position", "net"]
        ),
        tool!(
            "add_port",
            "schematic",
            true,
            "Place a port (connects to equally named ports on other sheets).",
            vec![
                ("sheet", n("Sheet id")),
                ("position", pnt("Hot spot")),
                ("name", s("Port name")),
                ("direction", json!({"type": "string", "enum": ["Unspecified", "Input", "Output", "Bidirectional"]}))
            ],
            &["position", "name"]
        ),
        tool!(
            "add_no_connect",
            "schematic",
            true,
            "Mark a pin as intentionally unconnected.",
            vec![("sheet", n("Sheet id")), ("position", pnt("Pin hot spot"))],
            &["position"]
        ),
        tool!(
            "add_junction",
            "schematic",
            true,
            "Place a junction (connects crossing wires).",
            vec![("sheet", n("Sheet id")), ("position", pnt("Position"))],
            &["position"]
        ),
        tool!(
            "add_sheet",
            "schematic",
            true,
            "Add a schematic sheet.",
            vec![
                ("name", s("Sheet name")),
                ("size", json!({"type": "string", "enum": hephaestus_core::SheetSize::ALL.map(|z| z.label())}))
            ],
            &["name"]
        ),
        tool!(
            "delete_objects",
            "schematic",
            true,
            "Delete schematic or PCB objects by id.",
            vec![("ids", json!({"type": "array", "items": {"type": "integer"}}))],
            &["ids"]
        ),
        // PCB
        tool!("get_pcb", "pcb", false, "Board outline, layer stack, design rules, object counts and routing completion.", vec![], &[]),
        tool!("get_footprints", "pcb", false, "Placed footprints with pads, pad nets and positions.", vec![("component", comp())], &[]),
        tool!(
            "get_tracks",
            "pcb",
            false,
            "Tracks, optionally filtered by net or layer.",
            vec![("net", s("Net")), ("layer", s("Layer name, e.g. \"Top Layer\""))],
            &[]
        ),
        tool!("get_vias", "pcb", false, "Vias, optionally filtered by net.", vec![("net", s("Net"))], &[]),
        tool!(
            "update_pcb",
            "pcb",
            true,
            "Design › Update PCB Document: create missing footprints, apply footprint changes and assign pad nets from the schematic.",
            vec![],
            &[]
        ),
        tool!(
            "place_component",
            "pcb",
            true,
            "Place (or create) a component's footprint at a board position.",
            vec![
                ("component", comp()),
                ("position", pnt("Footprint origin")),
                ("rotation", n("Degrees")),
                ("side", json!({"type": "string", "enum": ["Top", "Bottom"]}))
            ],
            &["component", "position"]
        ),
        tool!(
            "move_component",
            "pcb",
            true,
            "Move a footprint to a position or by an offset.",
            vec![("component", comp()), ("position", pnt("New position")), ("offset", pnt("Relative move"))],
            &["component"]
        ),
        tool!(
            "rotate_component",
            "pcb",
            true,
            "Rotate a footprint to an absolute angle or by a relative angle.",
            vec![("component", comp()), ("rotation", n("Absolute degrees")), ("by", n("Relative degrees"))],
            &["component"]
        ),
        tool!(
            "route_net",
            "pcb",
            true,
            "Route a net: along given points as one track run, or every unrouted connection with direct 45° tracks (no obstacle avoidance — check DRC).",
            vec![
                ("net", s("Net name")),
                ("layer", s("Copper layer")),
                ("width", len("Track width")),
                ("points", pnts("Optional explicit path"))
            ],
            &["net"]
        ),
        tool!(
            "create_via",
            "pcb",
            true,
            "Place a via.",
            vec![
                ("position", pnt("Centre")),
                ("net", s("Net")),
                ("diameter", len("Pad diameter")),
                ("drill", len("Hole")),
                ("from", s("Start layer")),
                ("to", s("End layer"))
            ],
            &["position"]
        ),
        tool!(
            "modify_track",
            "pcb",
            true,
            "Change a track's endpoints, width, layer or net.",
            vec![
                ("track", n("Track id")),
                ("start", pnt("Start")),
                ("end", pnt("End")),
                ("width", len("Width")),
                ("layer", s("Layer")),
                ("net", s("Net (empty for none)"))
            ],
            &["track"]
        ),
        tool!(
            "create_copper_zone",
            "pcb",
            true,
            "Create a polygon pour outline on a copper layer (zone filling is not computed yet).",
            vec![("layer", s("Copper layer")), ("net", s("Net")), ("outline", pnts("Outline vertices")), ("priority", n("Pour priority"))],
            &["layer", "outline"]
        ),
        tool!(
            "set_board_outline",
            "pcb",
            true,
            "Redefine the board shape as a polygon.",
            vec![("outline", pnts("Vertices, counter-clockwise"))],
            &["outline"]
        ),
        tool!(
            "align_components",
            "pcb",
            true,
            "Align objects or components.",
            vec![
                ("ids", json!({"type": "array", "items": {"type": ["integer", "string"]}})),
                ("mode", json!({"type": "string", "enum": ["left", "right", "top", "bottom", "center_horizontal", "center_vertical"]}))
            ],
            &["ids", "mode"]
        ),
        tool!(
            "distribute_components",
            "pcb",
            true,
            "Distribute objects evenly.",
            vec![
                ("ids", json!({"type": "array", "items": {"type": ["integer", "string"]}})),
                ("axis", json!({"type": "string", "enum": ["horizontal", "vertical"]}))
            ],
            &["ids", "axis"]
        ),
        // Validation
        tool!("run_erc", "validation", false, "Electrical rule check on the compiled schematic.", vec![], &[]),
        tool!("run_drc", "validation", false, "Design rule check on the board.", vec![], &[]),
        tool!(
            "get_violations",
            "validation",
            false,
            "Violations from the latest ERC/DRC (run again if the design changed).",
            vec![
                ("check", json!({"type": "string", "enum": ["erc", "drc", "all"]})),
                ("severity", json!({"type": "string", "enum": ["error", "warning"]}))
            ],
            &[]
        ),
        tool!(
            "inspect_violation",
            "validation",
            false,
            "One violation with details of the objects involved.",
            vec![("check", json!({"type": "string", "enum": ["erc", "drc"]})), ("index", n("Index from get_violations"))],
            &["check", "index"]
        ),
        // Manufacturing
        tool!(
            "export_gerber",
            "manufacturing",
            false,
            "Write Gerber X2 files (copper, overlay, mask, paste, profile).",
            vec![("directory", s("Output folder (relative to the project folder or absolute)"))],
            &["directory"]
        ),
        tool!(
            "export_drill_files",
            "manufacturing",
            false,
            "Write Excellon drill files (PTH and NPTH).",
            vec![("directory", s("Output folder"))],
            &["directory"]
        ),
        tool!(
            "export_bom",
            "manufacturing",
            false,
            "Write the bill of materials CSV (also returned inline).",
            vec![("directory", s("Output folder (omit to only return rows)"))],
            &[]
        ),
        tool!(
            "export_pick_and_place",
            "manufacturing",
            false,
            "Write the pick-and-place CSV.",
            vec![("directory", s("Output folder"))],
            &["directory"]
        ),
    ]
}

// --- helpers ------------------------------------------------------------------------------------

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    str_arg(args, key).ok_or_else(|| ApiError::invalid(format!("missing string {key}")))
}

fn opt_f64(args: &Value, key: &str) -> Result<Option<f64>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|f| f.is_finite()).map(Some).ok_or_else(|| ApiError::invalid(format!("{key} must be a number"))),
    }
}

fn opt_id(args: &Value, key: &str) -> Result<Option<ObjectId>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(|id| Some(ObjectId(id))).ok_or_else(|| ApiError::invalid(format!("{key} must be an object id"))),
    }
}

fn component_ref(v: Option<&Value>) -> Result<ComponentRef, ApiError> {
    match v {
        Some(Value::String(s)) => Ok(ComponentRef::Designator(s.clone())),
        Some(Value::Number(n)) => {
            n.as_u64().map(|id| ComponentRef::Id(ObjectId(id))).ok_or_else(|| ApiError::invalid("component id must be a positive integer"))
        }
        _ => Err(ApiError::invalid("component must be a designator or id")),
    }
}

fn resolve_component(design: &Design, r: &ComponentRef) -> Result<ObjectId, ApiError> {
    let planner = hephaestus_commands::Planner::new(design, &[]);
    planner.component(r).map_err(ApiError::from)
}

fn pin_ref_arg(v: Option<&Value>) -> Result<PinRef, ApiError> {
    let v = v.ok_or_else(|| ApiError::invalid("missing pin reference"))?;
    let pin = match v.get("pin") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err(ApiError::invalid("pin must be a pin number or name")),
    };
    Ok(PinRef { component: component_ref(v.get("component"))?, pin })
}

fn layer_arg(args: &Value, key: &str) -> Result<Option<Layer>, ApiError> {
    match str_arg(args, key) {
        None => Ok(None),
        Some(name) => name.parse::<Layer>().map(Some).map_err(ApiError::invalid),
    }
}

fn ids_arg(design: &Design, args: &Value) -> Result<Vec<ObjectId>, ApiError> {
    let Some(Value::Array(items)) = args.get("ids") else { return Err(ApiError::invalid("ids must be an array")) };
    items
        .iter()
        .map(|v| match v {
            Value::Number(n) => n.as_u64().map(ObjectId).ok_or_else(|| ApiError::invalid("ids must be positive integers")),
            Value::String(d) => resolve_component(design, &ComponentRef::Designator(d.clone())),
            _ => Err(ApiError::invalid("ids must be object ids or designators")),
        })
        .collect()
}

fn snap_point(p: Point, grid: i64) -> Point {
    p.snapped(grid)
}

// --- serialization of design objects --------------------------------------------------------------

fn designator(design: &Design, component: Option<ObjectId>) -> String {
    component.and_then(|c| design.components.get(&c)).map(|c| c.designator.clone()).unwrap_or_default()
}

pub fn component_json(design: &Design, c: &Component) -> Value {
    let part = design.library.parts.get(&c.part);
    let symbols: Vec<Value> = design
        .symbols_of(c.id)
        .map(|(o, s)| json!({"symbol_id": o.id, "sheet": o.sheet, "position": pt(s.position), "rotation": s.rotation, "mirror": s.mirror}))
        .collect();
    let pcb = design.footprint_of(c.id).map(
        |(o, f)| json!({"footprint_id": o.id, "position": pt(f.position), "rotation": f.rotation, "side": f.side, "locked": f.locked}),
    );
    json!({
        "id": c.id,
        "designator": c.designator,
        "value": c.value,
        "part": c.part,
        "description": part.map(|p| p.description.clone()).unwrap_or_default(),
        "footprint": c.footprint,
        "parameters": c.parameters,
        "part_parameters": part.map(|p| p.parameters.clone()).unwrap_or_default(),
        "schematic": symbols,
        "pcb": pcb,
    })
}

fn graphic_json(g: &SymbolGraphic) -> Value {
    match g {
        SymbolGraphic::Line { points, width } => {
            json!({"shape": "line", "points": points.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "width": mm(*width)})
        }
        SymbolGraphic::Rect { min, max, filled, .. } => json!({"shape": "rect", "min": pt(*min), "max": pt(*max), "filled": filled}),
        SymbolGraphic::Circle { center, radius, filled, .. } => {
            json!({"shape": "circle", "center": pt(*center), "radius": mm(*radius), "filled": filled})
        }
        SymbolGraphic::Arc { center, radius, start, end, .. } => {
            json!({"shape": "arc", "center": pt(*center), "radius": mm(*radius), "start": start, "end": end})
        }
        SymbolGraphic::Polygon { points, filled, .. } => {
            json!({"shape": "polygon", "points": points.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "filled": filled})
        }
        SymbolGraphic::Text { position, text, size } => {
            json!({"shape": "text", "position": pt(*position), "text": text, "size": mm(*size)})
        }
    }
}

pub fn sch_json(design: &Design, o: &SchObject) -> Value {
    let mut v = match &o.kind {
        SchKind::Symbol(s) => {
            let c = design.components.get(&s.component);
            json!({
                "component": s.component,
                "designator": c.map(|c| c.designator.clone()),
                "value": c.map(|c| c.value.clone()),
                "position": pt(s.position), "rotation": s.rotation, "mirror": s.mirror,
                "pins": design.pins(o).iter().map(|p| json!({"number": p.number, "name": p.name, "type": p.pin_type.label(), "position": pt(p.position)})).collect::<Vec<_>>(),
            })
        }
        SchKind::Wire(w) => json!({"points": w.points.iter().map(|p| pt(*p)).collect::<Vec<_>>()}),
        SchKind::Bus(w) => json!({"points": w.points.iter().map(|p| pt(*p)).collect::<Vec<_>>()}),
        SchKind::BusEntry(e) => json!({"start": pt(e.start), "end": pt(e.end)}),
        SchKind::Junction(j) => json!({"position": pt(j.position)}),
        SchKind::NetLabel(l) => json!({"position": pt(l.position), "text": l.text, "rotation": l.rotation}),
        SchKind::PowerPort(p) => json!({"position": pt(p.position), "net": p.net, "style": p.style, "rotation": p.rotation}),
        SchKind::Port(p) => json!({"position": pt(p.position), "name": p.name, "direction": p.direction, "rotation": p.rotation}),
        SchKind::NoConnect(n) => json!({"position": pt(n.position)}),
        SchKind::Text(t) => json!({"position": pt(t.position), "text": t.text, "size": mm(t.size)}),
        SchKind::SheetSymbol(s) => {
            json!({"position": pt(s.position), "width": mm(s.width), "height": mm(s.height), "child_sheet": s.child, "entries": s.entries.iter().map(|e| json!({"name": e.name, "offset": pt(e.offset)})).collect::<Vec<_>>()})
        }
        SchKind::Graphic(g) => graphic_json(g),
    };
    if let Some(map) = v.as_object_mut() {
        map.insert("id".into(), json!(o.id));
        map.insert("sheet".into(), json!(o.sheet));
        map.insert("type".into(), json!(o.kind.type_name()));
    }
    v
}

pub fn pcb_json(design: &Design, o: &PcbObject) -> Value {
    let mut v = match &o.kind {
        PcbKind::Footprint(f) => json!({
            "component": f.component, "designator": designator(design, f.component), "footprint": f.footprint,
            "position": pt(f.position), "rotation": f.rotation, "side": f.side, "locked": f.locked,
            "pads": design.pads(o).iter().map(|p| json!({
                "number": p.number, "net": p.net, "position": pt(p.center), "size": [mm(p.width), mm(p.height)],
                "shape": p.pad_shape.label(), "drill": p.drill.map(mm), "layers": p.layers.iter().map(|l| l.to_string()).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
        }),
        PcbKind::Track(t) => json!({"start": pt(t.start), "end": pt(t.end), "width": mm(t.width), "layer": t.layer, "net": t.net}),
        PcbKind::Via(v) => {
            json!({"position": pt(v.position), "diameter": mm(v.diameter), "drill": mm(v.drill), "net": v.net, "from": v.from, "to": v.to})
        }
        PcbKind::Zone(z) => {
            json!({"layer": z.layer, "net": z.net, "outline": z.outline.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "priority": z.priority, "poured": !z.filled.is_empty()})
        }
        PcbKind::Keepout(k) => json!({"layers": k.layers, "outline": k.outline.iter().map(|p| pt(*p)).collect::<Vec<_>>()}),
        PcbKind::Text(t) => json!({"layer": t.layer, "position": pt(t.position), "text": t.text, "height": mm(t.height)}),
        PcbKind::Line(l) => json!({"layer": l.layer, "start": pt(l.start), "end": pt(l.end), "width": mm(l.width)}),
    };
    if let Some(map) = v.as_object_mut() {
        map.insert("id".into(), json!(o.id));
        map.insert("type".into(), json!(o.kind.type_name()));
    }
    v
}

fn object_json(design: &Design, id: ObjectId) -> Value {
    match design.object(id) {
        Some(ObjectRef::Component(c)) => component_json(design, c),
        Some(ObjectRef::Sch(o)) => sch_json(design, o),
        Some(ObjectRef::Pcb(o)) => pcb_json(design, o),
        Some(ObjectRef::Sheet(s)) => json!({"id": s.id, "type": "Sheet", "name": s.name}),
        None => json!({"id": id, "type": "missing"}),
    }
}

fn violation_json(index: usize, v: &Violation) -> Value {
    json!({
        "index": index, "check": v.check, "rule": v.rule, "severity": v.severity, "message": v.message,
        "objects": v.objects, "location": v.location.map(pt), "sheet": v.sheet, "layer": v.layer,
    })
}

fn report_json(r: &Report) -> Value {
    json!({
        "errors": r.errors, "warnings": r.warnings, "clean": r.is_clean(),
        "violations": r.violations.iter().enumerate().map(|(i, v)| violation_json(i, v)).collect::<Vec<_>>(),
    })
}

// --- mutation path ------------------------------------------------------------------------------

fn new_errors(before: &Report, after: &Report) -> Vec<String> {
    let old: BTreeSet<&str> = before.violations.iter().map(|v| v.message.as_str()).collect();
    after
        .violations
        .iter()
        .filter(|v| v.severity == hephaestus_rules::Severity::Error && !old.contains(v.message.as_str()))
        .map(|v| format!("{}: {}", v.rule, v.message))
        .collect()
}

fn validation_delta(before: &Design, after: &Design) -> Value {
    let erc_new = new_errors(&erc::check(before), &erc::check(after));
    let drc_new = new_errors(&drc::check(before), &drc::check(after));
    json!({"new_erc_errors": erc_new, "new_drc_errors": drc_new})
}

fn mutate(session: &mut Session, command: Command, args: &Value, agent: Option<&str>) -> Result<Value, ApiError> {
    let source = Session::source(agent);
    let flag = |k: &str| args.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag("dry_run") {
        let (changes, preview) = session.editor.dry_run(&command)?;
        return Ok(json!({
            "status": "dry_run",
            "description": changes.description,
            "edits": changes.summary(),
            "would_create": changes.created(),
            "would_remove": changes.removed(),
            "validation": validation_delta(session.design(), &preview),
        }));
    }
    let needs_approval = matches!(source, Source::Agent(_)) && session.policy.approve_destructive && command.is_destructive();
    if flag("propose") || needs_approval {
        let label = command.label();
        let id = session.editor.propose(command, source.clone())?;
        session.note(format!("{source} proposed: {label} (proposal {id})"));
        return Ok(json!({
            "status": "pending_approval",
            "proposal": id,
            "description": label,
            "reason": if needs_approval { "destructive changes by agents need approval in Hephaestus" } else { "proposal requested" },
        }));
    }
    let outcome = if flag("require_clean") {
        let (changes, preview) = session.editor.dry_run(&command)?;
        let delta = validation_delta(session.design(), &preview);
        let blocked = delta["new_erc_errors"].as_array().is_some_and(|a| !a.is_empty())
            || delta["new_drc_errors"].as_array().is_some_and(|a| !a.is_empty());
        if blocked {
            return Err(ApiError::new("validation_failed", "the change introduces new rule errors").with_details(delta));
        }
        session.editor.apply(changes, source.clone())?
    } else {
        session.editor.execute(&command, source.clone())?
    };
    if source != Source::User {
        session.note(format!("{source}: {}", outcome.description));
    }
    let design = session.design();
    Ok(json!({
        "status": "committed",
        "seq": outcome.seq,
        "description": outcome.description,
        "created": outcome.created.iter().map(|id| object_summary(design, *id)).collect::<Vec<_>>(),
        "removed": outcome.removed,
        "modified": outcome.touched.iter().filter(|id| !outcome.created.contains(id) && !outcome.removed.contains(id)).collect::<Vec<_>>(),
    }))
}

fn object_summary(design: &Design, id: ObjectId) -> Value {
    match design.object(id) {
        Some(ObjectRef::Component(c)) => json!({"id": id, "type": "Component", "designator": c.designator}),
        Some(ObjectRef::Sch(o)) => json!({"id": id, "type": o.kind.type_name(), "sheet": o.sheet}),
        Some(ObjectRef::Pcb(o)) => json!({"id": id, "type": o.kind.type_name()}),
        Some(ObjectRef::Sheet(s)) => json!({"id": id, "type": "Sheet", "name": s.name}),
        None => json!({"id": id}),
    }
}

// --- dispatcher ---------------------------------------------------------------------------------

/// Run one tool. `agent` names the caller (`None` = the application user).
pub fn dispatch(session: &mut Session, tool: &str, args: &Value, agent: Option<&str>) -> Result<Value, ApiError> {
    let empty = json!({});
    let args = if args.is_null() { &empty } else { args };
    if !args.is_object() {
        return Err(ApiError::invalid("arguments must be a JSON object"));
    }
    let source = Session::source(agent);
    match tool {
        "list_tools" => Ok(json!(catalog().iter().map(|t| json!({"name": t.name, "category": t.category, "description": t.description, "mutates": t.mutates, "input_schema": t.schema})).collect::<Vec<_>>())),
        "get_project_info" => {
            let d = session.design();
            let netlist = sch_conn::compile(d);
            Ok(json!({
                "name": d.name,
                "manifest": session.files.as_ref().map(|f| f.manifest_path()),
                "dirty": session.editor.is_dirty(),
                "revision": session.editor.revision(),
                "seq": session.editor.seq(),
                "units": "mm",
                "sheets": d.sheets_in_order().iter().map(|s| json!({"id": s.id, "name": s.name, "size": s.size.label()})).collect::<Vec<_>>(),
                "components": d.components.len(),
                "nets": netlist.nets.len(),
                "pcb_objects": d.pcb.len(),
                "copper_layers": d.board.stack.copper_count(),
                "transaction": session.editor.transaction_name(),
                "pending_proposals": session.editor.proposals().count(),
                "libraries": session.libraries().iter().map(|l| json!({"name": l.name, "parts": l.parts.len()})).collect::<Vec<_>>(),
            }))
        }
        "list_documents" => {
            let d = session.design();
            let mut docs: Vec<Value> = d
                .sheets_in_order()
                .iter()
                .map(|s| json!({"type": "schematic", "id": s.id, "name": s.name, "size": s.size.label(), "objects": d.sheet_objects(s.id).count()}))
                .collect();
            docs.push(json!({"type": "pcb", "id": "pcb", "name": format!("{}.PcbDoc", d.name), "objects": d.pcb.len()}));
            docs.push(json!({"type": "library", "id": "library", "name": d.library.name, "parts": d.library.parts.len()}));
            Ok(json!(docs))
        }
        "inspect_document" => {
            let d = session.design();
            match args.get("document") {
                Some(Value::String(s)) if s == "pcb" => dispatch(session, "get_pcb", &json!({}), agent),
                Some(Value::String(s)) if s == "library" => Ok(json!({
                    "name": d.library.name,
                    "parts": d.library.parts.values().map(|p| json!({"name": p.name, "description": p.description, "symbol": p.symbol, "footprints": p.footprints})).collect::<Vec<_>>(),
                    "symbols": d.library.symbols.keys().collect::<Vec<_>>(),
                    "footprints": d.library.footprints.keys().collect::<Vec<_>>(),
                })),
                Some(v) => {
                    let id = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).map(ObjectId).ok_or_else(|| ApiError::invalid("document must be a sheet id, \"pcb\" or \"library\""))?;
                    let sheet = d.sheets.get(&id).ok_or_else(|| ApiError::new("unknown_object", format!("no sheet {id}")))?;
                    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
                    for o in d.sheet_objects(id) {
                        *counts.entry(o.kind.type_name()).or_default() += 1;
                    }
                    Ok(json!({"id": id, "name": sheet.name, "size": sheet.size.label(), "title": sheet.title, "object_counts": counts}))
                }
                None => Err(ApiError::invalid("missing document")),
            }
        }
        "save_project" => {
            let folder = str_arg(args, "folder").map(PathBuf::from);
            if let Some(f) = &folder
                && !f.is_absolute()
            {
                return Err(ApiError::invalid("folder must be absolute"));
            }
            let report = session.save(folder.as_deref())?;
            Ok(json!({"manifest": session.files.as_ref().map(|f| f.manifest_path()), "written": report.written, "unchanged": report.unchanged}))
        }
        "open_project" => {
            let path = PathBuf::from(req_str(args, "path")?);
            session.open(&path)?;
            dispatch(session, "get_project_info", &json!({}), agent)
        }
        "get_changes" => {
            let since = args.get("since").and_then(Value::as_u64).unwrap_or(0);
            Ok(json!({"seq": session.editor.seq(), "changes": session.editor.log_since(since)}))
        }
        "undo" => Ok(json!({"undone": session.editor.undo(source)?})),
        "redo" => Ok(json!({"redone": session.editor.redo(source)?})),
        "begin_transaction" => {
            session.editor.begin(req_str(args, "name")?, source)?;
            Ok(json!({"status": "open"}))
        }
        "commit_transaction" => {
            let o = session.editor.commit()?;
            Ok(json!({"status": "committed", "description": o.description, "created": o.created, "removed": o.removed}))
        }
        "rollback_transaction" => {
            session.editor.rollback()?;
            Ok(json!({"status": "rolled_back"}))
        }
        "list_proposals" => Ok(json!(session.editor.proposals().collect::<Vec<_>>())),
        "execute_command" => {
            let command: Command = serde_json::from_value(args.get("command").cloned().unwrap_or(Value::Null)).map_err(|e| ApiError::invalid(format!("bad command: {e}")))?;
            mutate(session, command, args, agent)
        }
        "search_library" => {
            let query = str_arg(args, "query").unwrap_or("");
            let category = str_arg(args, "category").map(str::to_lowercase);
            let mut out = Vec::new();
            for lib in std::iter::once(&session.design().library).chain(session.libraries()) {
                for p in lib.search(query) {
                    if category.as_ref().is_some_and(|c| !p.category.to_lowercase().starts_with(c)) {
                        continue;
                    }
                    if out.iter().any(|v: &Value| v["part"] == json!(p.name)) {
                        continue;
                    }
                    out.push(json!({"part": p.name, "title": p.title(), "kind": p.kind, "description": p.description, "category": p.category, "default_value": p.default_value, "footprints": p.footprints, "manufacturer": p.manufacturer, "mpn": p.mpn, "library": lib.name, "parameters": p.parameters}));
                }
            }
            Ok(json!(out))
        }
        "get_part" => {
            let name = req_str(args, "part")?;
            let lib = std::iter::once(&session.design().library).chain(session.libraries()).find(|l| l.parts.contains_key(name)).ok_or_else(|| ApiError::new("unknown_part", format!("no part {name}")))?;
            let part = &lib.parts[name];
            let symbol = lib.symbols.get(&part.symbol);
            Ok(json!({
                "part": part.name, "title": part.title(), "kind": part.kind, "family": part.family, "description": part.description, "category": part.category, "designator_prefix": part.designator_prefix,
                "default_value": part.default_value, "parameters": part.parameters, "library": lib.name, "revision": part.revision,
                "manufacturer": part.manufacturer, "mpn": part.mpn, "approved": part.approved, "origin": part.origin, "modified": part.modified,
                "pins": symbol.map(|s| s.pins.iter().map(|p| json!({"number": p.number, "name": p.name, "type": p.pin_type.label(), "pads": part.pads_for_pin(&p.number), "position": pt(p.position)})).collect::<Vec<_>>()),
                "footprints": part.footprints.iter().filter_map(|f| lib.footprints.get(f)).map(|f| json!({
                    "name": f.name, "description": f.description, "model_3d": f.model.as_ref().map(|m| m.source.clone()),
                    "pads": f.pads.iter().map(|p| json!({"number": p.number, "position": pt(p.position), "size": [mm(p.width), mm(p.height)], "shape": p.shape.label(), "drill": p.kind.drill().map(mm)})).collect::<Vec<_>>()
                })).collect::<Vec<_>>(),
            }))
        }
        "list_library_components" => {
            let status = check_library(session.design(), session.libraries());
            let lib = &session.design().library;
            Ok(json!(status.iter().filter_map(|st| lib.parts.get(&st.name).map(|p| json!({
                "part": p.name, "title": p.title(), "kind": p.kind, "family": p.family, "value": p.default_value, "footprints": p.footprints,
                "manufacturer": p.manufacturer, "mpn": p.mpn, "approved": p.approved, "used_by": st.used_by, "badges": st.badges(),
                "origin": p.origin,
            }))).collect::<Vec<_>>()))
        }
        "check_library" => {
            let status = check_library(session.design(), session.libraries());
            let attention: Vec<_> = status.iter().filter(|s| !s.badges().is_empty()).collect();
            Ok(json!({"components": status.len(), "needs_attention": attention}))
        }
        "update_library_component" => {
            let parameters = match args.get("parameters") {
                None | Some(Value::Null) => Default::default(),
                Some(v) => serde_json::from_value(v.clone()).map_err(|e| ApiError::invalid(format!("parameters: {e}")))?,
            };
            let approved = match args.get("approved") {
                None | Some(Value::Null) => None,
                Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| ApiError::invalid(format!("approved: {e}")))?),
            };
            let cmd = Command::UpdateLibraryComponent {
                part: req_str(args, "part")?.into(),
                description: str_arg(args, "description").map(String::from),
                manufacturer: str_arg(args, "manufacturer").map(String::from),
                mpn: str_arg(args, "mpn").map(String::from),
                parameters,
                approved,
            };
            mutate(session, cmd, args, agent)
        }
        "assign_component" => {
            let Some(Value::Array(items)) = args.get("components") else { return Err(ApiError::invalid("components must be an array")) };
            let components = items.iter().map(|v| component_ref(Some(v))).collect::<Result<Vec<_>, _>>()?;
            mutate(session, Command::AssignComponent { components, part: req_str(args, "part")?.into() }, args, agent)
        }
        "export_library_components" => {
            let lib = &session.design().library;
            let parts: Vec<String> = match args.get("parts") {
                Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                _ => lib.parts.keys().cloned().collect(),
            };
            if let Some(missing) = parts.iter().find(|p| !lib.parts.contains_key(*p)) {
                return Err(ApiError::new("unknown_part", format!("{missing} is not in the project library")));
            }
            let path = session.resolve(req_str(args, "path")?)?;
            let count = hephaestus_project::library_store::save_pack(lib, parts.iter().map(String::as_str), &path)?;
            Ok(json!({"path": path, "components": count}))
        }
        "import_library_components" => {
            let path = session.resolve(req_str(args, "path")?)?;
            let library = hephaestus_project::library_store::load_pack(&path)?;
            mutate(session, Command::ImportLibrary { library }, args, agent)
        }
        "get_schematic" => {
            let d = session.design();
            let sheet = opt_id(args, "sheet")?;
            if let Some(s) = sheet
                && !d.sheets.contains_key(&s)
            {
                return Err(ApiError::new("unknown_object", format!("no sheet {s}")));
            }
            let netlist = sch_conn::compile(d);
            let sheets: Vec<Value> = d
                .sheets_in_order()
                .iter()
                .filter(|s| sheet.is_none_or(|id| id == s.id))
                .map(|s| {
                    json!({
                        "id": s.id, "name": s.name, "size": s.size.label(),
                        "objects": d.sheet_objects(s.id).map(|o| {
                            let mut v = sch_json(d, o);
                            if let Some(net) = netlist.net_of_object(o.id) { v["net"] = json!(net.name); }
                            v
                        }).collect::<Vec<_>>(),
                        "junctions": netlist.junctions.get(&s.id).map(|j| j.iter().map(|p| pt(*p)).collect::<Vec<_>>()).unwrap_or_default(),
                    })
                })
                .collect();
            Ok(json!({"sheets": sheets}))
        }
        "get_components" => {
            let d = session.design();
            let filter = str_arg(args, "filter").map(str::to_lowercase);
            let mut list: Vec<&Component> = d
                .components
                .values()
                .filter(|c| filter.as_ref().is_none_or(|f| format!("{} {} {}", c.designator, c.value, c.part).to_lowercase().contains(f)))
                .collect();
            list.sort_by_key(|c| natural(&c.designator));
            Ok(json!(list.iter().map(|c| component_json(d, c)).collect::<Vec<_>>()))
        }
        "get_nets" => {
            let d = session.design();
            let netlist = sch_conn::compile(d);
            let routing = pcb_conn::analyze(d);
            let only = str_arg(args, "net");
            let nets: Vec<Value> = netlist
                .nets
                .iter()
                .filter(|n| only.is_none_or(|o| o == n.name))
                .map(|n| {
                    json!({
                        "name": n.name, "named": n.named, "power": n.power, "aliases": n.names,
                        "pins": n.nodes.iter().map(|p| format!("{}.{}", p.designator, p.pin)).collect::<Vec<_>>(),
                        "nodes": n.nodes.iter().map(|p| json!({"designator": p.designator, "pin": p.pin, "pin_name": p.pin_name, "pin_type": p.pin_type.label()})).collect::<Vec<_>>(),
                        "sheets": n.sheets,
                        "pcb": routing.nets.get(&n.name).map(|r| json!({"pads": r.pads, "unrouted_connections": r.unrouted})),
                    })
                })
                .collect();
            if only.is_some() && nets.is_empty() {
                return Err(ApiError::new("unknown_net", format!("no net {}", only.unwrap_or_default())));
            }
            Ok(json!({
                "nets": nets,
                "unconnected_pins": netlist.unconnected_pins.iter().map(|p| format!("{}.{}", p.designator, p.pin)).collect::<Vec<_>>(),
            }))
        }
        "add_component" => {
            let mut position = point(args.get("position").unwrap_or(&Value::Null), "position")?;
            if args.get("snap").and_then(Value::as_bool).unwrap_or(true) {
                position = snap_point(position, hephaestus_core::geometry::mil(100.0));
            }
            let parameters: BTreeMap<String, String> = match args.get("parameters") {
                Some(Value::Object(m)) => m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect(),
                _ => BTreeMap::new(),
            };
            let cmd = Command::PlaceComponent {
                part: req_str(args, "part")?.into(),
                sheet: opt_id(args, "sheet")?,
                position,
                rotation: opt_f64(args, "rotation")?.unwrap_or(0.0),
                mirror: args.get("mirror").and_then(Value::as_bool).unwrap_or(false),
                designator: str_arg(args, "designator").map(String::from),
                value: str_arg(args, "value").map(String::from),
                footprint: str_arg(args, "footprint").map(String::from),
                parameters,
            };
            mutate(session, cmd, args, agent)
        }
        "remove_component" => mutate(session, Command::RemoveComponent { component: component_ref(args.get("component"))? }, args, agent),
        "connect_pins" => {
            let style = match str_arg(args, "style") {
                None | Some("auto") => ConnectStyle::Auto,
                Some("wire") => ConnectStyle::Wire,
                Some("label") => ConnectStyle::Label,
                Some(other) => return Err(ApiError::invalid(format!("unknown style {other}"))),
            };
            let cmd = Command::ConnectPins { from: pin_ref_arg(args.get("from"))?, to: pin_ref_arg(args.get("to"))?, net: str_arg(args, "net").map(String::from), style };
            mutate(session, cmd, args, agent)
        }
        "create_net" => {
            let Some(Value::Array(pins)) = args.get("pins") else { return Err(ApiError::invalid("pins must be an array")) };
            let pins = pins.iter().map(|p| pin_ref_arg(Some(p))).collect::<Result<Vec<_>, _>>()?;
            mutate(session, Command::CreateNet { name: req_str(args, "name")?.into(), pins }, args, agent)
        }
        "move_symbol" => {
            let d = session.design();
            let c = resolve_component(d, &component_ref(args.get("component"))?)?;
            let (symbol, at) = d.symbols_of(c).next().map(|(o, s)| (o.id, s.position)).ok_or_else(|| ApiError::invalid("the component has no schematic symbol"))?;
            let delta = match (args.get("position"), args.get("offset")) {
                (Some(p), _) => snap_point(point(p, "position")?, hephaestus_core::geometry::mil(100.0)) - at,
                (None, Some(o)) => point(o, "offset")?,
                _ => return Err(ApiError::invalid("pass position or offset")),
            };
            let drag = args.get("drag_wires").and_then(Value::as_bool).unwrap_or(true);
            mutate(session, Command::MoveObjects { ids: vec![symbol], dx: delta.x, dy: delta.y, drag_wires: drag }, args, agent)
        }
        "modify_component_properties" => {
            let parameters: BTreeMap<String, Option<String>> = match args.get("parameters") {
                Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.as_str().map(String::from))).collect(),
                _ => BTreeMap::new(),
            };
            let cmd = Command::ModifyComponent {
                component: component_ref(args.get("component"))?,
                designator: str_arg(args, "designator").map(String::from),
                value: str_arg(args, "value").map(String::from),
                footprint: str_arg(args, "footprint").map(String::from),
                parameters,
            };
            mutate(session, cmd, args, agent)
        }
        "add_wire" => mutate(session, Command::AddWire { sheet: opt_id(args, "sheet")?, points: points(args.get("points"), "points")? }, args, agent),
        "add_net_label" => {
            let cmd = Command::AddNetLabel {
                sheet: opt_id(args, "sheet")?,
                position: point(args.get("position").unwrap_or(&Value::Null), "position")?,
                text: req_str(args, "text")?.into(),
                rotation: opt_f64(args, "rotation")?.unwrap_or(0.0),
            };
            mutate(session, cmd, args, agent)
        }
        "add_power_port" => {
            let style = match str_arg(args, "style") {
                None => None,
                Some(s) => Some(PowerStyle::ALL.into_iter().find(|p| format!("{p:?}").eq_ignore_ascii_case(s) || p.label().eq_ignore_ascii_case(s)).ok_or_else(|| ApiError::invalid(format!("unknown power port style {s}")))?),
            };
            let cmd = Command::AddPowerPort {
                sheet: opt_id(args, "sheet")?,
                position: point(args.get("position").unwrap_or(&Value::Null), "position")?,
                net: req_str(args, "net")?.into(),
                style,
                rotation: opt_f64(args, "rotation")?,
            };
            mutate(session, cmd, args, agent)
        }
        "add_port" => {
            let direction = match str_arg(args, "direction").map(str::to_ascii_lowercase).as_deref() {
                None | Some("unspecified") => PortDirection::Unspecified,
                Some("input") => PortDirection::Input,
                Some("output") => PortDirection::Output,
                Some("bidirectional") => PortDirection::Bidirectional,
                Some(other) => return Err(ApiError::invalid(format!("unknown direction {other}"))),
            };
            let cmd = Command::AddPort {
                sheet: opt_id(args, "sheet")?,
                position: point(args.get("position").unwrap_or(&Value::Null), "position")?,
                name: req_str(args, "name")?.into(),
                direction,
                rotation: opt_f64(args, "rotation")?.unwrap_or(0.0),
            };
            mutate(session, cmd, args, agent)
        }
        "add_no_connect" => mutate(session, Command::AddNoConnect { sheet: opt_id(args, "sheet")?, position: point(args.get("position").unwrap_or(&Value::Null), "position")? }, args, agent),
        "add_junction" => mutate(session, Command::AddJunction { sheet: opt_id(args, "sheet")?, position: point(args.get("position").unwrap_or(&Value::Null), "position")? }, args, agent),
        "add_sheet" => {
            let size = match str_arg(args, "size") {
                None => None,
                Some(s) => Some(hephaestus_core::SheetSize::ALL.into_iter().find(|z| z.label().eq_ignore_ascii_case(s)).ok_or_else(|| ApiError::invalid(format!("unknown sheet size {s}")))?),
            };
            mutate(session, Command::AddSheet { name: req_str(args, "name")?.into(), size }, args, agent)
        }
        "delete_objects" => {
            let ids = ids_arg(session.design(), args)?;
            mutate(session, Command::DeleteObjects { ids }, args, agent)
        }
        "get_pcb" => {
            let d = session.design();
            let routing = pcb_conn::analyze(d);
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for o in d.pcb.values() {
                *counts.entry(o.kind.type_name()).or_default() += 1;
            }
            let total: usize = routing.nets.values().map(|n| n.connections).sum();
            let unrouted = routing.unrouted.len();
            let r = &d.board.rules;
            Ok(json!({
                "outline": d.board.outline.iter().map(|p| pt(*p)).collect::<Vec<_>>(),
                "size": d.board.outline_bounds().map(|b| json!({"width": mm(b.width()), "height": mm(b.height())})),
                "origin": pt(d.board.origin),
                "layer_stack": d.board.stack.layers.iter().map(|l| json!({"layer": l.layer, "copper_thickness": mm(l.copper_thickness), "dielectric": l.dielectric.as_ref().map(|x| json!({"material": x.material, "thickness": mm(x.thickness), "dk": x.constant}))})).collect::<Vec<_>>(),
                "thickness": mm(d.board.stack.total_thickness()),
                "rules": {
                    "clearance": mm(r.clearance), "min_track_width": mm(r.min_track_width), "preferred_track_width": mm(r.preferred_track_width),
                    "max_track_width": mm(r.max_track_width), "min_via_diameter": mm(r.min_via_diameter), "min_via_drill": mm(r.min_via_drill),
                    "preferred_via_diameter": mm(r.preferred_via_diameter), "preferred_via_drill": mm(r.preferred_via_drill),
                    "min_annular_ring": mm(r.min_annular_ring), "board_edge_clearance": mm(r.board_edge_clearance),
                },
                "object_counts": counts,
                "routing": {"connections": total, "unrouted": unrouted, "completion_percent": if total == 0 { 100.0 } else { ((total.saturating_sub(unrouted)) as f64 / total as f64 * 1000.0).round() / 10.0 }},
                "shorts": routing.shorts.len(),
            }))
        }
        "get_footprints" => {
            let d = session.design();
            let only = match args.get("component") {
                None | Some(Value::Null) => None,
                v => Some(resolve_component(d, &component_ref(v)?)?),
            };
            Ok(json!(d.pcb.values().filter(|o| match &o.kind {
                PcbKind::Footprint(f) => only.is_none() || f.component == only,
                _ => false,
            }).map(|o| pcb_json(d, o)).collect::<Vec<_>>()))
        }
        "get_tracks" => {
            let d = session.design();
            let net = str_arg(args, "net");
            let layer = layer_arg(args, "layer")?;
            Ok(json!(d.pcb.values().filter(|o| match &o.kind {
                PcbKind::Track(t) => net.is_none_or(|n| t.net.as_deref() == Some(n)) && layer.is_none_or(|l| t.layer == l),
                _ => false,
            }).map(|o| pcb_json(d, o)).collect::<Vec<_>>()))
        }
        "get_vias" => {
            let d = session.design();
            let net = str_arg(args, "net");
            Ok(json!(d.pcb.values().filter(|o| match &o.kind {
                PcbKind::Via(v) => net.is_none_or(|n| v.net.as_deref() == Some(n)),
                _ => false,
            }).map(|o| pcb_json(d, o)).collect::<Vec<_>>()))
        }
        "update_pcb" => mutate(session, Command::UpdatePcb {}, args, agent),
        "place_component" => {
            let side = match str_arg(args, "side") {
                None => None,
                Some(s) if s.eq_ignore_ascii_case("top") => Some(Side::Top),
                Some(s) if s.eq_ignore_ascii_case("bottom") => Some(Side::Bottom),
                Some(s) => return Err(ApiError::invalid(format!("unknown side {s}"))),
            };
            let cmd = Command::PlaceFootprint {
                component: component_ref(args.get("component"))?,
                position: point(args.get("position").unwrap_or(&Value::Null), "position")?,
                rotation: opt_f64(args, "rotation")?,
                side,
            };
            mutate(session, cmd, args, agent)
        }
        "move_component" => {
            let d = session.design();
            let c = resolve_component(d, &component_ref(args.get("component"))?)?;
            let (id, at) = d.footprint_of(c).map(|(o, f)| (o.id, f.position)).ok_or_else(|| ApiError::invalid("the component has no footprint on the board; use place_component or update_pcb"))?;
            let delta = match (args.get("position"), args.get("offset")) {
                (Some(p), _) => point(p, "position")? - at,
                (None, Some(o)) => point(o, "offset")?,
                _ => return Err(ApiError::invalid("pass position or offset")),
            };
            mutate(session, Command::MoveObjects { ids: vec![id], dx: delta.x, dy: delta.y, drag_wires: false }, args, agent)
        }
        "rotate_component" => {
            let d = session.design();
            let c = resolve_component(d, &component_ref(args.get("component"))?)?;
            let (id, current) = d.footprint_of(c).map(|(o, f)| (o.id, f.rotation)).ok_or_else(|| ApiError::invalid("the component has no footprint on the board"))?;
            let by = match (opt_f64(args, "rotation")?, opt_f64(args, "by")?) {
                (Some(abs), _) => abs - current,
                (None, Some(rel)) => rel,
                _ => return Err(ApiError::invalid("pass rotation or by")),
            };
            mutate(session, Command::RotateObjects { ids: vec![id], degrees: by, pivot: None }, args, agent)
        }
        "route_net" => {
            let net = req_str(args, "net")?.to_string();
            let layer = layer_arg(args, "layer")?;
            let width = opt_length(args, "width")?;
            let cmd = match args.get("points") {
                Some(Value::Array(_)) => Command::AddTracks { points: points(args.get("points"), "points")?, layer: layer.unwrap_or(Layer::TopCopper), width, net: Some(net) },
                _ => Command::RouteNet { net, layer, width },
            };
            mutate(session, cmd, args, agent)
        }
        "create_via" => {
            let cmd = Command::AddVia {
                position: point(args.get("position").unwrap_or(&Value::Null), "position")?,
                diameter: opt_length(args, "diameter")?,
                drill: opt_length(args, "drill")?,
                net: str_arg(args, "net").map(String::from),
                from: layer_arg(args, "from")?,
                to: layer_arg(args, "to")?,
            };
            mutate(session, cmd, args, agent)
        }
        "modify_track" => {
            let track = opt_id(args, "track")?.ok_or_else(|| ApiError::invalid("missing track"))?;
            let cmd = Command::ModifyTrack {
                track,
                start: args.get("start").map(|p| point(p, "start")).transpose()?,
                end: args.get("end").map(|p| point(p, "end")).transpose()?,
                width: opt_length(args, "width")?,
                layer: layer_arg(args, "layer")?,
                net: str_arg(args, "net").map(String::from),
            };
            mutate(session, cmd, args, agent)
        }
        "create_copper_zone" => {
            let layer = layer_arg(args, "layer")?.ok_or_else(|| ApiError::invalid("missing layer"))?;
            let cmd = Command::AddZone {
                layer,
                net: str_arg(args, "net").map(String::from),
                outline: points(args.get("outline"), "outline")?,
                priority: args.get("priority").and_then(Value::as_u64).map(|p| p.min(u32::MAX as u64) as u32),
            };
            mutate(session, cmd, args, agent)
        }
        "set_board_outline" => mutate(session, Command::SetBoardOutline { outline: points(args.get("outline"), "outline")? }, args, agent),
        "align_components" => {
            let mode = match req_str(args, "mode")? {
                "left" => AlignMode::Left,
                "right" => AlignMode::Right,
                "top" => AlignMode::Top,
                "bottom" => AlignMode::Bottom,
                "center_horizontal" => AlignMode::CenterHorizontal,
                "center_vertical" => AlignMode::CenterVertical,
                other => return Err(ApiError::invalid(format!("unknown mode {other}"))),
            };
            let ids = pcb_ids(session.design(), ids_arg(session.design(), args)?);
            mutate(session, Command::AlignObjects { ids, mode }, args, agent)
        }
        "distribute_components" => {
            let axis = match req_str(args, "axis")? {
                "horizontal" => Axis::Horizontal,
                "vertical" => Axis::Vertical,
                other => return Err(ApiError::invalid(format!("unknown axis {other}"))),
            };
            let ids = pcb_ids(session.design(), ids_arg(session.design(), args)?);
            mutate(session, Command::DistributeObjects { ids, axis }, args, agent)
        }
        "run_erc" => {
            let report = erc::check(session.design());
            let out = report_json(&report);
            session.last_erc = Some((session.editor.revision(), report));
            Ok(out)
        }
        "run_drc" => {
            let report = drc::check(session.design());
            let out = report_json(&report);
            session.last_drc = Some((session.editor.revision(), report));
            Ok(out)
        }
        "get_violations" => {
            let which = str_arg(args, "check").unwrap_or("all");
            let severity = str_arg(args, "severity");
            let revision = session.editor.revision();
            if which != "drc" && session.last_erc.as_ref().is_none_or(|(r, _)| *r != revision) {
                session.last_erc = Some((revision, erc::check(session.design())));
            }
            if which != "erc" && session.last_drc.as_ref().is_none_or(|(r, _)| *r != revision) {
                session.last_drc = Some((revision, drc::check(session.design())));
            }
            let keep = |v: &Violation| severity.is_none_or(|s| format!("{:?}", v.severity).eq_ignore_ascii_case(s));
            let list = |r: &Option<(u64, Report)>| -> Vec<Value> {
                r.as_ref().map(|(_, r)| r.violations.iter().enumerate().filter(|(_, v)| keep(v)).map(|(i, v)| violation_json(i, v)).collect()).unwrap_or_default()
            };
            Ok(json!({
                "erc": if which == "drc" { Value::Null } else { json!(list(&session.last_erc)) },
                "drc": if which == "erc" { Value::Null } else { json!(list(&session.last_drc)) },
            }))
        }
        "inspect_violation" => {
            let check = req_str(args, "check")?;
            let index = args.get("index").and_then(Value::as_u64).ok_or_else(|| ApiError::invalid("missing index"))? as usize;
            let report = match check {
                "erc" => session.last_erc.as_ref(),
                "drc" => session.last_drc.as_ref(),
                _ => return Err(ApiError::invalid("check must be erc or drc")),
            };
            let (revision, report) = report.ok_or_else(|| ApiError::new("no_report", format!("run_{check} first")))?;
            let v = report.violations.get(index).ok_or_else(|| ApiError::new("unknown_violation", format!("no violation {index}")))?;
            let d = session.design();
            Ok(json!({
                "violation": violation_json(index, v),
                "stale": *revision != session.editor.revision(),
                "objects": v.objects.iter().map(|id| object_json(d, *id)).collect::<Vec<_>>(),
            }))
        }
        "export_gerber" => {
            let dir = session.resolve(req_str(args, "directory")?)?;
            let report = hephaestus_export::gerber::export(session.design(), &dir)?;
            Ok(json!(report))
        }
        "export_drill_files" => {
            let dir = session.resolve(req_str(args, "directory")?)?;
            Ok(json!(hephaestus_export::drill::export(session.design(), &dir)?))
        }
        "export_bom" => {
            let rows = hephaestus_export::bom::bom_rows(session.design());
            let files = match str_arg(args, "directory") {
                Some(dir) => {
                    let dir = session.resolve(dir)?;
                    hephaestus_export::bom(session.design(), &dir)?.files
                }
                None => Vec::new(),
            };
            Ok(json!({"rows": rows, "files": files}))
        }
        "export_pick_and_place" => {
            let dir = session.resolve(req_str(args, "directory")?)?;
            Ok(json!(hephaestus_export::pick_and_place(session.design(), &dir)?))
        }
        other => Err(ApiError::new("unknown_tool", format!("no tool named {other}"))),
    }
}

/// Components given by designator resolve to their footprint for PCB operations.
fn pcb_ids(design: &Design, ids: Vec<ObjectId>) -> Vec<ObjectId> {
    ids.into_iter()
        .map(|id| if design.components.contains_key(&id) { design.footprint_of(id).map(|(o, _)| o.id).unwrap_or(id) } else { id })
        .collect()
}

fn natural(designator: &str) -> (String, u64) {
    let split = designator.find(|c: char| c.is_ascii_digit()).unwrap_or(designator.len());
    let (prefix, number) = designator.split_at(split);
    (prefix.to_string(), number.parse().unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests;
