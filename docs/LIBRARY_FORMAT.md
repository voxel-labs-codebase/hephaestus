# Hephaestus — Component Library Format

Status: implemented 2026-10-29 as project format version 2 (`crates/project/src/library_store.rs`,
`crates/core/src/library.rs`, `crates/core/src/hash.rs`, `crates/core/src/library_check.rs`).
**Not yet built:** the shared cache, STEP model files (`models/`), and "update from origin"
(updates are detected and reported, not yet applied). The sections below describe the format as
built; where the reviewed draft differed, the change is noted.

## Goals

1. **Users see components, nothing else.** A component has a schematic symbol, a footprint and a
   3D model, plus its parameters and sourcing data. Shared building blocks, hashes and origins
   are storage details the UI never asks the user to manage (like Altium's Integrated Library:
   you never re-link anything).
2. **Every project owns its library** (like `node_modules`): a project folder opens with nothing
   else installed, and editing a component in one project never changes another.
3. **Shared shapes are stored once.** Hundreds of resistors and capacitors share a handful of
   footprints (0402, 0603 …) and symbols; SOT-23, SOIC-8 etc. are shared across many parts.
4. **Every component knows where it came from** (origin, version, content hash), so changes are
   detected and updates are offered, like a lockfile.
5. **Ready for sourcing:** manufacturer part numbers, DigiKey/Mouser/LCSC numbers, approved
   alternates and generic parts that agents fill with real part numbers.

## What the user sees

```
Components panel / project tree
└─ Library (12 components)
   ├─ R 10k 1% 0603                 generic · 3 approved MPNs
   │    Schematic  Footprint  3D       ← three preview tabs, editable
   ├─ C 100nF 50V X7R 0603           generic · 2 approved MPNs
   ├─ NE555DR                        TI · SOIC-8 · DigiKey / Mouser / LCSC
   └─ LED Red 0805                   generic · 1 approved MPN
```

- Each component shows **Schematic**, **Footprint** and **3D** views and a properties sheet
  (parameters, part numbers, suppliers, alternates).
- (With the symbol/footprint editors, planned.) Editing a footprint that other components share asks once: *"Used by 9 components in this
  project — change all, or only this one?"* "Only this one" silently makes a private copy.
- Copying, exporting or importing a component is one action; whatever it needs comes along.
- Status badges replace any notion of links: *modified locally*, *update available*,
  *missing 3D model*, *no part number*.

## Naming in code

| Concept | User sees | Code |
|---|---|---|
| Library entry | Component | `PartDef` (name kept to avoid churn across crates) |
| Placed instance (R1) | Component R1 | `Component` |
| Schematic shape | Schematic | `SymbolDef` |
| PCB shape | Footprint | `FootprintDef` |
| Body | 3D model | `Model3d` inside the footprint (generated box today) |

## Families and per-value components

Built-in generic parts are *families* (`RES`, `CAP`, `LED`, `HDR1X2` …). Placing one creates a
project component for that value and footprint, named `<family>_<value>_<footprint>`
(`RES_10k_0603`, shown as **R 10k 0603**), because part numbers depend on the value. Changing a
placed resistor's value or footprint moves it to the matching component (created on first
use); an auto-made component nobody uses any more is removed unless it carries part numbers or
local edits. The project library therefore lists exactly the parts of the BOM. Specific parts
(`NE555`, `NE555P`, `MMBT3904` …) are imported as they are.

## On-disk layout

```
MyProject/
  MyProject.pcbprj
  components.json                  placed components (R1, U1 …) → library component ids
  sheets/*.schdoc.json
  board.pcbdoc.json
  library/
    components/                    one file per component, human-readable names
      R_10k_1%_0603.pcbcomp.json
      NE555DR.pcbcomp.json
    symbols/   <hash>.json         shared, named by content hash
    footprints/<hash>.json
    (models/   <hash>.step         3D models (STEP), planned; generated bodies live in the footprint)
```

Component files keep readable names so the folder is browsable and diffs are meaningful; shared
shapes use hash names (full sha256 hex) because their identity *is* their content. Keys inside
files are written in sorted order. Replaces `library.pcblib.json` (see Migration).

## Component file

```json
{
  "format": "hephaestus-component",
  "version": 2,
  "name": "RES_10k_0603",
  "description": "Resistor, thick film",
  "category": "Passives/Resistors",
  "designator_prefix": "R",
  "default_value": "10k",
  "kind": "generic",
  "family": "RES",
  "symbol": "sha256:a1b2…",
  "footprint": "sha256:c3d4…",
  "alt_footprints": [],
  "pin_map": {},
  "parameters": {},
  "approved": [
    { "manufacturer": "YAGEO", "mpn": "RC0603FR-0710KL",
      "suppliers": { "digikey": "311-10.0KHRCT-ND", "mouser": "603-RC0603FR-0710KL", "lcsc": "C25804" },
      "jlc_basic": true, "checked": "2026-10-29" }
  ],
  "origin": { "source": "Hephaestus Basic", "item": "RES", "version": "2", "hash": "sha256:…" },
  "revision": 1,
  "hash": "sha256:…"
}
```

