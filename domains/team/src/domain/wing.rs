//! The engineering wing (ADR 0115): the team that works on the lab's ansible-nas fork through
//! branches the owner alone pushes. Its seats are a rule in code (one lead on Sonnet, at most
//! [`MOST_DEVELOPERS`] developers on Haiku or the house's model); a hire a lead asks for is a
//! request on the owner's desk, written and read here; and the records the flow leaves on a
//! task (a document, a submitted branch, a check of it) are plain data.

use super::brain::Brain;
use super::work::{Member, MemberId, Millis, Refusal, Role, RunId, TaskId};

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
    dev node; the result comes back as a receipt. Then submit the task for review (set_status \
    in_review, citing the receipts): Dee, the tester, reads the diff and the check; the lead \
    accepts. The owner alone pushes an accepted branch to the fork and merges it there: HQ \
    never pushes on its own, never opens a pull request, never merges. The lead plans the \
    clean-up, hands developers one branch each, and reviews; a developer reads the fork's docs \
    (ansible_nas_doc) before changing a role.";

/// Whether a member is in the wing: it leads one of the wing's projects (`leads`), or reports,
/// however far up, to one who does. The CEO is never in the wing.
#[must_use]
pub fn in_wing(member: &Member, leads: &[MemberId], members: &[Member]) -> bool {
    if member.role == Role::Ceo {
        return false;
    }
    let mut seen = vec![member.id];
    let mut at = member;
    loop {
        if leads.contains(&at.id) {
            return true;
        }
        let Some(up) = super::chain::manager(at, members) else {
            return false;
        };
        if seen.contains(&up) {
            return false;
        }
        seen.push(up);
        let Some(found) = members.iter().find(|m| m.id == up) else {
            return false;
        };
        at = found;
    }
}

/// Whether the wing has a seat for one more member in `role` thinking with `brain`, given who
/// sits in it now: one lead (on Sonnet or the house's model), at most [`MOST_DEVELOPERS`]
/// developers (on Haiku or the house's model), and no other role.
///
/// # Errors
/// [`Refusal::NoSeat`] with the count, or [`Refusal::WingBrain`].
pub fn may_seat(role: Role, brain: Brain, wing: &[&Member]) -> Result<(), Refusal> {
    let count = wing.iter().filter(|m| m.role == role).count();
    let (most, allowed, said) = match role {
        Role::Lead => (
            MOST_LEADS,
            matches!(brain, Brain::Sonnet | Brain::Local),
            "Claude Sonnet or the house's model",
        ),
        Role::Developer => (
            MOST_DEVELOPERS,
            matches!(brain, Brain::Haiku | Brain::Local),
            "Claude Haiku or the house's model",
        ),
        _ => (0, true, ""),
    };
    if count >= most {
        return Err(Refusal::NoSeat { role, count, most });
    }
    if !allowed {
        return Err(Refusal::WingBrain {
            role,
            allowed: said,
        });
    }
    Ok(())
}

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

