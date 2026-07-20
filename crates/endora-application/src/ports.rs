//! Ports the application defines and the infrastructure layer implements.
//!
//! These are the abstractions the application depends on for persistence. The
//! application never names a concrete storage engine; infrastructure provides
//! adapters (e.g. SQLite — see `docs/adr/0004-sqlite-first.md`). This keeps the
//! dependency direction pointing inward: `Infrastructure -> Application/Domain`.

use core::fmt;

use endora_domain::{
    Assumption, AssumptionId, AuditRecord, Belief, BeliefId, BeliefKind, ChatMessage, Confidence,
    Direction, DirectionId, Experiment, ExperimentId, MessageId, Observation, Preference,
    PreferenceId, PreferenceKind, ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId,
    SuggestionId, Target, TargetId, Timestamp, Value, ValueId,
};

/// A complete snapshot of the user's stored data, for the memory rights of the
/// constitution: it is what "export" hands back and what "delete" removes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemorySnapshot {
    /// All values.
    pub values: Vec<Value>,
    /// All directions.
    pub directions: Vec<Direction>,
    /// All targets.
    pub targets: Vec<Target>,
    /// All assumptions.
    pub assumptions: Vec<Assumption>,
    /// All experiments.
    pub experiments: Vec<Experiment>,
    /// All observations.
    pub observations: Vec<Observation>,
    /// All reflections.
    pub reflections: Vec<Reflection>,
    /// All proposed process changes.
    pub process_changes: Vec<ProposedProcessChange>,
    /// The full audit trail.
    pub audit: Vec<AuditRecord>,
    /// The whole conversation with the butler.
    pub messages: Vec<ChatMessage>,
    /// The preferences the butler has learned.
    pub preferences: Vec<Preference>,
    /// The butler's persisted suggestions (pending, applied, and dismissed).
    pub suggestions: Vec<Suggestion>,
    /// Endora's understanding of the person — the beliefs it holds.
    pub beliefs: Vec<Belief>,
}

/// The user's right to export and delete all of their data (constitution:
/// memory must be exportable and deletable).
pub trait MemoryStore {
    /// Collects everything the user has stored into a [`MemorySnapshot`].
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn export(&self) -> Result<MemorySnapshot, RepositoryError>;

    /// Permanently deletes all of the user's stored data.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn purge(&self) -> Result<(), RepositoryError>;
}

/// A failure from a storage backend behind a repository port.
///
/// Deliberately free of any engine-specific type: adapters translate their own
/// errors (driver, I/O, corrupt rows) into these variants so the application
/// stays independent of the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryError {
    /// The backend itself failed (connection, I/O, driver error, lock).
    Backend(String),
    /// Stored data could not be reconstituted into a valid domain value.
    Corrupt(String),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(msg) => write!(f, "storage backend error: {msg}"),
            Self::Corrupt(msg) => write!(f, "corrupt stored data: {msg}"),
        }
    }
}

impl core::error::Error for RepositoryError {}

/// Persists and retrieves [`Value`]s (the Identity & Values context).
pub trait ValueRepository {
    /// Inserts a value, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, value: &Value) -> Result<(), RepositoryError>;

    /// Fetches a value by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ValueId) -> Result<Option<Value>, RepositoryError>;

    /// Lists all values, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_all(&self) -> Result<Vec<Value>, RepositoryError>;

    /// Permanently removes a value. Callers are responsible for ensuring no
    /// North Star still references it first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: ValueId) -> Result<(), RepositoryError>;
}

/// Persists and retrieves [`Direction`]s (North Stars).
pub trait DirectionRepository {
    /// Inserts a direction, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, direction: &Direction) -> Result<(), RepositoryError>;

    /// Fetches a direction by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError>;

    /// Lists all directions, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_all(&self) -> Result<Vec<Direction>, RepositoryError>;

    /// Permanently removes a direction. Callers are responsible for ensuring it
    /// has no dependents first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: DirectionId) -> Result<(), RepositoryError>;
}

/// Persists and retrieves [`Target`]s.
pub trait TargetRepository {
    /// Inserts a target, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, target: &Target) -> Result<(), RepositoryError>;

    /// Fetches a target by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: TargetId) -> Result<Option<Target>, RepositoryError>;

