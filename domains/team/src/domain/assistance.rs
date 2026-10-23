//! Bounded, read-only requests within an owner-authorized conversation (ADR 0137).

use super::{MemberId, Millis, ProjectId, RunId};

/// A named immutable source in HQ's record. It is a locator, never a permission grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRef {
    /// An immutable wing/check document.
    Document(i64),
    /// A node-witnessed tool receipt.
    Receipt(i64),
}

impl SourceRef {
    /// Stable form shown to a member.
    #[must_use]
    pub fn said(self) -> String {
        match self {
            Self::Document(id) => format!("d{id}"),
            Self::Receipt(id) => format!("r{id}"),
        }
    }

    /// Parse an exact positive typed ID; extra prose never creates authority.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (kind, number) = match text.strip_prefix('d') {
            Some(number) => ('d', number),
            None => ('r', text.strip_prefix('r')?),
        };
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let id = number.parse::<i64>().ok().filter(|id| *id > 0)?;
        match kind {
            'd' => Some(Self::Document(id)),
            _ => Some(Self::Receipt(id)),
        }
    }

    /// Canonical storage form for at most five IDs.
    #[must_use]
    pub fn kept(all: &[Self]) -> String {
        all.iter()
            .map(|item| item.said())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Decode stored IDs. Unknown legacy text grants no access.
    #[must_use]
    pub fn read(text: &str) -> Vec<Self> {
        text.split(',').filter_map(Self::parse).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::SourceRef;

    #[test]
    fn source_locators_are_exact_typed_ids() {
        assert_eq!(SourceRef::parse("d12"), Some(SourceRef::Document(12)));
        assert_eq!(SourceRef::parse("r9"), Some(SourceRef::Receipt(9)));
        for invalid in ["d0", "d-1", "d+1", "d12 extra", "r1x", "task 1", "d", ""] {
            assert_eq!(SourceRef::parse(invalid), None, "{invalid}");
        }
    }
}

/// A request expires fifteen minutes after durable acceptance.
pub const DEADLINE_MS: Millis = 15 * 60 * 1_000;
/// An owner's source grant lasts one day and can be revoked sooner.
pub const GRANT_MS: Millis = 24 * 60 * 60 * 1_000;
/// At most five immutable original sources per conversation or request.
pub const SOURCES_MOST: usize = 5;
/// Maximum objective length before canonicalization.
pub const OBJECTIVE_MOST: usize = 400;
/// Maximum new peer-help requests admitted in one turn.
pub const PER_TURN: usize = 2;
/// Maximum new peer-help requests per requester and owner-calendar day.
pub const PER_DAY: usize = 6;
/// Maximum read rounds before an expert answers or a verifier finishes.
pub const READ_ROUNDS: usize = 6;

/// The validated intent presented to the persistence boundary.
pub struct NewRequest {
    /// Owner-authorized conversation.
    pub conversation: i64,
    /// Its trusted fixed charging project.
    pub project: ProjectId,
    /// Authenticated requester.
    pub requester: MemberId,
    /// Authenticated requester turn.
    pub requester_run: RunId,
    /// The other fixed participant.
    pub expert: MemberId,
    /// Stable retry key scoped to requester and conversation.
    pub key: String,
    /// Canonical bounded question.
    pub objective: String,
    /// Original evidence locators; these are not access grants.
    pub evidence: Vec<SourceRef>,
    /// Requester's owner-calendar day.
    pub day: i64,
    /// UTC start of that owner-calendar day, supplied by the authenticated application.
    pub day_start: Millis,
    /// Acceptance time.
    pub at: Millis,
}

/// One retained request with an authoritative run association.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Stable request identity.
    pub id: i64,
    /// Owner-authorized conversation.
    pub conversation: i64,
    /// Trusted charging project.
    pub project: ProjectId,
    /// Authenticated requester.
    pub requester: MemberId,
    /// Authenticated request turn.
    pub requester_run: RunId,
    /// Fixed recipient.
    pub expert: MemberId,
    /// Canonical objective.
    pub objective: String,
    /// Original locators, without access rights.
    pub evidence: Vec<SourceRef>,
    /// Current durable state.
    pub status: String,
    /// Absolute deadline.
    pub deadline_at: Millis,
    /// Expert turn, if one was queued.
    pub expert_run: Option<RunId>,
    /// Accepted answer, if any.
    pub answer: Option<String>,
    /// Original locators cited in the accepted answer.
    pub answer_evidence: Vec<SourceRef>,
    /// Whether the result wake has been delivered.
    pub notified: bool,
    /// Turn queued to deliver the accepted result, if any.
    pub notice_run: Option<RunId>,
    /// Owner stopped result delivery; the accepted answer remains retained.
    pub notice_cancelled: bool,
}
