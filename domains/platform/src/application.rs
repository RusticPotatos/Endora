//! Platform application layer — ports and use cases for audit and events.

use endora_kernel::ids::Timestamp;
use endora_kernel::{AppError, RepositoryError};

use crate::domain::AuditRecord;

/// Persists and reads the audit trail.
///
/// The audit log is append-only from the application's point of view: records
/// are added, never mutated. It is subject to the same memory rights as other
/// stored data (visible, exportable, deletable).
pub trait AuditLog {
    /// Appends a record to the trail.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn append(&self, record: &AuditRecord) -> Result<(), RepositoryError>;

    /// Returns the most recent records, newest first, up to `limit`.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn recent(&self, limit: usize) -> Result<Vec<AuditRecord>, RepositoryError>;
}

/// One entry in the butler's event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEvent {
    /// When it happened.
    pub at: Timestamp,
    /// A plain-language line ("Used the weather skill", "Turned news off").
    pub summary: String,
}

/// Persists and reads the butler's event log — an append-only record of what the
/// butler did and learned each turn, and the person's setting changes.
pub trait EventLog {
    /// Records one event at the given time.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn record(&self, at: Timestamp, summary: &str) -> Result<(), RepositoryError>;

    /// Returns the most recent events, newest first, up to `limit`.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn recent(&self, limit: usize) -> Result<Vec<ActivityEvent>, RepositoryError>;
}

/// Returns the most recent audit records, newest first, up to `limit`.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn recent_audit(audit: &impl AuditLog, limit: usize) -> Result<Vec<AuditRecord>, AppError> {
    Ok(audit.recent(limit)?)
}
