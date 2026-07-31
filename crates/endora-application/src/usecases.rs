//! Use cases — the butler turn and the flows around it.
//!
//! These orchestrate the contexts and the ports. They are the seam the interfaces
//! (node, CLI) call; they hold no transport or storage detail. Identifiers and
//! time come from the [`IdSource`] and [`Clock`] ports, so the domain stays pure
//! and the use cases stay testable with fakes.

use endora_conversation::{ChatMessage, MessageRole};
use endora_kernel::ids::{
    AuditId, BeliefId, IntentionId, MessageId, OutcomeId, PreferenceId, Timestamp,
};
use endora_kernel::{Decision, Reversibility};
use endora_platform::AuditRecord;
use endora_understanding::{
    Belief, Intention, Outcome, Preference, PreferenceKind, Reaction, RepairProposal,
};

use endora_capabilities::CapabilityRunner;
use endora_conversation::ChatRepository;
use endora_platform::{AuditLog, EventLog};
use endora_understanding::{
    BeliefRepository, IntentionRepository, OutcomeRepository, PreferenceRepository,
};

use endora_scheduling::{
    BriefSchedule, BriefScheduleRepository, CheckinRepository, CheckinSchedule,
    NightlyLoopSchedule, NightlyLoopScheduleRepository,
};

use crate::error::AppError;
use crate::ports::{
    Butler, ButlerContext, ButlerReply, CapabilityTool, Clock, ConversationSummary,
    ConversationSummaryStore, DeepAsker, FormedBelief, IdSource, MemorySnapshot, MemoryStore,
    TurnMessage,
};

/// Exports everything the user has stored (a memory right).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn export_memory(store: &impl MemoryStore) -> Result<MemorySnapshot, AppError> {
    Ok(store.export()?)
}

/// Permanently deletes all of the user's stored data (a memory right).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn purge_memory(store: &impl MemoryStore) -> Result<(), AppError> {
    Ok(store.purge()?)
}

/// Returns the most recent audit records, newest first, up to `limit`.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
// `recent_audit` moved to the platform context (ADR 0050); re-exported so
// `usecases::recent_audit` paths are unchanged during the migration.
pub use endora_platform::recent_audit;

/// What kind of thing an [`ActivityItem`] records.
///
/// Kept coarse on purpose: the feed groups by what sort of event it was, and the
/// human-readable summary carries the specifics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    /// A consequential decision was made and audited (see [`AuditLog`]).
    Decision,
    /// Something the butler did or learned this turn, or a setting the person
    /// changed — the butler's own action log (see [`EventLog`]).
    Action,
}

impl ActivityKind {
    /// A stable, lowercase name, suitable for the protocol and the UI.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Action => "action",
        }
    }
}

/// One entry in the activity feed.
///
/// This is a **read projection**, not a domain aggregate: it merges the
/// persisted facts that already carry a time — audited decisions and the butler's
/// own action log — into a single "what happened" timeline. Because it is derived,
/// it stores nothing new and needs no schema of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityItem {
    at: Timestamp,
    kind: ActivityKind,
    summary: String,
}

impl ActivityItem {
    /// When the recorded event happened.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The area of the loop this entry belongs to.
    #[must_use]
    pub const fn kind(&self) -> ActivityKind {
        self.kind
    }

    /// The human-readable description of what happened.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

/// Returns the most recent activity, newest first, up to `limit` entries.
///
/// The feed is a projection over what is already persisted with a timestamp:
/// audited decisions and the butler's own action log. As more of the system gains
/// durable timestamps, this timeline widens without a protocol change.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn recent_activity(
    audit: &impl AuditLog,
    events: &impl EventLog,
    limit: usize,
) -> Result<Vec<ActivityItem>, AppError> {
    let mut items = Vec::new();
    for r in audit.recent(limit)? {
        items.push(ActivityItem {
            at: r.at(),
            kind: ActivityKind::Decision,
            summary: r.summary().to_owned(),
        });
    }
    for e in events.recent(limit)? {
        items.push(ActivityItem {
            at: e.at,
            kind: ActivityKind::Action,
            summary: e.summary,
        });
    }
    // Newest first; break ties stably so equal timestamps keep a deterministic
    // order across calls.
    items.sort_by(|a, b| {
        b.at.unix_millis()
            .cmp(&a.at.unix_millis())
            .then_with(|| b.summary.cmp(&a.summary))
    });
    items.truncate(limit);
    Ok(items)
}

/// Marks a tool result as **evidence** or as an unverified **receipt** (ADR 0053).
///
/// Endora's architecture principle is *models propose, policy authorizes,
/// capabilities execute, **evidence verifies***. The first three were built; the
/// fourth was not, and the turn simply believed whatever an actuator claimed. That
/// gap is not hypothetical: Home Assistant was asked to turn a light off with a tool
/// that only sets brightness, matched the targets, changed nothing, and answered
/// `action_done` — so the butler announced success while the light stayed on. No
/// amount of grounding helps a model whose tool told it everything went fine.
///
/// A capability in the [`Reversibility::Observe`] band *reports state*, so its result
/// is an observation and stands on its own. Anything else returns the actuator's
/// account of its own work, which is exactly the thing that can be wrong. Saying so
/// in the tool result is not narration on the butler's behalf (ADR 0053) — it is part
/// of the real outcome, and it is the honest default for every integration, including
/// ones nobody has debugged yet.
pub fn note_verification(
    output: &str,
    spec: Option<&crate::CapabilitySpec>,
    observation: Option<&str>,
) -> String {
    note_verification_against(output, spec, None, observation)
}

/// As [`note_verification`], but knowing what the world looked like **before**.
///
/// Only the nominated reader of a server is classified as observing; every other tool on
/// it is treated as an actuator, because the server says nothing trustworthy about which
/// is which and deny-by-default is right for *authorization* (ADR 0054). Reusing that
/// classification for *verification* is where it goes wrong. Observed live: a call asking
/// Home Assistant for the **time** came back with five kilobytes of every device in the
/// house attached, under the instruction "answer from the OBSERVATION". The model was told
/// to answer "what time is it" from a list of lamps, three times in one turn, and the
/// morning briefing it eventually produced was one sentence about a bedroom light.
///
/// The fix is not to guess which tools actuate. It is that **an observation showing nothing
/// moved has nothing to show**: [`note_unchanged`] already states that verdict in one
/// sentence, which is the sharp signal [0053](../../docs/adr/0053-honesty-about-what-it-did.md)
/// actually cares about. Pasting the reading as well adds no information, floods the
/// context, and misdirects the answer.
///
/// So the reading travels when it differs from what was there before — when something
/// genuinely moved and the detail is the evidence — and not otherwise. Nothing about the
/// honesty guarantee changes: every call is still read back, and every verdict is still
/// reported.
fn note_verification_against(
    output: &str,
    spec: Option<&crate::CapabilitySpec>,
    before: Option<&str>,
    observation: Option<&str>,
) -> String {
    let observes = spec.is_some_and(|s| s.reversibility == Reversibility::Observe);
    if observes {
        return output.to_owned();
    }
    let Some(observed) = observation else {
        return format!(
            "{output}\n\n[unverified] This is what the tool reported about its own work. \
             Endora has NOT independently confirmed the effect. Say what was reported and \
             that it is unconfirmed — do not state the world has changed as though you had \
             checked."
        );
    };
    if before.is_some_and(|b| b.trim() == observed.trim()) {
        // Identical to what was there before, so the reading is not evidence of anything
        // this call did. `note_unchanged` supplies the verdict; repeating the whole world
        // here would only bury it.
        return format!(
            "{output}\n\n[observed] Endora read the state back and it is unchanged from \
             before the call."
        );
    }
    format!(
        "{output}\n\n[observed] Endora then read the state back. This is what the world \
         actually looks like now:\n{observed}\n\nAnswer from the OBSERVATION, not from \
         what the tool claimed. If they disagree, the observation wins and you should say \
         the action did not take effect."
    )
}

/// Says so when the world is **identical before and after** an action.
///
/// The sharpest form of the failure ADR 0053 exists for. Asked to turn the kitchen
/// light off, Home Assistant reported *"completed successfully on: Kitchen (area),
/// Kitchen Table (light)"* — entirely true, and useless: `Kitchen Table` was
/// unavailable and `Kitchen Main`, the switch that was actually on, stayed on. A claim
/// can be accurate about what it touched and still change nothing.
///
/// Two readings settle it without interpreting either. This compares strings and knows
/// nothing about any server's format, so it holds for every integration — the
/// observation-derived fact ADR 0054 named, and the one that makes the loop accumulate
/// rather than needing a patch per quirk.
///
/// Silent unless both readings exist and match: without a before, or with no reader at
/// all, there is nothing to claim and the honest answer is to say nothing.
fn did_change(before: Option<&str>, after: Option<&str>) -> Option<bool> {
    let (before, after) = (before?, after?);
    Some(before.trim() != after.trim())
}

/// How long to let a service settle before looking a second time.
///
/// Long enough for a smart-home integration to report a state change back (they are
/// typically well under half a second on a local network), short enough that a person
/// waiting on a reply does not notice. Paid only when the first reading suggested nothing
/// happened — which is either a real no-op, where the wait is the cost of being sure, or a
/// race, where it is the difference between the truth and a false record.
const SETTLE_BEFORE_LOOKING_AGAIN: std::time::Duration = std::time::Duration::from_millis(700);

/// The tool-result note when the world **did** move.
///
/// `note_unchanged` has always spoken up when nothing happened, and said nothing when
/// something did — which leaves the model with only the service's own sentence to go on.
/// Live, that sentence is often a hedge ("Home Assistant accepted this; its reply says
/// nothing about the result"), because Home Assistant answers before its integrations
/// report back. Endora had already compared two readings and knew the answer; it just was
/// not saying it.
///
/// Stating the verdict is not narration on the model's behalf (ADR 0053) — it is reporting
/// an observation Endora made, which is the one thing it is entitled to assert.
fn note_changed(before: Option<&str>, after: Option<&str>) -> String {
    let (Some(before), Some(after)) = (before, after) else {
        return String::new();
    };
    if before.trim() == after.trim() {
        return String::new();
    }
    "\n\n[changed] Endora read the state before and after, and it is different — this \
     worked. Say so plainly; do not hedge about it."
        .to_owned()
}

/// The tool-result note for [`did_change`] — see it for why two readings settle this.
fn note_unchanged(before: Option<&str>, after: Option<&str>) -> String {
    let (Some(before), Some(after)) = (before, after) else {
        return String::new();
    };
    if before.trim() != after.trim() {
        return String::new();
    }
    "\n\n[unchanged] Endora read the state before and after, and it is identical — \
     whatever the tool reported, nothing actually changed. Say that plainly, and if \
     something in the reading looks like what they meant, name it and ask."
        .to_owned()
}

/// Reads the world back after an actuation, so the turn answers from what is
/// **observed** rather than from what the actuator claimed (ADR 0053).
///
/// Runs after a failure too, and deliberately: a failed action's most useful output
/// is what actually exists. The live failure this was built for — `HassTurnOff`
/// returning `no_match_reason=AREA` — is far more actionable once the reply also
/// carries the entities that *are* in that area.
///
/// Best-effort: if the read fails there is simply no observation, and the result
/// falls back to being marked unverified. Verification must never turn a working
/// action into a broken turn.
fn read_state_back(
    capabilities: &dyn CapabilityRunner,
    id: &str,
    action_input: &str,
) -> Option<String> {
    let verifier = capabilities.verifier(id)?;
    let spec = capabilities
        .available()
        .into_iter()
        .find(|c| c.id == verifier)?;
    if !spec.configured {
        return None;
    }
    // Narrow the reading to what the action was aimed at (ADR 0053). The capabilities
    // context owns schema knowledge, so it works out which of the action's targeting
    // arguments this reader also accepts.
    let input = capabilities.read_back_input(id, action_input);
    capabilities.run(&verifier, &input).ok()
}

/// The single tool-calling conversation (ADR 0053). Seeds the conversation from the
/// chat history, then drives [`Butler::take_turn`]: each tool call the model makes is
/// executed **through policy**, and its real result — success **or** error — is
/// appended as a [`TurnMessage::ToolResult`] the model answers from in-context, until
/// it replies with no tool call. Bounded by `max_rounds` and a per-turn failure cap.
///
/// The loop adds **no canned narration**: an un-runnable tool yields a factual tool
/// result (needs setup / needs confirmation / failed), and the model writes the
/// user-facing answer from it. If the model misreports with the truth in front of it,
/// that is a model failure to surface — not a string to hardcode (ADR 0053).
/// Ask the model for one turn, retrying a completely empty completion.
///
/// A slow local model occasionally returns *nothing* — no tool call and no text —
/// on a turn it should have acted on (observed ~1 in 3 on qwen2.5:14b). Left alone
/// that whiff surfaces as the canned "not sure how to help" fallback with an empty
/// activity log. Retrying is cheap (an empty completion returns near-instantly) and,
/// because sampling is non-deterministic, usually turns the retry into a real tool
/// call or answer. Bounded so a persistently mute model still returns promptly.
/// Whether a reply is not an answer at all — the model responding to its own plumbing
/// rather than to the person.
///
/// Two shapes, one meaning. Both were observed within an hour of each other:
///
/// ```text
/// "here are the appropriate function calls: 1. **GetWeather** ..."   (named, called none)
/// "None of the functions provided pertain to the 'news' domain."     (protocol words)
/// ```
///
/// Treating this as **no reply** rather than as a bad one is what keeps it to a single
/// idea: every path already knows what to do when the model says nothing. The turn
/// retries; a check-in stays quiet; a chat answer falls back. None of them needed a new
/// mechanism, only a truer notion of "nothing".
fn not_an_answer(reply: &ButlerReply, context: &ButlerContext) -> bool {
    reply.tool_calls.is_empty()
        && (reply.text.trim().is_empty()
            // The model was unreachable, so there is no answer here to keep — only an
            // apology for its absence. Retried where someone is waiting; silent where
            // nobody asked (ADR 0056).
            || reply.degraded
            || only_described_a_tool(reply, context)
            || sounds_like_plumbing(&reply.text))
}

/// Whether a message is about Endora's plumbing rather than about the person.
///
/// A deliberate, narrow heuristic — and named as one. Two kinds of phrase, both of which
/// mean the reply is about the machinery rather than about the person: words from the tool
/// protocol, and the model disclaiming its own nature.
///
/// The consequence differs by path, and neither loses an answer:
///
/// - **unprompted** (a check-in, a brief) — suppressed, because silence is already the
///   default there and a false positive costs one skipped message;
/// - **asked for** — *retried*, bounded, and if the retries run out the person sees the
///   reply anyway. Suppressing an answer someone asked for would be worse than an awkward
///   one, so nothing is ever withheld.
fn sounds_like_plumbing(text: &str) -> bool {
    const PLUMBING: &[&str] = &[
        "function call",
        "functions provided",
        "exposed entities",
        "no exposed",
        "json object",
        "placeholder argument",
        "the 'news' domain",
        "tool call",
        // Bare, because a butler has no reason to say it. Live, posted unprompted at
        // 04:18: "None of the functions listed involve a 'person' domain, so there's no
        // need…" — the list held "functions provided" and missed this by one word, which
        // is what a list of exact phrases will always do.
        "functions",
        // Breaking the frame is the same failure wearing different clothes: the reply is
        // about what the model is rather than about what was asked. Live, to "How has your
        // day been": "I'm an AI assistant without personal experiences, so I don't have a
        // day to reflect on." A butler with a persona floor (ADR 0056) does not answer a
        // pleasantry by disclaiming its own nature, and asking the prompt to prevent it is
        // the kind of guarantee this architecture keeps in code instead.
        "i'm an ai assistant",
        "i am an ai assistant",
        "as an ai language model",
        "as an ai assistant",
        "i don't have personal experiences",
        "without personal experiences",
        "i do not have feelings",
    ];
    let lowered = text.to_lowercase();
    PLUMBING.iter().any(|marker| lowered.contains(marker))
}

/// Whether the model wrote *about* the tools instead of using one.
///
/// Observed, in answer to "I usually want news weather traffic":
///
/// ```text
/// Based on your request ... here are the appropriate function calls:
/// 1. **GetWeather** - To fetch the current weather.
/// 2. **GetTraffic** - To get traffic updates.
/// Here are the JSON objects for these functions with placeholder arguments:
/// ```
///
/// Nothing ran. A weak model asked to answer with tools available sometimes narrates the
/// call it would make rather than making it, and the person gets an essay about function
/// names instead of their weather.
///
/// The signal is taken from the **catalogue offered this turn**, not from a list of
/// suspicious words: naming a tool it was handed, while calling none of them, is
/// describing rather than doing. A reply that mentions no offered tool is left alone
/// however it is phrased.
fn only_described_a_tool(reply: &ButlerReply, context: &ButlerContext) -> bool {
    if !reply.tool_calls.is_empty() {
        return false;
    }
    let text = reply.text.to_lowercase();
    context.tools.iter().any(|tool| {
        // The bare name, since the model writes `GetWeather` rather than
        // `home-assistant.GetWeather`.
        let bare = tool
            .id
            .rsplit('.')
            .next()
            .unwrap_or(&tool.id)
            .to_lowercase();
        bare.len() > 3 && text.contains(&bare)
    })
}

fn take_turn_retrying_empty(
    butler: &dyn Butler,
    conversation: &[TurnMessage],
    prefs: &[Preference],
    context: &ButlerContext,
    on_token: &mut dyn FnMut(&str),
) -> Result<ButlerReply, AppError> {
    const MAX_EMPTY_RETRIES: usize = 2;
    let mut reply = butler
        .take_turn_streaming(conversation, prefs, context, on_token)
        .map_err(|e| AppError::Model {
            message: e.to_string(),
        })?;
    let mut retries = 0;
    while gave_nothing_useful(&reply, conversation, context) && retries < MAX_EMPTY_RETRIES {
        retries += 1;
        // A retry only happens when the round produced NOTHING, so nothing was streamed
        // and there is nothing for the person to see rewritten.
        reply = butler
            .take_turn_streaming(conversation, prefs, context, on_token)
            .map_err(|e| AppError::Model {
                message: e.to_string(),
            })?;
    }
    // The local model has now failed the same deterministic check three times. Asking it a
    // fourth time is the definition of not learning, so ask a different one — once
    // (ADR 0055).
    //
    // Reliability compounds: two independent attempts at p₁ and p₂ fail together only at
    // (1-p₁)(1-p₂), which is how a second model beats a bigger one without waiting for a
    // bigger one to exist. What makes this safe rather than clever is that the **trigger is
    // code**: a check Endora applied to the reply, never the model's own opinion of how it
    // did.
    if let Some(deeper) = butler
        .deeper()
        .filter(|_| gave_nothing_useful(&reply, conversation, context))
    {
        // Not streamed. The person has already seen whatever the local attempts emitted, and
        // a second voice writing into the same bubble would read as one confused answer.
        if let Ok(better) = deeper.take_turn(conversation, prefs, context) {
            if !gave_nothing_useful(&better, conversation, context) {
                return Ok(ButlerReply {
                    escalated: true,
                    ..better
                });
            }
        }
    }
    Ok(reply)
}

/// Whether a reply is worth keeping — [`not_an_answer`], plus saying the same thing again.
///
/// Repetition needs the conversation, which is why it lives here rather than in
/// `not_an_answer`. Observed live: asked what it had done while the person was out, the
/// butler reproduced **word for word** the answer it had given the previous day, with the
/// day's real record sitting in its context. A reply that only repeats the last one has
/// added nothing, and asking again is free where the person is waiting.
fn gave_nothing_useful(
    reply: &ButlerReply,
    conversation: &[TurnMessage],
    context: &ButlerContext,
) -> bool {
    not_an_answer(reply, context) || repeats_its_last_answer(reply, conversation)
}

/// Whether this reply is the butler's previous one again.
///
/// Compared on normalized words so trivial re-wording still counts, and only against the
/// **most recent** butler turn — a butler that answers "it's off" twice about the same light
/// on consecutive turns is correct, and only becomes suspect when nothing else was said in
/// between.
fn repeats_its_last_answer(reply: &ButlerReply, conversation: &[TurnMessage]) -> bool {
    let text = reply.text.trim();
    // Short confirmations are legitimately identical — "Done." twice is not a failure.
    const LONG_ENOUGH_TO_BE_A_CLAIM: usize = 40;
    if text.len() < LONG_ENOUGH_TO_BE_A_CLAIM || !reply.tool_calls.is_empty() {
        return false;
    }
    conversation
        .iter()
        .rev()
        .find_map(|m| match m {
            TurnMessage::Assistant { text, .. } if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        })
        .is_some_and(|last| words_of(&last) == words_of(text))
}

/// Tool rounds allowed in a **chat** turn. `run_tool_turn` already answers after one
/// tool round (retrying only on failure), so this only bounds a pathological loop —
/// kept low to stay fast on a slow local model.
const CHAT_TOOL_ROUNDS: usize = 3;
/// Tool rounds allowed in a proactive **check-in** — it mostly just needs to look.
const CHECKIN_TOOL_ROUNDS: usize = 3;
/// Tool rounds allowed in a **brief**, which legitimately gathers several things
/// (weather, alerts, news) before writing.
const BRIEF_TOOL_ROUNDS: usize = 6;
/// Tool rounds allowed in the **nightly loop** — it researches one focus, unhurried.
const NIGHTLY_TOOL_ROUNDS: usize = 4;

/// Where a turn's actions are recorded, and what motivated them (ADR 0053).
///
/// Grouped rather than threaded as two more loose parameters, because they are one
/// concern: an outcome and the belief it traces back to. `motivated_by` is `Some` only
/// where the turn genuinely has a reason on file — the nightly loop acting on its focus
/// belief — and `None` when the person simply asked for something.
pub(crate) struct OutcomeSink<'a> {
    outcomes: &'a dyn OutcomeRepository,
    motivated_by: Option<BeliefId>,
}

impl<'a> OutcomeSink<'a> {
    /// A sink for a turn with no belief behind it — the person asked.
    pub(crate) const fn unmotivated(outcomes: &'a dyn OutcomeRepository) -> Self {
        Self {
            outcomes,
            motivated_by: None,
        }
    }

    /// A sink for a turn Endora took because of something it believes.
    pub(crate) const fn motivated_by(
        outcomes: &'a dyn OutcomeRepository,
        belief: Option<BeliefId>,
    ) -> Self {
        Self {
            outcomes,
            motivated_by: belief,
        }
    }

    /// Records what an action claimed and what was observed afterwards.
    ///
    /// Skips the `Observe` band: a read changes nothing, so there is no outcome to have
    /// — its result is already evidence (ADR 0053). A capability with no visible spec is
    /// treated as an actuator and recorded, matching the deny-by-default rule elsewhere.
    ///
    /// **Best-effort.** A failed write is swallowed: recording what happened must never
    /// break a working action, the same rule ADR 0053 set for verification.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit dependencies, no hidden state"
    )]
    fn record(
        &self,
        spec: Option<&crate::CapabilitySpec>,
        capability: &str,
        input: &str,
        claim: &str,
        observation: Option<&str>,
        changed: Option<bool>,
        ids: &impl IdSource,
        clock: &impl Clock,
    ) -> Option<OutcomeId> {
        if spec.is_some_and(|s| s.reversibility == Reversibility::Observe) {
            return None;
        }
        let Ok(outcome) = Outcome::record(
            OutcomeId::new(ids.new_id()),
            capability,
            input,
            claim,
            observation,
            clock.now(),
            self.motivated_by,
            changed,
        ) else {
            return None;
        };
        let _ = self.outcomes.save(&outcome);
        Some(outcome.id())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
fn run_tool_turn(
    butler: &dyn Butler,
    capabilities: &dyn CapabilityRunner,
    audit: &dyn AuditLog,
    actions: &OutcomeSink<'_>,
    ids: &impl IdSource,
    clock: &impl Clock,
    history: &[ChatMessage],
    prefs: &[Preference],
    context: &ButlerContext,
    max_rounds: usize,
    on_step: &mut dyn FnMut(ButlerStep),
    on_token: &mut dyn FnMut(&str),
    activity: &mut Vec<String>,
    disclosures: &mut Vec<ActionDisclosure>,
) -> Result<ButlerReply, AppError> {
    // Stop hammering a dead end: after this many failed runs in a turn, stop executing
    // and let the model answer from what it has.
    const MAX_TOOL_FAILURES: usize = 2;
    // Seed the conversation with the plain chat so far.
    let mut conversation: Vec<TurnMessage> = history
        .iter()
        .map(|m| match m.role() {
            MessageRole::User => TurnMessage::User(m.text().to_owned()),
            MessageRole::Butler => TurnMessage::Assistant {
                text: m.text().to_owned(),
                tool_calls: Vec::new(),
            },
        })
        .collect();
    let mut failures = 0usize;
    // Whether the most recent action errored — see the recovery branch below.
    let mut last_action_failed = false;
    // Tool calls already made this turn (capability + input), to stop the model from
    // looping the same call — especially a read-only one that succeeds every time and
    // so never trips the failure cap.
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    for round in 0..=max_rounds {
        let reply = take_turn_retrying_empty(butler, &conversation, prefs, context, on_token)?;
        // No tool call → the final answer, grounded in the tool results so far — unless
        // the last thing that happened was a FAILED action and there is budget left.
        //
        // Observed live, twice: one call failed, the read-back came back naming what is
        // really there, and the model answered "Let's try again… Here is the request:"
        // and stopped. It wrote the preamble to a tool call instead of making one, and
        // the loop took that prose as its final word.
        //
        // This is loop policy, not persuasion: nothing is added to the prompt and the
        // model is told nothing. It simply is not taken at its word that it is finished
        // while the only thing it has done is fail. The existing round and failure caps
        // still bound everything, so a model that really is done just answers again.
        if reply.tool_calls.is_empty() {
            if last_action_failed && round < max_rounds && failures < MAX_TOOL_FAILURES {
                last_action_failed = false;
                conversation.push(TurnMessage::Assistant {
                    text: reply.text.clone(),
                    tool_calls: Vec::new(),
                });
                continue;
            }
            return Ok(reply);
        }
        // Out of rounds with tools still pending — fall through to a forced answer.
        if round == max_rounds {
            break;
        }
        // Record the assistant's tool-call turn, then run each call and append its
        // real result (OpenAI requires the assistant turn before its tool results).
        conversation.push(TurnMessage::Assistant {
            text: reply.text.clone(),
            tool_calls: reply.tool_calls.clone(),
        });
        for call in &reply.tool_calls {
            let id = call.capability.clone();
            // Action budget spent: don't execute more, but tell the model so it wraps
            // up (a system fact fed back as a tool result — not a user-facing string).
            if failures >= MAX_TOOL_FAILURES {
                conversation.push(TurnMessage::ToolResult {
                    call_id: call.id.clone(),
                    content: "not run — reached this turn's action limit; answer with what you \
                              have."
                        .to_owned(),
                });
                continue;
            }
            // Don't loop the same call: if this exact tool + input already ran this
            // turn, hand back a nudge instead of re-running it.
            // Compared by what the arguments *mean*, not how they are spelled. The
            // capabilities context owns tool-argument knowledge; this layer only asks.
            if !seen.insert((
                id.clone(),
                endora_capabilities::same_call_as(&call.input_json),
            )) {
                conversation.push(TurnMessage::ToolResult {
                    call_id: call.id.clone(),
                    content: "you already called this with the same input this turn — use the \
                              earlier result or answer now; do not repeat it."
                        .to_owned(),
                });
                continue;
            }
            let spec = capabilities.available().into_iter().find(|c| c.id == id);
            let cleared = spec.as_ref().is_some_and(|s| s.configured && s.autonomous);
            let (status, content) = if cleared {
                on_step(ButlerStep {
                    skill: id.clone(),
                    status: StepStatus::Running,
                    label: progress_label(&id),
                    output: None,
                });
                // Look BEFORE acting, so "did anything change?" is answerable at all.
                // A tool that claims success having changed nothing is the failure
                // ADR 0053 was built for, and comparing two readings settles it without
                // interpreting either — no knowledge of any server's format required.
                let before = read_state_back(capabilities, &id, &call.input_json);
                match capabilities.run(&id, &call.input_json) {
                    Ok(out) => {
                        last_action_failed = false;
                        activity.push(format!("Used the {id} skill"));
                        // Evidence verifies (ADR 0053): look at the world rather than
                        // taking the actuator's word for what it did.
                        let observed = read_state_back(capabilities, &id, &call.input_json);
                        // …but look again if it seems nothing moved. A service that
                        // updates state asynchronously answers the call first and changes
                        // the world a moment later, so the first reading races the change
                        // and loses. Observed: a light the person watched go off was
                        // recorded `changed: false`, and the butler then told them it
                        // could not turn it off — Endora asserting a falsehood and the
                        // model faithfully repeating it.
                        //
                        // Only on the "looks unchanged" path, so a change seen straight
                        // away costs nothing, and only once.
                        let observed =
                            if did_change(before.as_deref(), observed.as_deref()) == Some(false) {
                                std::thread::sleep(SETTLE_BEFORE_LOOKING_AGAIN);
                                read_state_back(capabilities, &id, &call.input_json).or(observed)
                            } else {
                                observed
                            };
                        if observed.is_some() {
                            activity.push(format!("Checked the result of {id}"));
                        }
                        // Memory learns (ADR 0053): the claim and the observation are
                        // kept, apart and unreconciled, so "did that help?" has
                        // something to be answered from later.
                        let recorded = actions.record(
                            spec.as_ref(),
                            &id,
                            &call.input_json,
                            &out,
                            observed.as_deref(),
                            did_change(before.as_deref(), observed.as_deref()),
                            ids,
                            clock,
                        );
                        disclose(
                            disclosures,
                            spec.as_ref(),
                            &id,
                            &out,
                            observed.as_deref(),
                            recorded,
                        );
                        (
                            StepStatus::Done,
                            note_verification_against(
                                &out,
                                spec.as_ref(),
                                before.as_deref(),
                                observed.as_deref(),
                            ) + &note_unchanged(before.as_deref(), observed.as_deref())
                                + &note_changed(before.as_deref(), observed.as_deref()),
                        )
                    }
                    Err(e) => {
                        failures += 1;
                        last_action_failed = true;
                        activity.push(format!("Tried the {id} skill, but it failed"));
                        // Read back on failure too: a failed action's most useful
                        // output is what actually exists, which is what lets the
                        // model retry against reality instead of guessing again.
                        //
                        // Deliberately UNSCOPED here, unlike the success path. When an
                        // action fails, its target is the prime suspect — "Area 'Kitchen
                        // Main' does not exist" — so reading back with the same target
                        // fails in exactly the same way and tells the model nothing.
                        // Observed live: the scoped read returned the identical error,
                        // twice, and the butler was left insisting there were no lights
                        // in a kitchen that had five. Widen when something went wrong.
                        let observed = read_state_back(capabilities, &id, "{}");
                        // A failed action is still something that happened, and its
                        // read-back is the most useful thing about it (ADR 0053), so it
                        // is recorded like any other.
                        let recorded = actions.record(
                            spec.as_ref(),
                            &id,
                            &call.input_json,
                            &format!("error: {e}"),
                            observed.as_deref(),
                            // The failure read-back is deliberately unscoped, so it is
                            // not comparable with the scoped `before` — and an action
                            // that errored made no claim to check anyway.
                            None,
                            ids,
                            clock,
                        );
                        disclose(
                            disclosures,
                            spec.as_ref(),
                            &id,
                            &format!("error: {e}"),
                            observed.as_deref(),
                            recorded,
                        );
                        let observed = observed.map_or_else(String::new, |o| {
                            format!(
                                "\n\n[observed] Endora read the state back anyway. \
                                 This is what is actually there:\n{o}"
                            )
                        });
                        (StepStatus::Failed, format!("error: {e}{observed}"))
                    }
                }
            } else {
                // Not cleared to run: audit the policy verdict and hand the model a
                // factual tool result so it answers honestly, in its own voice.
                let decision = capabilities.decision(&id);
                if spec.as_ref().is_some_and(|s| s.configured) {
                    if let Some(summary) = match decision {
                        Some(Decision::Block) => Some(format!(
                            "Policy blocked the '{id}' skill — irreversible and not opened"
                        )),
                        Some(Decision::Confirm) => Some(format!(
                            "Policy required confirmation for the '{id}' skill (not run on its own)"
                        )),
                        _ => None,
                    } {
                        if let Ok(rec) =
                            AuditRecord::new(AuditId::new(ids.new_id()), clock.now(), &summary)
                        {
                            let _ = audit.append(&rec);
                        }
                    }
                }
                let content = match (spec.as_ref(), decision) {
                    (Some(s), _) if !s.configured => {
                        format!("'{id}' is not set up — you can't use it; tell them plainly.")
                    }
                    (Some(_), Some(Decision::Block)) => {
                        format!("'{id}' isn't allowed yet — you can't run it even if asked.")
                    }
                    (Some(_), _) => {
                        format!("'{id}' needs their go-ahead — ask; don't claim you did it.")
                    }
                    (None, _) => format!("no such skill '{id}' — you can't do that."),
                };
                activity.push(format!(
                    "Couldn't use {id} (off, not set up, or needs confirming)"
                ));
                (StepStatus::Blocked, content)
            };
            on_step(ButlerStep {
                skill: id.clone(),
                status,
                label: progress_label(&id),
                output: Some(content.clone()),
            });
            conversation.push(TurnMessage::ToolResult {
                call_id: call.id.clone(),
                content,
            });
        }
        // Let the model decide when it's done: the next round it either calls another
        // tool (e.g. it read state first and now acts on it) or answers with no tool
        // call, which exits the loop above. We do NOT force an answer after one round —
        // that killed the read-then-act pattern (a "turn off the light" that first
        // checked state would stop at the check). The loop stays bounded regardless by
        // MAX_TOOL_ROUNDS, the failure cap, and the repeated-call guard, so a slow model
        // still can't run away.
    }
    // Reached here after acting (or the round cap): force the final answer with tools
    // OFF, so the model replies in prose grounded in what it gathered rather than
    // calling yet another tool and leaving the turn without an answer.
    let mut final_ctx = context.clone();
    final_ctx.tools = Vec::new();
    take_turn_retrying_empty(butler, &conversation, prefs, &final_ctx, on_token)
}

/// Sends a message to the butler and records both turns.
///
/// Appends the person's message, runs the butler's turn, records the reply, and
/// returns it together with a plain-language trail of what the butler did.
///
/// # Errors
/// [`AppError::Domain`] if the message text is blank, [`AppError::Model`] if the
/// butler brain is unavailable, or [`AppError::Repository`] if persistence fails.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn send_to_butler(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
) -> Result<(ChatMessage, Vec<String>), AppError> {
    // Non-streaming is just streaming with the tokens and steps discarded. Deep
    // escalation is wired on the streaming (primary chat) path; None here.
    send_to_butler_streaming(
        chat,
        preferences,
        beliefs,
        outcomes,
        capabilities,
        butler,
        audit,
        None,
        None,
        ids,
        clock,
        context,
        text,
        &mut |_| {},
        &mut |_| {},
        &mut Vec::new(),
    )
}

