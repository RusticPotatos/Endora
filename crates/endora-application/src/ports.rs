//! Ports the application defines and the infrastructure layer implements.
//!
//! These are the abstractions the application depends on for persistence. The
//! application never names a concrete storage engine; infrastructure provides
//! adapters (e.g. SQLite — see `docs/adr/0004-sqlite-first.md`). This keeps the
//! dependency direction pointing inward: `Infrastructure -> Application/Domain`.

use core::fmt;

use endora_capabilities::CapabilityUse;
use endora_conversation::{ChatMessage, MessageRole};
use endora_direction::{
    Assumption, Direction, Experiment, Observation, ProposedProcessChange, Reflection, Target,
    Value,
};
use endora_kernel::ids::{MessageId, SuggestionId, Timestamp};
use endora_platform::AuditRecord;
use endora_understanding::{Belief, BeliefKind, Confidence, Preference, PreferenceKind};

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

// `RepositoryError` is the shared persistence-failure vocabulary; it lives in the
// kernel so the shared `Db` handle and every context's repositories speak it. See
// ADR 0026. Re-exported here so `ports::RepositoryError` paths are unchanged.
pub use endora_kernel::RepositoryError;

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
    /// result. (Legacy single-call view; kept for the two-pass turn.)
    pub capability_use: Option<CapabilityUse>,
    /// Every tool the model asked to call this step, each with the id the endpoint
    /// assigned it (ADR 0028). The single-conversation loop runs these and appends
    /// their results as `role:tool` turns keyed by that id. Empty when it just talks.
    pub tool_calls: Vec<ToolCall>,
}

/// One tool call the model made through the endpoint's native tool-calling API — the
/// call id (so its result can be paired back), the capability id to run, and the JSON
/// arguments (ADR 0028).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolCall {
    /// The endpoint-assigned id, echoed on the matching `role:tool` result message.
    pub id: String,
    /// The capability id to run (the un-sanitised `server.tool` for MCP).
    pub capability: String,
    /// The JSON arguments for the call, as a string.
    pub input_json: String,
}

/// One turn in the butler's single tool-calling conversation (ADR 0028): the person's
/// message, an assistant turn (prose and/or tool calls), or a tool result paired to
/// the call that produced it. Replaces threading tool output through a system prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnMessage {
    /// Something the person said.
    User(String),
    /// The butler's turn — prose it wrote and/or the tools it asked to call.
    Assistant {
        /// Any prose the butler wrote on this turn (often empty when it only calls).
        text: String,
        /// The tool calls it made this turn, each with its id.
        tool_calls: Vec<ToolCall>,
    },
    /// The result of running a tool call, paired to it by `call_id`. Carries the real
    /// outcome — success or error — so the model answers grounded in what happened.
    ToolResult {
        /// The id of the [`ToolCall`] this is the result of.
        call_id: String,
        /// The tool's output, or its error message.
        content: String,
    },
}

// BeliefRepository moved to the understanding context (ADR 0026); re-exported
// from `endora_application` (see lib.rs) so existing paths hold.

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

// The check-in and daily-brief schedules — CheckinSchedule, BriefSchedule, and
// their repositories — now live in the scheduling context (ADR 0026). They are
// re-exported from `endora_application` (see lib.rs) so existing paths hold.

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
    /// The same skills in structured form, so the model layer can offer them through
    /// the endpoint's native tool-calling API (exact name + input schema) rather than
    /// relying on the model to hand-write an id. Parallel to `capabilities`.
    pub tools: Vec<CapabilityTool>,
    /// A result the butler just got back from a capability it used this turn —
    /// set only on the synthesis pass, so it can answer using real data.
    pub tool_result: Option<String>,
    /// The current date and time (human-readable), so the butler always knows what
    /// day it is rather than guessing or leaking a placeholder. Cheap local truth —
    /// grounded every turn, unlike weather/news which need a skill.
    pub now: String,
    /// This turn's FINAL answer is being written (prose for the person), not a
    /// tool-routing decision. In the mixture (ADR 0027) it routes to the
    /// *synthesizer* (the generalist), so plain conversation is answered by the
    /// model that's good at it rather than the tool-tuned router.
    pub synthesize: bool,
}

/// A skill offered to the model through native tool-calling: its exact id, what it
/// does, and its input schema (when it advertises one — MCP tools do; built-ins pass
/// `None` and rely on the prompt's examples).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityTool {
    /// The exact capability id the runner expects, e.g. `"home-assistant.HassTurnOn"`.
    pub id: String,
    /// One-line description of what it does.
    pub description: String,
    /// JSON-Schema for its arguments as a JSON string, if known — kept as text so the
    /// context stays comparable; the model layer parses it back when building the
    /// tool-calling request.
    pub input_schema: Option<String>,
}

