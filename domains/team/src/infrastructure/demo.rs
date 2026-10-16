//! HQ's demo mode: a team of scripted members working through the simulated runner, so the
//! office can be built, tested and watched before any real worker exists. Every action a
//! scripted member takes goes through the same engine calls a real worker will make through
//! the team protocol; nothing here bypasses a rule. No model, no cost.

use crate::application::engine::{Author, Done, Engine};
use crate::application::ports::{Clock, NewTask, TeamStore};
use crate::domain::{GoalId, Member, Priority, Role, RunStatus, Task, TaskStatus};
use crate::infrastructure::SimulatedRunner;

/// What the demo team works on, in turn.
const WORK: &[&str] = &[
    "Check the backups ran last night",
    "Find out why a container keeps restarting",
    "Tidy the media library's unmatched files",
    "Compare two ways to watch the NAS's disks",
    "Write up what changed in the home lab this week",
];

/// Sets up the demo team: four members, a goal of the owner's, one Endora proposes, and the
/// first piece of work. Returns the owner's goal.
///
/// # Errors
/// What the engine refuses, or a store failure.
pub fn seed<S: TeamStore, C: Clock>(hq: &Engine<S, SimulatedRunner, C>) -> Done<GoalId> {
    hq.hire("Ada", Role::Lead, 25, 2, 0)?;
    hq.hire("Bo", Role::Developer, 40, 2, 4)?;
    hq.hire("Cy", Role::Researcher, 15, 2, 2)?;
    hq.hire("Dee", Role::Reviewer, 20, 2, 1)?;
    let goal = hq.set_goal(
        "Keep the home lab healthy",
        "A demo goal: the team is scripted",
        Author::Owner,
    )?;
    hq.set_goal(
        "Make Endora handle most things itself",
        "A demo proposal, waiting on your desk",
        Author::Endora,
    )?;
    next_piece_of_work(hq, goal, 0)?;
    Ok(goal)
}

fn next_piece_of_work<S: TeamStore, C: Clock>(
    hq: &Engine<S, SimulatedRunner, C>,
    goal: GoalId,
    done_so_far: usize,
) -> Done<()> {
    let lead = role(hq, Role::Lead)?;
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            parent: None,
            title: WORK[done_so_far % WORK.len()].to_owned(),
            description: "Demo work: the team is scripted, and nothing real is touched.".to_owned(),
            assignee: lead.map(|m| m.id),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )?;
    Ok(())
}

fn role<S: TeamStore, C: Clock>(
    hq: &Engine<S, SimulatedRunner, C>,
    role: Role,
) -> Done<Option<Member>> {
    Ok(hq.store().members()?.into_iter().find(|m| m.role == role))
}

/// Lets each scripted member take the turn it is running, then ends it; when the team has
/// nothing left to do, the owner hands it the next piece of work. Returns how many turns were
/// taken. Call it after each tick.
///
/// # Errors
/// What the engine refuses, or a store failure.
pub fn act<S: TeamStore, C: Clock>(
    hq: &Engine<S, SimulatedRunner, C>,
    goal: GoalId,
) -> Done<usize> {
    let members = hq.store().members()?;
    let mut taken = 0;
    for run in hq.store().runs()? {
        if run.status != RunStatus::Running {
            continue;
        }
        let Some(member) = members.iter().find(|m| m.id == run.member) else {
            continue;
        };
        if let Some(task) = run.task {
            let task = hq.store().task(task)?;
            if let Some(task) = task {
                take_a_turn(hq, member, &task, &members)?;
            }
        }
        hq.runner().finish(run.id, true);
        taken += 1;
    }
    let tasks = hq.store().tasks()?;
    if !tasks.is_empty() && tasks.iter().all(|t| t.status.finished()) {
        let finished = tasks.iter().filter(|t| t.parent.is_none()).count();
        next_piece_of_work(hq, goal, finished)?;
    }
    Ok(taken)
}

