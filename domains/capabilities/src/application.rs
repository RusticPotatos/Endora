//! Capabilities application layer — ports for running and configuring skills.

use endora_kernel::{Decision, RepositoryError, Reversibility};

use crate::domain::{Change, Transition, Watched, note_reading};

pub use crate::domain::{
    AutonomyEnvelope, ConfigWrite, McpServer, McpTransport, StandingTrouble, TargetAlias,
    WORTH_SAYING_AFTER_DAYS, WriteKind, not_answering, worth_raising,
};

/// An optional **deep model** — a bigger/cloud AI the person configures for hard
/// questions the local model can't handle well (like a phone escalating to a bigger
/// brain). Off unless configured. The key is a secret, stored server-side and never
/// returned to a client.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeepModel {
    /// OpenAI-compatible base URL (`.../v1`).
    pub url: String,
    /// Model name to request.
    pub model: String,
    /// API key sent as a bearer token (empty for keyless/local endpoints).
    pub api_key: String,
    /// Whether Endora may reach this model **on its own** — falling back when the local one
    /// fails a deterministic check, and wording the daily brief (ADR 0055).
    ///
    /// **Off by default, and it stays a decision rather than an optimisation.** The deep
    /// model is usually somebody else's API, and until now it was reached only when the
    /// person pressed a button — so every use of it was a choice to send that conversation
    /// off the box. Making the fallback automatic without asking would quietly convert a
    /// local butler into one that phones out whenever the small model stumbles, which it
    /// does often. Reliability is worth a lot; it is not worth deciding this for someone.
    pub escalate: bool,
}

/// Persists the single [`DeepModel`] configuration.
pub trait DeepModelRepository {
    /// Returns the configured deep model, or `None` if unset.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<Option<DeepModel>, RepositoryError>;

    /// Stores the deep model configuration.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, model: &DeepModel) -> Result<(), RepositoryError>;
}

/// Sampling parameters for one model call. Every field is optional — a `None`
/// leaves that knob to the endpoint's own default. `temperature` and `top_p` are
/// standard OpenAI-compatible fields honoured everywhere; `top_k` and
/// `repeat_penalty` are non-standard extensions honoured by local runtimes
/// (Ollama) but rejected by strict cloud endpoints, so providers that need them
/// off leave them unset. See ADR 0055 — the discovery loop tunes these per slot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sampling {
    /// Randomness. Lower = more deterministic. Router wants this cold (~0.0–0.2)
    /// for reliable skill selection; the synthesizer wants it warmer for prose.
    pub temperature: Option<f64>,
    /// Nucleus sampling cutoff.
    pub top_p: Option<f64>,
    /// Top-k cutoff (Ollama/local only).
    pub top_k: Option<u32>,
    /// Repetition penalty (Ollama/local only).
    pub repeat_penalty: Option<f64>,
}

/// One model "slot": the model name plus its sampling parameters. The base URL
/// and API key live once on the parent [`ButlerModelConfig`] (all slots share an
/// endpoint).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelSlot {
    /// Model name to request (e.g. `qwen2.5:7b`, `gpt-4o-mini`).
    pub model: String,
    /// Sampling parameters for this slot.
    pub sampling: Sampling,
}

/// The butler's model configuration, editable at runtime from the console
/// (ADR 0055). Shared endpoint + key; either a single model or the router +
/// synthesizer mixture. The key is a secret stored server-side and never
/// returned to a client. When unset, the node falls back to its environment
/// configuration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ButlerModelConfig {
    /// OpenAI-compatible base URL (`.../v1`) shared by every slot.
    pub base_url: String,
    /// API key sent as a bearer token (empty for keyless/local endpoints).
    pub api_key: String,
    /// `true` runs the router + synthesizer mixture; `false` a single model.
    pub mixture: bool,
    /// The single-model slot (used when `mixture` is false).
    pub single: ModelSlot,
    /// The router slot — a tool-tuned specialist that picks skills.
    pub router: ModelSlot,
    /// The synthesizer slot — a generalist that writes the reply.
    pub synth: ModelSlot,
}

/// A schedule for the self-improving model tune (ADR 0055) — off by default.
/// When on, the heartbeat runs the local-model evaluation + gated adoption once a
/// day at `hour_utc`; pick an off-hour so the eval doesn't contend with chat on
/// the GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTuneSchedule {
    /// Whether the nightly tune is on.
    pub enabled: bool,
    /// The UTC hour (0–23) to run it.
    pub hour_utc: u8,
    /// When it last ran (so it fires once per day).
    pub last_ms: i64,
}

