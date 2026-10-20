//! Use cases the interfaces call: the activity feed, and the owner's rights over Endora's record
//! of what it did (export and delete). They hold no transport or storage detail.

use endora_kernel::Timestamp;
use endora_platform::{AuditLog, EventLog};

use crate::error::AppError;
use crate::ports::{MemorySnapshot, MemoryStore};

/// Exports everything the user has stored (a memory right).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn export_memory(store: &impl MemoryStore) -> Result<MemorySnapshot, AppError> {
    Ok(store.export()?)
}

/// Permanently deletes all of the user's stored data (a memory right).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn purge_memory(store: &impl MemoryStore) -> Result<(), AppError> {
    Ok(store.purge()?)
}

/// Returns the most recent audit records, newest first, up to `limit`.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
// `recent_audit` moved to the platform context (ADR 0050); re-exported so
// `usecases::recent_audit` paths are unchanged during the migration.
pub use endora_platform::recent_audit;

/// What kind of thing an [`ActivityItem`] records.
///
/// Kept coarse on purpose: the feed groups by what sort of event it was, and the
/// human-readable summary carries the specifics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    /// A consequential decision was made and audited (see [`AuditLog`]).
    Decision,
    /// Something Endora did, or a setting the person changed: its own action log (see
    /// [`EventLog`]).
    Action,
}

impl ActivityKind {
    /// A stable, lowercase name, suitable for the protocol and the UI.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Action => "action",
        }
    }
}

/// One entry in the activity feed.
///
/// This is a **read projection**, not a domain aggregate: it merges the
/// persisted facts that already carry a time — audited decisions and Endora's own
/// action log — into a single "what happened" timeline. Because it is derived,
/// it stores nothing new and needs no schema of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityItem {
    at: Timestamp,
    kind: ActivityKind,
    summary: String,
}

impl ActivityItem {
    /// When the recorded event happened.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The area of the loop this entry belongs to.
    #[must_use]
    pub const fn kind(&self) -> ActivityKind {
        self.kind
    }

    /// The human-readable description of what happened.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

/// Returns the most recent activity, newest first, up to `limit` entries.
///
/// The feed is a projection over what is already persisted with a timestamp:
/// audited decisions and Endora's own action log. As more of the system gains
/// durable timestamps, this timeline widens without a protocol change.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn recent_activity(
    audit: &impl AuditLog,
    events: &impl EventLog,
    limit: usize,
) -> Result<Vec<ActivityItem>, AppError> {
    activity_page(audit, events, limit, 0)
}

/// A page of the activity feed: `limit` entries after skipping the newest `offset`, newest
/// first. Each source is read to `offset + limit` entries, which is enough to fill the page
/// from either one however the two interleave.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn activity_page(
    audit: &impl AuditLog,
    events: &impl EventLog,
    limit: usize,
    offset: usize,
) -> Result<Vec<ActivityItem>, AppError> {
    let reach = offset.saturating_add(limit);
    let mut items = Vec::new();
    for r in audit.recent(reach)? {
        items.push(ActivityItem {
            at: r.at(),
            kind: ActivityKind::Decision,
            summary: r.summary().to_owned(),
        });
    }
    for e in events.recent(reach)? {
        items.push(ActivityItem {
            at: e.at,
            kind: ActivityKind::Action,
            summary: e.summary,
        });
    }
    // Newest first; break ties stably so equal timestamps keep a deterministic
    // order across calls.
    items.sort_by(|a, b| {
        b.at.unix_millis()
            .cmp(&a.at.unix_millis())
            .then_with(|| b.summary.cmp(&a.summary))
    });
    items.drain(..offset.min(items.len()));
    items.truncate(limit);
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::{ActivityKind, activity_page};
    use endora_kernel::{AuditId, RepositoryError, Timestamp};
    use endora_platform::{ActivityEvent, AuditLog, AuditRecord, EventLog};

    struct Audit(Vec<AuditRecord>);
    impl AuditLog for Audit {
        fn append(&self, _: &AuditRecord) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn recent(&self, limit: usize) -> Result<Vec<AuditRecord>, RepositoryError> {
            Ok(self.0.iter().take(limit).cloned().collect())
        }
    }
    struct Events(Vec<ActivityEvent>);
    impl EventLog for Events {
        fn record(&self, _: Timestamp, _: &str) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn recent(&self, limit: usize) -> Result<Vec<ActivityEvent>, RepositoryError> {
            Ok(self.0.iter().take(limit).cloned().collect())
        }
    }

    #[test]
    fn the_feed_interleaves_newest_first_and_pages_on() {
        let at = Timestamp::from_unix_millis;
        let audit = Audit(vec![
            AuditRecord::new(AuditId::new(2), at(30), "decided").unwrap(),
            AuditRecord::new(AuditId::new(1), at(10), "decided earlier").unwrap(),
        ]);
        let events = Events(vec![ActivityEvent {
            at: at(20),
            summary: "did".to_owned(),
        }]);
        let first = activity_page(&audit, &events, 2, 0).unwrap();
        let said: Vec<&str> = first.iter().map(|i| i.summary()).collect();
        assert_eq!(said, ["decided", "did"]);
        assert_eq!(first[1].kind(), ActivityKind::Action);
        let rest = activity_page(&audit, &events, 2, 2).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].summary(), "decided earlier");
    }
}