/// Where a [`ButlerStep`] is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// The skill is running right now.
    Running,
    /// The skill returned a result.
    Done,
    /// The skill ran but failed to produce a result.
    Failed,
    /// The skill couldn't run — off, not set up, or it needs confirmation.
    Blocked,
}

impl StepStatus {
    /// A stable lowercase tag for the wire/UI.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}

/// One live step in the butler's agentic loop, surfaced to the UI as it works —
/// a Claude-Code-style expandable action trail, kept distinct from the reply
/// prose (which streams separately as tokens). Steps are emitted sequentially:
/// a skill's [`StepStatus::Running`] is always followed by its terminal
/// [`Done`](StepStatus::Done)/[`Failed`](StepStatus::Failed); a
/// [`Blocked`](StepStatus::Blocked) step is terminal on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButlerStep {
    /// The capability id this step concerns (e.g. `weather`).
    pub skill: String,
    /// Where the step is in its lifecycle.
    pub status: StepStatus,
    /// A short, present-tense label ("Checking the weather").
    pub label: String,
    /// The skill's raw result on a terminal step, for the UI to reveal on demand
    /// (what the butler actually got back). `None` while running or when blocked.
    pub output: Option<String>,
}

/// Records an actuation for the person to see, whatever the reply ends up saying
/// (ADR 0053).
///
/// Skips the `Observe` band on the same reasoning as the outcome record: a read changes
/// nothing, so there is nothing to disclose about it. A capability with no visible spec
/// is treated as an actuator, matching deny-by-default everywhere else.
fn disclose(
    disclosures: &mut Vec<ActionDisclosure>,
    spec: Option<&crate::CapabilitySpec>,
    skill: &str,
    claimed: &str,
    observed: Option<&str>,
    outcome: Option<OutcomeId>,
) {
    if spec.is_some_and(|s| s.reversibility == Reversibility::Observe) {
        return;
    }
    // Deliberately NOT trimmed down when the reading is unchanged, unlike what the model
    // is sent. The two have opposite needs: the model gets a reading it must not answer
    // from, and a person gets the evidence. "The switch is still on" next to a claim of
    // `action_done` is the whole disclosure — replacing it with "nothing changed" would
    // take away the fact and leave the verdict, which is the wrong way round (ADR 0053).
    disclosures.push(ActionDisclosure {
        skill: skill.to_owned(),
        claimed: claimed.trim().to_owned(),
        observed: observed.map(|o| o.trim().to_owned()),
        outcome: outcome.map(|id| id.value().to_string()),
    });
}

/// What to say when the model produced no words but the turn did something.
///
/// Observed: a light was switched on by direct reach, the model returned an empty reply,
/// and Endora answered "I'm not sure how to help with that yet" — an apology for work it
/// had just completed. The person then cannot tell whether to try again.
///
/// The tool's own report is quoted and attributed rather than paraphrased, because Endora
/// is entitled to say what a tool told it and not to invent a summary of it.
fn acted_note(disclosures: &[ActionDisclosure]) -> Option<String> {
    let done: Vec<&ActionDisclosure> = disclosures
        .iter()
        .filter(|d| !d.claimed.trim_start().starts_with("error:"))
        .collect();
    let first = done.first()?;
    // The claim's opening line: enough to say what happened, without pasting a reading.
    let said = first
        .claimed
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("it reported success")
        .trim();
    let more = match done.len() {
        1 => String::new(),
        n => format!(" ({} actions in all — the trail below has each one.)", n),
    };
    Some(format!("Done. {} reported: {said}{more}", first.skill))
}

/// The facts behind an answer, for the things it named.
///
/// ADR 0053 verifies what Endora **did** and has never verified what it **says**. On a turn
/// that answers a question, Endora is holding the very reading the answer came from — so
/// the facts were available all along and simply never shown. Live, that gap produced
/// "the kitchen table light is already on" about a light that was off, and "several lights
/// are on" when the reading listed exactly nine.
///
/// This does not judge the prose. It **discloses** — the same move the action trail makes,
/// applied to an answer: whatever the reply named, here is what the service says about it,
/// so the person can see in one glance whether the two agree. Correcting the model would
/// mean understanding the sentence; showing the facts does not.
///
/// Only names the reply actually mentions, longest first so `Kitchen Main Light` is
/// matched before `Kitchen`, and capped — a wall of state is its own kind of noise.
fn facts_behind(text: &str, mut states: Vec<(String, String)>) -> String {
    const SHOWN: usize = 5;
    const WORTH_MATCHING: usize = 4;
    let words = words_of(text);
    // A reply that asserts nothing about state needs no facts about state. Live, under a
    // greeting — "How can I assist you with your home automation setup?" — this appended
    // `[state] Home is 0`, because the house contains something called `Home` and the word
    // "home" was in the sentence. Technically an exact match, and completely meaningless.
    //
    // The vocabulary for "is this about state?" comes from the **reading**, never from a
    // list of English words: whatever values this service just reported are exactly the
    // words a reply asserting state would use. A service that reports `open`/`closed` is
    // handled by the same code as one reporting `on`/`off`, and one whose vocabulary
    // Endora has never seen contributes nothing — the same rule as ADR 0054's categories.
    let asserts_a_state = states
        .iter()
        .any(|(_, state)| contains_all_words(&words, &words_of(state)));
    if !asserts_a_state {
        return String::new();
    }
    states.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut said: Vec<String> = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    for (name, state) in states {
        if name.len() < WORTH_MATCHING || said.len() >= SHOWN {
            continue;
        }
        let name_words = words_of(&name);
        // Whole words, not a substring: `Home` must not match "homework", and `Den` must
        // not match "identify". The previous version compared raw substrings.
        if !contains_all_words(&words, &name_words) {
            continue;
        }
        // Skip a name already contained in a longer one that was matched, so a reply
        // mentioning `Kitchen Main Light` does not also report `Kitchen`.
        if name_words.iter().all(|w| covered.contains(w)) {
            continue;
        }
        covered.extend(name_words);
        said.push(format!("{name} is {state}"));
    }
    if said.is_empty() {
        return String::new();
    }
    format!("\n\n[state] {}", said.join(" · "))
}

/// Splits text into lowercase alphanumeric words, so matching happens on whole words.
fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `words` contains `wanted` as a **contiguous run** of whole words.
fn contains_all_words(words: &[String], wanted: &[String]) -> bool {
    if wanted.is_empty() || wanted.len() > words.len() {
        return false;
    }
    words.windows(wanted.len()).any(|run| run == wanted)
}

/// Appends what Endora actually did, when the reply makes a claim about its own activity.
///
/// The third attempt at this, and the first that does not depend on the model. Asked "did you
/// do anything while I was out?", the butler:
///
/// 1. answered out of the house's lights — so Endora's record became a **skill**;
/// 2. reached past that skill and called `HassTurnOn`, trying to switch a light on — so the
///    account was **stated in the turn's context** instead;
/// 3. ignored the context and repeated, word for word, the answer it had given the previous
///    day: *"no specific activities were recorded."*
///
/// Eleven things were in the record at that moment, including the three messages it had sent
/// unprompted. Each fix was a better place to put the facts, and all three were still asking
/// the model to use them.
///
/// So this is the mechanism [0053](../../docs/adr/0053-honesty-about-what-it-did.md) already
/// uses for exactly this shape of problem: **a claim and the record disagree, so the record is
/// appended.** Nothing is rewritten and nothing is judged — the account is put next to the
/// sentence and the person can see which to believe.
///
/// The trigger is a claim *about activity in general*, matched on a deliberately tiny
/// vocabulary — `activity`, `activities`, `proactive`, `recorded` — words that are rare in
/// ordinary butler prose and are precisely what appears when this question is answered badly.
/// A heuristic, and named as one; both ways of being wrong are cheap. A false positive appends
/// true facts to a reply that did not need them; a false negative leaves things as they were.
fn account_behind(text: &str, did_lately: &[String]) -> String {
    const CLAIMS_ABOUT_ACTIVITY: &[&str] = &["activity", "activities", "proactive", "recorded"];
    if did_lately.is_empty() {
        return String::new();
    }
    let words = words_of(text);
    if !CLAIMS_ABOUT_ACTIVITY
        .iter()
        .any(|marker| words.iter().any(|w| w == marker))
    {
        return String::new();
    }
    format!("\n\n[did] {}", did_lately.join("\n"))
}

/// The line to append when a turn tried to change something and changed nothing.
///
/// Empty in every other case — including a turn that took no action at all, where there
/// is nothing to correct, and a turn where anything succeeded, where "nothing changed"
/// would itself be false.
///
/// Only actuating actions are considered, because reads never reach the disclosure list.
fn nothing_changed_note(disclosures: &[ActionDisclosure]) -> String {
    let tried_and_failed = !disclosures.is_empty()
        && disclosures
            .iter()
            .all(|d| d.claimed.trim_start().starts_with("error:"));
    if !tried_and_failed {
        return String::new();
    }
    // The person gets what the model was given. When a name did not match, Endora already
    // worked out what does exist; sending that only to the model wasted it — observed, the
    // reply offered to "check the living room instead" while the shortlist sat unread in
    // the tool result.
    let names = candidates_offered(disclosures);
    if names.is_empty() {
        return "\n\n(Nothing was changed — everything I tried failed.)".to_owned();
    }
    format!(
        "\n\n(Nothing was changed — everything I tried failed. These exist and look like \
         what you asked for: {}.)",
        names.join(", ")
    )
}

/// The names the target search offered this turn, lifted back out of the tool results.
///
/// Reading Endora's own marker, not a server's format — the one place that is fair game,
/// because this text was written here.
fn candidates_offered(disclosures: &[ActionDisclosure]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for claimed in disclosures.iter().map(|d| d.claimed.as_str()) {
        let Some(block) = claimed.split("[candidates]").nth(1) else {
            continue;
        };
        for line in block.lines() {
            let Some(name) = line.trim().strip_prefix("- ") else {
                continue;
            };
            let name = name.trim();
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
    }
    names.truncate(4);
    names
}

/// One action a turn took, and whether Endora saw the effect for itself (ADR 0053).
///
/// The **deterministic** half of honesty about actions. [ADR 0053](../../docs/adr/0034-evidence-verifies.md)
/// put the read-back in the tool result and asked the model to respect it; measured, a
/// weak local model ignores that instruction — it asserts an unverified success every
/// time. So the guarantee stops being *"the butler will report this honestly"* (a claim
/// about a model) and becomes *"the person can always see it"* (a claim about code).
///
/// This never touches `reply.text`. The butler writes what it writes; this sits beside
/// it, the same way the activity trail already does. Putting words in the butler's mouth
/// is what ADR 0053 forbids — showing the person what happened is not that.
///
/// It derives no verdict. Deciding *contradicted* versus *confirmed* needs a model of
/// what the caller intended, which does not exist (ADR 0053); the claim and the reading
/// are shown side by side and the person judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDisclosure {
    /// The capability that acted.
    pub skill: String,
    /// The actuator's own account of its work — success text, or the error.
    pub claimed: String,
    /// What Endora observed afterwards, when the integration has a reader. `None` means
    /// nothing could check — which is the fact worth surfacing, not a gap to hide.
    pub observed: Option<String>,
    /// The stored outcome this disclosure describes, so the person can say how it landed
    /// from **where they already are** (ADR 0053).
    ///
    /// `None` when nothing was recorded — a read, or a failed write. The machinery for
    /// judging an outcome was complete for months and had never once been used, because
    /// the only place to do it was a section further down a screen nobody opens, which by
    /// its own design never asked. A loop with no input is not a loop.
    pub outcome: Option<String>,
}

impl ActionDisclosure {
    /// Whether Endora saw the effect for itself rather than only being told about it.
    #[must_use]
    pub const fn was_observed(&self) -> bool {
        self.observed.is_some()
    }
}

/// Bounds the prompt for a long chat (ADR 0053): returns the recent verbatim window
/// plus a compact running summary of everything before it. The summary is extended
/// (one model call) only when new messages have scrolled past the window since it was
/// last built; otherwise the cached summary is reused. If summarising isn't available
/// (the default butler) or fails, the recent window still stands — just without the
/// earlier context folded in.
fn compact_history(
    butler: &dyn Butler,
    summary: &dyn ConversationSummaryStore,
    full: &[ChatMessage],
    window: usize,
) -> (Vec<ChatMessage>, Option<String>) {
    if full.len() <= window {
        return (full.to_vec(), None);
    }
    // Fold the overflow into the summary only once enough new messages have piled up —
    // NOT every turn. Each turn adds messages, so the window boundary advances every
    // turn; summarising on every advance would pay a slow model call per turn on a long
    // chat (which is exactly what degraded late turns).
    const SUMMARY_BATCH: usize = 12;
    // Hard cap on how many recent messages ride verbatim, so the prompt can't blow up
    // even when the summary is behind: the tail window plus at most one pending batch.
    let max_verbatim = window + SUMMARY_BATCH;
    let boundary = full.len() - window; // messages [0..boundary) are older than the tail window
    let mut current = summary.get().unwrap_or_default();
    if boundary >= current.covered + SUMMARY_BATCH {
        // Fold exactly ONE batch per turn, not the whole backlog. A big backlog — e.g.
        // after a restart cleared the in-memory summary — is then caught up over a few
        // turns, each a small, fast call rather than one giant summarisation that would
        // itself time out on a slow model.
        let end = current.covered + SUMMARY_BATCH;
        let chunk = render_transcript(&full[current.covered..end]);
        if let Ok(s) = butler.summarize(&current.text, &chunk) {
            let s = s.trim();
            if !s.is_empty() {
                current = ConversationSummary {
                    text: s.to_owned(),
                    covered: end,
                };
                summary.set(current.clone());
            }
        }
        // Summariser unavailable/failed: keep the prior summary and coverage; the cap
        // below still bounds the prompt.
    }
    let text = (!current.text.is_empty()).then_some(current.text);
    // Verbatim = the not-yet-summarised tail, but never more than `max_verbatim`. If the
    // summary is lagging (summariser failing), drop the oldest beyond the cap rather than
    // send a giant prompt — lost detail beats a timeout, and durable facts live in
    // beliefs, not the transcript.
    let start = current.covered.max(full.len().saturating_sub(max_verbatim));
    (full[start..].to_vec(), text)
}

/// Renders messages as a plain `User:/Butler:` transcript, for summarising.
fn render_transcript(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            let who = match m.role() {
                MessageRole::User => "User",
                MessageRole::Butler => "Butler",
            };
            format!("{who}: {}", m.text())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Like [`send_to_butler`], but streams the reply's prose to `on_token` as the
/// butler produces it (for a live, token-by-token chat). The person's message is
/// persisted **before** the butler is called, and the butler's reply is persisted
/// once complete — so the exchange survives a reload even if the stream is
/// interrupted mid-way (the last stored message is then still the person's).
///
/// # Errors
/// [`AppError::Model`] if the butler fails, or [`AppError::Repository`] on a
/// backend failure.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn send_to_butler_streaming(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    deep: Option<&dyn DeepAsker>,
    summary: Option<&dyn ConversationSummaryStore>,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
    on_token: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(ButlerStep),
    disclosures: &mut Vec<ActionDisclosure>,
) -> Result<(ChatMessage, Vec<String>), AppError> {
    let user = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        text,
        clock.now(),
    )?;
    chat.append(&user)?;

    // CONTEXT COMPACTION (ADR 0053): send the model a bounded prompt — a compact running
    // summary of earlier turns + a recent verbatim window — not the whole transcript.
    // A big prompt slows a local model past the timeout, so a long chat would otherwise
    // degrade; but a butler shouldn't forget the day, so the overflow is summarised
    // rather than dropped. The summary is regenerated only when the window overflows
    // FURTHER (new messages scroll past it), so most turns pay nothing.
    const RECENT_WINDOW: usize = 12;
    let full = chat.list()?;
    let (history, summary_text) = match summary {
        Some(store) => compact_history(butler, store, &full, RECENT_WINDOW),
        // No summary store (non-streaming path/tests): just bound with the recent
        // window, no summarising.
        None => (
            full.iter()
                .skip(full.len().saturating_sub(RECENT_WINDOW))
                .cloned()
                .collect(),
            None,
        ),
    };
    let mut context = context.clone();
    context.conversation_summary = summary_text;
    let context = &context;
    let prefs = preferences.list_all()?;

    // `activity` — a plain-language record of what the butler did this turn.
    let mut activity: Vec<String> = Vec::new();
    // They asked, so nothing on file motivated it (ADR 0053).
    let actions = OutcomeSink::unmotivated(outcomes);

    // SINGLE TOOL-CALLING CONVERSATION (ADR 0053): the butler runs its tools through
    // policy and answers grounded in their real results — success or error — with no
    // deterministic narration. A failed tool comes back as a factual tool result the
    // model must relay honestly; if it misreports with the truth in front of it, that
    // is a model failure to surface, not a canned string to hardcode. The proactive
    // flows (check-in, brief, nightly loop) run on this same loop.
    let reply = {
        let mut relay = |chunk: &str| on_token(chunk);
        run_tool_turn(
            butler,
            capabilities,
            audit,
            &actions,
            ids,
            clock,
            &history,
            &prefs,
            context,
            CHAT_TOOL_ROUNDS,
            on_step,
            &mut relay,
            &mut activity,
            disclosures,
        )?
    };

    // The capability ladder (local-first, ADR 0055): if the local model came up empty,
    // climb to the deeper (bigger/cloud) model when the person configured one. Prose
    // only — never an action — so it stays a reasoning aid behind the policy boundary,
    // and the deep asker applies the egress guard since the question leaves the device.
    // Whether the answering round spoke for itself. When it did, that text has already
    // reached the person token by token; when it did not, whatever stands in for it is
    // new to them.
    let answered_in_its_own_words = !reply.text.trim().is_empty();
    let reply_text = if reply.text.trim().is_empty() {
        match deep.and_then(|d| d.ask(text)).map(|a| a.trim().to_owned()) {
            Some(answer) if !answer.is_empty() => {
                activity.push("Asked the deep model (the local model came up empty)".to_owned());
                answer
            }
            // Nothing from either model. If the turn ACTED, apologising is simply false —
            // it did something and knows what. ADR 0053 rejected deterministic narration
            // because code-written sentences got contradicted by the model; there is
            // nothing to contradict when the model produced no sentence at all, which is
            // what makes this safe rather than a relapse.
            _ => acted_note(disclosures).unwrap_or_else(|| {
                "I'm not sure how to help with that yet — can you say a bit more?".to_owned()
            }),
        }
    } else {
        reply.text.trim().to_owned()
    };
    // Say it plainly when a turn changed nothing (ADR 0053). Observed: asked to turn on
    // the kitchen table, the only action failed, and the reply announced that "the guest
    // bedroom left lamp is already on" — a device from earlier in the conversation that
    // this turn never touched. The activity trail showed the failure; the sentence the
    // person actually reads did not.
    //
    // Deterministic, and it does not rewrite what the model said: it appends what is
    // true. Whether the model narrates well is not something Endora can fix, but whether
    // the person is told nothing happened is.
    let mut appended = nothing_changed_note(disclosures);
    // On a turn that answered rather than acted, show the facts behind whatever it named
    // (ADR 0053). Scoped to answers because that is where a claim about state goes
    // unchecked, and because an acting turn already discloses its own before-and-after.
    if disclosures.is_empty() {
        appended.push_str(&facts_behind(&reply_text, capabilities.current_states()));
        appended.push_str(&account_behind(&reply_text, &context.did_lately));
    }
    // Where the words went. Disclosed on every escalated turn rather than left to a
    // setting the person configured once and will not remember (ADR 0055).
    if reply.escalated {
        appended.push_str(
            "\n\n[deep] My local model could not answer that, so I asked the deep model \
             instead — this conversation left the box.",
        );
    }
    let reply_text = format!("{reply_text}{appended}");
    // `take_turn` is non-streaming, so deliver the final answer at once.
    // Send only what the person has not already seen.
    //
    // Diffing against everything streamed does not work: the model often writes a line
    // before calling a tool ("let me check the kitchen"), which reaches the person and is
    // then NOT part of the final answer — so the accumulated stream is no longer a prefix
    // of the reply, and the whole answer would arrive a second time.
    //
    // The signal is simpler than a diff. If the answering round produced text, that text
    // is exactly what streamed, and only the notes appended after it are new. If it
    // produced nothing, whatever stands in for it was never streamed at all.
    if answered_in_its_own_words {
        if !appended.is_empty() {
            on_token(&appended);
        }
    } else {
        on_token(&reply_text);
    }
    let butler_msg = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        &reply_text,
        clock.now(),
    )?;
    chat.append(&butler_msg)?;

    // Understanding is KEPT (ADR 0052) — but only on a CONVERSATION turn. On an action
    // turn (a tool ran, failed, or was blocked) we skip the extra model round: it's the
    // biggest latency saving on a slow model, and belief-forming matters far less when
    // the person was giving a command than when they were talking. On a pure chat turn,
    // a tools-OFF envelope pass forms beliefs (an empty `tools` context keeps the JSON
    // envelope, so understanding comes back without pulling the model into an action).
    let acted = activity.iter().any(|a| {
        a.starts_with("Used the ") || a.starts_with("Tried the ") || a.starts_with("Couldn't use ")
    });
    if !acted {
        let mut belief_ctx = context.clone();
        belief_ctx.tools = Vec::new();
        let formed = butler
            .respond(&history, &prefs, &belief_ctx)
            .map(|r| r.beliefs)
            .unwrap_or_default();
        record_formed_beliefs(beliefs, formed, ids, clock, &mut activity)?;
    }

    Ok((butler_msg, activity))
}

/// Persists the beliefs the butler formed — Endora's own understanding (not
/// actions), kept directly then reviewable/correctable (ADR 0052). If the butler
/// restated something Endora already believes, the existing belief is **affirmed**
/// (raising confidence) rather than storing a near-duplicate. Each stored/affirmed
/// belief is appended to `activity` in plain language. Shared by the chat turn and
/// the nightly self-improvement loop (ADR 0051) — the "reflect" step.
///
/// # Errors
/// [`AppError::Domain`] if a statement is invalid, or [`AppError::Repository`] on
/// a backend failure.
fn record_formed_beliefs(
    beliefs: &impl BeliefRepository,
    formed: Vec<FormedBelief>,
    ids: &impl IdSource,
    clock: &impl Clock,
    activity: &mut Vec<String>,
) -> Result<(), AppError> {
    // Grows as the batch stores. Read once and left alone, a turn that forms two
    // paraphrases of one thought stores both — which is exactly how three cards saying
    // "Fahrenheit" ended up on one screen, each asking to be confirmed on its own.
    let mut existing = beliefs.list()?;
    for belief in formed {
        // A command the person gave is not a fact about them. Dropping these keeps
        // the model of the person from filling up with spent instructions.
        if reads_as_an_instruction(&belief.statement) {
            continue;
        }
        // Nor is Endora's own conduct evidence about the person (ADR 0052).
        if is_about_endora_itself(&belief.evidence) {
            continue;
        }
        if let Some(mut prior) = existing
            .iter()
            .find(|b| similar(b.statement(), &belief.statement))
            .cloned()
        {
            activity.push(format!("Grew more sure that {}", prior.statement()));
            prior.affirm(clock.now());
            beliefs.save(&prior)?;
            if let Some(slot) = existing.iter_mut().find(|b| b.id() == prior.id()) {
                *slot = prior;
            }
            continue;
        }
        // Surface a disagreement rather than resolving it. Endora holding two
        // contradictory beliefs means it is wrong about something, which is the most
        // useful thing understanding can tell the person — and which of them is true
        // is exactly the judgement that belongs to them, not to the butler
        // (constitution §4: distinguish evidence from assumption, never present a
        // guess as a fact). Both are kept; the person resolves it by correcting one.
        for conflicting in existing
            .iter()
            .filter(|b| b.status() == crate::BeliefStatus::Active)
            .filter(|b| statements_disagree(b.statement(), &belief.statement))
        {
            activity.push(format!(
                "Noticed this sits oddly with something I already thought: \"{}\" vs \"{}\"",
                conflicting.statement(),
                belief.statement.trim()
            ));
        }
        activity.push(format!("Learned that {}", belief.statement.trim()));
        let stored = Belief::new(
            BeliefId::new(ids.new_id()),
            &belief.statement,
            belief.kind,
            belief.confidence,
            &belief.evidence,
            clock.now(),
        )?;
        beliefs.save(&stored)?;
        existing.push(stored);
    }
    Ok(())
}

