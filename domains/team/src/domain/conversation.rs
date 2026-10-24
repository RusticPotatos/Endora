//! Bounded information records, separate from task ownership and execution (ADR 0137).

use super::{MemberId, Millis, ProjectId, Ref, RunId};

/// Maximum characters of one information message.
pub const MESSAGE_MOST: usize = 1_200;
/// Maximum original evidence locators on a message.
pub const REFERENCES_MOST: usize = 5;
/// Maximum accepted information sends in a turn.
pub const PER_TURN: usize = 12;
/// Maximum accepted information sends per member's calendar day.
pub const PER_DAY: usize = 60;
/// A read page; one additional row establishes whether more exists.
pub const PAGE: usize = 50;
/// Maximum rows removed by one deletion pass.
pub const PURGE_BATCH: usize = 1_000;
/// Maximum serialized arguments to a new worker messaging tool.
pub const ARGUMENT_BYTES: usize = 8 * 1_024;

/// Owner-created scope; names and task ownership are not communication authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    /// Stable, never-reused identity.
    pub id: i64,
    /// Its fixed project.
    pub project: ProjectId,
    /// Its two fixed participants.
    pub participants: [MemberId; 2],
    /// Creation time.
    pub at: Millis,
    /// Newest accepted message, if any.
    pub latest: Option<i64>,
    /// Number of logical messages, not HTTP attempts.
    pub messages: i64,
}

/// A persistent message is a claim, never a successful tool-execution receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Stable identity.
    pub id: i64,
    /// Its scope.
    pub conversation: i64,
    /// Authenticated author.
    pub member: MemberId,
    /// Author's authenticated run.
    pub run: RunId,
    /// Canonical text, with peer-input controls defused.
    pub text: String,
    /// Original record locators; these do not grant access.
    pub evidence: Vec<Ref>,
    /// Acceptance time.
    pub at: Millis,
}

/// Content-free outcome of one messaging operation, for owner inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// Stable paging identity.
    pub id: i64,
    /// Authorized scope, absent for protocol refusals and directory reads.
    pub conversation: Option<i64>,
    /// Authenticated member.
    pub member: MemberId,
    /// Authenticated turn.
    pub run: RunId,
    /// Operation performed or refused.
    pub operation: String,
    /// Static outcome or reason, never peer text.
    pub reason: String,
    /// Operation time.
    pub at: Millis,
}

/// Validated intent for atomic acceptance and deduplication.
pub struct Information {
    /// Conversation identity.
    pub conversation: i64,
    /// Authenticated sender.
    pub member: MemberId,
    /// Authenticated run.
    pub run: RunId,
    /// Client retry identity.
    pub key: String,
    /// Canonical message.
    pub text: String,
    /// Checked original locators.
    pub evidence: Vec<Ref>,
    /// Owner-calendar day for the sender's rate limit.
    pub day: i64,
    /// Acceptance time.
    pub at: Millis,
}

/// Validate bounds before canonicalizing; reject rather than truncate input.
///
/// # Errors
/// A stable refusal for an invalid key, message or evidence bound.
pub fn words(key: &str, text: &str, evidence: &[Ref]) -> Result<String, &'static str> {
    if !(8..=64).contains(&key.len())
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("a message key needs 8 to 64 ASCII letters, digits, underscores or hyphens");
    }
    if text.chars().count() > MESSAGE_MOST || evidence.len() > REFERENCES_MOST {
        return Err("a message permits 1200 characters and five original references");
    }
    let clean = super::record::scrub(text).trim().to_owned();
    if clean.is_empty() {
        return Err("a message needs text");
    }
    Ok(clean)
}
