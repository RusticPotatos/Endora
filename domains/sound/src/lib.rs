//! Endora's sound context (ADR 0103): sources, speakers and the wires between them, each wire
//! with its own level; the groups the wires make; and the mix each group plays.
//!
//! Self-contained on purpose: endora-sound is its own service with its own state, works
//! without Endora, and talks to the Endora node only over its API.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;