fn take_a_turn<S: TeamStore, C: Clock>(
    hq: &Engine<S, SimulatedRunner, C>,
    member: &Member,
    task: &Task,
    members: &[Member],
) -> Done<()> {
    let me = Author::Member(member.id);
    let reviewer = members.iter().find(|m| m.role == Role::Reviewer);
    match member.role {
        Role::Lead => lead_turn(hq, member, task, members),
        Role::Developer | Role::Researcher => {
            if task.status != TaskStatus::Todo && task.status != TaskStatus::Doing {
                return Ok(());
            }
            hq.claim(member.id, task.id)?;
            let doing = if member.role == Role::Developer {
                "Made the change and ran the tests."
            } else {
                "Read up on it and wrote down what I found."
            };
            hq.post(me, task.id, doing)?;
            hq.set_status(member.id, task.id, TaskStatus::InReview, "ready for a look")?;
            hq.record_gate(task.id, true, "")?;
            if let Some(reviewer) = reviewer {
                hq.post(
                    me,
                    task.id,
                    &format!("@{} could you review this?", reviewer.name),
                )?;
            }
            Ok(())
        }
        Role::Reviewer => {
            if task.status != TaskStatus::InReview || task.owner == Some(member.id) {
                return Ok(());
            }
            hq.approve(member.id, task.id)?;
            hq.post(me, task.id, "Checked it. Approved.")
        }
    }
}

/// The lead splits a piece of work between the developer and the researcher, waits for them,
/// and sends the whole for review when both are done.
fn lead_turn<S: TeamStore, C: Clock>(
    hq: &Engine<S, SimulatedRunner, C>,
    lead: &Member,
    task: &Task,
    members: &[Member],
) -> Done<()> {
    let me = Author::Member(lead.id);
    let subtasks: Vec<Task> = hq
        .store()
        .tasks()?
        .into_iter()
        .filter(|t| t.parent == Some(task.id))
        .collect();
    if subtasks.is_empty() {
        if task.status != TaskStatus::Todo {
            return Ok(());
        }
        hq.claim(lead.id, task.id)?;
        for (role, prefix) in [(Role::Developer, "Do"), (Role::Researcher, "Look into")] {
            let Some(to) = members.iter().find(|m| m.role == role) else {
                continue;
            };
            hq.add_task(
                me,
                NewTask {
                    goal: task.goal,
                    parent: Some(task.id),
                    title: format!("{prefix}: {}", task.title.to_lowercase()),
                    description: String::new(),
                    assignee: Some(to.id),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )?;
        }
        hq.post(me, task.id, "Split this between Bo and Cy.")?;
        return hq.set_status(
            lead.id,
            task.id,
            TaskStatus::Blocked,
            "waiting on the sub-tasks",
        );
    }
    if task.status != TaskStatus::Blocked || subtasks.iter().any(|t| !t.status.finished()) {
        return Ok(());
    }
    hq.set_status(lead.id, task.id, TaskStatus::Doing, "both parts are done")?;
    hq.set_status(lead.id, task.id, TaskStatus::InReview, "ready for a look")?;
    hq.record_gate(task.id, true, "")?;
    if let Some(reviewer) = members.iter().find(|m| m.role == Role::Reviewer) {
        hq.post(
            me,
            task.id,
            &format!("@{} the whole of it, please.", reviewer.name),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::engine::Settings;
    use crate::infrastructure::SqliteTeamStore;
    use std::sync::atomic::{AtomicI64, Ordering};

    struct Ticking(AtomicI64);
    impl Clock for &Ticking {
        fn now(&self) -> i64 {
            self.0.fetch_add(10_000, Ordering::SeqCst)
        }
    }

    #[test]
    fn the_demo_team_finishes_work_by_the_rules_and_keeps_going() {
        let clock = Ticking(AtomicI64::new(1_792_166_400_000));
        let hq = Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            SimulatedRunner::new(4),
            &clock,
            Settings::default(),
        );
        let goal = seed(&hq).unwrap();
        for _ in 0..30 {
            hq.tick().unwrap();
            act(&hq, goal).unwrap();
        }
        let desk = hq.store().desk().unwrap();
        let merged: Vec<_> = desk.iter().filter(|d| d.kind == "ready_to_merge").collect();
        assert!(
            merged.len() >= 3,
            "parts and the whole reached the desk: {desk:?}"
        );
        // Every finished task was approved by someone other than the one who did it.
        let members = hq.store().members().unwrap();
        let reviewer = members
            .iter()
            .find(|m| m.role == Role::Reviewer)
            .unwrap()
            .id;
        for task in hq.store().tasks().unwrap() {
            if task.status == TaskStatus::Done {
                assert_ne!(task.owner, Some(reviewer), "{}", task.title);
            }
        }
        let firsts = hq
            .store()
            .tasks()
            .unwrap()
            .into_iter()
            .filter(|t| t.parent.is_none())
            .count();
        assert!(firsts >= 2, "new work arrived once the first was done");
        assert!(desk.iter().any(|d| d.kind == "goal"), "the proposal waits");
        assert!(
            desk.iter().all(|d| d.kind != "stuck"),
            "the demo never gets stuck: {desk:?}"
        );
    }
}
