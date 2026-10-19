//! Why work closed (ADR 0124): done and dropped each carry a reason from a small set, and may
//! name the task that carries the work on. A drop with no reason left the work that waited on
//! it guessing whether the need had gone or only this attempt had; and "done" said nothing of
//! whether the criteria were met or the work was overtaken.

use super::guards::GuardRefusal;
use super::work::{Millis, Refusal, TaskId, TaskStatus};

/// The refusal of a closing reason not in the set for `status`.
#[must_use]
pub const fn not_a_closure(status: TaskStatus) -> Refusal {
    Refusal::Guard(GuardRefusal::NotAClosure(status))
}

/// Whether `task`, closed this way, names what it must: work done as superseded says which task
/// carries it on (ADR 0124, amended), and no task is its own successor.
///
/// # Errors
/// [`GuardRefusal::NoSuccessor`]; [`GuardRefusal::SuccessorIsItself`].
pub fn may_close(
    task: TaskId,
    closure: Closure,
    successor: Option<TaskId>,
) -> Result<(), GuardRefusal> {
    if successor == Some(task) {
        return Err(GuardRefusal::SuccessorIsItself(task));
    }
    if closure == Closure::Done(DoneWhy::Superseded) && successor.is_none() {
        return Err(GuardRefusal::NoSuccessor);
    }
    Ok(())
}

/// Why work is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoneWhy {
    /// Its criteria were met, on evidence.
    MetCriteria,
    /// Other work overtook it: its successor carries it on.
    Superseded,
    /// It turned out not to be needed.
    NotNeeded,
}

/// Why work is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropWhy {
    /// What it was for is gone.
    Obsolete,
    /// Another task is the same work.
    Duplicate(TaskId),
    /// It will not be done.
    WontDo,
    /// It cannot go on, and will not be able to.
    BlockedForever,
}

/// How work closed, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closure {
    /// Done, and why.
    Done(DoneWhy),
    /// Dropped, and why.
    Dropped(DropWhy),
}

impl Closure {
    /// The status a task closed this way is in.
    #[must_use]
    pub const fn status(self) -> TaskStatus {
        match self {
            Self::Done(_) => TaskStatus::Done,
            Self::Dropped(_) => TaskStatus::Dropped,
        }
    }

    /// Stable lowercase name of the reason.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Done(DoneWhy::MetCriteria) => "met_criteria",
            Self::Done(DoneWhy::Superseded) => "superseded",
            Self::Done(DoneWhy::NotNeeded) => "not_needed",
            Self::Dropped(DropWhy::Obsolete) => "obsolete",
            Self::Dropped(DropWhy::Duplicate(_)) => "duplicate",
            Self::Dropped(DropWhy::WontDo) => "wont_do",
            Self::Dropped(DropWhy::BlockedForever) => "blocked_forever",
        }
    }

    /// The task it duplicates, for a duplicate.
    #[must_use]
    pub const fn duplicate_of(self) -> Option<TaskId> {
        match self {
            Self::Dropped(DropWhy::Duplicate(of)) => Some(of),
            _ => None,
        }
    }

    /// The reason in the owner's words.
    #[must_use]
    pub fn plainly(self) -> String {
        match self {
            Self::Done(DoneWhy::MetCriteria) => "it met its criteria".to_owned(),
            Self::Done(DoneWhy::Superseded) => "other work superseded it".to_owned(),
            Self::Done(DoneWhy::NotNeeded) => "it was not needed".to_owned(),
            Self::Dropped(DropWhy::Obsolete) => "what it was for is gone".to_owned(),
            Self::Dropped(DropWhy::Duplicate(of)) => format!("it duplicates #{of}"),
            Self::Dropped(DropWhy::WontDo) => "it will not be done".to_owned(),
            Self::Dropped(DropWhy::BlockedForever) => "it cannot go on".to_owned(),
        }
    }

    /// A reason as a member or the owner gave it, for work closing as `status`: one of the
    /// set's names, a duplicate naming what it duplicates.
    ///
    /// # Errors
    /// [`not_a_closure`] for a name not in the set for that status, or a duplicate with
    /// nothing named.
    pub fn read(
        status: TaskStatus,
        why: &str,
        duplicate_of: Option<TaskId>,
    ) -> Result<Self, Refusal> {
        let closure = match (status, why.trim()) {
            (TaskStatus::Done, "met_criteria") => Self::Done(DoneWhy::MetCriteria),
            (TaskStatus::Done, "superseded") => Self::Done(DoneWhy::Superseded),
            (TaskStatus::Done, "not_needed") => Self::Done(DoneWhy::NotNeeded),
            (TaskStatus::Dropped, "obsolete") => Self::Dropped(DropWhy::Obsolete),
            (TaskStatus::Dropped, "duplicate") => match duplicate_of {
                Some(of) => Self::Dropped(DropWhy::Duplicate(of)),
                None => return Err(not_a_closure(status)),
            },
            (TaskStatus::Dropped, "wont_do") => Self::Dropped(DropWhy::WontDo),
            (TaskStatus::Dropped, "blocked_forever") => Self::Dropped(DropWhy::BlockedForever),
            _ => return Err(not_a_closure(status)),
        };
        Ok(closure)
    }

    /// The reasons there are for work closing as `status`, by name.
    #[must_use]
    pub const fn names(status: TaskStatus) -> &'static [&'static str] {
        match status {
            TaskStatus::Done => &["met_criteria", "superseded", "not_needed"],
            TaskStatus::Dropped => &["obsolete", "duplicate", "wont_do", "blocked_forever"],
            _ => &[],
        }
    }
}

