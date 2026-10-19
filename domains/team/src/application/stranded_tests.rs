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
                project: None,
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
fn still_stranded_after_its_recovery_it_climbs_the_ladder_one_rung_at_a_time_and_stays_assigned() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = stranded(&clock);
    let lead = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    // What a tick queued starts on the next: two ticks, then every turn that started ends.
    let finish_all = || {
        hq.tick().unwrap();
        hq.tick().unwrap();
        for c in hq.runner().started() {
            hq.runner().finish(c.run, true);
        }
        hq.tick().unwrap();
    };
    later(&clock, STRANDED_AFTER_MS + 1);
    finish_all();
    assert_eq!(stuck(&hq, task), 0, "not ten minutes since the recovery");
    let reasons = |member: MemberId| -> Vec<WakeReason> {
        hq.wakes()
            .unwrap()
            .into_iter()
            .filter(|w| w.member == member && w.task == Some(task))
            .map(|w| w.reason)
            .collect()
    };
    // The first rung: its area's lead decides.
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(reasons(lead), [WakeReason::Decide]);
    finish_all();
    // Still stuck: a huddle hears who touched it, then the lead decides.
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert!(
        reasons(dev).contains(&WakeReason::Huddle),
        "{:?}",
        reasons(dev)
    );
    finish_all();
    assert_eq!(reasons(lead), [WakeReason::Decide, WakeReason::Decide]);
    finish_all();
    assert_eq!(stuck(&hq, task), 0, "the lead and a huddle come first");
    // Still stuck, with no CEO to climb to: the owner's desk, as before there was one, once.
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(stuck(&hq, task), 1);
    for _ in 0..3 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
    }
    assert_eq!(stuck(&hq, task), 1, "once");
    assert_eq!(recoveries(&hq, task).len(), 1, "and no more recovery wakes");
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

/// Live (task 670): work in review whose tester came and ended without a verdict was demoted to
/// `blocked` at the next quiet window, 44 minutes after it entered review. A blocked task is out
/// of the tester's reach (she passes only work in review), and only its holder could put it
/// back: three attempts to set it back were refused and it climbed to the CEO five times. The
/// work was fine; the review had not happened. It stays in review, and its lead is woken.
///
/// Given work in review whose tester was woken once and ended without a verdict,
/// when it goes on sitting, window after window,
/// then it stays in review, its lead is woken to decide it, and it is never blocked.
#[test]
fn work_in_review_with_no_verdict_stays_in_review_and_its_lead_is_woken() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = stranded(&clock);
    let lead = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    hq.store().claim(task, dev).unwrap();
    hq.store()
        .set_task_status(task, TaskStatus::InReview)
        .unwrap();
    // The tester's work and the lead's decision wait for the Mac (ADR 0131): the node says free,
    // and what started ends without a word about the task.
    let all_over = || {
        for _ in 0..3 {
            hq.tell_mac(true, "ready");
            hq.tick().unwrap();
            for started in hq.runner().started() {
                hq.runner().finish(started.run, true);
            }
        }
    };
    let status = || hq.store().task(task).unwrap().unwrap().status;
    let reasons = |member: MemberId| -> Vec<WakeReason> {
        hq.wakes()
            .unwrap()
            .into_iter()
            .filter(|w| w.member == member && w.task == Some(task))
            .map(|w| w.reason)
            .collect()
    };
    // The first quiet window: the tester is woken, comes, and ends without a verdict.
    later(&clock, STRANDED_AFTER_MS + 1_000);
    all_over();
    assert_eq!(reasons(dee), [WakeReason::ReviewRequested]);
    assert!(reasons(lead).is_empty(), "the lead is not asked yet");
    // The second: it was nudged once and nobody came back to it, so its lead decides it. The
    // task is still in review, where the tester can pass it and the lead accept it.
    later(&clock, STRANDED_AFTER_MS + 1_000);
    all_over();
    assert_eq!(status(), TaskStatus::InReview, "never blocked");
    assert_eq!(reasons(lead), [WakeReason::Decide]);
    let thread = hq.store().thread(task).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.text.contains("in review") && p.text.contains("no verdict")),
        "the lead is told what is stuck: {thread:?}"
    );
    // However long it sits, it is never put out of the tester's reach.
    for _ in 0..4 {
        later(&clock, STRANDED_AFTER_MS + 1_000);
        all_over();
        assert_eq!(status(), TaskStatus::InReview);
    }
    assert!(
        hq.store()
            .task(task)
            .unwrap()
            .unwrap()
            .owner
            .is_some_and(|o| o == dev),
        "and it is still its member's"
    );
}

/// Found by checking HQ against the design it borrows (2026-10-17): a task waiting on one
/// that was dropped was released as if the wait were done, though the work it waited for
/// never happened. A dropped wait releases nothing; the waiting task's member is told why in
/// its thread and woken once, ten minutes on, through stranded recovery; still stuck after
/// that, it goes on the desk.
///
/// Given a task that waits on another,
/// when the one it waits on is dropped,
/// then it is not woken as if its wait were done, and its member is told the wait was dropped.
#[test]
fn a_dropped_wait_releases_nothing_and_its_member_is_told() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, first) = stranded(&clock);
    let goal = hq.store().task(first).unwrap().unwrap().goal;
    let after = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Restore from the backup".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: vec![first],
            },
        )
        .unwrap();
    hq.drop_task(first).unwrap();
    hq.tick().unwrap();
    let wait_finished = |hq: &Hq<'_>| {
        hq.store()
            .wakes()
            .unwrap()
            .iter()
            .filter(|w| w.task == Some(after) && w.reason == WakeReason::WaitFinished)
            .count()
    };
    assert_eq!(
        wait_finished(&hq),
        0,
        "a dropped wait is not a finished one"
    );
    // Its member is told at once that the wait was dropped (ADR 0124), not ten minutes on.
    let told_at_once = hq
        .store()
        .wakes()
        .unwrap()
        .iter()
        .filter(|w| w.task == Some(after) && w.reason == WakeReason::WaitDropped)
        .count();
    assert_eq!(told_at_once, 1, "its member is woken once");
    let told = hq.store().thread(after).unwrap();
    assert!(
        told.iter().any(|p| p.text.contains("was dropped")),
        "the thread says why: {told:?}"
    );
    assert_eq!(wait_finished(&hq), 0);
}