    /// Lists the targets belonging to a direction, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Target>, RepositoryError>;

    /// Permanently removes a target. Callers are responsible for ensuring it has
    /// no dependents first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: TargetId) -> Result<(), RepositoryError>;
}

/// Persists and retrieves [`Assumption`]s.
pub trait AssumptionRepository {
    /// Inserts an assumption, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, assumption: &Assumption) -> Result<(), RepositoryError>;

    /// Fetches an assumption by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: AssumptionId) -> Result<Option<Assumption>, RepositoryError>;

    /// Lists the assumptions belonging to a target, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_target(&self, target: TargetId) -> Result<Vec<Assumption>, RepositoryError>;
}

/// Persists and retrieves [`Experiment`]s.
pub trait ExperimentRepository {
    /// Inserts an experiment, or replaces the existing one with the same id
    /// (used both to create and to persist lifecycle transitions).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, experiment: &Experiment) -> Result<(), RepositoryError>;

    /// Fetches an experiment by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ExperimentId) -> Result<Option<Experiment>, RepositoryError>;

    /// Lists the experiments testing an assumption, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_assumption(
        &self,
        assumption: AssumptionId,
    ) -> Result<Vec<Experiment>, RepositoryError>;

    /// Lists experiments whose scheduled review is due as of `now` — a review
    /// was scheduled for at or before `now` and the experiment is not concluded.
    /// Ordered by review time, soonest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_due_reviews(&self, now: Timestamp) -> Result<Vec<Experiment>, RepositoryError>;
}

/// Persists and retrieves [`Observation`]s.
pub trait ObservationRepository {
    /// Inserts an observation, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, observation: &Observation) -> Result<(), RepositoryError>;

    /// Lists the observations recorded for an experiment, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_experiment(
        &self,
        experiment: ExperimentId,
    ) -> Result<Vec<Observation>, RepositoryError>;

    /// Lists the most recently recorded observations across all experiments,
    /// newest first, up to `limit`. Used to build the activity feed.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn recent(&self, limit: usize) -> Result<Vec<Observation>, RepositoryError>;
}

/// A failure from a reasoning model behind the [`Proposer`] port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalError {
    /// The model could not be reached, or its response could not be understood.
    Unavailable(String),
    /// The model returned an empty proposal.
    Empty,
}

impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(msg) => write!(f, "model unavailable: {msg}"),
            Self::Empty => write!(f, "model returned an empty proposal"),
        }
    }
}

impl core::error::Error for ProposalError {}

/// Produces proposals from a reasoning model (see
/// `docs/adr/0008-local-model-adapter.md`).
///
/// A proposer is a reasoning component, not an authority: whatever it returns is
/// only *input* to the deterministic policy boundary. The domain never depends
/// on it; infrastructure implements it (e.g. a local, OpenAI-compatible model).
pub trait Proposer {
    /// Proposes a one-line process-change description for a reflection, given
    /// its summary and how many observations it cites.
    ///
    /// # Errors
    /// [`ProposalError`] if the model is unreachable or returns nothing usable.
    fn propose_process_change(
        &self,
        reflection_summary: &str,
        evidence_count: usize,
    ) -> Result<String, ProposalError>;
}

/// A structured action the butler proposes. This is a **closed set**: the butler
/// can only suggest these, and each maps to an existing use case. The model
/// *proposes* one; the person *confirms*; deterministic code executes. The model
/// can never step outside this set or act on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButlerProposal {
    /// Create a value (a "why").
    CreateValue {
        /// The value's name.
        name: String,
    },
    /// Create a North Star (direction).
    CreateNorthStar {
        /// The North Star's title.
        title: String,
    },
    /// Create a target under an existing North Star.
    CreateTarget {
        /// How the butler referred to the North Star — a real direction id, or a
        /// title/name (small models often give the name). Resolved to a concrete
        /// North Star when the suggestion is applied, not at parse time, so the
        /// proposal is never silently dropped.
        direction_ref: String,
        /// The target statement.
        statement: String,
    },
    /// Remember a preference about the person (so it stops re-asking).
    RememberPreference {
        /// The preference text.
        text: String,
        /// Whether it is taste or a grant of authority.
        kind: PreferenceKind,
    },
}

