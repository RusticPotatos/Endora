//! Use cases for the Direction & Targets slice.
//!
//! These orchestrate the domain and the ports. They are the seam the interfaces
//! (node, CLI) call; they hold no transport or storage detail. Identifiers and
//! time come from the [`IdSource`] and [`Clock`] ports, so the domain stays pure
//! and the use cases stay testable with fakes.

use endora_domain::{
    Assumption, AssumptionId, AuditId, AuditRecord, AutonomyLevel, Belief, BeliefId, ChatMessage,
    Direction, DirectionId, Experiment, ExperimentId, LifecycleStatus, MessageId, MessageRole,
    Observation, ObservationId, PolicyDecision, Preference, PreferenceId, PreferenceKind,
    ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId, SuggestionId, Target,
    TargetId, Timestamp, Value, ValueId, authorize_process_change,
};

use crate::error::AppError;
use crate::ports::{
    AssumptionRepository, AttentionItem, AttentionKind, AuditLog, BeliefRepository, BriefSchedule,
    BriefScheduleRepository, Butler, ButlerContext, ButlerProposal, ButlerReply, CapabilityRunner,
    ChatRepository, CheckinRepository, CheckinSchedule, Clock, DirectionRepository, EventLog,
    ExperimentRepository, IdSource, MemorySnapshot, MemoryStore, NorthStarBrief,
    ObservationRepository, PreferenceRepository, ProcessChangeRepository, Proposer,
    ReflectionRepository, Snooze, SnoozeRepository, Suggestion, SuggestionRepository,
    SuggestionStatus, TargetRepository, ValueRepository,
};

/// Creates and stores a new [`Direction`].
///
/// # Errors
/// [`AppError::Domain`] if the title is invalid, or [`AppError::Repository`] if
/// persistence fails.
pub fn create_direction(
    directions: &impl DirectionRepository,
    ids: &impl IdSource,
    title: &str,
) -> Result<Direction, AppError> {
    let direction = Direction::new(DirectionId::new(ids.new_id()), title)?;
    directions.save(&direction)?;
    Ok(direction)
}

/// Creates and stores a new [`Value`] (the "why" a North Star serves).
///
/// # Errors
/// [`AppError::Domain`] if the name is invalid, or [`AppError::Repository`] if
/// persistence fails.
pub fn create_value(
    values: &impl ValueRepository,
    ids: &impl IdSource,
    name: &str,
) -> Result<Value, AppError> {
    let value = Value::new(ValueId::new(ids.new_id()), name)?;
    values.save(&value)?;
    Ok(value)
}

/// Lists all values, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_values(values: &impl ValueRepository) -> Result<Vec<Value>, AppError> {
    Ok(values.list_all()?)
}

/// Permanently deletes a value, refusing while any North Star still serves it.
///
/// # Errors
/// [`AppError::NotFound`] if the value does not exist, [`AppError::BadRequest`] if
/// a North Star still references it, or [`AppError::Repository`] on failure.
pub fn delete_value(
    values: &impl ValueRepository,
    directions: &impl DirectionRepository,
    id: ValueId,
) -> Result<(), AppError> {
    if values.get(id)?.is_none() {
        return Err(AppError::NotFound { entity: "value" });
    }
    if directions.list_all()?.iter().any(|d| d.value() == Some(id)) {
        return Err(AppError::BadRequest {
            message: "cannot delete a value while North Stars still serve it; \
                      re-file those North Stars first"
                .to_owned(),
        });
    }
    values.delete(id)?;
    Ok(())
}

/// Files a North Star under a value (or clears it with `None`).
///
/// The person — or the butler, by asking — sets this; it is never inferred.
///
/// # Errors
/// [`AppError::NotFound`] if the direction or the named value does not exist, or
/// [`AppError::Repository`] if persistence fails.
pub fn assign_direction_value(
    directions: &impl DirectionRepository,
    values: &impl ValueRepository,
    direction_id: DirectionId,
    value_id: Option<ValueId>,
) -> Result<Direction, AppError> {
    let mut direction = directions.get(direction_id)?.ok_or(AppError::NotFound {
        entity: "direction",
    })?;
    if let Some(v) = value_id {
        if values.get(v)?.is_none() {
            return Err(AppError::NotFound { entity: "value" });
        }
    }
    direction.set_value(value_id);
    directions.save(&direction)?;
    Ok(direction)
}

/// Creates and stores a new [`Target`] under an existing direction.
///
/// # Errors
/// [`AppError::NotFound`] if the direction does not exist, [`AppError::Domain`]
/// if the statement is invalid, or [`AppError::Repository`] if persistence fails.
pub fn create_target(
    directions: &impl DirectionRepository,
    targets: &impl TargetRepository,
    ids: &impl IdSource,
    direction: DirectionId,
    statement: &str,
) -> Result<Target, AppError> {
    if directions.get(direction)?.is_none() {
        return Err(AppError::NotFound {
            entity: "direction",
        });
    }
    let target = Target::new(TargetId::new(ids.new_id()), direction, statement)?;
    targets.save(&target)?;
    Ok(target)
}

/// Lists all directions, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_directions(directions: &impl DirectionRepository) -> Result<Vec<Direction>, AppError> {
    Ok(directions.list_all()?)
}

/// Lists the targets under a direction, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_targets(
    targets: &impl TargetRepository,
    direction: DirectionId,
) -> Result<Vec<Target>, AppError> {
    Ok(targets.list_for_direction(direction)?)
}

/// Sets a direction's lifecycle status (achieve, abandon, archive, or reopen).
///
/// # Errors
/// [`AppError::NotFound`] if the direction does not exist, or
/// [`AppError::Repository`] if persistence fails.
pub fn set_direction_status(
    directions: &impl DirectionRepository,
    id: DirectionId,
    status: LifecycleStatus,
) -> Result<Direction, AppError> {
    let mut direction = directions.get(id)?.ok_or(AppError::NotFound {
        entity: "direction",
    })?;
    direction.set_status(status);
    directions.save(&direction)?;
    Ok(direction)
}

/// Permanently deletes a direction, refusing if any targets still hang off it.
///
/// Deletion is irreversible; archiving is the reversible alternative. A direction
/// with dependents must have them removed (or be archived instead) first.
///
/// # Errors
/// [`AppError::NotFound`] if the direction does not exist, [`AppError::BadRequest`]
/// if it still has targets, or [`AppError::Repository`] if persistence fails.
pub fn delete_direction(
    directions: &impl DirectionRepository,
    targets: &impl TargetRepository,
    id: DirectionId,
) -> Result<(), AppError> {
    if directions.get(id)?.is_none() {
        return Err(AppError::NotFound {
            entity: "direction",
        });
    }
    if !targets.list_for_direction(id)?.is_empty() {
        return Err(AppError::BadRequest {
            message: "cannot delete a North Star that still has targets; \
                      archive it, or remove its targets first"
                .to_owned(),
        });
    }
    directions.delete(id)?;
    Ok(())
}

/// Sets a target's lifecycle status (achieve, abandon, archive, or reopen).
///
/// # Errors
/// [`AppError::NotFound`] if the target does not exist, or
/// [`AppError::Repository`] if persistence fails.
pub fn set_target_status(
    targets: &impl TargetRepository,
    id: TargetId,
    status: LifecycleStatus,
) -> Result<Target, AppError> {
    let mut target = targets
        .get(id)?
        .ok_or(AppError::NotFound { entity: "target" })?;
    target.set_status(status);
    targets.save(&target)?;
    Ok(target)
}

/// Permanently deletes a target, refusing if any assumptions still hang off it.
///
/// Deletion is irreversible; archiving is the reversible alternative.
///
/// # Errors
/// [`AppError::NotFound`] if the target does not exist, [`AppError::BadRequest`]
/// if it still has assumptions, or [`AppError::Repository`] if persistence fails.
pub fn delete_target(
    targets: &impl TargetRepository,
    assumptions: &impl AssumptionRepository,
    id: TargetId,
) -> Result<(), AppError> {
    if targets.get(id)?.is_none() {
        return Err(AppError::NotFound { entity: "target" });
    }
    if !assumptions.list_for_target(id)?.is_empty() {
        return Err(AppError::BadRequest {
            message: "cannot delete a target that still has assumptions; \
                      archive it, or remove its assumptions first"
                .to_owned(),
        });
    }
    targets.delete(id)?;
    Ok(())
}

/// Creates and stores a new [`Assumption`] under an existing target.
///
/// # Errors
/// [`AppError::NotFound`] if the target does not exist, [`AppError::Domain`] if
/// the statement is invalid, or [`AppError::Repository`] if persistence fails.
pub fn create_assumption(
    targets: &impl TargetRepository,
    assumptions: &impl AssumptionRepository,
    ids: &impl IdSource,
    target: TargetId,
    statement: &str,
) -> Result<Assumption, AppError> {
    if targets.get(target)?.is_none() {
        return Err(AppError::NotFound { entity: "target" });
    }
    let assumption = Assumption::new(AssumptionId::new(ids.new_id()), target, statement)?;
    assumptions.save(&assumption)?;
    Ok(assumption)
}

/// Lists the assumptions under a target, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_assumptions(
    assumptions: &impl AssumptionRepository,
    target: TargetId,
) -> Result<Vec<Assumption>, AppError> {
    Ok(assumptions.list_for_target(target)?)
}

/// Proposes and stores a new [`Experiment`] under an existing assumption.
///
/// # Errors
/// [`AppError::NotFound`] if the assumption does not exist, [`AppError::Domain`]
/// if the hypothesis is invalid, or [`AppError::Repository`] if persistence fails.
pub fn propose_experiment(
    assumptions: &impl AssumptionRepository,
    experiments: &impl ExperimentRepository,
    ids: &impl IdSource,
    assumption: AssumptionId,
    hypothesis: &str,
) -> Result<Experiment, AppError> {
    if assumptions.get(assumption)?.is_none() {
        return Err(AppError::NotFound {
            entity: "assumption",
        });
    }
    let experiment = Experiment::propose(ExperimentId::new(ids.new_id()), assumption, hypothesis)?;
    experiments.save(&experiment)?;
    Ok(experiment)
}

/// Lists the experiments testing an assumption, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_experiments(
    experiments: &impl ExperimentRepository,
    assumption: AssumptionId,
) -> Result<Vec<Experiment>, AppError> {
    Ok(experiments.list_for_assumption(assumption)?)
}

/// Starts a proposed experiment and persists the transition.
///
/// # Errors
/// [`AppError::NotFound`] if the experiment does not exist, [`AppError::Domain`]
/// if it is not in a startable state, or [`AppError::Repository`] on failure.
pub fn start_experiment(
    experiments: &impl ExperimentRepository,
    id: ExperimentId,
) -> Result<Experiment, AppError> {
    let mut experiment = experiments.get(id)?.ok_or(AppError::NotFound {
        entity: "experiment",
    })?;
    experiment.start()?;
    experiments.save(&experiment)?;
    Ok(experiment)
}

/// Concludes a running experiment and persists the transition.
///
/// # Errors
/// [`AppError::NotFound`] if the experiment does not exist, [`AppError::Domain`]
/// if it is not running, or [`AppError::Repository`] on failure.
pub fn conclude_experiment(
    experiments: &impl ExperimentRepository,
    id: ExperimentId,
) -> Result<Experiment, AppError> {
    let mut experiment = experiments.get(id)?.ok_or(AppError::NotFound {
        entity: "experiment",
    })?;
    experiment.conclude()?;
    experiments.save(&experiment)?;
    Ok(experiment)
}

/// Number of milliseconds in a day, for scheduling reviews some days out.
const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;

/// Schedules a reminder to review an experiment `in_days` from now, using the
/// [`Clock`] port for the current time.
///
/// This only sets a reminder; it changes no lifecycle state and takes no action
/// on its own (see `docs/adr/0010-autonomy-model.md`).
///
/// # Errors
/// [`AppError::NotFound`] if the experiment does not exist, or
/// [`AppError::Repository`] if persistence fails.
pub fn schedule_experiment_review(
    experiments: &impl ExperimentRepository,
    clock: &impl Clock,
    id: ExperimentId,
    in_days: u32,
) -> Result<Experiment, AppError> {
    let mut experiment = experiments.get(id)?.ok_or(AppError::NotFound {
        entity: "experiment",
    })?;
    let at = Timestamp::from_unix_millis(
        clock.now().unix_millis() + i64::from(in_days) * MILLIS_PER_DAY,
    );
    experiment.schedule_review(at);
    experiments.save(&experiment)?;
    Ok(experiment)
}

