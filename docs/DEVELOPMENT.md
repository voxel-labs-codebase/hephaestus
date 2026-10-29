# Hephaestus — Development

## Requirements

Rust 1.90+ (edition 2024). Linux needs the usual winit/wgpu system libraries (Wayland or X11,
Vulkan or GL drivers). No other toolchains are needed; the Python SDK is pure standard library.

## Build and run

```sh
cargo run -p hephaestus                                  # empty project
cargo run -p hephaestus -- --demo                        # LED flasher example in memory
cargo run -p hephaestus -- examples/led-flasher/LED_Flasher.pcbprj
cargo run --release -p hephaestus                         # for large designs
cargo run -p hephaestus-mcp -- --demo                     # headless MCP server on stdio
```

App options: `--port N` (agent port, `0` disables), `--document pcb|SHEET`,
`--select R1,U1`, `--panel NAME`, `--size WxH`, `--screenshot OUT.png [--exit]`,
`--write-demo FOLDER`, `--component NAME[:schematic|footprint|3d]` (show a library component in
the Components panel).

## Quality gates

Run before every commit:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Workspace lints forbid `unwrap()`, `expect()`, `panic!`, `todo!` and `unimplemented!` in shipped
code (tests may use them, `clippy.toml`), and `unsafe`. Files, agent requests and command
parameters are untrusted input: return `Result`, cap sizes, check floats are finite.

## Tests

| Crate | What is tested |
|---|---|
| geometry | units/parsing, rotations, transforms, shape gaps (circles, capsules, rects, concave polygons), spatial index |
| core | pin/pad geometry (rotation, bottom side), integrity checks, JSON round trip |
| libraries | every part resolves, unique on-grid pins, pin maps, search, file round trip |
| connectivity | wires, T-junctions, crossings, labels/power ports, no-connects, dangling ends, pin maps; PCB ratsnest, vias between layers, shorts |
| commands | per-value generic components (create, re-point, prune), library edits and packs, undo/redo exactness, atomic batches, connect/create net, drag-wires, Update PCB, route_net, footprint swap identity, locks, transactions, proposals, change log, JSON commands |
| rules | DRC clean after routing, clearance, width/via/edge violations, ERC floating inputs |
| project | library folder (hash-named shapes, hand-edit detection, stale files), format 1 conversion, component packs, round trip, incremental saves, stale sheet removal, version and path checks, autosave recovery |
| export | BOM grouping, pick-and-place, Gerber/drill files |
| agent-api | tool catalog covers the specification, end-to-end agent flow, dry run/proposals, structured errors, `require_clean`, change visibility, exports, live TCP round trip, demo netlist and DRC |
| canvas | transforms, zoom anchor, fit, window/crossing selection, hot-spot snapping |
| ui | headless egui_kittest interaction tests: wire tool, click/drag move with undo, rubber band, placement tools, part placement with rotation, two-key shortcuts, interactive routing, every panel/dialog renders, cross-select |

## Visual validation

See UI_DESIGN.md › Visual validation procedure. The application renders its own screenshots
(`--screenshot`, `ui.screenshot`), which works on Wayland where external capture tools are
unavailable. Compare against Altium reference material for density, alignment and behaviour.

## Adding things

- **A command:** add the variant to `crates/commands/src/command.rs` (with `label` and
  `is_destructive`), implement it in `Planner::run` using the primitive helpers so it is
  validated and invertible, and test it in `crates/commands/src/tests.rs`.
- **An agent tool:** add a `tool!` entry to `catalog()` and a `dispatch` arm in
  `crates/agent-api/src/tools.rs`; mutations go through `mutate()` so they get dry-run, propose
  and `require_clean`. Document it in AGENT_API.md.
- **A library part:** generate the symbol/footprint in `crates/libraries`, add the part, and the
  library tests check it resolves.
- **A panel:** add a `PanelId`, a module in `crates/ui/src/panels`, and a body arm in
  `shell::Content`.
- **A format version:** bump `FORMAT_VERSION` and add a migration step in
  `crates/project/src/lib.rs::migrate`, with a test loading the old version.

## Repository layout

```
crates/           Rust workspace (see ARCHITECTURE.md)
sdk/python/       Python client (hephaestus package)
examples/         led-flasher project, agent_rc_filter.py
docs/             ARCHITECTURE, UI_DESIGN, ROADMAP, DEVELOPMENT, AGENT_API, screenshots
```
