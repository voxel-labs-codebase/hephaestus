//! Excellon drill files (XNC subset), metric with explicit decimal points so no zero-suppression
//! convention is needed.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use hephaestus_core::geometry::{Coord, Point};
use hephaestus_core::{Design, PcbKind};

use crate::{ExportError, ExportReport, write};

fn mm(v: Coord) -> String {
    format!("{:.3}", v as f64 / 1_000_000.0)
}

fn file(holes: &BTreeMap<Coord, Vec<Point>>, plated: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "M48");
    let _ = writeln!(out, "; DRILL file {{Hephaestus {}}}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "; FORMAT={{-:-/ absolute / metric / decimal}}");
    let _ =
        writeln!(out, "; #@! TF.FileFunction,{},1,2,{}", if plated { "Plated" } else { "NonPlated" }, if plated { "PTH" } else { "NPTH" });
    let _ = writeln!(out, "FMAT,2");
    let _ = writeln!(out, "METRIC");
    for (i, size) in holes.keys().enumerate() {
        let _ = writeln!(out, "T{}C{}", i + 1, mm(*size));
    }
    let _ = writeln!(out, "%");
    let _ = writeln!(out, "G90");
    let _ = writeln!(out, "G05");
    for (i, points) in holes.values().enumerate() {
        let _ = writeln!(out, "T{}", i + 1);
        for p in points {
            let _ = writeln!(out, "X{}Y{}", mm(p.x), mm(p.y));
        }
    }
    let _ = writeln!(out, "M30");
    out
}

/// Write `<name>-PTH.DRL` and `<name>-NPTH.DRL`.
pub fn export(design: &Design, dir: &Path) -> Result<ExportReport, ExportError> {
    let mut report = ExportReport::default();
    let base = design.name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "_");
    let mut plated: BTreeMap<Coord, Vec<Point>> = BTreeMap::new();
    let mut non_plated: BTreeMap<Coord, Vec<Point>> = BTreeMap::new();
    for pad in design.all_pads() {
        if let Some(d) = pad.drill {
            if pad.plated { &mut plated } else { &mut non_plated }.entry(d).or_default().push(pad.center);
        }
    }
    for o in design.pcb.values() {
        if let PcbKind::Via(v) = &o.kind {
            plated.entry(v.drill).or_default().push(v.position);
        }
    }
    for list in plated.values_mut().chain(non_plated.values_mut()) {
        list.sort();
    }
    write(dir, &format!("{base}-PTH.DRL"), &file(&plated, true), &mut report)?;
    write(dir, &format!("{base}-NPTH.DRL"), &file(&non_plated, false), &mut report)?;
    Ok(report)
}