/// A task's closing, as HQ keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closed {
    /// The task.
    pub task: TaskId,
    /// How, and why.
    pub closure: Closure,
    /// The task that carries its work on, if any.
    pub successor: Option<TaskId>,
    /// Who closed it: `owner`, `endora`, or a member's name.
    pub by: String,
    /// When.
    pub at: Millis,
}

impl Closed {
    /// What the work waiting on a dropped task is told: why it was dropped, that the wait is
    /// not over, and what carries it on if anything does.
    #[must_use]
    pub fn told_to_waiters(&self, title: &str) -> String {
        let successor = self.successor.map_or_else(String::new, |s| {
            format!(" Its work goes on in #{s}: wait on that instead if it is what you need.")
        });
        format!(
            "#{} ({title}), which this waits on, was dropped: {}. This stays blocked: what it \
             waited for will not happen.{successor} Decide whether this still needs doing, and \
             say so here, or ask.",
            self.task,
            self.closure.plainly()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_and_dropped_each_have_a_small_set_of_reasons() {
        for name in Closure::names(TaskStatus::Done) {
            let c = Closure::read(TaskStatus::Done, name, None).unwrap();
            assert_eq!((c.name(), c.status()), (*name, TaskStatus::Done));
        }
        for name in Closure::names(TaskStatus::Dropped) {
            let c = Closure::read(TaskStatus::Dropped, name, Some(TaskId(7))).unwrap();
            assert_eq!((c.name(), c.status()), (*name, TaskStatus::Dropped));
        }
        assert_eq!(
            Closure::read(TaskStatus::Done, "obsolete", None),
            Err(not_a_closure(TaskStatus::Done)),
            "a drop's reason is not done's"
        );
        assert_eq!(
            Closure::read(TaskStatus::Dropped, "because", None),
            Err(not_a_closure(TaskStatus::Dropped))
        );
        assert_eq!(
            Closure::read(TaskStatus::Dropped, "duplicate", None),
            Err(not_a_closure(TaskStatus::Dropped)),
            "a duplicate names what it duplicates"
        );
        let dup = Closure::read(TaskStatus::Dropped, "duplicate", Some(TaskId(7))).unwrap();
        assert_eq!(dup.duplicate_of(), Some(TaskId(7)));
        assert_eq!(dup.plainly(), "it duplicates #7");
        assert!(
            not_a_closure(TaskStatus::Dropped)
                .to_string()
                .contains("obsolete, duplicate")
        );
    }

    #[test]
    fn work_closed_as_superseded_names_what_carries_it_on_and_nothing_supersedes_itself() {
        let superseded = Closure::Done(DoneWhy::Superseded);
        assert_eq!(
            may_close(TaskId(4), superseded, None),
            Err(GuardRefusal::NoSuccessor)
        );
        assert_eq!(may_close(TaskId(4), superseded, Some(TaskId(9))), Ok(()));
        assert_eq!(
            may_close(TaskId(4), superseded, Some(TaskId(4))),
            Err(GuardRefusal::SuccessorIsItself(TaskId(4)))
        );
        // The other reasons need none, and a drop may name one but not itself.
        for closure in [
            Closure::Done(DoneWhy::MetCriteria),
            Closure::Done(DoneWhy::NotNeeded),
            Closure::Dropped(DropWhy::Obsolete),
            Closure::Dropped(DropWhy::WontDo),
            Closure::Dropped(DropWhy::BlockedForever),
            Closure::Dropped(DropWhy::Duplicate(TaskId(2))),
        ] {
            assert_eq!(may_close(TaskId(4), closure, None), Ok(()), "{closure:?}");
            assert_eq!(
                may_close(TaskId(4), closure, Some(TaskId(9))),
                Ok(()),
                "{closure:?}"
            );
            assert_eq!(
                may_close(TaskId(4), closure, Some(TaskId(4))),
                Err(GuardRefusal::SuccessorIsItself(TaskId(4))),
                "{closure:?}"
            );
        }
        assert!(
            GuardRefusal::NoSuccessor
                .to_string()
                .contains("names the task that carries it on")
        );
    }

    #[test]
    fn work_waiting_on_a_dropped_task_is_told_why_and_what_carries_it_on() {
        let closed = Closed {
            task: TaskId(4),
            closure: Closure::Dropped(DropWhy::Obsolete),
            successor: Some(TaskId(9)),
            by: "Ada".to_owned(),
            at: 0,
        };
        let told = closed.told_to_waiters("Move the indexer");
        assert!(
            told.starts_with(
                "#4 (Move the indexer), which this waits on, was dropped: what it was for is gone."
            ),
            "{told}"
        );
        assert!(told.contains("This stays blocked"), "{told}");
        assert!(told.contains("#9"), "{told}");
        let alone = Closed {
            successor: None,
            ..closed
        };
        assert!(!alone.told_to_waiters("x").contains("goes on in"));
    }
}