impl ButlerProposal {
    /// A stable, lowercase kind name for the protocol.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::CreateValue { .. } => "create_value",
            Self::CreateNorthStar { .. } => "create_north_star",
            Self::CreateTarget { .. } => "create_target",
            Self::RememberPreference { .. } => "remember_preference",
        }
    }

    /// A human-readable one-line summary of what confirming would do.
    #[must_use]
    pub fn label(&self) -> String {
        // Natural, action-oriented phrasing — the confirm card shouldn't recite
        // the internal taxonomy (value / North Star / target) at the person; that
        // vocabulary lives in the profile views, not the conversation.
        match self {
            Self::CreateValue { name } => format!("Note that this matters to you: \"{name}\""),
            Self::CreateNorthStar { title } => {
                format!("Keep this as something you're working toward: \"{title}\"")
            }
            Self::CreateTarget { statement, .. } => {
                format!("Add a concrete next step: \"{statement}\"")
            }
            Self::RememberPreference { text, .. } => format!("Remember this about you: \"{text}\""),
        }
    }
}

/// A belief the butler has formed about the person this turn — understanding,
/// not an action. Stored directly (Endora owns its own model); the person reviews
/// and corrects it (ADR 0020). Distinct from a [`ButlerProposal`], which the
/// person must authorize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormedBelief {
    /// What is believed, in plain language.
    pub statement: String,
    /// What sort of belief it is.
    pub kind: BeliefKind,
    /// How sure the butler is.
    pub confidence: Confidence,
    /// What in the conversation supports it.
    pub evidence: String,
}

/// What the butler says back: a reply, any actions it proposes, and any
/// understanding it formed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ButlerReply {
    /// The butler's text reply.
    pub text: String,
    /// Structured actions it proposes (may be empty). Never auto-executed.
    pub proposals: Vec<ButlerProposal>,
    /// Beliefs it formed about the person this turn (may be empty). Understanding,
    /// not actions — stored directly, then reviewable/correctable.
    pub beliefs: Vec<FormedBelief>,
    /// A skill it wants to use this turn (may be none). The policy layer decides
    /// whether to run it; on a read-only skill the butler then answers with the
    /// result.
    pub capability_use: Option<CapabilityUse>,
}

/// Persists the person's [`Belief`]s — Endora's living understanding of them.
pub trait BeliefRepository {
    /// Inserts a belief, or replaces the one with the same id (create + update).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, belief: &Belief) -> Result<(), RepositoryError>;

    /// Fetches a belief by id, `None` if absent.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: BeliefId) -> Result<Option<Belief>, RepositoryError>;

    /// Lists all beliefs, most-recently-affirmed first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self) -> Result<Vec<Belief>, RepositoryError>;
}

/// Where a persisted [`Suggestion`] is in its life: proposed and waiting, applied
/// to the person's memory, or dismissed. A suggestion is a butler proposal made
/// durable — it survives reloads and accumulates, so chat learnings are not lost
/// when the conversation moves on (ADR 0019 §"persistent suggestions").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionStatus {
    /// Proposed and awaiting the person's decision.
    Pending,
    /// Applied — the underlying create ran and it is now part of memory.
    Applied,
    /// Dismissed by the person.
    Dismissed,
}

impl SuggestionStatus {
    /// The stable string form used for storage and the protocol.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Dismissed => "dismissed",
        }
    }

    /// Parses the stable string form, if recognised.
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "applied" => Some(Self::Applied),
            "dismissed" => Some(Self::Dismissed),
            _ => None,
        }
    }
}

/// A butler proposal made durable: the conversation's learnings persisted as an
/// event the person can apply or dismiss at any time (not only in the moment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// This suggestion's id.
    pub id: SuggestionId,
    /// The proposed action (from the closed set the butler may propose).
    pub proposal: ButlerProposal,
    /// Its current state.
    pub status: SuggestionStatus,
    /// The butler message it came from, if known.
    pub from_message: Option<MessageId>,
    /// When it was proposed.
    pub created_at: Timestamp,
    /// When it was applied or dismissed, if it has been.
    pub decided_at: Option<Timestamp>,
}

/// Persists the butler's [`Suggestion`]s so they outlive a single reply.
pub trait SuggestionRepository {
    /// Inserts a suggestion, or replaces the existing one with the same id
    /// (used both to create and to record an apply/dismiss decision).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, suggestion: &Suggestion) -> Result<(), RepositoryError>;

