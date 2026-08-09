//! # Platform context
//!
//! A supporting bounded context: the durable accountability records every other
//! context leans on — the **audit trail** (consequential decisions, for the
//! person's protection) and the butler's **event log** (what it did and learned,
//! and setting changes). Because these are cross-cutting supporting records,
//! other contexts depend on `platform` to write them; `platform` depends on no
//! other context. See `docs/adr/0026-package-by-bounded-context.md`.
//!
//! Layered internally, dependencies pointing inward: [`domain`] (pure model),
//! [`application`] (ports + use cases), [`infrastructure`] (SQLite adapters). The
//! HTTP interface for `/v1/audit` is mounted by the composition root.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{ActivityEvent, AuditLog, EventLog, recent_audit};
pub use domain::AuditRecord;
pub use infrastructure::{AuditStore, EventStore, migrate};