impl ModelTuneSchedule {
    /// Off, defaulting to 4am UTC — a quiet hour.
    #[must_use]
    pub const fn disabled_default() -> Self {
        Self {
            enabled: false,
            hour_utc: 4,
            last_ms: 0,
        }
    }

    /// Whether the tune is due: enabled, the current UTC hour matches, and it
    /// hasn't run in the last ~20h (so it fires once per day).
    #[must_use]
    pub fn is_due(&self, now_ms: i64) -> bool {
        if !self.enabled {
            return false;
        }
        let hour = (now_ms.div_euclid(3_600_000) % 24) as u8;
        hour == self.hour_utc && (now_ms - self.last_ms) >= 20 * 60 * 60 * 1_000
    }
}

/// Persists the single [`ModelTuneSchedule`].
pub trait ModelTuneScheduleRepository {
    /// Returns the schedule, defaulting to off when unset.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<ModelTuneSchedule, RepositoryError>;

    /// Stores the schedule.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, schedule: &ModelTuneSchedule) -> Result<(), RepositoryError>;
}

/// Persists the single [`ButlerModelConfig`].
pub trait ButlerModelConfigRepository {
    /// Returns the configured butler models, or `None` if unset (use the
    /// environment configuration).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn get(&self) -> Result<Option<ButlerModelConfig>, RepositoryError>;

    /// Stores the butler model configuration.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set(&self, config: &ButlerModelConfig) -> Result<(), RepositoryError>;
}

/// Persists the person's [`AutonomyEnvelope`] (ADR 0051).
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

/// Persists per-capability **settings** — the values a skill needs to run (a model
/// name, an API key, a URL), keyed by capability id then setting key (ADR 0054).
/// Secrets live only here and are never echoed back to clients.
pub trait CapabilitySettingsRepository {
    /// All stored settings, as `(capability_id, key, value)` triples.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn all_settings(&self) -> Result<Vec<(String, String, String)>, RepositoryError>;

    /// Sets one setting value for a capability (upsert).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_setting(
        &self,
        capability_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), RepositoryError>;
}

/// The one stored word of a tool's permission (ADR 0062): may it run, and how.
///
/// Everything else about permission is **derived, never stored** — the band supplies the
/// default, the record supplies graduation, the envelope supplies the ceiling. Eight stored
/// axes used to answer this one question, and every bug in the permission model was two of
/// them disagreeing. Two axes cannot disagree when there is one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stance {
    /// Visible but blocked. The default for the un-undoable and the unproven.
    Off,
    /// Runs after the person confirms, each time.
    Ask,
    /// Runs on its own — within the envelope, and only where the band or the record
    /// justifies it.
    Auto,
}

impl Stance {
    /// The stored word for this stance.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Ask => "ask",
            Self::Auto => "auto",
        }
    }

    /// Reads a stored word back, refusing anything unrecognised — a permission that
    /// deserialises leniently is a permission that widens by typo.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "off" => Some(Self::Off),
            "ask" => Some(Self::Ask),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Persists the person's per-tool stance (ADR 0062). Only overrides are stored — a tool
/// with no row keeps its band's default.
pub trait CapabilityConfigRepository {
    /// Every stored stance, as `(id, stance)` pairs.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn stances(&self) -> Result<Vec<(String, Stance)>, RepositoryError>;

    /// Sets a tool's stance (upsert by id).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_stance(&self, id: &str, stance: Stance) -> Result<(), RepositoryError>;

    /// Removes a stored stance, returning the tool to its band's default.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn clear_stance(&self, id: &str) -> Result<(), RepositoryError>;
}

/// Stores what the person said a server's targets are really called (ADR 0054).
/// The log of changes Endora has made to services' own configuration (ADR 0054).
///
/// Append-only in spirit: a write is recorded, and undoing marks it rather than deleting
/// it. What Endora changed about someone's house is not something it should be able to
/// make disappear.
pub trait ConfigWriteLog {
    /// Records a change, with the prior value that undoes it.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn record(&self, write: &ConfigWrite) -> Result<(), RepositoryError>;