/// A name for a new member when the request gave none: the first spare not taken, else
/// `Member <n>`.
#[must_use]
pub fn spare_name(members: &[Member]) -> String {
    SPARE_NAMES
        .iter()
        .find(|n| !members.iter().any(|m| m.name.eq_ignore_ascii_case(n)))
        .map_or_else(
            || format!("Member {}", members.len() + 1),
            |n| (*n).to_owned(),
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
    /// The result in a line, once done: "syntax ok, ansible-lint 2 findings, yamllint ok".
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

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: i64, name: &str, role: Role, brain: Brain, to: Option<i64>) -> Member {
        Member {
            id: MemberId(id),
            name: name.to_owned(),
            role,
            share_percent: role.share(),
            slots: 2,
            look: 0,
            paused: false,
            brain,
            reports_to: to.map(MemberId),
        }
    }

    fn team() -> Vec<Member> {
        vec![
            member(1, "Mara", Role::Ceo, Brain::Opus, None),
            member(2, "Ada", Role::Lead, Brain::Sonnet, Some(1)),
            member(3, "Dee", Role::Reviewer, Brain::Local, Some(2)),
            member(4, "Kit", Role::Lead, Brain::Sonnet, Some(1)),
            member(5, "Rae", Role::Developer, Brain::Haiku, Some(4)),
            member(6, "Zed", Role::Developer, Brain::Haiku, Some(4)),
        ]
    }

    #[test]
    fn the_wing_is_its_project_lead_and_whoever_reports_up_to_them() {
        let team = team();
        let leads = [MemberId(4)];
        let names: Vec<&str> = team
            .iter()
            .filter(|m| in_wing(m, &leads, &team))
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(names, ["Kit", "Rae", "Zed"]);
        let mut circular = team.clone();
        circular[1].reports_to = Some(MemberId(3));
        circular[2].reports_to = Some(MemberId(2));
        assert!(!in_wing(&circular[1], &leads, &circular), "a circle ends");
        assert!(!in_wing(&team[0], &[MemberId(1)], &team), "never the CEO");
    }

    #[test]
    fn the_wing_seats_one_sonnet_lead_and_five_haiku_developers() {
        let team = team();
        let wing: Vec<&Member> = team.iter().filter(|m| m.id.0 >= 4).collect();
        assert_eq!(may_seat(Role::Developer, Brain::Haiku, &wing), Ok(()));
        assert_eq!(may_seat(Role::Developer, Brain::Local, &wing), Ok(()));
        assert_eq!(
            may_seat(Role::Lead, Brain::Sonnet, &wing),
            Err(Refusal::NoSeat {
                role: Role::Lead,
                count: 1,
                most: 1
            })
        );
        assert!(matches!(
            may_seat(Role::Developer, Brain::Sonnet, &wing),
            Err(Refusal::WingBrain { .. })
        ));
        assert!(matches!(
            may_seat(Role::Researcher, Brain::Haiku, &wing),
            Err(Refusal::NoSeat { most: 0, .. })
        ));
        let mut full = team.clone();
        for i in 7..10 {
            full.push(member(
                i,
                &format!("Dev{i}"),
                Role::Developer,
                Brain::Haiku,
                Some(4),
            ));
        }
        let wing: Vec<&Member> = full.iter().filter(|m| m.id.0 >= 4).collect();
        assert_eq!(wing.iter().filter(|m| m.role == Role::Developer).count(), 5);
        let refused = may_seat(Role::Developer, Brain::Haiku, &wing).unwrap_err();
        assert_eq!(
            refused,
            Refusal::NoSeat {
                role: Role::Developer,
                count: 5,
                most: 5
            }
        );
        assert!(refused.to_string().contains("5 of its 5"), "{refused}");
        assert!(
            Refusal::NoSeat {
                role: Role::Researcher,
                count: 0,
                most: 0
            }
            .to_string()
            .contains("no seat for a researcher")
        );
    }

    #[test]
    fn a_hire_request_is_written_for_the_owner_and_read_back_whole() {
        let asked = HireRequest {
            name: "Sam".to_owned(),
            role: Role::Developer,
            brain: Brain::Haiku,
            reports_to: MemberId(4),
            why: "two branches wait with nobody free".to_owned(),
        };
        let text = asked.write("Kit", "task 12, r41");
        assert_eq!(
            text,
            "Hire Sam as developer on claude-haiku-5-5, reporting to member 4.\nKit asks: two branches wait with nobody free\nEvidence: task 12, r41"
        );
        assert_eq!(HireRequest::read(&text), Some(asked));
        assert_eq!(HireRequest::read("Push hq/x to your fork"), None);
        assert_eq!(
            HireRequest::read("Hire Sam as wizard on claude-haiku-5-5, reporting to member 4."),
            None
        );
        assert_eq!(spare_name(&team()), "Sam");
        let mut taken = team();
        for n in SPARE_NAMES {
            taken.push(member(10, n, Role::Developer, Brain::Local, None));
        }
        assert_eq!(spare_name(&taken), "Member 13");
    }
}
