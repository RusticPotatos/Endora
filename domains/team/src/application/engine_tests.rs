//! The engine, end to end over HQ's real SQLite store and the simulated runner.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, EngineError, Settings};
use super::ports::{Clock, NewTask, TeamStore};
use crate::domain::money::MOST_A_TURN;
use crate::domain::runs::{LONGEST_TURN_MS, RETRY_AFTER_MS};
use crate::domain::{
    GoalId, MemberId, Priority, Receipt, Refusal, Refused, Role, RunStatus, TaskId, TaskStatus,
    WakeReason,
};
use crate::infrastructure::{Script, SimulatedRunner, SqliteTeamStore};

/// A clock the test moves by hand.
struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
impl Hands {
    fn forward(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000; // 2026-10-16 12:00 UTC

fn hands() -> Hands {
    Hands(AtomicI64::new(NOON))
}

fn hq(clock: &Hands, capacity: usize) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(capacity),
        clock,
        Settings::default(),
    )
}

struct Team {
    lead: MemberId,
    dev: MemberId,
    reviewer: MemberId,
    goal: GoalId,
}

fn team(hq: &Hq<'_>) -> Team {
    Team {
        lead: hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap(),
        dev: hq.hire("Linus", Role::Developer, 40, 2, 0).unwrap(),
        reviewer: hq.hire("Grace", Role::Reviewer, 20, 2, 0).unwrap(),
        goal: hq
            .set_goal("Ship the watch tracker", "you asked for it", Author::Owner)
            .unwrap(),
    }
}

fn task_for(hq: &Hq<'_>, goal: GoalId, to: Option<MemberId>, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: to,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn status_of(hq: &Hq<'_>, run: i64) -> RunStatus {
    hq.store()
        .run(crate::domain::RunId(run))
        .unwrap()
        .unwrap()
        .status
}

fn kinds(hq: &Hq<'_>) -> Vec<String> {
    hq.store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

#[test]
fn an_assigned_task_queues_a_turn_and_the_tick_starts_it_with_its_why() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, Some(t.dev), "Add a list of watches");
    let report = hq.tick().unwrap();
    assert_eq!(report.started, 1);
    let started = hq.runner().started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].member, t.dev);
    assert_eq!(started[0].task, Some(task));
    assert!(
        started[0]
            .told
            .contains("You are Linus, the team's developer.")
    );
    assert!(
        started[0]
            .told
            .contains("Why it exists: Add a list of watches ← goal: Ship the watch tracker")
    );
    let wakes = hq.wakes().unwrap();
    assert_eq!(wakes[0].receipt, Receipt::Started);
    let feed = kinds(&hq);
    assert!(feed.contains(&"wake.queued".to_owned()) && feed.contains(&"run.started".to_owned()));
}

#[test]
fn a_second_wake_for_the_member_on_the_task_folds_into_its_turn() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    assert_eq!(
        hq.wake(t.dev, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Folded
    );
    assert_eq!(hq.store().runs().unwrap().len(), 1, "still one turn");
}

#[test]
fn a_wake_for_another_member_is_held_and_released_oldest_first_when_the_turn_ends() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    let running = hq.store().runs().unwrap()[0].id;
    assert_eq!(
        hq.wake(t.reviewer, Some(task), WakeReason::Mentioned)
            .unwrap(),
        Receipt::Held
    );
    assert_eq!(
        hq.wake(t.lead, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Held
    );
    assert_eq!(
        hq.wake(t.reviewer, Some(task), WakeReason::Mentioned)
            .unwrap(),
        Receipt::Refused(Refused::Duplicate)
    );
    hq.runner().finish(running, true);
    hq.tick().unwrap();
    let runs = hq.store().runs().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(
        runs[1].member, t.reviewer,
        "the oldest held wake goes first"
    );
    assert_eq!(
        runs[1].status,
        RunStatus::Running,
        "and starts in the same tick"
    );
    let held: Vec<MemberId> = hq
        .store()
        .held_wakes(task)
        .unwrap()
        .iter()
        .map(|w| w.member)
        .collect();
    assert_eq!(held, [t.lead], "the next waits its turn");
}

#[test]
fn a_task_is_claimed_by_one_member_and_the_other_is_told_who_has_it() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, None, "Pick a database");
    hq.claim(t.dev, task).unwrap();
    assert_eq!(
        hq.claim(t.reviewer, task),
        Err(EngineError::Refused(Refusal::TakenBy(t.dev)))
    );
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!(now.owner, Some(t.dev));
    assert_eq!(now.status, TaskStatus::Doing);
}

