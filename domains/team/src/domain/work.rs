//! The work: members, goals and tasks, and the rules about who may do what with them.

use core::fmt;

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub i64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id!(
    /// A member of the team.
    MemberId
);
id!(
    /// A goal.
    GoalId
);
id!(
    /// A task.
    TaskId
);
id!(
    /// One turn of a member.
    RunId
);
id!(
    /// A request that a member take a turn.
    WakeId
);
id!(
    /// An item on the owner's desk.
    DeskId
);

/// What a member does on the team. The rank decides who may hand work to whom: by default a
/// member hands work only to members below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Looks over the team's work and hands it out.
    Lead,
    /// Writes and changes code.
    Developer,
    /// Finds things out and writes them up.
    Researcher,
    /// Reviews what others did.
    Reviewer,
}

impl Role {
    /// Stable lowercase name, for storage and the event feed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Developer => "developer",
            Self::Researcher => "researcher",
            Self::Reviewer => "reviewer",
        }
    }

    /// The role with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "lead" => Some(Self::Lead),
            "developer" => Some(Self::Developer),
            "researcher" => Some(Self::Researcher),
            "reviewer" => Some(Self::Reviewer),
            _ => None,
        }
    }

    /// Lower is more senior.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Lead => 0,
            Self::Developer | Self::Researcher | Self::Reviewer => 1,
        }
    }
}

/// One worker on the team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Its id.
    pub id: MemberId,
    /// What it is called.
    pub name: String,
    /// What it does.
    pub role: Role,
    /// Its share of the team pool, in percent.
    pub share_percent: u8,
    /// How many tasks it may work at once.
    pub slots: u8,
    /// Which character it looks like in the office.
    pub look: u8,
    /// Whether the owner has paused it.
    pub paused: bool,
}

/// Where a goal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    /// Proposed by Endora, waiting for the owner.
    Proposed,
    /// Being worked towards.
    Active,
    /// Reached.
    Done,
    /// Given up.
    Dropped,
}

impl GoalStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Active => "active",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    /// The status with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "proposed" => Some(Self::Proposed),
            "active" => Some(Self::Active),
            "done" => Some(Self::Done),
            "dropped" => Some(Self::Dropped),
            _ => None,
        }
    }
}

/// What a team is for, in a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    /// Its id.
    pub id: GoalId,
    /// The sentence.
    pub title: String,
    /// Why it matters.
    pub reason: String,
    /// Where it is.
    pub status: GoalStatus,
    /// The goal it serves, if it is a smaller one.
    pub parent: Option<GoalId>,
}

/// Where a task is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Not started.
    Todo,
    /// Being worked.
    Doing,
    /// Waiting for the gate and a review.
    InReview,
    /// The gate passed and a different member approved it.
    Done,
    /// Cannot go on without something or someone.
    Blocked,
    /// Given up.
    Dropped,
}

impl TaskStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::InReview => "in_review",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Dropped => "dropped",
        }
    }

    /// The status with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "todo" => Some(Self::Todo),
            "doing" => Some(Self::Doing),
            "in_review" => Some(Self::InReview),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "dropped" => Some(Self::Dropped),
            _ => None,
        }
    }

    /// Whether the task is over: nothing more will be done on it.
    #[must_use]
    pub const fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

/// How soon a task matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Before anything else.
    Urgent,
    /// Soon.
    High,
    /// In turn.
    Normal,
    /// When there is nothing else.
    Low,
}

impl Priority {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }

    /// The priority with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "urgent" => Some(Self::Urgent),
            "high" => Some(Self::High),
            "normal" => Some(Self::Normal),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

/// One piece of work towards a goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Its id.
    pub id: TaskId,
    /// The goal it is for.
    pub goal: GoalId,
    /// The task it is part of, if it is a sub-task.
    pub parent: Option<TaskId>,
    /// What it is.
    pub title: String,
    /// What is wanted.
    pub description: String,
    /// The one member working it, if any.
    pub owner: Option<MemberId>,
    /// Who it is meant for: the member it was handed to, whether or not they have taken it.
    pub assignee: Option<MemberId>,
    /// Where it is.
    pub status: TaskStatus,
    /// How soon it matters.
    pub priority: Priority,
    /// How many hands it passed through from the goal.
    pub depth: u8,
    /// The tasks it waits on.
    pub waits_on: Vec<TaskId>,
    /// Whether it has had its one recovery nudge.
    pub nudged: bool,
}