/// Re-reads what is already stored against the rules as they stand today (ADR 0052).
///
/// Every guard on understanding ran **at formation only**, which quietly means the model
/// of the person is frozen at the quality of the rules on the day each belief was formed.
/// Observed: a card reading *you want me to turn off the kitchen light — because turn off
/// the kitchen light entity*, sitting there long after the rule that rejects exactly that
/// shape shipped. It was never going to leave, because nothing ever looked at it again.
///
/// Two rules, both already written and both applied here to the past rather than the
/// future:
///
/// - a statement that reads as an **instruction** is a spent command, not a fact about a
///   person, and is expired;
/// - a statement that says what an **older** one already says is expired, and the older one
///   is affirmed — the same thought arriving twice is genuinely evidence for it.
///
/// [`BeliefStatus::Expired`] rather than deletion or `Corrected`: the person did not say it
/// was wrong, so claiming they did would put words in their mouth, and a belief Endora
/// dropped on its own is worth being able to see.
///
/// Returns how many it retired, for the activity trail.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
pub fn tidy_understanding(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<usize, AppError> {
    let mut held: Vec<Belief> = beliefs
        .list()?
        .into_iter()
        .filter(|b| b.status() == crate::BeliefStatus::Active)
        .collect();
    // Oldest first, so the belief that survives a merge is the one that has been held
    // longest — it carries the earlier evidence and the longer history of affirmation.
    held.sort_by_key(|b| b.created_at().unix_millis());
    let mut kept: Vec<Belief> = Vec::new();
    let mut retired = 0;
    for mut belief in held {
        if reads_as_an_instruction(belief.statement()) || is_about_endora_itself(belief.evidence())
        {
            belief.expire();
            beliefs.save(&belief)?;
            retired += 1;
            continue;
        }
        if let Some(older) = kept
            .iter_mut()
            .find(|k| similar(k.statement(), belief.statement()))
        {
            older.affirm(clock.now());
            beliefs.save(older)?;
            belief.expire();
            beliefs.save(&belief)?;
            retired += 1;
            continue;
        }
        kept.push(belief);
    }
    Ok(retired)
}

/// Verbs that name a change to the world rather than something about the person.
const ACTION_VERBS: &[&str] = &[
    "turn", "switch", "set", "play", "pause", "resume", "dim", "brighten", "lock", "unlock",
    "open", "close", "start", "stop", "send", "add", "remove", "delete", "book", "order", "buy",
    "schedule", "cancel", "call", "text", "email", "post", "put", "move",
];

/// Whether a "belief" is really a **one-off instruction** the person gave, rather
/// than something true about them.
///
/// Observed live: the butler filed "you want me to turn off the kitchen light" as a
/// durable preference, and later "you want me to turn on the kitchen lights" beside
/// it. Neither is understanding — they are two commands from two moments, recorded
/// as if they described a person. Left alone they accumulate, contradict each other,
/// and pollute the context every later turn reasons from.
///
/// The discriminator is **what follows the request**. "You want me to *be more
/// direct*" is a genuine standing preference about how Endora should behave and must
/// be kept; "you want me to *turn off* the light" names an action on the world and is
/// spent the moment it is carried out.
pub fn reads_as_an_instruction(statement: &str) -> bool {
    let text = normalized(statement);
    let addressed_to_endora = [
        "want me to",
        "asked me to",
        "told me to",
        "need me to",
        "would like me to",
        "d like me to",
        "wanted me to",
    ]
    .iter()
    .any(|phrase| text.contains(phrase));
    if !addressed_to_endora {
        return false;
    }
    polarity_tokens(&text)
        .iter()
        .any(|w| ACTION_VERBS.contains(&w.as_str()))
}

/// Whether the evidence for a belief is Endora describing **itself**.
///
/// Live, top of the understanding screen:
///
/// ```text
/// you find it more convenient for the assistant to wait for instructions
///   because I didn't do anything proactive since I rely on your instructions.
/// ```
///
/// Endora had been passive — because its direct reach into the house was broken — and
/// concluded from its own conduct that the person prefers passivity. It then carried that
/// into every later turn as something it knew about them.
///
/// This is the sharpest possible violation of the rule that understanding admits **only
/// facts about the person** (ADR 0052): a butler that mistakes its own behaviour for
/// evidence will reinforce whatever it happens to be doing, including its own faults.
///
/// The discriminator is narrow — evidence whose **first word is "I"** is Endora as the
/// subject. *"You asked 'where did you find this?' after I listed some events"* keeps its
/// belief, because the person is the subject and Endora only appears in passing.
fn is_about_endora_itself(evidence: &str) -> bool {
    let mut words = evidence.split_whitespace();
    let first = words
        .next()
        .unwrap_or_default()
        .trim_matches(|c: char| !c.is_alphanumeric());
    first.eq_ignore_ascii_case("i")
}

/// Normalizes a belief statement for duplicate detection: lowercase, collapse
/// whitespace, drop trailing punctuation.
fn normalized(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .to_owned()
}

/// Reduces a word to a crude stem so inflections match: `retirement`/`retire`,
/// `travelling`/`travel`, `flights`/`flight`. Deliberately simple — this only has
/// to make duplicate detection robust, not to be a linguistically correct stemmer,
/// and an over-eager one would merge genuinely different beliefs.
fn stem(word: &str) -> String {
    for suffix in [
        "ements", "ement", "ations", "ation", "ings", "ing", "ies", "ed", "es", "s",
    ] {
        if let Some(root) = word.strip_suffix(suffix) {
            // Keep a recognisable root; "ties" must not collapse to "t".
            if root.len() >= 3 {
                let root = if suffix == "ies" {
                    format!("{root}y")
                } else {
                    root.to_owned()
                };
                return trim_stem(&root);
            }
        }
    }
    trim_stem(word)
}

/// Drops a trailing `e` and un-doubles a final consonant, so British and American
/// inflections land on one stem (`travelling` → `travell` → `travel`).
fn trim_stem(root: &str) -> String {
    let root = root.trim_end_matches('e');
    let mut chars: Vec<char> = root.chars().collect();
    if chars.len() > 3 {
        let last = chars[chars.len() - 1];
        if last == chars[chars.len() - 2] && !"aeiou".contains(last) {
            chars.pop();
        }
    }
    chars.into_iter().collect()
}

/// Content words of a statement, stemmed — drops filler so two phrasings of the
/// same belief share keywords. The stop list covers the scaffolding a butler
/// naturally varies between turns ("you want X" / "you'd like to have X").
fn keywords(s: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "you", "your", "are", "the", "and", "for", "that", "this", "with", "have", "was", "its",
        "but", "not", "can", "want", "like", "get", "getting", "able", "being", "been", "once",
        "when", "while", "into", "about", "some", "more", "much", "very", "really", "just",
        "still", "also", "would", "could", "should", "them", "they", "their", "she", "him", "her",
        "his", "who", "why", "how", "what", "where", "than", "then", "there", "here",
    ];
    normalized(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !STOP.contains(w))
        .map(stem)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Every word of a statement, lowercased with apostrophes closed up so `don't`
/// becomes `dont`. Unlike [`keywords`] this drops **nothing** — the words that flip
/// a statement's meaning (`on`, `no`, `not`) are exactly the short, common ones a
/// keyword filter throws away.
fn polarity_tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace(['\'', '\u{2019}'], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Words that reverse a statement's sense. Asymmetry in these means two statements
/// disagree however much vocabulary they share.
const NEGATIONS: &[&str] = &[
    "not", "never", "no", "dont", "doesnt", "didnt", "wont", "isnt", "arent", "cant", "cannot",
    "without", "instead", "rather", "dislike", "dislikes", "hate", "hates", "avoid", "avoids",
];

/// Pairs where holding one side contradicts the other.
const ANTONYMS: &[(&str, &str)] = &[
    ("on", "off"),
    ("more", "less"),
    ("always", "never"),
    ("like", "dislike"),
    ("prefer", "avoid"),
    ("before", "after"),
    ("morning", "evening"),
    ("morning", "night"),
    ("early", "late"),
    ("hot", "cold"),
    ("warm", "cool"),
    ("start", "stop"),
    ("open", "close"),
    ("lock", "unlock"),
    ("increase", "decrease"),
    ("up", "down"),
    ("fahrenheit", "celsius"),
    ("metric", "imperial"),
];

/// Whether two statements **disagree** — one negates where the other does not, or
/// they sit on opposite sides of an antonym pair.
///
/// This is checked on the raw words rather than the stemmed keywords, because the
/// decisive tokens are the ones keyword filtering removes: `on` is two characters,
/// `like` is a stopword, and `don't` splits into fragments. Without this,
/// "turn **off** the kitchen light" and "turn **on** the kitchen lights" reduce to
/// the same keyword set and merge into one belief — silently discarding the
/// disagreement instead of surfacing it.
#[must_use]
pub fn statements_disagree(a: &str, b: &str) -> bool {
    // Two statements can only contradict each other if they are **about** the same thing.
    // Without this, negation asymmetry alone counts: "you find it more convenient to
    // measure temperature in Celsius rather than Fahrenheit" was read as disagreeing with
    // every other belief Endora held, including "you want to know where I found the event
    // information". Harmless while the answer only fed duplicate detection, which also
    // required overlap; put on screen, it accused every card of contradicting one belief
    // about thermometers.
    if !about_the_same_thing(a, b) {
        return false;
    }
    let (ta, tb) = (polarity_tokens(a), polarity_tokens(b));
    let negated = |t: &[String]| t.iter().any(|w| NEGATIONS.contains(&w.as_str()));
    if negated(&ta) != negated(&tb) {
        return true;
    }
    let has = |t: &[String], w: &str| t.iter().any(|x| x == w);
    ANTONYMS
        .iter()
        .any(|(x, y)| (has(&ta, x) && has(&tb, y)) || (has(&ta, y) && has(&tb, x)))
}

/// Whether two belief statements are effectively the same (a paraphrase), by
/// stemmed keyword overlap — public so the live smoke check can assert the same rule the
/// tidy pass applies, rather than a second implementation of it that can drift — so "you want more energy so you can travel when you
/// retire" and "you're motivated by being able to travel in retirement" are one
/// belief, not two.
///
/// Uses **containment** rather than symmetric Jaccard: if one statement's keywords
/// are largely a subset of the other's, it says nothing new. A plain Jaccard
/// penalises the longer, more specific phrasing and lets near-duplicates through —
/// which is what let them accumulate before (the `no-duplicate` eval case, ADR 0055).
/// The threshold stays deliberately high: **wrongly merging two distinct beliefs
/// silently loses understanding, which is worse than storing one duplicate the
/// person can correct.**
///
/// Statements that [disagree](statements_disagree) are **never** similar, however
/// much wording they share. Two contradictory beliefs are the most informative thing
/// understanding can contain — they mean Endora is wrong about something — and
/// collapsing them into one keeps whichever arrived first and destroys the signal.
#[must_use]
pub fn says_the_same_thing(a: &str, b: &str) -> bool {
    similar(a, b)
}

/// See [`says_the_same_thing`] for the public form.
fn similar(a: &str, b: &str) -> bool {
    if statements_disagree(a, b) {
        return false;
    }
    let (ka, kb) = (subject_words(a), subject_words(b));
    if ka.is_empty() || kb.is_empty() {
        return normalized(a) == normalized(b);
    }
    let shared = ka.iter().filter(|w| kb.contains(w)).count() as f64;
    let smaller = ka.len().min(kb.len()) as f64;
    shared / smaller >= 0.75
}

/// Whether two statements concern the same subject at all — the precondition for them
/// being able to agree or disagree.
///
/// Half of the shorter statement's subject words, which is a much weaker bar than
/// [`similar`]'s: *you like running* and *you hate running* are about the same thing
/// without being the same belief, and that gap is precisely where contradiction lives.
fn about_the_same_thing(a: &str, b: &str) -> bool {
    let (sa, sb) = (subject_words(a), subject_words(b));
    if sa.is_empty() || sb.is_empty() {
        return false;
    }
    let shared = sa.iter().filter(|w| sb.contains(w)).count() as f64;
    shared / sa.len().min(sb.len()) as f64 >= 0.5
}

/// Words that describe **having a stance** rather than what the stance is about.
///
/// Three cards said the same thing on one screen — *prefer temperatures in Fahrenheit*,
/// *prefer temperature measurements in Fahrenheit*, *find it more convenient and accurate
/// to measure temperature in Fahrenheit*. Only the first two matched, because the third
/// spends most of its words on how strongly the person feels and only two on the subject.
/// Comparing everything means a wordier paraphrase looks like a different belief.
///
/// Polarity words are stripped **too** — `like` alongside `hate` — which is deliberate and
/// only safe because of the division of labour above: [`subject_words`] decides whether two
/// statements are about the same thing, and [`statements_disagree`] (checked first) decides
/// whether they say opposite things about it. Keeping only the positive ones here would
/// merge *you like running* with *you hate running* whenever disagreement missed it.
const STANCE_WORDS: &[&str] = &[
    "prefer",
    "want",
    "like",
    "love",
    "enjoy",
    "hate",
    "dislik",
    "avoid",
    "find",
    "feel",
    "think",
    "believ",
    "convenient",
    "accurat",
    "comfort",
    "usual",
    "typical",
    "general",
    "realli",
    "more",
    "most",
    "veri",
    "would",
    "rather",
];

/// What a statement is **about**, with the stance words removed.
///
/// Falls back to the full keywords when nothing survives: *you'd really rather not* is all
/// stance and no subject, and comparing two empty sets would merge every such statement
/// with every other.
fn subject_words(s: &str) -> Vec<String> {
    let all = keywords(s);
    let subject: Vec<String> = all
        .iter()
        .filter(|w| !STANCE_WORDS.contains(&w.as_str()))
        .cloned()
        .collect();
    if subject.is_empty() { all } else { subject }
}

/// A short, human present-tense label for a skill in progress — what the butler
/// shows while it works a step ("· Checking the weather…"), so the person can see
/// it moving toward the goal rather than sitting on a silent "one moment".
fn progress_label(id: &str) -> String {
    match id {
        "weather" => "Checking the weather",
        "news" | "local_news" => "Checking the news",
        "safety_alerts" => "Checking safety alerts",
        "web_search" | "web_answers" => "Searching the web",
        "web_fetch" => "Reading the page",
        "knowledge" => "Looking that up",
        "home_assistant" => "Checking your home",
        "local_events" => "Checking local events",
        "image_review" => "Looking at the image",
        other => return format!("Using the {other} skill"),
    }
    .to_owned()
}

/// Formats a Unix-millisecond timestamp as `"Weekday, YYYY-MM-DD HH:MM UTC"` — no
/// date dependency, using the standard civil-from-days algorithm. UTC for now; a
/// later refinement can localise from the person's known location.
fn format_datetime_utc(ms: i64) -> String {
    let day = ms.div_euclid(86_400_000);
    let secs = ms.rem_euclid(86_400_000) / 1000;
    let (hh, mm) = (secs / 3600, (secs % 3600) / 60);
    // Weekday: Unix day 0 (1970-01-01) was a Thursday.
    const DOW: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let dow = DOW[(day.rem_euclid(7) + 4).rem_euclid(7) as usize];
    // Civil date from days since epoch (Howard Hinnant's algorithm).
    let z = day + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    format!("{dow}, {year:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
}

/// Endora's living understanding of the person — its active beliefs, most
/// recently affirmed first. Corrected beliefs are omitted.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn understanding(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Vec<Belief>, AppError> {
    let now = clock.now();
    Ok(beliefs
        .list()?
        .into_iter()
        .filter(|b| b.status() == crate::BeliefStatus::Active)
        // Beliefs weaken without reinforcement and eventually fade out entirely
        // (ADR 0052). Filtering on read means understanding is honest the moment it
        // is asked for, whether or not the nightly loop has run.
        .filter(|b| !b.has_faded(now))
        .collect())
}

/// Ages understanding: marks every faded belief as expired, so Endora stops acting
/// on things nothing has supported in a long time and the person can see that it
/// let them go. Run by the nightly loop.
///
/// Read-side filtering already hides faded beliefs, so this changes no behaviour —
/// it makes the forgetting **durable and visible** rather than implicit, which is
/// what the memory rights require of anything Endora holds (constitution §6).
/// Returns the statements it expired, for the activity trail.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn expire_faded_beliefs(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Vec<String>, AppError> {
    let now = clock.now();
    let mut expired = Vec::new();
    for mut belief in beliefs.list()? {
        if !belief.has_faded(now) {
            continue;
        }
        expired.push(belief.statement().to_owned());
        belief.expire();
        beliefs.save(&belief)?;
    }
    Ok(expired)
}

/// The person confirms a belief is right: raise its confidence.
///
/// # Errors
/// [`AppError::NotFound`] if it does not exist, or [`AppError::Repository`].
pub fn affirm_belief(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
    id: BeliefId,
) -> Result<Belief, AppError> {
    let mut belief = beliefs
        .get(id)?
        .ok_or(AppError::NotFound { entity: "belief" })?;
    belief.affirm(clock.now());
    beliefs.save(&belief)?;
    Ok(belief)
}

/// How many recent outcomes the track record is built from. Bounded because the
/// prompt has to stay small on a slow local model, and because how an action landed
/// last month says little about now.
const TRACK_RECORD_WINDOW: usize = 50;

/// What Endora is pursuing and has pursued, most recently moved first (ADR 0052).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn intentions(intentions: &impl IntentionRepository) -> Result<Vec<Intention>, AppError> {
    Ok(intentions.list()?)
}

/// What Endora has noticed is wrong with its own tooling (ADR 0054).
///
/// Derived from outcome history on every read — there is no store of proposals, nothing
/// to dismiss and nothing to groom, which is how ADR 0052's approval queue is made
/// impossible rather than merely discouraged. A proposal disappears on its own when the
/// outcomes age out or an action finally changes something.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn repairs(outcomes: &impl OutcomeRepository) -> Result<Vec<RepairProposal>, AppError> {
    Ok(endora_understanding::repair_proposals(&outcomes.list()?))
}

/// The person tells Endora to stop working on something (ADR 0052).
///
/// Their whole authority over an intention, and deliberately the only verb they have:
/// Endora forms its own, from what it understands, and there is no path by which the
/// person can create or edit one.
///
/// # Errors
/// [`AppError::NotFound`] if there is no such intention, or [`AppError::Repository`].
pub fn drop_intention(
    intentions: &impl IntentionRepository,
    id: IntentionId,
) -> Result<Intention, AppError> {
    let mut intention = intentions.get(id)?.ok_or(AppError::NotFound {
        entity: "intention",
    })?;
    intention.abandon();
    intentions.save(&intention)?;
    Ok(intention)
}

/// The outcomes of what Endora has done, most recent first (ADR 0053) — the memory
/// right to *see* what it did, alongside the beliefs it holds.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn recent_outcomes(
    outcomes: &impl OutcomeRepository,
    limit: usize,
) -> Result<Vec<Outcome>, AppError> {
    Ok(outcomes.list()?.into_iter().take(limit).collect())
}

/// The person says how an action landed (ADR 0053). They are never asked for this;
/// it is offered where the action already appears, and the latest word wins.
///
/// # Errors
/// [`AppError::NotFound`] if there is no such outcome, or [`AppError::Repository`].
pub fn react_to_outcome(
    outcomes: &impl OutcomeRepository,
    id: OutcomeId,
    reaction: Reaction,
) -> Result<Outcome, AppError> {
    let mut outcome = outcomes
        .get(id)?
        .ok_or(AppError::NotFound { entity: "outcome" })?;
    outcome.react(reaction);
    outcomes.save(&outcome)?;
    Ok(outcome)
}

/// How the butler's past actions have landed, per skill — one line each (ADR 0053).
///
/// **Only skills the person has actually reacted to appear.** An action nobody
/// commented on says nothing about whether it helped, and padding the prompt with
/// "5 uses, no feedback" would spend a slow local model's context on noise. Empty
/// until they have said something, which is the normal early state.
///
/// Ordered by how much was said about a skill, so the most-judged comes first, and
/// capped — this is a nudge, not a report.
fn track_record(outcomes: &[Outcome]) -> Vec<String> {
    /// Skills listed in the prompt at most.
    const MAX_SKILLS: usize = 5;
    let mut by_skill: std::collections::BTreeMap<&str, (usize, usize)> =
        std::collections::BTreeMap::new();
    for outcome in outcomes {
        let entry = match outcome.reaction() {
            Some(Reaction::Helped) => (1, 0),
            Some(Reaction::DidNotHelp) => (0, 1),
            // No reaction, or "made no difference": nothing to learn from.
            _ => continue,
        };
        let counts = by_skill.entry(outcome.capability()).or_insert((0, 0));
        counts.0 += entry.0;
        counts.1 += entry.1;
    }
    let mut ranked: Vec<_> = by_skill.into_iter().collect();
    ranked.sort_by_key(|(skill, (helped, missed))| {
        // Most-judged first; the skill name breaks ties so the prompt is stable.
        (std::cmp::Reverse(helped + missed), *skill)
    });
    ranked
        .into_iter()
        .take(MAX_SKILLS)
        .map(|(skill, (helped, missed))| match (helped, missed) {
            (h, 0) => format!("{skill} — helped {h} time(s)"),
            (0, m) => format!("{skill} — didn't help {m} time(s)"),
            (h, m) => format!("{skill} — helped {h} time(s), didn't help {m}"),
        })
        .collect()
}

/// The person says a belief is wrong: mark it corrected (drops out of understanding).
///
/// # Errors
/// [`AppError::NotFound`] if it does not exist, or [`AppError::Repository`].
pub fn correct_belief(beliefs: &impl BeliefRepository, id: BeliefId) -> Result<(), AppError> {
    let mut belief = beliefs
        .get(id)?
        .ok_or(AppError::NotFound { entity: "belief" })?;
    belief.correct();
    beliefs.save(&belief)?;
    Ok(())
}

/// Returns the whole conversation with the butler, oldest first.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn chat_history(chat: &impl ChatRepository) -> Result<Vec<ChatMessage>, AppError> {
    Ok(chat.list()?)
}

/// Returns the person's proactive check-in schedule, defaulting to **off**.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn checkin_schedule(
    checkins: &impl CheckinRepository,
    clock: &impl Clock,
) -> Result<CheckinSchedule, AppError> {
    Ok(checkins
        .get()?
        .unwrap_or_else(|| CheckinSchedule::disabled_default(clock.now())))
}

/// Sets the check-in cadence. Enabling (or changing the interval) schedules the
/// next check-in one interval from now, so turning it on is not an instant ping.
///
/// # Errors
/// [`AppError::BadRequest`] if the interval is not positive, or [`AppError::Repository`].
pub fn set_checkin_schedule(
    checkins: &impl CheckinRepository,
    clock: &impl Clock,
    enabled: bool,
    interval_ms: i64,
) -> Result<CheckinSchedule, AppError> {
    if interval_ms <= 0 {
        return Err(AppError::BadRequest {
            message: "check-in interval must be positive".to_owned(),
        });
    }
    let now = clock.now();
    let schedule = CheckinSchedule {
        enabled,
        interval_ms,
        next_at: Timestamp::from_unix_millis(now.unix_millis() + interval_ms),
    };
    checkins.set(&schedule)?;
    Ok(schedule)
}

/// Considers reaching out, and does so **only if the butler has a reason**.
///
/// The clock no longer decides. [`CheckinSchedule`] is a *budget* — it bounds how
/// often the butler may speak uninvited and keeps it from talking over someone who
/// just spoke — and within that budget the butler judges whether anything is worth
/// raising, given what it understands about the person (ADR 0056).
///
/// Two deterministic properties make this safe to leave running:
///
/// 1. **The budget is spent whether or not it speaks.** Deciding "nothing to say"
///    costs the same as speaking, so the butler cannot re-ask every thirty seconds
///    until it talks itself into a reason.
/// 2. **Silence is the default.** An empty reply, an unavailable model, or a failed
///    turn all mean no message. Nothing is ever posted to fill the slot.
///
/// The reason it gives is recorded to the activity trail, so "why did it message
/// me?" always has an answer.
///
/// This is an `act` on the low-stakes end of the autonomy model (ADR 0051): a
/// message, never a consequential action.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails, or [`AppError::Domain`] if the
/// generated message is somehow invalid.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn consider_reaching_out(
    chat: &impl ChatRepository,
    checkins: &impl CheckinRepository,
    preferences: &impl PreferenceRepository,
    outcomes: &impl OutcomeRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let now = clock.now();
    let Some(mut schedule) = checkins.get()? else {
        return Ok(None);
    };
    // When the person themselves last spoke — if they are around, they can just ask.
    let last_person_activity = chat
        .list()?
        .iter()
        .rev()
        .find(|m| m.role() == MessageRole::User)
        .map(ChatMessage::at);
    if !schedule.may_reach_out(now, last_person_activity) {
        return Ok(None);
    }
    // Spend the budget up front, whether or not it finds something to say. This is
    // what stops a "no" from becoming a retry loop, and it also means a slow write
    // can't double-post on the next tick.
    schedule.next_at = Timestamp::from_unix_millis(now.unix_millis() + schedule.interval_ms);
    checkins.set(&schedule)?;

    let prefs = preferences.list_all().unwrap_or_default();
    let mut activity: Vec<String> = Vec::new();
    let actions = OutcomeSink::unmotivated(outcomes);
    let ask_ctx = ButlerContext {
        now: format_datetime_utc(now.unix_millis()),
        ..context.clone()
    };
    let ask = [ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        "You have a moment to reach out to them, unprompted. Consider what you \
         understand about them and what you could look up — is there anything \
         genuinely worth saying right now? \
         Say NOTHING AT ALL unless there is. An empty reply is the right answer most \
         of the time, and is far better service than manufacturing something to fill \
         the silence. There is no obligation to speak. \
         If there IS something, open with it plainly — the specific thing you noticed \
         or found — not a greeting and not a status report. Add nothing you don't \
         actually know.",
        now,
    )?];
    let reply = run_tool_turn(
        butler,
        capabilities,
        audit,
        &actions,
        ids,
        clock,
        &ask,
        &prefs,
        &ask_ctx,
        CHECKIN_TOOL_ROUNDS,
        &mut |_step| {},
        &mut |_token: &str| {},
        &mut activity,
        &mut Vec::new(),
    )
    .ok();
    // Nothing worth saying — the common and correct case — and a reply that is really
    // about the plumbing counts as nothing (ADR 0056: silence is the default here, so the
    // cost of being strict is one skipped window).
    let text = reply
        .as_ref()
        .filter(|r| !not_an_answer(r, &ask_ctx))
        .map(|r| r.text.trim().to_owned())
        .filter(|t| !t.is_empty());
    let Some(text) = text else {
        activity.push("Considered reaching out, and had nothing worth saying".to_owned());
        return Ok(None);
    };

    let message = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        &text,
        now,
    )?;
    chat.append(&message)?;
    activity.push("Reached out because there was something worth raising".to_owned());
    Ok(Some((message, activity)))
}

/// Composes a **daily briefing** — an act of service (ADR 0056). The butler is handed
/// its skill catalogue and decides for itself what a brief needs today, gathering
/// across tool rounds and then writing it. Policy gates every call to configured +
/// reversible + autonomous, so a briefing never does anything consequential
/// (ADR 0051), and each result — success or failure — comes back as a tool message the
/// butler answers from, so the prose is grounded in what actually happened (ADR 0053).
///
/// There is **no scripted fallback**: if the butler gathered nothing worth saying, or
/// the model is unavailable, this returns `None` and no brief is posted. A brief
/// assembled by fixed code from a hardcoded skill list would be Endora claiming to
/// have thought about the person's day when it had not.
///
/// Returns the posted message and a plain-language activity trail, or `None`.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn daily_brief(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    outcomes: &impl OutcomeRepository,
    troubles: &impl endora_capabilities::StandingTroubleRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let prefs = preferences.list_all()?;
    let mut activity: Vec<String> = Vec::new();
    let actions = OutcomeSink::unmotivated(outcomes);

    // ONE agentic pass (ADR 0056/0028): the butler reaches for whatever it decides a
    // brief needs, each result comes back as a tool message, and it writes the brief
    // from those results in the same conversation. No gather/synthesize split, and no
    // scripted weather→safety→news sweep underneath it.
    let brief_ctx = ButlerContext {
        now: format_datetime_utc(clock.now().unix_millis()),
        ..context.clone()
    };
    // What Endora already knows, assembled here rather than asked for. A brief that
    // depends on the model choosing the right tools is a brief that arrives as "the
    // kitchen and garage lights are on" — which is what four days of them looked like.
    // Only what has been wrong long enough to be worth raising, so a brief cannot become
    // the pile of chores ADR 0056 forbids.
    let open = troubles.troubles().unwrap_or_default();
    let raisable = endora_capabilities::worth_raising(&open, clock.now().unix_millis());
    let already_known = whats_worth_saying_this_morning(&brief_ctx, &raisable);
    let ask = [ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        &format!(
            "Put together my brief. Here is what you already know, which you must include \
             all of:\n{already_known}\n\nThen reach for anything else relevant — news I'd \
             care about, safety alerts where I'm based — and give me a short, warm rundown. \
             If a skill failed or you couldn't reach something, say so plainly rather than \
             filling the gap."
        ),
        clock.now(),
    )?];
    let text = run_tool_turn(
        butler,
        capabilities,
        audit,
        &actions,
        ids,
        clock,
        &ask,
        &prefs,
        &brief_ctx,
        BRIEF_TOOL_ROUNDS,
        &mut |_step| {},
        &mut |_token: &str| {},
        &mut activity,
        &mut Vec::new(),
    )
    .ok()
    .map(|reply| reply.text.trim().to_owned())
    .filter(|t| !t.is_empty());
    // Nothing worth saying (or no model) — stay quiet rather than post a hollow brief.
    let Some(text) = text else {
        return Ok(None);
    };
    let message = post_butler_message(chat, ids, clock, &text)?;
    Ok(Some((message, activity)))
}

/// Posts a butler message to the chat (used by out-of-band paths like the deep-model
/// answer). Returns the persisted message.
///
/// # Errors
/// [`AppError::Domain`] if the text is blank, or [`AppError::Repository`] on failure.
pub fn post_butler_message(
    chat: &impl ChatRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    text: &str,
) -> Result<ChatMessage, AppError> {
    let message = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        text,
        clock.now(),
    )?;
    chat.append(&message)?;
    Ok(message)
}

/// Posts a person's message to the chat (used by out-of-band paths like a manual
/// "Ask deep", so the exchange shows both sides). Returns the persisted message.
///
/// # Errors
/// [`AppError::Domain`] if the text is blank, or [`AppError::Repository`] on failure.
pub fn post_user_message(
    chat: &impl ChatRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    text: &str,
) -> Result<ChatMessage, AppError> {
    let message = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        text,
        clock.now(),
    )?;
    chat.append(&message)?;
    Ok(message)
}

/// Returns the daily-brief schedule, defaulting to **off**.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn brief_schedule(briefs: &impl BriefScheduleRepository) -> Result<BriefSchedule, AppError> {
    Ok(briefs
        .get()?
        .unwrap_or_else(BriefSchedule::disabled_default))
}

/// Turns the daily brief on/off and sets the UTC hour it prepares at.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn set_brief_schedule(
    briefs: &impl BriefScheduleRepository,
    enabled: bool,
    hour_utc: u8,
) -> Result<BriefSchedule, AppError> {
    let current = briefs
        .get()?
        .unwrap_or_else(BriefSchedule::disabled_default);
    let schedule = BriefSchedule {
        enabled,
        hour_utc: hour_utc.min(23),
        // Keep last_at so toggling doesn't re-fire the same day.
        last_at: current.last_at,
    };
    briefs.set(&schedule)?;
    Ok(schedule)
}

/// If a daily brief is **due** (enabled, the hour matches, none prepared today),
/// prepares one via [`daily_brief`] and records that it fired. Called by the
/// heartbeat. Returns the posted brief + activity, or `None`.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn run_due_brief(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    outcomes: &impl OutcomeRepository,
    troubles: &impl endora_capabilities::StandingTroubleRepository,
    briefs: &impl BriefScheduleRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let now = clock.now();
    let Some(mut schedule) = briefs.get()? else {
        return Ok(None);
    };
    if !schedule.is_due(now) {
        return Ok(None);
    }
    // Mark fired first, so a slow compose can't double-post on the next tick.
    schedule.last_at = now;
    briefs.set(&schedule)?;
    daily_brief(
        chat,
        preferences,
        outcomes,
        troubles,
        capabilities,
        butler,
        audit,
        ids,
        clock,
        context,
    )
}

/// The stored nightly-loop schedule, defaulting to **off** (ADR 0051).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn nightly_loop_schedule(
    schedules: &impl NightlyLoopScheduleRepository,
) -> Result<NightlyLoopSchedule, AppError> {
    Ok(schedules
        .get()?
        .unwrap_or_else(NightlyLoopSchedule::disabled_default))
}

/// Sets the nightly-loop cadence (on/off + UTC hour), preserving `last_at` so
/// toggling it doesn't re-fire the same night (ADR 0051).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn set_nightly_loop_schedule(
    schedules: &impl NightlyLoopScheduleRepository,
    enabled: bool,
    hour_utc: u8,
) -> Result<NightlyLoopSchedule, AppError> {
    let current = schedules
        .get()?
        .unwrap_or_else(NightlyLoopSchedule::disabled_default);
    let schedule = NightlyLoopSchedule {
        enabled,
        hour_utc: hour_utc.min(23),
        last_at: current.last_at,
    };
    schedules.set(&schedule)?;
    Ok(schedule)
}