/// Lists the experiments whose scheduled review is due as of now, per the
/// [`Clock`] port. These are surfaced to the person; nothing acts on them.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn list_due_reviews(
    experiments: &impl ExperimentRepository,
    clock: &impl Clock,
) -> Result<Vec<Experiment>, AppError> {
    Ok(experiments.list_due_reviews(clock.now())?)
}

/// Records an [`Observation`] against an existing experiment, timestamped by the
/// [`Clock`] port.
///
/// # Errors
/// [`AppError::NotFound`] if the experiment does not exist, [`AppError::Domain`]
/// if the note is invalid, or [`AppError::Repository`] if persistence fails.
pub fn record_observation(
    experiments: &impl ExperimentRepository,
    observations: &impl ObservationRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    experiment: ExperimentId,
    note: &str,
) -> Result<Observation, AppError> {
    if experiments.get(experiment)?.is_none() {
        return Err(AppError::NotFound {
            entity: "experiment",
        });
    }
    let observation = Observation::record(
        ObservationId::new(ids.new_id()),
        experiment,
        note,
        clock.now(),
    )?;
    observations.save(&observation)?;
    Ok(observation)
}

/// Lists the observations recorded for an experiment, oldest first.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_observations(
    observations: &impl ObservationRepository,
    experiment: ExperimentId,
) -> Result<Vec<Observation>, AppError> {
    Ok(observations.list_for_experiment(experiment)?)
}

/// Creates and stores a new [`Reflection`] over one or more observations, under
/// an existing target.
///
/// # Errors
/// [`AppError::NotFound`] if the target does not exist, [`AppError::Domain`] if the
/// summary is blank or no evidence is cited, or [`AppError::Repository`] on
/// failure.
pub fn create_reflection(
    targets: &impl TargetRepository,
    reflections: &impl ReflectionRepository,
    ids: &impl IdSource,
    target: TargetId,
    summary: &str,
    evidence: Vec<ObservationId>,
) -> Result<Reflection, AppError> {
    if targets.get(target)?.is_none() {
        return Err(AppError::NotFound { entity: "target" });
    }
    let reflection = Reflection::new(ReflectionId::new(ids.new_id()), target, summary, evidence)?;
    reflections.save(&reflection)?;
    Ok(reflection)
}

/// Lists the reflections for a target, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_reflections(
    reflections: &impl ReflectionRepository,
    target: TargetId,
) -> Result<Vec<Reflection>, AppError> {
    Ok(reflections.list_for_target(target)?)
}

/// Proposes and stores a new [`ProposedProcessChange`] from an existing
/// reflection. It starts pending human approval.
///
/// # Errors
/// [`AppError::NotFound`] if the reflection does not exist, [`AppError::Domain`]
/// if the description is blank, or [`AppError::Repository`] on failure.
pub fn propose_process_change(
    reflections: &impl ReflectionRepository,
    changes: &impl ProcessChangeRepository,
    ids: &impl IdSource,
    reflection: ReflectionId,
    description: &str,
) -> Result<ProposedProcessChange, AppError> {
    if reflections.get(reflection)?.is_none() {
        return Err(AppError::NotFound {
            entity: "reflection",
        });
    }
    let change = ProposedProcessChange::propose(
        ProcessChangeId::new(ids.new_id()),
        reflection,
        description,
    )?;
    changes.save(&change)?;
    Ok(change)
}

/// Uses a reasoning model to *draft* a process change from an existing
/// reflection, then stores it as a pending proposal.
///
/// The model is a reasoning component, not an authority: its output becomes an
/// ordinary [`ProposedProcessChange`] in the `Pending` state, which still needs
/// human approval and passes through the deterministic policy boundary like any
/// other. See `docs/adr/0005-models-propose-policy-authorizes.md`.
///
/// # Errors
/// [`AppError::NotFound`] if the reflection does not exist, [`AppError::Model`]
/// if the model is unavailable or returns nothing usable, [`AppError::Domain`]
/// if the drafted text is empty, or [`AppError::Repository`] on failure.
pub fn draft_process_change(
    reflections: &impl ReflectionRepository,
    changes: &impl ProcessChangeRepository,
    ids: &impl IdSource,
    proposer: &dyn Proposer,
    reflection: ReflectionId,
) -> Result<ProposedProcessChange, AppError> {
    let Some(reflection_record) = reflections.get(reflection)? else {
        return Err(AppError::NotFound {
            entity: "reflection",
        });
    };
    let description = proposer.propose_process_change(
        reflection_record.summary(),
        reflection_record.evidence().len(),
    )?;
    let change = ProposedProcessChange::propose(
        ProcessChangeId::new(ids.new_id()),
        reflection,
        &description,
    )?;
    changes.save(&change)?;
    Ok(change)
}

/// Lists the proposed changes from a reflection, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_process_changes(
    changes: &impl ProcessChangeRepository,
    reflection: ReflectionId,
) -> Result<Vec<ProposedProcessChange>, AppError> {
    Ok(changes.list_for_reflection(reflection)?)
}

/// Approves a pending process change (an explicit human decision) and persists
/// it.
///
/// # Errors
/// [`AppError::NotFound`] if the change does not exist, [`AppError::Domain`] if
/// it was already decided, or [`AppError::Repository`] on failure.
pub fn approve_process_change(
    changes: &impl ProcessChangeRepository,
    id: ProcessChangeId,
) -> Result<ProposedProcessChange, AppError> {
    let mut change = changes.get(id)?.ok_or(AppError::NotFound {
        entity: "process change",
    })?;
    change.approve()?;
    changes.save(&change)?;
    Ok(change)
}

/// Rejects a pending process change and persists it.
///
/// # Errors
/// [`AppError::NotFound`] if the change does not exist, [`AppError::Domain`] if
/// it was already decided, or [`AppError::Repository`] on failure.
pub fn reject_process_change(
    changes: &impl ProcessChangeRepository,
    id: ProcessChangeId,
) -> Result<ProposedProcessChange, AppError> {
    let mut change = changes.get(id)?.ok_or(AppError::NotFound {
        entity: "process change",
    })?;
    change.reject()?;
    changes.save(&change)?;
    Ok(change)
}

/// Decides whether an actor may enact a proposed process change, and records
/// the decision to the audit trail.
///
/// The decision itself is made by the deterministic domain policy
/// ([`authorize_process_change`]); this use case is the seam that ties that
/// decision to accountability — every consequential decision is audited.
///
/// # Errors
/// [`AppError::Domain`] if the audit summary is somehow invalid, or
/// [`AppError::Repository`] if writing the audit record fails.
pub fn decide_process_change(
    change: &ProposedProcessChange,
    actor: AutonomyLevel,
    ids: &impl IdSource,
    clock: &impl Clock,
    audit: &impl AuditLog,
) -> Result<PolicyDecision, AppError> {
    let decision = authorize_process_change(change, actor);
    let summary = format!(
        "policy {} enacting process change {} (actor: {actor:?})",
        describe(decision),
        change.id().value(),
    );
    let record = AuditRecord::new(AuditId::new(ids.new_id()), clock.now(), &summary)?;
    audit.append(&record)?;
    Ok(decision)
}

/// Loads a stored process change and decides whether `actor` may enact it,
/// recording the decision to the audit trail.
///
/// This is the seam that ties the persisted change, the deterministic policy,
/// and accountability together — the interface-facing form of
/// [`decide_process_change`].
///
/// # Errors
/// [`AppError::NotFound`] if the change does not exist, or [`AppError`] if the
/// decision cannot be recorded.
pub fn decide_stored_process_change(
    changes: &impl ProcessChangeRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    audit: &impl AuditLog,
    id: ProcessChangeId,
    actor: AutonomyLevel,
) -> Result<PolicyDecision, AppError> {
    let change = changes.get(id)?.ok_or(AppError::NotFound {
        entity: "process change",
    })?;
    decide_process_change(&change, actor, ids, clock, audit)
}

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
pub fn recent_audit(audit: &impl AuditLog, limit: usize) -> Result<Vec<AuditRecord>, AppError> {
    Ok(audit.recent(limit)?)
}

/// What kind of thing an [`ActivityItem`] records.
///
/// Kept coarse on purpose: the feed groups by the area of the loop, and the
/// human-readable summary carries the specifics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    /// An observation was recorded against an experiment.
    Observation,
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
            Self::Observation => "observation",
            Self::Decision => "decision",
            Self::Action => "action",
        }
    }
}

/// One entry in the activity feed.
///
/// This is a **read projection**, not a domain aggregate: it merges the
/// persisted facts that already carry a time — recorded observations and audited
/// decisions — into a single "what happened" timeline. Because it is derived, it
/// stores nothing new and needs no schema of its own.
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

