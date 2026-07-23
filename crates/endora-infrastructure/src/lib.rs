//! # Endora infrastructure layer
//!
//! Adapters that implement the [`endora_application`] ports. This crate depends
//! inward on the application and domain abstractions and provides concrete
//! technology — here, SQLite-backed persistence (see
//! `docs/adr/0004-sqlite-first.md`). Nothing above the infrastructure layer
//! names SQLite; callers hold the port traits.

#![forbid(unsafe_code)]

pub mod butler;
pub mod clock;
pub mod ids;
pub mod model;
pub mod model_layer;
pub mod sqlite;

pub use butler::{
    ConfigurableButler, DeepModelAsker, LlmButler, MixtureButler, ScriptedButler, ask_deep_model,
    butler_from_config, list_models, test_connection, transcribe_audio,
};
pub use model_layer::{
    AdoptionDecision, AdoptionOutcome, CaseResult, ModelCandidate, Scorecard, ScoredCandidate,
    decide_adoption, evaluate, is_local, run_model_layer,
};
// The capabilities context owns the skills, runner, and egress guard (ADR 0026);
// re-exported so `endora_infrastructure::{RegistryRunner, …}` paths are unchanged.
pub use clock::SystemClock;
pub use endora_capabilities::{
    Capability, CapabilityError, CapabilityInfo, CapabilitySettings, RegistryRunner, SettingSpec,
    default_capabilities, redact_pii_in_value, scan_outbound_secret,
};
pub use ids::RandomIdSource;
pub use model::OpenAiCompatibleProposer;
pub use sqlite::SqliteStore;
