use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use hephaestus_core::{Design, Library, ObjectId};
use serde::Serialize;

use crate::{ChangeSet, Command, CommandError, Planner};

/// Who made a change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum Source {
    User,
    Agent(String),
    Script,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::User => f.write_str("User"),
            Source::Agent(name) => write!(f, "Agent ({name})"),
            Source::Script => f.write_str("Script"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HistoryEntry {
    pub seq: u64,
    pub description: String,
    pub source: Source,
    pub changes: ChangeSet,
    pub time_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Do,
    Undo,
    Redo,
}

/// One line of the change log that agents read to see what happened since they last looked.
#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub seq: u64,
    pub kind: LogKind,
    pub description: String,
    pub source: Source,
    pub touched: Vec<ObjectId>,
    pub time_ms: u64,
}

/// A change waiting for approval. It is re-planned against the current design when accepted.
#[derive(Clone, Debug, Serialize)]
pub struct Proposal {
    pub id: u64,
    pub command: Command,
    pub source: Source,
    pub description: String,
    /// The edits as planned when proposed (informational; acceptance re-plans).
    pub preview: Vec<String>,
    pub time_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub seq: u64,
    pub description: String,
    pub created: Vec<ObjectId>,
    pub removed: Vec<ObjectId>,
    pub touched: Vec<ObjectId>,
}

struct Transaction {
    name: String,
    source: Source,
    changes: ChangeSet,
}

const LOG_LIMIT: usize = 2000;
const UNDO_LIMIT: usize = 500;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Owns a design and every way of changing it.
pub struct Editor {
    design: Design,
    sources: Vec<Library>,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    log: Vec<LogEntry>,
    seq: u64,
    revision: u64,
    saved_revision: u64,
    transaction: Option<Transaction>,
    proposals: BTreeMap<u64, Proposal>,
    next_proposal: u64,
}

impl Editor {
    /// `sources` are the libraries parts can be placed from (the built-in library and any user
    /// libraries); the design's own project library is always searched first.
    pub fn new(design: Design, sources: Vec<Library>) -> Self {
        Self {
            design,
            sources,
            undo: Vec::new(),
            redo: Vec::new(),
            log: Vec::new(),
            seq: 0,
            revision: 1,
            saved_revision: 1,
            transaction: None,
            proposals: BTreeMap::new(),
            next_proposal: 1,
        }
    }

    pub fn design(&self) -> &Design {
        &self.design
    }

    pub fn sources(&self) -> &[Library] {
        &self.sources
    }

    pub fn add_source(&mut self, library: Library) {
        self.sources.retain(|l| l.name != library.name);
        self.sources.push(library);
    }

    /// Increments on every change to the design (including undo/redo); caches key on it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Replace the whole design (open/new project). History, transactions and proposals reset.
    pub fn replace_design(&mut self, design: Design) {
        self.design = design;
        self.undo.clear();
        self.redo.clear();
        self.transaction = None;
        self.proposals.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
    }

    /// Plan a command without applying it: the change set and the design as it would be.
    pub fn dry_run(&self, command: &Command) -> Result<(ChangeSet, Design), CommandError> {
        let mut planner = Planner::new(&self.design, &self.sources);
        planner.run(command)?;
        let before: BTreeSet<String> = self.design.integrity_problems().into_iter().collect();
        let introduced: Vec<String> = planner.design.integrity_problems().into_iter().filter(|p| !before.contains(p)).collect();
        if !introduced.is_empty() {
            return Err(CommandError::Integrity(introduced));
        }
        let preview = planner.design.clone();
        Ok((planner.finish(command.label()), preview))
    }

    /// Plan, validate and apply a command.
    pub fn execute(&mut self, command: &Command, source: Source) -> Result<Outcome, CommandError> {
        let (changes, _) = self.dry_run(command)?;
        self.apply(changes, source)
    }

    /// Apply an already planned change set (e.g. from [`Self::dry_run`]); it is checked against
    /// the current design and refused if anything changed in between.
    pub fn apply(&mut self, changes: ChangeSet, source: Source) -> Result<Outcome, CommandError> {
        changes.apply(&mut self.design)?;
        self.seq += 1;
        self.revision += 1;
        let outcome = Outcome {
            seq: self.seq,
            description: changes.description.clone(),
            created: changes.created(),
            removed: changes.removed(),
            touched: changes.touched(),
        };
        self.push_log(LogKind::Do, &changes, &source);
        match &mut self.transaction {
            Some(t) => t.changes.append(changes),
            None => {
                if !changes.is_empty() {
                    self.push_undo(HistoryEntry {
                        seq: self.seq,
                        description: changes.description.clone(),
                        source,
                        changes,
                        time_ms: now_ms(),
                    });
                }
            }
        }
        Ok(outcome)
    }

