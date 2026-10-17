//! HQ's demo mode: a team of scripted members working through the simulated runner, so the
//! office can be built, tested and watched before any real worker exists. Every action a
//! scripted member takes goes through the same engine calls a real worker will make through
//! the team protocol; nothing here bypasses a rule. No model, no cost.

use crate::application::engine::{Author, Done, Engine};
use crate::application::ports::{Clock, NewTask, TeamStore, WorkerAdapter};
use crate::domain::{
    Goal, GoalId, GoalStatus, Member, Priority, Role, RunId, RunStatus, Task, TaskStatus,
};
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
pub fn seed<S: TeamStore, W: WorkerAdapter, C: Clock>(hq: &Engine<S, W, C>) -> Done<GoalId> {
    hq.hire("Ada", Role::Lead, 25, 2, 0)?;
    hq.hire("Bo", Role::Developer, 40, 2, 4)?;
    hq.hire("Cy", Role::Researcher, 15, 2, 2)?;
    hq.hire("Dee", Role::Reviewer, 20, 2, 1)?;
    let goal = hq.set_goal("Keep the home lab healthy", DEMO_GOAL, Author::Owner)?;
    hq.set_goal(
        "Make Endora handle most things itself",
        DEMO_PROPOSAL,
        Author::Endora,
    )?;
    next_piece_of_work(hq, goal, 0)?;
    Ok(goal)
}

/// The reason the demo's goal is given, by which HQ knows it again.
pub const DEMO_GOAL: &str = "A demo goal: the team is scripted";
/// The reason the demo's proposed goal is given.
pub const DEMO_PROPOSAL: &str = "A demo proposal, waiting on your desk";

/// The demo's goal, for a team already set up: the active goal the demo gave, or a new one
/// with its first piece of work when that was retired. Never a goal of the owner's.
///
/// # Errors
/// What the engine refuses, or a store failure.
pub fn goal<S: TeamStore, W: WorkerAdapter, C: Clock>(hq: &Engine<S, W, C>) -> Done<GoalId> {
    if hq.store().members()?.is_empty() {
        return seed(hq);
    }
    let found = hq
        .store()
        .goals()?
        .into_iter()
        .find(|g| g.status == GoalStatus::Active && g.reason == DEMO_GOAL);
    if let Some(found) = found {
        return Ok(found.id);
    }
    let goal = hq.set_goal("Keep the home lab healthy", DEMO_GOAL, Author::Owner)?;
    next_piece_of_work(hq, goal, 0)?;
    Ok(goal)
}

/// Puts the demo's work away, for HQ started with the demo off: its goals (the one it set and
/// the one it proposed) are retired, with their open tasks and the turns queued for them, so
/// none of it becomes a real turn. A notice on the owner's desk says what went. Returns how
/// many goals it retired: none once they are gone, so starting again changes nothing.
///
/// # Errors
/// What the engine refuses, or a store failure.
pub fn retire<S: TeamStore, W: WorkerAdapter, C: Clock>(hq: &Engine<S, W, C>) -> Done<usize> {
    let goals: Vec<Goal> = hq
        .store()
        .goals()?
        .into_iter()
        .filter(|g| matches!(g.status, GoalStatus::Active | GoalStatus::Proposed))
        .filter(|g| g.reason == DEMO_GOAL || g.reason == DEMO_PROPOSAL)
        .collect();
    if goals.is_empty() {
        return Ok(0);
    }
    let mut tasks = 0;
    for goal in &goals {
        tasks += hq.retire_goal(goal.id, "the demo is off")?;
    }
    let titles: Vec<String> = goals.iter().map(|g| format!("\"{}\"", g.title)).collect();
    hq.notice(
        None,
        &format!(
            "The demo is off, so its work was put away: {} dropped, with {tasks} open task{}. \
             None of it becomes a real turn. Set the team a goal of your own to start it.",
            titles.join(" and "),
            if tasks == 1 { "" } else { "s" },
        ),
    )?;
    Ok(goals.len())
}

