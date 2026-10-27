//! What the engine needs from the store for the engineering wing (ADR 0115): documents, branches
//! and their checks.

use super::ports::Stored;
use crate::domain::{Branch, Check, Document, TaskId};

/// The wing's part of HQ's store.
pub trait WingStore {
    /// Adds a document to a task (ADR 0115; its `id` is ignored) and returns its id.
    fn add_document(&self, document: &Document) -> Stored<i64>;
    /// A document, by its id.
    fn document(&self, id: i64) -> Stored<Option<Document>>;
    /// A task's documents, oldest first.
    fn documents_of(&self, task: TaskId) -> Stored<Vec<Document>>;
    /// Records a submitted branch (its `id` is ignored) and returns its id.
    fn add_branch(&self, branch: &Branch) -> Stored<i64>;
    /// Every submitted branch, oldest first.
    fn branches(&self) -> Stored<Vec<Branch>>;
    /// Saves a branch's status.
    fn save_branch(&self, branch: &Branch) -> Stored<()>;
    /// Queues a check of a branch (its `id` and `token` are ignored): returns its id and the
    /// job's own token, made by the store.
    fn add_check(&self, check: &Check) -> Stored<(i64, String)>;
    /// Every check, oldest first.
    fn checks(&self) -> Stored<Vec<Check>>;
    /// Saves a check's status, result and times.
    fn save_check(&self, check: &Check) -> Stored<()>;
}