/// If the **nightly self-improvement loop** is due (enabled, the hour matches, it
/// hasn't run tonight), runs it and records that it fired (ADR 0051). Called by the
/// heartbeat at a quiet off-hour.
///
/// The loop stays entirely within the **reversible band**: it may run reversible
/// *information* skills to research a topic the person cares about (the *experiment*
/// step), reviews the recent conversation and Endora's current understanding, has
/// the butler **reflect** — forming and refining beliefs (saved like a normal
/// turn) — and leaves a short **overnight note**. It never runs a consequential or
/// irreversible skill — policy clears only reversible, autonomous ones — so there is
/// nothing here it could do that it couldn't undo. Returns the posted note (if any)
/// + activity, or `None` when not due / nothing to say.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure, or [`AppError::Model`] if the
/// butler errors.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn run_due_nightly_loop(
    chat: &impl ChatRepository,
    beliefs: &impl BeliefRepository,
    preferences: &impl PreferenceRepository,
    outcomes: &impl OutcomeRepository,
    intentions: &impl IntentionRepository,
    schedules: &impl NightlyLoopScheduleRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    audit: &dyn AuditLog,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let now = clock.now();
    let Some(mut schedule) = schedules.get()? else {
        return Ok(None);
    };
    if !schedule.is_due(now) {
        return Ok(None);
    }
    // Mark fired first, so a slow reflection can't double-run on the next tick.
    schedule.last_at = now;
    schedules.set(&schedule)?;

    let mut history = chat.list()?;
    let prefs = preferences.list_all()?;
    let mut activity: Vec<String> = Vec::new();

    // AGENTIC overnight review (ADR 0056/0024): name a focus the person cares about
    // and let the butler reach for WHATEVER skills help look into it — its choice,
    // not a fixed script — then review the day and reflect, all in one grounded,
    // policy-gated pass (reversible + autonomous only; it drafts and forms beliefs
    // but does nothing it couldn't undo). The final reply IS the reflection.
    // Retire what's over before deciding anything, so a spent or stale thread can't
    // occupy the one active slot (ADR 0052).
    if let Some(why) = retire_finished_intention(intentions, clock)? {
        activity.push(format!("Stopped working on something — {why}"));
    }
    // Continue what Endora is already pursuing, or take something up. This is the
    // change ADR 0052 is for: seven nights on one thing, not seven unrelated evenings.
    let intention = match intentions.active()? {
        Some(existing) => Some(existing),
        None => take_up_an_intention(intentions, beliefs, ids, clock, &mut activity)?,
    };
    // Overnight work traces to the belief that prompted it (ADR 0053/0036).
    let actions = OutcomeSink::motivated_by(
        outcomes,
        intention.as_ref().map(Intention::motivating_belief),
    );
    let instruction = match intention.as_ref() {
        // Picked it up tonight — nothing found yet, so there is nothing to resume.
        Some(i) if i.note().is_empty() => format!(
            "It's the quiet overnight hour and they're away. You're taking up \"{}\" — \
             something they care about — so start looking into it, reaching for whatever \
             skills would help. Then review our recent conversation and what you understand \
             about them: note privately what you've learned or grown more sure of (as \
             beliefs), and leave a short, warm note they'll see in the morning — what you \
             looked into, what you noticed, and what you'll keep an eye on. Add nothing you \
             don't actually know.",
            i.statement()
        ),
        // Already under way: hand back its own account of where it got to, so tonight
        // continues the thread instead of starting it again.
        Some(i) => format!(
            "It's the quiet overnight hour and they're away. You're already looking into \
             \"{}\" — this is night {} of it. Here is what you found last time, in your own \
             words:\n\n{}\n\nCarry on from there rather than starting over: what's the next \
             thing worth checking? Reach for whatever skills would help. Then review our \
             recent conversation and what you understand about them, note privately what \
             you've learned (as beliefs), and leave a short, warm note they'll see in the \
             morning. Add nothing you don't actually know.",
            i.statement(),
            i.steps_taken() + 1,
            i.note()
        ),
        None => "It's the quiet overnight hour and they're away. Review our recent \
             conversation and what you understand about them: note privately what you've \
             learned or grown more sure of (as beliefs), and leave a short, warm note \
             they'll see in the morning — what you noticed and what you'll keep an eye on. \
             If there's genuinely nothing new, keep the note brief or say so plainly."
            .to_owned(),
    };
    history.push(ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        &instruction,
        now,
    )?);
    let review_ctx = ButlerContext {
        now: format_datetime_utc(now.unix_millis()),
        ..context.clone()
    };
    let reply = run_tool_turn(
        butler,
        capabilities,
        audit,
        &actions,
        ids,
        clock,
        &history,
        &prefs,
        &review_ctx,
        NIGHTLY_TOOL_ROUNDS,
        &mut |_step| {},
        &mut |_token: &str| {},
        &mut activity,
        &mut Vec::new(),
    )?;

    // Judged before the beliefs are moved out of the reply, and before anything is stored:
    // a night that produced no answer must not leave a trace as though it had.
    let worth_recording = !not_an_answer(&reply, &review_ctx);
    // Reflect: persist the understanding it formed (same path as a chat turn).
    record_formed_beliefs(beliefs, reply.beliefs, ids, clock, &mut activity)?;

    // Remember where tonight got to, in the butler's own words, so the next night can
    // pick the thread up (ADR 0052). Prose in, prose out — no state machine for the
    // model to maintain, and nothing here can corrupt if it writes something odd.
    if let Some(mut intention) = intention {
        let note = reply.text.trim();
        // A night that produced no answer is not a night's work. Endora's one long-running
        // intention — "you want to run more often" — reached five of its seven steps with
        // its stored progress reading *"Sorry, I couldn't reach my language model just
        // now."* An apology is non-empty, so it counted as a step, was written down as
        // what happened, and was handed to the next night as the thread to pick up.
        //
        // Two things were being spent on nothing: the step budget that makes an intention
        // retire itself, and the only record of what Endora has been doing about the thing
        // it decided to care about. The same predicate every other path already uses
        // settles it (ADR 0052).
        if !note.is_empty() && worth_recording {
            intention.progress(note, now);
            intentions.save(&intention)?;
            activity.push(format!(
                "Made progress on \"{}\" (night {})",
                intention.statement(),
                intention.steps_taken()
            ));
        }
    }

    // Forget: age out beliefs nothing has reinforced in a long time (ADR 0052).
    // Understanding is a living model, so the overnight review is where it *loses*
    // things as well as gains them.
    for statement in expire_faded_beliefs(beliefs, clock)? {
        activity.push(format!("Let go of a stale belief: {statement}"));
    }

    // Surface: leave the overnight note, if it wrote one. If it only refined
    // beliefs and had nothing worth saying, don't post a chat message — the beliefs
    // are already saved and reviewable in Understanding; we just stay quiet.
    let text = reply.text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let message = post_butler_message(chat, ids, clock, text)?;
    activity.push("Left an overnight note".to_owned());
    Ok(Some((message, activity)))
}

/// The topic the nightly loop researches, taken from what Endora actually
/// **understands** about the person (ADR 0052): the intent it is most sure of, since
/// intent is the slow-changing thing worth looking into overnight. Failing that, the
/// strongest belief of any kind. `None` when Endora understands nothing yet, so the
/// loop simply reflects without researching.
///
/// Confidence is the ranking, deliberately: researching a tentative guess spends the
/// night on something Endora may be wrong about.
/// Retires the active intention if it is spent or stale, freeing the one slot
/// (ADR 0052). Returns why, in plain words, for the activity trail.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
fn retire_finished_intention(
    intentions: &impl IntentionRepository,
    clock: &impl Clock,
) -> Result<Option<String>, AppError> {
    let Some(mut active) = intentions.active()? else {
        return Ok(None);
    };
    let Some(why) = active.retire_if_over(clock.now()) else {
        return Ok(None);
    };
    let reason = format!("{why} (\"{}\")", active.statement());
    intentions.save(&active)?;
    Ok(Some(reason))
}

/// Takes up something to pursue, from the belief Endora is most sure about (ADR 0052).
///
/// Only ever called with no active intention, so the one-at-a-time rule holds by
/// construction rather than by a check that could be forgotten. `None` when there is
/// nothing understood well enough to pursue — a new Endora simply has no thread yet.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails, or [`AppError::Domain`] if the
/// belief's statement somehow does not make a valid intention.
fn take_up_an_intention(
    intentions: &impl IntentionRepository,
    beliefs: &impl BeliefRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    activity: &mut Vec<String>,
) -> Result<Option<Intention>, AppError> {
    let Some((belief, statement)) = nightly_focus(beliefs, clock)? else {
        return Ok(None);
    };
    let intention = Intention::form(
        IntentionId::new(ids.new_id()),
        &statement,
        belief,
        clock.now(),
    )
    .map_err(AppError::Domain)?;
    intentions.save(&intention)?;
    activity.push(format!("Started looking into \"{statement}\""));
    Ok(Some(intention))
}

fn nightly_focus(
    beliefs: &impl BeliefRepository,
    clock: &impl Clock,
) -> Result<Option<(BeliefId, String)>, AppError> {
    let active = understanding(beliefs, clock)?;
    let strongest = |kind: Option<crate::BeliefKind>| {
        active
            .iter()
            .filter(|b| kind.is_none_or(|k| b.kind() == k))
            .max_by_key(|b| match b.confidence() {
                crate::Confidence::High => 2,
                crate::Confidence::Medium => 1,
                crate::Confidence::Low => 0,
            })
            .map(|b| (b.id(), b.statement().to_owned()))
    };
    Ok(strongest(Some(crate::BeliefKind::Intent)).or_else(|| strongest(None)))
}

/// Assembles the [`ButlerContext`] — a snapshot of what Endora understands about
/// the person and the skills it can reach right now — so the butler's conversation
/// is grounded in that rather than starting cold each turn.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn butler_context(
    beliefs: &impl BeliefRepository,
    outcomes: &impl OutcomeRepository,
    aliases: &(
         impl endora_capabilities::TargetAliasRepository
         + endora_capabilities::ConfigWriteLog
         + endora_capabilities::StandingTroubleRepository
     ),
    chat: &impl ChatRepository,
    capabilities: &dyn CapabilityRunner,
    clock: &impl Clock,
) -> Result<ButlerContext, AppError> {
    let understanding = understanding(beliefs, clock)?
        .into_iter()
        .map(|b| {
            format!(
                "[{}] {} ({} confidence)",
                b.kind().name(),
                b.statement(),
                b.confidence().name()
            )
        })
        .collect();
    // Ground the butler in the skills it can actually reach right now (configured
    // ones only), so it uses a real capability instead of only talking about it.
    let available: Vec<_> = capabilities
        .available()
        .into_iter()
        .filter(|c| c.configured)
        .collect();
    let skills = available
        .iter()
        .map(|c| format!("{} — {}", c.id, c.description))
        .collect();
    // The same skills structured for native tool-calling (exact id + input schema).
    let tools = available
        .iter()
        .map(|c| CapabilityTool {
            id: c.id.clone(),
            description: c.description.clone(),
            input_schema: c.input_schema.clone(),
        })
        .collect();
    // How its own past actions landed (ADR 0053) — a bounded read, since only the
    // recent stretch is informative and the prompt has to stay small.
    let recent = recent_outcomes(outcomes, TRACK_RECORD_WINDOW)?;
    Ok(ButlerContext {
        understanding,
        capabilities: skills,
        tools,
        // Live, and cheap: the services already read for other reasons know whether the
        // person is in the house.
        present: capabilities.about_the_person(),
        // What Endora itself has been doing, stated rather than fetched (ADR 0056). The
        // skill version of this was shipped first and a 7B model answered "did you do
        // anything while I was out?" by trying to turn a light on.
        did_lately: recently_did(chat, outcomes, aliases, aliases, clock)?,
        now: format_datetime_utc(clock.now().unix_millis()),
        conversation_summary: None,
        track_record: track_record(&recent),
        // What the person has said these targets are really called, and what Endora has
        // since made to stand for several of them (ADR 0054).
        target_aliases: aliases
            .aliases()?
            .iter()
            .map(endora_capabilities::TargetAlias::as_context)
            .chain(collections_worth_naming(aliases))
            .collect(),
    })
}

/// The last few things Endora did, short enough to sit in every turn's context.
///
/// The same facts [`what_it_has_been_doing`] reports, trimmed to what fits: the most recent
/// handful, newest last. A turn pays for its whole context, so this is a summary and the
/// skill is the detail.
fn recently_did(
    chat: &impl ChatRepository,
    outcomes: &impl OutcomeRepository,
    writes: &impl endora_capabilities::ConfigWriteLog,
    troubles: &impl endora_capabilities::StandingTroubleRepository,
    clock: &impl Clock,
) -> Result<Vec<String>, AppError> {
    /// Enough to answer "did anything happen?" without becoming the prompt.
    const FITS_IN_A_TURN: usize = 4;
    let now_ms = clock.now().unix_millis();
    let report =
        what_it_has_been_doing(chat, outcomes, writes, troubles, now_ms - A_DAY_MS, now_ms)?;
    // The empty answer is a sentence, not a list, and belongs here as one line: "nothing
    // happened" is exactly what the butler needs to be able to say without guessing.
    Ok(report
        .lines()
        .rev()
        .take(FITS_IN_A_TURN)
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect())
}

