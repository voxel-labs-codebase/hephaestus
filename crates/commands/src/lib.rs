//! The only way to change a design.
//!
//! A [`Command`] is planned against a scratch copy of the design into a [`ChangeSet`] of
//! primitive, invertible [`Edit`]s. The [`Editor`] validates the result (no new integrity
//! problems), applies it, and records it for undo/redo. Transactions group several commands into
//! one undo step; dry runs return the change without applying it; proposals park a change until
//! someone approves it. The UI and external agents share this path, so neither can bypass
//! validation and both see each other's changes in the history.

mod command;
mod edit;
mod editor;
mod planner;

pub use command::{AlignMode, Axis, Command, ComponentRef, ConnectStyle, PinRef};
pub use edit::{Change, ChangeSet, Edit};
pub use editor::{Editor, HistoryEntry, LogEntry, LogKind, Outcome, Proposal, Source};
pub use planner::{Planner, route_45};

use hephaestus_core::ObjectId;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, thiserror::Error)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum CommandError {
    #[error("object {0} does not exist")]
    UnknownObject(ObjectId),
    #[error("{0} is locked")]
    Locked(ObjectId),
    #[error("{0}")]
    Invalid(String),
    #[error("the design changed underneath this edit: {0}")]
    Conflict(String),
    #[error("command {index} of the batch failed: {error}")]
    Batch { index: usize, error: Box<CommandError> },
    #[error("the change would leave the design inconsistent: {}", .0.join("; "))]
    Integrity(Vec<String>),
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error("a transaction is already open")]
    TransactionOpen,
    #[error("no transaction is open")]
    NoTransaction,
    #[error("unknown proposal {0}")]
    UnknownProposal(u64),
}

impl CommandError {
    /// A stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CommandError::UnknownObject(_) => "unknown_object",
            CommandError::Locked(_) => "locked",
            CommandError::Invalid(_) => "invalid",
            CommandError::Conflict(_) => "conflict",
            CommandError::Batch { .. } => "batch",
            CommandError::Integrity(_) => "integrity",
            CommandError::NothingToUndo => "nothing_to_undo",
            CommandError::NothingToRedo => "nothing_to_redo",
            CommandError::TransactionOpen => "transaction_open",
            CommandError::NoTransaction => "no_transaction",
            CommandError::UnknownProposal(_) => "unknown_proposal",
        }
    }
}

#[cfg(test)]
mod tests;