Empty optional fields (`family`, `manufacturer`, `mpn`, `approved`, `origin`, `modified`) are
omitted. A specific component has `"kind": "specific"` with `manufacturer` and `mpn`.

- **`kind`**: `generic` (spec only — "10 kΩ 1 % 0603"; agents choose MPNs) or `specific`
  (one exact part, `manufacturer` and `mpn` set). Generic is the norm for passives, discretes
  in common packages and LEDs; ICs and connectors are specific.
- **`approved`**: manufacturer part numbers allowed for this component, first = preferred. For a
  specific component these are its alternates. Supplier numbers and the JLC Basic flag are
  cached here with a date; live stock and price stay in the sourcing cache, not the project.
- **`pin_map`**: symbol pin → pad numbers (several pads per pin allowed), as today.
- **`origin`**: the source library's name, the item there (for a per-value component, its
  family), the source version, and the item's hash when imported. Absent for components made
  in the project.
- **`hash`**: the component's hash when saved. A file whose content no longer matches it was
  edited by hand and loads as *modified locally*.

## Shared shapes

`symbols/<hash>.json` (`{"format": "hephaestus-symbol", "hash", "symbol": …}`) and
`footprints/<hash>.json` (`"hephaestus-footprint"`, `"footprint": …`) hold `SymbolDef` /
`FootprintDef` unchanged in content. Generated bodies (`Model3d { source, height }`) stay inline
in the footprint; STEP files will go to `models/`. In memory, shapes are keyed by name; two
different shapes with the same name load as `name` and `name_2`.

**Hashing.** `sha256` of the canonical JSON (sorted keys, no whitespace) of the object. A
component's hash covers its own fields plus the hashes of the shapes it references, excluding
`origin`, `modified` and `approved` (sourcing data), so any change to its design data, symbol or
footprint changes it.

**Sharing.** Two components referencing the same hash share that shape file. Files no
component references are removed on save. (Editing shapes in the app arrives with the symbol
and footprint editors; "change all or only this one" is designed for then.)

## Change detection ("lockfile" behaviour)

`check_library` (core) gives each component its badges:

- **modified locally**: edited in this project since import (description, part number,
  parameters, or a hand-edited file). Editing approved parts does not count.
- **update available**: the origin library now has different content for `origin.item` than
  `origin.hash`. Applying updates is not built yet.
- **no part number**, **no 3D model**, **unused**, **broken** (missing symbol, footprint or pad).

## Shared cache (planned)

`~/.local/share/hephaestus/cache/` (platform data dir) holds the same three folders keyed by hash,
plus downloaded components keyed by origin. Importing a component into a project copies it and
its shapes from the cache. Projects never reference the cache directly, so they stay
standalone. Optional later: keep large STEP files only in the cache and embed them on export.

## Moving components

- **Copy between projects**: copies the component file plus any shape files the target lacks.
- **Export**: a component pack, `*.pcbcomp.json` (`"format": "hephaestus-component-pack"`), holding
  one or many components with their symbols and footprints — the Integrated Library experience.
  JSON rather than the drafted zip, since there are no binary models yet.
- **Import**: the reverse; identical shapes are shared, and a name already used by different
  content gets `_2`, `_3` … (Components panel › Import…, or the agent tools).

## Built-in library

Built-in library version 2: generic families `RES`, `CAP`, `CAP_POL`, `IND`, `LED`, `CRYSTAL`,
`TP`, `HDR1X2…8`; specific parts with manufacturer and part number in fields (`NE555` = NE555DR
in SOIC-8, new `NE555P` in DIP-8, `LM358`/`LM358P`, `DIODE`, `NPN`, `NMOS`, `OPAMP_SOT23-5`,
`AMS1117-3.3`, `ATMEGA328P-AU`). Symbols and footprints were already shared by name.

## Migration

Opening a format 1 project (`library.pcblib.json`) converts it in memory; the first save writes
the `library/` folder and renames the old file to `library.pcblib.json.bak`. Part numbers move
from parameters into fields; parts that match a built-in part get its kind and origin (treated
as current); placed components of generic families get their per-value component and the
unused family is dropped. The `.pcbprj` `version` goes from 1 to 2; older builds refuse
version 2 with a clear message rather than misreading it. `examples/led-flasher` is converted.

## Agent tools

`list_library_components`, `check_library`, `update_library_component`, `assign_component`,
`export_library_components`, `import_library_components`; `search_library` and `get_part`
include kind, part numbers, approved parts and origin. See [AGENT_API.md](AGENT_API.md). All
writes go through the command system (undo, dry run, approval).

## Decisions (reviewed 2026-10-29)

1. One main footprint per component plus optional alternatives; "10k 0603" and "10k 0805" are
   two components.
2. 3D models embedded in the project by default (applies once STEP files are supported).
3. Readable component file names.
