//! Ports the application defines and the infrastructure layer implements.
//!
//! These are the abstractions the application depends on for persistence. The
//! application never names a concrete storage engine; infrastructure provides
//! adapters (e.g. SQLite — see `docs/adr/0004-sqlite-first.md`). This keeps the
//! dependency direction pointing inward: `Infrastructure -> Application/Domain`.

use endora_platform::{ActivityEvent, AuditRecord};

/// Endora's record of what it did, for the memory rights of the constitution: it is what
/// "export" hands back and what "delete" removes. Since the chat was retired (ADR 0136) the
/// record is the audit trail and the event log; HQ keeps the team's work in its own store.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemorySnapshot {
    /// The full audit trail.
    pub audit: Vec<AuditRecord>,
    /// Endora's own action log, and the person's setting changes.
    pub events: Vec<ActivityEvent>,
}

/// The user's right to export and delete Endora's record (constitution: memory must be
/// exportable and deletable).
pub trait MemoryStore {
    /// Collects the record into a [`MemorySnapshot`].
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn export(&self) -> Result<MemorySnapshot, RepositoryError>;

    /// Permanently deletes the record.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn purge(&self) -> Result<(), RepositoryError>;
}

// `RepositoryError` is the shared persistence-failure vocabulary; it lives in the
// kernel so the shared `Db` handle and every context's repositories speak it. See
// ADR 0050. Re-exported here so `ports::RepositoryError` paths are unchanged.
pub use endora_kernel::RepositoryError;

/// What one model call used, as the provider reported it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelUsage {
    /// Tokens sent in (cached ones included).
    pub prompt_tokens: u64,
    /// Tokens generated.
    pub completion_tokens: u64,
    /// Of the prompt, how many the provider served from its cache (cheaper), when it says.
    pub cached_tokens: u64,
}

/// A running total of model calls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageTotals {
    /// How many calls.
    pub calls: u64,
    /// Prompt tokens across them.
    pub prompt_tokens: u64,
    /// Generated tokens across them.
    pub completion_tokens: u64,
    /// Cached prompt tokens across them.
    pub cached_tokens: u64,
}

/// Somewhere to record what a paid model's calls used (ADR 0091): the team's relay records
/// each call it makes. Recording must never fail a call, so it returns nothing.
pub trait UsageLog: Send + Sync {
    /// Notes one call by `role` (for example `"team"`) to `model`.
    fn record(&self, role: &str, model: &str, usage: ModelUsage);
}

// `Clock` and `IdSource` are shared-kernel ports (time and identity enter the
// pure layers through them), re-exported here so `endora_application::{Clock,
// IdSource}` and `ports::…` paths are unchanged. See ADR 0050.
pub use endora_kernel::{Clock, IdSource};
