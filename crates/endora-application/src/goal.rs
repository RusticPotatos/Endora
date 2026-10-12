//! Small, durable-friendly goal state machine for delegated work.
//!
//! A goal is the application boundary between a person's objective and the
//! capabilities that may help with it. Models may suggest steps; this record
//! owns progress and terminal state. The checkpoint is deliberately JSON so it
//! can be stored by an existing repository before a dedicated goal table exists.

use endora_kernel::Timestamp;
use serde_json::{Value, json};

/// Version of the serialized goal checkpoint. Version 2 adds the approaches that
/// failed; a version 1 checkpoint still resumes, without that history.
pub const GOAL_CHECKPOINT_VERSION: u64 = 2;

/// How many approaches may fail in a row before the objective stops trying.
///
/// Three is the smallest number that distinguishes bad luck from a dead subject:
/// one refusal is routine, two can be the same outage twice, and three different
/// approaches failing is the objective telling you something.
const ENOUGH_DEAD_ENDS: usize = 3;

/// Operations one pursuit may spend before yielding, so a goal cannot monopolise
/// the model or run unbounded between checkpoints.
const MOST_STEPS_PER_PURSUIT: usize = 8;

/// Why a step produced no evidence, and — the part that matters — whether the
/// objective is still reachable by some other route.
///
/// The distinction is the whole fix. Treating every executor error as terminal
/// meant one source answering 403 ended the thing the person asked for, which is
/// not what a person means by asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    /// This approach failed but the objective is still reachable another way: a
    /// refused source, a timeout, a rate limit, a malformed argument. The goal
    /// records the dead end and chooses something else.
    TryAnother(String),
    /// Nothing further may be attempted — the capability is gone, permission was
    /// withdrawn, the budget is spent. **Never used to shop for a substitute that
    /// defeats the same restriction**; a policy denial stops the goal.
    Stop(String),
}

/// Whether `fresh` tells the goal nothing it does not already have in `held`:
/// every word of it already appears there.
///
/// Measured twice, and both measurements shaped this. Word-for-word equality was
/// tried first and is useless, because the model's repeats are *reworded* — the
/// second pass dropped a clause and said the same thing. The belief rule
/// (`says_the_same_thing`) was tried before that and is unsafe, because it merged
/// "evidence 1" with "evidence 2".
///
/// A subset test threads between them. A finding that introduces a new word —
/// a time, a number, a name, a place — is kept, so "the 41 bus arrives at 14:05"
/// and "...at 15:10" stay two findings, which is the error that must never
/// happen. A finding that only re-arranges words already held adds nothing, and
/// is the repeat worth stopping on. Compared against each held finding
/// separately rather than the union of all of them, so a later genuine finding
/// is not swallowed just because the words it uses have each been seen before.
fn says_nothing_new(held: &str, fresh: &str) -> bool {
    let words = |s: &str| -> std::collections::BTreeSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let (held, fresh) = (words(held), words(fresh));
    !fresh.is_empty() && fresh.is_subset(&held)
}