/// Returns the most recent activity across the learning loop, newest first, up
/// to `limit` entries.
///
/// The feed is a projection over what is already persisted with a timestamp:
/// recorded observations and audited decisions. As more of the loop gains
/// durable timestamps, this timeline widens without a protocol change.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn recent_activity(
    observations: &impl ObservationRepository,
    audit: &impl AuditLog,
    events: &impl EventLog,
    limit: usize,
) -> Result<Vec<ActivityItem>, AppError> {
    let mut items = Vec::new();
    for o in observations.recent(limit)? {
        items.push(ActivityItem {
            at: o.recorded_at(),
            kind: ActivityKind::Observation,
            summary: o.note().to_owned(),
        });
    }
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

/// Sends a message to the butler and records both turns.
///
/// Appends the person's message, asks the [`Butler`] to respond to the full
/// conversation, records the butler's reply, and returns the reply message and
/// any [`ButlerProposal`]s. The proposals are *suggestions only* — they are not
/// executed here; the person confirms each one separately (models propose, the
/// person authorizes).
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
    suggestions: &impl SuggestionRepository,
    beliefs: &impl BeliefRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
) -> Result<(ChatMessage, Vec<Suggestion>, Vec<String>), AppError> {
    // Non-streaming is just streaming with the tokens discarded.
    send_to_butler_streaming(
        chat,
        preferences,
        suggestions,
        beliefs,
        capabilities,
        butler,
        ids,
        clock,
        context,
        text,
        &mut |_| {},
    )
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
    suggestions: &impl SuggestionRepository,
    beliefs: &impl BeliefRepository,
    capabilities: &dyn CapabilityRunner,
    butler: &dyn Butler,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
    text: &str,
    on_token: &mut dyn FnMut(&str),
) -> Result<(ChatMessage, Vec<Suggestion>, Vec<String>), AppError> {
    let user = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::User,
        text,
        clock.now(),
    )?;
    chat.append(&user)?;

    let history = chat.list()?;
    let prefs = preferences.list_all()?;
    let mut reply = butler
        .respond_streaming(&history, &prefs, context, on_token)
        .map_err(|e| AppError::Model {
            message: e.to_string(),
        })?;

    // `activity` is a plain-language record of what Endora did behind the scenes
    // this turn — skills used, learnings, suggestions — so the person can see what
    // the conversation actually changed (transparency + debugging).
    let mut activity: Vec<String> = Vec::new();

    // Interventions: if the butler asked to use a skill, the policy check here
    // decides what happens (the model proposes; policy authorizes; the capability
    // executes — ADRs 0019/0020). One tool round per turn. Whatever the outcome —
    // ran, failed, needs setup, needs confirmation, or unknown — the butler ALWAYS
    // answers again with that outcome, so it never dead-ends on a "one moment"
    // placeholder and never invents a result it didn't fetch.
    let model_requested_skill = reply.capability_use.is_some();
    if let Some(used) = reply.capability_use.take() {
        let id = &used.capability;
        let spec = capabilities.available().into_iter().find(|c| &c.id == id);
        let outcome = match spec {
            // Cleared to run on its own (configured + read-only/low-stakes).
            Some(s) if s.configured && s.autonomous => {
                match capabilities.run(id, &used.input_json) {
                    Ok(out) => {
                        activity.push(format!("Used the {id} skill"));
                        format!(
                            "You used the '{id}' skill and it returned:\n{out}\n\
                             Relay this to the person — share the specifics in your own words."
                        )
                    }
                    Err(e) => {
                        activity.push(format!("Tried the {id} skill, but it failed"));
                        format!(
                            "You tried the '{id}' skill but it failed: {e}. Tell the person \
                             plainly you couldn't get it — do not make up an answer."
                        )
                    }
                }
            }
            // Present, but awaiting setup — cannot be used yet.
            Some(s) if !s.configured => {
                activity.push(format!("The {id} skill isn't set up yet"));
                format!(
                    "The '{id}' skill isn't set up yet, so you could NOT check it. Tell the \
                     person plainly you can't do that yet — do not invent a result — and offer \
                     something you can actually do."
                )
            }
            // Configured but consequential — must be confirmed, never auto-run.
            Some(_) => {
                activity.push(format!("The {id} skill needs your OK first"));
                format!(
                    "The '{id}' skill needs the person's go-ahead before it runs. Ask them if \
                     they'd like you to — don't claim you've done it."
                )
            }
            // No such skill.
            None => {
                activity.push(format!("No {id} skill is available"));
                format!(
                    "There is no '{id}' skill available, so you could NOT do that. Tell the \
                     person plainly you can't yet — do not invent a result — and offer what you \
                     can do instead."
                )
            }
        };
        let mut ctx = context.clone();
        ctx.tool_result = Some(outcome);
        // Answer again using the real outcome (a second, non-streamed pass). If it
        // fails, the first reply still stands.
        if let Ok(synth) = butler.respond(&history, &prefs, &ctx) {
            reply = synth;
        }
    }

    // Deterministic net against fabrication: if the person clearly asked something
    // factual (weather, news, active safety alerts) and the model reached for NO
    // skill at all, it would be answering from imagination. So run the matching
    // skill ourselves — with their known home location — and let the butler answer
    // from that real result instead. Policy still gates it (configured + read-only),
    // and it only fires when the model requested nothing, so a correct model-driven
    // tool use (e.g. a specific city it named) is left untouched.
    if let Some(skill) = follow_up_intent(text, &history).filter(|_| !model_requested_skill) {
        let spec = capabilities.available().into_iter().find(|c| c.id == skill);
        let cleared = spec.as_ref().is_some_and(|s| s.configured && s.autonomous);
        if cleared {
            // Usable: run it with the person's home location, then have the butler
            // answer from the real result. Without a location we can't (the model's
            // own reply then stands).
            if let Some(location) = known_location(&prefs) {
                let result = match capabilities.run(skill, &json_location(&location)) {
                    Ok(out) => {
                        activity.push(format!("Used the {skill} skill"));
                        format!(
                            "You used the '{skill}' skill for {location} and it returned:\n{out}\n\
                             Relay this to the person — share the specifics in your own words, \
                             and add nothing that isn't here."
                        )
                    }
                    Err(e) => {
                        activity.push(format!("Tried the {skill} skill, but it failed"));
                        format!(
                            "You tried the '{skill}' skill but it failed: {e}. Tell the person \
                             plainly you couldn't get it — do not make up an answer."
                        )
                    }
                };
                let mut ctx = context.clone();
                ctx.tool_result = Some(result);
                if let Ok(synth) = butler.respond(&history, &prefs, &ctx) {
                    reply = synth;
                }
            }
        } else {
            // The person clearly asked for something factual, but the skill that
            // would answer it is off, not set up, or outside the autonomy envelope.
            // The model has proven it will fabricate here even when told not to, so
            // we do NOT ask it — we set an honest reply deterministically. This makes
            // inventing a fact structurally impossible in the can't-serve case.
            activity.push(format!("Couldn't check {skill} — it's off or not set up"));
            reply = ButlerReply {
                text: "I can't check that for you right now — the skill I'd use is turned off \
                       or not set up. You can manage what I'm allowed to do on my own under \
                       Skills."
                    .to_owned(),
                ..ButlerReply::default()
            };
        }
    }

    // A brain that returns nothing usable still owes the person a reply.
    let reply_text = if reply.text.trim().is_empty() {
        "I'm not sure how to help with that yet — can you say a bit more?"
    } else {
        reply.text.trim()
    };
    let butler = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        reply_text,
        clock.now(),
    )?;
    chat.append(&butler)?;

    // Persist the proposals as durable, pending suggestions tied to this reply,
    // so the conversation's learnings survive a reload and can be applied later —
    // not lost the moment the chat moves on (ADR 0019).
    let mut saved = Vec::with_capacity(reply.proposals.len());
    for proposal in reply.proposals {
        let suggestion = Suggestion {
            id: SuggestionId::new(ids.new_id()),
            proposal,
            status: SuggestionStatus::Pending,
            from_message: Some(butler.id()),
            created_at: clock.now(),
            decided_at: None,
        };
        activity.push(format!(
            "Added to your inbox — {}",
            suggestion.proposal.label()
        ));
        suggestions.save(&suggestion)?;
        saved.push(suggestion);
    }

    // Store the understanding the butler formed this turn. These are Endora's own
    // beliefs (not actions) — kept directly, then reviewable/correctable (ADR 0020).
    // If the butler restated something Endora already believes, affirm the existing
    // belief (raising confidence) rather than storing a near-duplicate.
    let existing = beliefs.list()?;
    for formed in reply.beliefs {
        if let Some(mut prior) = existing
            .iter()
            .find(|b| similar(b.statement(), &formed.statement))
            .cloned()
        {
            activity.push(format!("Grew more sure that {}", prior.statement()));
            prior.affirm(clock.now());
            beliefs.save(&prior)?;
            continue;
        }
        activity.push(format!("Learned that {}", formed.statement.trim()));
        let belief = Belief::new(
            BeliefId::new(ids.new_id()),
            &formed.statement,
            formed.kind,
            formed.confidence,
            &formed.evidence,
            clock.now(),
        )?;
        beliefs.save(&belief)?;
    }

    Ok((butler, saved, activity))
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

/// Content words of a statement (drops short/common words), for fuzzy matching.
fn keywords(s: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "you", "your", "are", "the", "and", "for", "that", "this", "with", "have", "was", "its",
        "but", "not", "can", "want", "like",
    ];
    normalized(s)
        .split_whitespace()
        .filter(|w| w.len() > 2 && !STOP.contains(w))
        .map(str::to_owned)
        .collect()
}

/// Whether two belief statements are effectively the same (a paraphrase), by
/// keyword overlap — so "you want information about your surroundings" and
/// "you're looking for information about your surroundings" are one belief.
fn similar(a: &str, b: &str) -> bool {
    let (ka, kb) = (keywords(a), keywords(b));
    if ka.is_empty() || kb.is_empty() {
        return normalized(a) == normalized(b);
    }
    let inter = ka.iter().filter(|w| kb.contains(w)).count();
    let union = ka.len() + kb.iter().filter(|w| !ka.contains(w)).count();
    inter as f64 / union as f64 >= 0.6
}

/// Maps a clear factual request to the skill that should answer it, so the butler
/// is grounded in real data rather than left to invent one. Deliberately narrow —
/// only the requests where answering from memory would be a fabrication (weather,
/// news, active safety alerts). Returns the capability id, or `None` to leave the
/// turn to ordinary conversation.
fn route_intent(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| t.contains(n));
    // Safety first: an "is it safe / any warnings" ask is about active alerts.
    if has(&[
        "safety alert",
        "safety warning",
        "severe weather",
        "any warnings",
        "is it safe",
    ]) {
        return Some("safety_alerts");
    }
    if has(&["news", "headline", "headlines"]) {
        return Some("news");
    }
    if has(&[
        "weather",
        "forecast",
        "temperature",
        "how hot",
        "how cold",
        "raining",
    ]) {
        return Some("weather");
    }
    None
}

/// Like [`route_intent`], but also catches a **deictic follow-up** — "right now?",
/// "currently?", "what about now" — by reusing the intent of the person's previous
/// message. So a follow-up to a weather/news answer re-runs the skill instead of
/// letting the model invent a fresh number. `history` ends with the current message.
fn follow_up_intent(text: &str, history: &[ChatMessage]) -> Option<&'static str> {
    if let Some(skill) = route_intent(text) {
        return Some(skill);
    }
    let t = text.to_lowercase();
    let deictic = [
        "right now",
        "currently",
        "at the moment",
        "what about now",
        "how about now",
        "and now",
    ]
    .iter()
    .any(|p| t.contains(p))
        || matches!(t.trim(), "now" | "now?");
    if !deictic {
        return None;
    }
    // Reuse the intent of the most recent earlier user message (skip the current
    // one, which is the last entry).
    history
        .iter()
        .rev()
        .skip(1)
        .filter(|m| m.role() == MessageRole::User)
        .find_map(|m| route_intent(m.text()))
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

/// The person's stated home location, if they've set one (the location setup
/// stores it as a preference like "Based in: Charlotte"). Used to answer local
/// asks without pestering them for a place each time.
fn known_location(preferences: &[Preference]) -> Option<String> {
    for p in preferences {
        let t = p.text().trim();
        let lower = t.to_lowercase();
        if let Some(rest) = lower.strip_prefix("based in") {
            let start = t.len() - rest.len();
            let loc = t[start..].trim_start_matches([':', ' ']).trim();
            if !loc.is_empty() {
                return Some(loc.to_owned());
            }
        }
    }
    None
}