#[test]
fn only_the_owner_moves_a_task_and_never_to_done() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, None, "Pick a database");
    hq.claim(t.dev, task).unwrap();
    assert_eq!(
        hq.set_status(t.reviewer, task, TaskStatus::InReview, "looks fine"),
        Err(EngineError::Refused(Refusal::NotYours))
    );
    assert_eq!(
        hq.set_status(t.dev, task, TaskStatus::Done, "I am sure"),
        Err(EngineError::Refused(Refusal::OnlyTheGate))
    );
    hq.set_status(t.dev, task, TaskStatus::InReview, "ready")
        .unwrap();
    let thread = hq.store().thread(task).unwrap();
    assert_eq!(thread.last().unwrap().text, "doing → in_review: ready");
}

#[test]
fn done_is_the_gate_and_a_different_reviewer_and_lands_on_the_desk() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, None, "Add a list");
    hq.claim(t.dev, task).unwrap();
    hq.set_status(t.dev, task, TaskStatus::InReview, "ready")
        .unwrap();
    assert_eq!(
        hq.approve(t.reviewer, task),
        Err(EngineError::Refused(Refusal::GateNotPassed))
    );
    hq.record_gate(task, true, "").unwrap();
    assert_eq!(
        hq.approve(t.dev, task),
        Err(EngineError::Refused(Refusal::OwnWork))
    );
    hq.approve(t.reviewer, task).unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    let desk = hq.store().desk().unwrap();
    assert_eq!(desk.last().unwrap().kind, "ready_to_merge");
}

#[test]
fn work_doubted_is_handed_back_to_the_lead_not_dropped() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, None, "Rewrite everything");
    hq.claim(t.dev, task).unwrap();
    assert_eq!(
        hq.set_status(t.dev, task, TaskStatus::Dropped, "pointless"),
        Err(EngineError::Refused(Refusal::HandBack))
    );
    hq.set_status(t.dev, task, TaskStatus::Todo, "this would break the app")
        .unwrap();
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!((now.owner, now.status), (None, TaskStatus::Todo));
    let woken: Vec<(MemberId, WakeReason)> = hq
        .wakes()
        .unwrap()
        .iter()
        .map(|w| (w.member, w.reason))
        .collect();
    assert!(woken.contains(&(t.lead, WakeReason::Assigned)), "{woken:?}");
}

#[test]
fn a_member_hands_work_only_down_and_the_task_counts_the_hands() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let parent = task_for(&hq, t.goal, Some(t.lead), "Plan the tracker");
    let new = |assignee| NewTask {
        goal: t.goal,
        parent: Some(parent),
        title: "Build the list".to_owned(),
        description: String::new(),
        assignee: Some(assignee),
        priority: Priority::High,
        waits_on: Vec::new(),
    };
    assert_eq!(
        hq.add_task(Author::Member(t.dev), new(t.lead)),
        Err(EngineError::Refused(Refusal::NotBelow))
    );
    let child = hq.add_task(Author::Member(t.lead), new(t.dev)).unwrap();
    assert_eq!(hq.store().task(child).unwrap().unwrap().depth, 1);
}

#[test]
fn naming_a_member_in_a_thread_wakes_them_but_not_the_writer() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, None, "Pick a database");
    hq.post_as(t.dev, task, "@Grace can you check this? @Linus noted")
        .unwrap();
    let woken: Vec<MemberId> = hq.wakes().unwrap().iter().map(|w| w.member).collect();
    assert_eq!(woken, [t.reviewer]);
}

#[test]
fn a_turn_whose_container_vanishes_is_retried_twice_a_minute_apart_then_fails() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    hq.runner().script(t.dev, Script::Vanishes);
    task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap(); // started; vanishes at once
    let run = hq.store().runs().unwrap()[0].id.0;
    assert_eq!(hq.tick().unwrap().retrying, 1);
    assert_eq!(status_of(&hq, run), RunStatus::RetryWaiting);
    hq.tick().unwrap();
    assert_eq!(
        status_of(&hq, run),
        RunStatus::RetryWaiting,
        "not before its minute"
    );
    clock.forward(RETRY_AFTER_MS);
    let report = hq.tick().unwrap();
    assert_eq!((report.requeued, report.started), (1, 1), "second attempt");
    hq.tick().unwrap(); // vanishes again: one retry left
    clock.forward(RETRY_AFTER_MS);
    hq.tick().unwrap(); // third attempt starts
    assert_eq!(hq.tick().unwrap().ended, 1);
    assert_eq!(status_of(&hq, run), RunStatus::Failed);
    let ended = hq.store().run(crate::domain::RunId(run)).unwrap().unwrap();
    assert_eq!(
        (ended.attempts, ended.end_reason.as_str()),
        (3, "its container is gone")
    );
}

