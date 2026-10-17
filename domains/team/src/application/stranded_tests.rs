//! Assigned work nobody will come back to: an open task with a member, no turn queued,
//! running or waiting to retry and no wake held for it, for ten minutes, gets one recovery
//! wake for its member; still stranded ten minutes after that, it goes on the owner's desk as
//! stuck, once. It is never handed to anyone else.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, STRANDED_AFTER_MS, Settings};
use super::ports::{Clock, NewTask, TeamStore};
use crate::domain::{MemberId, Priority, Receipt, Role, TaskId, TaskStatus, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000;

/// A task handed to Linus whose first turn ran and ended without claiming it: `todo`, an
/// assignee, no owner, and nothing queued for it.
fn stranded(clock: &Hands) -> (Hq<'_>, MemberId, TaskId) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal(
            "Keep the home lab healthy",
            "you asked for it",
            Author::Owner,
        )
        .unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                parent: None,
                title: "Clear the stuck downloads".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let first = hq.runner().started()[0].clone();
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();
    (hq, dev, task)
}

fn later(clock: &Hands, ms: i64) {
    clock.0.fetch_add(ms, Ordering::SeqCst);
}

fn recoveries(hq: &Hq<'_>, task: TaskId) -> Vec<Receipt> {
    hq.store()
        .wakes()
        .unwrap()
        .into_iter()
        .filter(|w| w.task == Some(task) && w.reason == WakeReason::AssignmentRecovery)
        .map(|w| w.receipt)
        .collect()
}

fn stuck(hq: &Hq<'_>, task: TaskId) -> usize {
    hq.store()
        .desk()
        .unwrap()
        .iter()
        .filter(|d| d.kind == "stuck" && d.task == Some(task))
        .count()
}

#[test]
fn stranded_assigned_work_gets_one_recovery_wake_after_ten_minutes() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = stranded(&clock);
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Todo
    );
    later(&clock, STRANDED_AFTER_MS - 1_000);
    hq.tick().unwrap();
    assert!(recoveries(&hq, task).is_empty(), "not ten minutes yet");
    later(&clock, 2_000);
    hq.tick().unwrap();
    assert_eq!(recoveries(&hq, task), [Receipt::Queued]);
    let woke = hq.store().wakes().unwrap().pop().unwrap();
    assert_eq!((woke.member, woke.task), (dev, Some(task)));
    assert!(
        !WakeReason::AssignmentRecovery.brings_news(),
        "the back-off applies"
    );
    assert_eq!(
        WakeReason::from_name("assignment_recovery"),
        Some(WakeReason::AssignmentRecovery)
    );
    // Its turn is queued, then running: not stranded, so no second wake.
    hq.tick().unwrap();
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(recoveries(&hq, task).len(), 1, "one recovery wake");
}

#[test]
fn still_stranded_after_its_recovery_it_goes_on_the_desk_once_and_stays_assigned() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = stranded(&clock);
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    hq.tick().unwrap();
    let recovery = hq.runner().started().pop().unwrap();
    hq.runner().finish(recovery.run, true);
    hq.tick().unwrap();
    assert_eq!(stuck(&hq, task), 0, "not ten minutes since the recovery");
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(stuck(&hq, task), 1);
    for _ in 0..3 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
    }
    assert_eq!(stuck(&hq, task), 1, "once");
    assert_eq!(recoveries(&hq, task).len(), 1, "and no more wakes");
    let after = hq.store().task(task).unwrap().unwrap();
    assert_eq!(after.assignee, Some(dev), "never handed to anyone else");
}

#[test]
fn a_paused_member_is_not_woken_and_its_work_is_not_called_stuck() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = stranded(&clock);
    hq.pause(dev, true).unwrap();
    for _ in 0..3 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
    }
    assert!(recoveries(&hq, task).is_empty());
    assert_eq!(stuck(&hq, task), 0);
}

/// Work moved to review before the reviewer was woken for it (as task 661 was, live, before the
/// reviewer path existed): in review, nothing queued for it, and nobody coming. Ten minutes
/// on, the reviewer is woken to look at it, once; the task's owner is not.
#[test]
fn work_left_in_review_with_no_review_coming_wakes_the_reviewer() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = stranded(&clock);
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    hq.store().claim(task, dev).unwrap();
    hq.store()
        .set_task_status(task, TaskStatus::InReview)
        .unwrap();
    later(&clock, STRANDED_AFTER_MS + 1_000);
    hq.tick().unwrap();
    let asked: Vec<_> = hq
        .store()
        .wakes()
        .unwrap()
        .into_iter()
        .filter(|w| w.task == Some(task) && w.reason == WakeReason::ReviewRequested)
        .collect();
    assert_eq!(asked.len(), 1, "the reviewer is woken once");
    assert_eq!(asked[0].member, dee);
    assert!(
        recoveries(&hq, task).is_empty(),
        "the owner is not woken for work under review"
    );
    later(&clock, 1_000);
    hq.tick().unwrap();
    assert_eq!(
        hq.store()
            .wakes()
            .unwrap()
            .iter()
            .filter(|w| w.task == Some(task) && w.reason == WakeReason::ReviewRequested)
            .count(),
        1,
        "and only once"
    );
}
