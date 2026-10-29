# Hephaestus — Agent API

External agents are first-class users of Hephaestus. They use the same engineering core as the
user interface, through validated commands, with undo, transactions, dry runs and approvals.

Three ways in, one tool set (`crates/agent-api/src/tools.rs`):

| Client | Transport | Use |
|---|---|---|
| **MCP** (`hephaestus-mcp`) | stdio, MCP via the official Rust SDK (rmcp 3.7) | Claude Code, OpenAI Codex, any MCP client |
| **Python SDK** (`sdk/python`) | control protocol | scripts and custom agents |
| **Control protocol** | newline-delimited JSON over TCP on 127.0.0.1:7272 | anything that can open a socket |

## Connecting

Build once: `cargo build --release -p hephaestus -p hephaestus-mcp`.

`hephaestus-mcp` connects to a running Hephaestus on 127.0.0.1:7272 by default, so the agent edits
the project the user has open and changes appear live. If Hephaestus is not running it falls back
to a headless session. Options:

```
hephaestus-mcp [--connect HOST:PORT] [--project PATH.pcbprj | --demo | --headless] [--agent NAME]
```

### Claude Code

```sh
claude mcp add hephaestus -- /absolute/path/to/target/release/hephaestus-mcp --agent claude-code
```

or a project `.mcp.json`:

```json
{
  "mcpServers": {
    "hephaestus": {
      "command": "/absolute/path/to/target/release/hephaestus-mcp",
      "args": ["--agent", "claude-code"]
    }
  }
}
```

### OpenAI Codex CLI (`~/.codex/config.toml`)

```toml
[mcp_servers.hephaestus]
command = "/absolute/path/to/target/release/hephaestus-mcp"
args = ["--agent", "codex"]
```

### Python

```python
import sys; sys.path.insert(0, "sdk/python")   # or: pip install ./sdk/python
from hephaestus import Client
pcb = Client(agent="my-script")                  # 127.0.0.1:7272
pcb.add_component(part="RES", position=["1000mil", "1000mil"])
print(pcb.get_nets())
```

Every tool is a method (`pcb.route_net(net="GND")`); errors raise `HephaestusError` with
`code`, `message` and `details`. See `examples/agent_rc_filter.py` for a complete session.

### Security

The control port listens on 127.0.0.1 only. Set `HEPHAESTUS_TOKEN` in the environment of both
Hephaestus and the client to require a shared secret on every request. Set the port to `0` in
Preferences (or `--port 0`) to disable agent access.

## Conventions