/// Why something a member asked for is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Another member has the task.
    TakenBy(MemberId),
    /// The member does not have the task.
    NotYours,
    /// Only the gate decides that work is done.
    OnlyTheGate,
    /// That change is not one a task makes.
    NotAStep {
        /// Where it is.
        from: TaskStatus,
        /// Where it was asked to go.
        to: TaskStatus,
    },
    /// Work is handed only to members below.
    NotBelow,
    /// The task is over.
    Finished,
    /// Asked to review its own work.
    OwnWork,
    /// The gate has not passed.
    GateNotPassed,
    /// Work a member doubts is handed back to its lead, never dropped by it.
    HandBack,
    /// The member's last question about the task still waits on the owner's desk.
    AlreadyAsked,
    /// The task is not one this turn may touch.
    OutOfReach,
    /// The turn has ended.
    TurnOver,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TakenBy(member) => write!(f, "member {member} has this task"),
            Self::NotYours => f.write_str("this task is not yours; claim it first"),
            Self::OnlyTheGate => f.write_str("only the gate decides that work is done"),
            Self::NotAStep { from, to } => {
                write!(
                    f,
                    "a task does not go from {} to {}",
                    from.name(),
                    to.name()
                )
            }
            Self::NotBelow => f.write_str("work is handed only to members below you"),
            Self::Finished => f.write_str("this task is over"),
            Self::OwnWork => f.write_str("work is reviewed by a different member"),
            Self::GateNotPassed => f.write_str("the gate has not passed"),
            Self::HandBack => {
                f.write_str("a member does not drop work; hand it back to your lead with a reason")
            }
            Self::OutOfReach => f.write_str(
                "that task is not part of this turn: you act on your own task and its sub-tasks, \
                 and write there, on the task above yours, or where you were named; ask in your \
                 own task's thread instead",
            ),
            Self::TurnOver => f.write_str("this turn has ended: stop now"),
            Self::AlreadyAsked => f.write_str(
                "your last question about this task is still waiting on the owner's desk; end your turn",
            ),
        }
    }
}

/// Whether a member may move its task from one status to another. `done` is never a member's
/// to set: only the gate and a different member's review make it. Nor is `dropped`: work a
/// member doubts goes back to its lead (`todo`, with a reason), and only the owner drops it.
///
/// # Errors
/// The [`Refusal`] that says why not.
pub fn may_set_status(from: TaskStatus, to: TaskStatus) -> Result<(), Refusal> {
    use TaskStatus::{Blocked, Doing, Done, Dropped, InReview, Todo};
    if to == Done {
        return Err(Refusal::OnlyTheGate);
    }
    if to == Dropped {
        return Err(Refusal::HandBack);
    }
    if from.finished() {
        return Err(Refusal::Finished);
    }
    let allowed = matches!(
        (from, to),
        (Todo | Blocked | InReview, Doing) | (Doing, InReview | Blocked | Todo)
    );
    if !allowed {
        return Err(Refusal::NotAStep { from, to });
    }
    Ok(())
}

/// Whether `from` may hand work to `to`: only to members below it.
///
/// # Errors
/// [`Refusal::NotBelow`] otherwise.
pub fn may_delegate(from: Role, to: Role) -> Result<(), Refusal> {
    if to.rank() > from.rank() {
        return Ok(());
    }
    Err(Refusal::NotBelow)
}

/// Whether work is done: the gate passed, and the member approving it is not the one who did
/// it. A member saying it is done changes nothing.
///
/// # Errors
/// The [`Refusal`] that says why it is not done.
pub fn is_done(
    gate_passed: bool,
    worker: Option<MemberId>,
    reviewer: MemberId,
) -> Result<(), Refusal> {
    if worker == Some(reviewer) {
        return Err(Refusal::OwnWork);
    }
    if !gate_passed {
        return Err(Refusal::GateNotPassed);
    }
    Ok(())
}

