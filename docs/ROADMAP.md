# Hephaestus — Roadmap

Status as of 2026-10-29. **Done** means implemented and covered by tests or visual validation;
**Partial** says exactly what is missing; **Planned** is not implemented.

## Phase status

| Phase | Status |
|---|---|
| 1. Repository audit and foundation | Done — audit of Vector Craft (craft-ui reused), workspace, core data model, documentation. |
| 2. Professional application shell | Done — menus, quick access, standard toolbar, Active Bar, document tabs, craft-ui docking with persistence, Projects/Properties/Messages panels, status bar, dark theme. |
| 3. Engineering canvas infrastructure | Done — camera, transforms, adaptive grid, snapping (grid and electrical), Altium navigation, selection boxes, cached spatial hit testing, viewport culling. Partial: GPU-batched rendering (see Performance). |
| 4. Schematic workspace | Done for components, pins, wires, junctions, net labels, power ports, ports, no-ERC, text. Partial: buses and bus entries (data model and rendering only, no tool or bus connectivity), sheet symbols (model, rendering, port-name connectivity; no placement tool). |
| 5. PCB workspace | Done — board outline, layer manager (View Configuration, layer tabs, single-layer mode, layer stack), footprints, pads, tracks, vias, selection, placement, properties. |
| 6. EDA core | Done — connectivity, net management, component↔symbol↔footprint identity, versioned persistence, transactions, undo/redo. |
| 7. Libraries and validation | Done — 20-part built-in library generated in code, project library, library JSON files, ERC, DRC, violation navigation. Partial: no symbol/footprint editors yet. |
| 8. Interactive editing | Done — schematic and PCB editing workflows, interactive routing with clearance preview and layer-change vias, cross-probing/cross-select, copy/paste, align/distribute, lock. Partial: copper pours (outline only, no fill computation). |
| 9. Agent integration | Done — agent API (55 tools), MCP server (rmcp, connected or headless), Python SDK, dry runs, transactions, proposals/approval, change log, structured errors. |
| 10. Advanced capabilities | Partial — manufacturing exports done (Gerber X2, Excellon, BOM, pick-and-place). Planned: 3D view, SPICE, KiCad import/export, Altium import investigation. |

## Current focus: agent part sourcing

The near-term goal is automating part selection, availability and alternatives with agents.

1. **Component library format** — Done: per-project library folder, per-value generic
   components, part numbers and approved parts, origin and status, component packs, agent tools.
   Remaining: shared cache, applying updates from origin, STEP models.
2. **Supplier data** — Planned: DigiKey and Mouser APIs (keys in a local config), search, stock,
   price, lifecycle, alternatives, BOM panel.
3. **LCSC/JLCPCB and KiCad import** — Planned: LCSC numbers, JLC Basic/Extended, JLC BOM and
   pick-and-place export, KiCad symbol and footprint import.

## Known gaps, in priority order

1. **Polygon pour filling.** Pours are stored, drawn and validated as outlines; DRC warns that
   they are unpoured and Gerber export skips them with a warning. Needs polygon clipping
   (clearance subtraction, thermal reliefs, island removal).
2. **KiCad interoperability.** Import of `.kicad_sch` / `.kicad_pcb` / `.kicad_mod` /
   `.kicad_sym` through an adapter crate that maps into the core model and reports every
   unsupported construct. Export of KiCad footprints/symbols afterwards.
3. **GPU rendering path for large boards.** Today the canvases tessellate per frame with egui's
   painter, culled to the viewport and backed by cached spatial indexes; this is comfortable for
   designs of a few thousand objects. Next: per-layer meshes cached per revision and drawn
   through an egui wgpu paint callback.
4. **Silkscreen text in Gerber.** Free text is skipped with a warning; needs a stroke font.
5. **Symbol and footprint editors** (Altium SchLib/PcbLib documents) and library import.
6. **Buses** (tool, bus connectivity with `D[0..7]` expansion) and **hierarchical sheet
   symbols** placement with automatic sheet entries.
7. **Obstacle-avoiding routing** (push-and-shove, walk-around) and length tuning; differential
   pairs. Interactive routing currently previews clearance violations but does not avoid them.
8. **3D board view** (generated body heights are already in the library; STEP loading planned).
9. **SPICE** through an external engine (ngspice) behind a simulation-model interface on parts.
10. **IPC-2581, DXF, STEP** export.
11. Arcs as a primitive for tracks and board outlines (footprint and symbol art already has arcs).
12. Net-class rule scopes beyond width/clearance (via styles, diff pairs), per-layer rules.

## Performance targets

- Hit testing and drawing must stay interactive at 10 000 tracks/pads: spatial index queries and
  viewport culling are in place; measure with a synthetic board before the GPU path lands.
- Netlist compile and board connectivity run once per design revision (cached), not per frame.
