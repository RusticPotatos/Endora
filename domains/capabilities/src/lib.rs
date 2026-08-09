//! # Capabilities context
//!
//! The butler's **skills** and everything that governs running them: the skill
//! catalog and the [`RegistryRunner`] that lists and executes them behind the
//! policy gate (models propose, policy authorizes — ADRs 0019/0020), the egress
//! guard (SSRF protection + outbound-secret tripwire, ADR 0051), the autonomy
//! envelope (ADR 0051), and the ports for a skill's settings, enablement, and the
//! optional deep-model escalation.
//!
//! Layered inward: [`domain`] (the pure [`AutonomyEnvelope`]), [`application`]
//! (ports), [`infrastructure`] (the runner, the concrete skills, the egress
//! guard). Depends only on the shared kernel. See ADR 0050.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod home_assistant;
pub mod infrastructure;
pub mod mcp_http;
pub mod mcp_registry;
pub mod mcp_stdio;
pub mod store;
pub mod target_search;

pub use application::{
    AutonomyEnvelope, AutonomyEnvelopeRepository, ButlerModelConfig, ButlerModelConfigRepository,
    CapabilityConfigRepository, CapabilityRunner, CapabilitySettingsRepository, CapabilitySpec,
    CapabilityUse, ConfigWrite, ConfigWriteLog, DeepModel, DeepModelRepository,
    KEEP_TRANSITIONS_FOR_MS, McpServer, McpServerRegistry, McpTransport, ModelSlot,
    ModelTuneSchedule, ModelTuneScheduleRepository, Recipe, RecipeInput, RecipeInputKind,
    RecipeRepository, Sampling, Stance, StandingTrouble, StandingTroubleRepository, TargetAlias,
    TargetAliasRepository, TransitionLog, WriteKind, not_answering, watch_for_change,
    watch_for_trouble, worth_raising, worth_waking_for,
};
pub use domain::{DWELL_MS, SetupField, SetupForm, Transition, Watched};
pub use home_assistant::{HomeAssistant, paired_server};
pub use infrastructure::{
    AliasRunner, Capability, CapabilityError, CapabilityInfo, CapabilitySettings, CompositeRunner,
    McpClient, McpResource, McpRunner, McpToolInfo, NativeChannel, OpenerRunner, RecipeRunner,
    RegistryRunner, ReversibleOnlyRunner, SettingSpec, TargetSearchRunner, WithdrawnRunner,
    arguments_for_a_test_call, channels_of, default_capabilities, default_stance, place_filled_in,
    redact_pii_in_text, redact_pii_in_value, same_call_as, scan_outbound_secret, settings_complete,
    tools_the_toggle_governs, tools_to_open_on_connect,
};
pub use mcp_http::HttpMcpClient;
pub use mcp_stdio::StdioMcpClient;
pub use store::{ConfigStore, migrate};
pub use target_search::{
    Candidate, candidates, is_fragment_of, only_real_match, retarget, shortlist, target_fields,
    target_words, target_words_with_kinds,
};