/// Builds a `{"location":"..."}` JSON input for a capability, escaping the value
/// so a place name with quotes can't break the JSON. Kept here (no serde in the
/// application layer) since the shape is trivial and fixed.
fn json_location(location: &str) -> String {
    let escaped = location.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{{\"location\":\"{escaped}\"}}")
}

/// Endora's living understanding of the person — its active beliefs, most
/// recently affirmed first. Corrected beliefs are omitted.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn understanding(beliefs: &impl BeliefRepository) -> Result<Vec<Belief>, AppError> {
    Ok(beliefs
        .list()?
        .into_iter()
        .filter(|b| b.status() == endora_domain::BeliefStatus::Active)
        .collect())
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

/// Lists the butler's persisted suggestions, newest first, optionally filtered by
/// status (e.g. only the pending ones for an inbox).
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn list_suggestions(
    suggestions: &impl SuggestionRepository,
    status: Option<SuggestionStatus>,
) -> Result<Vec<Suggestion>, AppError> {
    Ok(suggestions.list(status)?)
}

/// Dismisses a pending suggestion (records the decision; nothing is created).
///
/// # Errors
/// [`AppError::NotFound`] if it does not exist, or [`AppError::Repository`].
pub fn dismiss_suggestion(
    suggestions: &impl SuggestionRepository,
    clock: &impl Clock,
    id: SuggestionId,
) -> Result<(), AppError> {
    let mut suggestion = suggestions.get(id)?.ok_or(AppError::NotFound {
        entity: "suggestion",
    })?;
    suggestion.status = SuggestionStatus::Dismissed;
    suggestion.decided_at = Some(clock.now());
    suggestions.save(&suggestion)?;
    Ok(())
}

/// Applies a pending suggestion: runs the deterministic create it stands for and
/// records that it was applied. This is the human-authorized step — the butler
/// only ever *proposed* it. Returns the resolved [`Suggestion`].
///
/// For a target, the North Star reference the butler gave (an id, or a name) is
/// resolved here: an exact id if it exists, else a case-insensitive title match
/// against an existing North Star. If it cannot be resolved, the suggestion is
/// left pending and an error is returned so the caller can explain why.
///
/// # Errors
/// [`AppError::NotFound`] if the suggestion (or a referenced North Star) is
/// missing, [`AppError::Domain`] on invalid content, or [`AppError::Repository`].
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn apply_suggestion(
    suggestions: &impl SuggestionRepository,
    values: &impl ValueRepository,
    directions: &impl DirectionRepository,
    targets: &impl TargetRepository,
    preferences: &impl PreferenceRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    id: SuggestionId,
) -> Result<Suggestion, AppError> {
    let mut suggestion = suggestions.get(id)?.ok_or(AppError::NotFound {
        entity: "suggestion",
    })?;

    match &suggestion.proposal {
        ButlerProposal::CreateValue { name } => {
            create_value(values, ids, name)?;
        }
        ButlerProposal::CreateNorthStar { title } => {
            create_direction(directions, ids, title)?;
        }
        ButlerProposal::CreateTarget {
            direction_ref,
            statement,
        } => {
            let direction = resolve_direction(directions, direction_ref)?;
            create_target(directions, targets, ids, direction, statement)?;
        }
        ButlerProposal::RememberPreference { text, kind } => {
            create_preference(preferences, ids, clock, text, *kind)?;
        }
    }

    suggestion.status = SuggestionStatus::Applied;
    suggestion.decided_at = Some(clock.now());
    suggestions.save(&suggestion)?;
    Ok(suggestion)
}

/// Resolves a North Star reference (an id, or a name the model used) to a real
/// [`DirectionId`]: an exact id that exists, else a case-insensitive title match.
fn resolve_direction(
    directions: &impl DirectionRepository,
    reference: &str,
) -> Result<DirectionId, AppError> {
    // An exact, existing id wins.
    if let Ok(raw) = reference.parse::<u128>() {
        let id = DirectionId::new(raw);
        if directions.get(id)?.is_some() {
            return Ok(id);
        }
    }
    // Otherwise match the name (the common case — the model gives a title).
    let wanted = reference.trim().to_lowercase();
    directions
        .list_all()?
        .into_iter()
        .find(|d| d.title().trim().to_lowercase() == wanted)
        .map(|d| d.id())
        .ok_or(AppError::NotFound {
            entity: "direction",
        })
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

/// If a check-in is due, has the butler **reach out**: append a proactive opening
/// message (grounded in what needs attention and what the person is working
/// toward) and advance the schedule. Called by the node's heartbeat. Returns the
/// message if one was posted, or `None` if nothing was due.
///
/// This is an `act` on the low-stakes end of the autonomy model (ADR 0010): a
/// message, never a consequential action — those still go through propose→confirm.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails, or [`AppError::Domain`] if the
/// generated message is somehow invalid.
pub fn run_due_checkin(
    chat: &impl ChatRepository,
    checkins: &impl CheckinRepository,
    ids: &impl IdSource,
    clock: &impl Clock,
    context: &ButlerContext,
) -> Result<Option<ChatMessage>, AppError> {
    let now = clock.now();
    let Some(mut schedule) = checkins.get()? else {
        return Ok(None);
    };
    if !schedule.is_due(now) {
        return Ok(None);
    }
    // Advance first, so a slow write can't double-post on the next tick.
    schedule.next_at = Timestamp::from_unix_millis(now.unix_millis() + schedule.interval_ms);
    checkins.set(&schedule)?;

    let message = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        &checkin_text(context),
        now,
    )?;
    chat.append(&message)?;
    Ok(Some(message))
}

/// Composes a **daily briefing** — an act of service (ADR 0025): it runs the
/// reversible information skills (weather, active safety alerts, local news) for the
/// person's home location and posts a single, grounded briefing to the chat. Uses
/// ONLY skills that are configured *and* cleared to run autonomously (reversible,
/// read-only), so a briefing never does anything consequential (ADR 0024). Returns
/// the posted message and a plain-language activity trail, or `None` if there's no
/// home location set or nothing to report.
///
/// # Errors
/// [`AppError::Repository`] on a backend failure.
pub fn daily_brief(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    capabilities: &dyn CapabilityRunner,
    ids: &impl IdSource,
    clock: &impl Clock,
) -> Result<Option<(ChatMessage, Vec<String>)>, AppError> {
    let prefs = preferences.list_all()?;
    let Some(location) = known_location(&prefs) else {
        return Ok(None);
    };
    let input = json_location(&location);
    let available = capabilities.available();
    let mut sections: Vec<String> = Vec::new();
    let mut activity: Vec<String> = Vec::new();
    // Each is run only if it's usable AND cleared to act on its own (reversible).
    for (id, label) in [
        ("weather", "Weather"),
        ("safety_alerts", "Safety"),
        ("news", "News"),
    ] {
        let cleared = available
            .iter()
            .any(|c| c.id == id && c.configured && c.autonomous);
        if !cleared {
            continue;
        }
        if let Ok(out) = capabilities.run(id, &input) {
            sections.push(format!("{label} — {out}"));
            activity.push(format!("Used the {id} skill for your brief"));
        }
    }
    if sections.is_empty() {
        return Ok(None);
    }
    let text = format!(
        "Here's your brief for {location}:\n\n{}",
        sections.join("\n\n")
    );
    let message = ChatMessage::new(
        MessageId::new(ids.new_id()),
        MessageRole::Butler,
        &text,
        clock.now(),
    )?;
    chat.append(&message)?;
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
pub fn run_due_brief(
    chat: &impl ChatRepository,
    preferences: &impl PreferenceRepository,
    briefs: &impl BriefScheduleRepository,
    capabilities: &dyn CapabilityRunner,
    ids: &impl IdSource,
    clock: &impl Clock,
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
    daily_brief(chat, preferences, capabilities, ids, clock)
}

/// Composes a proactive check-in, grounded in the person's life and always asking
/// how the butler can serve them better (the self-improvement loop, in dialogue).
fn checkin_text(context: &ButlerContext) -> String {
    let focus = "Is there anything you'd like me to focus on more, or do differently?";
    // Lead with understanding — the check-in is grounded in what Endora knows of
    // the person, not a task list. Stored beliefs already read "you …".
    if let Some(u) = context.understanding.first() {
        // Strip the leading "[kind] " tag and trailing " (… confidence)".
        let plain = u
            .split_once("] ")
            .map_or(u.as_str(), |(_, rest)| rest)
            .rsplit_once(" (")
            .map_or(u.as_str(), |(head, _)| head);
        return format!("Checking in. I've had the sense that {plain}. {focus}");
    }
    if let Some(item) = context.attention.first() {
        return format!(
            "A moment when you have one — I noticed {item}. Want to look at it together? {focus}"
        );
    }
    format!(
        "Good to see you. What would you like to work on — and {}",
        focus.to_lowercase()
    )
}

/// Assembles the [`ButlerContext`] — a snapshot of the person's current life
/// (values, North Stars with status/value/whether they have a target, and what
/// needs attention) — so the butler's conversation is grounded in what exists.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
#[expect(
    clippy::too_many_arguments,
    reason = "explicit dependencies, no hidden state"
)]
pub fn butler_context(
    values: &impl ValueRepository,
    directions: &impl DirectionRepository,
    targets: &impl TargetRepository,
    experiments: &impl ExperimentRepository,
    snoozes: &impl SnoozeRepository,
    beliefs: &impl BeliefRepository,
    capabilities: &dyn CapabilityRunner,
    clock: &impl Clock,
) -> Result<ButlerContext, AppError> {
    let value_list = values.list_all()?;
    let mut north_stars = Vec::new();
    for d in directions.list_all()? {
        let has_active_target = targets
            .list_for_direction(d.id())?
            .iter()
            .any(|t| t.status().is_active());
        let value = d.value().and_then(|vid| {
            value_list
                .iter()
                .find(|v| v.id() == vid)
                .map(|v| v.name().to_owned())
        });
        north_stars.push(NorthStarBrief {
            id: d.id().value().to_string(),
            title: d.title().to_owned(),
            status: d.status().name().to_owned(),
            value,
            has_active_target,
        });
    }
    let attention = attention(directions, targets, experiments, snoozes, clock)?
        .into_iter()
        .map(|i| i.headline)
        .collect();
    let understanding = understanding(beliefs)?
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
    let skills = capabilities
        .available()
        .into_iter()
        .filter(|c| c.configured)
        .map(|c| format!("{} — {}", c.id, c.description))
        .collect();
    Ok(ButlerContext {
        values: value_list.iter().map(|v| v.name().to_owned()).collect(),
        north_stars,
        attention,
        understanding,
        capabilities: skills,
        tool_result: None,
        now: format_datetime_utc(clock.now().unix_millis()),
    })
}

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

/// Computes what currently needs the person's attention, most pressing first,
/// with snoozed items suppressed (see `docs/adr/0016-adaptive-attention.md`).
///
/// This is a fresh read each time, so resolving an item removes it and new ones
/// appear on their own — reprioritization is automatic.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails or stored data is corrupt.
pub fn attention(
    directions: &impl DirectionRepository,
    targets: &impl TargetRepository,
    experiments: &impl ExperimentRepository,
    snoozes: &impl SnoozeRepository,
    clock: &impl Clock,
) -> Result<Vec<AttentionItem>, AppError> {
    let now = clock.now();
    let mut candidates: Vec<AttentionItem> = Vec::new();

    // Due reviews are the most pressing.
    for e in experiments.list_due_reviews(now)? {
        candidates.push(AttentionItem {
            kind: AttentionKind::ReviewDue,
            subject: e.id().value().to_string(),
            headline: format!("Review due: \"{}\"", e.hypothesis()),
        });
    }
    // Active North Stars that are unfiled or have no active target.
    for d in directions.list_all()? {
        if !d.status().is_active() {
            continue;
        }
        if d.value().is_none() {
            candidates.push(AttentionItem {
                kind: AttentionKind::UnfiledNorthStar,
                subject: d.id().value().to_string(),
                headline: format!(
                    "\"{}\" isn't filed under a value yet — what does it serve?",
                    d.title()
                ),
            });
        }
        let has_active_target = targets
            .list_for_direction(d.id())?
            .iter()
            .any(|t| t.status().is_active());
        if !has_active_target {
            candidates.push(AttentionItem {
                kind: AttentionKind::EmptyNorthStar,
                subject: d.id().value().to_string(),
                headline: format!(
                    "\"{}\" has no active target yet — add one to start.",
                    d.title()
                ),
            });
        }
    }

    // Drop anything currently snoozed.
    let mut out = Vec::new();
    for item in candidates {
        let hidden = snoozes
            .get(item.kind.name(), &item.subject)?
            .is_some_and(|s| s.until.unix_millis() > now.unix_millis());
        if !hidden {
            out.push(item);
        }
    }
    Ok(out)
}

/// Snoozes an attention item ("not now"), with exponential backoff: each snooze
/// roughly doubles the hidden interval (1, 2, 4, … days, capped), so a
/// repeatedly-deferred item is raised less and less.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn snooze_attention(
    snoozes: &impl SnoozeRepository,
    clock: &impl Clock,
    kind: AttentionKind,
    subject: &str,
) -> Result<Snooze, AppError> {
    let now = clock.now();
    let count = snoozes.get(kind.name(), subject)?.map_or(0, |s| s.count);
    // 1, 2, 4, 8, … days, capped at 64 so an item never disappears forever.
    let days = 1i64 << count.min(6);
    let until = Timestamp::from_unix_millis(now.unix_millis() + days * MILLIS_PER_DAY);
    let snooze = Snooze {
        count: count + 1,
        until,
    };
    snoozes.set(kind.name(), subject, snooze)?;
    Ok(snooze)
}

/// A short verb phrase for an audit summary.
fn describe(decision: PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Permit => "permitted",
        PolicyDecision::RequireHumanApproval => "requires human approval before",
        PolicyDecision::Deny { .. } => "denied",
    }
}

