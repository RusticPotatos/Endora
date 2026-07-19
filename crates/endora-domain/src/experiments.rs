//! Experiments & Learning context.
//!
//! An [`Experiment`] is a small, bounded test of an assumption. It moves through
//! a deliberate lifecycle, and evidence gathered while it runs is captured as
//! [`Observation`]s.

use crate::error::{DomainError, require_non_empty};
use crate::ids::{AssumptionId, ExperimentId, ObservationId, Timestamp};

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
    /// A stable, lowercase name for the state (used in error messages).
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Running => "running",
            Self::Concluded => "concluded",
        }
    }
}

/// A small, bounded test of an [`Assumption`](crate::goals::Assumption).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experiment {
    id: ExperimentId,
    assumption: AssumptionId,
    hypothesis: String,
    status: ExperimentStatus,
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
    use crate::error::DomainError;
    use crate::ids::{AssumptionId, ExperimentId, ObservationId, Timestamp};

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
