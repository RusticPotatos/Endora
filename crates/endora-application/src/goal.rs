//! Small, durable-friendly goal state machine for delegated work.
//!
//! A goal is the application boundary between a person's objective and the
//! capabilities that may help with it. Models may suggest steps; this record
//! owns progress and terminal state. The checkpoint is deliberately JSON so it
//! can be stored by an existing repository before a dedicated goal table exists.

use endora_kernel::Timestamp;
use serde_json::{Value, json};

/// Version of the serialized goal checkpoint.
pub const GOAL_CHECKPOINT_VERSION: u64 = 1;

/// Lifecycle of a delegated goal. Terminal states cannot be reopened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GoalStatus {
    Planned,
    Running,
    Waiting,
    Complete,
    Partial,
    Blocked,
    Failed,
    Cancelled,
}

#[allow(missing_docs)]
impl GoalStatus {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "planned" => Self::Planned,
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "complete" => Self::Complete,
            "partial" => Self::Partial,
            "blocked" => Self::Blocked,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Complete | Self::Partial | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }
}

/// Result of checking a goal's evidence against its success criteria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum GoalVerdict {
    Continue,
    Complete,
    Partial,
    Blocked,
    Failed,
}

/// One validated step in a bounded goal plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalStep {
    /// The phase shown in the checkpoint while this step runs.
    pub phase: String,
    /// Stable step name, usually a capability id.
    pub name: String,
}

impl GoalStep {
    /// Creates a plan step.
    pub fn new(phase: &str, name: &str) -> Result<Self, &'static str> {
        if phase.trim().is_empty() || name.trim().is_empty() {
            return Err("goal step fields must not be empty");
        }
        Ok(Self {
            phase: phase.trim().to_owned(),
            name: name.trim().to_owned(),
        })
    }
}

/// One committed result from a goal step.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct GoalEvidence {
    pub step: String,
    pub output: String,
}

/// Mutable execution record for one objective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    objective: String,
    status: GoalStatus,
    phase: String,
    next_step: String,
    reason: String,
    evidence: Vec<GoalEvidence>,
}

/// A goal together with the stable request key and timestamps needed to resume it.
/// The request key is immutable: retries of the same request address this record
/// instead of creating another worker-owned goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalRecord {
    request_key: String,
    goal: Goal,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl GoalRecord {
    /// Creates a new persisted goal record.
    pub fn new(request_key: &str, goal: Goal, at: Timestamp) -> Result<Self, &'static str> {
        let request_key = request_key.trim();
        if request_key.is_empty() || request_key.chars().count() > 200 {
            return Err("goal request key must be 1–200 characters");
        }
        Ok(Self {
            request_key: request_key.to_owned(),
            goal,
            created_at: at,
            updated_at: at,
        })
    }

    /// Returns the idempotency key supplied with the request.
    #[must_use]
    pub fn request_key(&self) -> &str {
        &self.request_key
    }

    /// Returns the mutable goal state.
    #[must_use]
    pub const fn goal(&self) -> &Goal {
        &self.goal
    }

    /// Returns the creation time.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// Returns the last checkpoint time.
    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }

    /// Replaces the mutable goal state while preserving identity and creation time.
    #[must_use]
    pub fn with_goal(&self, goal: Goal, at: Timestamp) -> Self {
        Self {
            request_key: self.request_key.clone(),
            goal,
            created_at: self.created_at,
            updated_at: at,
        }
    }
}

