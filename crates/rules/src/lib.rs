//! Design validation on the structured design data.
//!
//! [`erc::check`] runs the electrical rule check on the compiled schematic and [`drc::check`]
//! the design rule check on the board. Both return [`Violation`]s that name the objects involved
//! and where they are, so the Messages panel and agents can navigate to them.

pub mod drc;
pub mod erc;

use hephaestus_core::geometry::Point;
use hephaestus_core::{Layer, ObjectId};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
            Severity::Info => "Info",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Erc,
    Drc,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Violation {
    pub check: Check,
    /// The rule's name, e.g. `"Clearance"`, `"Floating input pin"`.
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub objects: Vec<ObjectId>,
    pub location: Option<Point>,
    /// Schematic violations: the sheet to open.
    pub sheet: Option<ObjectId>,
    /// PCB violations: the layer involved.
    pub layer: Option<Layer>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub violations: Vec<Violation>,
    pub errors: usize,
    pub warnings: usize,
}

impl Report {
    pub fn new(mut violations: Vec<Violation>) -> Self {
        violations.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.rule.cmp(&b.rule)));
        let errors = violations.iter().filter(|v| v.severity == Severity::Error).count();
        let warnings = violations.iter().filter(|v| v.severity == Severity::Warning).count();
        Self { violations, errors, warnings }
    }

    pub fn is_clean(&self) -> bool {
        self.errors == 0
    }
}