#[cfg(test)]
mod tests {
    use super::draft_process_change;
    use super::{
        approve_process_change, assign_direction_value, conclude_experiment, create_assumption,
        create_direction, create_reflection, create_target, create_value, decide_process_change,
        decide_stored_process_change, delete_direction, delete_target, delete_value,
        list_assumptions, list_due_reviews, list_experiments, list_observations,
        list_process_changes, list_reflections, list_targets, list_values, propose_experiment,
        propose_process_change, recent_audit, record_observation, reject_process_change,
        schedule_experiment_review, set_direction_status, set_target_status, start_experiment,
    };
    use crate::error::AppError;
    use crate::ports::{
        AssumptionRepository, AttentionKind, AuditLog, BeliefRepository, Butler, ButlerContext,
        ButlerProposal, ButlerReply, CapabilityRunner, ChatRepository, CheckinRepository,
        CheckinSchedule, Clock, DirectionRepository, EventLog, ExperimentRepository, IdSource,
        ObservationRepository, PreferenceRepository, ProcessChangeRepository, ProposalError,
        Proposer, ReflectionRepository, RepositoryError, Snooze, SnoozeRepository, Suggestion,
        SuggestionRepository, SuggestionStatus, TargetRepository, ValueRepository,
    };
    use endora_domain::LifecycleStatus;
    use endora_domain::{
        ApprovalState, Assumption, AssumptionId, AuditRecord, AutonomyLevel, ChatMessage,
        Direction, DirectionId, Experiment, ExperimentId, ExperimentStatus, MessageId, MessageRole,
        Observation, ObservationId, PolicyDecision, Preference, PreferenceId, PreferenceKind,
        ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId, SuggestionId, Target,
        TargetId, Timestamp, Value, ValueId,
    };
    use endora_domain::{Belief, BeliefId};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// An in-memory store implementing the repository ports, for tests only.
    /// A capability runner with no skills — the default for tests that don't
    /// exercise the interventions loop (the butler never proposes a `use`).
    struct NoCapabilities;
    impl CapabilityRunner for NoCapabilities {
        fn available(&self) -> Vec<crate::ports::CapabilitySpec> {
            Vec::new()
        }
        fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
            Err("no capabilities".to_owned())
        }
    }

    #[derive(Default)]
    struct FakeStore {
        values: RefCell<HashMap<u128, Value>>,
        messages: RefCell<Vec<ChatMessage>>,
        preferences: RefCell<Vec<Preference>>,
        snoozes: RefCell<HashMap<(String, String), Snooze>>,
        directions: RefCell<HashMap<u128, Direction>>,
        targets: RefCell<HashMap<u128, Target>>,
        assumptions: RefCell<HashMap<u128, Assumption>>,
        experiments: RefCell<HashMap<u128, Experiment>>,
        observations: RefCell<HashMap<u128, Observation>>,
        reflections: RefCell<HashMap<u128, Reflection>>,
        changes: RefCell<HashMap<u128, ProposedProcessChange>>,
        suggestions: RefCell<Vec<Suggestion>>,
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

    impl SuggestionRepository for FakeStore {
        fn save(&self, s: &Suggestion) -> Result<(), RepositoryError> {
            let mut v = self.suggestions.borrow_mut();
            if let Some(existing) = v.iter_mut().find(|e| e.id == s.id) {
                *existing = s.clone();
            } else {
                v.push(s.clone());
            }
            Ok(())
        }
        fn get(&self, id: SuggestionId) -> Result<Option<Suggestion>, RepositoryError> {
            Ok(self
                .suggestions
                .borrow()
                .iter()
                .find(|s| s.id == id)
                .cloned())
        }
        fn list(
            &self,
            status: Option<SuggestionStatus>,
        ) -> Result<Vec<Suggestion>, RepositoryError> {
            Ok(self
                .suggestions
                .borrow()
                .iter()
                .filter(|s| status.is_none_or(|w| s.status == w))
                .cloned()
                .collect())
        }
    }

    impl ProcessChangeRepository for FakeStore {
        fn save(&self, change: &ProposedProcessChange) -> Result<(), RepositoryError> {
            self.changes
                .borrow_mut()
                .insert(change.id().value(), change.clone());
            Ok(())
        }
        fn get(
            &self,
            id: ProcessChangeId,
        ) -> Result<Option<ProposedProcessChange>, RepositoryError> {
            Ok(self.changes.borrow().get(&id.value()).cloned())
        }
        fn list_for_reflection(
            &self,
            reflection: ReflectionId,
        ) -> Result<Vec<ProposedProcessChange>, RepositoryError> {
            let mut found: Vec<ProposedProcessChange> = self
                .changes
                .borrow()
                .values()
                .filter(|c| c.reflection() == reflection)
                .cloned()
                .collect();
            found.sort_by_key(|c| c.id().value());
            Ok(found)
        }
    }

    impl ReflectionRepository for FakeStore {
        fn save(&self, reflection: &Reflection) -> Result<(), RepositoryError> {
            self.reflections
                .borrow_mut()
                .insert(reflection.id().value(), reflection.clone());
            Ok(())
        }
        fn get(&self, id: ReflectionId) -> Result<Option<Reflection>, RepositoryError> {
            Ok(self.reflections.borrow().get(&id.value()).cloned())
        }
        fn list_for_target(&self, target: TargetId) -> Result<Vec<Reflection>, RepositoryError> {
            let mut found: Vec<Reflection> = self
                .reflections
                .borrow()
                .values()
                .filter(|r| r.target() == target)
                .cloned()
                .collect();
            found.sort_by_key(|r| r.id().value());
            Ok(found)
        }
    }

    impl ObservationRepository for FakeStore {
        fn save(&self, observation: &Observation) -> Result<(), RepositoryError> {
            self.observations
                .borrow_mut()
                .insert(observation.id().value(), observation.clone());
            Ok(())
        }
        fn list_for_experiment(
            &self,
            experiment: ExperimentId,
        ) -> Result<Vec<Observation>, RepositoryError> {
            let mut found: Vec<Observation> = self
                .observations
                .borrow()
                .values()
                .filter(|o| o.experiment() == experiment)
                .cloned()
                .collect();
            found.sort_by_key(|o| o.id().value());
            Ok(found)
        }
        fn recent(&self, limit: usize) -> Result<Vec<Observation>, RepositoryError> {
            let mut all: Vec<Observation> = self.observations.borrow().values().cloned().collect();
            all.sort_by_key(|o| (o.recorded_at().unix_millis(), o.id().value()));
            all.reverse();
            all.truncate(limit);
            Ok(all)
        }
    }

    impl AssumptionRepository for FakeStore {
        fn save(&self, assumption: &Assumption) -> Result<(), RepositoryError> {
            self.assumptions
                .borrow_mut()
                .insert(assumption.id().value(), assumption.clone());
            Ok(())
        }
        fn get(&self, id: AssumptionId) -> Result<Option<Assumption>, RepositoryError> {
            Ok(self.assumptions.borrow().get(&id.value()).cloned())
        }
        fn list_for_target(&self, target: TargetId) -> Result<Vec<Assumption>, RepositoryError> {
            let mut found: Vec<Assumption> = self
                .assumptions
                .borrow()
                .values()
                .filter(|a| a.target() == target)
                .cloned()
                .collect();
            found.sort_by_key(|a| a.id().value());
            Ok(found)
        }
    }

    impl ExperimentRepository for FakeStore {
        fn save(&self, experiment: &Experiment) -> Result<(), RepositoryError> {
            self.experiments
                .borrow_mut()
                .insert(experiment.id().value(), experiment.clone());
            Ok(())
        }
        fn get(&self, id: ExperimentId) -> Result<Option<Experiment>, RepositoryError> {
            Ok(self.experiments.borrow().get(&id.value()).cloned())
        }
        fn list_for_assumption(
            &self,
            assumption: AssumptionId,
        ) -> Result<Vec<Experiment>, RepositoryError> {
            let mut found: Vec<Experiment> = self
                .experiments
                .borrow()
                .values()
                .filter(|e| e.assumption() == assumption)
                .cloned()
                .collect();
            found.sort_by_key(|e| e.id().value());
            Ok(found)
        }
        fn list_due_reviews(&self, now: Timestamp) -> Result<Vec<Experiment>, RepositoryError> {
            let mut found: Vec<Experiment> = self
                .experiments
                .borrow()
                .values()
                .filter(|e| e.is_review_due(now))
                .cloned()
                .collect();
            found.sort_by_key(|e| (e.review_by().map(|t| t.unix_millis()), e.id().value()));
            Ok(found)
        }
    }

    impl ChatRepository for FakeStore {
        fn append(&self, message: &ChatMessage) -> Result<(), RepositoryError> {
            self.messages.borrow_mut().push(message.clone());
            Ok(())
        }
        fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError> {
            Ok(self.messages.borrow().clone())
        }
    }

    impl SnoozeRepository for FakeStore {
        fn get(&self, kind: &str, subject: &str) -> Result<Option<Snooze>, RepositoryError> {
            Ok(self
                .snoozes
                .borrow()
                .get(&(kind.to_owned(), subject.to_owned()))
                .copied())
        }
        fn set(&self, kind: &str, subject: &str, snooze: Snooze) -> Result<(), RepositoryError> {
            self.snoozes
                .borrow_mut()
                .insert((kind.to_owned(), subject.to_owned()), snooze);
            Ok(())
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

    /// A butler that echoes the newest message and always proposes a North Star,
    /// so the act/ask + propose flow can be exercised deterministically.
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
                text: format!("Shall I set that up? You said: {last}"),
                proposals: vec![ButlerProposal::CreateNorthStar {
                    title: last.to_owned(),
                }],
                ..ButlerReply::default()
            })
        }
    }

    impl ValueRepository for FakeStore {
        fn save(&self, value: &Value) -> Result<(), RepositoryError> {
            self.values
                .borrow_mut()
                .insert(value.id().value(), value.clone());
            Ok(())
        }
        fn get(&self, id: ValueId) -> Result<Option<Value>, RepositoryError> {
            Ok(self.values.borrow().get(&id.value()).cloned())
        }
        fn list_all(&self) -> Result<Vec<Value>, RepositoryError> {
            let mut found: Vec<Value> = self.values.borrow().values().cloned().collect();
            found.sort_by_key(|v| v.id().value());
            Ok(found)
        }
        fn delete(&self, id: ValueId) -> Result<(), RepositoryError> {
            self.values.borrow_mut().remove(&id.value());
            Ok(())
        }
    }

    impl DirectionRepository for FakeStore {
        fn save(&self, direction: &Direction) -> Result<(), RepositoryError> {
            self.directions
                .borrow_mut()
                .insert(direction.id().value(), direction.clone());
            Ok(())
        }
        fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError> {
            Ok(self.directions.borrow().get(&id.value()).cloned())
        }
        fn list_all(&self) -> Result<Vec<Direction>, RepositoryError> {
            let mut found: Vec<Direction> = self.directions.borrow().values().cloned().collect();
            found.sort_by_key(|d| d.id().value());
            Ok(found)
        }
        fn delete(&self, id: DirectionId) -> Result<(), RepositoryError> {
            self.directions.borrow_mut().remove(&id.value());
            Ok(())
        }
    }

    impl TargetRepository for FakeStore {
        fn save(&self, target: &Target) -> Result<(), RepositoryError> {
            self.targets
                .borrow_mut()
                .insert(target.id().value(), target.clone());
            Ok(())
        }
        fn get(&self, id: TargetId) -> Result<Option<Target>, RepositoryError> {
            Ok(self.targets.borrow().get(&id.value()).cloned())
        }
        fn list_for_direction(
            &self,
            direction: DirectionId,
        ) -> Result<Vec<Target>, RepositoryError> {
            let mut found: Vec<Target> = self
                .targets
                .borrow()
                .values()
                .filter(|g| g.direction() == direction)
                .cloned()
                .collect();
            found.sort_by_key(|g| g.id().value());
            Ok(found)
        }
        fn delete(&self, id: TargetId) -> Result<(), RepositoryError> {
            self.targets.borrow_mut().remove(&id.value());
            Ok(())
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

    /// An in-memory event log (the butler's action log).
    #[derive(Default)]
    struct FakeEvents {
        rows: RefCell<Vec<crate::ports::ActivityEvent>>,
    }

    impl EventLog for FakeEvents {
        fn record(&self, at: Timestamp, summary: &str) -> Result<(), RepositoryError> {
            self.rows.borrow_mut().push(crate::ports::ActivityEvent {
                at,
                summary: summary.to_owned(),
            });
            Ok(())
        }
        fn recent(
            &self,
            limit: usize,
        ) -> Result<Vec<crate::ports::ActivityEvent>, RepositoryError> {
            let all = self.rows.borrow();
            Ok(all.iter().rev().take(limit).cloned().collect())
        }
    }

    fn approved_change() -> ProposedProcessChange {
        let mut p = ProposedProcessChange::propose(
            ProcessChangeId::new(7),
            ReflectionId::new(1),
            "Default runs to mornings",
        )
        .unwrap();
        p.approve().unwrap();
        p
    }

    #[test]
    fn deciding_a_process_change_records_the_decision() {
        let ids = SeqIds::default();
        let clock = FixedClock(1_700_000_000_000);
        let audit = FakeAudit::default();

        let decision = decide_process_change(
            &approved_change(),
            AutonomyLevel::ActWithinPolicy,
            &ids,
            &clock,
            &audit,
        )
        .unwrap();

        assert_eq!(decision, PolicyDecision::Permit);
        let records = audit.records.borrow();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].at(),
            Timestamp::from_unix_millis(1_700_000_000_000)
        );
        assert!(records[0].summary().contains("permitted"));
        assert!(records[0].summary().contains("process change 7"));
    }

    #[test]
    fn an_unapproved_change_is_audited_as_requiring_approval() {
        let ids = SeqIds::default();
        let clock = FixedClock(0);
        let audit = FakeAudit::default();
        let mut change = approved_change();
        // A fresh (unapproved) proposal:
        change = ProposedProcessChange::propose(
            change.id(),
            ReflectionId::new(1),
            "Default runs to mornings",
        )
        .unwrap();

        let decision = decide_process_change(
            &change,
            AutonomyLevel::ActWithinPolicy,
            &ids,
            &clock,
            &audit,
        )
        .unwrap();

        assert_eq!(decision, PolicyDecision::RequireHumanApproval);
        assert_eq!(audit.recent(10).unwrap().len(), 1);
        assert!(
            audit.recent(1).unwrap()[0]
                .summary()
                .contains("requires human approval")
        );
    }

    #[test]
    fn create_direction_persists_and_returns_it() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let d = create_direction(&store, &ids, "Be healthier").unwrap();
        assert_eq!(DirectionRepository::get(&store, d.id()).unwrap(), Some(d));
    }

    #[test]
    fn create_target_requires_an_existing_direction() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err =
            create_target(&store, &store, &ids, DirectionId::new(999), "Run a 5k").unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "direction"
            }
        );
    }

    #[test]
    fn create_target_under_a_direction_then_list() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();

        let g1 = create_target(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        let g2 = create_target(&store, &store, &ids, direction.id(), "Sleep 8h").unwrap();

        assert_eq!(list_targets(&store, direction.id()).unwrap(), vec![g1, g2]);
    }

    #[test]
    fn target_lifecycle_status_is_set_and_persisted() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let target = create_target(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        assert_eq!(target.status(), LifecycleStatus::Active);

        let achieved = set_target_status(&store, target.id(), LifecycleStatus::Achieved).unwrap();
        assert_eq!(achieved.status(), LifecycleStatus::Achieved);
        // Persisted: a re-list reflects the new status.
        assert_eq!(
            list_targets(&store, direction.id()).unwrap()[0].status(),
            LifecycleStatus::Achieved
        );
    }

    #[test]
    fn setting_status_on_a_missing_target_is_not_found() {
        let store = FakeStore::default();
        let err =
            set_target_status(&store, TargetId::new(1), LifecycleStatus::Archived).unwrap_err();
        assert_eq!(err, AppError::NotFound { entity: "target" });
    }

    #[test]
    fn deleting_a_target_with_assumptions_is_refused_then_allowed() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let target = create_target(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        create_assumption(&store, &store, &ids, target.id(), "Mornings are freest").unwrap();

        // Refused while a dependent assumption exists.
        let err = delete_target(&store, &store, target.id()).unwrap_err();
        assert!(matches!(err, AppError::BadRequest { .. }));

        // Allowed once it has no assumptions.
        let a = list_assumptions(&store, target.id()).unwrap()[0].id();
        store.assumptions.borrow_mut().remove(&a.value());
        delete_target(&store, &store, target.id()).unwrap();
        assert!(list_targets(&store, direction.id()).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_direction_with_targets_is_refused() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        create_target(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();

        let err = delete_direction(&store, &store, direction.id()).unwrap_err();
        assert!(matches!(err, AppError::BadRequest { .. }));

        // Archiving is the reversible alternative and is always allowed.
        let archived =
            set_direction_status(&store, direction.id(), LifecycleStatus::Archived).unwrap();
        assert_eq!(archived.status(), LifecycleStatus::Archived);
    }

    #[test]
    fn sending_to_the_butler_records_both_turns_and_returns_proposals() {
        use super::{chat_history, send_to_butler};
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);

        let (reply, suggestions, _activity) = send_to_butler(
            &store,
            &store,
            &store,
            &store,
            &NoCapabilities,
            &ScriptedTestButler,
            &ids,
            &clock,
            &ButlerContext::default(),
            "I want to run more",
        )
        .unwrap();
        assert_eq!(reply.role(), MessageRole::Butler);
        assert!(reply.text().contains("I want to run more"));
        // The proposal is persisted as a pending suggestion tied to the reply.
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].status, SuggestionStatus::Pending);
        assert_eq!(
            suggestions[0].proposal,
            ButlerProposal::CreateNorthStar {
                title: "I want to run more".to_owned()
            }
        );
        assert_eq!(suggestions[0].from_message, Some(reply.id()));
        // And it is durable — listable afterwards.
        assert_eq!(
            super::list_suggestions(&store, Some(SuggestionStatus::Pending))
                .unwrap()
                .len(),
            1
        );

        // Both turns are persisted, oldest first.
        let history = chat_history(&store).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role(), MessageRole::User);
        assert_eq!(history[1].role(), MessageRole::Butler);
    }

    #[test]
    fn a_cleared_skill_runs_and_the_butler_answers_with_its_result() {
        use super::send_to_butler;

        // A butler that first asks to use the "weather" skill, then (once a skill
        // result is in the context) answers using it. This is the propose → policy
        // authorizes → execute → synthesize loop the use case drives.
        struct ToolButler;
        impl Butler for ToolButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                context: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                if let Some(result) = &context.tool_result {
                    // Synthesis pass: answer using the real result.
                    return Ok(ButlerReply {
                        text: format!("Here's what I found — {result}"),
                        ..ButlerReply::default()
                    });
                }
                // First pass: brief reply + a skill request.
                Ok(ButlerReply {
                    text: "One moment — checking.".to_owned(),
                    capability_use: Some(crate::ports::CapabilityUse {
                        capability: "weather".to_owned(),
                        input_json: "{\"location\":\"Charlotte\"}".to_owned(),
                    }),
                    ..ButlerReply::default()
                })
            }
        }

        // A runner offering one cleared (configured + autonomous) skill.
        struct OneSkill;
        impl CapabilityRunner for OneSkill {
            fn available(&self) -> Vec<crate::ports::CapabilitySpec> {
                vec![crate::ports::CapabilitySpec {
                    id: "weather".to_owned(),
                    description: "current conditions".to_owned(),
                    configured: true,
                    autonomous: true,
                }]
            }
            fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
                assert_eq!(id, "weather");
                assert!(input_json.contains("Charlotte"));
                Ok("sunny, 30C".to_owned())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let (reply, _suggestions, activity) = send_to_butler(
            &store,
            &store,
            &store,
            &store,
            &OneSkill,
            &ToolButler,
            &ids,
            &clock,
            &ButlerContext::default(),
            "what's the weather in Charlotte?",
        )
        .unwrap();

        // The persisted reply is the synthesis (using the skill result), not the
        // brief "one moment" placeholder.
        assert!(reply.text().contains("sunny, 30C"));
        // And the skill use is recorded in the turn's activity.
        assert!(activity.iter().any(|a| a.contains("weather")));
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
                    capability_use: Some(crate::ports::CapabilityUse {
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
            fn available(&self) -> Vec<crate::ports::CapabilitySpec> {
                vec![crate::ports::CapabilitySpec {
                    id: "flights".to_owned(),
                    description: "find flights".to_owned(),
                    configured: false,
                    autonomous: false,
                }]
            }
            fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
                panic!("a gated skill must never be auto-run");
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let (reply, _s, activity) = send_to_butler(
            &store,
            &store,
            &store,
            &store,
            &GatedSkill,
            &ToolButler,
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
    fn intent_routing_and_location_helpers() {
        use super::{json_location, known_location, route_intent};
        assert_eq!(
            route_intent("how about what's in the news locally?"),
            Some("news")
        );
        assert_eq!(route_intent("what's the weather today"), Some("weather"));
        assert_eq!(
            route_intent("is it safe outside right now"),
            Some("safety_alerts")
        );
        assert_eq!(route_intent("let's talk about my week"), None);

        let t = Timestamp::from_unix_millis(0);
        let prefs = vec![
            Preference::new(PreferenceId::new(1), "Likes tea", PreferenceKind::Taste, t).unwrap(),
            Preference::new(
                PreferenceId::new(2),
                "Based in: 28277",
                PreferenceKind::Taste,
                t,
            )
            .unwrap(),
        ];
        assert_eq!(known_location(&prefs).as_deref(), Some("28277"));
        assert_eq!(known_location(&[]), None);

        assert_eq!(json_location("Charlotte"), "{\"location\":\"Charlotte\"}");
        // A value with a quote is escaped so the JSON stays well-formed.
        assert_eq!(json_location("a\"b"), "{\"location\":\"a\\\"b\"}");
    }

    #[test]
    fn a_deictic_follow_up_reuses_the_previous_intent() {
        use super::follow_up_intent;
        let msg = |role, text: &str| {
            ChatMessage::new(
                MessageId::new(1),
                role,
                text,
                Timestamp::from_unix_millis(0),
            )
            .unwrap()
        };
        // "Right now?" on its own has no intent...
        assert_eq!(follow_up_intent("Right now?", &[]), None);
        // ...but after a weather question, it re-runs weather.
        let history = vec![
            msg(MessageRole::User, "what's the weather today"),
            msg(MessageRole::Butler, "It's clear and warm."),
            msg(MessageRole::User, "Right now?"),
        ];
        assert_eq!(follow_up_intent("Right now?", &history), Some("weather"));
        // A non-deictic follow-up is left alone (no false trigger).
        let h2 = vec![
            msg(MessageRole::User, "what's the weather today"),
            msg(MessageRole::User, "ok what should I cook"),
        ];
        assert_eq!(follow_up_intent("ok what should I cook", &h2), None);
    }

    #[test]
    fn a_factual_ask_the_model_ignores_is_still_answered_from_a_skill() {
        use super::{create_preference, send_to_butler};

        // A butler that never reaches for a skill and would happily answer the news
        // question from imagination — exactly the fabrication we must prevent.
        struct FabricatingButler;
        impl Butler for FabricatingButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                context: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                if let Some(result) = &context.tool_result {
                    return Ok(ButlerReply {
                        text: format!("Here's the latest — {result}"),
                        ..ButlerReply::default()
                    });
                }
                Ok(ButlerReply {
                    text: "There's a big festival downtown this weekend.".to_owned(),
                    ..ButlerReply::default()
                })
            }
        }

        struct NewsSkill;
        impl CapabilityRunner for NewsSkill {
            fn available(&self) -> Vec<crate::ports::CapabilitySpec> {
                vec![crate::ports::CapabilitySpec {
                    id: "news".to_owned(),
                    description: "headlines".to_owned(),
                    configured: true,
                    autonomous: true,
                }]
            }
            fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
                assert_eq!(id, "news");
                assert!(input_json.contains("28277"));
                Ok("headline: council meets tonight".to_owned())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        // The person has told Endora where they're based.
        create_preference(
            &store,
            &ids,
            &clock,
            "Based in: 28277",
            PreferenceKind::Taste,
        )
        .unwrap();

        let (reply, _s, activity) = send_to_butler(
            &store,
            &store,
            &store,
            &store,
            &NewsSkill,
            &FabricatingButler,
            &ids,
            &clock,
            &ButlerContext::default(),
            "what's in the news?",
        )
        .unwrap();

        // The net ran the news skill and the answer came from its result — the
        // model's imagined "festival" was replaced.
        assert!(reply.text().contains("council meets tonight"));
        assert!(!reply.text().contains("festival"));
        assert!(activity.iter().any(|a| a.contains("news")));
    }

    #[test]
    fn a_factual_ask_for_a_disabled_skill_gets_honest_closure_not_a_fabrication() {
        use super::{create_preference, send_to_butler};

        struct FabricatingButler;
        impl Butler for FabricatingButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[Preference],
                context: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                if let Some(result) = &context.tool_result {
                    return Ok(ButlerReply {
                        text: format!("Honestly — {result}"),
                        ..ButlerReply::default()
                    });
                }
                Ok(ButlerReply {
                    text: "There's a big festival downtown this weekend.".to_owned(),
                    ..ButlerReply::default()
                })
            }
        }

        // News exists but is turned OFF (configured=false); it must never run.
        struct OffNews;
        impl CapabilityRunner for OffNews {
            fn available(&self) -> Vec<crate::ports::CapabilitySpec> {
                vec![crate::ports::CapabilitySpec {
                    id: "news".to_owned(),
                    description: "headlines".to_owned(),
                    configured: false,
                    autonomous: true,
                }]
            }
            fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
                panic!("a disabled skill must never run");
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        create_preference(
            &store,
            &ids,
            &clock,
            "Based in: 28277",
            PreferenceKind::Taste,
        )
        .unwrap();

        let (reply, _s, activity) = send_to_butler(
            &store,
            &store,
            &store,
            &store,
            &OffNews,
            &FabricatingButler,
            &ids,
            &clock,
            &ButlerContext::default(),
            "what's in the news?",
        )
        .unwrap();

        // No fabricated festival; the butler was grounded in "it's off".
        assert!(!reply.text().contains("festival"));
        assert!(activity.iter().any(|a| a.contains("Couldn't check news")));
    }

    #[test]
    fn brief_is_due_only_at_its_hour_once_per_day() {
        use crate::ports::BriefSchedule;
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
    fn daily_brief_composes_from_reversible_skills_and_needs_a_location() {
        use super::{create_preference, daily_brief};

        struct BriefSkills;
        impl CapabilityRunner for BriefSkills {
            fn available(&self) -> Vec<crate::ports::CapabilitySpec> {
                vec![crate::ports::CapabilitySpec {
                    id: "weather".to_owned(),
                    description: "w".to_owned(),
                    configured: true,
                    autonomous: true,
                }]
            }
            fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
                assert_eq!(id, "weather");
                assert!(input_json.contains("28277"));
                Ok("clear, 25C".to_owned())
            }
        }

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        // No home location yet ⇒ nothing to brief.
        assert!(
            daily_brief(&store, &store, &BriefSkills, &ids, &clock)
                .unwrap()
                .is_none()
        );
        // With a location, the brief is composed from the (reversible) weather skill.
        create_preference(
            &store,
            &ids,
            &clock,
            "Based in: 28277",
            PreferenceKind::Context,
        )
        .unwrap();
        let (msg, activity) = daily_brief(&store, &store, &BriefSkills, &ids, &clock)
            .unwrap()
            .unwrap();
        assert!(msg.text().contains("Weather — clear, 25C"));
        assert!(activity.iter().any(|a| a.contains("weather")));
    }

    #[test]
    fn checkin_runs_when_due_posts_a_message_and_advances_the_schedule() {
        use super::{chat_history, run_due_checkin, set_checkin_schedule};
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let ctx = ButlerContext::default();

        // Off by default: nothing posts.
        assert!(
            run_due_checkin(&store, &store, &ids, &FixedClock(1_000), &ctx)
                .unwrap()
                .is_none()
        );

        // Enable with a 60s interval; next check-in is one interval out.
        let sched = set_checkin_schedule(&store, &FixedClock(1_000), true, 60_000).unwrap();
        assert!(sched.enabled);
        assert_eq!(sched.next_at.unix_millis(), 61_000);

        // Before it is due: still nothing.
        assert!(
            run_due_checkin(&store, &store, &ids, &FixedClock(30_000), &ctx)
                .unwrap()
                .is_none()
        );

        // At/after the due time: the butler reaches out, and the schedule advances.
        let posted = run_due_checkin(&store, &store, &ids, &FixedClock(61_000), &ctx).unwrap();
        let msg = posted.expect("a check-in should have posted");
        assert_eq!(msg.role(), MessageRole::Butler);
        assert!(!msg.text().is_empty());
        assert_eq!(chat_history(&store).unwrap().len(), 1);
        assert_eq!(
            CheckinRepository::get(&store)
                .unwrap()
                .unwrap()
                .next_at
                .unix_millis(),
            121_000
        );

        // It does not double-post on the very next tick.
        assert!(
            run_due_checkin(&store, &store, &ids, &FixedClock(61_500), &ctx)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn butler_forms_understanding_that_is_reviewable_and_correctable() {
        use super::{affirm_belief, correct_belief, send_to_butler, understanding};
        use endora_domain::{BeliefKind, Confidence};

        struct BeliefButler;
        impl Butler for BeliefButler {
            fn respond(
                &self,
                _h: &[ChatMessage],
                _p: &[endora_domain::Preference],
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
            &store,
            &NoCapabilities,
            &BeliefButler,
            &ids,
            &clock,
            &ButlerContext::default(),
            "I'd love to see more of the world",
        )
        .unwrap();

        // The butler formed understanding, stored directly (no confirm step).
        let u = understanding(&store).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].statement(), "wants energy to travel");
        assert_eq!(u[0].confidence(), Confidence::Low);

        // Affirming raises confidence; correcting drops it from understanding.
        let id = u[0].id();
        affirm_belief(&store, &clock, id).unwrap();
        assert_eq!(
            understanding(&store).unwrap()[0].confidence(),
            Confidence::Medium
        );
        correct_belief(&store, id).unwrap();
        assert!(understanding(&store).unwrap().is_empty());
    }

    #[test]
    fn applying_a_target_suggestion_resolves_the_north_star_by_name() {
        use super::{apply_suggestion, create_direction, dismiss_suggestion};
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);

        // An existing North Star, and a target suggestion that refers to it by
        // *name* (as a small model would), not by id.
        let ns = create_direction(&store, &ids, "Get back into running").unwrap();
        let target_sugg = Suggestion {
            id: SuggestionId::new(ids.new_id()),
            proposal: ButlerProposal::CreateTarget {
                direction_ref: "get back into running".to_owned(),
                statement: "Run 3x a week".to_owned(),
            },
            status: SuggestionStatus::Pending,
            from_message: None,
            created_at: clock.now(),
            decided_at: None,
        };
        SuggestionRepository::save(&store, &target_sugg).unwrap();

        // Applying it resolves the name to the real North Star and creates the
        // target under it.
        let applied = apply_suggestion(
            &store,
            &store,
            &store,
            &store,
            &store,
            &ids,
            &clock,
            target_sugg.id,
        )
        .unwrap();
        assert_eq!(applied.status, SuggestionStatus::Applied);
        let targets = store.list_for_direction(ns.id()).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].statement(), "Run 3x a week");

        // A suggestion that names a North Star that doesn't exist stays pending.
        let orphan = Suggestion {
            id: SuggestionId::new(ids.new_id()),
            proposal: ButlerProposal::CreateTarget {
                direction_ref: "Nonexistent star".to_owned(),
                statement: "x".to_owned(),
            },
            status: SuggestionStatus::Pending,
            from_message: None,
            created_at: clock.now(),
            decided_at: None,
        };
        SuggestionRepository::save(&store, &orphan).unwrap();
        assert!(
            apply_suggestion(
                &store, &store, &store, &store, &store, &ids, &clock, orphan.id
            )
            .is_err()
        );
        assert_eq!(
            SuggestionRepository::get(&store, orphan.id)
                .unwrap()
                .unwrap()
                .status,
            SuggestionStatus::Pending
        );

        // And dismiss records the decision.
        dismiss_suggestion(&store, &clock, orphan.id).unwrap();
        assert_eq!(
            SuggestionRepository::get(&store, orphan.id)
                .unwrap()
                .unwrap()
                .status,
            SuggestionStatus::Dismissed
        );
    }

    #[test]
    fn attention_surfaces_unfiled_and_empty_north_stars_then_backs_off_on_snooze() {
        use super::{attention, snooze_attention};
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let day = 24 * 60 * 60 * 1_000;

        // An active North Star with no value and no target: two attention items.
        let d = create_direction(&store, &ids, "Get back into running").unwrap();
        let items = attention(&store, &store, &store, &store, &FixedClock(0)).unwrap();
        let kinds: Vec<_> = items.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&AttentionKind::UnfiledNorthStar));
        assert!(kinds.contains(&AttentionKind::EmptyNorthStar));

        // Snooze the "unfiled" item: hidden for 1 day, back after.
        snooze_attention(
            &store,
            &FixedClock(0),
            AttentionKind::UnfiledNorthStar,
            &d.id().value().to_string(),
        )
        .unwrap();
        let after = attention(&store, &store, &store, &store, &FixedClock(day / 2)).unwrap();
        assert!(
            !after
                .iter()
                .any(|i| i.kind == AttentionKind::UnfiledNorthStar)
        );
        let later = attention(&store, &store, &store, &store, &FixedClock(day + 1)).unwrap();
        assert!(
            later
                .iter()
                .any(|i| i.kind == AttentionKind::UnfiledNorthStar)
        );

        // A second snooze backs off to ~2 days.
        let s = snooze_attention(
            &store,
            &FixedClock(day + 1),
            AttentionKind::UnfiledNorthStar,
            &d.id().value().to_string(),
        )
        .unwrap();
        assert_eq!(s.count, 2);
        assert_eq!(s.until.unix_millis(), day + 1 + 2 * day);
    }

    #[test]
    fn preferences_are_recorded_deletable_and_passed_to_the_butler() {
        use super::{create_preference, delete_preference, list_preferences, send_to_butler};
        use endora_domain::PreferenceKind;

        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(0);
        let p = create_preference(
            &store,
            &ids,
            &clock,
            "prefers mornings",
            PreferenceKind::Taste,
        )
        .unwrap();
        assert_eq!(list_preferences(&store).unwrap().len(), 1);

        // A butler that reports how many preferences it was handed.
        struct EchoPrefsButler;
        impl Butler for EchoPrefsButler {
            fn respond(
                &self,
                _history: &[ChatMessage],
                preferences: &[Preference],
                _context: &ButlerContext,
            ) -> Result<ButlerReply, ProposalError> {
                Ok(ButlerReply {
                    text: format!("I already know {} thing(s) about you", preferences.len()),
                    ..ButlerReply::default()
                })
            }
        }
        let (reply, _, _) = send_to_butler(
            &store,
            &store,
            &store,
            &store,
            &NoCapabilities,
            &EchoPrefsButler,
            &ids,
            &clock,
            &ButlerContext::default(),
            "hi",
        )
        .unwrap();
        assert!(reply.text().contains("1 thing"));

        // Memory is deletable.
        delete_preference(&store, p.id()).unwrap();
        assert!(list_preferences(&store).unwrap().is_empty());
    }

    #[test]
    fn a_north_star_can_be_filed_under_a_value_and_cleared() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let value = create_value(&store, &ids, "Health").unwrap();
        let direction = create_direction(&store, &ids, "Get back into running").unwrap();
        assert_eq!(direction.value(), None);

        let filed =
            assign_direction_value(&store, &store, direction.id(), Some(value.id())).unwrap();
        assert_eq!(filed.value(), Some(value.id()));
        assert_eq!(list_values(&store).unwrap(), vec![value.clone()]);

        // Unfiling clears the link.
        let unfiled = assign_direction_value(&store, &store, direction.id(), None).unwrap();
        assert_eq!(unfiled.value(), None);
    }

    #[test]
    fn filing_under_an_unknown_value_is_not_found() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Get back into running").unwrap();
        let err = assign_direction_value(&store, &store, direction.id(), Some(ValueId::new(999)))
            .unwrap_err();
        assert_eq!(err, AppError::NotFound { entity: "value" });
    }

    #[test]
    fn deleting_a_value_in_use_is_refused_then_allowed() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let value = create_value(&store, &ids, "Health").unwrap();
        let direction = create_direction(&store, &ids, "Get back into running").unwrap();
        assign_direction_value(&store, &store, direction.id(), Some(value.id())).unwrap();

        // Refused while a North Star still serves it.
        let err = delete_value(&store, &store, value.id()).unwrap_err();
        assert!(matches!(err, AppError::BadRequest { .. }));

        // Allowed once the North Star is re-filed (unfiled).
        assign_direction_value(&store, &store, direction.id(), None).unwrap();
        delete_value(&store, &store, value.id()).unwrap();
        assert!(list_values(&store).unwrap().is_empty());
    }

    #[test]
    fn invalid_title_is_a_domain_error() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_direction(&store, &ids, "   ").unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn create_assumption_requires_an_existing_target() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_assumption(
            &store,
            &store,
            &ids,
            TargetId::new(404),
            "Mornings are freest",
        )
        .unwrap_err();
        assert_eq!(err, AppError::NotFound { entity: "target" });
    }

    #[test]
    fn create_assumption_under_a_target_then_list() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let target = create_target(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();

        let a1 =
            create_assumption(&store, &store, &ids, target.id(), "Mornings are freest").unwrap();
        let a2 = create_assumption(&store, &store, &ids, target.id(), "Rain is rare").unwrap();

        assert_eq!(list_assumptions(&store, target.id()).unwrap(), vec![a1, a2]);
    }

    /// Builds direction → target → assumption and returns the assumption id.
    fn seed_assumption(store: &FakeStore, ids: &SeqIds) -> AssumptionId {
        let direction = create_direction(store, ids, "Be healthier").unwrap();
        let target = create_target(store, store, ids, direction.id(), "Run a 5k").unwrap();
        create_assumption(store, store, ids, target.id(), "Mornings are freest")
            .unwrap()
            .id()
    }

    #[test]
    fn propose_experiment_requires_an_existing_assumption() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = propose_experiment(&store, &store, &ids, AssumptionId::new(404), "Try mornings")
            .unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "assumption"
            }
        );
    }

    #[test]
    fn experiment_lifecycle_persists_transitions() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let assumption = seed_assumption(&store, &ids);

        let e = propose_experiment(&store, &store, &ids, assumption, "Try mornings").unwrap();
        assert_eq!(e.status(), ExperimentStatus::Proposed);

        let started = start_experiment(&store, e.id()).unwrap();
        assert_eq!(started.status(), ExperimentStatus::Running);

        let concluded = conclude_experiment(&store, e.id()).unwrap();
        assert_eq!(concluded.status(), ExperimentStatus::Concluded);

        assert_eq!(
            list_experiments(&store, assumption).unwrap(),
            vec![concluded]
        );
    }

    #[test]
    fn scheduling_a_review_persists_the_due_time_and_surfaces_it_when_due() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let assumption = seed_assumption(&store, &ids);
        let e = propose_experiment(&store, &store, &ids, assumption, "Try mornings").unwrap();

        // "Now" is day zero; schedule a review 7 days out.
        let day = 24 * 60 * 60 * 1_000;
        let scheduled = schedule_experiment_review(&store, &FixedClock(0), e.id(), 7).unwrap();
        assert_eq!(
            scheduled.review_by().map(|t| t.unix_millis()),
            Some(7 * day)
        );

        // Not yet due at day 3.
        assert!(
            list_due_reviews(&store, &FixedClock(3 * day))
                .unwrap()
                .is_empty()
        );

        // Due once the scheduled time arrives.
        let due = list_due_reviews(&store, &FixedClock(7 * day)).unwrap();
        assert_eq!(due, vec![scheduled]);
    }

    #[test]
    fn a_concluded_experiment_is_not_surfaced_for_review() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let assumption = seed_assumption(&store, &ids);
        let e = propose_experiment(&store, &store, &ids, assumption, "Try mornings").unwrap();
        schedule_experiment_review(&store, &FixedClock(0), e.id(), 1).unwrap();
        start_experiment(&store, e.id()).unwrap();
        conclude_experiment(&store, e.id()).unwrap();

        let far_future = FixedClock(365 * 24 * 60 * 60 * 1_000);
        assert!(list_due_reviews(&store, &far_future).unwrap().is_empty());
    }

    #[test]
    fn scheduling_a_review_for_a_missing_experiment_is_not_found() {
        let store = FakeStore::default();
        let err = schedule_experiment_review(&store, &FixedClock(0), ExperimentId::new(1), 3)
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[test]
    fn concluding_a_proposed_experiment_is_a_domain_error() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let assumption = seed_assumption(&store, &ids);
        let e = propose_experiment(&store, &store, &ids, assumption, "Try mornings").unwrap();

        let err = conclude_experiment(&store, e.id()).unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn starting_a_missing_experiment_is_not_found() {
        let store = FakeStore::default();
        let err = start_experiment(&store, ExperimentId::new(1)).unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "experiment"
            }
        );
    }

    #[test]
    fn recording_an_observation_timestamps_it_from_the_clock() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_700_000_000_000);
        let assumption = seed_assumption(&store, &ids);
        let e = propose_experiment(&store, &store, &ids, assumption, "Try mornings").unwrap();

        let o = record_observation(&store, &store, &ids, &clock, e.id(), "felt good").unwrap();
        assert_eq!(
            o.recorded_at(),
            Timestamp::from_unix_millis(1_700_000_000_000)
        );
        assert_eq!(list_observations(&store, e.id()).unwrap(), vec![o]);
    }

    #[test]
    fn recording_against_a_missing_experiment_is_not_found() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(0);
        let err = record_observation(&store, &store, &ids, &clock, ExperimentId::new(1), "x")
            .unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "experiment"
            }
        );
    }

    #[test]
    fn create_reflection_requires_an_existing_target() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_reflection(
            &store,
            &store,
            &ids,
            TargetId::new(404),
            "went well",
            vec![ObservationId::new(1)],
        )
        .unwrap_err();
        assert_eq!(err, AppError::NotFound { entity: "target" });
    }

    #[test]
    fn create_reflection_requires_evidence() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let target = create_target(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        let err =
            create_reflection(&store, &store, &ids, target.id(), "went well", vec![]).unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn create_reflection_then_list() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let target = create_target(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        let r = create_reflection(
            &store,
            &store,
            &ids,
            target.id(),
            "mornings worked",
            vec![ObservationId::new(1), ObservationId::new(2)],
        )
        .unwrap();
        assert_eq!(list_reflections(&store, target.id()).unwrap(), vec![r]);
    }

    /// Builds direction → target → reflection and returns the reflection id.
    fn seed_reflection(store: &FakeStore, ids: &SeqIds) -> ReflectionId {
        let direction = create_direction(store, ids, "Be healthier").unwrap();
        let target = create_target(store, store, ids, direction.id(), "Run a 5k").unwrap();
        create_reflection(
            store,
            store,
            ids,
            target.id(),
            "mornings worked",
            vec![ObservationId::new(1)],
        )
        .unwrap()
        .id()
    }

    #[test]
    fn propose_process_change_requires_an_existing_reflection() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err =
            propose_process_change(&store, &store, &ids, ReflectionId::new(404), "Do mornings")
                .unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "reflection"
            }
        );
    }

    #[test]
    fn process_change_starts_pending_then_is_approved() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let reflection = seed_reflection(&store, &ids);

        let c = propose_process_change(&store, &store, &ids, reflection, "Default to mornings")
            .unwrap();
        assert_eq!(c.approval(), ApprovalState::Pending);

        let approved = approve_process_change(&store, c.id()).unwrap();
        assert!(approved.is_approved());
        assert_eq!(
            list_process_changes(&store, reflection).unwrap(),
            vec![approved]
        );
    }

    #[test]
    fn approving_a_rejected_change_is_a_domain_error() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let reflection = seed_reflection(&store, &ids);
        let c = propose_process_change(&store, &store, &ids, reflection, "Default to mornings")
            .unwrap();

        reject_process_change(&store, c.id()).unwrap();
        let err = approve_process_change(&store, c.id()).unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn approving_a_missing_change_is_not_found() {
        let store = FakeStore::default();
        let err = approve_process_change(&store, ProcessChangeId::new(1)).unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "process change"
            }
        );
    }

    #[test]
    fn deciding_a_stored_approved_change_permits_and_audits() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(1_000);
        let audit = FakeAudit::default();
        let reflection = seed_reflection(&store, &ids);
        let c = propose_process_change(&store, &store, &ids, reflection, "Do mornings").unwrap();
        approve_process_change(&store, c.id()).unwrap();

        let decision = decide_stored_process_change(
            &store,
            &ids,
            &clock,
            &audit,
            c.id(),
            AutonomyLevel::ActWithinPolicy,
        )
        .unwrap();
        assert_eq!(decision, PolicyDecision::Permit);
        assert_eq!(recent_audit(&audit, 10).unwrap().len(), 1);
    }

    #[test]
    fn activity_merges_observations_and_decisions_newest_first() {
        use super::{ActivityKind, recent_activity};
        use crate::ports::ObservationRepository;
        use endora_domain::{AuditId, AuditRecord, ExperimentId, Observation, ObservationId};

        let store = FakeStore::default();
        let audit = FakeAudit::default();

        // Two observations and one audited decision, at distinct times.
        ObservationRepository::save(
            &store,
            &Observation::record(
                ObservationId::new(1),
                ExperimentId::new(9),
                "ran at 7am",
                Timestamp::from_unix_millis(100),
            )
            .unwrap(),
        )
        .unwrap();
        ObservationRepository::save(
            &store,
            &Observation::record(
                ObservationId::new(2),
                ExperimentId::new(9),
                "slept in",
                Timestamp::from_unix_millis(300),
            )
            .unwrap(),
        )
        .unwrap();
        audit
            .append(
                &AuditRecord::new(
                    AuditId::new(3),
                    Timestamp::from_unix_millis(200),
                    "policy permitted change 5",
                )
                .unwrap(),
            )
            .unwrap();

        let events = FakeEvents::default();
        events
            .record(Timestamp::from_unix_millis(400), "Used the weather skill")
            .unwrap();

        // Merged and ordered newest first: 400 (action), 300 (obs), 200 (decision),
        // 100 (obs).
        let feed = recent_activity(&store, &audit, &events, 10).unwrap();
        assert_eq!(feed.len(), 4);
        assert_eq!(feed[0].summary(), "Used the weather skill");
        assert_eq!(feed[0].kind(), ActivityKind::Action);
        assert_eq!(feed[1].summary(), "slept in");
        assert_eq!(feed[1].kind(), ActivityKind::Observation);
        assert_eq!(feed[2].kind(), ActivityKind::Decision);
        assert_eq!(feed[3].summary(), "ran at 7am");

        // The limit truncates after the merge, keeping the newest.
        let top = recent_activity(&store, &audit, &events, 1).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].summary(), "Used the weather skill");
    }

    #[test]
    fn deciding_a_missing_change_is_not_found() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let clock = FixedClock(0);
        let audit = FakeAudit::default();
        let err = decide_stored_process_change(
            &store,
            &ids,
            &clock,
            &audit,
            ProcessChangeId::new(1),
            AutonomyLevel::Observe,
        )
        .unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "process change"
            }
        );
    }

    /// A proposer that returns a canned line, or a failure, for tests.
    struct FakeProposer(Result<String, ProposalError>);

    impl Proposer for FakeProposer {
        fn propose_process_change(
            &self,
            _summary: &str,
            _evidence_count: usize,
        ) -> Result<String, ProposalError> {
            self.0.clone()
        }
    }

    #[test]
    fn drafting_stores_a_pending_change_from_the_model() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let reflection = seed_reflection(&store, &ids);
        let proposer = FakeProposer(Ok("Default runs to mornings".to_owned()));

        let change = draft_process_change(&store, &store, &ids, &proposer, reflection).unwrap();
        assert_eq!(change.approval(), ApprovalState::Pending);
        assert_eq!(change.description(), "Default runs to mornings");
        // The drafted proposal is an ordinary pending change awaiting approval.
        assert_eq!(
            list_process_changes(&store, reflection).unwrap(),
            vec![change]
        );
    }

    #[test]
    fn drafting_against_a_missing_reflection_is_not_found() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let proposer = FakeProposer(Ok("x".to_owned()));
        let err = draft_process_change(&store, &store, &ids, &proposer, ReflectionId::new(1))
            .unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "reflection"
            }
        );
    }

    #[test]
    fn an_unavailable_model_is_a_model_error() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let reflection = seed_reflection(&store, &ids);
        let proposer = FakeProposer(Err(ProposalError::Unavailable("no server".to_owned())));
        let err = draft_process_change(&store, &store, &ids, &proposer, reflection).unwrap_err();
        assert!(matches!(err, AppError::Model { .. }));
    }
}