#[allow(missing_docs)]
impl Goal {
    /// Runs a validated plan, one step at a time, until the verifier finishes it
    /// or the bounded plan is exhausted. The executor is the only code allowed
    /// to perform I/O; the verifier is the only code allowed to finish success.
    pub fn run_plan<F, V>(&mut self, plan: &[GoalStep], mut execute: F, mut verify: V) -> GoalStatus
    where
        F: FnMut(&GoalStep) -> Result<String, String>,
        V: FnMut(&Goal, &GoalStep) -> GoalVerdict,
    {
        const MAX_STEPS: usize = 8;
        if self.status.terminal() {
            return self.status;
        }
        if plan.is_empty() {
            self.apply(GoalVerdict::Failed, "the goal had no executable steps");
            return self.status;
        }
        for step in plan.iter().take(MAX_STEPS) {
            self.start(&step.phase, &step.name);
            match execute(step) {
                Ok(output) if !output.trim().is_empty() => {
                    self.commit(&step.name, &output);
                    let verdict = verify(self, step);
                    self.apply(verdict, "the verifier evaluated the committed evidence");
                    if self.status.terminal() {
                        return self.status;
                    }
                }
                Ok(_) => {
                    self.apply(GoalVerdict::Partial, "the step returned no evidence");
                    return self.status;
                }
                Err(reason) => {
                    self.apply(GoalVerdict::Failed, &reason);
                    return self.status;
                }
            }
        }
        self.apply(
            GoalVerdict::Partial,
            "the bounded goal plan ended before completion",
        );
        self.status
    }

    /// Creates a planned goal from the user's objective.
    pub fn new(objective: &str) -> Result<Self, &'static str> {
        let objective = objective.trim();
        if objective.is_empty() {
            return Err("goal objective must not be empty");
        }
        Ok(Self {
            objective: objective.to_owned(),
            status: GoalStatus::Planned,
            phase: "planning".to_owned(),
            next_step: String::new(),
            reason: String::new(),
            evidence: Vec::new(),
        })
    }

    /// Marks the next bounded step as running.
    pub fn start(&mut self, phase: &str, step: &str) -> bool {
        if self.status.terminal() || phase.trim().is_empty() || step.trim().is_empty() {
            return false;
        }
        self.status = GoalStatus::Running;
        self.phase = phase.trim().to_owned();
        self.next_step = step.trim().to_owned();
        self.reason.clear();
        true
    }

    /// Commits a step's result before the verifier decides what happens next.
    pub fn commit(&mut self, step: &str, output: &str) -> bool {
        if self.status.terminal() || step.trim().is_empty() || output.trim().is_empty() {
            return false;
        }
        self.evidence.push(GoalEvidence {
            step: step.trim().to_owned(),
            output: output.to_owned(),
        });
        self.next_step.clear();
        true
    }

    /// Applies a deterministic verifier verdict.
    pub fn apply(&mut self, verdict: GoalVerdict, reason: &str) {
        if self.status.terminal() {
            return;
        }
        self.reason = reason.trim().to_owned();
        self.status = match verdict {
            GoalVerdict::Continue => GoalStatus::Running,
            GoalVerdict::Complete => GoalStatus::Complete,
            GoalVerdict::Partial => GoalStatus::Partial,
            GoalVerdict::Blocked => GoalStatus::Blocked,
            GoalVerdict::Failed => GoalStatus::Failed,
        };
    }

    /// Puts a goal into a resumable waiting state.
    pub fn wait(&mut self, phase: &str, step: &str) {
        if self.status.terminal() {
            return;
        }
        self.status = GoalStatus::Waiting;
        self.phase = phase.to_owned();
        self.next_step = step.to_owned();
    }

    /// Cancels the goal. Cancellation is terminal and cannot be undone by a late result.
    pub fn cancel(&mut self, reason: &str) {
        if self.status.terminal() {
            return;
        }
        self.status = GoalStatus::Cancelled;
        self.reason = reason.to_owned();
        self.next_step.clear();
    }

    #[must_use]
    pub fn objective(&self) -> &str {
        &self.objective
    }
    #[must_use]
    pub const fn status(&self) -> GoalStatus {
        self.status
    }
    #[must_use]
    pub fn phase(&self) -> &str {
        &self.phase
    }
    #[must_use]
    pub fn next_step(&self) -> &str {
        &self.next_step
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub fn evidence(&self) -> &[GoalEvidence] {
        &self.evidence
    }

    /// Serializes the resumable state. Unknown or newer versions must not be resumed silently.
    #[must_use]
    pub fn checkpoint(&self) -> String {
        json!({
            "version": GOAL_CHECKPOINT_VERSION,
            "status": self.status.name(),
            "phase": self.phase,
            "next_step": self.next_step,
            "reason": self.reason,
            "evidence": self.evidence.iter().map(|e| json!({"step": e.step, "output": e.output})).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// Restores state from a checkpoint produced by [`Self::checkpoint`].
    pub fn from_checkpoint(objective: &str, checkpoint: &str) -> Result<Self, &'static str> {
        let value: Value =
            serde_json::from_str(checkpoint).map_err(|_| "invalid goal checkpoint")?;
        if value["version"].as_u64() != Some(GOAL_CHECKPOINT_VERSION) {
            return Err("unsupported goal checkpoint version");
        }
        let status = value["status"]
            .as_str()
            .and_then(GoalStatus::from_name)
            .ok_or("invalid goal status")?;
        let mut goal = Self::new(objective)?;
        goal.status = status;
        goal.phase = value["phase"].as_str().unwrap_or("planning").to_owned();
        goal.next_step = value["next_step"].as_str().unwrap_or("").to_owned();
        goal.reason = value["reason"].as_str().unwrap_or("").to_owned();
        goal.evidence = value["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(GoalEvidence {
                    step: item["step"].as_str()?.to_owned(),
                    output: item["output"].as_str()?.to_owned(),
                })
            })
            .collect();
        Ok(goal)
    }
}