// The capabilities ports — CapabilityUse, CapabilitySpec, CapabilityRunner,
// DeepModel(+Repository), AutonomyEnvelope(+Repository), and the settings/config
// repositories — now live in the capabilities context (ADR 0026). They are
// re-exported from `endora_application` (see lib.rs) so existing paths hold.

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

    /// One step of the **single tool-calling conversation** (ADR 0028): given the
    /// conversation so far — including assistant tool-call turns and their
    /// [`TurnMessage::ToolResult`]s — produce the next assistant turn. A reply with
    /// `tool_calls` means "run these and give me their results"; an empty
    /// `tool_calls` with `text` is the final answer. The application drives the loop
    /// (executing calls through policy, appending results); this only voices the
    /// model side.
    ///
    /// The default flattens the conversation to plain user/assistant messages and
    /// answers via [`respond`](Self::respond), so any [`Butler`] works. A model-backed
    /// butler overrides it to run real native tool-calling against the endpoint.
    ///
    /// # Errors
    /// [`ProposalError`] if a backing model is unreachable or returns nothing.
    fn take_turn(
        &self,
        conversation: &[TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
    ) -> Result<ButlerReply, ProposalError> {
        let history: Vec<ChatMessage> = conversation
            .iter()
            .filter_map(|m| match m {
                TurnMessage::User(text) => Some((MessageRole::User, text.as_str())),
                TurnMessage::Assistant { text, .. } if !text.is_empty() => {
                    Some((MessageRole::Butler, text.as_str()))
                }
                _ => None,
            })
            .filter_map(|(role, text)| {
                ChatMessage::new(
                    MessageId::new(0),
                    role,
                    text,
                    Timestamp::from_unix_millis(0),
                )
                .ok()
            })
            .collect();
        // Bridge for butlers without native tool-calling: surface the latest tool
        // result to `respond` (as `tool_result`) so it can answer from it, and express
        // any `capability_use` it returns as a `tool_call` so the single loop (ADR
        // 0028) can drive it just like a native tool-caller.
        let mut ctx = context.clone();
        if let Some(TurnMessage::ToolResult { content, .. }) = conversation
            .iter()
            .rev()
            .find(|m| matches!(m, TurnMessage::ToolResult { .. }))
        {
            ctx.tool_result = Some(content.clone());
        }
        let reply = self.respond(&history, preferences, &ctx)?;
        if reply.tool_calls.is_empty() {
            if let Some(used) = reply.capability_use.clone() {
                return Ok(ButlerReply {
                    tool_calls: vec![ToolCall {
                        id: "call".to_owned(),
                        capability: used.capability,
                        input_json: used.input_json,
                    }],
                    ..reply
                });
            }
        }
        Ok(reply)
    }
}

/// The next rung of the **capability ladder**: a deeper (bigger / cloud) model the
/// person has configured for questions the local one can't handle. Local-first —
/// the turn only reaches for this when the local rung comes up empty, and only when
/// the person has opted in by configuring one.
///
/// Escalation sends the question off the device, so an implementation must apply
/// the same egress protections as any outbound call (withhold apparent secrets,
/// minimize personal data). It returns prose only — never an action — so it stays a
/// *reasoning* aid behind the deterministic policy boundary, not a way around it.
pub trait DeepAsker {
    /// Escalate a question to the deeper model. Returns its answer, or `None` if no
    /// deeper model is configured, the request is withheld (egress guard), or the
    /// call fails — in which case the caller keeps its local answer / honest
    /// fallback.
    fn ask(&self, question: &str) -> Option<String>;
}

// PreferenceRepository moved to the understanding context (ADR 0026);
// re-exported from `endora_application` (see lib.rs) so existing paths hold.

// ChatRepository moved to the conversation context (ADR 0026); re-exported from
// `endora_application` (see lib.rs) so existing paths hold.

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

// `Clock` and `IdSource` are shared-kernel ports (time and identity enter the
// pure layers through them), re-exported here so `endora_application::{Clock,
// IdSource}` and `ports::…` paths are unchanged. See ADR 0026.
pub use endora_kernel::{Clock, IdSource};

// The audit trail and the butler's event log — `AuditLog`, `EventLog`, and
// `ActivityEvent` — now belong to the platform context (ADR 0026). They are
// re-exported from `endora_application` (see lib.rs) so existing paths hold.
