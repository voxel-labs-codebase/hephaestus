//! Bill of materials and pick-and-place CSV files.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use hephaestus_core::{Design, PcbKind, Side};

use crate::{ExportError, ExportReport, write};

fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n']) { format!("\"{}\"", field.replace('"', "\"\"")) } else { field.to_string() }
}

fn natural(designator: &str) -> (String, u64) {
    let split = designator.find(|c: char| c.is_ascii_digit()).unwrap_or(designator.len());
    let (prefix, number) = designator.split_at(split);
    (prefix.to_string(), number.parse().unwrap_or(u64::MAX))
}

/// One BOM line per (part, value, footprint), designators listed in natural order.
pub fn bom_rows(design: &Design) -> Vec<BTreeMap<&'static str, String>> {
    let mut groups: BTreeMap<(String, String, String), Vec<&hephaestus_core::Component>> = BTreeMap::new();
    for c in design.components.values() {
        groups.entry((c.part.clone(), c.value.clone(), c.footprint.clone())).or_default().push(c);
    }
    let mut rows = Vec::new();
    for ((part, value, footprint), mut components) in groups {
        components.sort_by_key(|c| natural(&c.designator));
        let def = design.library.parts.get(&part);
        let param = |key: &str| {
            components
                .first()
                .and_then(|c| c.parameters.get(key))
                .or_else(|| def.and_then(|d| d.parameters.get(key)))
                .cloned()
                .unwrap_or_default()
        };
        let mut row = BTreeMap::new();
        row.insert("Comment", value);
        row.insert("Description", def.map(|d| d.description.clone()).unwrap_or_default());
        row.insert("Designator", components.iter().map(|c| c.designator.as_str()).collect::<Vec<_>>().join(", "));
        row.insert("Footprint", footprint);
        row.insert("LibRef", part);
        row.insert("Quantity", components.len().to_string());
        // A part number set on the placed component wins, then the library component's own
        // (or preferred approved) part.
        let sourcing = def.and_then(|d| d.sourcing());
        let pick = |key: &str, from_library: Option<&str>| {
            let own = components.first().and_then(|c| c.parameters.get(key)).cloned();
            own.or(from_library.filter(|s| !s.is_empty()).map(String::from)).unwrap_or_else(|| param(key))
        };
        row.insert("Manufacturer", pick("Manufacturer", sourcing.map(|s| s.0)));
        row.insert("Manufacturer Part Number", pick("Manufacturer Part Number", sourcing.map(|s| s.1)));
        rows.push(row);
    }
    rows.sort_by_key(|r| r.get("Designator").map(|d| natural(d.split(',').next().unwrap_or(""))).unwrap_or_default());
    rows
}

const BOM_COLUMNS: [&str; 8] =
    ["Comment", "Description", "Designator", "Footprint", "LibRef", "Quantity", "Manufacturer", "Manufacturer Part Number"];

pub fn bom_csv(design: &Design) -> String {
    let mut out = BOM_COLUMNS.join(",");
    out.push('\n');
    for row in bom_rows(design) {
        let line: Vec<String> = BOM_COLUMNS.iter().map(|k| csv(row.get(k).map(String::as_str).unwrap_or(""))).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

pub fn bom(design: &Design, dir: &Path) -> Result<ExportReport, ExportError> {
    let mut report = ExportReport::default();
    write(dir, &format!("{}-BOM.csv", design.name), &bom_csv(design), &mut report)?;
    Ok(report)
}

/// Centroid file in Altium's column layout, millimetres relative to the board origin.
pub fn pick_and_place_csv(design: &Design) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let mut rows = Vec::new();
    for c in design.components.values() {
        let Some((_, fp)) = design.footprint_of(c.id) else {
            warnings.push(format!("{} has no footprint on the board and is not in the pick-and-place file", c.designator));
            continue;
        };
        let origin = design.board.origin;
        let x = (fp.position.x - origin.x) as f64 / 1e6;
        let y = (fp.position.y - origin.y) as f64 / 1e6;
        let side = if fp.side == Side::Top { "TopLayer" } else { "BottomLayer" };
        let description = design.library.parts.get(&c.part).map(|p| p.description.clone()).unwrap_or_default();
        rows.push((
            natural(&c.designator),
            format!(
                "{},{},{},{},{x:.4},{y:.4},{:.2},{}",
                csv(&c.designator),
                csv(&c.value),
                side,
                csv(&fp.footprint),
                fp.rotation,
                csv(&description)
            ),
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from("Designator,Comment,Layer,Footprint,Center-X(mm),Center-Y(mm),Rotation,Description\n");
    for (_, line) in rows {
        let _ = writeln!(out, "{line}");
    }
    let mechanical = design.pcb.values().filter(|o| matches!(&o.kind, PcbKind::Footprint(f) if f.component.is_none())).count();
    if mechanical > 0 {
        warnings.push(format!("{mechanical} mechanical footprint(s) without a component are not placed by assembly and were left out"));
    }
    (out, warnings)
}

pub fn pick_and_place(design: &Design, dir: &Path) -> Result<ExportReport, ExportError> {
    let mut report = ExportReport::default();
    let (text, warnings) = pick_and_place_csv(design);
    report.warnings = warnings;
    write(dir, &format!("{}-PickPlace.csv", design.name), &text, &mut report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hephaestus_commands::{Command, Editor, Source};
    use hephaestus_core::geometry::{Point, mil};

    fn design() -> Design {
        let mut e = Editor::new(Design::new("Bom"), vec![hephaestus_libraries::builtin()]);
        for (part, x) in [("RES", 0.0), ("RES", 1000.0), ("CAP", 2000.0), ("RES", 3000.0)] {
            e.execute(
                &Command::PlaceComponent {
                    part: part.into(),
                    sheet: None,
                    position: Point::new(mil(x), 0),
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
        }
        e.execute(&Command::UpdatePcb {}, Source::User).unwrap();
        e.design().clone()
    }

    #[test]
    fn bom_groups_identical_parts() {
        let text = bom_csv(&design());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[2].contains("\"R1, R2, R3\""), "{text}");
        assert!(lines[2].contains(",3,"));
    }

    #[test]
    fn pick_and_place_lists_every_placed_component() {
        let (text, warnings) = pick_and_place_csv(&design());
        assert_eq!(text.lines().count(), 5);
        assert!(warnings.is_empty());
        assert!(text.contains("R1,10k,TopLayer,0603,"));
    }

    #[test]
    fn gerber_and_drill_files_are_written() {
        let dir = std::env::temp_dir().join(format!("hephaestus-gerber-{}", std::process::id()));
        let d = design();
        let report = crate::gerber::export(&d, &dir).unwrap();
        assert_eq!(report.files.len(), 9);
        let top = std::fs::read_to_string(dir.join("Bom.GTL")).unwrap();
        assert!(top.starts_with("%TF.GenerationSoftware,Hephaestus"));
        assert!(top.contains("%FSLAX46Y46*%"));
        assert!(top.trim_end().ends_with("M02*"));
        assert!(top.contains("G36*"), "rounded-rectangle pads are regions");
        let drill = crate::drill::export(&d, &dir).unwrap();
        assert_eq!(drill.files.len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
