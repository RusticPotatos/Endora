//! Use cases for the Direction & Goals slice.
//!
//! These orchestrate the domain and the ports. They are the seam the interfaces
//! (node, CLI) call; they hold no transport or storage detail. Identifiers and
//! time come from the [`IdSource`] and [`Clock`] ports, so the domain stays pure
//! and the use cases stay testable with fakes.

use endora_domain::{
    Assumption, AssumptionId, AuditId, AuditRecord, AutonomyLevel, Direction, DirectionId,
    Experiment, ExperimentId, Goal, GoalId, Observation, ObservationId, PolicyDecision,
    ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId, authorize_process_change,
};

use crate::error::AppError;
use crate::ports::{
    AssumptionRepository, AuditLog, Clock, DirectionRepository, ExperimentRepository,
    GoalRepository, IdSource, ObservationRepository, ProcessChangeRepository, ReflectionRepository,
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

/// Creates and stores a new [`Goal`] under an existing direction.
///
/// # Errors
/// [`AppError::NotFound`] if the direction does not exist, [`AppError::Domain`]
/// if the statement is invalid, or [`AppError::Repository`] if persistence fails.
pub fn create_goal(
    directions: &impl DirectionRepository,
    goals: &impl GoalRepository,
    ids: &impl IdSource,
    direction: DirectionId,
    statement: &str,
) -> Result<Goal, AppError> {
    if directions.get(direction)?.is_none() {
        return Err(AppError::NotFound {
            entity: "direction",
        });
    }
    let goal = Goal::new(GoalId::new(ids.new_id()), direction, statement)?;
    goals.save(&goal)?;
    Ok(goal)
}

/// Lists the goals under a direction, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_goals(
    goals: &impl GoalRepository,
    direction: DirectionId,
) -> Result<Vec<Goal>, AppError> {
    Ok(goals.list_for_direction(direction)?)
}

/// Creates and stores a new [`Assumption`] under an existing goal.
///
/// # Errors
/// [`AppError::NotFound`] if the goal does not exist, [`AppError::Domain`] if
/// the statement is invalid, or [`AppError::Repository`] if persistence fails.
pub fn create_assumption(
    goals: &impl GoalRepository,
    assumptions: &impl AssumptionRepository,
    ids: &impl IdSource,
    goal: GoalId,
    statement: &str,
) -> Result<Assumption, AppError> {
    if goals.get(goal)?.is_none() {
        return Err(AppError::NotFound { entity: "goal" });
    }
    let assumption = Assumption::new(AssumptionId::new(ids.new_id()), goal, statement)?;
    assumptions.save(&assumption)?;
    Ok(assumption)
}

