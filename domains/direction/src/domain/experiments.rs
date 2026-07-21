//! Experiments & Learning context.
//!
//! An [`Experiment`] is a small, bounded test of an assumption. It moves through
//! a deliberate lifecycle, and evidence gathered while it runs is captured as
//! [`Observation`]s.

use endora_kernel::error::{DomainError, require_non_empty};
use endora_kernel::ids::{AssumptionId, ExperimentId, ObservationId, Timestamp};

/// The lifecycle state of an [`Experiment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperimentStatus {
    /// Designed but not yet started.
    Proposed,
    /// Currently running; observations may be recorded.
    Running,
    /// Finished; no further observations are expected.
    Concluded,
}

impl ExperimentStatus {
    /// A stable, lowercase name for the state. Stable enough for storage and
    /// error messages; the round trip with [`from_name`](Self::from_name) is
    /// part of the contract.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Running => "running",
            Self::Concluded => "concluded",
        }
    }

    /// Parses a status from its [`name`](Self::name), or `None` if unrecognized.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "proposed" => Some(Self::Proposed),
            "running" => Some(Self::Running),
            "concluded" => Some(Self::Concluded),
            _ => None,
        }
    }
}

/// A small, bounded test of an [`Assumption`](crate::domain::targets::Assumption).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experiment {
    id: ExperimentId,
    assumption: AssumptionId,
    hypothesis: String,
    status: ExperimentStatus,
    /// When the person asked to be reminded to review this experiment, if ever.
    review_by: Option<Timestamp>,
}

impl Experiment {
    /// Designs a new experiment in the [`ExperimentStatus::Proposed`] state.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `hypothesis` is blank.
    pub fn propose(
        id: ExperimentId,
        assumption: AssumptionId,
        hypothesis: &str,
    ) -> Result<Self, DomainError> {
        let hypothesis = require_non_empty("experiment.hypothesis", hypothesis)?;
        Ok(Self {
            id,
            assumption,
            hypothesis,
            status: ExperimentStatus::Proposed,
            review_by: None,
        })
    }

    /// Reconstitutes an experiment from persisted parts, including its stored
    /// `status`.
    ///
    /// This is for **storage adapters** loading a previously-saved experiment;
    /// it restores state rather than running the lifecycle. Prefer [`propose`]
    /// for new experiments.
    ///
    /// [`propose`]: Self::propose
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `hypothesis` is blank.
    pub fn from_parts(
        id: ExperimentId,
        assumption: AssumptionId,
        hypothesis: &str,
        status: ExperimentStatus,
        review_by: Option<Timestamp>,
    ) -> Result<Self, DomainError> {
        let hypothesis = require_non_empty("experiment.hypothesis", hypothesis)?;
        Ok(Self {
            id,
            assumption,
            hypothesis,
            status,
            review_by,
        })
    }

    /// The experiment's identifier.
    #[must_use]
    pub const fn id(&self) -> ExperimentId {
        self.id
    }

    /// The assumption this experiment tests.
    #[must_use]
    pub const fn assumption(&self) -> AssumptionId {
        self.assumption
    }

    /// The hypothesis being tested.
    #[must_use]
    pub fn hypothesis(&self) -> &str {
        &self.hypothesis
    }

    /// The current lifecycle state.
    #[must_use]
    pub const fn status(&self) -> ExperimentStatus {
        self.status
    }

    /// Starts a proposed experiment.
    ///
    /// # Errors
    /// [`DomainError::InvalidTransition`] unless the experiment is currently
    /// [`ExperimentStatus::Proposed`].
    pub fn start(&mut self) -> Result<(), DomainError> {
        if self.status != ExperimentStatus::Proposed {
            return Err(DomainError::InvalidTransition {
                from: self.status.name(),
                to: "running",
            });
        }
        self.status = ExperimentStatus::Running;
        Ok(())
    }

    /// Concludes a running experiment.
    ///
    /// # Errors
    /// [`DomainError::InvalidTransition`] unless the experiment is currently
    /// [`ExperimentStatus::Running`].
    pub fn conclude(&mut self) -> Result<(), DomainError> {
        if self.status != ExperimentStatus::Running {
            return Err(DomainError::InvalidTransition {
                from: self.status.name(),
                to: "concluded",
            });
        }
        self.status = ExperimentStatus::Concluded;
        Ok(())
    }

    /// When a review of this experiment is due, if the person scheduled one.
    #[must_use]
    pub const fn review_by(&self) -> Option<Timestamp> {
        self.review_by
    }

    /// Schedules (or reschedules) a reminder to review this experiment at `at`.
    ///
    /// This is a *reminder* only — it changes no lifecycle state and takes no
    /// action; see `docs/adr/0010-autonomy-model.md`.
    pub const fn schedule_review(&mut self, at: Timestamp) {
        self.review_by = Some(at);
    }

    /// Whether a review is due as of `now`: a review was scheduled for a time at
    /// or before `now`, and the experiment is not already concluded.
    #[must_use]
    pub fn is_review_due(&self, now: Timestamp) -> bool {
        matches!(self.review_by, Some(at) if at <= now)
            && self.status != ExperimentStatus::Concluded
    }
}