    /// Fetches a suggestion by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: SuggestionId) -> Result<Option<Suggestion>, RepositoryError>;

    /// Lists suggestions, newest first, optionally filtered by status.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self, status: Option<SuggestionStatus>) -> Result<Vec<Suggestion>, RepositoryError>;
}

/// The person's cadence for proactive **check-ins** — the butler reaching out on
/// its own (ADR 0019 §heartbeat/check-ins). The person owns it: whether it is on,
/// how often, and when the next one is due. Interval-based for now; time-of-day
/// windows ("mornings") are a later refinement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckinSchedule {
    /// Whether proactive check-ins are on. Off by default — the butler never
    /// reaches out uninvited until the person turns it on.
    pub enabled: bool,
    /// How long between check-ins, in milliseconds.
    pub interval_ms: i64,
    /// When the next check-in is due.
    pub next_at: Timestamp,
}

impl CheckinSchedule {
    /// The default: **off**, with a daily cadence ready if the person enables it.
    #[must_use]
    pub fn disabled_default(now: Timestamp) -> Self {
        let day_ms = 24 * 60 * 60 * 1_000;
        Self {
            enabled: false,
            interval_ms: day_ms,
            next_at: Timestamp::from_unix_millis(now.unix_millis() + day_ms),
        }
    }

    /// Whether a check-in is due now (enabled and past its next time).
    #[must_use]
    pub fn is_due(&self, now: Timestamp) -> bool {
        self.enabled && now.unix_millis() >= self.next_at.unix_millis()
    }
}

/// Persists the single [`CheckinSchedule`].
pub trait CheckinRepository {
    /// Returns the stored schedule, or `None` if never set.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self) -> Result<Option<CheckinSchedule>, RepositoryError>;

    /// Stores the schedule (replacing any previous one).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, schedule: &CheckinSchedule) -> Result<(), RepositoryError>;
}

/// The person's **autonomy envelope** (ADR 0022): the deterministic boundary the
/// butler acts independently *within*. Widening it grants more independence; the
/// policy layer — never the model — still enforces the edges. This first slice has
/// two coarse levers; finer axes (spend vs. privacy, per-domain) come later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutonomyEnvelope {
    /// May the butler use read-only skills that **leave the device** (weather,
    /// news, a web page) on its own? Default yes.
    pub auto_external: bool,
    /// May it take **confirm-required** (consequential) actions on its own, rather
    /// than surfacing them for approval? Default no — the safe posture.
    pub auto_consequential: bool,
}

impl Default for AutonomyEnvelope {
    fn default() -> Self {
        // Preserves the established behaviour: read-only skills act on their own,
        // consequential ones ask (ADR 0010).
        Self {
            auto_external: true,
            auto_consequential: false,
        }
    }
}

/// Persists the person's [`AutonomyEnvelope`] (ADR 0022).
pub trait AutonomyEnvelopeRepository {
    /// The stored envelope, or the default if never set.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<AutonomyEnvelope, RepositoryError>;

    /// Stores the envelope (replacing any previous one).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, envelope: &AutonomyEnvelope) -> Result<(), RepositoryError>;
}

/// Persists per-capability configuration the person controls from the Skills view
/// (ADR 0021). This first slice stores only the **enabled** flag; only overrides
/// are stored — a capability with no row keeps its built-in default (enabled).
pub trait CapabilityConfigRepository {
    /// The stored enabled/disabled overrides, as `(id, enabled)` pairs. Ids not
    /// present here have never been toggled and use their default.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn enabled_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError>;

    /// Sets whether a capability is enabled (upsert by id).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), RepositoryError>;
}

/// A brief of one North Star, for grounding the butler's conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NorthStarBrief {
    /// The North Star's id (so the butler can propose a target under it).
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its lifecycle status.
    pub status: String,
    /// The value it serves, if filed.
    pub value: Option<String>,
    /// Whether it has an active target yet.
    pub has_active_target: bool,
}

