//! Durable research results owned by a commissioned intention (ADR 0077).

use endora_kernel::error::{DomainError, require_non_empty};
use endora_kernel::ids::{IntentionId, Timestamp};

/// Maximum model invocations reserved by one assignment.
pub const RESEARCH_MODEL_LIMIT: u32 = 16;
/// Maximum external read attempts reserved by one assignment.
pub const RESEARCH_READ_LIMIT: u32 = 16;
/// Maximum lifetime of an assignment, including time while the node is stopped.
pub const RESEARCH_LIFETIME_MS: i64 = 24 * 60 * 60 * 1_000;

/// Deterministic lifecycle of a research assignment. A model never chooses it.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchStatus {
    Queued,
    Running,
    RetryWait,
    Delivered,
    Blocked,
    BudgetExhausted,
    Cancelled,
    Failed,
}

#[allow(missing_docs)]
impl ResearchStatus {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Delivered => "delivered",
            Self::Blocked => "blocked",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn from_name(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "retry_wait" => Self::RetryWait,
            "delivered" => Self::Delivered,
            "blocked" => Self::Blocked,
            "budget_exhausted" => Self::BudgetExhausted,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Delivered
                | Self::Blocked
                | Self::BudgetExhausted
                | Self::Cancelled
                | Self::Failed
        )
    }
}

/// Persistent, bounded work attached one-to-one to a user-requested intention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchAssignment {
    intention_id: IntentionId,
    assignment: String,
    request_key: String,
    source_urls_json: String,
    search_capability_id: Option<String>,
    status: ResearchStatus,
    phase: String,
    reason_code: String,
    reason: String,
    checkpoint: String,
    report: Option<ResearchReport>,
    model_calls_used: u32,
    read_attempts_used: u32,
    created_at: Timestamp,
    updated_at: Timestamp,
    expires_at: Timestamp,
}

