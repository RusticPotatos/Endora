//! The engineering wing (ADR 0115): the team that works on the lab's ansible-nas fork through
//! branches the owner alone pushes. Its seats are a rule in code (one lead on Sonnet, at most
//! [`MOST_DEVELOPERS`] developers on Haiku or the house's model); a hire a lead asks for is a
//! request on the owner's desk, written and read here; and the records the flow leaves on a
//! task (a document, a submitted branch, a check of it) are plain data.

use super::brain::Brain;
use super::work::{MemberId, Millis, Role, RunId, TaskId};

/// The owner's goal the wing works towards.
pub const WING_GOAL: &str = "Improve the lab";
/// The wing's first project, under that goal.
pub const WING_PROJECT: &str = "ansible-nas clean-up";
/// The most developers the wing seats.
pub const MOST_DEVELOPERS: usize = 5;
/// The most leads the wing seats.
pub const MOST_LEADS: usize = 1;
/// Where the team's branches live: under `hq/`, and nowhere else (the node enforces it).
pub const BRANCH_PREFIX: &str = "hq/";

/// What every member working towards the wing's goal is told: the goal's notes from the owner,
/// seeded once (the owner may edit them on the HQ screen).
pub const WING_NOTE: &str = "The wing's job is the owner's public ansible-nas fork, which \
    builds the home lab. Your base is the fork's develop branch (main only mirrors upstream): \
    repo_checkout brings it into /work/repo in your sandbox and says so (git is set up there: \
    commits are authored Endora HQ and carry the trailer). Never edit, cite or copy from \
    group_vars/*.yml, host_vars or any inventory: they hold the owner's own values, and a \
    branch touching them is refused. Work on a branch named hq/task-<N>-<slug> cut from the \
    base, commit in small steps, then repo_submit sends the branch to Endora, which keeps it, \
    writes its diff as a \
    document on the task and runs the checks (ansible syntax, ansible-lint, yamllint) on the \
    dev node; the result comes back as a receipt. The fork's develop already has findings, so \
    the check judges your branch against develop: it passes when no check is worse than it \
    is on develop, and the receipt says both. Then submit the task for review (set_status \
    in_review, citing the receipts): Dee, the tester, checks the branch out and reads the diff \
    and the check; the lead \
    accepts. The owner alone pushes an accepted branch to the fork and merges it there: HQ \
    never pushes on its own, never opens a pull request, never merges. The lead plans the \
    clean-up, hands developers one branch each, and reviews; a developer reads the fork's docs \
    (ansible_nas_doc) before changing a role.";

/// What every member of the wing is told about the fork's own rules (ADR 0116).
pub const FORK_RULES: &str = "The fork's own AGENTS.md, when it exists, is the repo's rules; \
    propose one if missing.\n";

/// What the wing's lead is told: the same, and that the draft is the first branch (ADR 0116).
pub const FORK_RULES_LEAD: &str = "The fork's own AGENTS.md, when it exists, is the repo's \
    rules; propose one if missing. Until it exists it is the first branch to propose: the draft \
    is docs/drafts/ansible-nas-AGENTS.md in Endora's library (read_doc).\n";

/// A hire a lead asks the owner for, as it is written on the desk and read back from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HireRequest {
    /// The new member's name.
    pub name: String,
    /// Its role.
    pub role: Role,
    /// What it thinks with.
    pub brain: Brain,
    /// Whom it reports to: the lead who asked.
    pub reports_to: MemberId,
    /// Why, in the lead's words.
    pub why: String,
}

impl HireRequest {
    /// The first line of the desk item, which [`Self::read`] reads back; the rest is the
    /// lead's why and evidence, for the owner.
    #[must_use]
    pub fn write(&self, asker: &str, evidence: &str) -> String {
        format!(
            "Hire {} as {} on {}, reporting to member {}.\n{asker} asks: {}\nEvidence: {evidence}",
            self.name,
            self.role.name(),
            self.brain.name(),
            self.reports_to.0,
            self.why.trim()
        )
    }

    /// The request from a desk item's text, if it is one.
    #[must_use]
    pub fn read(text: &str) -> Option<Self> {
        let first = text.lines().next()?.strip_prefix("Hire ")?;
        let (name, rest) = first.split_once(" as ")?;
        let (role, rest) = rest.split_once(" on ")?;
        let (brain, rest) = rest.split_once(", reporting to member ")?;
        let to = rest.strip_suffix('.')?.parse::<i64>().ok()?;
        let why = text
            .lines()
            .nth(1)
            .and_then(|l| l.split_once(" asks: "))
            .map_or("", |(_, why)| why);
        Some(Self {
            name: name.to_owned(),
            role: Role::from_name(role)?,
            brain: Brain::from_name(brain)?,
            reports_to: MemberId(to),
            why: why.to_owned(),
        })
    }
}

/// The names a hire is given when its request names none, the first not taken.
pub const SPARE_NAMES: [&str; 6] = ["Sam", "Lee", "Max", "Ash", "Rio", "Val"];

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
    /// `declined`.
    pub status: String,
    /// When it was submitted.
    pub at: Millis,
}

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