/// A snapshot of the person's current life the butler is given each turn, so the
/// conversation is grounded in what actually exists rather than starting cold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ButlerContext {
    /// The person's value names.
    pub values: Vec<String>,
    /// The person's North Stars.
    pub north_stars: Vec<NorthStarBrief>,
    /// What currently needs attention (headlines).
    pub attention: Vec<String>,
    /// What Endora already understands about the person (its active beliefs), so
    /// the butler builds on and refines them rather than re-forming duplicates.
    pub understanding: Vec<String>,
    /// The skills the butler can actually use right now (id + one-line what),
    /// so it reaches for a real capability instead of only talking about it.
    pub capabilities: Vec<String>,
    /// A result the butler just got back from a capability it used this turn —
    /// set only on the synthesis pass, so it can answer using real data.
    pub tool_result: Option<String>,
    /// The current date and time (human-readable), so the butler always knows what
    /// day it is rather than guessing or leaking a placeholder. Cheap local truth —
    /// grounded every turn, unlike weather/news which need a skill.
    pub now: String,
}

/// A capability the butler asked to use this turn (parsed from its reply). The
/// policy layer decides whether to run it; the model never executes directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityUse {
    /// The capability id, e.g. `"weather"`.
    pub capability: String,
    /// The JSON input for it, as a string.
    pub input_json: String,
}

/// What a capability the butler could use looks like to the application layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySpec {
    /// Stable id, e.g. `"weather"`.
    pub id: String,
    /// One-line description of what it does.
    pub description: String,
    /// Ready to use (vs awaiting setup).
    pub configured: bool,
    /// May it run on its own (read-only/low-stakes), or must the person authorize?
    pub autonomous: bool,
}

/// Runs the butler's skills. The application asks this port to execute a
/// capability the butler proposed, keeping the model out of the execution path
/// (models propose, policy authorizes, capabilities execute — ADRs 0019/0020).
pub trait CapabilityRunner {
    /// The skills currently available (for grounding the butler).
    fn available(&self) -> Vec<CapabilitySpec>;

    /// Runs a capability with JSON input, returning its JSON output or an error
    /// message. Only ever called for capabilities the policy layer has cleared.
    fn run(&self, id: &str, input_json: &str) -> Result<String, String>;
}

/// The butler brain: given the conversation so far, produce a reply and any
/// proposed actions.
///
/// Like [`Proposer`], this is a reasoning component, not an authority — it only
/// *proposes*. Infrastructure supplies a scripted implementation (offline/tests)
/// or a model-backed one (`docs/adr/0014-the-butler-conversation-values-attention.md`).
pub trait Butler {
    /// Responds to the conversation so far (the last message is the newest),
    /// given the preferences already learned and a snapshot of the person's
    /// current life ([`ButlerContext`]) so it can speak about what actually
    /// exists and propose the next concrete step.
    ///
    /// # Errors
    /// [`ProposalError`] if a backing model is unreachable or returns nothing.
    fn respond(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError>;

    /// Like [`respond`](Self::respond), but streams the reply's prose to
    /// `on_token` as it is produced (each call receives the *next* chunk of
    /// text), returning the complete [`ButlerReply`] — including proposals — at
    /// the end. Enables a live, token-by-token chat.
    ///
    /// The default implementation is non-streaming: it computes the whole reply
    /// and emits it in one chunk, so any [`Butler`] works with a streaming
    /// caller. A model-backed butler overrides this to stream for real.
    ///
    /// # Errors
    /// [`ProposalError`] if a backing model is unreachable or returns nothing.
    fn respond_streaming(
        &self,
        history: &[ChatMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        let reply = self.respond(history, preferences, context)?;
        if !reply.text.is_empty() {
            on_token(&reply.text);
        }
        Ok(reply)
    }
}

/// Persists and retrieves [`Preference`]s (what the butler has learned).
pub trait PreferenceRepository {
    /// Inserts a preference, or replaces the existing one with the same id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, preference: &Preference) -> Result<(), RepositoryError>;

    /// Lists all preferences, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_all(&self) -> Result<Vec<Preference>, RepositoryError>;

    /// Permanently removes a preference.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn delete(&self, id: PreferenceId) -> Result<(), RepositoryError>;
}

/// Persists and retrieves the conversation with the butler.
pub trait ChatRepository {
    /// Appends a message to the conversation.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn append(&self, message: &ChatMessage) -> Result<(), RepositoryError>;

    /// Lists the conversation, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError>;
}

/// The kind of thing needing the person's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionKind {
    /// An experiment whose scheduled review has arrived.
    ReviewDue,
    /// An active North Star not yet filed under a value.
    UnfiledNorthStar,
    /// An active North Star with no active target under it.
    EmptyNorthStar,
}

impl AttentionKind {
    /// A stable, lowercase name for the protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReviewDue => "review_due",
            Self::UnfiledNorthStar => "unfiled_north_star",
            Self::EmptyNorthStar => "empty_north_star",
        }
    }

    /// Parses a kind from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "review_due" => Some(Self::ReviewDue),
            "unfiled_north_star" => Some(Self::UnfiledNorthStar),
            "empty_north_star" => Some(Self::EmptyNorthStar),
            _ => None,
        }
    }
}

