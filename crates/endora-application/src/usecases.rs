//! Use cases for the Direction & Targets slice.
//!
//! These orchestrate the domain and the ports. They are the seam the interfaces
//! (node, CLI) call; they hold no transport or storage detail. Identifiers and
//! time come from the [`IdSource`] and [`Clock`] ports, so the domain stays pure
//! and the use cases stay testable with fakes.

use endora_domain::{
    Assumption, AssumptionId, AuditId, AuditRecord, AutonomyLevel, Direction, DirectionId,
    Experiment, ExperimentId, LifecycleStatus, Observation, ObservationId, PolicyDecision,
    ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId, Target, TargetId, Timestamp,
    authorize_process_change,
};

use crate::error::AppError;
use crate::ports::{
    AssumptionRepository, AuditLog, Clock, DirectionRepository, ExperimentRepository, IdSource,
    MemorySnapshot, MemoryStore, ObservationRepository, ProcessChangeRepository, Proposer,
    ReflectionRepository, TargetRepository,
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
}

impl ActivityKind {
    /// A stable, lowercase name, suitable for the protocol and the UI.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Decision => "decision",
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
        approve_process_change, conclude_experiment, create_assumption, create_direction,
        create_reflection, create_target, decide_process_change, decide_stored_process_change,
        delete_direction, delete_target, list_assumptions, list_due_reviews, list_experiments,
        list_observations, list_process_changes, list_reflections, list_targets,
        propose_experiment, propose_process_change, recent_audit, record_observation,
        reject_process_change, schedule_experiment_review, set_direction_status, set_target_status,
        start_experiment,
    };
    use crate::error::AppError;
    use crate::ports::{
        AssumptionRepository, AuditLog, Clock, DirectionRepository, ExperimentRepository, IdSource,
        ObservationRepository, ProcessChangeRepository, ProposalError, Proposer,
        ReflectionRepository, RepositoryError, TargetRepository,
    };
    use endora_domain::LifecycleStatus;
    use endora_domain::{
        ApprovalState, Assumption, AssumptionId, AuditRecord, AutonomyLevel, Direction,
        DirectionId, Experiment, ExperimentId, ExperimentStatus, Observation, ObservationId,
        PolicyDecision, ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId, Target,
        TargetId, Timestamp,
    };
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// An in-memory store implementing the repository ports, for tests only.
    #[derive(Default)]
    struct FakeStore {
        directions: RefCell<HashMap<u128, Direction>>,
        targets: RefCell<HashMap<u128, Target>>,
        assumptions: RefCell<HashMap<u128, Assumption>>,
        experiments: RefCell<HashMap<u128, Experiment>>,
        observations: RefCell<HashMap<u128, Observation>>,
        reflections: RefCell<HashMap<u128, Reflection>>,
        changes: RefCell<HashMap<u128, ProposedProcessChange>>,
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

        // Merged and ordered newest first: 300 (obs), 200 (decision), 100 (obs).
        let feed = recent_activity(&store, &audit, 10).unwrap();
        assert_eq!(feed.len(), 3);
        assert_eq!(feed[0].summary(), "slept in");
        assert_eq!(feed[0].kind(), ActivityKind::Observation);
        assert_eq!(feed[1].kind(), ActivityKind::Decision);
        assert_eq!(feed[2].summary(), "ran at 7am");

        // The limit truncates after the merge, keeping the newest.
        let top = recent_activity(&store, &audit, 1).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].summary(), "slept in");
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