#[allow(missing_docs)]
impl ResearchAssignment {
    pub fn new(
        intention_id: IntentionId,
        assignment: &str,
        request_key: &str,
        source_urls_json: &str,
        search_capability_id: Option<String>,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        let assignment = require_non_empty("research.assignment", assignment)?;
        let request_key = require_non_empty("research.request_key", request_key)?;
        Ok(Self {
            intention_id,
            assignment,
            request_key,
            source_urls_json: source_urls_json.to_owned(),
            search_capability_id,
            status: ResearchStatus::Queued,
            phase: "gathering".to_owned(),
            reason_code: String::new(),
            reason: String::new(),
            checkpoint: r#"{"version":1,"resume":"restart_read_only_turn"}"#.to_owned(),
            report: None,
            model_calls_used: 0,
            read_attempts_used: 0,
            created_at: at,
            updated_at: at,
            expires_at: Timestamp::from_unix_millis(at.unix_millis() + RESEARCH_LIFETIME_MS),
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_parts(
        intention_id: IntentionId,
        assignment: String,
        request_key: String,
        source_urls_json: String,
        search_capability_id: Option<String>,
        status: ResearchStatus,
        phase: String,
        reason_code: String,
        reason: String,
        checkpoint: String,
        report: Option<ResearchReport>,
        model_calls_used: u32,
        read_attempts_used: u32,
        created_at: Timestamp,
        updated_at: Timestamp,
        expires_at: Timestamp,
    ) -> Self {
        Self {
            intention_id,
            assignment,
            request_key,
            source_urls_json,
            search_capability_id,
            status,
            phase,
            reason_code,
            reason,
            checkpoint,
            report,
            model_calls_used,
            read_attempts_used,
            created_at,
            updated_at,
            expires_at,
        }
    }

    pub fn reserve_model_call(&mut self, at: Timestamp) -> bool {
        if self.status.terminal()
            || self.model_calls_used >= RESEARCH_MODEL_LIMIT
            || at >= self.expires_at
        {
            if !self.status.terminal() {
                self.exhaust(at);
            }
            return false;
        }
        self.model_calls_used += 1;
        self.status = ResearchStatus::Running;
        self.reason_code.clear();
        self.reason.clear();
        self.updated_at = at;
        true
    }

    pub fn reserve_read(&mut self, at: Timestamp) -> bool {
        if self.status.terminal()
            || self.read_attempts_used >= RESEARCH_READ_LIMIT
            || at >= self.expires_at
        {
            if !self.status.terminal() {
                self.exhaust(at);
            }
            return false;
        }
        self.read_attempts_used += 1;
        self.status = ResearchStatus::Running;
        self.reason_code.clear();
        self.reason.clear();
        self.updated_at = at;
        true
    }

    pub fn queue(&mut self, checkpoint: &str, phase: &str, at: Timestamp) {
        if self.status.terminal() {
            return;
        }
        self.checkpoint = checkpoint.to_owned();
        self.phase = phase.to_owned();
        self.status = ResearchStatus::Queued;
        self.updated_at = at;
    }

    pub fn deliver(&mut self, report: ResearchReport, at: Timestamp) {
        if self.status.terminal() {
            return;
        }
        self.report = Some(report);
        self.status = ResearchStatus::Delivered;
        self.updated_at = at;
    }

    pub fn block(&mut self, code: &str, reason: &str, at: Timestamp) {
        if self.status.terminal() {
            return;
        }
        self.status = ResearchStatus::Blocked;
        self.reason_code = code.to_owned();
        self.reason = reason.to_owned();
        self.updated_at = at;
    }

    pub fn fail(&mut self, code: &str, reason: &str, at: Timestamp) {
        if self.status.terminal() {
            return;
        }
        self.status = ResearchStatus::Failed;
        self.reason_code = code.to_owned();
        self.reason = reason.to_owned();
        self.updated_at = at;
    }

    pub fn retry(&mut self, code: &str, reason: &str, at: Timestamp) {
        if self.status.terminal() {
            return;
        }
        self.status = ResearchStatus::RetryWait;
        self.reason_code = code.to_owned();
        self.reason = reason.to_owned();
        self.updated_at = at;
    }

    pub fn cancel(&mut self, at: Timestamp) {
        if self.status.terminal() {
            return;
        }
        self.status = ResearchStatus::Cancelled;
        self.reason_code = "cancelled".to_owned();
        self.reason = "You cancelled this research assignment.".to_owned();
        self.updated_at = at;
    }

    fn exhaust(&mut self, at: Timestamp) {
        self.status = ResearchStatus::BudgetExhausted;
        self.reason_code = "budget_exhausted".to_owned();
        self.reason = "The research budget ended before a complete report was ready.".to_owned();
        self.updated_at = at;
    }

    #[must_use]
    pub const fn intention_id(&self) -> IntentionId {
        self.intention_id
    }
    #[must_use]
    pub fn assignment(&self) -> &str {
        &self.assignment
    }
    #[must_use]
    pub fn request_key(&self) -> &str {
        &self.request_key
    }
    #[must_use]
    pub fn source_urls_json(&self) -> &str {
        &self.source_urls_json
    }
    #[must_use]
    pub fn search_capability_id(&self) -> Option<&str> {
        self.search_capability_id.as_deref()
    }
    #[must_use]
    pub const fn status(&self) -> ResearchStatus {
        self.status
    }
    #[must_use]
    pub fn phase(&self) -> &str {
        &self.phase
    }
    #[must_use]
    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub fn checkpoint(&self) -> &str {
        &self.checkpoint
    }

    /// Replaces the versioned recovery checkpoint after the application has validated
    /// its contents. A terminal assignment cannot be moved back into work.
    pub fn set_checkpoint(&mut self, checkpoint: &str, at: Timestamp) {
        if self.status.terminal() {
            return;
        }
        self.checkpoint = checkpoint.to_owned();
        self.updated_at = at;
    }
    #[must_use]
    pub fn report(&self) -> Option<&ResearchReport> {
        self.report.as_ref()
    }
    #[must_use]
    pub const fn model_calls_used(&self) -> u32 {
        self.model_calls_used
    }
    #[must_use]
    pub const fn read_attempts_used(&self) -> u32 {
        self.read_attempts_used
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
    #[must_use]
    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}

/// A validated local report. Delivery does not assert semantic correctness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchReport {
    intention_id: IntentionId,
    markdown: String,
    partial: bool,
    created_at: Timestamp,
}

#[allow(missing_docs)]
impl ResearchReport {
    pub fn new(
        intention_id: IntentionId,
        markdown: &str,
        partial: bool,
        created_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let markdown = require_non_empty("research_report.markdown", markdown)?;
        if !markdown.lines().any(|line| line.starts_with("# ")) {
            return Err(DomainError::Malformed {
                field: "research_report.markdown",
                reason: "must have a title",
            });
        }
        if !markdown.contains("\n## Sources\n") || !markdown.contains("[S1]") {
            return Err(DomainError::Malformed {
                field: "research_report.markdown",
                reason: "must cite at least one retrieved source",
            });
        }
        Ok(Self {
            intention_id,
            markdown,
            partial,
            created_at,
        })
    }

    #[must_use]
    pub const fn intention_id(&self) -> IntentionId {
        self.intention_id
    }
    #[must_use]
    pub fn markdown(&self) -> &str {
        &self.markdown
    }
    #[must_use]
    pub const fn partial(&self) -> bool {
        self.partial
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::{RESEARCH_MODEL_LIMIT, ResearchAssignment, ResearchReport, ResearchStatus};
    use endora_kernel::ids::{IntentionId, Timestamp};

    #[test]
    fn only_terminal_states_are_terminal() {
        assert!(!ResearchStatus::Queued.terminal());
        assert!(!ResearchStatus::Running.terminal());
        assert!(ResearchStatus::Delivered.terminal());
        assert!(ResearchStatus::Cancelled.terminal());
    }

    #[test]
    fn an_empty_report_cannot_be_delivered() {
        assert!(
            ResearchReport::new(
                IntentionId::new(1),
                " ",
                false,
                Timestamp::from_unix_millis(0)
            )
            .is_err()
        );
    }

    #[test]
    fn a_report_without_a_source_cannot_be_delivered() {
        assert!(
            ResearchReport::new(
                IntentionId::new(1),
                "# Answer\n\nNo references.",
                false,
                Timestamp::from_unix_millis(0),
            )
            .is_err()
        );
    }

    #[test]
    fn model_budget_is_reserved_before_work_and_exhausts() {
        let at = Timestamp::from_unix_millis(0);
        let mut work =
            ResearchAssignment::new(IntentionId::new(1), "research it", "key", "[]", None, at)
                .unwrap();
        for _ in 0..RESEARCH_MODEL_LIMIT {
            assert!(work.reserve_model_call(at));
            work.queue("{}", "gathering", at);
        }
        assert!(!work.reserve_model_call(at));
        assert_eq!(work.status(), ResearchStatus::BudgetExhausted);
    }

    #[test]
    fn a_checkpoint_survives_a_yield_without_resurrecting_terminal_work() {
        let at = Timestamp::from_unix_millis(0);
        let mut work =
            ResearchAssignment::new(IntentionId::new(1), "research it", "key", "[]", None, at)
                .unwrap();
        work.set_checkpoint(
            r#"{"version":1,"evidence":[["https://example.com","page"]]}"#,
            at,
        );
        assert!(work.checkpoint().contains("example.com"));
        work.cancel(at);
        work.set_checkpoint("{}", Timestamp::from_unix_millis(1));
        assert!(work.checkpoint().contains("example.com"));
    }
}
