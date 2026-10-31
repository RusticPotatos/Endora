//! The engineering wing (ADR 0115): the team that works on the lab's ansible-nas fork through
//! branches the owner alone pushes. The records the flow leaves on a
//! task (a document, a submitted branch, a check of it) are plain data.

use super::work::{MemberId, Millis, RunId, TaskId};

/// The owner's goal the wing works towards.
pub const WING_GOAL: &str = "Improve the lab";
/// The wing's first project, under that goal.
pub const WING_PROJECT: &str = "ansible-nas clean-up";
/// Where the team's branches live: under `hq/`, and nowhere else (the node enforces it).
pub const BRANCH_PREFIX: &str = "hq/";

/// What every member of the wing is told about the fork's own rules (ADR 0116).
pub const FORK_RULES: &str = "The fork's own AGENTS.md, when it exists, is the repo's rules; \
    propose one if missing.\n";

/// What the wing's lead is told: the same, and that the draft is the first branch (ADR 0116).
pub const FORK_RULES_LEAD: &str = "The fork's own AGENTS.md, when it exists, is the repo's \
    rules; propose one if missing. Until it exists it is the first branch to propose: the draft \
    is docs/drafts/ansible-nas-AGENTS.md in Endora's library (read_doc).\n";

/// A document on a task (ADR 0115): a branch's diff, a check's report. Read by the tester,
/// the lead and the owner; never changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Its id: `d<id>` as members name it.
    pub id: i64,
    /// The task it is on.
    pub task: TaskId,
    /// The turn it came from, if one.
    pub run: Option<RunId>,
    /// Its title, one line.
    pub title: String,
    /// Its text.
    pub text: String,
    /// When.
    pub at: Millis,
}

/// A branch the wing submitted (ADR 0115), kept on the node as a bundle and here as its record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Its id.
    pub id: i64,
    /// The task it is for.
    pub task: TaskId,
    /// The turn that submitted it.
    pub run: RunId,
    /// The member who submitted it.
    pub member: MemberId,
    /// Its name, under `hq/`.
    pub name: String,
    /// Its head.
    pub head: String,
    /// The default branch's head it sits on.
    pub base: String,
    /// How many files it changes.
    pub files: usize,
    /// Lines added.
    pub plus: i64,
    /// Lines removed.
    pub minus: i64,
    /// Whether the node can push it (a deploy key is set up).
    pub can_push: bool,
    /// `submitted`, `offered` (on the owner's desk), `approved`, `pushed`, `failed` or
    /// `declined`; [`IN_PROGRESS`] for work a turn kept when it ran out of room.
    pub status: String,
    /// When it was submitted.
    pub at: Millis,
}

/// A branch's status when it is work in progress, kept by the worker as a turn ran out of room
/// (ADR 0142, amended 2026-10-31): kept on the node for the next turn, never checked, reviewed or
/// offered for a push.
pub const IN_PROGRESS: &str = "in_progress";

/// A check of a submitted branch (ADR 0115): the first piece of the lab, run model-free on the
/// dev node's runner, reported back with its own token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Its id: `c<id>` as members name it.
    pub id: i64,
    /// The task.
    pub task: TaskId,
    /// The branch it checks.
    pub branch: i64,
    /// The turn that submitted the branch.
    pub run: RunId,
    /// The job's own token, which opens the bundle on the node and reports the result.
    pub token: String,
    /// `waiting`, `running` or `done`.
    pub status: String,
    /// Whether every check passed, once done.
    pub ok: Option<bool>,
    /// The result in a line, once done: "syntax ok, ansible-lint 2 findings, yamllint ok", or,
    /// when the branch failed something its base failed too, both counts and the verdict:
    /// "syntax ok, ansible-lint 25 findings (develop 25), yamllint ok: no worse than develop".
    pub summary: String,
    /// When it was queued.
    pub queued_at: Millis,
    /// When the runner took it.
    pub started_at: Option<Millis>,
    /// When it reported.
    pub done_at: Option<Millis>,
}

/// How long a check may run before it is given up as lost.
pub const CHECK_LOST_AFTER_MS: Millis = 30 * 60 * 1000;