/// How many items may wait on the owner's desk before the demo team is handed no more work:
/// a desk the owner has stopped clearing is a team with nothing to do, not a pile that grows.
pub const MOST_ON_THE_DESK: usize = 20;

fn next_piece_of_work<S: TeamStore, W: WorkerAdapter, C: Clock>(
    hq: &Engine<S, W, C>,
    goal: GoalId,
    done_so_far: usize,
) -> Done<()> {
    let lead = role(hq, Role::Lead)?;
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: None,
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

fn role<S: TeamStore, W: WorkerAdapter, C: Clock>(
    hq: &Engine<S, W, C>,
    role: Role,
) -> Done<Option<Member>> {
    Ok(hq.store().members()?.into_iter().find(|m| m.role == role))
}

/// Lets each scripted member take the turn it is running, then ends it; when the team has
/// nothing left to do, the owner hands it the next piece of work. Returns how many turns were
/// taken. Call it after each tick, with the simulated runner `hq` takes its turns on.
///
/// # Errors
/// What the engine refuses, or a store failure.
pub fn act<S: TeamStore, W: WorkerAdapter, C: Clock>(
    hq: &Engine<S, W, C>,
    scripted: &SimulatedRunner,
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
                take_a_turn(hq, run.id, member, &task, &members)?;
            }
        }
        scripted.finish(run.id, true);
        taken += 1;
    }
    let tasks = hq.store().tasks()?;
    let waiting = hq
        .store()
        .desk()?
        .iter()
        .filter(|d| d.status == "waiting")
        .count();
    if waiting < MOST_ON_THE_DESK && !tasks.is_empty() && tasks.iter().all(|t| t.status.finished())
    {
        let finished = tasks.iter().filter(|t| t.parent.is_none()).count();
        next_piece_of_work(hq, goal, finished)?;
    }
    Ok(taken)
}