    fn push_undo(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn push_log(&mut self, kind: LogKind, changes: &ChangeSet, source: &Source) {
        self.log.push(LogEntry {
            seq: self.seq,
            kind,
            description: changes.description.clone(),
            source: source.clone(),
            touched: changes.touched(),
            time_ms: now_ms(),
        });
        if self.log.len() > LOG_LIMIT {
            self.log.drain(..self.log.len() - LOG_LIMIT);
        }
    }

    pub fn can_undo(&self) -> bool {
        self.transaction.is_none() && !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        self.transaction.is_none() && !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.description.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.description.as_str())
    }

    pub fn undo(&mut self, source: Source) -> Result<String, CommandError> {
        if self.transaction.is_some() {
            return Err(CommandError::TransactionOpen);
        }
        let Some(entry) = self.undo.pop() else { return Err(CommandError::NothingToUndo) };
        let inverse = entry.changes.inverse();
        if let Err(e) = inverse.apply(&mut self.design) {
            self.undo.push(entry);
            return Err(e);
        }
        self.seq += 1;
        self.revision += 1;
        self.push_log(LogKind::Undo, &inverse, &source);
        let description = entry.description.clone();
        self.redo.push(entry);
        Ok(description)
    }

    pub fn redo(&mut self, source: Source) -> Result<String, CommandError> {
        if self.transaction.is_some() {
            return Err(CommandError::TransactionOpen);
        }
        let Some(entry) = self.redo.pop() else { return Err(CommandError::NothingToRedo) };
        if let Err(e) = entry.changes.apply(&mut self.design) {
            self.redo.push(entry);
            return Err(e);
        }
        self.seq += 1;
        self.revision += 1;
        self.push_log(LogKind::Redo, &entry.changes, &source);
        let description = entry.description.clone();
        self.undo.push(entry);
        Ok(description)
    }

    /// Open a transaction: commands apply immediately (the UI shows them) but undo as one step,
    /// and [`Self::rollback`] discards them all.
    pub fn begin(&mut self, name: &str, source: Source) -> Result<(), CommandError> {
        if self.transaction.is_some() {
            return Err(CommandError::TransactionOpen);
        }
        self.transaction = Some(Transaction {
            name: name.to_string(),
            source,
            changes: ChangeSet { description: name.to_string(), ..Default::default() },
        });
        Ok(())
    }

    pub fn transaction_name(&self) -> Option<&str> {
        self.transaction.as_ref().map(|t| t.name.as_str())
    }

    pub fn commit(&mut self) -> Result<Outcome, CommandError> {
        let Some(t) = self.transaction.take() else { return Err(CommandError::NoTransaction) };
        let outcome = Outcome {
            seq: self.seq,
            description: t.name.clone(),
            created: t.changes.created(),
            removed: t.changes.removed(),
            touched: t.changes.touched(),
        };
        if !t.changes.is_empty() {
            self.push_undo(HistoryEntry { seq: self.seq, description: t.name, source: t.source, changes: t.changes, time_ms: now_ms() });
        }
        Ok(outcome)
    }

    pub fn rollback(&mut self) -> Result<(), CommandError> {
        let Some(t) = self.transaction.take() else { return Err(CommandError::NoTransaction) };
        let inverse = t.changes.inverse();
        if let Err(e) = inverse.apply(&mut self.design) {
            self.transaction = Some(t);
            return Err(e);
        }
        self.seq += 1;
        self.revision += 1;
        self.push_log(LogKind::Undo, &inverse, &t.source);
        Ok(())
    }

    /// Park a command for approval after checking that it plans cleanly now.
    pub fn propose(&mut self, command: Command, source: Source) -> Result<u64, CommandError> {
        let (changes, _) = self.dry_run(&command)?;
        let id = self.next_proposal;
        self.next_proposal += 1;
        self.proposals
            .insert(id, Proposal { id, description: command.label(), preview: changes.summary(), command, source, time_ms: now_ms() });
        self.revision += 1;
        Ok(id)
    }

    pub fn proposals(&self) -> impl Iterator<Item = &Proposal> {
        self.proposals.values()
    }

    pub fn accept(&mut self, id: u64) -> Result<Outcome, CommandError> {
        let Some(p) = self.proposals.remove(&id) else { return Err(CommandError::UnknownProposal(id)) };
        match self.execute(&p.command, p.source.clone()) {
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                self.proposals.insert(id, p);
                Err(e)
            }
        }
    }

    pub fn reject(&mut self, id: u64) -> Result<Proposal, CommandError> {
        let p = self.proposals.remove(&id).ok_or(CommandError::UnknownProposal(id))?;
        self.revision += 1;
        Ok(p)
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.undo
    }

    /// Log entries with `seq` greater than `after`.
    pub fn log_since(&self, after: u64) -> Vec<&LogEntry> {
        self.log.iter().filter(|e| e.seq > after).collect()
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }
}
