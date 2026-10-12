//! Ownership checks at the dispatch boundary for durable goal work.
use crate::{
    CapabilityRead, CapabilityRunner, CapabilitySpec, Clock, GoalLease, GoalObservation,
    GoalRepository,
};

/// Preserves the existing policy runner and rejects dispatch after ownership loss.
pub struct OwnedGoalRunner<'a> {
    runner: &'a dyn CapabilityRunner,
    goals: &'a dyn GoalRepository,
    lease: &'a GoalLease,
    clock: &'a dyn Clock,
}

impl<'a> OwnedGoalRunner<'a> {
    /// Wraps the production policy runner for one current claim.
    pub fn new(
        runner: &'a dyn CapabilityRunner,
        goals: &'a dyn GoalRepository,
        lease: &'a GoalLease,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            runner,
            goals,
            lease,
            clock,
        }
    }

    fn owned(&self) -> bool {
        self.goals
            .goal_owned(self.lease, self.clock.now())
            .unwrap_or(false)
    }
}

impl CapabilityRunner for OwnedGoalRunner<'_> {
    fn available(&self) -> Vec<CapabilitySpec> {
        self.runner.available()
    }
    fn run(&self, id: &str, input: &str) -> Result<String, String> {
        self.run_observed(id, input).map(|read| read.text)
    }
    fn run_observed(&self, id: &str, input: &str) -> Result<CapabilityRead, String> {
        if !self.owned() {
            return Err("goal ownership expired, was replaced, or was cancelled".to_owned());
        }
        self.runner.run_observed(id, input)
    }
    fn about_the_person(&self) -> Vec<String> {
        if !self.owned() {
            return Vec::new();
        }
        self.runner.about_the_person()
    }
    fn current_states(&self) -> Vec<(String, String)> {
        if !self.owned() {
            return Vec::new();
        }
        self.runner.current_states()
    }
    fn verifier(&self, id: &str) -> Option<String> {
        self.runner.verifier(id)
    }
    fn read_back_input(&self, id: &str, input: &str) -> String {
        self.runner.read_back_input(id, input)
    }
    fn decision(&self, id: &str) -> Option<endora_kernel::Decision> {
        self.runner.decision(id)
    }
}

/// Records actual calls while preserving the complete underlying policy surface.
pub struct ObservingGoalRunner<'a> {
    runner: &'a dyn CapabilityRunner,
    clock: &'a dyn Clock,
    receipts: &'a std::cell::RefCell<Vec<GoalObservation>>,
}

impl<'a> ObservingGoalRunner<'a> {
    /// Captures observations from this execution only; it never asks the model for them.
    pub fn new(
        runner: &'a dyn CapabilityRunner,
        clock: &'a dyn Clock,
        receipts: &'a std::cell::RefCell<Vec<GoalObservation>>,
    ) -> Self {
        Self {
            runner,
            clock,
            receipts,
        }
    }
}

impl CapabilityRunner for ObservingGoalRunner<'_> {
    fn available(&self) -> Vec<CapabilitySpec> {
        self.runner.available()
    }
    fn run(&self, id: &str, input: &str) -> Result<String, String> {
        let result = self.runner.run_observed(id, input);
        let (effective, output, data, error) = match &result {
            Ok(read) => (
                read.input.clone(),
                read.text.clone(),
                read.data.clone(),
                None,
            ),
            Err(error) => (input.to_owned(), String::new(), None, Some(error.clone())),
        };
        self.receipts.borrow_mut().push(GoalObservation {
            tool: id.to_owned(),
            input: effective,
            output,
            data,
            error,
            at: self.clock.now(),
        });
        result.map(|read| read.text)
    }
    fn about_the_person(&self) -> Vec<String> {
        self.runner.about_the_person()
    }
    fn current_states(&self) -> Vec<(String, String)> {
        self.runner.current_states()
    }
    fn verifier(&self, id: &str) -> Option<String> {
        self.runner.verifier(id)
    }
    fn read_back_input(&self, id: &str, input: &str) -> String {
        self.runner.read_back_input(id, input)
    }
    fn decision(&self, id: &str) -> Option<endora_kernel::Decision> {
        self.runner.decision(id)
    }
}