    /// The most recent changes, newest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn writes(&self, limit: usize) -> Result<Vec<ConfigWrite>, RepositoryError>;

    /// One change by id.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn write(&self, id: u128) -> Result<Option<ConfigWrite>, RepositoryError>;

    /// Marks a change as put back. The row stays.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn mark_undone(&self, id: u128) -> Result<(), RepositoryError>;
}

/// What is currently wrong in the services, and since when (ADR 0056).
///
/// Deliberately not a log. There is one row per thing-that-is-wrong-right-now, created the
/// first time it is seen and **deleted the moment it is well again** — so the store is
/// bounded by the state of the world rather than by how long Endora has been watching it.
pub trait StandingTroubleRepository {
    /// Records that something is in trouble, keeping the earliest sighting. Called every
    /// time it is seen that way; only the first one sets the clock.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn note_trouble(&self, trouble: &StandingTrouble) -> Result<(), RepositoryError>;

    /// Forgets a thing that is well again — including anything the person had accepted,
    /// because a device that comes back is a different situation from one that never did.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn clear_trouble(&self, server: &str, thing: &str) -> Result<(), RepositoryError>;

    /// Everything currently wrong, oldest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn troubles(&self) -> Result<Vec<StandingTrouble>, RepositoryError>;

    /// Marks one as the person's business rather than a problem, so it stops being raised.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn accept_trouble(&self, server: &str, thing: &str) -> Result<(), RepositoryError>;
}

/// Stores what the person said a server's targets are really called (ADR 0054).
pub trait TargetAliasRepository {
    /// Every alias, so the turn can be grounded in all of them.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails or stored data is corrupt.
    fn aliases(&self) -> Result<Vec<TargetAlias>, RepositoryError>;

    /// Records one, replacing any earlier answer for the same server and wording — the
    /// person may correct themselves, and the latest word wins.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_alias(&self, alias: &TargetAlias) -> Result<(), RepositoryError>;

    /// Forgets one. Memory rights apply to what Endora knows about its tools too.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn forget_alias(&self, server: &str, said: &str) -> Result<(), RepositoryError>;
}

/// Persists the **MCP servers** the catalog draws tools from (ADR 0054). The stored
/// rows are plain configuration; adding one is a gated capability (deny-by-default),
/// and every tool a server exposes is still band-classified before it can run — an
/// unknown tool is treated as irreversible and blocked (ADR 0051). Servers are keyed
/// by [`McpServer::name`], which also namespaces their tools (`name.tool`).
pub trait McpServerRegistry {
    /// All registered servers, enabled or not.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn list(&self) -> Result<Vec<McpServer>, RepositoryError>;

    /// Adds a server or replaces the one with the same name (upsert), persisting its
    /// transport and enabled flag.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn register(&self, server: &McpServer) -> Result<(), RepositoryError>;

    /// Switches a server on or off by name, leaving its transport untouched. A no-op
    /// if no server has that name.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), RepositoryError>;

    /// Sets the auto-allow flag for a server by name, leaving everything else
    /// untouched. A no-op if no server has that name. Enforcement (opening the tools)
    /// happens on the next connect, from this stored flag.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn set_trust_all(&self, name: &str, trust_all: bool) -> Result<(), RepositoryError>;

    /// Removes a server by name (idempotent — removing an absent name is fine).
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn remove(&self, name: &str) -> Result<(), RepositoryError>;
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
    /// The JSON-Schema for the skill's input as a JSON string, when known (MCP tools
    /// advertise one via `tools/list`). Lets the model layer offer the skill through
    /// the endpoint's native tool-calling API — exact name + schema-validated
    /// arguments — instead of a hand-written id the weak local model mis-emits. Kept
    /// as text so it crosses context boundaries without a serde dependency. `None` for
    /// built-ins that describe their inputs in the prompt instead.
    pub input_schema: Option<String>,
    /// How undoable this capability's effect is. Surfaced to the application so the
    /// turn can tell an **observation** from a **receipt**: a capability in the
    /// [`Reversibility::Observe`] band reports state, so its result *is* evidence,
    /// while anything else returns the actuator's claim about what it did — which
    /// may be untrue (ADR 0053).
    pub reversibility: Reversibility,
}

