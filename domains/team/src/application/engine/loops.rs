//! One team of loops (ADR 0142): every active project that is not a scenario world is a loop,
//! working its plan (its tasks, by priority then age) one item at a time. A loop with nothing in
//! hand gives its top item to the member at its starting rank, or the nearest one above it who is
//! free to take work. Only members who are one model take part: until the owner sets the
//! members' models, the team works as before.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{
    Member, MemberId, ProjectId, ProjectStatus, Refusal, Task, TaskId, TaskStatus, WakeReason,
};

/// How many failed checks at one rank move an item up (ADR 0142).
const FAILURES_A_RANK: u32 = 3;

/// Where an item's failures at its rank are counted.
fn failures_key(task: TaskId) -> String {
    format!("loop_failures:{}", task.0)
}

/// The rank a loop starts at when the owner has not said: the house's model.
const START: u8 = 5;

/// Where a loop's starting rank is kept.
fn rank_key(project: ProjectId) -> String {
    format!("loop_rank:{}", project.0)
}

/// Whether a member takes part in the loops: it is one model, and free to take work.
fn in_the_loops(member: &Member) -> bool {
    member.brain == member.work_brain && member.at_work()
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The owner sets the rank a loop's items start at: 1 (Opus) to 5 (the house's model).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project that does not exist; [`Refusal::Scenario`] for a
    /// rank outside 1 to 5; a store failure.
    pub fn set_loop_rank(&self, project: ProjectId, rank: u8) -> Done<()> {
        let _held = self.held();
        self.project(project)?;
        if !(1..=5).contains(&rank) {
            return Err(EngineError::Refused(Refusal::Scenario(
                "a loop starts at a rank from 1 (Opus) to 5 (the house's model)".to_owned(),
            )));
        }
        self.store
            .set_setting(&rank_key(project), &rank.to_string())?;
        Ok(())
    }

    /// The rank a loop's items start at.
    pub(super) fn loop_rank(&self, project: ProjectId) -> Done<u8> {
        Ok(self
            .store
            .setting(&rank_key(project))?
            .and_then(|r| r.parse().ok())
            .unwrap_or(START))
    }

    /// The members who take part in the loops.
    fn loop_members(&self) -> Done<Vec<Member>> {
        Ok(self
            .store
            .members()?
            .into_iter()
            .filter(in_the_loops)
            .collect())
    }

    /// The member who holds an item, if it is one who takes part in the loops.
    fn loop_holder(&self, task: &Task) -> Done<Option<Member>> {
        let Some(holder) = task.owner.or(task.assignee) else {
            return Ok(None);
        };
        Ok(self.loop_members()?.into_iter().find(|m| m.id == holder))
    }

    /// The member nearest above `rank` (a smaller number), if any.
    fn one_rank_up(members: &[Member], rank: u8) -> Option<&Member> {
        members
            .iter()
            .filter(|m| m.brain.rank() < rank)
            .max_by_key(|m| (m.brain.rank(), std::cmp::Reverse(m.id.0)))
    }

    /// Who reviews an item in the loops (ADR 0142): the member one rank above its holder, and
    /// the top rank's work the member just below it. `None` for work outside the loops.
    ///
    /// # Errors
    /// A store failure.
    pub(crate) fn loop_reviewer_of(&self, task: &Task) -> Done<Option<Member>> {
        let Some(holder) = self.loop_holder(task)? else {
            return Ok(None);
        };
        let members = self.loop_members()?;
        let rank = holder.brain.rank();
        let up = Self::one_rank_up(&members, rank).or_else(|| {
            members
                .iter()
                .filter(|m| m.id != holder.id && m.brain.rank() > rank)
                .min_by_key(|m| (m.brain.rank(), m.id.0))
        });
        Ok(up.cloned())
    }

    /// An item's failures at its present rank.
    ///
    /// # Errors
    /// A store failure.
    pub fn loop_failures(&self, task: TaskId) -> Done<u32> {
        Ok(self
            .store
            .setting(&failures_key(task))?
            .and_then(|n| n.parse().ok())
            .unwrap_or(0))
    }

    /// One of an item's failures (a failed check, or a send-back), the lock held. The third at
    /// a rank hands it to the member one rank up; the top rank's third goes to the owner.
    ///
    /// # Errors
    /// A store failure.
    pub(crate) fn loop_failed(&self, task: TaskId) -> Done<()> {
        let found = self.task(task)?;
        let Some(holder) = self.loop_holder(&found)? else {
            return Ok(());
        };
        let failures = self.loop_failures(task)? + 1;
        if failures < FAILURES_A_RANK {
            self.store
                .set_setting(&failures_key(task), &failures.to_string())?;
            return Ok(());
        }
        self.store.set_setting(&failures_key(task), "0")?;
        let members = self.loop_members()?;
        let Some(up) = Self::one_rank_up(&members, holder.brain.rank()) else {
            let text = format!(
                "{} failed at every rank, Mara's last: {}. Its thread says what was tried.",
                found.title, holder.name
            );
            self.store
                .add_desk("question", Some(task), None, None, &text, self.clock.now())?;
            return self.emit(
                crate::domain::EventKind::DeskAdded,
                serde_json::json!({ "kind": "question", "task": task.0 }),
            );
        };
        let up: MemberId = up.id;
        let to = self.member(up)?;
        self.store.release(task)?;
        self.store.set_assignee(task, Some(up))?;
        if found.status != TaskStatus::Todo {
            self.store.set_task_status(task, TaskStatus::Todo)?;
        }
        self.post_held_lock(
            super::Author::Endora,
            task,
            &format!(
                "It failed {FAILURES_A_RANK} times at {}'s rank: it goes to {}, one rank up.",
                holder.name, to.name
            ),
        )?;
        self.wake_held_lock(up, Some(task), WakeReason::Assigned)?;
        Ok(())
    }

    /// The loops' part of the tick: each loop with nothing in hand hands out its top item.
    pub(super) fn loops_tick(&self) -> Done<()> {
        let members: Vec<Member> = self
            .store
            .members()?
            .into_iter()
            .filter(in_the_loops)
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        let tasks = self.store.tasks()?;
        for project in self.store.projects()? {
            if project.status != ProjectStatus::Active || project.pause.is_some() {
                continue;
            }
            if self.holds_scenarios(project.id).is_ok() {
                continue;
            }
            let plan: Vec<_> = tasks
                .iter()
                .filter(|t| t.project == Some(project.id))
                .collect();
            let in_hand = plan.iter().any(|t| {
                (t.assignee.is_some() || t.owner.is_some())
                    && matches!(t.status, TaskStatus::Todo | TaskStatus::Doing)
            });
            if in_hand {
                continue;
            }
            let Some(top) = plan
                .iter()
                .filter(|t| {
                    t.status == TaskStatus::Todo && t.assignee.is_none() && t.owner.is_none()
                })
                .min_by_key(|t| (t.priority, t.id.0))
            else {
                continue;
            };
            let start = self.loop_rank(project.id)?;
            // The member at the starting rank, or the nearest one above it.
            let Some(member) = members
                .iter()
                .filter(|m| m.brain.rank() <= start)
                .max_by_key(|m| (m.brain.rank(), std::cmp::Reverse(m.id.0)))
            else {
                continue;
            };
            self.store.set_assignee(top.id, Some(member.id))?;
            self.post_held_lock(
                super::Author::Endora,
                top.id,
                &format!(
                    "{} takes this: the next item of the {} loop.",
                    member.name, project.title
                ),
            )?;
            self.wake_held_lock(member.id, Some(top.id), WakeReason::Assigned)?;
        }
        Ok(())
    }
}
