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

/// What a turn on an item in a repository outside the lab is told in place of how the lab is
/// built (ADR 0115, amended 2026-11-05): which repository it is, that the lab fork's rule of
/// the owner's values does not describe it, and what holds there instead.
#[must_use]
pub fn outside_repo_said(repo: &str) -> String {
    format!(
        "Your item's repository: {repo}, a public repository outside the lab. repo_checkout \
         and repo_submit work in it, not in the lab's ansible-nas fork, and the fork's rules \
         of which paths hold the owner's values do not apply: its own files, an inventory or \
         a vault module among them, are ordinary code. Never put the owner's own values, \
         credentials or personal details in it. A branch you submit is kept on the node for \
         judging and never pushed.\n"
    )
}

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

/// A branch's status when a turn submitted it: checked, then reviewed.
pub const SUBMITTED: &str = "submitted";

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
    /// "syntax ok, ansible-lint 25 findings (develop 25), yamllint ok: no more findings than develop: met".
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

/// How a successful `repo_checkout` receipt names a checkout of the repository's default branch
/// (correction-placement review): `checked out the base at <full head>: <branch> (<n> KB)`. The
/// full head comes first, so HQ's excerpt limit never cuts it, whatever the branch is called.
pub const BASE_CHECKOUT: &str = "checked out the base at ";

/// The full revision a node receipt names as the default branch it handed over, when the
/// receipt names one: 40 hexadecimal characters right after [`BASE_CHECKOUT`]. An older,
/// abbreviated receipt names none.
#[must_use]
pub fn base_revision(excerpt: &str) -> Option<&str> {
    let rest = excerpt.strip_prefix(BASE_CHECKOUT)?;
    let head = rest.get(..40)?;
    let full = head.bytes().all(|b| b.is_ascii_hexdigit())
        && rest[40..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_hexdigit());
    full.then_some(head)
}

/// One task branch the worker read in its sandbox: its name and full head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldBranch {
    /// `hq/task-N-<slug>`.
    pub branch: String,
    /// Its full head.
    pub head: String,
}

/// What the worker said of its sandbox beside a closing call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxReport {
    /// Read: the task's branches, perhaps none.
    Read(Vec<HeldBranch>),
    /// It could not be read, or what came could not be understood: why.
    Unknown(String),
}

/// A revision the member chose to leave unsubmitted as it closes the item, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftUnsubmitted {
    /// The branch.
    pub branch: String,
    /// Its exact full head.
    pub head: String,
    /// Why, never empty.
    pub why: String,
}

/// Why an item may not be closed over what its sandbox holds, if it may not (correction-placement
/// review): each reported task-branch head must be a finished submission of this item on that
/// branch at that exact head, the default branch's revision the node handed over, or named as left
/// unsubmitted with a reason. Work kept only in progress is not finished; a kept branch's checkout
/// is no baseline.
#[must_use]
pub fn outstanding(
    report: &SandboxReport,
    finished: &[(String, String)],
    in_progress: &[(String, String)],
    bases: &[String],
    left: &[LeftUnsubmitted],
) -> Option<String> {
    let held = match report {
        SandboxReport::Unknown(why) => {
            return Some(format!(
                "your sandbox could not be read ({why}), so HQ cannot tell whether it holds work \
                 never submitted: nothing changed"
            ));
        }
        SandboxReport::Read(held) => held,
    };
    let pending: Vec<&HeldBranch> = held
        .iter()
        .filter(|h| {
            !finished
                .iter()
                .any(|(b, at)| *b == h.branch && *at == h.head)
        })
        .filter(|h| !bases.contains(&h.head))
        .filter(|h| {
            !left
                .iter()
                .any(|l| l.branch == h.branch && l.head == h.head && !l.why.trim().is_empty())
        })
        .collect();
    let first = pending.first()?;
    let kept = in_progress
        .iter()
        .any(|(b, at)| *b == first.branch && *at == first.head);
    let how = if kept {
        "kept only as work in progress, never submitted as finished work"
    } else {
        "never submitted as finished work"
    };
    Some(format!(
        "your sandbox holds {b} at {h}, {how}: nothing changed. Submit it with repo_submit, \
         then set the task again; or, if that revision is not to be delivered, set it again with \
         \"unsubmitted\": [{{\"branch\": \"{b}\", \"head\": \"{h}\", \"why\": \"<why it is \
         not delivered>\"}}]. Sandbox-only work may disappear when the item closes.",
        b = first.branch,
        h = first.head
    ))
}

#[cfg(test)]
mod outstanding_tests {
    use super::*;

    fn at(unit: &str) -> String {
        unit.repeat(5)
    }

    fn holding(branch: &str, head: &str) -> SandboxReport {
        SandboxReport::Read(vec![HeldBranch {
            branch: branch.to_owned(),
            head: head.to_owned(),
        }])
    }

    #[test]
    fn a_base_checkout_names_its_full_revision_however_long_the_branch() {
        let head = at("0123abcd");
        let long = "b".repeat(280);
        let said = format!("{BASE_CHECKOUT}{head}: {long} (4 KB)");
        let cut: String = said.chars().take(300).collect();
        assert_eq!(base_revision(&cut), Some(head.as_str()));
        assert_eq!(
            base_revision("checked out main at 0123abcd0123 (4 KB)"),
            None
        );
        assert_eq!(
            base_revision(&format!("{BASE_CHECKOUT}{}", &head[..12])),
            None
        );
        assert_eq!(base_revision(&format!("{BASE_CHECKOUT}{head}ab: x")), None);
    }

    #[test]
    fn only_a_finished_head_a_base_or_a_named_disposition_clears_a_branch() {
        let (b, new, done) = ("hq/task-1-fix", at("5faf6aed"), at("a1b2c3d4"));
        let finished = vec![(b.to_owned(), done.clone())];
        assert!(outstanding(&holding(b, &done), &finished, &[], &[], &[]).is_none());
        let said = outstanding(&holding(b, &new), &finished, &[], &[], &[]).unwrap();
        assert!(
            said.contains(&format!("{b} at {new}, never submitted")),
            "{said}"
        );
        assert!(outstanding(&holding(b, &new), &[], &[], &[new.clone()], &[]).is_none());
        let left = |head: &str, why: &str| LeftUnsubmitted {
            branch: b.to_owned(),
            head: head.to_owned(),
            why: why.to_owned(),
        };
        assert!(outstanding(&holding(b, &new), &[], &[], &[], &[left(&new, "wrong")]).is_none());
        assert!(outstanding(&holding(b, &new), &[], &[], &[], &[left(&new, " ")]).is_some());
        assert!(outstanding(&holding(b, &new), &[], &[], &[], &[left(&done, "x")]).is_some());
        let other = vec![("hq/task-1-other".to_owned(), new.clone())];
        assert!(outstanding(&holding(b, &new), &other, &[], &[], &[]).is_some());
        let wip = vec![(b.to_owned(), new.clone())];
        let said = outstanding(&holding(b, &new), &[], &wip, &[], &[]).unwrap();
        assert!(said.contains("kept only as work in progress"), "{said}");
        assert!(outstanding(&SandboxReport::Read(vec![]), &[], &[], &[], &[]).is_none());
        let said = outstanding(&SandboxReport::Unknown("no git".into()), &[], &[], &[], &[]);
        assert!(said.unwrap().contains("could not be read"));
    }
}