/// Runs the butler's skills. The application asks this port to execute a
/// capability the butler proposed, keeping the model out of the execution path
/// (models propose, policy authorizes, capabilities execute — ADRs 0019/0020).
pub trait CapabilityRunner {
    /// The skills currently available (for grounding the butler).
    fn available(&self) -> Vec<CapabilitySpec>;

    /// What the person's own services can say about them **right now** — one short line
    /// each, e.g. `john is not home`.
    ///
    /// Live state, not a belief: it is true this minute and worthless tomorrow, which is
    /// why it goes into the turn's context rather than into understanding. A butler that
    /// does not know whether someone is in the house is guessing every time it decides
    /// whether to speak.
    ///
    /// Empty by default. A service that has nothing to say about the person says nothing.
    fn about_the_person(&self) -> Vec<String> {
        Vec::new()
    }

    /// What the services currently say is true, as `(name, state)` — the facts an answer
    /// about state should agree with (ADR 0053).
    ///
    /// Verification covers what Endora **did**: read the world before and after, compare,
    /// record both. It has never covered what Endora **says**. Yet on a turn that answers
    /// a question, Endora is holding the very reading the answer came from, so the facts
    /// behind the prose are available and were simply never shown.
    ///
    /// Empty by default, and empty for any service that cannot be asked.
    fn current_states(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Runs a capability with JSON input, returning its JSON output or an error
    /// message. Only ever called for capabilities the policy layer has cleared.
    fn run(&self, id: &str, input_json: &str) -> Result<String, String>;

    /// The capability that **observes what this one changes**, if any — the read
    /// used to verify an actuation (ADR 0053).
    ///
    /// Endora's architecture says *evidence verifies*. Without this, the turn can
    /// only report what an actuator claimed about its own work, which is exactly the
    /// thing that can be untrue. A source that knows one of its capabilities reads
    /// the same state another writes returns that reader's id here, and the turn
    /// looks at the world instead of taking the tool's word.
    ///
    /// This is **one mapping per integration**, not one per tool: an MCP server that
    /// exposes a state reader can name it for every action it hosts.
    ///
    /// The default is `None` — nothing can be verified, so results stay marked
    /// unverified. Honest for integrations nobody has taught Endora about.
    fn verifier(&self, _id: &str) -> Option<String> {
        None
    }

    /// The arguments to call [`verifier`](Self::verifier) with, so the read-back is
    /// scoped to **what the action was aimed at**.
    ///
    /// Observed live: asked to turn off the kitchen main, the turn read state back with
    /// no arguments, got every device in the house, and the butler answered about the
    /// *garage* — the first thing in the dump that happened to be on. An observation
    /// that wide does not verify an action, it buries it.
    ///
    /// The default is `"{}"` — the whole reading, which is what a source that cannot
    /// match up schemas should do. A source that knows both schemas narrows it by
    /// passing along the targeting arguments the reader shares with the action. Nothing
    /// in that rule names an integration: it is schema against schema.
    fn read_back_input(&self, _action_id: &str, _action_input: &str) -> String {
        "{}".to_owned()
    }

    /// The deterministic policy [`Decision`] for a capability by id (ADRs
    /// 0005/0024) — what policy does with it: [`Act`](Decision::Act) on its own,
    /// [`Confirm`](Decision::Confirm) first, or [`Block`](Decision::Block) outright.
    /// `None` if there is no such skill.
    ///
    /// The default derives a coarse verdict from [`CapabilitySpec::autonomous`]
    /// (act, else confirm); a runner that classifies reversibility bands overrides
    /// this to report [`Block`](Decision::Block) for the un-undoable.
    fn decision(&self, id: &str) -> Option<Decision> {
        self.available().into_iter().find(|s| s.id == id).map(|s| {
            if s.autonomous {
                Decision::Act
            } else {
                Decision::Confirm
            }
        })
    }
}

/// Brings the record of what is wrong into line with a fresh reading of one service
/// (ADR 0056).
///
/// Called with everything the service just reported. Anything that is not answering gets
/// its clock started (or left alone, if it was already running); **everything else is
/// cleared**, which is what keeps the store equal to the present rather than to a history.
/// A thing that recovers leaves nothing behind — including any answer the person had given
/// about it, because a device that comes back is a different situation from one that never
/// did.
///
/// Deliberately takes the reading rather than fetching it: the caller already has one, and
/// this way the whole rule is testable without a service.
///
/// How long the transition log keeps what it saw.
///
/// A fortnight, matching how long a notion may go unsupported (ADR 0057): the window a thought
/// can be built from and the window it can die in are the same span, so a notion is never
/// starved by a log that could not remember far enough back.
pub const KEEP_TRANSITIONS_FOR_MS: i64 = 14 * 24 * 60 * 60 * 1_000;

/// Where the transition log lives (ADR 0058).
pub trait TransitionLog {
    /// What Endora is currently watching, and what each thing last settled on.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn watching(&self) -> Result<Vec<Watched>, RepositoryError>;