#[cfg(test)]
mod tests {
    use super::{Goal, GoalStatus, GoalStep, GoalVerdict};

    #[test]
    fn the_runner_iterates_until_a_verifier_marks_the_goal_complete() {
        let mut goal = Goal::new("collect and verify").unwrap();
        let plan = vec![
            GoalStep::new("gathering", "search").unwrap(),
            GoalStep::new("checking", "verify").unwrap(),
        ];
        let mut calls = 0;
        let status = goal.run_plan(
            &plan,
            |_| {
                calls += 1;
                Ok(format!("evidence {calls}"))
            },
            |goal, step| {
                if step.name == "verify" && goal.evidence().len() == 2 {
                    GoalVerdict::Complete
                } else {
                    GoalVerdict::Continue
                }
            },
        );
        assert_eq!(status, GoalStatus::Complete);
        assert_eq!(calls, 2);
        assert_eq!(goal.evidence().len(), 2);
    }

    #[test]
    fn a_goal_commits_evidence_and_only_the_verifier_can_finish_it() {
        let mut goal = Goal::new("find the next three games").unwrap();
        assert_eq!(goal.status(), GoalStatus::Planned);
        assert!(goal.start("gathering", "sports_schedule"));
        assert!(goal.commit("sports_schedule", "three verified games"));
        assert_eq!(goal.status(), GoalStatus::Running);
        goal.apply(GoalVerdict::Complete, "three dates are on or after today");
        assert_eq!(goal.status(), GoalStatus::Complete);
        assert_eq!(goal.evidence().len(), 1);
        assert!(!goal.start("again", "another_call"));
    }

    #[test]
    fn a_checkpoint_round_trips_and_a_late_result_cannot_resurrect_cancellation() {
        let mut goal = Goal::new("find the next three games").unwrap();
        goal.start("gathering", "sports_schedule");
        goal.commit("sports_schedule", "evidence");
        let restored = Goal::from_checkpoint(goal.objective(), &goal.checkpoint()).unwrap();
        assert_eq!(restored, goal);
        goal.cancel("cancelled by the person");
        goal.apply(GoalVerdict::Complete, "late result");
        assert_eq!(goal.status(), GoalStatus::Cancelled);
    }

    #[test]
    fn an_unknown_checkpoint_version_is_visible() {
        let error = Goal::from_checkpoint("an objective", r#"{"version":99}"#).unwrap_err();
        assert_eq!(error, "unsupported goal checkpoint version");
    }
}
