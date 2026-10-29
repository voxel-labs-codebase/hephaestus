//! The agent-facing API of Hephaestus.
//!
//! A [`Session`] wraps the command [`hephaestus_commands::Editor`] with a project location, the last
//! validation reports and agent policy. [`tools::catalog`] describes every operation (name,
//! description, JSON schema) and [`tools::dispatch`] runs one. The GUI and headless servers share
//! this code, so agents see the same behaviour either way, and the GUI shows agent changes as
//! they happen because they go through the same editor.
//!
//! [`control`] is the local transport: newline-delimited JSON over TCP on 127.0.0.1.

pub mod control;
pub mod demo;
mod session;
pub mod tools;
pub mod units;

use serde::Serialize;
use serde_json::{Value, json};

pub use session::{AgentPolicy, Session};

/// A structured error returned to agents.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), details: Value::Null }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_argument", message)
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn to_json(&self) -> Value {
        json!(self)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<hephaestus_commands::CommandError> for ApiError {
    fn from(e: hephaestus_commands::CommandError) -> Self {
        ApiError::new(e.code(), e.to_string()).with_details(json!(e))
    }
}

impl From<hephaestus_project::ProjectError> for ApiError {
    fn from(e: hephaestus_project::ProjectError) -> Self {
        ApiError::new("project_io", e.to_string())
    }
}

impl From<hephaestus_export::ExportError> for ApiError {
    fn from(e: hephaestus_export::ExportError) -> Self {
        ApiError::new("export_failed", e.to_string())
    }
}