    /// Stores what is known about one thing, replacing any earlier row for it.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn remember(&self, watched: &Watched) -> Result<(), RepositoryError>;

    /// Writes down a change that really happened.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn record(&self, transition: &Transition) -> Result<(), RepositoryError>;

    /// Every change recorded at or after a moment, most recent first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn since(&self, ms: i64) -> Result<Vec<Transition>, RepositoryError>;

    /// Drops everything older than a moment.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    fn forget_before(&self, ms: i64) -> Result<(), RepositoryError>;
}

/// Notes one pass over a server's readings, returning the changes that really happened.
///
/// The counterpart to [`watch_for_trouble`], and the reason the watch loop is worth more than
/// it was: that one only ever asks *"is this thing answering?"*, so a house could change all
/// day and Endora would carry nothing out of it. This asks *"did something move?"* and keeps
/// the answer.
///
/// A change is only written down once it has held for [`crate::domain::DWELL_MS`] — see
/// [`note_reading`](crate::domain::note_reading), where all of that judgement lives
/// and is tested.
///
/// **Prunes on every pass**, so the retention window is a guarantee rather than a hope and
/// nothing accumulates for anybody to clear.
///
/// # Errors
/// [`RepositoryError`] if the backend fails.
pub fn watch_for_change(
    log: &impl TransitionLog,
    server: &str,
    reading: &[(String, String)],
    now_ms: i64,
) -> Result<Vec<Transition>, RepositoryError> {
    let watching = log.watching()?;
    let mut moved = Vec::new();
    for (thing, state) in reading {
        // Namespaced, because a network controller and a home hub may both call something
        // "phone" and they are not the same thing.
        let key = format!("{server}::{thing}");
        let prior = watching.iter().find(|w| w.key == key);
        match note_reading(prior, &key, state, now_ms) {
            Change::Nothing => {}
            Change::Noted(now) => log.remember(&now)?,
            Change::Moved {
                from,
                to,
                at_ms,
                now,
            } => {
                log.remember(&now)?;
                let transition = Transition {
                    key,
                    from,
                    to,
                    at_ms,
                };
                log.record(&transition)?;
                moved.push(transition);
            }
        }
    }
    log.forget_before(now_ms - KEEP_TRANSITIONS_FOR_MS)?;
    Ok(moved)
}

/// # Errors
/// [`RepositoryError`] if the backend fails.
pub fn watch_for_trouble(
    repo: &impl StandingTroubleRepository,
    server: &str,
    reading: &[(String, String)],
    now_ms: i64,
) -> Result<(), RepositoryError> {
    for (thing, state) in reading {
        if not_answering(state) {
            repo.note_trouble(&StandingTrouble {
                server: server.to_owned(),
                thing: thing.clone(),
                trouble: state.trim().to_lowercase(),
                since_ms: now_ms,
                accepted: false,
            })?;
            continue;
        }
        repo.clear_trouble(server, thing)?;
    }
    Ok(())
}

#[cfg(test)]
mod keeping_a_record_of_what_moved {
    //! `watch_for_change` (ADR 0058). Its sibling `watch_for_trouble` has no tests; this one
    //! does, because a transition log that lies quietly is worse than no log — everything
    //! downstream inherits the noise.

    use super::{TransitionLog, watch_for_change};
    use crate::domain::{DWELL_MS, Transition, Watched};
    use endora_kernel::RepositoryError;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeLog {
        watching: RefCell<Vec<Watched>>,
        moved: RefCell<Vec<Transition>>,
        pruned_before: RefCell<Option<i64>>,
    }

