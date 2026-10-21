//! # Endora infrastructure layer
//!
//! Adapters that implement the [`endora_application`] ports. This crate depends
//! inward on the application and domain abstractions and provides concrete
//! technology — here, SQLite-backed persistence (see
//! `docs/adr/0004-sqlite-first.md`). Nothing above the infrastructure layer
//! names SQLite; callers hold the port traits.

#![forbid(unsafe_code)]

pub mod clock;
pub mod hq;
pub mod ids;
pub mod model_server;
pub mod model_watch;
pub mod openbao;
pub mod relay;
pub mod sound;
pub mod sqlite;
pub mod time_zone;
pub mod vault_store;

pub use clock::SystemClock;
pub use model_server::{
    is_private_endpoint, list_models, ollama_loaded, reaches_only_the_house, test_connection,
};
// The capabilities context owns the skills and the egress guard (ADR 0050); re-exported so
// `endora_infrastructure::{Capability, …}` paths are unchanged.
pub use endora_capabilities::{
    Capability, CapabilityError, CapabilityInfo, CapabilitySettings, SettingSpec,
    default_capabilities, redact_pii_in_value, scan_outbound_secret,
};
pub use hq::{HqClient, HqError, Passed, Passing};
pub use ids::RandomIdSource;
pub use sound::{Method as SoundMethod, SoundClient, SoundError};
pub use sqlite::{SECRET_COLUMNS, SqliteStore, forget_sealed_secrets};
pub use time_zone::TimeZone;