- **Units.** Lengths are millimetres when numbers, or strings with units: `"100mil"`,
  `"0.254mm"`, `"0.1in"`. Points are `{"x": …, "y": …}` or `[x, y]`. Results are millimetres.
  Y points up (as in Altium's readout and Gerber).
- **References.** Components by designator (`"U1"`) or object id; pins by number or unique name
  (`{"component": "U1", "pin": "VCC"}`). Every object has a stable integer id.
- **Mutating tools** accept:
  - `dry_run: true` — plan and validate only; returns the edits, ids that would be created or
    removed, and the **new** ERC/DRC errors the change would introduce;
  - `propose: true` — park the change for a person to accept in the Agents panel;
  - `require_clean: true` — refuse (`validation_failed`) if the change adds ERC or DRC errors.
- **Approval.** In the GUI, destructive agent commands (deletions, board outline, layer stack,
  rules) become proposals (`"status": "pending_approval"`) unless the user turns this off in the
  Agents panel. Headless sessions apply them directly.
- **Responses.** Mutations return `{"status": "committed", "seq", "description", "created":
  [{id, type, designator…}], "removed", "modified"}`.
- **Errors** are structured: `{"code", "message", "details"}`. Codes include
  `invalid_argument`, `invalid`, `unknown_object`, `locked`, `conflict`, `integrity`, `batch`,
  `validation_failed`, `unknown_tool`, `no_transaction`, `transaction_open`, `nothing_to_undo`,
  `project_io`, `export_failed`.

## Tools

### Project
| Tool | Purpose |
|---|---|
| `get_project_info` | Name, location, dirty state, revision and change sequence, sheets, counts, layer count, open transaction, pending proposals, libraries. |
| `list_documents` | Sheets, the PCB document, the project library. |
| `inspect_document` | Contents summary of a sheet id, `"pcb"` or `"library"`. |
| `save_project` / `open_project` | Save (optionally to a new absolute folder) / open a `.pcbprj` or `.hephaestus.json`. |
| `get_changes` | Change log after a sequence number: who (`user`, `agent`, `script`) did what, including undo/redo and the objects touched. Use it to see the user's manual edits. |
| `undo` / `redo` | History. |
| `begin_transaction` / `commit_transaction` / `rollback_transaction` | Group changes into one undo step; rollback discards them. |
| `list_proposals` | Changes awaiting approval. |
| `execute_command` | Run a raw core `Command` (nanometres; see `crates/commands/src/command.rs`). Validated like everything else. |

### Library
`search_library` (keywords, category; matches part numbers too), `get_part` (pins with pad
mapping, footprints with pads, parameters, 3D model reference, kind, part numbers, approved
parts, origin).

The project library holds one component per value and footprint for generic parts: placing
`RES` with value 10k on 0603 creates (or reuses) `RES_10k_0603`, and changing a resistor's value
or footprint moves it to the matching component. Auto-made components nobody uses any more are
removed unless they carry part numbers. See [LIBRARY_FORMAT.md](LIBRARY_FORMAT.md).

| Tool | What it does |
|---|---|
| `list_library_components` | Project components with title (`R 10k 0603`), kind, part numbers, approved parts, users and status badges. |
| `check_library` | Components needing attention: broken, update available, modified locally, no part number, no 3D model, unused. |
| `update_library_component` | Set description, manufacturer/MPN, parameters, or the approved manufacturer parts with supplier numbers (`digikey`, `mouser`, `lcsc`) and the JLC Basic flag. Sourcing changes do not mark a component modified. |
| `assign_component` | Point placed components at another library component (pin numbers must match). |
| `export_library_components` / `import_library_components` | Component packs (`*.pcbcomp.json`): components with their symbols and footprints in one file. Identical shapes are shared on import; clashing names get `_2`. |

### Schematic
`get_schematic` (objects with positions and nets, junctions), `get_components`, `get_nets`
(pins per net, PCB routing status, unconnected pins), `add_component` (snaps to 100 mil),
`remove_component`, `connect_pins` (a wire when a clean orthogonal route exists, otherwise net
labels), `create_net` (label stubs on each pin), `move_symbol` (wires follow), 
`modify_component_properties` (designator, value, footprint, parameters; footprint changes keep
PCB placement and nets), `add_wire`, `add_net_label`, `add_power_port`, `add_port`,
`add_no_connect`, `add_junction`, `add_sheet`, `delete_objects`.

### PCB
`get_pcb` (outline, size, stack, rules, routing completion, shorts), `get_footprints` (pads with
nets and layers), `get_tracks`, `get_vias`, `update_pcb` (Altium's Update PCB Document),
`place_component`, `move_component`, `rotate_component`, `route_net`, `create_via`,
`modify_track`, `create_copper_zone`, `set_board_outline`, `align_components`,
`distribute_components`.

`route_net` with `points` lays a track run along them. Without points it routes every unrouted
connection of the net with a direct two-segment 45° path on one layer, choosing the shape that
keeps clearance to other nets, and **refuses** connections with no clean direct path. It is not
an obstacle-avoiding autorouter.

### Validation
`run_erc`, `run_drc`, `get_violations` (re-runs when stale), `inspect_violation` (the violation
plus full details of every object involved). Violations carry `rule`, `severity`, `message`,
`objects`, `location`, `sheet`/`layer`.

ERC: floating input pins, unconnected power and other pins, No-ERC on connected pins,
conflicting outputs, outputs on power nets, nets without drivers, single-pin nets, nets with
several names, floating labels, unconnected wire ends, duplicate/unannotated designators,
library reference problems.
DRC: clearance (net classes, pad-to-pad inside a footprint ignored as in Altium), shorts,
unrouted connections, track width, via diameter/hole/annular ring, pad holes and rings, board
edge clearance, keep-outs, components off board, overlapping courtyards, unpoured or off-board
polygons.

### Manufacturing
`export_gerber` (Gerber X2: copper, overlay, mask, paste, profile), `export_drill_files`
(Excellon PTH/NPTH), `export_bom` (rows inline, CSV optional), `export_pick_and_place`.
Relative directories resolve inside the project folder. Exporters report what they could not
represent (unpoured pours, free text) as warnings.

### GUI-only methods (control protocol)
`ui.screenshot {path}` (absolute PNG path), `ui.open_document {document: "pcb" | sheet name}`,
`ui.select {components: ["U1", …]}`.

## Control protocol

```
→ {"id": 1, "tool": "get_nets", "args": {"net": "GND"}, "agent": "my-agent"}
← {"id": 1, "ok": true, "result": {...}}
← {"id": 2, "ok": false, "error": {"code": "invalid", "message": "...", "details": {...}}}
```

`"method"`/`"params"` are accepted as aliases of `"tool"`/`"args"`; `"token"` carries the shared
secret. `{"tool": "list_tools"}` returns the catalog with JSON schemas.

## A typical agent session

```
search_library {query: "op amp"}        → OPAMP_SOT23-5
begin_transaction {name: "Buffer stage"}
add_component {part: "OPAMP_SOT23-5", position: ["3800mil", "3900mil"]}
create_net {name: "OUT", pins: [{component: "U1", pin: "OUT"}, {component: "U1", pin: "-"}]}
run_erc → 0 errors
commit_transaction
update_pcb
place_component {component: "U1", position: [24, 12.5]}
route_net {net: "OUT", require_clean: true}
run_drc → get_violations → inspect_violation …
export_gerber {directory: "Outputs"}
```