/// The members named in a thread post (`@name`, case-insensitive), each once, in order.
#[must_use]
pub fn mentions<'a>(text: &str, members: &'a [Member]) -> Vec<&'a Member> {
    let mut found: Vec<&Member> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || c == ',' || c == '.') {
        let Some(name) = word.strip_prefix('@') else {
            continue;
        };
        let name = name.trim_end_matches(|c: char| !c.is_alphanumeric());
        if let Some(member) = members.iter().find(|m| m.name.eq_ignore_ascii_case(name)) {
            if !found.iter().any(|f| f.id == member.id) {
                found.push(member);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: i64, name: &str, role: Role) -> Member {
        Member {
            id: MemberId(id),
            name: name.to_owned(),
            role,
            share_percent: 25,
            slots: 2,
            look: 0,
            paused: false,
        }
    }

    #[test]
    fn a_member_never_says_its_own_work_is_done() {
        for from in [TaskStatus::Todo, TaskStatus::Doing, TaskStatus::InReview] {
            assert_eq!(
                may_set_status(from, TaskStatus::Done),
                Err(Refusal::OnlyTheGate)
            );
        }
    }

    #[test]
    fn a_task_moves_only_by_real_steps_and_not_after_it_is_over() {
        assert!(may_set_status(TaskStatus::Todo, TaskStatus::Doing).is_ok());
        assert!(may_set_status(TaskStatus::Doing, TaskStatus::InReview).is_ok());
        assert!(may_set_status(TaskStatus::Doing, TaskStatus::Blocked).is_ok());
        assert!(may_set_status(TaskStatus::Blocked, TaskStatus::Doing).is_ok());
        assert!(
            may_set_status(TaskStatus::Doing, TaskStatus::Todo).is_ok(),
            "handed back"
        );
        assert_eq!(
            may_set_status(TaskStatus::Doing, TaskStatus::Dropped),
            Err(Refusal::HandBack),
            "doubted work goes back to the lead"
        );
        assert_eq!(
            may_set_status(TaskStatus::Todo, TaskStatus::InReview),
            Err(Refusal::NotAStep {
                from: TaskStatus::Todo,
                to: TaskStatus::InReview
            })
        );
        assert_eq!(
            may_set_status(TaskStatus::Done, TaskStatus::Doing),
            Err(Refusal::Finished)
        );
    }

    #[test]
    fn work_is_handed_down_and_never_up_or_across() {
        assert!(may_delegate(Role::Lead, Role::Developer).is_ok());
        assert!(may_delegate(Role::Lead, Role::Reviewer).is_ok());
        assert_eq!(
            may_delegate(Role::Developer, Role::Lead),
            Err(Refusal::NotBelow)
        );
        assert_eq!(
            may_delegate(Role::Developer, Role::Researcher),
            Err(Refusal::NotBelow)
        );
        assert_eq!(may_delegate(Role::Lead, Role::Lead), Err(Refusal::NotBelow));
    }

    #[test]
    fn done_needs_the_gate_and_a_different_reviewer() {
        let dev = Some(MemberId(1));
        assert_eq!(is_done(true, dev, MemberId(1)), Err(Refusal::OwnWork));
        assert_eq!(
            is_done(false, dev, MemberId(2)),
            Err(Refusal::GateNotPassed)
        );
        assert!(is_done(true, dev, MemberId(2)).is_ok());
    }

    #[test]
    fn a_post_names_members_once_each() {
        let team = [
            member(1, "Ada", Role::Lead),
            member(2, "Linus", Role::Developer),
        ];
        let named: Vec<i64> = mentions("@ada can you ask @Linus, then @ADA again? @nobody", &team)
            .iter()
            .map(|m| m.id.0)
            .collect();
        assert_eq!(named, [1, 2]);
    }

    #[test]
    fn names_round_trip() {
        for status in [
            TaskStatus::Todo,
            TaskStatus::Doing,
            TaskStatus::InReview,
            TaskStatus::Done,
            TaskStatus::Blocked,
            TaskStatus::Dropped,
        ] {
            assert_eq!(TaskStatus::from_name(status.name()), Some(status));
        }
        for role in [
            Role::Lead,
            Role::Developer,
            Role::Researcher,
            Role::Reviewer,
        ] {
            assert_eq!(Role::from_name(role.name()), Some(role));
        }
        for p in [
            Priority::Urgent,
            Priority::High,
            Priority::Normal,
            Priority::Low,
        ] {
            assert_eq!(Priority::from_name(p.name()), Some(p));
        }
        for g in [
            GoalStatus::Proposed,
            GoalStatus::Active,
            GoalStatus::Done,
            GoalStatus::Dropped,
        ] {
            assert_eq!(GoalStatus::from_name(g.name()), Some(g));
        }
    }
}