#[test]
fn a_turn_that_goes_on_too_long_is_stopped_as_timed_out() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    let run = hq.store().runs().unwrap()[0].id;
    clock.forward(LONGEST_TURN_MS + 1);
    hq.tick().unwrap();
    assert_eq!(status_of(&hq, run.0), RunStatus::TimedOut);
    assert_eq!(hq.runner().stopped(), [run]);
}

#[test]
fn doing_without_a_turn_gets_one_nudge_then_goes_to_the_desk_as_stuck() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    hq.runner().script(t.dev, Script::Succeeds);
    let task = task_for(&hq, t.goal, None, "Pick a database");
    hq.claim(t.dev, task).unwrap(); // doing, and no turn
    let report = hq.tick().unwrap();
    assert_eq!(report.nudged, 1);
    let nudge = hq.wakes().unwrap();
    assert_eq!(nudge.last().unwrap().reason, WakeReason::Recovery);
    // The nudged turn starts and ends without moving the task.
    hq.tick().unwrap();
    hq.tick().unwrap();
    let after = hq.store().task(task).unwrap().unwrap();
    assert_eq!(after.status, TaskStatus::Blocked);
    let desk = hq.store().desk().unwrap();
    assert_eq!(desk.last().unwrap().kind, "stuck");
}

#[test]
fn a_turn_that_moves_the_task_earns_it_a_fresh_nudge() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, None, "Pick a database");
    hq.claim(t.dev, task).unwrap();
    hq.tick().unwrap(); // nudged: its turn is queued after this tick's starts (step 5 after 4)
    hq.tick().unwrap(); // and starts on the next
    let run = hq.store().runs().unwrap()[0].id;
    assert_eq!(status_of(&hq, run.0), RunStatus::Running);
    hq.post_as(t.dev, task, "Compared three; SQLite fits.")
        .unwrap();
    hq.runner().finish(run, true);
    let report = hq.tick().unwrap();
    // It is still doing with no turn, but it moved: a fresh nudge, not the desk.
    assert_eq!((report.nudged, report.stuck), (1, 0));
    assert!(hq.store().desk().unwrap().is_empty(), "not stuck: it moved");
}

#[test]
fn a_turn_reserves_before_it_starts_and_is_stopped_the_moment_it_crosses() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    let run = hq.store().runs().unwrap()[0].clone();
    assert_eq!(run.reserved, MOST_A_TURN);
    assert!(
        hq.report_usage(run.id, 300_000).unwrap(),
        "inside its limit"
    );
    assert!(
        !hq.report_usage(run.id, 150_000).unwrap(),
        "crossed: stop now"
    );
    assert_eq!(status_of(&hq, run.id.0), RunStatus::Stopped);
    assert_eq!(hq.runner().stopped(), [run.id]);
    assert!(kinds(&hq).contains(&"budget.crossed".to_owned()));
}

#[test]
fn a_report_too_large_to_add_up_is_held_to_a_turn_and_hq_keeps_going() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    let run = hq.store().runs().unwrap()[0].clone();
    let day = crate::domain::money::day_of(NOON, 0);
    assert!(hq.report_usage(run.id, 1).unwrap());
    assert!(
        !hq.report_usage(run.id, i64::MAX).unwrap(),
        "held to the most a turn may spend, which crosses it"
    );
    hq.tick().unwrap();
    assert_eq!(
        hq.store().committed(None, day).unwrap(),
        MOST_A_TURN + 1,
        "charged what was reported, held to a turn"
    );
    hq.snapshot().unwrap();
    assert!(
        !hq.report_usage(run.id, i64::MAX).unwrap(),
        "a late report is held the same way"
    );
    hq.tick().unwrap();
    assert_eq!(
        hq.store().committed(None, day).unwrap(),
        2 * MOST_A_TURN + 1
    );
    assert!(
        !hq.report_usage(run.id, -5).unwrap(),
        "nothing below zero is taken off"
    );
    assert_eq!(
        hq.store().committed(None, day).unwrap(),
        2 * MOST_A_TURN + 1
    );
}