/// Lists the assumptions under a goal, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_assumptions(
    assumptions: &impl AssumptionRepository,
    goal: GoalId,
) -> Result<Vec<Assumption>, AppError> {
    Ok(assumptions.list_for_goal(goal)?)
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
/// an existing goal.
///
/// # Errors
/// [`AppError::NotFound`] if the goal does not exist, [`AppError::Domain`] if the
/// summary is blank or no evidence is cited, or [`AppError::Repository`] on
/// failure.
pub fn create_reflection(
    goals: &impl GoalRepository,
    reflections: &impl ReflectionRepository,
    ids: &impl IdSource,
    goal: GoalId,
    summary: &str,
    evidence: Vec<ObservationId>,
) -> Result<Reflection, AppError> {
    if goals.get(goal)?.is_none() {
        return Err(AppError::NotFound { entity: "goal" });
    }
    let reflection = Reflection::new(ReflectionId::new(ids.new_id()), goal, summary, evidence)?;
    reflections.save(&reflection)?;
    Ok(reflection)
}

/// Lists the reflections for a goal, in a stable order.
///
/// # Errors
/// [`AppError::Repository`] if persistence fails.
pub fn list_reflections(
    reflections: &impl ReflectionRepository,
    goal: GoalId,
) -> Result<Vec<Reflection>, AppError> {
    Ok(reflections.list_for_goal(goal)?)
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

/// Returns the most recent audit records, newest first, up to `limit`.
///
/// # Errors
/// [`AppError::Repository`] if the backend fails.
pub fn recent_audit(audit: &impl AuditLog, limit: usize) -> Result<Vec<AuditRecord>, AppError> {
    Ok(audit.recent(limit)?)
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
    use super::{
        approve_process_change, conclude_experiment, create_assumption, create_direction,
        create_goal, create_reflection, decide_process_change, decide_stored_process_change,
        list_assumptions, list_experiments, list_goals, list_observations, list_process_changes,
        list_reflections, propose_experiment, propose_process_change, recent_audit,
        record_observation, reject_process_change, start_experiment,
    };
    use crate::error::AppError;
    use crate::ports::{
        AssumptionRepository, AuditLog, Clock, DirectionRepository, ExperimentRepository,
        GoalRepository, IdSource, ObservationRepository, ProcessChangeRepository,
        ReflectionRepository, RepositoryError,
    };
    use endora_domain::{
        ApprovalState, Assumption, AssumptionId, AuditRecord, AutonomyLevel, Direction,
        DirectionId, Experiment, ExperimentId, ExperimentStatus, Goal, GoalId, Observation,
        ObservationId, PolicyDecision, ProcessChangeId, ProposedProcessChange, Reflection,
        ReflectionId, Timestamp,
    };
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// An in-memory store implementing the repository ports, for tests only.
    #[derive(Default)]
    struct FakeStore {
        directions: RefCell<HashMap<u128, Direction>>,
        goals: RefCell<HashMap<u128, Goal>>,
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
        fn list_for_goal(&self, goal: GoalId) -> Result<Vec<Reflection>, RepositoryError> {
            let mut found: Vec<Reflection> = self
                .reflections
                .borrow()
                .values()
                .filter(|r| r.goal() == goal)
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
        fn list_for_goal(&self, goal: GoalId) -> Result<Vec<Assumption>, RepositoryError> {
            let mut found: Vec<Assumption> = self
                .assumptions
                .borrow()
                .values()
                .filter(|a| a.goal() == goal)
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
    }

    impl GoalRepository for FakeStore {
        fn save(&self, goal: &Goal) -> Result<(), RepositoryError> {
            self.goals
                .borrow_mut()
                .insert(goal.id().value(), goal.clone());
            Ok(())
        }
        fn get(&self, id: GoalId) -> Result<Option<Goal>, RepositoryError> {
            Ok(self.goals.borrow().get(&id.value()).cloned())
        }
        fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Goal>, RepositoryError> {
            let mut found: Vec<Goal> = self
                .goals
                .borrow()
                .values()
                .filter(|g| g.direction() == direction)
                .cloned()
                .collect();
            found.sort_by_key(|g| g.id().value());
            Ok(found)
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
    fn create_goal_requires_an_existing_direction() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_goal(&store, &store, &ids, DirectionId::new(999), "Run a 5k").unwrap_err();
        assert_eq!(
            err,
            AppError::NotFound {
                entity: "direction"
            }
        );
    }

    #[test]
    fn create_goal_under_a_direction_then_list() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();

        let g1 = create_goal(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        let g2 = create_goal(&store, &store, &ids, direction.id(), "Sleep 8h").unwrap();

        assert_eq!(list_goals(&store, direction.id()).unwrap(), vec![g1, g2]);
    }

    #[test]
    fn invalid_title_is_a_domain_error() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_direction(&store, &ids, "   ").unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn create_assumption_requires_an_existing_goal() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_assumption(
            &store,
            &store,
            &ids,
            GoalId::new(404),
            "Mornings are freest",
        )
        .unwrap_err();
        assert_eq!(err, AppError::NotFound { entity: "goal" });
    }

    #[test]
    fn create_assumption_under_a_goal_then_list() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let goal = create_goal(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();

        let a1 = create_assumption(&store, &store, &ids, goal.id(), "Mornings are freest").unwrap();
        let a2 = create_assumption(&store, &store, &ids, goal.id(), "Rain is rare").unwrap();

        assert_eq!(list_assumptions(&store, goal.id()).unwrap(), vec![a1, a2]);
    }

    /// Builds direction → goal → assumption and returns the assumption id.
    fn seed_assumption(store: &FakeStore, ids: &SeqIds) -> AssumptionId {
        let direction = create_direction(store, ids, "Be healthier").unwrap();
        let goal = create_goal(store, store, ids, direction.id(), "Run a 5k").unwrap();
        create_assumption(store, store, ids, goal.id(), "Mornings are freest")
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
    fn create_reflection_requires_an_existing_goal() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let err = create_reflection(
            &store,
            &store,
            &ids,
            GoalId::new(404),
            "went well",
            vec![ObservationId::new(1)],
        )
        .unwrap_err();
        assert_eq!(err, AppError::NotFound { entity: "goal" });
    }

    #[test]
    fn create_reflection_requires_evidence() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let goal = create_goal(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        let err =
            create_reflection(&store, &store, &ids, goal.id(), "went well", vec![]).unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn create_reflection_then_list() {
        let store = FakeStore::default();
        let ids = SeqIds::default();
        let direction = create_direction(&store, &ids, "Be healthier").unwrap();
        let goal = create_goal(&store, &store, &ids, direction.id(), "Run a 5k").unwrap();
        let r = create_reflection(
            &store,
            &store,
            &ids,
            goal.id(),
            "mornings worked",
            vec![ObservationId::new(1), ObservationId::new(2)],
        )
        .unwrap();
        assert_eq!(list_reflections(&store, goal.id()).unwrap(), vec![r]);
    }

    /// Builds direction → goal → reflection and returns the reflection id.
    fn seed_reflection(store: &FakeStore, ids: &SeqIds) -> ReflectionId {
        let direction = create_direction(store, ids, "Be healthier").unwrap();
        let goal = create_goal(store, store, ids, direction.id(), "Run a 5k").unwrap();
        create_reflection(
            store,
            store,
            ids,
            goal.id(),
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
}