/// One thing the butler would raise, unless snoozed. A read projection, ranked by
/// the order it is produced (most pressing first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionItem {
    /// What kind of attention this is.
    pub kind: AttentionKind,
    /// The id of the experiment or North Star it concerns.
    pub subject: String,
    /// A human-readable one-line description.
    pub headline: String,
}

/// A recorded deferral of an attention item: how many times it has been snoozed,
/// and the time it stays hidden until. Each snooze roughly doubles the interval,
/// so a repeatedly-deferred item is raised less and less.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snooze {
    /// How many times the item has been snoozed.
    pub count: u32,
    /// The item stays hidden until this time.
    pub until: Timestamp,
}

/// Persists deferral (snooze) state for attention items, keyed by
/// `(kind, subject)`.
pub trait SnoozeRepository {
    /// The current snooze for an item, if any.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, kind: &str, subject: &str) -> Result<Option<Snooze>, RepositoryError>;

    /// Records (or replaces) the snooze for an item.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, kind: &str, subject: &str, snooze: Snooze) -> Result<(), RepositoryError>;
}

/// Supplies the current time to use cases.
///
/// The domain never reads the clock, so time enters through this port. The node
/// wires a real system clock; tests wire a fixed one.
pub trait Clock {
    /// The current instant, as a domain [`Timestamp`].
    fn now(&self) -> Timestamp;
}

/// Supplies fresh, unique identifier values to use cases.
///
/// The domain never generates identifiers, so they enter through this port. Use
/// cases wrap the raw value in the appropriate typed id. The node wires a random
/// source; tests wire a deterministic one.
pub trait IdSource {
    /// Returns a fresh identifier value, unique within this store's lifetime.
    fn new_id(&self) -> u128;
}

/// Persists and retrieves [`Reflection`]s (including their evidence).
pub trait ReflectionRepository {
    /// Inserts a reflection and its evidence, or replaces the one with the same
    /// id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, reflection: &Reflection) -> Result<(), RepositoryError>;

    /// Fetches a reflection by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ReflectionId) -> Result<Option<Reflection>, RepositoryError>;

    /// Lists the reflections for a target, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_target(&self, target: TargetId) -> Result<Vec<Reflection>, RepositoryError>;
}

/// Persists and retrieves [`ProposedProcessChange`]s.
pub trait ProcessChangeRepository {
    /// Inserts a proposed change, or replaces the one with the same id (used to
    /// create and to persist approval/rejection).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn save(&self, change: &ProposedProcessChange) -> Result<(), RepositoryError>;

    /// Fetches a proposed change by id, returning `None` if it does not exist.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn get(&self, id: ProcessChangeId) -> Result<Option<ProposedProcessChange>, RepositoryError>;

    /// Lists the proposed changes from a reflection, in a stable order.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn list_for_reflection(
        &self,
        reflection: ReflectionId,
    ) -> Result<Vec<ProposedProcessChange>, RepositoryError>;
}

/// Appends to and reads the audit trail.
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

/// One entry in the butler's own event log: something it did or learned, or a
/// setting the person changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEvent {
    /// When it happened.
    pub at: Timestamp,
    /// A plain-language line ("Used the weather skill", "Turned news off").
    pub summary: String,
}

/// The butler's **action log** (ADR 0012's activity feed, widened): a durable,
/// append-only record of what the butler did and learned each turn, and the
/// person's setting changes — so the activity view shows the butler's actions and
/// system events, not just policy decisions and experiment observations.
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