    impl TransitionLog for FakeLog {
        fn watching(&self) -> Result<Vec<Watched>, RepositoryError> {
            Ok(self.watching.borrow().clone())
        }
        fn remember(&self, w: &Watched) -> Result<(), RepositoryError> {
            let mut all = self.watching.borrow_mut();
            match all.iter_mut().find(|x| x.key == w.key) {
                Some(existing) => *existing = w.clone(),
                None => all.push(w.clone()),
            }
            Ok(())
        }
        fn record(&self, t: &Transition) -> Result<(), RepositoryError> {
            self.moved.borrow_mut().push(t.clone());
            Ok(())
        }
        fn since(&self, ms: i64) -> Result<Vec<Transition>, RepositoryError> {
            Ok(self
                .moved
                .borrow()
                .iter()
                .filter(|t| t.at_ms >= ms)
                .cloned()
                .collect())
        }
        fn forget_before(&self, ms: i64) -> Result<(), RepositoryError> {
            *self.pruned_before.borrow_mut() = Some(ms);
            self.moved.borrow_mut().retain(|t| t.at_ms >= ms);
            Ok(())
        }
    }

    fn reading(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    #[test]
    fn the_first_look_at_a_house_records_nothing_but_remembers_everything() {
        // Otherwise Endora's first tick writes a transition for every entity in the house,
        // and not one of them happened.
        let log = FakeLog::default();
        let moved = watch_for_change(
            &log,
            "house",
            &reading(&[("light.kitchen", "on"), ("person.john", "home")]),
            1_000,
        )
        .unwrap();
        assert!(moved.is_empty());
        assert_eq!(log.watching.borrow().len(), 2);
    }

    #[test]
    fn a_change_that_holds_is_recorded_once_and_not_again() {
        let log = FakeLog::default();
        watch_for_change(&log, "house", &reading(&[("person.john", "home")]), 0).unwrap();
        // It changes, but has not held yet.
        let moved = watch_for_change(
            &log,
            "house",
            &reading(&[("person.john", "not_home")]),
            1_000,
        )
        .unwrap();
        assert!(moved.is_empty(), "not yet settled");

        // It holds.
        let moved = watch_for_change(
            &log,
            "house",
            &reading(&[("person.john", "not_home")]),
            1_000 + DWELL_MS,
        )
        .unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].from, "home");
        assert_eq!(moved[0].to, "not_home");
        assert_eq!(moved[0].at_ms, 1_000, "when it happened");

        // A later pass with the same reading must not record it a second time.
        let moved = watch_for_change(
            &log,
            "house",
            &reading(&[("person.john", "not_home")]),
            9_000_000,
        )
        .unwrap();
        assert!(moved.is_empty());
        assert_eq!(log.moved.borrow().len(), 1);
    }

    #[test]
    fn a_phone_that_flaps_produces_no_record_at_all() {
        // The reason the dwell exists. Wi-Fi drops the device and picks it straight back up.
        let log = FakeLog::default();
        watch_for_change(&log, "house", &reading(&[("device.phone", "home")]), 0).unwrap();
        for (state, at) in [("not_home", 1_000), ("home", 60_000), ("not_home", 61_000)] {
            watch_for_change(&log, "house", &reading(&[("device.phone", state)]), at).unwrap();
        }
        assert!(
            log.moved.borrow().is_empty(),
            "nothing held long enough to have happened"
        );
    }

    #[test]
    fn two_servers_cannot_collide_on_a_name() {
        // Both a network controller and a home hub may call something "phone".
        let log = FakeLog::default();
        watch_for_change(&log, "house", &reading(&[("phone", "home")]), 0).unwrap();
        watch_for_change(&log, "network", &reading(&[("phone", "away")]), 0).unwrap();
        let keys: Vec<String> = log
            .watching
            .borrow()
            .iter()
            .map(|w| w.key.clone())
            .collect();
        assert!(keys.contains(&"house::phone".to_owned()));
        assert!(keys.contains(&"network::phone".to_owned()));
    }

    #[test]
    fn the_log_forgets_on_its_own() {
        // Bounded by the retention window rather than by uptime, the same argument that made
        // standing trouble safe to store. Nothing here needs anybody to clear it.
        let log = FakeLog::default();
        watch_for_change(
            &log,
            "house",
            &reading(&[("light.kitchen", "on")]),
            5_000_000,
        )
        .unwrap();
        assert_eq!(
            *log.pruned_before.borrow(),
            Some(5_000_000 - super::KEEP_TRANSITIONS_FOR_MS)
        );
    }
}
