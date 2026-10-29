# Hephaestus — Architecture

Hephaestus is a Rust workspace. The engineering core has no GUI dependency; the egui application
and external agents are two clients of the same core.

```
                ┌──────────────────────────┐        ┌─────────────────────────┐
  Person ──────▶│ hephaestus (app) + ui      │        │ hephaestus-mcp (stdio)     │◀── Claude Code / Codex /
                │ egui shell, editors      │        │ rmcp server              │    any MCP client
                └──────────┬───────────────┘        └──────────┬──────────────┘
                           │ owns                               │ connected: control client
                           ▼                                    │ headless: owns a Session
                ┌──────────────────────────┐  JSON lines/TCP    │
                │ agent-api: Session,      │◀───────────────────┘   Python SDK ──┘
                │ tool catalog, dispatch,  │   127.0.0.1:7272
                │ control server/client    │
                └──────────┬───────────────┘
                           ▼
      commands (Editor: plan → validate → apply, undo/redo, transactions, proposals)
                           ▼
   core (Design database) ◀── connectivity ◀── rules (ERC/DRC)     project (persistence)
        ▲                                                         export (Gerber/drill/BOM/PnP)
   geometry                        libraries (built-in parts)      canvas (egui camera/grid/gestures)
```

## Crates

| Crate | Depends on egui | Responsibility |
|---|---|---|
| `hephaestus-geometry` | no | Integer-nanometre `Point`/`Rect`, units and parsing, transforms, shape distance (`Shape::gap`) for DRC, `GridIndex` spatial hash. |
| `hephaestus-core` | no | The design database: `Design` with `Component`s, `Library` (symbols, footprints, parts), `Sheet`s and `SchObject`s, `BoardSettings` (outline, layer stack, rules) and `PcbObject`s. Derived geometry (pin and pad positions, bounds) and integrity checks. |
| `hephaestus-libraries` | no | The built-in library generated in code (IPC-7351 lands, manufacturer pinouts), library file I/O. |
| `hephaestus-connectivity` | no | Schematic netlist compilation (Altium connection rules, global labels/power ports/ports, automatic junctions) and PCB copper connectivity (islands, ratsnest MST, shorts). |
| `hephaestus-rules` | no | ERC and DRC on the structured data, returning `Violation`s with object ids, locations, sheet/layer. |
| `hephaestus-commands` | no | The only mutation path: `Command` → `Planner` → `ChangeSet` of invertible `Edit`s → `Editor` (validation, apply, undo/redo, transactions, dry runs, proposals, change log). |
| `hephaestus-project` | no | Versioned multi-file JSON project format, migrations, incremental atomic saves, autosave and recovery, single-file bundles. |
| `hephaestus-export` | no | Gerber X2, Excellon, BOM and pick-and-place. |
| `hephaestus-agent-api` | no | `Session`, the agent tool catalog and dispatcher (mm units, structured errors, dry-run/propose/require_clean), the JSON-lines control protocol, the demo project. |
| `hephaestus-mcp` | no | MCP server binary (rmcp, stdio). Forwards to a running GUI or owns a headless session. |
| `hephaestus-canvas` | yes | Engineering canvas infrastructure shared by both editors: camera, transforms, adaptive grid, snapping, Altium navigation, selection-box gestures. |
| `hephaestus-ui` | yes | The application: shell, docking, panels, schematic and PCB editors, dialogs. |
| `hephaestus` | yes | Desktop entry point and CLI. |

Each crate is non-empty and tested; there are no placeholder crates. Schematic and PCB data live
as modules of `core` rather than separate crates because they share one id space and one
component identity; splitting them would add coupling, not remove it.

## Data model decisions

- **Integer nanometres, Y up.** Exact for both metric and imperial grids, matches Gerber's
  4.6 mm format one-to-one, and readouts match Altium's Y-up convention.
- **One project-wide `ObjectId` space.** Components, sheets, schematic and PCB objects share it;
  ids are never reused. Selection, hit testing, history and agent references are all just ids.
- **Logical component ↔ symbol ↔ footprint.** A `Component` is the electrical identity. A
  `SymbolInstance` (schematic) and a `Footprint` (PCB) refer to it. Changing the footprint keeps
  placement, side and pad nets; renaming the designator changes both views.
- **Parts and pin maps.** A `PartDef` names a symbol, compatible footprints and a pin→pads map
  (e.g. the AMS1117 VOUT pin drives pad 2 and the tab). Pad nets come from that map.
- **Self-contained projects.** Placing a part copies it (symbol, footprints) into the project
  library, so a project never depends on the machine's libraries.
- **Pad nets are assigned by Update PCB** (Altium's ECO model). PCB connectivity is then checked
  against them; tracks carry the net they were routed with.

## Mutation, validation and history

Every change, whether from the UI, an agent or a script, is a `Command`. The `Planner` works on a
scratch copy of the design and records primitive `Edit`s (`Insert`/`Remove`/`Modify` with full
before/after state). The `Editor`:

1. refuses changes that add integrity problems (dangling references, duplicate designators,
   objects on layers not in the stack);
2. applies the `ChangeSet` atomically, checking each edit against current state (a stale change
   is a `Conflict`, never a silent overwrite);
3. records it for undo/redo, or in the open transaction;
4. appends to the change log with the `Source` (User, Agent(name), Script), which agents read
   with `get_changes`.

Dry runs return the change set and the would-be design (the agent API diffs ERC/DRC on it).
Proposals park a command until someone accepts it; acceptance re-plans against the current design.

## Rendering

The editors draw with egui's painter through `draw::Canvas`. Per design revision the UI caches
the netlist, board connectivity, pad geometry, object bounds and `GridIndex` spatial indexes
(`ui/src/cache.rs`); nothing derived is rebuilt per frame. Hit testing queries the index, and
drawing culls objects outside the viewport. Concave regions (board, pours) are filled by ear
clipping. A wgpu paint callback with GPU-batched layer meshes is the planned next step for very
large boards (see ROADMAP).

## Agent transport

The GUI binds `127.0.0.1:7272` (configurable, `0` disables) and serves the control protocol on
its UI thread, so agent commands share the user's undo history and appear immediately.
`hephaestus-mcp` speaks MCP over stdio and either forwards to that port (default when the GUI runs)
or owns a headless `Session` (`--project`, `--demo`, `--headless`). Optional shared-secret
authentication: `HEPHAESTUS_TOKEN`. See AGENT_API.md.

## Persistence

See `crates/project/src/lib.rs`. A project is a folder of pretty JSON documents (manifest,
components, library, board, one file per sheet), each with `format` and `version`. Loading
migrates older versions and refuses newer ones; saving rewrites only changed documents, each
atomically, manifest last. Autosave writes a bundle under `.autosave/`; a newer autosave than
the manifest triggers the recovery prompt.

## Reuse from Vector Craft

Vector Craft's `ui-egui` crate is tied to its illustration engine and was not copied. Hephaestus
depends on the suite's shared **craft-ui** crate (MIT OR Apache-2.0, same egui 0.36, pinned to
the revision Vector Craft uses) for: transactional docking (`DockArea`, serde layouts, floating
groups, drag/drop), accessible tab fitting (`tabs::fit`, `Tab`), icon buttons and widget
palettes. Patterns adopted (not code): the no-panic lint policy, a JSON-lines control port on the
GUI thread for agents, and screenshot-driven UI validation. egui_tiles was evaluated: its current
release requires egui 0.37 and would duplicate craft-ui's docking.