/// Recorded evidence from an [`Experiment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    id: ObservationId,
    experiment: ExperimentId,
    note: String,
    recorded_at: Timestamp,
}

impl Observation {
    /// Records an observation against an experiment at a caller-supplied time.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `note` is blank.
    pub fn record(
        id: ObservationId,
        experiment: ExperimentId,
        note: &str,
        recorded_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let note = require_non_empty("observation.note", note)?;
        Ok(Self {
            id,
            experiment,
            note,
            recorded_at,
        })
    }

    /// The observation's identifier.
    #[must_use]
    pub const fn id(&self) -> ObservationId {
        self.id
    }

    /// The experiment this observation belongs to.
    #[must_use]
    pub const fn experiment(&self) -> ExperimentId {
        self.experiment
    }

    /// The recorded note.
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }

    /// When the observation was recorded.
    #[must_use]
    pub const fn recorded_at(&self) -> Timestamp {
        self.recorded_at
    }
}

#[cfg(test)]
mod tests {
    use super::{Experiment, ExperimentStatus, Observation};
    use endora_kernel::error::DomainError;
    use endora_kernel::ids::{AssumptionId, ExperimentId, ObservationId, Timestamp};

    fn proposed() -> Experiment {
        Experiment::propose(
            ExperimentId::new(1),
            AssumptionId::new(1),
            "Morning runs stick",
        )
        .unwrap()
    }

    #[test]
    fn a_new_experiment_is_proposed() {
        assert_eq!(proposed().status(), ExperimentStatus::Proposed);
    }

    #[test]
    fn experiment_rejects_empty_hypothesis() {
        assert_eq!(
            Experiment::propose(ExperimentId::new(1), AssumptionId::new(1), "  "),
            Err(DomainError::EmptyField {
                field: "experiment.hypothesis"
            })
        );
    }

    #[test]
    fn lifecycle_runs_proposed_to_running_to_concluded() {
        let mut e = proposed();
        e.start().unwrap();
        assert_eq!(e.status(), ExperimentStatus::Running);
        e.conclude().unwrap();
        assert_eq!(e.status(), ExperimentStatus::Concluded);
    }

    #[test]
    fn cannot_conclude_before_running() {
        let mut e = proposed();
        assert_eq!(
            e.conclude(),
            Err(DomainError::InvalidTransition {
                from: "proposed",
                to: "concluded"
            })
        );
    }

    #[test]
    fn cannot_start_twice() {
        let mut e = proposed();
        e.start().unwrap();
        assert_eq!(
            e.start(),
            Err(DomainError::InvalidTransition {
                from: "running",
                to: "running"
            })
        );
    }

    #[test]
    fn observation_keeps_its_time_and_note() {
        let at = Timestamp::from_unix_millis(1_000);
        let o = Observation::record(ObservationId::new(1), ExperimentId::new(1), "felt good", at)
            .unwrap();
        assert_eq!(o.recorded_at(), at);
        assert_eq!(o.note(), "felt good");
    }

    #[test]
    fn status_names_round_trip() {
        for s in [
            ExperimentStatus::Proposed,
            ExperimentStatus::Running,
            ExperimentStatus::Concluded,
        ] {
            assert_eq!(ExperimentStatus::from_name(s.name()), Some(s));
        }
        assert_eq!(ExperimentStatus::from_name("bogus"), None);
    }

    #[test]
    fn from_parts_restores_a_stored_status() {
        let e = Experiment::from_parts(
            ExperimentId::new(1),
            AssumptionId::new(1),
            "Morning runs stick",
            ExperimentStatus::Running,
            None,
        )
        .unwrap();
        assert_eq!(e.status(), ExperimentStatus::Running);
    }

    #[test]
    fn a_review_is_due_once_its_time_passes() {
        let mut e = proposed();
        assert!(!e.is_review_due(Timestamp::from_unix_millis(1_000)));
        e.schedule_review(Timestamp::from_unix_millis(500));
        assert!(!e.is_review_due(Timestamp::from_unix_millis(400))); // not yet
        assert!(e.is_review_due(Timestamp::from_unix_millis(500))); // due
        assert!(e.is_review_due(Timestamp::from_unix_millis(1_000))); // still due
    }

    #[test]
    fn a_concluded_experiment_is_never_due_for_review() {
        let mut e = proposed();
        e.schedule_review(Timestamp::from_unix_millis(100));
        e.start().unwrap();
        e.conclude().unwrap();
        assert!(!e.is_review_due(Timestamp::from_unix_millis(1_000)));
    }

    #[test]
    fn observation_rejects_empty_note() {
        assert_eq!(
            Observation::record(
                ObservationId::new(1),
                ExperimentId::new(1),
                "",
                Timestamp::from_unix_millis(0)
            ),
            Err(DomainError::EmptyField {
                field: "observation.note"
            })
        );
    }
}
