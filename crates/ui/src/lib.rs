//! The Hephaestus desktop interface (egui).
//!
//! The UI is a client of the engineering core: panels and editors read the design through the
//! agent-api [`hephaestus_agent_api::Session`] and change it only by executing validated commands.
//! The same session answers agent requests on the UI thread, so agent changes appear live.

pub mod actions;
pub mod app;
mod cache;
mod commands;
mod dialogs;
mod documents;
mod draw;
pub mod icons;
mod panels;
mod pcb;
mod sch;
mod shell;
pub mod theme;
pub mod workspace;

pub use app::{HephaestusApp, StartOptions};

#[cfg(test)]
mod tests;