#[test]
fn a_charge_that_lands_after_the_turn_ended_is_still_counted() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    let run = hq.store().runs().unwrap()[0].clone();
    let day = crate::domain::money::day_of(NOON, 0);
    assert!(hq.report_usage(run.id, 20_000).unwrap());
    hq.stop_all("trying it").unwrap();
    assert_eq!(
        hq.store().committed(None, day).unwrap(),
        20_000,
        "the reservation released, what it spent charged"
    );
    // A stream that was still running when the kill switch was pulled.
    assert!(
        !hq.report_usage(run.id, 150_000).unwrap(),
        "it may not go on"
    );
    assert_eq!(hq.store().committed(None, day).unwrap(), 170_000);
    assert!(!hq.report_usage(run.id, 30_000).unwrap());
    assert_eq!(
        hq.store().committed(None, day).unwrap(),
        200_000,
        "each charge counted once"
    );
    assert_eq!(
        hq.store().committed(Some(t.dev), day).unwrap(),
        200_000,
        "and to its member"
    );
    let ended = hq.store().run(run.id).unwrap().unwrap();
    assert_eq!((ended.status, ended.spent), (RunStatus::Stopped, 200_000));
    assert!(
        hq.turn_of(&hq.runner().started()[0].token)
            .unwrap()
            .is_none(),
        "the charge opens nothing again"
    );
}

#[test]
fn a_member_whose_day_is_spent_is_turned_away() {
    let clock = hands();
    let hq = hq(&clock, 8);
    let t = team(&hq);
    // The researcher's day is about $0.35: one turn reserves it all.
    let researcher = hq.hire("Marie", Role::Researcher, 15, 4, 0).unwrap();
    let first = task_for(&hq, t.goal, Some(researcher), "Read up on straps");
    hq.tick().unwrap();
    assert_eq!(hq.store().runs().unwrap()[0].task, Some(first));
    let second = task_for(&hq, t.goal, None, "Read up on bracelets");
    assert_eq!(
        hq.wake(researcher, Some(second), WakeReason::Assigned)
            .unwrap(),
        Receipt::Refused(Refused::OutOfBudget)
    );
    // Tomorrow is a new day.
    let run = hq.store().runs().unwrap()[0].id;
    hq.runner().finish(run, true);
    hq.tick().unwrap();
    clock.forward(24 * 60 * 60 * 1000);
    assert_eq!(
        hq.wake(researcher, Some(second), WakeReason::Assigned)
            .unwrap(),
        Receipt::Queued
    );
}

#[test]
fn a_member_works_only_as_many_tasks_as_it_has_slots_and_a_runner_its_capacity() {
    let clock = hands();
    let hq = hq(&clock, 2);
    let t = team(&hq);
    for title in ["One", "Two", "Three"] {
        task_for(&hq, t.goal, Some(t.dev), title);
    }
    task_for(&hq, t.goal, Some(t.reviewer), "Four");
    hq.tick().unwrap();
    let running = |hq: &Hq<'_>| {
        hq.store()
            .runs()
            .unwrap()
            .into_iter()
            .filter(|r| r.status == RunStatus::Running)
            .count()
    };
    assert_eq!(running(&hq), 2, "the runner's capacity");
    let dev_running = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.status == RunStatus::Running && r.member == t.dev)
        .count();
    assert_eq!(dev_running, 2, "the developer's two slots, oldest first");
}

#[test]
fn urgent_work_starts_before_older_normal_work() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    task_for(&hq, t.goal, Some(t.dev), "Normal");
    let urgent = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                parent: None,
                title: "The site is down".to_owned(),
                description: String::new(),
                assignee: Some(t.reviewer),
                priority: Priority::Urgent,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(hq.runner().started()[0].task, Some(urgent));
}

