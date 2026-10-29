//! Manufacturing outputs.
//!
//! - [`gerber`]: Gerber X2 (RS-274X with attributes), one file per copper, overlay, solder mask
//!   and paste layer plus the board profile.
//! - [`drill`]: Excellon drill files, plated and non-plated separately.
//! - [`bom`] and [`pick_and_place`]: CSV assembly files.
//!
//! Every exporter returns warnings for anything it could not represent instead of dropping it
//! silently (for example unpoured polygons and silkscreen text, which is not yet stroked).

pub mod bom;
pub mod drill;
pub mod gerber;

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct ExportReport {
    pub files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct ExportError(pub String);

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExportError {}

pub(crate) fn write(dir: &Path, name: &str, contents: &str, report: &mut ExportReport) -> Result<(), ExportError> {
    std::fs::create_dir_all(dir).map_err(|e| ExportError(format!("{}: {e}", dir.display())))?;
    let path = dir.join(name);
    std::fs::write(&path, contents).map_err(|e| ExportError(format!("{}: {e}", path.display())))?;
    report.files.push(path);
    Ok(())
}

pub use bom::{bom, pick_and_place};
