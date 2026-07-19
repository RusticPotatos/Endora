//! # Endora domain layer
//!
//! This crate holds the **Domain** layer of the Endora modular monolith. It
//! contains only pure concepts and rules of the platform. By construction it
//! has no dependencies and must never depend on:
//!
//! - HTTP, transports, or serialization formats
//! - databases or storage engines
//! - AI vendors or model-specific concepts
//! - user-interface frameworks
//! - operating-system integrations
//!
//! Higher layers (Application, Infrastructure, Interface) depend inward on this
//! crate; this crate depends on nothing. See `docs/architecture.md`.
//!
//! The types here are intentionally minimal. Endora is in a foundation phase:
//! we establish boundaries first and grow the domain one vertical slice at a
//! time (see `docs/domain-map.md`). Speculative entities are deliberately
//! avoided.

#![forbid(unsafe_code)]

pub mod autonomy;

pub use autonomy::AutonomyLevel;
