//! Ports the application defines and the infrastructure layer implements.
//!
//! These are the abstractions the application depends on for persistence. The
//! application never names a concrete storage engine; infrastructure provides
//! adapters (e.g. SQLite — see `docs/adr/0004-sqlite-first.md`). This keeps the
//! dependency direction pointing inward: `Infrastructure -> Application/Domain`.

use core::fmt;

use endora_capabilities::CapabilityUse;
use endora_conversation::{ChatMessage, MessageRole};
use endora_kernel::ids::{MessageId, Timestamp};
use endora_platform::AuditRecord;
use endora_understanding::{Belief, BeliefKind, Confidence, Intention, Outcome, Preference};

/// A complete snapshot of the user's stored data, for the memory rights of the
/// constitution: it is what "export" hands back and what "delete" removes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemorySnapshot {
    /// The full audit trail.
    pub audit: Vec<AuditRecord>,
    /// The whole conversation with the butler.
    pub messages: Vec<ChatMessage>,
    /// The preferences the butler has learned.
    pub preferences: Vec<Preference>,
    /// Endora's understanding of the person — the beliefs it holds.
    pub beliefs: Vec<Belief>,
    /// What happened after Endora acted — claim, observation and reaction (ADR 0053).
    pub outcomes: Vec<Outcome>,
    /// What Endora has pursued, is pursuing, and dropped (ADR 0052).
    pub intentions: Vec<Intention>,
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
// ADR 0050. Re-exported here so `ports::RepositoryError` paths are unchanged.
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

/// A belief the butler has formed about the person this turn — understanding,
/// not an action. Stored directly (Endora owns its own model); the person reviews
/// and corrects it (ADR 0052).
///
/// Understanding is the only thing the butler *files* on its own. Actions in the
/// world stay behind the policy boundary, where they are executed as capability
/// calls that deterministic policy authorizes — not as records for the person to
/// approve later.
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
    /// Beliefs it formed about the person this turn (may be empty). Understanding,
    /// not actions — stored directly, then reviewable/correctable.
    pub beliefs: Vec<FormedBelief>,
    /// A skill it wants to use this turn (may be none). The policy layer decides
    /// whether to run it; on a read-only skill the butler then answers with the
    /// result. (Legacy single-call view; kept for the two-pass turn.)
    pub capability_use: Option<CapabilityUse>,
    /// Every tool the model asked to call this step, each with the id the endpoint
    /// assigned it (ADR 0053). The single-conversation loop runs these and appends
    /// their results as `role:tool` turns keyed by that id. Empty when it just talks.
    pub tool_calls: Vec<ToolCall>,
}

/// One tool call the model made through the endpoint's native tool-calling API — the
/// call id (so its result can be paired back), the capability id to run, and the JSON
/// arguments (ADR 0053).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolCall {
    /// The endpoint-assigned id, echoed on the matching `role:tool` result message.
    pub id: String,
    /// The capability id to run (the un-sanitised `server.tool` for MCP).
    pub capability: String,
    /// The JSON arguments for the call, as a string.
    pub input_json: String,
}

/// One turn in the butler's single tool-calling conversation (ADR 0053): the person's
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

// BeliefRepository moved to the understanding context (ADR 0050); re-exported
// from `endora_application` (see lib.rs) so existing paths hold.

// The check-in and daily-brief schedules — CheckinSchedule, BriefSchedule, and
// their repositories — now live in the scheduling context (ADR 0050). They are
// re-exported from `endora_application` (see lib.rs) so existing paths hold.

/// A snapshot of what Endora currently knows, handed to the butler each turn so
/// the conversation is grounded in the person it has come to understand rather
/// than starting cold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ButlerContext {
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
    /// The current date and time (human-readable), so the butler always knows what
    /// day it is rather than guessing or leaking a placeholder. Cheap local truth —
    /// grounded every turn, unlike weather/news which need a skill.
    pub now: String,
    /// A compact running summary of the earlier conversation this session — the part
    /// that has scrolled out of the recent verbatim window. Keeps the day's thread
    /// present without sending the whole transcript (which slows a local model), so
    /// the butler stays coherent over a long chat. `None` until the window overflows.
    pub conversation_summary: Option<String>,
    /// What the person has told Endora its tools' targets are really called (ADR 0054)
    /// — one line each, e.g. `on home-assistant, "kitchen main" means "Kitchen Main"`.
    ///
    /// Grounding, the way understanding is: it tells the butler what a thing is called
    /// here. It is **not** a substitution — nothing rewrites the target a model asks
    /// for, because that could act on the wrong thing and would hide the mistake from
    /// the battery that exists to measure it (ADR 0053).
    pub target_aliases: Vec<String>,
    /// What the person's own services say about them **right now** — one line each,
    /// e.g. `rustic is not home`.
    ///
    /// Live state rather than a belief: true this minute and worthless tomorrow, so it
    /// belongs to the turn and never to understanding. It is what lets the butler know
    /// whether anyone is there before it decides to speak.
    pub present: Vec<String>,
    /// How the butler's own past actions have landed, per skill — built from the
    /// outcomes it recorded and what the person said about them (ADR 0053).
    ///
    /// Only skills the person has actually reacted to appear, so this stays short and
    /// carries signal rather than noise. Empty until they have said something about
    /// anything, which is the normal early state.
    pub track_record: Vec<String>,
}