/// The facts a morning brief is *for*, gathered from what Endora already holds.
///
/// The brief was one instruction — *"reach for whatever's relevant"* — and four days of them
/// read like "the kitchen and garage lights are on". Every fact worth having was already in
/// the turn: what is on today, what it is like outside, what has stopped answering, what
/// Endora did overnight. None of it depended on the model choosing a tool, and all of it was
/// left to the model to choose a tool for.
///
/// Assembled deterministically for the same reason the activity account is
/// ([0056](../../docs/adr/0056-how-it-behaves-toward-you.md)): a report of stored facts is
/// the one thing Endora is entitled to assert, and the model's job is to say it nicely.
fn whats_worth_saying_this_morning(
    context: &ButlerContext,
    troubles: &[&endora_capabilities::StandingTrouble],
) -> String {
    let mut lines: Vec<String> = Vec::new();
    // Where they are, what is on, what it is like outside — the services' own words.
    lines.extend(context.present.iter().cloned());
    // Things that have been wrong long enough to be worth a mention. Only the ones already
    // judged worth raising, so the brief cannot become the pile of chores ADR 0056 forbids.
    for trouble in troubles {
        lines.push(trouble.statement(trouble.since_ms));
    }
    // What Endora itself has been doing, which is the question a proactive butler is asked
    // first and could not answer at all until recently.
    lines.extend(context.did_lately.iter().cloned());
    if lines.is_empty() {
        return "- nothing on today, nothing wrong, and nothing I did overnight".to_owned();
    }
    lines
        .iter()
        .map(|l| format!("- {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The collections Endora has made, as lines the butler can act on.
///
/// A group is useless if the model does not know it exists. Live: "All Lights" was created
/// and worked perfectly when named — and "turn off all lights" still produced a call aimed
/// at nothing, because nothing told the model there was now one thing to name.
///
/// This is grounding, exactly as a confirmed alias is: what a thing is called here. Only
/// collections Endora made and that have not been undone, so the list stays short and
/// every line is something that really exists.
fn collections_worth_naming(log: &impl endora_capabilities::ConfigWriteLog) -> Vec<String> {
    log.writes(COLLECTIONS_SHOWN)
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.kind == endora_capabilities::WriteKind::Collection && !w.undone)
        .map(|w| {
            format!(
                "on {}, \"{}\" is one thing standing for {} others — name it to act on \
                 them all at once",
                w.server,
                w.added,
                w.was.len()
            )
        })
        .collect()
}

/// How far back to look for collections. They are rare and long-lived; this only bounds
/// the read.
const COLLECTIONS_SHOWN: usize = 50;

/// Records a preference the butler should keep in mind. In this build every
/// preference is created by explicit confirmation, so it is always a *stated*
/// preference (the person's own words), never inferred.
///
/// # Errors
/// [`AppError::Domain`] if the text is blank, or [`AppError::Repository`] on
/// failure.
pub fn create_preference(
    preferences: &impl PreferenceRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    text: &str,
    kind: PreferenceKind,
) -> Result<Preference, AppError> {
    let preference = Preference::new(PreferenceId::new(ids.new_id()), text, kind, clock.now())?;
    preferences.save(&preference)?;
    Ok(preference)
}

/// Lists the preferences the butler has learned, oldest first.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn list_preferences(
    preferences: &impl PreferenceRepository,
) -> Result<Vec<Preference>, AppError> {
    Ok(preferences.list_all()?)
}

/// Forgets a preference (memory is correctable and deletable).
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn delete_preference(
    preferences: &impl PreferenceRepository,
    id: PreferenceId,
) -> Result<(), AppError> {
    preferences.delete(id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::OutcomeSink;
    use crate::ports::{
        Butler, ButlerContext, ButlerReply, Clock, IdSource, ProposalError, RepositoryError,
    };
    use crate::{
        AuditRecord, ChatMessage, MessageId, MessageRole, Preference, PreferenceId, Timestamp,
    };
    use crate::{Belief, BeliefId};
    use endora_capabilities::CapabilityRunner;
    use endora_conversation::ChatRepository;
    use endora_kernel::Reversibility;
    use endora_platform::AuditLog;
    use endora_scheduling::{CheckinRepository, CheckinSchedule};
    use endora_understanding::{
        BeliefRepository, IntentionRepository, OutcomeRepository, PreferenceRepository,
    };
    use std::cell::{Cell, RefCell};

    /// An in-memory store implementing the repository ports, for tests only.
    /// A capability runner with no skills — the default for tests that don't
    /// exercise the interventions loop (the butler never proposes a `use`).
    struct NoCapabilities;
    impl CapabilityRunner for NoCapabilities {
        fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
            Vec::new()
        }
        fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
            Err("no capabilities".to_owned())
        }
    }

    #[derive(Default)]
    struct FakeStore {
        messages: RefCell<Vec<ChatMessage>>,
        preferences: RefCell<Vec<Preference>>,
        checkin: RefCell<Option<CheckinSchedule>>,
        beliefs: RefCell<Vec<Belief>>,
    }

    impl BeliefRepository for FakeStore {
        fn save(&self, b: &Belief) -> Result<(), RepositoryError> {
            let mut v = self.beliefs.borrow_mut();
            if let Some(e) = v.iter_mut().find(|e| e.id() == b.id()) {
                *e = b.clone();
            } else {
                v.push(b.clone());
            }
            Ok(())
        }
        fn get(&self, id: BeliefId) -> Result<Option<Belief>, RepositoryError> {
            Ok(self.beliefs.borrow().iter().find(|b| b.id() == id).cloned())
        }
        fn list(&self) -> Result<Vec<Belief>, RepositoryError> {
            Ok(self.beliefs.borrow().clone())
        }
    }

    impl CheckinRepository for FakeStore {
        fn get(&self) -> Result<Option<CheckinSchedule>, RepositoryError> {
            Ok(*self.checkin.borrow())
        }
        fn set(&self, schedule: &CheckinSchedule) -> Result<(), RepositoryError> {
            *self.checkin.borrow_mut() = Some(*schedule);
            Ok(())
        }
    }

    impl ChatRepository for FakeStore {
        fn between(
            &self,
            from_ms: i64,
            to_ms: i64,
        ) -> Result<Vec<ChatMessage>, endora_kernel::RepositoryError> {
            Ok(ChatRepository::list(self)?
                .into_iter()
                .filter(|m| m.at().unix_millis() >= from_ms && m.at().unix_millis() < to_ms)
                .collect())
        }

        fn days(
            &self,
            _offset: i64,
        ) -> Result<Vec<(String, usize)>, endora_kernel::RepositoryError> {
            Ok(Vec::new())
        }

        fn append(&self, message: &ChatMessage) -> Result<(), RepositoryError> {
            self.messages.borrow_mut().push(message.clone());
            Ok(())
        }
        fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError> {
            Ok(self.messages.borrow().clone())
        }
    }

    impl PreferenceRepository for FakeStore {
        fn save(&self, preference: &Preference) -> Result<(), RepositoryError> {
            self.preferences.borrow_mut().push(preference.clone());
            Ok(())
        }
        fn list_all(&self) -> Result<Vec<Preference>, RepositoryError> {
            Ok(self.preferences.borrow().clone())
        }
        fn delete(&self, id: PreferenceId) -> Result<(), RepositoryError> {
            self.preferences.borrow_mut().retain(|p| p.id() != id);
            Ok(())
        }
    }

    /// A butler that echoes the newest message, so the turn can be exercised
    /// deterministically without a model.
    struct ScriptedTestButler;
    impl Butler for ScriptedTestButler {
        fn respond(
            &self,
            history: &[ChatMessage],
            _preferences: &[Preference],
            _context: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            let last = history.last().map(ChatMessage::text).unwrap_or_default();
            Ok(ButlerReply {
                text: format!("Noted. You said: {last}"),
                ..ButlerReply::default()
            })
        }
    }

    /// A deterministic id source: 1, 2, 3, ...
    #[derive(Default)]
    struct SeqIds {
        next: Cell<u128>,
    }

    impl IdSource for SeqIds {
        fn new_id(&self) -> u128 {
            let id = self.next.get() + 1;
            self.next.set(id);
            id
        }
    }

    /// The two directions of belief de-duplication. The live eval (ADR 0055) found
    /// qwen2.5:7b re-stating what it already understood, so the deterministic
    /// backstop — not the prompt — has to hold this line
    /// (see the "deterministic over prompting" lesson behind ADR 0053).
    mod dedup {
        use super::super::{similar, statements_disagree};

        const KNOWN: &str = "you want more energy so you can travel when you retire";

        #[test]
        fn rephrasings_of_the_same_belief_are_one_belief() {
            for rephrasing in [
                "you want more energy to travel when you retire",
                "you want to have the energy to travel once you retire",
                "you would like the energy for travelling after you retire",
                "you want the energy to travel in retirement",
            ] {
                assert!(
                    similar(KNOWN, rephrasing),
                    "should have merged: {rephrasing:?}"
                );
            }
        }

        #[test]
        fn distinct_beliefs_are_never_merged() {
            // Wrongly merging loses understanding silently, with nothing for the
            // person to correct — strictly worse than keeping a duplicate.
            for distinct in [
                "you find long flights physically difficult",
                "you are dreading retirement",
                "you want more energy for your work",
                "your brother is planning a hike in September",
                "you travel for work more than you'd like",
                // Borderline on purpose: shares the travel-in-retirement thread but
                // claims a different driver (motivation, not energy). Kept separate —
                // merging would silently drop the new claim.
                "you are motivated by the freedom of retirement",
            ] {
                assert!(
                    !similar(KNOWN, distinct),
                    "should NOT have merged: {distinct:?}"
                );
            }
        }

        #[test]
        fn stemming_matches_inflections_without_collapsing_short_words() {
            assert!(similar("you enjoy hiking", "you enjoy hikes"));
            assert!(similar("you value retirement", "you value retiring"));
            // Short words must not stem down to a shared stub.
            assert!(!similar("you like ties", "you like tea"));
        }

        /// Every one of these was live in the deployed database at once. `similar`
        /// merged all four — three of them contradictions — so the disagreement was
        /// silently discarded and whichever arrived first won.
        #[test]
        fn contradictory_beliefs_are_never_merged() {
            for (a, b) in [
                (
                    "you want me to turn off the kitchen light",
                    "You want me to turn on the kitchen lights.",
                ),
                (
                    "You prefer temperature measurements in Fahrenheit.",
                    "You find it more convenient and accurate to measure temperature in \
                     Celsius rather than Fahrenheit.",
                ),
                ("you like tea", "you don't like tea"),
                (
                    "you always run in the morning",
                    "you never run in the morning",
                ),
            ] {
                assert!(!similar(a, b), "merged a contradiction:\n  {a:?}\n  {b:?}");
                assert!(statements_disagree(a, b), "should disagree: {a:?} / {b:?}");
            }
        }

        #[test]
        fn polarity_is_read_from_words_a_keyword_filter_would_drop() {
            // "on" is two characters, "like" is a stopword, "don't" splits into
            // fragments — all invisible to `keywords`, all decisive here.
            assert!(statements_disagree("turn it on", "turn it off"));
            assert!(statements_disagree("you like tea", "you dont like tea"));
            assert!(statements_disagree("you run early", "you run late"));
        }

        #[test]
        fn agreeing_statements_are_not_mistaken_for_disagreement() {
            // Both negated, so the negation is symmetric and not a disagreement.
            assert!(!statements_disagree(
                "you never run in the morning",
                "you dont run in the morning"
            ));
            // Sharing no polarity vocabulary at all.
            assert!(!statements_disagree(
                "you want more energy to travel",
                "you want the energy to travel in retirement"
            ));
            // ...and a genuine rephrasing still merges.
            assert!(similar(
                "you want more energy so you can travel when you retire",
                "you want the energy to travel in retirement"
            ));
        }

        #[test]
        fn an_empty_statement_falls_back_to_exact_comparison() {
            assert!(similar("", ""));
            assert!(!similar("", KNOWN));
        }
    }

    /// ADR 0053: the turn must be able to tell an observation from an actuator's
    /// claim about its own work.
    mod verification {
        use super::super::note_verification;
        use endora_capabilities::CapabilitySpec;
        use endora_kernel::Reversibility;

        fn spec(band: Reversibility) -> CapabilitySpec {
            CapabilitySpec {
                id: "x".to_owned(),
                description: "x".to_owned(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: band,
            }
        }

        #[test]
        fn an_observation_stands_on_its_own() {
            // A read reports state; the result IS the evidence.
            let out = note_verification("72F, sunny", Some(&spec(Reversibility::Observe)), None);
            assert_eq!(out, "72F, sunny");
        }

        #[test]
        fn an_actuators_own_account_is_marked_unverified() {
            // The live failure: HA reported action_done for a call that changed
            // nothing, and the butler announced success while the light stayed on.
            let out = note_verification(
                "The action completed successfully on: Kitchen Table (light).",
                Some(&spec(Reversibility::Irreversible)),
                None,
            );
            assert!(
                out.contains("The action completed successfully"),
                "keeps the result"
            );
            assert!(out.contains("[unverified]"), "marks it: {out}");
            assert!(
                out.contains("NOT independently confirmed"),
                "says why it is unverified: {out}"
            );
        }

        #[test]
        fn every_band_that_changes_anything_is_marked() {
            for band in [
                Reversibility::Reversible,
                Reversibility::OutwardReversible,
                Reversibility::Irreversible,
            ] {
                assert!(
                    note_verification("done", Some(&spec(band)), None).contains("[unverified]"),
                    "{band:?} should be marked"
                );
            }
        }

        #[test]
        fn an_unknown_capability_is_treated_as_an_actuator() {
            // Failing closed: if we cannot tell what it did, we do not vouch for it.
            assert!(note_verification("done", None, None).contains("[unverified]"));
        }
    }

    /// ADR 0053 layer 1. The payload is the real one captured from a live Home
    /// Assistant during the session that produced this code.
    mod read_back {
        use super::super::note_verification;
        use endora_capabilities::CapabilitySpec;
        use endora_kernel::Reversibility;

        fn actuator() -> CapabilitySpec {
            CapabilitySpec {
                id: "home-assistant.HassTurnOff".to_owned(),
                description: "x".to_owned(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: Reversibility::Irreversible,
            }
        }

        #[test]
        fn an_observation_replaces_the_actuators_claim() {
            let out = note_verification(
                "The action completed successfully on: Kitchen (area).",
                Some(&actuator()),
                Some("switch Kitchen is 'on'"),
            );
            assert!(out.contains("[observed]"), "{out}");
            assert!(
                out.contains("switch Kitchen is 'on'"),
                "carries the reading: {out}"
            );
            assert!(
                out.contains("the observation wins"),
                "tells the model which to believe: {out}"
            );
            assert!(
                !out.contains("[unverified]"),
                "no longer merely unverified: {out}"
            );
        }

        #[test]
        fn without_a_read_back_it_stays_unverified() {
            let out = note_verification("done", Some(&actuator()), None);
            assert!(out.contains("[unverified]"), "{out}");
        }
    }

    /// Commands are not facts about a person. Live data had two of them filed as
    /// durable beliefs, contradicting each other.
    mod instructions {
        use super::super::reads_as_an_instruction;

        #[test]
        fn a_one_off_command_is_not_understanding() {
            for command in [
                "you want me to turn off the kitchen light",
                "You want me to turn on the kitchen lights.",
                "you asked me to play some music",
                "you told me to add milk to the shopping list",
                "you would like me to lock the front door",
            ] {
                assert!(reads_as_an_instruction(command), "should drop: {command:?}");
            }
        }

        #[test]
        fn a_standing_preference_about_endora_is_kept() {
            // These are also phrased as "you want me to …" but describe how Endora
            // should *be*, not a task to carry out. Dropping them would lose the
            // most useful thing the person can tell the butler about itself.
            for preference in [
                "you want me to be more direct",
                "you want me to keep an eye on your sleep",
                "you asked me to check in less often",
                "you would like me to explain my reasoning",
            ] {
                assert!(
                    !reads_as_an_instruction(preference),
                    "should keep: {preference:?}"
                );
            }
        }

        #[test]
        fn ordinary_beliefs_are_untouched() {
            for belief in [
                "you want more energy to travel when you retire",
                "you prefer temperatures in Fahrenheit",
                "you find mornings hard",
                "you turn in early on weeknights",
            ] {
                assert!(!reads_as_an_instruction(belief), "should keep: {belief:?}");
            }
        }
    }

    /// The most recent skill result in the conversation, for test butlers that have
    /// no native tool-calling. The default [`Butler::take_turn`] shim folds each tool
    /// result into the history as a `[skill result] …` turn (ADR 0053 — results ride
    /// in the conversation, never the system prompt), so this is how a `respond`-only
    /// butler sees what came back.
    fn last_skill_result(history: &[ChatMessage]) -> Option<String> {
        history.iter().rev().find_map(|m| {
            m.text()
                .strip_prefix("[skill result] ")
                .map(|rest| rest.split('\n').next().unwrap_or(rest).to_owned())
        })
    }

    /// A clock fixed at a chosen instant.
    struct FixedClock(i64);

    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            Timestamp::from_unix_millis(self.0)
        }
    }

    /// An in-memory audit log.
    #[derive(Default)]
    struct FakeAudit {
        records: RefCell<Vec<AuditRecord>>,
    }

    impl AuditLog for FakeAudit {
        fn append(&self, record: &AuditRecord) -> Result<(), RepositoryError> {
            self.records.borrow_mut().push(record.clone());
            Ok(())
        }
        fn recent(&self, limit: usize) -> Result<Vec<AuditRecord>, RepositoryError> {
            let all = self.records.borrow();
            Ok(all.iter().rev().take(limit).cloned().collect())
        }
    }

    /// An in-memory [`OutcomeRepository`] (ADR 0053), so a test can assert on what the
    /// turn recorded about the actions it took.
    #[derive(Default)]
    struct FakeOutcomes {
        saved: RefCell<Vec<crate::Outcome>>,
    }

    impl OutcomeRepository for FakeOutcomes {
        fn save(&self, outcome: &crate::Outcome) -> Result<(), RepositoryError> {
            let mut all = self.saved.borrow_mut();
            // Mirror the store's INSERT OR REPLACE so reacting updates rather than
            // appending a second row.
            if let Some(existing) = all.iter_mut().find(|o| o.id() == outcome.id()) {
                *existing = outcome.clone();
                return Ok(());
            }
            all.push(outcome.clone());
            Ok(())
        }
        fn get(&self, id: crate::OutcomeId) -> Result<Option<crate::Outcome>, RepositoryError> {
            Ok(self.saved.borrow().iter().find(|o| o.id() == id).cloned())
        }
        fn list(&self) -> Result<Vec<crate::Outcome>, RepositoryError> {
            Ok(self.saved.borrow().clone())
        }
    }

    /// An in-memory [`IntentionRepository`] (ADR 0052), so a test can watch Endora
    /// take something up, carry it across nights, and drop it.
    #[derive(Default)]
    struct FakeIntentions {
        saved: RefCell<Vec<crate::Intention>>,
    }

    impl IntentionRepository for FakeIntentions {
        fn save(&self, intention: &crate::Intention) -> Result<(), RepositoryError> {
            let mut all = self.saved.borrow_mut();
            if let Some(existing) = all.iter_mut().find(|i| i.id() == intention.id()) {
                *existing = intention.clone();
                return Ok(());
            }
            all.push(intention.clone());
            Ok(())
        }
        fn get(&self, id: crate::IntentionId) -> Result<Option<crate::Intention>, RepositoryError> {
            Ok(self.saved.borrow().iter().find(|i| i.id() == id).cloned())
        }
        fn active(&self) -> Result<Option<crate::Intention>, RepositoryError> {
            Ok(self.saved.borrow().iter().find(|i| i.is_active()).cloned())
        }
        fn list(&self) -> Result<Vec<crate::Intention>, RepositoryError> {
            Ok(self.saved.borrow().clone())
        }
    }

    /// An [`OutcomeRepository`] whose writes always fail — ADR 0053 makes recording
    /// best-effort, so a broken store must never break a working action.
    struct BrokenOutcomes;

    impl OutcomeRepository for BrokenOutcomes {
        fn save(&self, _outcome: &crate::Outcome) -> Result<(), RepositoryError> {
            Err(RepositoryError::Backend("disk on fire".to_owned()))
        }
        fn get(&self, _id: crate::OutcomeId) -> Result<Option<crate::Outcome>, RepositoryError> {
            Ok(None)
        }
        fn list(&self) -> Result<Vec<crate::Outcome>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn sending_to_the_butler_records_both_turns() {
        use super::{chat_history, send_to_butler};
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);

        let (reply, _activity) = send_to_butler(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &NoCapabilities,
            &ScriptedTestButler,
            &FakeAudit::default(),
            &ids,
            &clock,
            &ButlerContext::default(),
            "I want to run more",
        )
        .unwrap();
        assert_eq!(reply.role(), MessageRole::Butler);
        assert!(reply.text().contains("I want to run more"));

        // Both turns are persisted, oldest first.
        let history = chat_history(&store).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role(), MessageRole::User);
        assert_eq!(history[1].role(), MessageRole::Butler);
    }

    #[test]
    fn a_cleared_skill_runs_and_the_butler_answers_with_its_result() {
        use super::send_to_butler;

        // A butler that first asks to use the "weather" skill, then — once the skill
        // result arrives as a turn in the conversation — answers using it. This is the
        // propose → policy authorizes → execute → answer loop the use case drives.
        struct ToolButler;
        impl Butler for ToolButler {
            fn respond(
                &self,
                history: &[ChatMessage],
                _p: &[Preference],
                _context: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                if let Some(result) = last_skill_result(history) {
                    // The result is in front of it — answer from it.
                    return Ok(ButlerReply {
                        text: format!("Here's what I found — {result}"),
                        ..ButlerReply::default()
                    });
                }
                // First pass: brief reply + a skill request.
                Ok(ButlerReply {
                    text: "One moment — checking.".to_owned(),
                    capability_use: Some(endora_capabilities::CapabilityUse {
                        capability: "weather".to_owned(),
                        input_json: "{\"location\":\"New York\"}".to_owned(),
                    }),
                    ..ButlerReply::default()
                })
            }
        }

        // A runner offering one cleared (configured + autonomous) skill.
        struct OneSkill;
        impl CapabilityRunner for OneSkill {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                vec![endora_capabilities::CapabilitySpec {
                    id: "weather".to_owned(),
                    description: "current conditions".to_owned(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: endora_kernel::Reversibility::Observe,
                }]
            }
            fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
                assert_eq!(id, "weather");
                assert!(input_json.contains("New York"));
                Ok("sunny, 30C".to_owned())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let (reply, activity) = send_to_butler(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &OneSkill,
            &ToolButler,
            &FakeAudit::default(),
            &ids,
            &clock,
            &ButlerContext::default(),
            "what's the weather in New York?",
        )
        .unwrap();

        // The persisted reply is the synthesis (using the skill result), not the
        // brief "one moment" placeholder.
        assert!(reply.text().contains("sunny, 30C"));
        // And the skill use is recorded in the turn's activity.
        assert!(activity.iter().any(|a| a.contains("weather")));
    }

    #[test]
    fn a_failing_tool_is_not_retried_more_than_twice() {
        use super::send_to_butler;

        // A butler that asks to use the same tool every round and never answers on its
        // own — so only the failure cap can stop the loop.
        struct RelentlessButler;
        impl Butler for RelentlessButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "trying".to_owned(),
                    capability_use: Some(endora_capabilities::CapabilityUse {
                        capability: "home.HassLightSet".to_owned(),
                        input_json: "{}".to_owned(),
                    }),
                    ..ButlerReply::default()
                })
            }
        }

        // A cleared (autonomous) tool whose run always fails.
        struct AlwaysFails;
        impl CapabilityRunner for AlwaysFails {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                vec![endora_capabilities::CapabilitySpec {
                    id: "home.HassLightSet".to_owned(),
                    description: "set a light".to_owned(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: endora_kernel::Reversibility::Observe,
                }]
            }
            fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
                Err("no lights match 'kitchen'".to_owned())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let (_reply, activity) = send_to_butler(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &AlwaysFails,
            &RelentlessButler,
            &FakeAudit::default(),
            &ids,
            &clock,
            &ButlerContext::default(),
            "turn on the kitchen lights",
        )
        .unwrap();

        // A dead end is never hammered: with the same tool+input each round, the
        // repeated-call guard runs it once; a burst of distinct failures caps at two.
        let tries = activity.iter().filter(|a| a.contains("failed")).count();
        assert!(
            tries <= 2,
            "should never retry a dead end past the cap, got: {activity:?}"
        );
    }

    // A cleared (configured + autonomous) skill whose run returns `result`.
    struct FixedSkill {
        id: &'static str,
        result: Result<String, String>,
    }
    impl CapabilityRunner for FixedSkill {
        fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
            vec![endora_capabilities::CapabilitySpec {
                id: self.id.to_owned(),
                description: "does a thing".to_owned(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: endora_kernel::Reversibility::Observe,
            }]
        }
        fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
            self.result.clone()
        }
    }

    /// An actuator with a declared band and an optional state reader that verifies it
    /// (ADR 0053), so a test can exercise what the turn *records* about an action.
    struct BandedSkill {
        id: &'static str,
        band: Reversibility,
        result: Result<String, String>,
        /// What the read-back reports, if this integration has one.
        reads_back: Option<&'static str>,
    }
    impl CapabilityRunner for BandedSkill {
        fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
            let mut specs = vec![endora_capabilities::CapabilitySpec {
                id: self.id.to_owned(),
                description: "does a thing".to_owned(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: self.band,
            }];
            if self.reads_back.is_some() {
                specs.push(endora_capabilities::CapabilitySpec {
                    id: "reader".to_owned(),
                    description: "reads state".to_owned(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: Reversibility::Observe,
                });
            }
            specs
        }
        fn verifier(&self, id: &str) -> Option<String> {
            (id == self.id && self.reads_back.is_some()).then(|| "reader".to_owned())
        }
        fn run(&self, id: &str, _input: &str) -> Result<String, String> {
            if id == "reader" {
                return Ok(self.reads_back.unwrap_or_default().to_owned());
            }
            self.result.clone()
        }
    }

    /// Runs one turn against `skill` and hands back everything it recorded.
    fn outcomes_of(skill: &BandedSkill, sink: &OutcomeSink<'_>) -> Vec<crate::Outcome> {
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(4_000), FakeAudit::default());
        let _ = super::run_tool_turn(
            &CallThenEcho {
                capability: skill.id,
            },
            skill,
            &audit,
            sink,
            &ids,
            &clock,
            &one_user_turn("do the thing"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut Vec::new(),
            &mut Vec::new(),
        );
        sink.outcomes.list().unwrap()
    }

    /// An outcome for `capability` with a given reaction, for track-record tests.
    fn judged(id: u128, capability: &str, reaction: Option<crate::Reaction>) -> crate::Outcome {
        let mut outcome = crate::Outcome::record(
            crate::OutcomeId::new(id),
            capability,
            "{}",
            "done",
            None,
            Timestamp::from_unix_millis(id as i64),
            None,
            None,
        )
        .expect("valid");
        if let Some(reaction) = reaction {
            outcome.react(reaction);
        }
        outcome
    }

    #[test]
    fn the_track_record_only_mentions_skills_the_person_judged() {
        use crate::Reaction::{DidNotHelp, Helped, NoReaction};
        let record = super::track_record(&[
            judged(1, "weather", Some(Helped)),
            judged(2, "weather", Some(Helped)),
            judged(3, "weather", Some(DidNotHelp)),
            // Never reacted to: says nothing about whether it helped, so it must not
            // pad the prompt (ADR 0053).
            judged(4, "news", None),
            // "Made no difference" is not a judgement to learn from either.
            judged(5, "web_search", Some(NoReaction)),
        ]);
        assert_eq!(record, vec!["weather — helped 2 time(s), didn't help 1"]);
    }

    #[test]
    fn the_track_record_is_empty_before_the_person_says_anything() {
        // The normal early state — and it must stay out of the prompt entirely.
        let record = super::track_record(&[judged(1, "weather", None)]);
        assert!(record.is_empty(), "{record:?}");
    }

    #[test]
    fn the_track_record_leads_with_the_most_judged_skill() {
        use crate::Reaction::{DidNotHelp, Helped};
        let record = super::track_record(&[
            judged(1, "news", Some(Helped)),
            judged(2, "weather", Some(Helped)),
            judged(3, "weather", Some(DidNotHelp)),
        ]);
        assert_eq!(
            record,
            vec![
                "weather — helped 1 time(s), didn't help 1",
                "news — helped 1 time(s)",
            ]
        );
    }

    #[test]
    fn reacting_to_an_outcome_records_the_latest_word() {
        use super::react_to_outcome;
        let store = FakeOutcomes::default();
        store.save(&judged(1, "weather", None)).unwrap();

        let out = react_to_outcome(&store, crate::OutcomeId::new(1), crate::Reaction::Helped)
            .expect("the outcome exists");
        assert_eq!(out.reaction(), Some(crate::Reaction::Helped));

        // They may change their mind.
        react_to_outcome(
            &store,
            crate::OutcomeId::new(1),
            crate::Reaction::DidNotHelp,
        )
        .unwrap();
        assert_eq!(store.list().unwrap().len(), 1, "updated, not duplicated");
        assert_eq!(
            store.list().unwrap()[0].reaction(),
            Some(crate::Reaction::DidNotHelp)
        );
    }

    #[test]
    fn reacting_to_an_outcome_that_does_not_exist_is_not_found() {
        use super::react_to_outcome;
        let store = FakeOutcomes::default();
        let err = react_to_outcome(&store, crate::OutcomeId::new(9), crate::Reaction::Helped);
        assert!(matches!(
            err,
            Err(crate::AppError::NotFound { entity: "outcome" })
        ));
    }

    /// Runs one turn and returns what the interface would show about its actions.
    fn disclosures_of(skill: &BandedSkill) -> Vec<super::ActionDisclosure> {
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let mut disclosed = Vec::new();
        let _ = super::run_tool_turn(
            &CallThenEcho {
                capability: skill.id,
            },
            skill,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("do the thing"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut Vec::new(),
            &mut disclosed,
        );
        disclosed
    }

    #[test]
    fn an_unverified_action_is_disclosed_even_when_the_reply_claims_success() {
        // ADR 0053, and the whole reason it exists. Measured, this model asserts an
        // unverified success EVERY time (verify:unconfirmed-is-not-overclaimed 0/3).
        // `CallThenEcho` reproduces that: it echoes the tool's "action_done" as its
        // answer. The disclosure must appear anyway — the guarantee is about code, not
        // about what the model chose to say.
        let disclosed = disclosures_of(&BandedSkill {
            id: "home.HassTurnOn",
            band: Reversibility::Irreversible,
            result: Ok("action_done".to_owned()),
            reads_back: None,
        });

        assert_eq!(disclosed.len(), 1, "the action is shown: {disclosed:?}");
        assert_eq!(disclosed[0].skill, "home.HassTurnOn");
        assert_eq!(disclosed[0].claimed, "action_done");
        assert!(
            !disclosed[0].was_observed(),
            "and it is shown as unconfirmed, which is the point"
        );
    }

    #[test]
    fn a_claim_of_success_that_changed_nothing_is_called_out() {
        // The live case, exactly. Home Assistant reported "completed successfully on:
        // Kitchen (area), Kitchen Table (light)" — true, and useless: Kitchen Table was
        // unavailable and Kitchen Main, the switch actually on, stayed on. Two readings
        // settle it; neither has to be understood.
        struct ActsButChangesNothing;
        impl CapabilityRunner for ActsButChangesNothing {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                ["home.HassTurnOff", "home.GetLiveContext"]
                    .into_iter()
                    .map(|id| endora_capabilities::CapabilitySpec {
                        id: id.to_owned(),
                        description: String::new(),
                        configured: true,
                        autonomous: true,
                        input_schema: None,
                        reversibility: if id.ends_with("GetLiveContext") {
                            Reversibility::Observe
                        } else {
                            Reversibility::Irreversible
                        },
                    })
                    .collect()
            }
            fn verifier(&self, id: &str) -> Option<String> {
                (id == "home.HassTurnOff").then(|| "home.GetLiveContext".to_owned())
            }
            fn run(&self, id: &str, _input: &str) -> Result<String, String> {
                if id == "home.GetLiveContext" {
                    // Identical every time — the switch never moves.
                    return Ok("Kitchen Main | switch | state: on".to_owned());
                }
                Ok("The action completed successfully on: Kitchen (area).".to_owned())
            }
        }

        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let reply = super::run_tool_turn(
            &CallThenEcho {
                capability: "home.HassTurnOff",
            },
            &ActsButChangesNothing,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn the kitchen light off"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .expect("the turn answers");

        // `CallThenEcho` echoes the tool result, so the note reaches the model.
        assert!(
            reply.text.contains("[unchanged]"),
            "a success that changed nothing went unremarked: {}",
            reply.text
        );
    }

    #[test]
    fn a_real_change_is_not_called_unchanged() {
        // The note must stay silent when something did move, or it becomes noise the
        // model learns to ignore.
        assert_eq!(
            super::note_unchanged(Some("light: on"), Some("light: off")),
            ""
        );
        // And silent when there is nothing to compare.
        assert_eq!(super::note_unchanged(None, Some("light: off")), "");
        assert_eq!(super::note_unchanged(Some("light: on"), None), "");
    }

    #[test]
    fn a_disclosure_carries_the_reading_without_judging_it() {
        // The kitchen light. ADR 0053 declined to derive a verdict — that needs a model
        // of intent which doesn't exist — so the claim and the reading travel side by
        // side and the person judges. Collapsing them here would be the canned string
        // ADR 0053 deleted, wearing a hat.
        let disclosed = disclosures_of(&BandedSkill {
            id: "home.HassTurnOff",
            band: Reversibility::Irreversible,
            result: Ok("action_done".to_owned()),
            reads_back: Some("kitchen switch: on"),
        });

        assert_eq!(disclosed.len(), 1);
        assert_eq!(disclosed[0].claimed, "action_done");
        assert_eq!(disclosed[0].observed.as_deref(), Some("kitchen switch: on"));
        assert!(disclosed[0].was_observed());
    }

    #[test]
    fn reading_the_world_is_not_disclosed_as_an_action() {
        // Same rule as the outcome record: an Observe capability changes nothing, so
        // there is nothing to disclose and no reason to clutter the reply with it.
        let disclosed = disclosures_of(&BandedSkill {
            id: "weather",
            band: Reversibility::Observe,
            result: Ok("72F, sunny".to_owned()),
            reads_back: None,
        });
        assert!(disclosed.is_empty(), "a read was disclosed: {disclosed:?}");
    }

    #[test]
    fn a_turn_does_not_end_on_a_failed_action_while_budget_remains() {
        // Observed live twice: one call failed, the read-back named what is really
        // there, and the model answered "Let's try again… Here is the request:" — the
        // preamble to a tool call, with no call. The loop took that as its final word.
        struct FailsThenNarratesThenRecovers {
            turns: std::cell::Cell<usize>,
        }
        impl Butler for FailsThenNarratesThenRecovers {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn take_turn(
                &self,
                _conversation: &[crate::TurnMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                let n = self.turns.get();
                self.turns.set(n + 1);
                match n {
                    // Calls with the wrong target; it fails.
                    0 => Ok(ButlerReply {
                        tool_calls: vec![crate::ToolCall {
                            id: "c".to_owned(),
                            capability: "home.HassTurnOff".to_owned(),
                            input_json: r#"{"name":"wrong"}"#.to_owned(),
                        }],
                        ..ButlerReply::default()
                    }),
                    // Narrates a plan instead of acting — the live failure.
                    1 => Ok(ButlerReply {
                        text: "Let's try again. Here is the request:".to_owned(),
                        ..ButlerReply::default()
                    }),
                    // Given another round, actually corrects itself.
                    _ => Ok(ButlerReply {
                        text: "Turned it off.".to_owned(),
                        tool_calls: vec![crate::ToolCall {
                            id: "c2".to_owned(),
                            capability: "home.HassTurnOff".to_owned(),
                            input_json: r#"{"name":"Kitchen Main Light"}"#.to_owned(),
                        }],
                        ..ButlerReply::default()
                    }),
                }
            }
        }

        struct FailsTheWrongName;
        impl CapabilityRunner for FailsTheWrongName {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                vec![endora_capabilities::CapabilitySpec {
                    id: "home.HassTurnOff".to_owned(),
                    description: String::new(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: Reversibility::Irreversible,
                }]
            }
            fn run(&self, _id: &str, input: &str) -> Result<String, String> {
                if input.contains("wrong") {
                    return Err("no_match_reason=NAME".to_owned());
                }
                Ok("action_done".to_owned())
            }
        }

        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let mut activity = Vec::new();
        let reply = super::run_tool_turn(
            &FailsThenNarratesThenRecovers {
                turns: std::cell::Cell::new(0),
            },
            &FailsTheWrongName,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn the kitchen switch off"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .expect("the turn answers");

        // It got a second chance and used it, rather than the turn ending on prose.
        assert!(
            activity.iter().filter(|a| a.contains("Used the")).count() == 1,
            "the corrected call never ran: {activity:?}"
        );
        assert!(
            !reply.text.contains("Here is the request"),
            "the turn ended on the narration: {}",
            reply.text
        );
    }

    #[test]
    fn a_turn_that_simply_has_nothing_more_to_do_still_ends() {
        // The recovery branch must not become a loop. A model that answers plainly
        // after a failure gets its second chance, answers again, and that stands.
        struct FailsThenAnswers {
            turns: std::cell::Cell<usize>,
        }
        impl Butler for FailsThenAnswers {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn take_turn(
                &self,
                _conversation: &[crate::TurnMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                let n = self.turns.get();
                self.turns.set(n + 1);
                if n == 0 {
                    return Ok(ButlerReply {
                        tool_calls: vec![crate::ToolCall {
                            id: "c".to_owned(),
                            capability: "home.HassTurnOff".to_owned(),
                            input_json: r#"{"name":"wrong"}"#.to_owned(),
                        }],
                        ..ButlerReply::default()
                    });
                }
                Ok(ButlerReply {
                    text: "I couldn't find that one, sir.".to_owned(),
                    ..ButlerReply::default()
                })
            }
        }

        struct AlwaysFails;
        impl CapabilityRunner for AlwaysFails {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                vec![endora_capabilities::CapabilitySpec {
                    id: "home.HassTurnOff".to_owned(),
                    description: String::new(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: Reversibility::Irreversible,
                }]
            }
            fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
                Err("no_match_reason=NAME".to_owned())
            }
        }

        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let reply = super::run_tool_turn(
            &FailsThenAnswers {
                turns: std::cell::Cell::new(0),
            },
            &AlwaysFails,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn it off"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .expect("the turn answers");
        assert_eq!(reply.text, "I couldn't find that one, sir.");
    }

    #[test]
    fn a_failed_action_reads_the_world_back_without_inheriting_its_bad_target() {
        // Live: HassTurnOff{area:"Kitchen Main"} failed because that area does not
        // exist — and the scoped read-back inherited the same argument and returned the
        // identical error, so the butler kept insisting there were no lights in a
        // kitchen that had five. When an action fails, its target is the prime suspect;
        // reading back with it is guaranteed to teach nothing.
        struct FailsThenReads;
        impl CapabilityRunner for FailsThenReads {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                ["home.HassTurnOff", "home.GetLiveContext"]
                    .into_iter()
                    .map(|id| endora_capabilities::CapabilitySpec {
                        id: id.to_owned(),
                        description: String::new(),
                        configured: true,
                        autonomous: true,
                        input_schema: None,
                        reversibility: if id.ends_with("GetLiveContext") {
                            Reversibility::Observe
                        } else {
                            Reversibility::Irreversible
                        },
                    })
                    .collect()
            }
            fn verifier(&self, id: &str) -> Option<String> {
                (id == "home.HassTurnOff").then(|| "home.GetLiveContext".to_owned())
            }
            fn read_back_input(&self, _action: &str, action_input: &str) -> String {
                // Stands in for the real scoping: whatever it is handed.
                action_input.to_owned()
            }
            fn run(&self, id: &str, input: &str) -> Result<String, String> {
                if id == "home.HassTurnOff" {
                    return Err("Area 'Kitchen Main' does not exist".to_owned());
                }
                // The reader fails the same way IF it inherits the bad target.
                if input.contains("Kitchen Main") {
                    return Err("Area 'Kitchen Main' does not exist".to_owned());
                }
                Ok("Kitchen Main | domain: switch | state: on".to_owned())
            }
        }

        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let mut disclosed = Vec::new();
        let _ = super::run_tool_turn(
            &CallThenEcho {
                capability: "home.HassTurnOff",
            },
            &FailsThenReads,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn off the kitchen main"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut Vec::new(),
            &mut disclosed,
        );

        let observed = disclosed
            .first()
            .and_then(|d| d.observed.clone())
            .expect("a failed action still reads the world back");
        assert!(
            observed.contains("switch"),
            "the reading must show what IS there, not repeat the failure: {observed}"
        );
    }

    #[test]
    fn a_failed_action_is_disclosed_too() {
        let disclosed = disclosures_of(&BandedSkill {
            id: "home.HassTurnOff",
            band: Reversibility::Irreversible,
            result: Err("no_match_reason=AREA".to_owned()),
            reads_back: Some("kitchen switch: on"),
        });
        assert_eq!(disclosed.len(), 1);
        assert!(disclosed[0].claimed.contains("no_match_reason=AREA"));
        assert_eq!(disclosed[0].observed.as_deref(), Some("kitchen switch: on"));
    }

    #[test]
    fn the_disclosure_never_edits_the_reply() {
        // ADR 0053 stands: the butler's words are its own. This adds a channel beside
        // the reply, it does not append to or rewrite it — so a model that overclaims
        // still visibly overclaims, and the eval can still catch it.
        //
        // The butler here is the measured failure mode: it acts, then flatly asserts
        // success it never verified.
        const OVERCLAIM: &str = "All set — the lights are on.";
        struct ActsThenOverclaims;
        impl Butler for ActsThenOverclaims {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn take_turn(
                &self,
                conversation: &[crate::TurnMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                let acted = conversation
                    .iter()
                    .any(|m| matches!(m, crate::TurnMessage::ToolResult { .. }));
                if acted {
                    return Ok(ButlerReply {
                        text: OVERCLAIM.to_owned(),
                        ..ButlerReply::default()
                    });
                }
                Ok(ButlerReply {
                    tool_calls: vec![crate::ToolCall {
                        id: "c".to_owned(),
                        capability: "home.HassTurnOn".to_owned(),
                        input_json: "{}".to_owned(),
                    }],
                    ..ButlerReply::default()
                })
            }
        }

        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let skill = BandedSkill {
            id: "home.HassTurnOn",
            band: Reversibility::Irreversible,
            result: Ok("action_done".to_owned()),
            reads_back: None,
        };
        let mut disclosed = Vec::new();
        let reply = super::run_tool_turn(
            &ActsThenOverclaims,
            &skill,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn on the lights"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut Vec::new(),
            &mut disclosed,
        )
        .expect("the turn answers");

        // The overclaim survives verbatim. Nothing was appended, softened, or replaced.
        assert_eq!(
            reply.text, OVERCLAIM,
            "the butler's words must be its own (ADR 0053)"
        );
        // And the truth is available anyway, beside it.
        assert_eq!(
            disclosed.len(),
            1,
            "the fact lives beside the reply instead"
        );
        assert!(
            !disclosed[0].was_observed(),
            "shown as unconfirmed, contradicting the reply the person can also see"
        );
    }

    #[test]
    fn acting_leaves_an_outcome_holding_the_claim_and_the_observation() {
        // ADR 0053's core rule. The tool claims success; the read-back shows the light
        // still on. BOTH must survive, apart, with no verdict derived from them.
        let store = FakeOutcomes::default();
        let recorded = outcomes_of(
            &BandedSkill {
                id: "home.HassTurnOff",
                band: Reversibility::Irreversible,
                result: Ok("action_done".to_owned()),
                reads_back: Some("kitchen switch: on"),
            },
            &OutcomeSink::unmotivated(&store),
        );

        assert_eq!(recorded.len(), 1, "one action, one outcome");
        assert_eq!(recorded[0].capability(), "home.HassTurnOff");
        assert_eq!(recorded[0].claim(), "action_done");
        assert_eq!(recorded[0].observation(), Some("kitchen switch: on"));
        assert_eq!(recorded[0].reaction(), None, "the person is never asked");
    }

    #[test]
    fn reading_the_world_is_not_an_outcome() {
        // An `Observe` capability changes nothing, so there is nothing to have an
        // outcome about — its result is already evidence (ADR 0053).
        let store = FakeOutcomes::default();
        let recorded = outcomes_of(
            &BandedSkill {
                id: "weather",
                band: Reversibility::Observe,
                result: Ok("72F, sunny".to_owned()),
                reads_back: None,
            },
            &OutcomeSink::unmotivated(&store),
        );
        assert!(recorded.is_empty(), "a read left a record: {recorded:?}");
    }

    #[test]
    fn a_failed_action_is_recorded_too() {
        // A failure is still something that happened, and ADR 0053 says its read-back
        // is the most useful thing about it.
        let store = FakeOutcomes::default();
        let recorded = outcomes_of(
            &BandedSkill {
                id: "home.HassTurnOff",
                band: Reversibility::Irreversible,
                result: Err("no_match_reason=AREA".to_owned()),
                reads_back: Some("kitchen switch: on"),
            },
            &OutcomeSink::unmotivated(&store),
        );
        assert_eq!(recorded.len(), 1);
        assert!(
            recorded[0].claim().contains("no_match_reason=AREA"),
            "the failure is the claim: {:?}",
            recorded[0].claim()
        );
        assert_eq!(recorded[0].observation(), Some("kitchen switch: on"));
    }

    #[test]
    fn an_unverifiable_action_records_no_observation_rather_than_a_blank_one() {
        // Nothing could read the effect back. "We didn't look" must stay distinct from
        // "we looked and saw nothing" (ADR 0053's honest default).
        let store = FakeOutcomes::default();
        let recorded = outcomes_of(
            &BandedSkill {
                id: "mystery.Act",
                band: Reversibility::Irreversible,
                result: Ok("ok".to_owned()),
                reads_back: None,
            },
            &OutcomeSink::unmotivated(&store),
        );
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].observation(), None);
        assert!(!recorded[0].was_observed());
    }

    #[test]
    fn an_action_taken_for_a_reason_traces_back_to_the_belief() {
        let store = FakeOutcomes::default();
        let recorded = outcomes_of(
            &BandedSkill {
                id: "home.HassTurnOff",
                band: Reversibility::Irreversible,
                result: Ok("done".to_owned()),
                reads_back: None,
            },
            &OutcomeSink::motivated_by(&store, Some(BeliefId::new(7))),
        );
        assert_eq!(recorded[0].motivating_belief(), Some(BeliefId::new(7)));
    }

    #[test]
    fn a_broken_outcome_store_never_breaks_a_working_action() {
        // Best-effort recording (ADR 0053), the same rule ADR 0053 set for
        // verification: checking what happened must not break what happened.
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let skill = BandedSkill {
            id: "home.HassTurnOn",
            band: Reversibility::Irreversible,
            result: Ok("turned on".to_owned()),
            reads_back: None,
        };
        let mut activity = Vec::new();
        let reply = super::run_tool_turn(
            &CallThenEcho {
                capability: "home.HassTurnOn",
            },
            &skill,
            &audit,
            &OutcomeSink::unmotivated(&BrokenOutcomes),
            &ids,
            &clock,
            &one_user_turn("turn on the lights"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .expect("the turn survives a failed outcome write");
        assert!(
            reply.text.contains("turned on"),
            "the action still answered from its real result: {}",
            reply.text
        );
        assert!(activity.iter().any(|a| a.contains("Used the")));
    }

    // A butler that calls `capability` until a tool result appears, then answers by
    // echoing that result — so a test can see the model was grounded in it.
    struct CallThenEcho {
        capability: &'static str,
    }
    impl Butler for CallThenEcho {
        fn respond(
            &self,
            _h: &[ChatMessage],
            _p: &[Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            Ok(ButlerReply::default())
        }
        fn take_turn(
            &self,
            conversation: &[crate::TurnMessage],
            _p: &[Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            if let Some(crate::TurnMessage::ToolResult { content, .. }) = conversation
                .iter()
                .rev()
                .find(|m| matches!(m, crate::TurnMessage::ToolResult { .. }))
            {
                return Ok(ButlerReply {
                    text: format!("grounded: {content}"),
                    ..ButlerReply::default()
                });
            }
            Ok(ButlerReply {
                tool_calls: vec![crate::ToolCall {
                    id: "c1".to_owned(),
                    capability: self.capability.to_owned(),
                    input_json: "{}".to_owned(),
                }],
                ..ButlerReply::default()
            })
        }
    }

    fn one_user_turn(text: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage::new(
                MessageId::new(1),
                MessageRole::User,
                text,
                Timestamp::from_unix_millis(0),
            )
            .unwrap(),
        ]
    }

    #[test]
    fn compact_history_summarizes_overflow_and_keeps_the_recent_window() {
        use super::compact_history;
        use crate::ConversationSummaryStore as _;
        struct Store(std::cell::RefCell<Option<crate::ConversationSummary>>);
        impl crate::ConversationSummaryStore for Store {
            fn get(&self) -> Option<crate::ConversationSummary> {
                self.0.borrow().clone()
            }
            fn set(&self, s: crate::ConversationSummary) {
                *self.0.borrow_mut() = Some(s);
            }
        }
        struct SummBut;
        impl Butler for SummBut {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn summarize(&self, _prior: &str, _transcript: &str) -> Result<String, ProposalError> {
                Ok("EARLIER".to_owned())
            }
        }
        let msgs: Vec<ChatMessage> = (0u128..26)
            .map(|i| {
                let role = if i % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Butler
                };
                ChatMessage::new(
                    MessageId::new(i + 1),
                    role,
                    &format!("m{i}"),
                    Timestamp::from_unix_millis(0),
                )
                .unwrap()
            })
            .collect();
        let store = Store(std::cell::RefCell::new(None));

        // A small overflow (20 msgs, window 12 → 8 over) is UNDER the batch threshold:
        // don't pay a summariser call every turn — the overflow rides along verbatim.
        let (recent0, summary0) = compact_history(&SummBut, &store, &msgs[..20], 12);
        assert_eq!(
            recent0.len(),
            20,
            "under the batch threshold, nothing is folded"
        );
        assert!(summary0.is_none());
        assert!(store.get().is_none(), "no summary written yet");

        // 24 messages, window 12 → 12 over = a full batch: fold the oldest 12 into the
        // summary, keep the last 12 verbatim.
        let (recent, summary) = compact_history(&SummBut, &store, &msgs[..24], 12);
        assert_eq!(recent.len(), 12);
        assert_eq!(recent[0].text(), "m12");
        assert_eq!(summary.as_deref(), Some("EARLIER"));
        assert_eq!(store.get().unwrap().covered, 12);

        // Two more messages (26 total) is UNDER the next batch: don't re-summarise,
        // just carry the extra overflow verbatim on top of the covered summary.
        let (recent2, summary2) = compact_history(&SummBut, &store, &msgs, 12);
        assert_eq!(
            recent2.len(),
            14,
            "12 covered → the remaining 14 ride verbatim"
        );
        assert_eq!(recent2[0].text(), "m12");
        assert_eq!(summary2.as_deref(), Some("EARLIER"));
        assert_eq!(
            store.get().unwrap().covered,
            12,
            "no re-summarise under the batch"
        );

        // A short history (<= window) needs no summary.
        let (recent3, summary3) = compact_history(&SummBut, &store, &msgs[..5], 12);
        assert_eq!(recent3.len(), 5);
        assert!(summary3.is_none());
    }

    #[test]
    fn compact_history_caps_verbatim_when_the_summariser_fails() {
        use super::compact_history;
        use crate::ConversationSummaryStore as _;
        struct Store(std::cell::RefCell<Option<crate::ConversationSummary>>);
        impl crate::ConversationSummaryStore for Store {
            fn get(&self) -> Option<crate::ConversationSummary> {
                self.0.borrow().clone()
            }
            fn set(&self, s: crate::ConversationSummary) {
                *self.0.borrow_mut() = Some(s);
            }
        }
        // A summariser that always fails (e.g. the model timed out) — coverage never
        // advances, so without a cap the whole backlog would ride verbatim.
        struct FailSumm;
        impl Butler for FailSumm {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn summarize(&self, _p: &str, _t: &str) -> Result<String, ProposalError> {
                Err(ProposalError::Unavailable("timeout".to_owned()))
            }
        }
        let msgs: Vec<ChatMessage> = (0u128..60)
            .map(|i| {
                ChatMessage::new(
                    MessageId::new(i + 1),
                    MessageRole::User,
                    &format!("m{i}"),
                    Timestamp::from_unix_millis(0),
                )
                .unwrap()
            })
            .collect();
        let store = Store(std::cell::RefCell::new(None));

        // 60 messages, window 12, batch 12 → even though the summariser fails, verbatim
        // is capped at window + batch = 24 (the oldest are dropped, not sent).
        let (recent, summary) = compact_history(&FailSumm, &store, &msgs, 12);
        assert_eq!(
            recent.len(),
            24,
            "verbatim must stay bounded, got {recent:?}"
        );
        assert_eq!(recent[0].text(), "m36"); // 60 - 24
        assert!(summary.is_none());
        assert!(store.get().is_none(), "a failed summary is not stored");
    }

    #[test]
    fn run_tool_turn_runs_a_cleared_tool_and_answers_from_its_result() {
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let skill = FixedSkill {
            id: "home.HassTurnOn",
            result: Ok("turned on".to_owned()),
        };
        let mut activity = Vec::new();
        let mut steps = Vec::new();
        let reply = super::run_tool_turn(
            &CallThenEcho {
                capability: "home.HassTurnOn",
            },
            &skill,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn on the lights"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |s| steps.push(s),
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .unwrap();
        // The model answered grounded in the real result, and the tool actually ran.
        assert_eq!(reply.text, "grounded: turned on");
        assert!(
            activity
                .iter()
                .any(|a| a.contains("Used the home.HassTurnOn"))
        );
        assert!(steps.iter().any(|s| s.status == super::StepStatus::Done));
    }

    #[test]
    fn run_tool_turn_feeds_a_failure_back_so_the_model_answers_from_it() {
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let skill = FixedSkill {
            id: "home.HassLightSet",
            result: Err("no lights match 'kitchen'".to_owned()),
        };
        let mut activity = Vec::new();
        let reply = super::run_tool_turn(
            &CallThenEcho {
                capability: "home.HassLightSet",
            },
            &skill,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn on the kitchen lights"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .unwrap();
        // The model's answer carries the REAL error (no fabricated success).
        assert!(
            reply.text.contains("no lights match 'kitchen'"),
            "answer should be grounded in the failure, got: {}",
            reply.text
        );
        assert!(activity.iter().any(|a| a.contains("failed")));
    }

    #[test]
    fn run_tool_turn_caps_failed_executions_at_two() {
        // A butler that never stops asking for the failing tool.
        struct AlwaysCalls;
        impl Butler for AlwaysCalls {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn take_turn(
                &self,
                conversation: &[crate::TurnMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                // A DISTINCT input each round (so the repeated-call guard doesn't
                // short-circuit it) — this exercises the failure cap.
                Ok(ButlerReply {
                    tool_calls: vec![crate::ToolCall {
                        id: "c".to_owned(),
                        capability: "home.HassLightSet".to_owned(),
                        input_json: format!("{{\"n\":{}}}", conversation.len()),
                    }],
                    ..ButlerReply::default()
                })
            }
        }
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let skill = FixedSkill {
            id: "home.HassLightSet",
            result: Err("boom".to_owned()),
        };
        let mut activity = Vec::new();
        let _ = super::run_tool_turn(
            &AlwaysCalls,
            &skill,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("do it"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .unwrap();
        // Executions stop after two failures, not the full six rounds.
        let failed = activity.iter().filter(|a| a.contains("failed")).count();
        assert_eq!(failed, 2, "got: {activity:?}");
    }

    #[test]
    fn run_tool_turn_does_not_loop_the_same_read_only_call() {
        // A butler that keeps calling the SAME read-only tool with the same input —
        // it succeeds every time, so only the repeated-call guard stops the loop.
        struct RepeatsGetContext;
        impl Butler for RepeatsGetContext {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn take_turn(
                &self,
                _conversation: &[crate::TurnMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    tool_calls: vec![crate::ToolCall {
                        id: "c".to_owned(),
                        capability: "home.GetLiveContext".to_owned(),
                        input_json: "{}".to_owned(),
                    }],
                    ..ButlerReply::default()
                })
            }
        }
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let skill = FixedSkill {
            id: "home.GetLiveContext",
            result: Ok("the lights are off".to_owned()),
        };
        let mut activity = Vec::new();
        let _ = super::run_tool_turn(
            &RepeatsGetContext,
            &skill,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("what's on?"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .unwrap();
        // The read-only tool ran exactly ONCE despite being requested every round.
        let used = activity
            .iter()
            .filter(|a| a.contains("Used the home.GetLiveContext"))
            .count();
        assert_eq!(used, 1, "got: {activity:?}");
    }

    #[test]
    fn run_tool_turn_retries_an_empty_completion_then_acts() {
        // A slow local model sometimes whiffs: an empty first completion (no tool
        // call, no text). The turn must retry rather than surface the whiff, so the
        // second (non-deterministic) attempt gets to call the tool.
        struct EmptyThenCalls {
            calls: std::cell::Cell<usize>,
        }
        impl Butler for EmptyThenCalls {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn take_turn(
                &self,
                conversation: &[crate::TurnMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                // Once a tool result is in hand, answer from it.
                if let Some(crate::TurnMessage::ToolResult { content, .. }) = conversation
                    .iter()
                    .rev()
                    .find(|m| matches!(m, crate::TurnMessage::ToolResult { .. }))
                {
                    return Ok(ButlerReply {
                        text: format!("grounded: {content}"),
                        ..ButlerReply::default()
                    });
                }
                let n = self.calls.get();
                self.calls.set(n + 1);
                // First attempt: a complete whiff (no tool call, no text).
                if n == 0 {
                    return Ok(ButlerReply::default());
                }
                // Retry: the model commits to the action tool.
                Ok(ButlerReply {
                    tool_calls: vec![crate::ToolCall {
                        id: "c1".to_owned(),
                        capability: "home.HassTurnOn".to_owned(),
                        input_json: "{}".to_owned(),
                    }],
                    ..ButlerReply::default()
                })
            }
        }

        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let skill = FixedSkill {
            id: "home.HassTurnOn",
            result: Ok("turned on".to_owned()),
        };
        let mut activity = Vec::new();
        let reply = super::run_tool_turn(
            &EmptyThenCalls {
                calls: std::cell::Cell::new(0),
            },
            &skill,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn on the kitchen light"),
            &[],
            &ButlerContext::default(),
            3,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .unwrap();
        // The retry recovered: the tool ran and the answer is grounded in it —
        // not the empty whiff that would have become the canned fallback.
        assert!(
            activity
                .iter()
                .any(|a| a.contains("Used the home.HassTurnOn")),
            "expected the retried turn to run the tool, got: {activity:?}"
        );
        assert!(reply.text.contains("turned on"), "got: {}", reply.text);
    }

    #[test]
    fn run_tool_turn_reads_state_then_acts_in_a_later_round() {
        // The model often checks state before a command ("turn off the light" → read,
        // see it's on, then turn it off). The turn must give it a round to ACT after
        // the read, not force an answer the moment the read succeeds.
        struct ReadThenAct;
        impl Butler for ReadThenAct {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
            fn take_turn(
                &self,
                conversation: &[crate::TurnMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                let results = conversation
                    .iter()
                    .filter(|m| matches!(m, crate::TurnMessage::ToolResult { .. }))
                    .count();
                let call = |cap: &str| {
                    Ok(ButlerReply {
                        tool_calls: vec![crate::ToolCall {
                            id: "c".to_owned(),
                            capability: cap.to_owned(),
                            input_json: "{}".to_owned(),
                        }],
                        ..ButlerReply::default()
                    })
                };
                match results {
                    // First: read the live state.
                    0 => call("home.GetLiveContext"),
                    // Then: act on it.
                    1 => call("home.HassLightSet"),
                    // Finally: answer.
                    _ => Ok(ButlerReply {
                        text: "The kitchen light is now off.".to_owned(),
                        ..ButlerReply::default()
                    }),
                }
            }
        }

        // Both the read and the action must be runnable.
        struct TwoSkills;
        impl CapabilityRunner for TwoSkills {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                ["home.GetLiveContext", "home.HassLightSet"]
                    .into_iter()
                    .map(|id| endora_capabilities::CapabilitySpec {
                        id: id.to_owned(),
                        description: "does a thing".to_owned(),
                        configured: true,
                        autonomous: true,
                        input_schema: None,
                        reversibility: endora_kernel::Reversibility::Observe,
                    })
                    .collect()
            }
            fn run(&self, id: &str, _input: &str) -> Result<String, String> {
                match id {
                    "home.GetLiveContext" => Ok("kitchen light: on".to_owned()),
                    _ => Ok("set".to_owned()),
                }
            }
        }

        let (ids, clock, audit) = (SeqIds::default(), FixedClock(0), FakeAudit::default());
        let mut activity = Vec::new();
        let reply = super::run_tool_turn(
            &ReadThenAct,
            &TwoSkills,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("turn off the kitchen light"),
            &[],
            &ButlerContext::default(),
            3,
            &mut |_| {},
            &mut |_token: &str| {},
            &mut activity,
            &mut Vec::new(),
        )
        .unwrap();
        // The action ran in a round AFTER the read — not skipped by an early answer.
        assert!(
            activity
                .iter()
                .any(|a| a.contains("Used the home.GetLiveContext")),
            "expected the read to run, got: {activity:?}"
        );
        assert!(
            activity
                .iter()
                .any(|a| a.contains("Used the home.HassLightSet")),
            "expected the action to run after the read, got: {activity:?}"
        );
        assert!(reply.text.contains("now off"), "got: {}", reply.text);
    }

    #[test]
    fn an_unconfigured_skill_is_not_run_and_the_first_reply_stands() {
        use super::send_to_butler;

        struct ToolButler;
        impl Butler for ToolButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "I can't check flights yet.".to_owned(),
                    capability_use: Some(endora_capabilities::CapabilityUse {
                        capability: "flights".to_owned(),
                        input_json: "{}".to_owned(),
                    }),
                    ..ButlerReply::default()
                })
            }
        }

        // "flights" is present but must confirm (not autonomous): the policy layer
        // must refuse to auto-run it.
        struct GatedSkill;
        impl CapabilityRunner for GatedSkill {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                vec![endora_capabilities::CapabilitySpec {
                    id: "flights".to_owned(),
                    description: "find flights".to_owned(),
                    configured: false,
                    autonomous: false,
                    input_schema: None,
                    reversibility: endora_kernel::Reversibility::Observe,
                }]
            }
            fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
                panic!("a gated skill must never be auto-run");
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let (reply, activity) = send_to_butler(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &GatedSkill,
            &ToolButler,
            &FakeAudit::default(),
            &ids,
            &clock,
            &ButlerContext::default(),
            "book me a flight",
        )
        .unwrap();

        // The butler's own reply stands; nothing was run.
        assert!(reply.text().contains("can't check flights"));
        assert!(activity.iter().all(|a| !a.contains("Used")));
    }

    #[test]
    fn a_blocked_consequential_decision_is_recorded_in_the_audit_trail() {
        use super::send_to_butler;
        use endora_kernel::Decision;

        // A butler that asks for a configured-but-blocked skill.
        struct BookingButler;
        impl Butler for BookingButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "I shouldn't book that on my own.".to_owned(),
                    capability_use: Some(endora_capabilities::CapabilityUse {
                        capability: "booking".to_owned(),
                        input_json: "{}".to_owned(),
                    }),
                    ..ButlerReply::default()
                })
            }
        }

        // Configured and real, but policy BLOCKS it (irreversible, not opened).
        struct BlockedSkill;
        impl CapabilityRunner for BlockedSkill {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                vec![endora_capabilities::CapabilitySpec {
                    id: "booking".to_owned(),
                    description: "book travel".to_owned(),
                    configured: true,
                    autonomous: false,
                    input_schema: None,
                    reversibility: endora_kernel::Reversibility::Observe,
                }]
            }
            fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
                panic!("a blocked skill must never run");
            }
            fn decision(&self, _id: &str) -> Option<Decision> {
                Some(Decision::Block)
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let audit = FakeAudit::default();
        let _ = send_to_butler(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &BlockedSkill,
            &BookingButler,
            &audit,
            &ids,
            &clock,
            &ButlerContext::default(),
            "book me a first-class flight",
        )
        .unwrap();

        // Policy's decision to block a consequential action is recorded, for
        // accountability (ADRs 0005/0024) — not left only in the model's prose.
        let records = audit.recent(10).unwrap();
        assert!(
            records
                .iter()
                .any(|r| r.summary().contains("blocked") && r.summary().contains("booking")),
            "expected an audit record of the blocked booking, got: {:?}",
            records
                .iter()
                .map(|r| r.summary().to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_ladder_escalates_to_the_deep_model_when_the_local_comes_up_empty() {
        use super::send_to_butler_streaming;
        use crate::ports::DeepAsker;

        // The local rung: answers with nothing usable (an empty reply).
        struct EmptyButler;
        impl Butler for EmptyButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[crate::Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
        }
        // The deeper rung: answers.
        struct DeepModel;
        impl DeepAsker for DeepModel {
            fn ask(&self, q: &str) -> Option<String> {
                assert!(q.contains("quantum"), "the person's question is escalated");
                Some("A qubit can hold superposition.".to_owned())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let deep = DeepModel;
        let mut streamed = String::new();
        let (msg, activity) = send_to_butler_streaming(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &NoCapabilities,
            &EmptyButler,
            &FakeAudit::default(),
            Some(&deep),
            None,
            &ids,
            &clock,
            &ButlerContext::default(),
            "explain quantum superposition",
            &mut |t| streamed.push_str(t),
            &mut |_| {},
            &mut Vec::new(),
        )
        .unwrap();

        // The deep answer becomes the reply, is streamed to the person, and the
        // escalation is recorded — instead of the honest "I'm not sure" fallback.
        assert!(msg.text().contains("superposition"));
        assert!(
            streamed.contains("superposition"),
            "the escalated answer is streamed"
        );
        assert!(activity.iter().any(|a| a.contains("deep model")));
    }

    #[test]
    fn the_ladder_leaves_a_good_local_answer_alone() {
        use super::send_to_butler_streaming;
        use crate::ports::DeepAsker;

        struct GoodButler;
        impl Butler for GoodButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[crate::Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "Hello — glad to help.".to_owned(),
                    ..ButlerReply::default()
                })
            }
        }
        // A deeper model that must never be consulted when the local rung answered.
        struct PanicDeep;
        impl DeepAsker for PanicDeep {
            fn ask(&self, _q: &str) -> Option<String> {
                panic!("must not escalate when the local model answered");
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let deep = PanicDeep;
        let (msg, activity) = send_to_butler_streaming(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &NoCapabilities,
            &GoodButler,
            &FakeAudit::default(),
            Some(&deep),
            None,
            &ids,
            &clock,
            &ButlerContext::default(),
            "hi",
            &mut |_| {},
            &mut |_| {},
            &mut Vec::new(),
        )
        .unwrap();

        assert!(msg.text().contains("glad to help"));
        assert!(activity.iter().all(|a| !a.contains("deep model")));
    }

    #[test]
    fn datetime_formats_a_known_timestamp() {
        use super::format_datetime_utc;
        // 1_700_000_000_000 ms = Tue 2023-11-14 22:13 UTC.
        assert_eq!(
            format_datetime_utc(1_700_000_000_000),
            "Tuesday, 2023-11-14 22:13 UTC"
        );
        // Unix epoch itself.
        assert_eq!(format_datetime_utc(0), "Thursday, 1970-01-01 00:00 UTC");
    }

    #[test]
    fn progress_labels_read_as_present_tense_steps() {
        use super::progress_label;
        assert_eq!(progress_label("weather"), "Checking the weather");
        assert_eq!(progress_label("safety_alerts"), "Checking safety alerts");
        // An unknown skill still gets a sensible, generic label.
        assert_eq!(progress_label("calendar"), "Using the calendar skill");
    }

    #[test]
    fn brief_is_due_only_at_its_hour_once_per_day() {
        use endora_scheduling::BriefSchedule;
        let at = |d: i64, h: i64| Timestamp::from_unix_millis(d * 86_400_000 + h * 3_600_000);
        let day = 20_000; // a realistic day, so "since epoch" is far in the past
        let s = BriefSchedule {
            enabled: true,
            hour_utc: 12,
            last_at: Timestamp::from_unix_millis(0),
        };
        assert!(!s.is_due(at(day, 11))); // wrong hour
        assert!(s.is_due(at(day, 12))); // right hour, long since last
        // Just fired today ⇒ not due again the same day...
        let fired = BriefSchedule {
            last_at: at(day, 12),
            ..s
        };
        assert!(!fired.is_due(at(day, 12)));
        // ...but due again the next day.
        assert!(fired.is_due(at(day + 1, 12)));
        // Disabled is never due.
        let off = BriefSchedule {
            enabled: false,
            ..s
        };
        assert!(!off.is_due(at(day, 12)));
    }

    #[test]
    fn daily_brief_is_written_from_what_the_butler_actually_gathered() {
        use super::daily_brief;

        struct BriefSkills;
        impl CapabilityRunner for BriefSkills {
            fn available(&self) -> Vec<endora_capabilities::CapabilitySpec> {
                vec![endora_capabilities::CapabilitySpec {
                    id: "weather".to_owned(),
                    description: "w".to_owned(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: endora_kernel::Reversibility::Observe,
                }]
            }
            fn run(&self, id: &str, _input_json: &str) -> Result<String, String> {
                assert_eq!(id, "weather");
                Ok("clear, 25C".to_owned())
            }
        }

        // The butler reaches for a skill, the result comes back as a turn in the same
        // conversation, and it writes the brief from that — one pass, no synthesis
        // hand-off (ADR 0053).
        struct BriefButler;
        impl Butler for BriefButler {
            fn respond(
                &self,
                history: &[ChatMessage],
                _preferences: &[Preference],
                _context: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                let Some(result) = last_skill_result(history) else {
                    // Nothing gathered yet — reach for the weather.
                    return Ok(ButlerReply {
                        capability_use: Some(endora_capabilities::CapabilityUse {
                            capability: "weather".to_owned(),
                            input_json: "{\"location\":\"10001\"}".to_owned(),
                        }),
                        ..ButlerReply::default()
                    });
                };
                assert!(
                    result.contains("clear, 25C"),
                    "the brief must be written from the real gathered result"
                );
                Ok(ButlerReply {
                    text: "Good morning! It's clear and 25C where you are.".to_owned(),
                    ..ButlerReply::default()
                })
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let audit = FakeAudit::default();
        let ctx = ButlerContext::default();

        let (msg, activity) = daily_brief(
            &store,
            &store,
            &FakeOutcomes::default(),
            &crate::usecases::NoTrouble,
            &BriefSkills,
            &BriefButler,
            &audit,
            &ids,
            &clock,
            &ctx,
        )
        .unwrap()
        .unwrap();
        // Natural prose carrying the real fact — no label dump.
        assert!(msg.text().contains("Good morning"));
        assert!(msg.text().contains("25C"));
        assert!(!msg.text().contains("Weather —"));
        assert!(activity.iter().any(|a| a.contains("weather")));

        // No floor (ADR 0053): if the butler is unavailable there is NO brief. A
        // scripted one would be Endora claiming to have thought about the day.
        struct DeadButler;
        impl Butler for DeadButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Err(ProposalError::Unavailable("down".to_owned()))
            }
        }
        assert!(
            daily_brief(
                &store,
                &store,
                &FakeOutcomes::default(),
                &crate::usecases::NoTrouble,
                &BriefSkills,
                &DeadButler,
                &audit,
                &ids,
                &clock,
                &ctx,
            )
            .unwrap()
            .is_none(),
            "a brief is never fabricated when the butler can't think"
        );
    }

    #[test]
    fn the_butler_speaks_only_when_it_has_something_and_the_budget_allows() {
        use super::{chat_history, consider_reaching_out, set_checkin_schedule};
        use std::cell::RefCell;
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let ctx = ButlerContext::default();
        let audit = FakeAudit {
            records: RefCell::new(Vec::new()),
        };

        /// A butler that has something to say.
        struct HasSomething;
        impl Butler for HasSomething {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "That storm you mentioned is due tonight.".to_owned(),
                    ..ButlerReply::default()
                })
            }
        }

        let reach = |clock_ms: i64, butler: &dyn Butler| {
            consider_reaching_out(
                &store,
                &store,
                &store,
                &FakeOutcomes::default(),
                &NoCapabilities,
                butler,
                &audit,
                &ids,
                &FixedClock(clock_ms),
                &ctx,
            )
            .unwrap()
        };

        // Off by default: the butler never speaks uninvited until asked to.
        assert!(reach(1_000, &HasSomething).is_none());

        let sched = set_checkin_schedule(&store, &FixedClock(1_000), true, 60_000).unwrap();
        assert_eq!(sched.next_at.unix_millis(), 61_000);

        // Inside the budget window: not yet allowed, however much it has to say.
        assert!(reach(30_000, &HasSomething).is_none());

        // Budget available AND something to say → it speaks.
        let (msg, activity) = reach(61_000, &HasSomething).expect("should have reached out");
        assert_eq!(msg.role(), MessageRole::Butler);
        assert!(msg.text().contains("storm"));
        assert_eq!(chat_history(&store).unwrap().len(), 1);
        assert!(activity.iter().any(|a| a.contains("worth raising")));
        // The budget advanced, so it cannot immediately speak again.
        assert!(reach(61_500, &HasSomething).is_none());
    }

    #[test]
    fn having_nothing_to_say_posts_nothing_and_still_spends_the_budget() {
        use super::{chat_history, consider_reaching_out, set_checkin_schedule};
        use std::cell::RefCell;
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let ctx = ButlerContext::default();
        let audit = FakeAudit {
            records: RefCell::new(Vec::new()),
        };

        /// A butler with nothing worth raising — the common, correct case.
        struct Quiet;
        impl Butler for Quiet {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply::default())
            }
        }

        set_checkin_schedule(&store, &FixedClock(1_000), true, 60_000).unwrap();
        let out = consider_reaching_out(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &NoCapabilities,
            &Quiet,
            &audit,
            &ids,
            &FixedClock(61_000),
            &ctx,
        )
        .unwrap();
        assert!(out.is_none(), "silence must post nothing");
        assert!(chat_history(&store).unwrap().is_empty());

        // Crucially the budget is still spent: a "nothing to say" must not become a
        // retry loop that asks again every tick until it talks itself into speaking.
        assert_eq!(
            CheckinRepository::get(&store)
                .unwrap()
                .unwrap()
                .next_at
                .unix_millis(),
            121_000
        );
    }

    #[test]
    fn the_butler_does_not_talk_over_someone_who_just_spoke() {
        use super::{consider_reaching_out, post_user_message, set_checkin_schedule};
        use std::cell::RefCell;
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let ctx = ButlerContext::default();
        let audit = FakeAudit {
            records: RefCell::new(Vec::new()),
        };

        struct HasSomething;
        impl Butler for HasSomething {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "Something worth saying.".to_owned(),
                    ..ButlerReply::default()
                })
            }
        }

        set_checkin_schedule(&store, &FixedClock(1_000), true, 60_000).unwrap();
        // The person spoke a minute ago — they are present and can simply ask.
        post_user_message(&store, &ids, &FixedClock(61_000), "hey").unwrap();
        assert!(
            consider_reaching_out(
                &store,
                &store,
                &store,
                &FakeOutcomes::default(),
                &NoCapabilities,
                &HasSomething,
                &audit,
                &ids,
                &FixedClock(61_000),
                &ctx,
            )
            .unwrap()
            .is_none(),
            "must not reach out on top of an active conversation"
        );

        // An hour after they last spoke, it may.
        assert!(
            consider_reaching_out(
                &store,
                &store,
                &store,
                &FakeOutcomes::default(),
                &NoCapabilities,
                &HasSomething,
                &audit,
                &ids,
                &FixedClock(61_000 + 60 * 60 * 1_000),
                &ctx,
            )
            .unwrap()
            .is_some()
        );
    }

    #[test]
    fn butler_forms_understanding_that_is_reviewable_and_correctable() {
        use super::{affirm_belief, correct_belief, send_to_butler, understanding};
        use crate::{BeliefKind, Confidence};

        struct BeliefButler;
        impl Butler for BeliefButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[crate::Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "Travel seems to matter to you.".to_owned(),
                    beliefs: vec![crate::ports::FormedBelief {
                        statement: "wants energy to travel".to_owned(),
                        kind: BeliefKind::Intent,
                        confidence: Confidence::Low,
                        evidence: "mentioned wanting to travel".to_owned(),
                    }],
                    ..ButlerReply::default()
                })
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        send_to_butler(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &NoCapabilities,
            &BeliefButler,
            &FakeAudit::default(),
            &ids,
            &clock,
            &ButlerContext::default(),
            "I'd love to see more of the world",
        )
        .unwrap();

        // The butler formed understanding, stored directly (no confirm step).
        let u = understanding(&store, &FixedClock(1_000)).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].statement(), "wants energy to travel");
        assert_eq!(u[0].confidence(), Confidence::Low);

        // Affirming raises confidence; correcting drops it from understanding.
        let id = u[0].id();
        affirm_belief(&store, &clock, id).unwrap();
        assert_eq!(
            understanding(&store, &FixedClock(1_000)).unwrap()[0].confidence(),
            Confidence::Medium
        );
        correct_belief(&store, id).unwrap();
        assert!(
            understanding(&store, &FixedClock(1_000))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_nightly_loop_reflects_and_leaves_an_overnight_note_when_due() {
        use super::{run_due_nightly_loop, understanding};
        use crate::{BeliefKind, Confidence};
        use endora_scheduling::{NightlyLoopSchedule, NightlyLoopScheduleRepository};
        use std::cell::RefCell;

        // Overnight, the butler reflects: forms a belief and writes a short note.
        struct ReflectiveButler;
        impl Butler for ReflectiveButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[crate::Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: "Evening — I've noticed sleep is on your mind. I'll keep an eye on it."
                        .to_owned(),
                    beliefs: vec![crate::ports::FormedBelief {
                        statement: "wants more consistent sleep".to_owned(),
                        kind: BeliefKind::Intent,
                        confidence: Confidence::Low,
                        evidence: "mentioned being tired several times".to_owned(),
                    }],
                    ..ButlerReply::default()
                })
            }
        }

        // A due schedule: hour 3, clock at 27h (UTC hour 3, a day on from last_at 0).
        struct DueSchedule(RefCell<NightlyLoopSchedule>);
        impl NightlyLoopScheduleRepository for DueSchedule {
            fn get(&self) -> Result<Option<NightlyLoopSchedule>, RepositoryError> {
                Ok(Some(*self.0.borrow()))
            }
            fn set(&self, s: &NightlyLoopSchedule) -> Result<(), RepositoryError> {
                *self.0.borrow_mut() = *s;
                Ok(())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(27 * 3_600_000);
        let sched = DueSchedule(RefCell::new(NightlyLoopSchedule {
            enabled: true,
            hour_utc: 3,
            last_at: Timestamp::from_unix_millis(0),
        }));

        let audit = FakeAudit {
            records: RefCell::new(Vec::new()),
        };
        let (msg, activity) = run_due_nightly_loop(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &FakeIntentions::default(),
            &sched,
            &NoCapabilities,
            &ReflectiveButler,
            &audit,
            &ids,
            &clock,
            &ButlerContext::default(),
        )
        .unwrap()
        .expect("the loop runs when due");

        // Surfaced an overnight note...
        assert!(msg.text().contains("sleep"));
        assert!(activity.iter().any(|a| a.contains("overnight note")));
        // ...reflected (formed and saved a belief)...
        let u = understanding(&store, &FixedClock(1_000)).unwrap();
        assert!(u.iter().any(|b| b.statement().contains("sleep")));
        assert!(activity.iter().any(|a| a.contains("Learned")));
        // ...and marked itself fired, so it won't run again tonight.
        assert!(!sched.0.borrow().is_due(clock.now()));
    }

    /// A due nightly schedule at UTC hour 3 — the setup every intention test needs.
    fn due_nightly() -> impl endora_scheduling::NightlyLoopScheduleRepository {
        use endora_scheduling::{NightlyLoopSchedule, NightlyLoopScheduleRepository};
        struct Due(RefCell<NightlyLoopSchedule>);
        impl NightlyLoopScheduleRepository for Due {
            fn get(&self) -> Result<Option<NightlyLoopSchedule>, RepositoryError> {
                Ok(Some(*self.0.borrow()))
            }
            fn set(&self, s: &NightlyLoopSchedule) -> Result<(), RepositoryError> {
                *self.0.borrow_mut() = *s;
                Ok(())
            }
        }
        Due(RefCell::new(NightlyLoopSchedule {
            enabled: true,
            hour_utc: 3,
            last_at: Timestamp::from_unix_millis(0),
        }))
    }

    /// A butler that reports back whatever it was told, so a test can see exactly
    /// what instruction the loop handed it, and leaves a note.
    struct EchoesItsInstruction {
        seen: RefCell<String>,
    }
    impl Butler for EchoesItsInstruction {
        fn respond(
            &self,
            history: &[ChatMessage],
            _p: &[crate::Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            let last = history.last().map(ChatMessage::text).unwrap_or_default();
            self.seen.borrow_mut().push_str(last);
            Ok(ButlerReply {
                text: "Tonight I read about room temperature.".to_owned(),
                ..ButlerReply::default()
            })
        }
    }

    /// Runs one night and hands back the instruction the butler was given.
    fn one_night(
        beliefs: &FakeStore,
        intentions: &FakeIntentions,
        clock: &FixedClock,
    ) -> (String, Vec<String>) {
        let butler = EchoesItsInstruction {
            seen: RefCell::new(String::new()),
        };
        let (_, activity) = super::run_due_nightly_loop(
            beliefs,
            beliefs,
            beliefs,
            &FakeOutcomes::default(),
            intentions,
            &due_nightly(),
            &NoCapabilities,
            &butler,
            &FakeAudit::default(),
            &SeqIds::default(),
            clock,
            &ButlerContext::default(),
        )
        .unwrap()
        .expect("the loop runs when due");
        let seen = butler.seen.borrow().clone();
        (seen, activity)
    }

    /// A store holding one belief strong enough to be taken up.
    fn store_with_a_belief() -> FakeStore {
        use crate::{BeliefKind, Confidence};
        let store = FakeStore::default();
        BeliefRepository::save(
            &store,
            &Belief::new(
                BeliefId::new(7),
                "wants to sleep better",
                BeliefKind::Intent,
                Confidence::High,
                "said so twice",
                Timestamp::from_unix_millis(0),
            )
            .unwrap(),
        )
        .unwrap();
        store
    }

    #[test]
    fn the_nightly_loop_takes_up_one_thing_and_says_what_it_came_from() {
        // ADR 0052: Endora forms the intention itself, and it traces to a belief.
        let store = store_with_a_belief();
        let intentions = FakeIntentions::default();
        let (instruction, activity) = one_night(&store, &intentions, &FixedClock(27 * 3_600_000));

        assert!(
            instruction.contains("taking up") && instruction.contains("sleep better"),
            "night one starts the thread: {instruction}"
        );
        assert!(activity.iter().any(|a| a.contains("Started looking into")));

        let held = intentions.list().unwrap();
        assert_eq!(held.len(), 1, "one thing at a time");
        assert_eq!(held[0].motivating_belief(), BeliefId::new(7));
        assert_eq!(
            held[0].steps_taken(),
            1,
            "tonight counted as a night's work"
        );
        assert_eq!(held[0].note(), "Tonight I read about room temperature.");
    }

    #[test]
    fn the_next_night_continues_the_thread_instead_of_starting_over() {
        // The whole point of ADR 0052. Night two must be handed night one's findings
        // and told to carry on — not to take the same thing up afresh.
        let store = store_with_a_belief();
        let intentions = FakeIntentions::default();
        one_night(&store, &intentions, &FixedClock(27 * 3_600_000));
        let (instruction, _) = one_night(&store, &intentions, &FixedClock(51 * 3_600_000));

        assert!(
            instruction.contains("already looking into"),
            "night two resumes: {instruction}"
        );
        assert!(
            instruction.contains("night 2"),
            "it knows how far in it is: {instruction}"
        );
        assert!(
            instruction.contains("Tonight I read about room temperature."),
            "it is handed its own findings back: {instruction}"
        );
        assert!(
            !instruction.contains("taking up"),
            "it must not start over: {instruction}"
        );

        let held = intentions.list().unwrap();
        assert_eq!(held.len(), 1, "still one thread, not two");
        assert_eq!(held[0].steps_taken(), 2);
    }

    #[test]
    fn a_second_belief_does_not_start_a_second_thread() {
        // The cursor-not-queue rule (ADR 0052). Even with something new and strong to
        // chase, Endora finishes what it is on.
        use crate::{BeliefKind, Confidence};
        let store = store_with_a_belief();
        let intentions = FakeIntentions::default();
        one_night(&store, &intentions, &FixedClock(27 * 3_600_000));

        BeliefRepository::save(
            &store,
            &Belief::new(
                BeliefId::new(9),
                "wants to travel more",
                BeliefKind::Intent,
                Confidence::High,
                "brought it up today",
                Timestamp::from_unix_millis(30 * 3_600_000),
            )
            .unwrap(),
        )
        .unwrap();
        one_night(&store, &intentions, &FixedClock(51 * 3_600_000));

        assert_eq!(
            intentions.list().unwrap().len(),
            1,
            "a shinier belief must not open a second thread"
        );
    }

    #[test]
    fn a_spent_thread_is_dropped_so_something_else_can_be_taken_up() {
        // Nothing rots: the step budget frees the one slot on its own, visibly.
        let store = store_with_a_belief();
        let intentions = FakeIntentions::default();
        // Seven nights in, and spent.
        let mut spent = crate::Intention::form(
            crate::IntentionId::new(1),
            "an exhausted thread",
            BeliefId::new(7),
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        for _ in 0..7 {
            spent.progress("looked into it", Timestamp::from_unix_millis(0));
        }
        intentions.save(&spent).unwrap();

        let (instruction, activity) = one_night(&store, &intentions, &FixedClock(27 * 3_600_000));

        assert!(
            activity
                .iter()
                .any(|a| a.contains("Stopped working on something")),
            "it says it gave up, in the trail: {activity:?}"
        );
        assert!(
            instruction.contains("taking up"),
            "the freed slot is used for something new: {instruction}"
        );
        assert_eq!(
            intentions
                .list()
                .unwrap()
                .iter()
                .filter(|i| i.is_active())
                .count(),
            1,
            "still exactly one active"
        );
    }

    #[test]
    fn with_nothing_understood_yet_the_loop_still_reflects_without_a_thread() {
        // A new Endora has no belief strong enough to pursue. It must still run.
        let store = FakeStore::default();
        let intentions = FakeIntentions::default();
        let (instruction, _) = one_night(&store, &intentions, &FixedClock(27 * 3_600_000));

        assert!(
            !instruction.contains("taking up") && !instruction.contains("already looking into"),
            "no thread invented from nothing: {instruction}"
        );
        assert!(intentions.list().unwrap().is_empty());
    }

    #[test]
    fn the_nightly_loop_stays_quiet_when_not_due() {
        use super::run_due_nightly_loop;
        use endora_scheduling::{NightlyLoopSchedule, NightlyLoopScheduleRepository};

        struct NeverButler;
        impl Butler for NeverButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[crate::Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                panic!("the butler must not be consulted when the loop isn't due");
            }
        }
        // Off by default: never due, butler never called.
        struct OffSchedule;
        impl NightlyLoopScheduleRepository for OffSchedule {
            fn get(&self) -> Result<Option<NightlyLoopSchedule>, RepositoryError> {
                Ok(Some(NightlyLoopSchedule::disabled_default()))
            }
            fn set(&self, _s: &NightlyLoopSchedule) -> Result<(), RepositoryError> {
                Ok(())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(27 * 3_600_000);
        let audit = FakeAudit {
            records: std::cell::RefCell::new(Vec::new()),
        };
        let out = run_due_nightly_loop(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &FakeIntentions::default(),
            &OffSchedule,
            &NoCapabilities,
            &NeverButler,
            &audit,
            &ids,
            &clock,
            &ButlerContext::default(),
        )
        .unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn the_nightly_loop_researches_its_strongest_belief_reversibly() {
        use super::{run_due_nightly_loop, understanding};
        use endora_capabilities::CapabilitySpec;
        use endora_scheduling::{NightlyLoopSchedule, NightlyLoopScheduleRepository};
        use std::cell::RefCell;

        // A reversible research skill: `web_answers`, configured and cleared to run
        // on its own. It must be the ONLY skill the loop invokes.
        struct ResearchRunner;
        impl CapabilityRunner for ResearchRunner {
            fn available(&self) -> Vec<CapabilitySpec> {
                vec![
                    CapabilitySpec {
                        id: "web_answers".to_owned(),
                        description: "answer a question".to_owned(),
                        configured: true,
                        autonomous: true,
                        input_schema: None,
                        reversibility: endora_kernel::Reversibility::Observe,
                    },
                    // A consequential skill the loop must never reach for.
                    CapabilitySpec {
                        id: "flights".to_owned(),
                        description: "book flights".to_owned(),
                        configured: true,
                        autonomous: false,
                        input_schema: None,
                        reversibility: endora_kernel::Reversibility::Observe,
                    },
                ]
            }
            fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
                assert_eq!(id, "web_answers", "the loop must only run the cleared read");
                assert!(input_json.contains("sleep"), "researches the focus topic");
                Ok("Consistent sleep schedules improve energy and mood.".to_owned())
            }
        }

        // A butler that CHOOSES to research the focus (agentic): it reaches for the
        // reversible read first, then — once the finding is fed back — reflects.
        struct ResearchButler;
        impl Butler for ResearchButler {
            fn respond(
                &self,
                history: &[ChatMessage],
                _p: &[crate::Preference],
                _c: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                let researched =
                    last_skill_result(history).is_some_and(|t| t.contains("Consistent sleep"));
                if !researched {
                    // First pass: pick the reversible read to look into the focus.
                    return Ok(ButlerReply {
                        capability_use: Some(endora_capabilities::CapabilityUse {
                            capability: "web_answers".to_owned(),
                            input_json: "{\"query\":\"sleep habits\"}".to_owned(),
                        }),
                        ..ButlerReply::default()
                    });
                }
                // The finding is fed back — reflect and form a belief.
                Ok(ButlerReply {
                    text: "I looked into sleep tonight — worth keeping steady hours.".to_owned(),
                    beliefs: vec![crate::ports::FormedBelief {
                        statement: "benefits from consistent sleep".to_owned(),
                        kind: crate::BeliefKind::Intent,
                        confidence: crate::Confidence::Low,
                        evidence: "researched overnight".to_owned(),
                    }],
                    ..ButlerReply::default()
                })
            }
        }

        struct DueSchedule(RefCell<NightlyLoopSchedule>);
        impl NightlyLoopScheduleRepository for DueSchedule {
            fn get(&self) -> Result<Option<NightlyLoopSchedule>, RepositoryError> {
                Ok(Some(*self.0.borrow()))
            }
            fn set(&self, s: &NightlyLoopSchedule) -> Result<(), RepositoryError> {
                *self.0.borrow_mut() = *s;
                Ok(())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(27 * 3_600_000);
        let sched = DueSchedule(RefCell::new(NightlyLoopSchedule {
            enabled: true,
            hour_utc: 3,
            last_at: Timestamp::from_unix_millis(0),
        }));
        // The focus comes from what Endora understands (ADR 0052): the intent it is
        // most sure of wins, over a weaker belief and a stronger non-intent one.
        for (statement, kind, confidence) in [
            (
                "you want to sleep better",
                crate::BeliefKind::Intent,
                crate::Confidence::High,
            ),
            (
                "you want to read more",
                crate::BeliefKind::Intent,
                crate::Confidence::Low,
            ),
            (
                "you find noise stressful",
                crate::BeliefKind::Stressor,
                crate::Confidence::High,
            ),
        ] {
            BeliefRepository::save(
                &store,
                &Belief::new(
                    BeliefId::new(ids.new_id()),
                    statement,
                    kind,
                    confidence,
                    "said so",
                    clock.now(),
                )
                .unwrap(),
            )
            .unwrap();
        }
        let context = ButlerContext::default();

        let audit = FakeAudit {
            records: RefCell::new(Vec::new()),
        };
        let (msg, activity) = run_due_nightly_loop(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &FakeIntentions::default(),
            &sched,
            &ResearchRunner,
            &ResearchButler,
            &audit,
            &ids,
            &clock,
            &context,
        )
        .unwrap()
        .expect("the loop runs when due");

        // It chose the reversible read to research the focus, reflected, and left a note.
        assert!(activity.iter().any(|a| a.contains("web_answers")));
        assert!(msg.text().contains("sleep"));
        assert!(
            understanding(&store, &FixedClock(1_000))
                .unwrap()
                .iter()
                .any(|b| b.statement().contains("sleep"))
        );
    }

    #[test]
    fn a_turn_that_changed_nothing_says_so() {
        // Live: asked to turn on the kitchen table, the only action failed, and the reply
        // announced that "the guest bedroom left lamp is already on" — a device from
        // earlier in the conversation that the turn never touched.
        let failed = vec![super::ActionDisclosure {
            skill: "home.HassTurnOn".to_owned(),
            claimed: "error: MatchFailedError".to_owned(),
            observed: None,
            outcome: None,
        }];
        assert!(super::nothing_changed_note(&failed).contains("Nothing was changed"));
    }

    #[test]
    fn a_turn_that_changed_something_claims_nothing_extra() {
        // "Nothing changed" would itself be false the moment anything worked, including
        // when one action failed and a later one succeeded.
        let mixed = vec![
            super::ActionDisclosure {
                skill: "home.HassTurnOn".to_owned(),
                claimed: "error: MatchFailedError".to_owned(),
                observed: None,
                outcome: None,
            },
            super::ActionDisclosure {
                skill: "home.HassTurnOn".to_owned(),
                claimed: "The action completed successfully on: Kitchen Table".to_owned(),
                observed: None,
                outcome: None,
            },
        ];
        assert_eq!(super::nothing_changed_note(&mixed), "");
    }

    #[test]
    fn a_turn_that_acted_on_nothing_is_left_alone() {
        // A pure conversation turn has nothing to correct, and appending a denial to a
        // reply that never claimed to act would be noise.
        assert_eq!(super::nothing_changed_note(&[]), "");
    }

    #[test]
    fn a_failed_turn_tells_the_person_what_does_exist() {
        // Observed: the reply offered to "check the living room instead" while the
        // shortlist Endora had already worked out sat unread in the tool result. The
        // person gets what the model was given.
        let failed = vec![super::ActionDisclosure {
            skill: "home.HassTurnOff".to_owned(),
            claimed: "error: MatchFailedError\n\n[candidates] that name did not match \
                      anything. These exist:\n  - Kitchen Table\n  - Kitchen Bright"
                .to_owned(),
            observed: None,
            outcome: None,
        }];
        let note = super::nothing_changed_note(&failed);
        assert!(note.contains("Kitchen Table"), "{note}");
        assert!(note.contains("Kitchen Bright"), "{note}");
        assert!(note.contains("Nothing was changed"), "{note}");
    }

    #[test]
    fn a_failure_with_nothing_to_suggest_stays_short() {
        let failed = vec![super::ActionDisclosure {
            skill: "home.HassTurnOff".to_owned(),
            claimed: "error: the server is unreachable".to_owned(),
            observed: None,
            outcome: None,
        }];
        let note = super::nothing_changed_note(&failed);
        assert!(note.contains("Nothing was changed"), "{note}");
        assert!(!note.contains("look like"), "invented a suggestion: {note}");
    }

    #[test]
    fn a_turn_that_acted_never_apologises_for_it() {
        // Live: a light was switched on by direct reach, the model returned an empty
        // reply, and Endora answered "I'm not sure how to help with that yet" — an apology
        // for work it had just completed.
        let done = vec![super::ActionDisclosure {
            skill: "home-assistant.HassTurnOn".to_owned(),
            claimed: "Home Assistant accepted 'turn on' on light.kitchen_table.".to_owned(),
            observed: None,
            outcome: None,
        }];
        let said = super::acted_note(&done).expect("said nothing about work it had done");
        assert!(said.starts_with("Done."), "{said}");
        assert!(said.contains("light.kitchen_table"), "{said}");
        assert!(
            said.contains("home-assistant.HassTurnOn"),
            "attributes it: {said}"
        );
    }

    #[test]
    fn a_turn_that_only_failed_has_nothing_to_report() {
        // The apology is right here — and the "(Nothing was changed)" note carries the
        // rest of the story.
        let failed = vec![super::ActionDisclosure {
            skill: "home.HassTurnOn".to_owned(),
            claimed: "error: MatchFailedError".to_owned(),
            observed: None,
            outcome: None,
        }];
        assert!(super::acted_note(&failed).is_none());
        assert!(super::acted_note(&[]).is_none());
    }

    #[test]
    fn a_reading_that_moved_is_reported_as_having_worked() {
        // The hedge this replaces: Home Assistant answers before its integrations report
        // back, so its own sentence says nothing about the outcome. Endora compared two
        // readings and knew.
        let note = super::note_changed(Some("state: off"), Some("state: on"));
        assert!(note.contains("[changed]"), "{note}");
        assert!(note.contains("do not hedge"), "{note}");
        // Identical readings, or a missing one, assert nothing.
        assert_eq!(super::note_changed(Some("same"), Some("same")), "");
        assert_eq!(super::note_changed(None, Some("state: on")), "");
    }

    /// A butler that answers in pieces, the way a streaming endpoint does.
    struct SpeaksInPieces;

    impl Butler for SpeaksInPieces {
        fn respond(
            &self,
            _history: &[ChatMessage],
            _preferences: &[Preference],
            _context: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            Ok(ButlerReply::default())
        }
        fn take_turn(
            &self,
            _conversation: &[crate::TurnMessage],
            _preferences: &[Preference],
            _context: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            Ok(ButlerReply {
                text: "Good evening, sir.".to_owned(),
                ..ButlerReply::default()
            })
        }
        fn take_turn_streaming(
            &self,
            _conversation: &[crate::TurnMessage],
            _preferences: &[Preference],
            _context: &ButlerContext,
            on_token: &mut dyn FnMut(&str),
        ) -> Result<ButlerReply, ProposalError> {
            for piece in ["Good ", "evening, ", "sir."] {
                on_token(piece);
            }
            Ok(ButlerReply {
                text: "Good evening, sir.".to_owned(),
                ..ButlerReply::default()
            })
        }
    }

    #[test]
    fn the_reply_reaches_the_person_in_pieces() {
        // The turn used to hand the whole finished reply over in one call, so a streaming
        // client received a single lump and nothing appeared until the model had stopped
        // thinking. Each piece is now relayed as it arrives.
        let (ids, clock, audit) = (SeqIds::default(), FixedClock(4_000), FakeAudit::default());
        let chunks = std::cell::RefCell::new(Vec::<String>::new());
        let reply = super::run_tool_turn(
            &SpeaksInPieces,
            &NoCapabilities,
            &audit,
            &OutcomeSink::unmotivated(&FakeOutcomes::default()),
            &ids,
            &clock,
            &one_user_turn("evening"),
            &[],
            &ButlerContext::default(),
            6,
            &mut |_| {},
            &mut |t: &str| chunks.borrow_mut().push(t.to_owned()),
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .expect("the turn failed");
        let seen = chunks.into_inner();
        assert!(seen.len() > 1, "arrived as one lump: {seen:?}");
        assert_eq!(seen.concat(), reply.text, "the pieces are the whole reply");
    }

    /// A butler that says a line before calling a tool, then answers — the shape that
    /// exposed the duplication.
    struct ThinksAloudThenAnswers {
        round: std::cell::Cell<u8>,
    }

    impl Butler for ThinksAloudThenAnswers {
        fn respond(
            &self,
            _history: &[ChatMessage],
            _preferences: &[Preference],
            _context: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            Ok(ButlerReply::default())
        }
        fn take_turn(
            &self,
            _conversation: &[crate::TurnMessage],
            _preferences: &[Preference],
            _context: &ButlerContext,
        ) -> Result<ButlerReply, ProposalError> {
            unreachable!("the streaming round is used")
        }
        fn take_turn_streaming(
            &self,
            _conversation: &[crate::TurnMessage],
            _preferences: &[Preference],
            _context: &ButlerContext,
            on_token: &mut dyn FnMut(&str),
        ) -> Result<ButlerReply, ProposalError> {
            if self.round.get() == 0 {
                self.round.set(1);
                // A line the person sees, which is NOT part of the final answer.
                on_token("Let me check. ");
                return Ok(ButlerReply {
                    text: "Let me check. ".to_owned(),
                    tool_calls: vec![crate::ToolCall {
                        id: "1".to_owned(),
                        capability: "noop".to_owned(),
                        input_json: "{}".to_owned(),
                    }],
                    ..ButlerReply::default()
                });
            }
            on_token("It is on.");
            Ok(ButlerReply {
                text: "It is on.".to_owned(),
                ..ButlerReply::default()
            })
        }
    }

    #[test]
    fn a_line_before_a_tool_call_does_not_make_the_answer_arrive_twice() {
        // Diffing against everything streamed does not work: a model that writes "let me
        // check" before calling a tool breaks the prefix, so the whole answer would be
        // sent a second time after the person had already watched it arrive.
        //
        // Against the WHOLE turn, not just the tool loop — the second send happens after
        // the loop returns, which is where the duplication lived.
        let store = FakeStore::default();
        let (ids, clock) = (SeqIds::default(), FixedClock(4_000));
        let mut streamed = String::new();
        let (msg, _activity) = super::send_to_butler_streaming(
            &store,
            &store,
            &store,
            &FakeOutcomes::default(),
            &NoCapabilities,
            &ThinksAloudThenAnswers {
                round: std::cell::Cell::new(0),
            },
            &FakeAudit::default(),
            None,
            None,
            &ids,
            &clock,
            &ButlerContext::default(),
            "is the light on?",
            &mut |t| streamed.push_str(t),
            &mut |_| {},
            &mut Vec::new(),
        )
        .expect("the turn failed");
        assert!(
            streamed.contains("Let me check."),
            "the preamble reached them: {streamed}"
        );
        assert_eq!(
            streamed.matches("It is on.").count(),
            1,
            "the answer arrived twice: {streamed}"
        );
        assert_eq!(
            msg.text(),
            "It is on.",
            "the stored reply is the answer alone"
        );
    }

    /// A context offering the tools the live turn offered.
    fn offering(ids: &[&str]) -> ButlerContext {
        ButlerContext {
            tools: ids
                .iter()
                .map(|id| crate::CapabilityTool {
                    id: (*id).to_owned(),
                    description: String::new(),
                    input_schema: None,
                })
                .collect(),
            ..ButlerContext::default()
        }
    }

    fn said(text: &str) -> ButlerReply {
        ButlerReply {
            text: text.to_owned(),
            ..ButlerReply::default()
        }
    }

    #[test]
    fn naming_a_tool_instead_of_calling_it_is_not_an_answer() {
        // Live, in answer to "I usually want news weather traffic". Nothing ran.
        let ctx = offering(&["home-assistant.GetWeather", "home-assistant.GetTraffic"]);
        let described = said(
            "Based on your request, here are the appropriate function calls:\n             1. **GetWeather** - To fetch the current weather.",
        );
        assert!(super::not_an_answer(&described, &ctx));
    }

    #[test]
    fn talking_about_the_protocol_is_not_an_answer() {
        // Live, unprompted an hour later, straight into the person's inbox.
        let plumbing = said(
            "None of the functions provided pertain to the 'news' domain, hence no \
             exposed entities were found.",
        );
        assert!(super::not_an_answer(&plumbing, &offering(&[])));
    }

    #[test]
    fn an_ordinary_reply_is_left_alone() {
        let ctx = offering(&["home-assistant.GetWeather", "home-assistant.HassTurnOn"]);
        // Mentions no offered tool and no protocol words, however it is phrased.
        assert!(!super::not_an_answer(
            &said("It is 14 degrees and clear."),
            &ctx
        ));
        // Short words are not matched, so a tool called `Get` could never swallow prose.
        assert!(!super::not_an_answer(
            &said("I will get the door."),
            &offering(&["x.Get"])
        ));
    }

    #[test]
    fn a_reply_that_actually_called_something_is_always_an_answer() {
        // Naming a tool it also USED is just explaining itself, which is welcome.
        let ctx = offering(&["home-assistant.GetWeather"]);
        let acted = ButlerReply {
            text: "I used GetWeather: it is 14 degrees.".to_owned(),
            tool_calls: vec![crate::ToolCall {
                id: "1".to_owned(),
                capability: "home-assistant.GetWeather".to_owned(),
                input_json: "{}".to_owned(),
            }],
            ..ButlerReply::default()
        };
        assert!(!super::not_an_answer(&acted, &ctx));
    }

    fn house() -> Vec<(String, String)> {
        [
            ("Kitchen Table", "off"),
            ("Kitchen Main Light", "on"),
            ("Kitchen", "on"),
            ("Garage Main", "on"),
            ("Outside Color", "unavailable"),
        ]
        .into_iter()
        .map(|(n, s)| (n.to_owned(), s.to_owned()))
        .collect()
    }

    #[test]
    fn an_answer_carries_the_facts_it_spoke_about() {
        // Live: "the kitchen table light is already on" about a light that was off. The
        // reading said so at the time and nothing showed it.
        let note = super::facts_behind("The kitchen table light is already on.", house());
        assert!(note.contains("Kitchen Table is off"), "{note}");
    }

    #[test]
    fn a_longer_name_wins_over_the_one_inside_it() {
        // A reply about the ceiling light must not also report the whole room, or the
        // facts become their own noise.
        let note = super::facts_behind("Kitchen Main Light is on.", house());
        assert!(note.contains("Kitchen Main Light is on"), "{note}");
        assert_eq!(note.matches("Kitchen").count(), 1, "reported twice: {note}");
    }

    #[test]
    fn an_answer_that_named_nothing_is_left_alone() {
        // Most replies are not about state, and appending a wall of it would be noise.
        assert_eq!(super::facts_behind("Good evening, sir.", house()), "");
        assert_eq!(
            super::facts_behind("The kitchen table light is off.", Vec::new()),
            ""
        );
    }

    #[test]
    fn a_vague_answer_is_shown_the_things_it_gestured_at() {
        // "Several lights are on" is not false, and it is not an answer either. Whatever
        // it did name gets its actual state put beside the vagueness.
        let note = super::facts_behind(
            "There are several lights on in your home, including the kitchen and garage.",
            house(),
        );
        assert!(note.contains("Kitchen is on"), "{note}");
        // And only what it named: "garage" is not "Garage Main", so that is not claimed
        // to be what the reply meant. Guessing at a half-mentioned name is how a
        // disclosure starts inventing.
        assert!(!note.contains("Garage Main"), "{note}");
    }
}

#[cfg(test)]
mod three_cards_for_one_fact {
    use super::*;

    #[test]
    fn a_wordier_way_of_saying_the_same_thing_is_the_same_belief() {
        // All three of these were on one screen at once, each asking to be confirmed
        // separately. The third is the one plain overlap missed: it spends most of its
        // words on how strongly the person feels and only two on the subject.
        let plain = "you prefer temperatures in Fahrenheit";
        assert!(similar(
            plain,
            "You prefer temperature measurements in Fahrenheit."
        ));
        assert!(similar(
            plain,
            "You find it more convenient and accurate to measure temperature in Fahrenheit"
        ));
    }

    #[test]
    fn a_contradiction_needs_the_two_to_be_about_the_same_thing() {
        // Live, on the deployed instance: this one belief was reported as contradicting
        // every other belief Endora held, because negation asymmetry alone counted and
        // nothing checked the two were even on the same topic.
        let celsius = "You find it more convenient and accurate to measure temperature in Celsius \
             rather than Fahrenheit.";
        for unrelated in [
            "You want to know where I found the event information",
            "you usually want news, weather, and traffic",
            "you prefer to run after work on Tuesdays and Thursdays",
            "You like a positive, friendly greeting at the beginning of each day.",
        ] {
            assert!(
                !statements_disagree(celsius, unrelated),
                "claimed a contradiction with an unrelated belief: {unrelated:?}"
            );
        }
        // The one it genuinely does disagree with is still caught.
        assert!(statements_disagree(
            celsius,
            "you prefer temperatures in Fahrenheit"
        ));
    }

    #[test]
    fn stripping_the_stance_does_not_merge_opposites() {
        // The cost of ignoring stance words: two statements about the same subject look
        // identical whether they agree or not. That is safe only because disagreement is
        // checked first, so this is the test that holds the whole trade together.
        assert!(!similar("you like running", "you hate running"));
        assert!(!similar(
            "you prefer temperatures in Fahrenheit",
            "you do not want temperatures in Fahrenheit"
        ));
        // And different subjects stay different however similarly they are framed.
        assert!(!similar("you prefer coffee", "you prefer tea"));
    }
}

#[cfg(test)]
mod re_reading_what_is_stored {
    use super::*;
    use endora_kernel::Timestamp;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Remembered(Mutex<Vec<Belief>>);

    impl BeliefRepository for Remembered {
        fn save(&self, belief: &Belief) -> Result<(), endora_kernel::RepositoryError> {
            let mut held = self.0.lock().unwrap();
            if let Some(slot) = held.iter_mut().find(|b| b.id() == belief.id()) {
                *slot = belief.clone();
            } else {
                held.push(belief.clone());
            }
            Ok(())
        }
        fn get(&self, id: BeliefId) -> Result<Option<Belief>, endora_kernel::RepositoryError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|b| b.id() == id)
                .cloned())
        }
        fn list(&self) -> Result<Vec<Belief>, endora_kernel::RepositoryError> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    fn stored(id: u128, statement: &str, created_ms: i64) -> Belief {
        Belief::from_parts(
            BeliefId::new(id),
            statement.to_owned(),
            crate::BeliefKind::Preference,
            crate::Confidence::High,
            "because you said so".to_owned(),
            Timestamp::from_unix_millis(created_ms),
            Timestamp::from_unix_millis(created_ms),
            crate::BeliefStatus::Active,
        )
    }

    fn still_held(repo: &Remembered) -> Vec<String> {
        repo.list()
            .unwrap()
            .into_iter()
            .filter(|b| b.status() == crate::BeliefStatus::Active)
            .map(|b| b.statement().to_owned())
            .collect()
    }

    #[test]
    fn a_belief_the_rules_would_no_longer_form_does_not_get_to_stay() {
        // The card that prompted this: stored before the rule against instruction-shaped
        // statements existed, and untouchable afterwards because guards only ever ran at
        // formation. Rules improving has to reach backwards or the store is frozen at the
        // quality of the day each row was written.
        let repo = Remembered::default();
        repo.save(&stored(
            1,
            "you want me to turn off the kitchen light",
            1_000,
        ))
        .unwrap();
        repo.save(&stored(2, "you run on Tuesdays and Thursdays", 2_000))
            .unwrap();

        let retired = tidy_understanding(&repo, &FixedClock(9_000)).unwrap();

        assert_eq!(retired, 1);
        assert_eq!(still_held(&repo), vec!["you run on Tuesdays and Thursdays"]);
        // Expired, not corrected: the person never said it was wrong, and saying they did
        // would put words in their mouth.
        let dropped = repo.get(BeliefId::new(1)).unwrap().unwrap();
        assert_eq!(dropped.status(), crate::BeliefStatus::Expired);
    }

    #[test]
    fn three_ways_of_saying_one_thing_become_one_thing_the_oldest() {
        // Exactly what was on screen: three Fahrenheit cards, each asking separately.
        let repo = Remembered::default();
        repo.save(&stored(1, "you prefer temperatures in Fahrenheit", 1_000))
            .unwrap();
        repo.save(&stored(
            2,
            "You prefer temperature measurements in Fahrenheit.",
            2_000,
        ))
        .unwrap();
        repo.save(&stored(
            3,
            "You find it more convenient and accurate to measure temperature in Fahrenheit",
            3_000,
        ))
        .unwrap();

        let retired = tidy_understanding(&repo, &FixedClock(9_000)).unwrap();

        assert_eq!(retired, 2);
        assert_eq!(
            still_held(&repo),
            vec!["you prefer temperatures in Fahrenheit"]
        );
        // The survivor is the oldest, and the duplicates count as evidence for it rather
        // than being thrown away — the same thought arriving three times is why it is
        // worth being sure about.
        let survivor = repo.get(BeliefId::new(1)).unwrap().unwrap();
        assert!(survivor.last_affirmed_at().unix_millis() > survivor.created_at().unix_millis());
    }

    #[test]
    fn two_beliefs_that_disagree_are_both_left_alone() {
        // Merging these would silently pick a winner. Endora holding both is the honest
        // state, and which one is true is the person's call (ADR 0052).
        let repo = Remembered::default();
        repo.save(&stored(1, "you like running", 1_000)).unwrap();
        repo.save(&stored(2, "you hate running", 2_000)).unwrap();

        assert_eq!(tidy_understanding(&repo, &FixedClock(9_000)).unwrap(), 0);
        assert_eq!(still_held(&repo).len(), 2);
    }

    struct FixedClock(i64);
    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            Timestamp::from_unix_millis(self.0)
        }
    }
}

#[cfg(test)]
mod what_a_read_gets_back {
    use super::*;
    use crate::Reversibility;

    fn actuator() -> crate::CapabilitySpec {
        crate::CapabilitySpec {
            id: "home-assistant.GetDateTime".to_owned(),
            description: String::new(),
            configured: true,
            autonomous: false,
            reversibility: Reversibility::Irreversible,
            input_schema: None,
        }
    }

    /// The reading that was actually attached to a request for the time.
    const THE_WHOLE_HOUSE: &str = "Live Context: An overview of the areas and the devices \
        in this smart home:\n- names: All Lights\n  domain: light\n  state: 'on'\n\
        - names: Apple TV\n  domain: media_player\n  state: 'off'";

    #[test]
    fn a_call_that_moved_nothing_does_not_drag_the_world_along() {
        // Live: asking Home Assistant for the time came back with five kilobytes of every
        // device in the house, under "answer from the OBSERVATION" — three times in one
        // turn. The briefing that followed was one sentence about a bedroom light.
        let out = note_verification_against(
            "{\"time\": \"05:40:30\"}",
            Some(&actuator()),
            Some(THE_WHOLE_HOUSE),
            Some(THE_WHOLE_HOUSE),
        );
        assert!(!out.contains("Apple TV"), "pasted the house: {out}");
        assert!(out.contains("unchanged from before"), "{out}");
        // The verdict itself is untouched — that is the signal ADR 0053 exists for, and
        // it is still stated, in a sentence rather than in five kilobytes.
        let verdict = note_unchanged(Some(THE_WHOLE_HOUSE), Some(THE_WHOLE_HOUSE));
        assert!(verdict.contains("[unchanged]"));
    }

    #[test]
    fn a_call_that_moved_something_still_carries_the_evidence() {
        // The whole point of reading back. When the world differs, the detail *is* the
        // evidence and it must reach the model.
        let after = THE_WHOLE_HOUSE.replace("state: 'on'", "state: 'off'");
        let out = note_verification_against(
            "turned it off",
            Some(&actuator()),
            Some(THE_WHOLE_HOUSE),
            Some(&after),
        );
        assert!(out.contains("Answer from the OBSERVATION"), "{out}");
        assert!(out.contains("Apple TV"), "{out}");
    }

    #[test]
    fn nothing_to_compare_against_still_carries_the_reading() {
        // A reader that failed before the call and worked after leaves no comparison, and
        // the safe default is to show what was seen rather than to assert it means nothing.
        let out = note_verification_against("done", Some(&actuator()), None, Some(THE_WHOLE_HOUSE));
        assert!(out.contains("Answer from the OBSERVATION"), "{out}");
    }
}

#[cfg(test)]
mod one_call_two_spellings {
    use endora_capabilities::same_call_as;

    #[test]
    fn punctuation_does_not_make_it_a_different_call() {
        // Live, in one morning briefing: four rounds spent on two pairs of identical
        // calls, none caught, because the guard compared raw text.
        assert_eq!(same_call_as("{}"), same_call_as("{ }"));
        assert_eq!(same_call_as("{}"), same_call_as("\n  {}\n"));
        assert_eq!(
            same_call_as(r#"{"a":1,"b":2}"#),
            same_call_as(r#"{ "b": 2, "a": 1 }"#)
        );
    }

    #[test]
    fn genuinely_different_arguments_stay_different() {
        // The guard must never collapse two calls that ask for different things — that
        // would silently drop work the model actually needed.
        assert_ne!(
            same_call_as(r#"{"domain":["weather"]}"#),
            same_call_as(r#"{"domain":["light"]}"#)
        );
        assert_ne!(same_call_as("{}"), same_call_as(r#"{"domain":[]}"#));
    }

    #[test]
    fn arguments_that_will_not_parse_are_left_alone() {
        // No worse than before: unparseable text compares as text.
        assert_eq!(same_call_as("not json"), "not json");
        assert_ne!(same_call_as("not json"), same_call_as("other junk"));
    }
}

#[cfg(test)]
mod facts_only_where_they_mean_something {
    use super::*;

    fn house() -> Vec<(String, String)> {
        [
            ("Home", "0"),
            ("Kitchen Table", "off"),
            ("Kitchen Main Light", "on"),
            ("Bedroom", "off"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
    }

    #[test]
    fn a_greeting_gets_no_facts_attached_to_it() {
        // Live, under "It's good afternoon! How can I assist you today?": `[state] Home is
        // 0`. The house contains something called `Home`, the sentence contained the word
        // "home", and the disclosure was both an exact match and meaningless.
        let greeting = "It's good afternoon! How can I assist you today? Do you need help \
                        with any specific tasks or information related to your home \
                        automation setup?";
        assert_eq!(facts_behind(greeting, house()), "");
    }

    #[test]
    fn a_reply_that_asserts_a_state_still_carries_the_facts() {
        // The reason the disclosure exists: the model said "already on" about something the
        // reading says is off, and the person can see both.
        let claim = "The kitchen table light is already on.";
        let shown = facts_behind(claim, house());
        assert!(shown.contains("Kitchen Table is off"), "{shown}");
        // Longest first, and a name inside a matched longer one is not repeated.
        assert!(!shown.contains("Home is 0"), "{shown}");
    }

    #[test]
    fn a_name_is_matched_as_words_not_as_letters() {
        // `Home` must not match "homework", and the state word must be a word too —
        // "information" contains "on" and asserts nothing.
        let states = vec![("Home".to_owned(), "0".to_owned())];
        assert_eq!(
            facts_behind("I finished my homework, 0 problems", states.clone()),
            ""
        );
        assert!(facts_behind("home is 0 right now", states).contains("Home is 0"));
    }

    #[test]
    fn the_vocabulary_comes_from_the_reading_not_from_english() {
        // A service reporting open/closed is handled by the same code as one reporting
        // on/off, with nothing anywhere naming either pair.
        let garage = vec![("Garage Door".to_owned(), "closed".to_owned())];
        let shown = facts_behind("The garage door is open.", garage.clone());
        assert_eq!(shown, "", "'open' is not a value this service reported");
        assert!(
            facts_behind("The garage door is closed, I think.", garage)
                .contains("Garage Door is closed")
        );
    }
}

#[cfg(test)]
mod what_reaches_an_empty_room {
    use super::*;

    fn offering_nothing() -> ButlerContext {
        ButlerContext::default()
    }

    #[test]
    fn an_apology_for_an_unreachable_model_is_not_an_answer() {
        // Live, 00:00:14: the night loop ran, could not reach the model, and posted "Give
        // me a moment and try again" into an empty room. Nobody had asked anything.
        let degraded = ButlerReply {
            text: "Sorry — I couldn't reach my language model just now.".to_owned(),
            degraded: true,
            ..ButlerReply::default()
        };
        assert!(not_an_answer(&degraded, &offering_nothing()));

        // The same sentence, when it is genuinely the butler's answer, is kept — the flag
        // is what distinguishes them, not the words.
        let genuine = ButlerReply {
            text: "Sorry — I couldn't reach my language model just now.".to_owned(),
            ..ButlerReply::default()
        };
        assert!(!not_an_answer(&genuine, &offering_nothing()));
    }

    #[test]
    fn protocol_prose_posted_unprompted_is_not_an_answer() {
        // Live, 04:18, in the person's chat: the check-in's whole contribution for the day.
        // The suppression list held "functions provided" and missed this by one word.
        let leaked = ButlerReply {
            text: "None of the functions listed involve a 'person' domain, so there's no \
                   need to call any."
                .to_owned(),
            ..ButlerReply::default()
        };
        assert!(not_an_answer(&leaked, &offering_nothing()));
    }

    #[test]
    fn an_ordinary_reply_is_still_an_answer() {
        let real = ButlerReply {
            text: "Good morning, sir. The kitchen and garage lights are on.".to_owned(),
            ..ButlerReply::default()
        };
        assert!(!not_an_answer(&real, &offering_nothing()));
    }
}

/// How far back "while I was out" reaches, when nobody says.
///
/// A day, because the question is asked on returning and the answer people want is "since I
/// last paid attention", not "since the beginning".
pub const A_DAY_MS: i64 = 86_400_000;

/// At most this many of any one kind of thing, so the answer stays an answer.
const ENOUGH_OF_ANY_ONE_THING: usize = 5;

/// What Endora has actually been doing, read from its own record (ADR 0056).
///
/// Asked "did you do anything while I was out?" and then "nothing proactive done today?",
/// the butler replied **"No specific activities were recorded today"** — on a day it had
/// posted three unprompted messages, one of them a real morning brief four hours earlier.
/// Every part of the true answer was stored; none of it was reachable from a turn, so the
/// most important question a proactive butler can be asked was the one it could not answer.
///
/// Being proactive and unable to account for it is worse than not being proactive: the
/// person cannot tell a quiet day from a broken one.
///
/// Deterministic, and assembled here rather than asked of the model — this is a **report of
/// stored facts**, which is the one thing Endora is entitled to assert
/// ([0053](../../docs/adr/0053-honesty-about-what-it-did.md)). The model's job is to say it
/// nicely, not to remember it.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
pub fn what_it_has_been_doing(
    chat: &impl ChatRepository,
    outcomes: &impl OutcomeRepository,
    writes: &impl endora_capabilities::ConfigWriteLog,
    troubles: &impl endora_capabilities::StandingTroubleRepository,
    since_ms: i64,
    now_ms: i64,
) -> Result<String, AppError> {
    let mut lines: Vec<String> = Vec::new();

    // Messages Endora started itself. There is no stored flag for "proactive", so it is
    // read from adjacency: a butler message whose predecessor is not the person's is one
    // nobody asked for. A butler message at the very start of the window has no visible
    // predecessor and is counted, which is right far more often than not — the alternative
    // is silently dropping the brief that opened the day.
    let history = chat.between(since_ms, now_ms)?;
    let mut spoke_first: Vec<&ChatMessage> = Vec::new();
    for (i, message) in history.iter().enumerate() {
        if message.role() != MessageRole::Butler {
            continue;
        }
        let asked_for = i > 0 && history[i - 1].role() == MessageRole::User;
        if !asked_for {
            spoke_first.push(message);
        }
    }
    for message in spoke_first.iter().take(ENOUGH_OF_ANY_ONE_THING) {
        lines.push(format!(
            "{} — I wrote to you unprompted: \"{}\"",
            clock_time_of(message.at().unix_millis()),
            first_sentence_of(message.text())
        ));
    }

    // What it did, kept as claim-and-observation rather than a verdict, exactly as stored.
    let acted: Vec<_> = outcomes
        .list()?
        .into_iter()
        .filter(|o| o.at().unix_millis() >= since_ms)
        .collect();
    let (worked, failed): (Vec<_>, Vec<_>) = acted
        .iter()
        .partition(|o| !o.claim().trim_start().starts_with("error:"));
    for outcome in worked.iter().take(ENOUGH_OF_ANY_ONE_THING) {
        let verdict = match outcome.changed() {
            Some(true) => "and I saw it change",
            Some(false) => "but nothing changed when I looked",
            None => "though I could not check",
        };
        lines.push(format!(
            "{} — used {} {verdict}",
            clock_time_of(outcome.at().unix_millis()),
            outcome.capability()
        ));
    }
    for outcome in failed.iter().take(ENOUGH_OF_ANY_ONE_THING) {
        lines.push(format!(
            "{} — tried {} and it failed",
            clock_time_of(outcome.at().unix_millis()),
            outcome.capability()
        ));
    }

    // Changes it made inside somebody else's service, which are the consequential ones.
    for write in writes
        .writes(ENOUGH_OF_ANY_ONE_THING)?
        .iter()
        .filter(|w| w.at_ms >= since_ms)
    {
        lines.push(format!(
            "{} — changed {} in {}",
            clock_time_of(write.at_ms),
            write.target,
            write.server
        ));
    }

    // Things it started watching. Not the ones it is *raising* — those are a question the
    // person answers, and this is a report of what happened.
    for trouble in troubles
        .troubles()?
        .iter()
        .filter(|t| t.since_ms >= since_ms)
        .take(ENOUGH_OF_ANY_ONE_THING)
    {
        lines.push(format!(
            "{} — noticed {} stopped answering",
            clock_time_of(trouble.since_ms),
            trouble.thing
        ));
    }

    if lines.is_empty() {
        // The honest empty answer, and deliberately not "no activities were recorded".
        // Nothing happening and nothing being *recorded* are different claims, and the
        // second one was made falsely.
        return Ok(
            "Nothing. I did not write to you, act on anything, change any settings, or \
             notice anything stop answering in that time."
                .to_owned(),
        );
    }
    lines.sort();
    Ok(lines.join("\n"))
}

/// `HH:MM` in UTC, matching how the rest of the system reports times.
fn clock_time_of(ms: i64) -> String {
    let secs = ms.rem_euclid(A_DAY_MS) / 1000;
    format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

/// The first sentence, so a quoted brief does not become the whole report.
fn first_sentence_of(text: &str) -> String {
    const ENOUGH: usize = 90;
    let trimmed = text.trim();
    let end = trimmed
        .find(['.', '!', '?'])
        .map_or(trimmed.len(), |i| i + 1);
    let cut = end.min(ENOUGH);
    let mut out: String = trimmed.chars().take(cut).collect();
    if cut < trimmed.chars().count() {
        out.push('…');
    }
    out
}

/// A world with nothing wrong in it, for tests about something else.
#[cfg(test)]
#[derive(Default)]
struct NoTrouble;

#[cfg(test)]
impl endora_capabilities::StandingTroubleRepository for NoTrouble {
    fn note_trouble(
        &self,
        _t: &endora_capabilities::StandingTrouble,
    ) -> Result<(), endora_kernel::RepositoryError> {
        Ok(())
    }
    fn clear_trouble(&self, _s: &str, _t: &str) -> Result<(), endora_kernel::RepositoryError> {
        Ok(())
    }
    fn troubles(
        &self,
    ) -> Result<Vec<endora_capabilities::StandingTrouble>, endora_kernel::RepositoryError> {
        Ok(Vec::new())
    }
    fn accept_trouble(&self, _s: &str, _t: &str) -> Result<(), endora_kernel::RepositoryError> {
        Ok(())
    }
}

#[cfg(test)]
mod accounting_for_itself {
    use super::*;
    use endora_kernel::{RepositoryError, Timestamp};

    struct Said(Vec<ChatMessage>);
    impl ChatRepository for Said {
        fn append(&self, _m: &ChatMessage) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError> {
            Ok(self.0.clone())
        }
        fn between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<ChatMessage>, RepositoryError> {
            Ok(self
                .0
                .iter()
                .filter(|m| m.at().unix_millis() >= from_ms && m.at().unix_millis() <= to_ms)
                .cloned()
                .collect())
        }
        fn days(&self, _offset_minutes: i64) -> Result<Vec<(String, usize)>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    struct Nothing;
    impl OutcomeRepository for Nothing {
        fn save(&self, _o: &Outcome) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn get(&self, _id: crate::OutcomeId) -> Result<Option<Outcome>, RepositoryError> {
            Ok(None)
        }
        fn list(&self) -> Result<Vec<Outcome>, RepositoryError> {
            Ok(Vec::new())
        }
    }
    impl endora_capabilities::ConfigWriteLog for Nothing {
        fn record(&self, _w: &endora_capabilities::ConfigWrite) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn writes(
            &self,
            _limit: usize,
        ) -> Result<Vec<endora_capabilities::ConfigWrite>, RepositoryError> {
            Ok(Vec::new())
        }
        fn write(
            &self,
            _id: u128,
        ) -> Result<Option<endora_capabilities::ConfigWrite>, RepositoryError> {
            Ok(None)
        }
        fn mark_undone(&self, _id: u128) -> Result<(), RepositoryError> {
            Ok(())
        }
    }
    impl endora_capabilities::StandingTroubleRepository for Nothing {
        fn note_trouble(
            &self,
            _t: &endora_capabilities::StandingTrouble,
        ) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn clear_trouble(&self, _s: &str, _t: &str) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn troubles(&self) -> Result<Vec<endora_capabilities::StandingTrouble>, RepositoryError> {
            Ok(Vec::new())
        }
        fn accept_trouble(&self, _s: &str, _t: &str) -> Result<(), RepositoryError> {
            Ok(())
        }
    }

    fn message(role: MessageRole, text: &str, at_ms: i64) -> ChatMessage {
        ChatMessage::new(
            crate::MessageId::new(at_ms as u128),
            role,
            text,
            Timestamp::from_unix_millis(at_ms),
        )
        .expect("valid message")
    }

    /// 11:01 and 04:18 on the day the real failure happened.
    const BRIEF_AT: i64 = 39_660_000;
    const CHECKIN_AT: i64 = 15_480_000;

    #[test]
    fn the_brief_it_posted_is_in_the_answer() {
        // The live failure: asked "nothing proactive done today?", it said "No specific
        // activities were recorded today" — four hours after posting a real brief.
        let said = Said(vec![
            message(
                MessageRole::Butler,
                "Good morning, sir. The kitchen and garage lights are on, while others like the bedroom are off.",
                BRIEF_AT,
            ),
            message(MessageRole::User, "Good afternoon", BRIEF_AT + 1_000),
            message(
                MessageRole::Butler,
                "It's good afternoon! How can I assist you?",
                BRIEF_AT + 2_000,
            ),
        ]);
        let report = what_it_has_been_doing(&said, &Nothing, &Nothing, &Nothing, 0, A_DAY_MS)
            .expect("report");

        assert!(report.contains("11:01"), "{report}");
        assert!(report.contains("unprompted"), "{report}");
        assert!(report.contains("Good morning, sir."), "{report}");
        // The reply it gave when ASKED is not something it did unprompted, and must not be
        // reported as though it were.
        assert!(!report.contains("good afternoon! How can"), "{report}");
    }

    #[test]
    fn a_quiet_day_says_so_without_claiming_nothing_was_recorded() {
        // "No specific activities were recorded today" was false, and would have been a
        // different claim from "nothing happened" even if it were true. Endora reports what
        // it did, not what its own storage managed to capture.
        let report =
            what_it_has_been_doing(&Said(Vec::new()), &Nothing, &Nothing, &Nothing, 0, A_DAY_MS)
                .expect("report");
        assert!(report.starts_with("Nothing."), "{report}");
        assert!(!report.to_lowercase().contains("recorded"), "{report}");
    }

    #[test]
    fn two_unprompted_messages_are_both_reported_oldest_first() {
        let said = Said(vec![
            message(
                MessageRole::Butler,
                "None of the functions listed involve a person domain.",
                CHECKIN_AT,
            ),
            message(
                MessageRole::Butler,
                "Good morning, sir. The kitchen lights are on.",
                BRIEF_AT,
            ),
        ]);
        let report = what_it_has_been_doing(&said, &Nothing, &Nothing, &Nothing, 0, A_DAY_MS)
            .expect("report");
        let checkin = report.find("04:18").expect("the check-in");
        let brief = report.find("11:01").expect("the brief");
        assert!(checkin < brief, "oldest first, got: {report}");
    }
}

#[cfg(test)]
mod the_record_next_to_the_claim {
    use super::*;

    fn did() -> Vec<String> {
        vec![
            "11:01 — I wrote to you unprompted: \"Good morning, sir.…\"".to_owned(),
            "21:33 — tried home-assistant.HassTurnOn and it failed".to_owned(),
        ]
    }

    #[test]
    fn denying_activity_gets_the_record_attached() {
        // The exact live sentence, given verbatim twice on two different days, with eleven
        // things in the record at the time.
        let denial = "I checked, but no specific activities were recorded while you were \
                      out. Would you like to hear about any events happening nearby?";
        let shown = account_behind(denial, &did());
        assert!(shown.contains("[did]"), "{shown}");
        assert!(shown.contains("I wrote to you unprompted"), "{shown}");
        assert!(shown.contains("HassTurnOn and it failed"), "{shown}");
    }

    #[test]
    fn hedging_about_proactive_actions_also_gets_it() {
        // The other live shape: "It seems like you're asking about whether any proactive
        // actions were taken today. However, to provide a more accurate response…"
        let hedge = "It seems like you're asking about whether any proactive actions were \
                     taken today. Could you please specify the area or domain?";
        assert!(account_behind(hedge, &did()).contains("[did]"));
    }

    #[test]
    fn an_ordinary_reply_gets_nothing_attached() {
        // The vocabulary is tiny on purpose: these words are rare in butler prose, so a
        // normal answer is left alone.
        for ordinary in [
            "The kitchen table light is already on.",
            "Good evening, sir. I hope you had a productive day.",
            "I turned the garage light off for you.",
            "It's 9:41 in the evening.",
        ] {
            assert_eq!(account_behind(ordinary, &did()), "", "{ordinary}");
        }
    }

    #[test]
    fn with_nothing_to_report_nothing_is_attached() {
        // No record, no disclosure — appending an empty account would be its own kind of
        // noise, and the reply denying activity would then be correct anyway.
        let denial = "No specific activities were recorded.";
        assert_eq!(account_behind(denial, &[]), "");
    }
}

#[cfg(test)]
mod when_the_local_model_will_not_do_it {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A local model that always fails the deterministic check, whatever it is asked.
    struct AlwaysNothing(AtomicUsize);
    impl Butler for AlwaysNothing {
        fn respond(
            &self,
            _h: &[ChatMessage],
            _p: &[Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, crate::ProposalError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ButlerReply::default())
        }
        fn deeper(&self) -> Option<std::sync::Arc<dyn Butler + Send + Sync>> {
            Some(std::sync::Arc::new(Deeper))
        }
    }

    /// The stronger model, which answers.
    struct Deeper;
    impl Butler for Deeper {
        fn respond(
            &self,
            _h: &[ChatMessage],
            _p: &[Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, crate::ProposalError> {
            Ok(ButlerReply {
                text: "The kitchen table light is off.".to_owned(),
                ..ButlerReply::default()
            })
        }
    }

    /// A local model with nothing better to fall back to — the default.
    struct Alone;
    impl Butler for Alone {
        fn respond(
            &self,
            _h: &[ChatMessage],
            _p: &[Preference],
            _c: &ButlerContext,
        ) -> Result<ButlerReply, crate::ProposalError> {
            Ok(ButlerReply::default())
        }
    }

    #[test]
    fn a_second_model_answers_after_the_local_one_keeps_failing() {
        // Reliability compounds: two independent attempts fail together only at
        // (1-p1)(1-p2). The trigger is a check Endora applied to the reply, never the
        // model's opinion of how it did.
        let local = AlwaysNothing(AtomicUsize::new(0));
        let reply = take_turn_retrying_empty(
            &local,
            &[TurnMessage::User(
                "is the kitchen table light on?".to_owned(),
            )],
            &[],
            &ButlerContext::default(),
            &mut |_| {},
        )
        .expect("a turn");

        assert_eq!(reply.text, "The kitchen table light is off.");
        assert!(reply.escalated, "an escalated reply must say so");
        // The local model is tried, and retried, BEFORE anything leaves the box.
        assert_eq!(local.0.load(Ordering::SeqCst), 3, "local attempts");
    }

    #[test]
    fn with_no_fallback_the_local_answer_stands() {
        let reply = take_turn_retrying_empty(
            &Alone,
            &[TurnMessage::User("hello".to_owned())],
            &[],
            &ButlerContext::default(),
            &mut |_| {},
        )
        .expect("a turn");
        assert!(reply.text.is_empty());
        assert!(!reply.escalated);
    }

    #[test]
    fn saying_the_same_thing_again_counts_as_saying_nothing() {
        // Live: asked what it had done while the person was out, the butler reproduced
        // word for word the answer it had given the previous day, with the day's real
        // record in its context.
        let said = "I checked, but no specific activities were recorded while you were out.";
        let repeat = ButlerReply {
            text: said.to_owned(),
            ..ButlerReply::default()
        };
        let conversation = vec![
            TurnMessage::Assistant {
                text: said.to_owned(),
                tool_calls: Vec::new(),
            },
            TurnMessage::User("did you do anything while I was out?".to_owned()),
        ];
        assert!(repeats_its_last_answer(&repeat, &conversation));

        // A different answer is not a repeat, and a short confirmation is allowed to be
        // identical — "Done." twice is not a failure.
        let different = ButlerReply {
            text: "I posted a brief at 11:01 and two actions failed.".to_owned(),
            ..ButlerReply::default()
        };
        assert!(!repeats_its_last_answer(&different, &conversation));
        let terse = ButlerReply {
            text: "Done.".to_owned(),
            ..ButlerReply::default()
        };
        assert!(!repeats_its_last_answer(
            &terse,
            &[TurnMessage::Assistant {
                text: "Done.".to_owned(),
                tool_calls: Vec::new()
            }]
        ));
    }
}

#[cfg(test)]
mod a_night_that_did_nothing {
    use super::*;
    use endora_kernel::Timestamp;

    fn an_intention() -> Intention {
        Intention::form(
            crate::IntentionId::new(1),
            "you want to run more often",
            crate::BeliefId::new(2),
            Timestamp::from_unix_millis(0),
        )
        .expect("a valid intention")
    }

    #[test]
    fn an_apology_is_not_a_nights_work() {
        // Live: Endora's one long-running intention reached five of its seven steps with
        // its stored progress reading "Sorry — I couldn't reach my language model just
        // now." Non-empty, so it counted as a step, was written down as what happened, and
        // was handed to the next night as the thread to pick up.
        let degraded = ButlerReply {
            text: "Sorry — I couldn't reach my language model just now.".to_owned(),
            degraded: true,
            ..ButlerReply::default()
        };
        assert!(
            not_an_answer(&degraded, &ButlerContext::default()),
            "a degraded night must not count as progress"
        );

        // And the step budget is the thing being protected: seven nights is the whole
        // lifetime of an intention, so a night spent on nothing is a seventh of it.
        let mut spent = an_intention();
        for _ in 0..7 {
            spent.progress("looked into it", Timestamp::from_unix_millis(1));
        }
        assert!(
            spent.is_exhausted(),
            "seven steps spends it — which is why one wasted on an apology matters"
        );
    }

    #[test]
    fn a_real_night_still_counts() {
        let real = ButlerReply {
            text: "Found three routes near you under 5km; the river loop looks best.".to_owned(),
            ..ButlerReply::default()
        };
        assert!(!not_an_answer(&real, &ButlerContext::default()));

        let mut going = an_intention();
        going.progress("Found three routes", Timestamp::from_unix_millis(1));
        assert_eq!(going.steps_taken(), 1);
        assert!(going.is_active());
    }
}

#[cfg(test)]
mod its_own_behaviour_is_not_evidence_about_you {
    use super::*;

    #[test]
    fn a_belief_drawn_from_endoras_own_conduct_is_refused() {
        // Live, top of the understanding screen. Endora had been passive because its reach
        // into the house was broken, and concluded the person prefers passivity — then
        // carried that into every later turn as something it knew about them.
        assert!(is_about_endora_itself(
            "I didn't do anything proactive since I rely on your instructions."
        ));
        // A butler that mistakes its own behaviour for evidence reinforces whatever it
        // happens to be doing, including its own faults.
        assert!(is_about_endora_itself("I checked and found nothing"));
    }

    #[test]
    fn evidence_about_the_person_survives_endora_appearing_in_it() {
        // The discriminator is the SUBJECT, not the pronoun. These are all real evidence
        // strings from the same screen, and every one of them is about the person.
        for real in [
            "You asked 'Where did you find this?' after I listed some upcoming events in New York.",
            "you requested temperature in Fahrenheit",
            "your request for an evening briefing suggests this",
            "previous request",
        ] {
            assert!(!is_about_endora_itself(real), "{real}");
        }
    }
}