/// One approach that did not work, kept so the goal does not spend its budget
/// re-trying it. Visible to whatever chooses the next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadEnd {
    /// The step that failed, usually a capability id.
    pub step: String,
    /// What went wrong, in the executor's own words.
    pub why: String,
}

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
    dead_ends: Vec<DeadEnd>,
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
    pub fn run_plan<F, V>(&mut self, plan: &[GoalStep], mut execute: F, verify: V) -> GoalStatus
    where
        F: FnMut(&GoalStep) -> Result<String, String>,
        V: FnMut(&Goal, &GoalStep) -> GoalVerdict,
    {
        if self.status.terminal() {
            return self.status;
        }
        if plan.is_empty() {
            self.apply(GoalVerdict::Failed, "the goal had no executable steps");
            return self.status;
        }
        // A fixed plan is the degenerate case of choosing the next step: walk it in
        // order. Routed through `pursue` so there is one loop rather than two, which
        // also means a failing step here moves on instead of ending the objective.
        let mut next = 0usize;
        self.pursue(
            plan.len(),
            |_| {
                let step = plan.get(next).cloned();
                next += 1;
                step
            },
            |step| execute(step).map_err(StepError::TryAnother),
            verify,
        )
    }

    /// Pursues the objective by repeatedly choosing the next step from what is
    /// known so far — the loop that makes a goal make progress.
    ///
    /// `propose` is handed the goal's current state (its evidence **and** its dead
    /// ends) and returns the next thing to try, or `None` when it has nothing left.
    /// A model may choose; this code decides what happens to the result, which is
    /// the ordinary division of labour — models propose, policy authorizes.
    ///
    /// Three properties are the point, and each one is a defect this replaces:
    ///
    /// - **It iterates.** The worker used to hand a one-element array to a fixed
    ///   plan, so every objective finished after exactly one capability call.
    /// - **A failed approach is not a failed objective.** `TryAnother` records a
    ///   dead end and chooses again; only `Stop` ends it, and only
    ///   [`ENOUGH_DEAD_ENDS`] consecutive failures exhaust it.
    /// - **It is bounded, and pausing is not failing.** `steps` caps the operations
    ///   this pass may spend (at most [`MOST_STEPS_PER_PURSUIT`]). Spending them
    ///   leaves the goal [`GoalStatus::Waiting`] — *not* terminal — so the next
    ///   pass resumes it with its evidence and dead ends intact. Only `propose`
    ///   returning `None` means there is genuinely nothing left to try.
    pub fn pursue<P, F, V>(
        &mut self,
        steps: usize,
        mut propose: P,
        mut execute: F,
        mut verify: V,
    ) -> GoalStatus
    where
        P: FnMut(&Self) -> Option<GoalStep>,
        F: FnMut(&GoalStep) -> Result<String, StepError>,
        V: FnMut(&Self, &GoalStep) -> GoalVerdict,
    {
        if self.status.terminal() {
            return self.status;
        }
        let mut in_a_row = 0usize;
        for _ in 0..steps.min(MOST_STEPS_PER_PURSUIT) {
            let Some(step) = propose(&*self) else {
                // Nothing left to try. What was gathered still counts, but an
                // objective with no evidence at all is blocked, never complete.
                let verdict = if self.evidence.is_empty() {
                    GoalVerdict::Blocked
                } else {
                    GoalVerdict::Partial
                };
                self.apply(verdict, "nothing further could be tried for this objective");
                return self.status;
            };
            self.start(&step.phase, &step.name);
            match execute(&step) {
                Ok(output) if !output.trim().is_empty() => {
                    if !self.commit(&step.name, &output) {
                        // The pass restated something already held. The goal has what
                        // it came for and more passes cannot improve on it, so stop
                        // here with what was found rather than spending the rest of
                        // the budget producing copies. The counter cannot do this job:
                        // it is local to one pursuit, and the worker calls this once
                        // per tick, so a cross-pass stall would never be seen.
                        self.note_dead_end(&step.name, "the pass repeated what was already known");
                        self.apply(
                            GoalVerdict::Partial,
                            "it stopped learning anything new about this",
                        );
                        return self.status;
                    }
                    in_a_row = 0;
                    let verdict = verify(&*self, &step);
                    self.apply(verdict, "the verifier evaluated the committed evidence");
                    if self.status.terminal() {
                        return self.status;
                    }
                }
                Ok(_) => {
                    in_a_row += 1;
                    self.note_dead_end(&step.name, "the step returned no evidence");
                    if in_a_row >= ENOUGH_DEAD_ENDS {
                        self.apply(GoalVerdict::Partial, "the step returned no evidence");
                        return self.status;
                    }
                }
                Err(StepError::TryAnother(why)) => {
                    in_a_row += 1;
                    self.note_dead_end(&step.name, &why);
                    if in_a_row >= ENOUGH_DEAD_ENDS {
                        self.apply(GoalVerdict::Partial, &why);
                        return self.status;
                    }
                }
                Err(StepError::Stop(why)) => {
                    self.note_dead_end(&step.name, &why);
                    self.apply(GoalVerdict::Blocked, &why);
                    return self.status;
                }
            }
        }
        // Out of operations for this pass, not out of objective. This must be
        // `Waiting` and not `Partial`: `Partial` is terminal, so ending a bounded
        // pass with it would stop the very goal the budget exists to let continue —
        // the same one-pass-and-done defect in a new place. `Waiting` is what
        // `active_goals` picks up again.
        self.status = GoalStatus::Waiting;
        self.phase = "gathering".to_owned();
        self.next_step = "continue".to_owned();
        self.reason = "paused after a bounded pass; the next pass resumes it".to_owned();
        self.status
    }

    /// Remembers an approach that did not work, so the next proposal can avoid it.
    fn note_dead_end(&mut self, step: &str, why: &str) {
        self.next_step.clear();
        self.dead_ends.push(DeadEnd {
            step: step.trim().to_owned(),
            why: why.trim().to_owned(),
        });
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
            dead_ends: Vec::new(),
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
    ///
    /// Refuses a finding that tells the goal nothing new. Measured live: once a
    /// pass has answered the objective it restates the answer, and without this the
    /// restatement is stored as a fresh finding and the loop re-runs the same step
    /// until its budget is gone. Returning `false` is what lets [`Self::pursue`] see
    /// that a pass added nothing.
    ///
    /// "Nothing new" is [`says_nothing_new`] — every word already held — and that
    /// rule was arrived at by two failed attempts, both recorded there. Merging two
    /// findings throws away real information **and** stops the objective, so the
    /// bar for calling something a repeat has to be high enough that "the 41 bus
    /// arrives at 14:05" and "...at 15:10" stay two findings, and low enough to
    /// catch a reworded restatement, which is what the model actually produces.
    pub fn commit(&mut self, step: &str, output: &str) -> bool {
        if self.status.terminal() || step.trim().is_empty() || output.trim().is_empty() {
            return false;
        }
        if self
            .evidence
            .iter()
            .any(|held| says_nothing_new(&held.output, output))
        {
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

    /// The approaches that did not work, so whatever chooses the next step can
    /// avoid spending the budget on them again.
    #[must_use]
    pub fn dead_ends(&self) -> &[DeadEnd] {
        &self.dead_ends
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
            "dead_ends": self.dead_ends.iter().map(|d| json!({"step": d.step, "why": d.why})).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// Restores state from a checkpoint produced by [`Self::checkpoint`].
    pub fn from_checkpoint(objective: &str, checkpoint: &str) -> Result<Self, &'static str> {
        let value: Value =
            serde_json::from_str(checkpoint).map_err(|_| "invalid goal checkpoint")?;
        // Every version up to the current one is resumable: a goal written before
        // dead ends existed simply has none, and refusing it would strand work that
        // is otherwise perfectly readable. A *newer* version is still refused,
        // because this build cannot know what it left out.
        match value["version"].as_u64() {
            Some(v) if (1..=GOAL_CHECKPOINT_VERSION).contains(&v) => {}
            _ => return Err("unsupported goal checkpoint version"),
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
        goal.dead_ends = value["dead_ends"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(DeadEnd {
                    step: item["step"].as_str()?.to_owned(),
                    why: item["why"].as_str()?.to_owned(),
                })
            })
            .collect();
        Ok(goal)
    }
}

#[cfg(test)]
mod tests {
    use super::{Goal, GoalStatus, GoalStep, GoalVerdict, StepError};

    /// A step that always works, for tests about the loop rather than the work.
    fn works(output: &str) -> Result<String, StepError> {
        Ok(output.to_owned())
    }

    #[test]
    fn a_goal_keeps_choosing_steps_while_the_evidence_is_incomplete() {
        // The defect this exists for: the worker called `run_plan` with a
        // one-element array, so every objective reached a terminal state after
        // exactly one capability call. Progress means choosing the *next* step
        // from what is known so far, repeatedly.
        let mut goal = Goal::new("find somewhere to eat on Sunday").unwrap();
        let mut proposed = 0;
        let status = goal.pursue(
            8,
            |g| {
                proposed += 1;
                // The proposal adapts to the evidence already gathered.
                match g.evidence().len() {
                    0 => Some(GoalStep::new("gathering", "search").unwrap()),
                    1 => Some(GoalStep::new("gathering", "opening_hours").unwrap()),
                    2 => Some(GoalStep::new("checking", "confirm").unwrap()),
                    _ => None,
                }
            },
            |step| works(&format!("result of {}", step.name)),
            |g, _| {
                if g.evidence().len() == 3 {
                    GoalVerdict::Complete
                } else {
                    GoalVerdict::Continue
                }
            },
        );
        assert_eq!(status, GoalStatus::Complete);
        assert_eq!(proposed, 3, "it should have chosen three successive steps");
        assert_eq!(goal.evidence().len(), 3);
    }

    #[test]
    fn a_refused_source_is_not_the_end_of_the_objective() {
        // Live shape: the first source answered 403. That ended the whole goal,
        // because every executor error was terminal. A refused source is a reason
        // to try another one, not a reason to abandon what was asked for.
        let mut goal = Goal::new("find the next three games").unwrap();
        let mut attempt = 0;
        let status = goal.pursue(
            8,
            |_| {
                attempt += 1;
                match attempt {
                    1 => Some(GoalStep::new("gathering", "first_source").unwrap()),
                    2 => Some(GoalStep::new("gathering", "second_source").unwrap()),
                    _ => None,
                }
            },
            |step| {
                if step.name == "first_source" {
                    return Err(StepError::TryAnother("the source answered 403".to_owned()));
                }
                works("three dated fixtures")
            },
            |_, _| GoalVerdict::Complete,
        );
        assert_eq!(status, GoalStatus::Complete);
        assert_eq!(goal.evidence().len(), 1, "only the source that worked");
        assert_eq!(
            goal.dead_ends().len(),
            1,
            "the refusal is remembered, not forgotten"
        );
        assert_eq!(goal.dead_ends()[0].step, "first_source");
    }

    #[test]
    fn a_dead_end_is_visible_so_the_same_approach_is_not_retried() {
        // Without this the planner re-proposes the approach that just failed, which
        // is a different flavour of the same stall.
        let mut goal = Goal::new("an objective").unwrap();
        goal.pursue(
            8,
            |g| {
                if g.dead_ends().iter().any(|d| d.step == "broken") {
                    Some(GoalStep::new("gathering", "working").unwrap())
                } else {
                    Some(GoalStep::new("gathering", "broken").unwrap())
                }
            },
            |step| {
                if step.name == "broken" {
                    return Err(StepError::TryAnother("still broken".to_owned()));
                }
                works("evidence")
            },
            |_, _| GoalVerdict::Complete,
        );
        assert_eq!(goal.status(), GoalStatus::Complete);
        assert_eq!(goal.evidence()[0].step, "working");
    }

    #[test]
    fn enough_dead_ends_ends_honestly_rather_than_looping() {
        let mut goal = Goal::new("an impossible objective").unwrap();
        let mut tries = 0;
        let status = goal.pursue(
            8,
            |_| {
                tries += 1;
                Some(GoalStep::new("gathering", "nope").unwrap())
            },
            |_| Err(StepError::TryAnother("refused again".to_owned())),
            |_, _| GoalVerdict::Complete,
        );
        assert_eq!(status, GoalStatus::Partial, "it stops, and says why");
        assert!(goal.reason().contains("refused again"), "{}", goal.reason());
        assert!(tries <= 3, "bounded, not an endless retry: {tries}");
    }

    #[test]
    fn a_withdrawn_capability_blocks_instead_of_trying_a_substitute() {
        // Policy denial must not become a search for something that defeats the
        // same restriction.
        let mut goal = Goal::new("an objective").unwrap();
        let status = goal.pursue(
            8,
            |_| Some(GoalStep::new("gathering", "fetch").unwrap()),
            |_| Err(StepError::Stop("web fetch was revoked".to_owned())),
            |_, _| GoalVerdict::Complete,
        );
        assert_eq!(status, GoalStatus::Blocked);
        assert_eq!(goal.reason(), "web fetch was revoked");
    }

    #[test]
    fn nothing_left_to_try_is_blocked_when_nothing_was_gathered() {
        let mut goal = Goal::new("an objective").unwrap();
        let status = goal.pursue(
            8,
            |_| None,
            |_| works("unused"),
            |_, _| GoalVerdict::Complete,
        );
        assert_eq!(
            status,
            GoalStatus::Blocked,
            "no evidence and no approach is blocked, never complete"
        );
    }

    #[test]
    fn nothing_left_to_try_is_partial_when_something_was_gathered() {
        let mut goal = Goal::new("an objective").unwrap();
        let mut first = true;
        let status = goal.pursue(
            8,
            |_| {
                if first {
                    first = false;
                    return Some(GoalStep::new("gathering", "search").unwrap());
                }
                None
            },
            |_| works("something useful but incomplete"),
            |_, _| GoalVerdict::Continue,
        );
        assert_eq!(status, GoalStatus::Partial);
        assert_eq!(goal.evidence().len(), 1, "what it did find is kept");
    }

    #[test]
    fn dead_ends_survive_a_checkpoint_and_an_older_one_still_resumes() {
        let mut goal = Goal::new("an objective").unwrap();
        goal.pursue(
            8,
            |g| (g.dead_ends().is_empty()).then(|| GoalStep::new("gathering", "broken").unwrap()),
            |_| Err(StepError::TryAnother("the source refused".to_owned())),
            |_, _| GoalVerdict::Continue,
        );
        let restored = Goal::from_checkpoint(goal.objective(), &goal.checkpoint()).unwrap();
        assert_eq!(restored, goal, "a resumed goal remembers its dead ends");

        // A checkpoint written before dead ends existed still resumes, without them.
        let older = r#"{"version":1,"status":"running","phase":"gathering",
            "next_step":"search","reason":"","evidence":[]}"#;
        let v1 = Goal::from_checkpoint("an objective", older).unwrap();
        assert_eq!(v1.status(), GoalStatus::Running);
        assert!(v1.dead_ends().is_empty());
    }

    #[test]
    fn spending_a_passs_budget_leaves_the_goal_resumable() {
        // The trap this guards: ending a bounded pass with `Partial` reads like
        // "paused" but `Partial` is terminal, so the goal the budget exists to let
        // continue would stop instead — one-pass-and-done again, one layer down.
        // `Waiting` is the status `active_goals` picks back up.
        let mut goal = Goal::new("a long objective").unwrap();
        let mut passes = 0;
        let status = goal.pursue(
            2,
            |_| Some(GoalStep::new("gathering", "a step").unwrap()),
            |_| {
                passes += 1;
                // Distinct findings: this test is about the budget, and a verbatim
                // repeat would now (correctly) stop the pursuit for a different reason.
                works(&format!("a little more evidence, part {passes}"))
            },
            |_, _| GoalVerdict::Continue,
        );
        assert_eq!(passes, 2, "it spent exactly the budget it was given");
        assert_eq!(status, GoalStatus::Waiting);
        assert!(!status.terminal(), "a paused goal must be resumable");
        assert_eq!(goal.evidence().len(), 2, "and keeps what it gathered");

        // Resuming really does continue rather than start over.
        let status = goal.pursue(
            1,
            |_| Some(GoalStep::new("gathering", "a step").unwrap()),
            |_| works("the last piece"),
            |g, _| {
                if g.evidence().len() == 3 {
                    GoalVerdict::Complete
                } else {
                    GoalVerdict::Continue
                }
            },
        );
        assert_eq!(status, GoalStatus::Complete);
        assert_eq!(goal.evidence().len(), 3);
    }

    #[test]
    fn restating_what_is_already_known_is_not_progress() {
        // Measured live on qwen2.5:14b: pass 1 fetched the page and answered it
        // correctly, then passes 2-6 re-ran the same step and restated the same
        // sentence. Six pieces of evidence, one fact, five wasted model turns —
        // "one response then stops" become "one response then repeats itself".
        // A repeat has to end the pursuit, because the goal plainly has what it
        // came for and spending more passes cannot improve on it.
        let mut goal = Goal::new("what is that page for").unwrap();
        let status = goal.pursue(
            8,
            |_| Some(GoalStep::new("gathering", "next step").unwrap()),
            |_| works("The page is an example domain for use in documentation."),
            |_, _| GoalVerdict::Continue,
        );
        assert_eq!(
            goal.evidence().len(),
            1,
            "the same finding must be stored once: {:?}",
            goal.evidence()
        );
        assert_eq!(status, GoalStatus::Partial, "and the pursuit stops there");
        assert!(
            goal.reason().contains("stopped learning"),
            "saying why, plainly: {}",
            goal.reason()
        );
    }

    #[test]
    fn a_rewording_is_kept_because_discarding_a_real_finding_is_worse() {
        // The deliberate limit of the rule. The belief-sameness helper was tried
        // here and merged "evidence 1" with "evidence 2", which is the shape of
        // "the 41 bus arrives at 14:05" against "...at 15:10" — two findings a
        // looser rule would collapse into one, losing the second and stopping the
        // objective. So a rewording is kept, and the loop relies on the verbatim
        // repeat that actually follows it.
        let mut goal = Goal::new("what is that page for").unwrap();
        let mut pass = 0;
        let status = goal.pursue(
            8,
            |_| Some(GoalStep::new("gathering", "next step").unwrap()),
            |_| {
                pass += 1;
                if pass == 1 {
                    return works("The page is titled Example Domain.");
                }
                works("The page serves as an example domain for documentation.")
            },
            |_, _| GoalVerdict::Continue,
        );
        assert_eq!(
            goal.evidence().len(),
            2,
            "the rewording is kept, then the verbatim repeat stops it: {:?}",
            goal.evidence()
        );
        assert_eq!(status, GoalStatus::Partial);
        assert!(
            goal.reason().contains("stopped learning"),
            "{}",
            goal.reason()
        );
    }

    #[test]
    fn the_repeat_measured_live_is_caught_and_a_new_particular_is_not() {
        // Verbatim from the live run on qwen2.5:14b. The second pass dropped a
        // clause and said the same thing, which word-for-word equality missed.
        const FIRST: &str = "The page at https://example.com/ is titled \"Example Domain\" \
             and serves as an example domain for use in documentation without requiring \
             permission, though it's noted that this should not be relied upon for \
             testing or monitoring purposes.";
        const REWORDED: &str = "The page at https://example.com/ serves as an example \
             domain for use in documentation without requiring permission, though it's \
             noted that this should not be relied upon for testing or monitoring purposes.";
        assert!(
            super::says_nothing_new(FIRST, REWORDED),
            "the reworded repeat measured live must be caught"
        );

        // And the error that must never happen: two readings that differ in a
        // material particular are two findings, not one.
        assert!(!super::says_nothing_new(
            "The 41 bus arrives at 14:05.",
            "The 41 bus arrives at 15:10."
        ));
        // Including the shape that disqualified the belief rule.
        assert!(!super::says_nothing_new("evidence 1", "evidence 2"));
    }

    #[test]
    fn genuinely_new_findings_still_accumulate() {
        // The guard must not stop a goal that is actually learning things.
        let mut goal = Goal::new("plan an evening").unwrap();
        let mut pass = 0;
        let status = goal.pursue(
            8,
            |_| Some(GoalStep::new("gathering", "next step").unwrap()),
            |_| {
                pass += 1;
                match pass {
                    1 => works("The restaurant on Mill Lane opens at six on Sundays."),
                    2 => works("The last train home leaves at eleven twenty."),
                    _ => works("Rain is forecast from nine in the evening."),
                }
            },
            |g, _| {
                if g.evidence().len() == 3 {
                    GoalVerdict::Complete
                } else {
                    GoalVerdict::Continue
                }
            },
        );
        assert_eq!(status, GoalStatus::Complete);
        assert_eq!(goal.evidence().len(), 3);
    }

    #[test]
    fn an_executor_error_no_longer_ends_a_multi_step_plan() {
        // `run_plan`'s own defect, fixed by routing it through the same loop: a
        // failing step used to make the whole plan terminal.
        let mut goal = Goal::new("an objective").unwrap();
        let plan = vec![
            GoalStep::new("gathering", "first").unwrap(),
            GoalStep::new("gathering", "second").unwrap(),
        ];
        let status = goal.run_plan(
            &plan,
            |step| {
                if step.name == "first" {
                    return Err("the first source failed".to_owned());
                }
                Ok("evidence from the second".to_owned())
            },
            |_, _| GoalVerdict::Complete,
        );
        assert_eq!(status, GoalStatus::Complete);
        assert_eq!(goal.evidence().len(), 1);
    }

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