/// A compact summary of the conversation so far, and how many messages it folds in —
/// so the turn knows whether new messages have scrolled past the recent window and
/// the summary needs extending (ADR 0053 context compaction).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationSummary {
    /// The running summary text.
    pub text: String,
    /// How many of the oldest messages this summary already folds in.
    pub covered: usize,
}

/// Stores the running conversation summary between turns, so it is regenerated only
/// when the recent window overflows (not every turn). An in-memory implementation is
/// fine — the durable, cross-session facts live in beliefs (ADR 0052); this only
/// keeps the current session's thread compact.
pub trait ConversationSummaryStore {
    /// The current summary, if one has been formed this session.
    fn get(&self) -> Option<ConversationSummary>;
    /// Replaces the stored summary.
    fn set(&self, summary: ConversationSummary);
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
// repositories — now live in the capabilities context (ADR 0050). They are
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

    /// [`take_turn`](Self::take_turn), streaming the answer's prose to `on_token` as it
    /// arrives.
    ///
    /// The default answers in one piece and hands the whole thing over, so every butler
    /// works with a streaming caller — the same shape as
    /// [`respond_streaming`](Self::respond_streaming). A model-backed butler overrides it
    /// to stream for real.
    ///
    /// A round that turns out to be **tool calls** has no prose to stream, and emits
    /// nothing. Only the round that answers produces tokens.
    ///
    /// # Errors
    /// [`ProposalError`] if a backing model is unreachable or returns nothing.
    fn take_turn_streaming(
        &self,
        conversation: &[TurnMessage],
        preferences: &[Preference],
        context: &ButlerContext,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<ButlerReply, ProposalError> {
        let reply = self.take_turn(conversation, preferences, context)?;
        if reply.tool_calls.is_empty() && !reply.text.is_empty() {
            on_token(&reply.text);
        }
        Ok(reply)
    }

    /// One step of the **single tool-calling conversation** (ADR 0053): given the
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
        // Bridge for butlers without native tool-calling. Tool results are folded into
        // the *conversation* as ordinary turns — never into the system prompt (ADR 0053
        // retires that channel): a result the model reads as a message it just received
        // is what keeps its answer grounded, and it keeps success and failure on
        // exactly the same footing.
        let history: Vec<ChatMessage> = conversation
            .iter()
            .filter_map(|m| match m {
                TurnMessage::User(text) => Some((MessageRole::User, text.clone())),
                TurnMessage::Assistant { text, .. } if !text.is_empty() => {
                    Some((MessageRole::Butler, text.clone()))
                }
                TurnMessage::ToolResult { content, .. } => Some((
                    MessageRole::User,
                    format!(
                        "[skill result] {content}\nAnswer from this. Relay what it actually \
                         says — including a failure — and add nothing that isn't here."
                    ),
                )),
                TurnMessage::Assistant { .. } => None,
            })
            .filter_map(|(role, text)| {
                ChatMessage::new(
                    MessageId::new(0),
                    role,
                    &text,
                    Timestamp::from_unix_millis(0),
                )
                .ok()
            })
            .collect();
        // Express any `capability_use` it returns as a `tool_call`, so the single loop
        // (ADR 0053) drives it just like a native tool-caller.
        let reply = self.respond(&history, preferences, context)?;
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

    /// Compacts a chunk of conversation (optionally folding a prior summary) into a
    /// short running summary — used to keep a long chat's prompt bounded without
    /// dropping the day's thread (ADR 0053 context compaction). The default returns an
    /// empty string, meaning "no compaction available"; the caller then simply keeps
    /// the recent verbatim window. A model-backed butler overrides it.
    ///
    /// # Errors
    /// [`ProposalError`] if a backing model is unreachable or returns nothing.
    fn summarize(&self, _prior_summary: &str, _transcript: &str) -> Result<String, ProposalError> {
        Ok(String::new())
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

// PreferenceRepository moved to the understanding context (ADR 0050);
// re-exported from `endora_application` (see lib.rs) so existing paths hold.

// ChatRepository moved to the conversation context (ADR 0050); re-exported from
// `endora_application` (see lib.rs) so existing paths hold.

// `Clock` and `IdSource` are shared-kernel ports (time and identity enter the
// pure layers through them), re-exported here so `endora_application::{Clock,
// IdSource}` and `ports::…` paths are unchanged. See ADR 0050.
pub use endora_kernel::{Clock, IdSource};

// The audit trail and the butler's event log — `AuditLog`, `EventLog`, and
// `ActivityEvent` — now belong to the platform context (ADR 0050). They are
// re-exported from `endora_application` (see lib.rs) so existing paths hold.