#[test]
fn the_kill_switch_stops_every_turn_and_turns_every_wake_away_until_resumed() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, Some(t.dev), "Add a list");
    task_for(&hq, t.goal, Some(t.reviewer), "Review");
    hq.tick().unwrap();
    hq.stop_all("trying it").unwrap();
    assert!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .all(|r| r.status == RunStatus::Stopped)
    );
    assert_eq!(hq.runner().stopped().len(), 2);
    assert_eq!(
        hq.wake(t.dev, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Refused(Refused::KillSwitch)
    );
    assert_eq!(hq.tick().unwrap().started, 0);
    hq.resume().unwrap();
    assert_eq!(
        hq.wake(t.dev, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Queued
    );
    let feed = kinds(&hq);
    assert!(feed.contains(&"stopped".to_owned()) && feed.contains(&"resumed".to_owned()));
}

#[test]
fn a_task_that_waits_is_woken_once_when_what_it_waits_on_finishes() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let first = task_for(&hq, t.goal, None, "Design the schema");
    let second = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                parent: None,
                title: "Write the migrations".to_owned(),
                description: String::new(),
                assignee: Some(t.dev),
                priority: Priority::Normal,
                waits_on: vec![first],
            },
        )
        .unwrap();
    assert!(hq.wakes().unwrap().is_empty(), "nothing to do yet");
    hq.drop_task(first).unwrap();
    assert_eq!(hq.tick().unwrap().woken, 1);
    assert_eq!(hq.tick().unwrap().woken, 0, "once");
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!(
        (wake.member, wake.task, wake.reason),
        (t.dev, Some(second), WakeReason::WaitFinished)
    );
}

#[test]
fn when_the_last_sub_task_finishes_the_parents_owner_is_woken() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let parent = task_for(&hq, t.goal, None, "Plan");
    hq.claim(t.lead, parent).unwrap();
    let child = |title: &str| {
        hq.add_task(
            Author::Member(t.lead),
            NewTask {
                goal: t.goal,
                parent: Some(parent),
                title: title.to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap()
    };
    let (a, b) = (child("A"), child("B"));
    hq.drop_task(a).unwrap();
    assert!(hq.wakes().unwrap().is_empty());
    hq.drop_task(b).unwrap();
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!(
        (wake.member, wake.reason),
        (t.lead, WakeReason::SubtasksFinished)
    );
}

#[test]
fn a_goal_endora_proposes_waits_on_the_owners_desk() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let goal = hq
        .set_goal(
            "Tidy the home lab",
            "three containers keep restarting",
            Author::Endora,
        )
        .unwrap();
    assert_eq!(
        hq.store().goal(goal).unwrap().unwrap().status,
        crate::domain::GoalStatus::Proposed
    );
    assert_eq!(hq.store().desk().unwrap()[0].kind, "goal");
    assert!(kinds(&hq).contains(&"goal.proposed".to_owned()));
}

#[test]
fn a_paused_member_is_turned_away_and_a_runner_that_says_no_is_a_retry() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    hq.pause(t.reviewer, true).unwrap();
    let task = task_for(&hq, t.goal, None, "Review");
    assert_eq!(
        hq.wake(t.reviewer, Some(task), WakeReason::Mentioned)
            .unwrap(),
        Receipt::Refused(Refused::Paused)
    );
    hq.runner().script(t.dev, Script::WillNotStart);
    task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    let run = hq.store().runs().unwrap()[0].clone();
    assert_eq!(run.status, RunStatus::RetryWaiting);
    assert!(run.end_reason.starts_with("could not start"));
}

#[test]
fn a_timer_wakes_the_lead_and_a_late_tick_fires_it_once() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    hq.store()
        .add_timer(t.lead, WakeReason::LeadReview, NOON, 60 * 60 * 1000)
        .unwrap();
    clock.forward(5 * 60 * 60 * 1000); // five hours late
    assert_eq!(hq.tick().unwrap().timers, 1);
    assert_eq!(
        hq.tick().unwrap().timers,
        0,
        "missed ticks are not replayed"
    );
    let wake = &hq.wakes().unwrap()[0];
    assert_eq!((wake.member, wake.reason), (t.lead, WakeReason::LeadReview));
}