fn take_a_turn<S: TeamStore, W: WorkerAdapter, C: Clock>(
    hq: &Engine<S, W, C>,
    run: RunId,
    member: &Member,
    task: &Task,
    members: &[Member],
) -> Done<()> {
    let reviewer = members.iter().find(|m| m.role == Role::Reviewer);
    match member.role {
        // The demo team has no CEO; one added to it takes no scripted step.
        Role::Ceo => Ok(()),
        Role::Lead => lead_turn(hq, run, member, task, members),
        Role::Developer | Role::Researcher => {
            if task.status != TaskStatus::Todo && task.status != TaskStatus::Doing {
                return Ok(());
            }
            hq.claim_in(run, task.id)?;
            let doing = if member.role == Role::Developer {
                "Made the change and ran the tests."
            } else {
                "Read up on it and wrote down what I found."
            };
            hq.post_in(run, task.id, doing)?;
            hq.set_status_in(run, task.id, TaskStatus::InReview, "ready for a look")?;
            hq.record_gate(task.id, true, "")?;
            if let Some(reviewer) = reviewer {
                hq.post_in(
                    run,
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
            hq.post_in(run, task.id, "Checked it. Approved.")
        }
    }
}

/// The lead splits a piece of work between the developer and the researcher, waits for them,
/// and sends the whole for review when both are done.
fn lead_turn<S: TeamStore, W: WorkerAdapter, C: Clock>(
    hq: &Engine<S, W, C>,
    run: RunId,
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
        hq.claim_in(run, task.id)?;
        for (role, prefix) in [(Role::Developer, "Do"), (Role::Researcher, "Look into")] {
            let Some(to) = members.iter().find(|m| m.role == role) else {
                continue;
            };
            hq.add_task(
                me,
                NewTask {
                    goal: task.goal,
                    project: None,
                    parent: Some(task.id),
                    title: format!("{prefix}: {}", task.title.to_lowercase()),
                    description: String::new(),
                    assignee: Some(to.id),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )?;
        }
        hq.post_in(run, task.id, "Split this between Bo and Cy.")?;
        return hq.set_status_in(
            run,
            task.id,
            TaskStatus::Blocked,
            "waiting on the sub-tasks",
        );
    }
    if task.status != TaskStatus::Blocked || subtasks.iter().any(|t| !t.status.finished()) {
        return Ok(());
    }
    hq.set_status_in(run, task.id, TaskStatus::Doing, "both parts are done")?;
    hq.set_status_in(run, task.id, TaskStatus::InReview, "ready for a look")?;
    hq.record_gate(task.id, true, "")?;
    if let Some(reviewer) = members.iter().find(|m| m.role == Role::Reviewer) {
        hq.post_in(
            run,
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
            act(&hq, hq.runner(), goal).unwrap();
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

    #[test]
    fn with_the_demo_off_its_work_is_put_away_once_and_the_owners_is_left_alone() {
        let clock = Ticking(AtomicI64::new(1_792_166_400_000));
        let hq = Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            SimulatedRunner::new(4),
            &clock,
            Settings::default(),
        );
        let demo = seed(&hq).unwrap();
        for _ in 0..3 {
            hq.tick().unwrap();
            act(&hq, hq.runner(), demo).unwrap();
        }
        let owners = hq
            .set_goal("Keep the home lab healthy", "", Author::Owner)
            .unwrap();
        assert_eq!(retire(&hq).unwrap(), 2, "the demo's goal and its proposal");
        let goals = hq.store().goals().unwrap();
        for g in goals.iter().filter(|g| g.id != owners) {
            assert_eq!(g.status, GoalStatus::Dropped, "{}", g.title);
        }
        assert_eq!(
            goals.iter().find(|g| g.id == owners).unwrap().status,
            GoalStatus::Active
        );
        assert!(
            hq.store()
                .tasks()
                .unwrap()
                .iter()
                .all(|t| t.status.finished()),
            "no open demo work"
        );
        let notices: Vec<_> = hq
            .store()
            .desk()
            .unwrap()
            .into_iter()
            .filter(|d| d.kind == "notice")
            .collect();
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].text.contains("\"Keep the home lab healthy\""),
            "{}",
            notices[0].text
        );
        // No demo turn starts after, and starting again changes nothing.
        let before = hq.runner().started().len();
        hq.tick().unwrap();
        hq.tick().unwrap();
        assert_eq!(hq.runner().started().len(), before);
        assert_eq!(retire(&hq).unwrap(), 0);
        // Turned on again, the demo works on a goal of its own, never the owner's.
        let again = goal(&hq).unwrap();
        assert_ne!(again, owners);
    }

    #[test]
    fn the_team_has_no_new_work_once_twenty_wait_on_the_desk_and_resumes_when_some_are_cleared() {
        let clock = Ticking(AtomicI64::new(1_792_166_400_000));
        let hq = Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            SimulatedRunner::new(4),
            &clock,
            Settings::default(),
        );
        let goal = seed(&hq).unwrap();
        let waiting = || {
            hq.store()
                .desk()
                .unwrap()
                .into_iter()
                .filter(|d| d.status == "waiting")
                .collect::<Vec<_>>()
        };
        for _ in 0..600 {
            hq.tick().unwrap();
            act(&hq, hq.runner(), goal).unwrap();
        }
        let full = waiting();
        assert_eq!(
            full.len(),
            MOST_ON_THE_DESK,
            "the desk filled to twenty and stopped"
        );
        let pieces = || {
            hq.store()
                .tasks()
                .unwrap()
                .iter()
                .filter(|t| t.parent.is_none())
                .count()
        };
        let before = pieces();
        for _ in 0..50 {
            hq.tick().unwrap();
            act(&hq, hq.runner(), goal).unwrap();
        }
        assert_eq!(pieces(), before, "no new work while the desk is full");
        // The owner clears some: the team gets work again.
        for item in full.iter().filter(|d| d.kind == "ready_to_merge").take(3) {
            hq.decide(item.id, "merged").unwrap();
        }
        for _ in 0..50 {
            hq.tick().unwrap();
            act(&hq, hq.runner(), goal).unwrap();
        }
        assert!(pieces() > before, "work resumed once the desk had room");
        assert!(waiting().len() <= MOST_ON_THE_DESK);
    }
}
