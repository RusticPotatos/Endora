//! # Understanding context
//!
//! What the butler has learned about the person: its **beliefs** (what it
//! currently understands, with confidence and evidence), the person's
//! **preferences** (durable things to keep in mind), and the **outcomes** of what
//! it did — the claim an action made and what Endora then observed (ADR 0035).
//! This is how the butler "gets smarter over time" — accumulated, visible,
//! correctable memory rather than opaque model training (ADR 0010).
//!
//! Layered inward: [`domain`] (Belief, Preference, Outcome — pure) and
//! [`application`] (their repository ports). Persisted suggestions and the
//! butler-turn contract live in the orchestration layer, which composes this
//! context with the others. Depends only on the shared kernel. See ADR 0026.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::{
    BeliefRepository, IntentionRepository, OutcomeRepository, PreferenceRepository,
};
pub use domain::{
    Belief, BeliefKind, BeliefStatus, Confidence, Intention, IntentionState, Outcome, Preference,
    PreferenceKind, Reaction,
};
pub use infrastructure::{UnderstandingStore, migrate};