#[test]
fn the_snapshot_holds_what_is_on_screen_and_where_the_stream_resumes() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let parent = task_for(&hq, t.goal, Some(t.lead), "Plan the tracker");
    let child = hq
        .add_task(
            Author::Member(t.lead),
            NewTask {
                goal: t.goal,
                parent: Some(parent),
                title: "Build the list".to_owned(),
                description: String::new(),
                assignee: Some(t.dev),
                priority: Priority::High,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["stopped"], false);
    assert_eq!(snap["members"].as_array().unwrap().len(), 3);
    assert_eq!(snap["members"][1]["per_day"], 933_333);
    let row = snap["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == child.0)
        .unwrap()
        .clone();
    assert_eq!(
        row["why"],
        serde_json::json!([
            "Build the list",
            "Plan the tracker",
            "Goal: Ship the watch tracker"
        ])
    );
    assert_eq!(
        snap["runs"].as_array().unwrap().len(),
        2,
        "both turns are live"
    );
    let last = snap["last_event"].as_i64().unwrap();
    assert!(
        hq.events_after(last).unwrap().is_empty(),
        "nothing after the snapshot yet"
    );
    hq.post(Author::Owner, child, "How is it going?").unwrap();
    assert_eq!(hq.events_after(last).unwrap()[0].kind, "thread.posted");
}

#[test]
fn the_owner_decides_each_desk_item_once() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let proposed = hq
        .set_goal(
            "Tidy the home lab",
            "containers keep restarting",
            Author::Endora,
        )
        .unwrap();
    let item = hq.store().desk().unwrap()[0].id;
    hq.decide(item, "approve").unwrap();
    assert_eq!(
        hq.store().goal(proposed).unwrap().unwrap().status,
        crate::domain::GoalStatus::Active
    );
    assert_eq!(
        hq.decide(item, "approve"),
        Err(EngineError::Refused(Refusal::Finished))
    );
    assert!(
        hq.snapshot().unwrap()["desk"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Stuck work, tried again: back to its member with a fresh nudge.
    hq.runner().script(t.dev, Script::Succeeds);
    let task = task_for(&hq, t.goal, None, "Pick a database");
    hq.claim(t.dev, task).unwrap();
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.tick().unwrap();
    let stuck = hq.store().desk().unwrap().pop().unwrap();
    assert_eq!(stuck.kind, "stuck");
    assert_eq!(
        hq.decide(stuck.id, "approve"),
        Err(EngineError::Unknown("decision for this item"))
    );
    hq.decide(stuck.id, "retry").unwrap();
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!(
        (now.status, now.owner, now.nudged),
        (TaskStatus::Todo, None, false)
    );
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!(
        (wake.member, wake.reason),
        (t.dev, WakeReason::ProposalDecided)
    );
}

#[test]
fn only_whole_work_reaches_the_desk_and_research_is_to_read() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let researcher = hq.hire("Marie", Role::Researcher, 15, 2, 2).unwrap();
    let whole = task_for(&hq, t.goal, None, "Find out why a container restarts");
    let part = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                parent: Some(whole),
                title: "Read its logs".to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    for (task, worker) in [(part, t.dev), (whole, researcher)] {
        hq.claim(worker, task).unwrap();
        hq.set_status(worker, task, TaskStatus::InReview, "ready")
            .unwrap();
        hq.record_gate(task, true, "").unwrap();
        hq.approve(t.reviewer, task).unwrap();
    }
    let desk = hq.store().desk().unwrap();
    assert_eq!(desk.len(), 1, "the part stayed with the team: {desk:?}");
    assert_eq!(
        (desk[0].kind.as_str(), desk[0].task),
        ("ready_to_read", Some(whole))
    );
    assert!(desk[0].text.starts_with("Ready to read: "));
    hq.decide(desk[0].id, "read").unwrap();
}

#[test]
fn the_snapshot_counts_what_the_office_shows_and_old_events_can_go() {
    let clock = hands();
    let hq = hq(&clock, 4);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, Some(t.dev), "Add a list");
    hq.tick().unwrap();
    hq.wake(t.dev, Some(task), WakeReason::Mentioned).unwrap();
    hq.wake(t.reviewer, Some(task), WakeReason::Mentioned)
        .unwrap();
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["counts"]["ticks"], 1);
    assert_eq!(snap["counts"]["running"], 1);
    assert_eq!(snap["counts"]["folded"], 1);
    assert_eq!(snap["counts"]["held"], 1);
    assert_eq!(snap["counts"]["open_tasks"], 1);
    assert_eq!(snap["last_tick_at"], NOON);
    let before = hq.store().events_after(0).unwrap().len();
    assert!(before > 3);
    assert_eq!(hq.prune_events(3).unwrap(), before - 3);
    assert_eq!(
        hq.store().events_after(0).unwrap().len(),
        3,
        "the newest three stay"
    );
}
