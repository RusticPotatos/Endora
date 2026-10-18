//! The guards (ADR 0124), through the engine: the stopped diagnosis that decides what climbs
//! the ladder, receipted recovery wakes, and stuck desk items that close themselves.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, STRANDED_AFTER_MS, Settings};
use super::guard_ports::GuardStore;
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use crate::domain::{MemberId, Priority, Role, TaskId, TaskStatus, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

pub(super) struct Hands(pub(super) AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(super) type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

pub(super) const NOON: i64 = 1_792_166_400_000;

pub(super) fn later(clock: &Hands, ms: i64) {
    clock.0.fetch_add(ms, Ordering::SeqCst);
}

/// A team (with a lead, unless `lead` is false) and Linus's task, handed to him by the owner.
pub(super) fn team(clock: &Hands, lead: bool) -> (Hq<'_>, MemberId, TaskId) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    if lead {
        hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    }
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
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
    (hq, dev, task)
}

/// The member's turn that the tick starts next.
pub(super) fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn started")
}

fn wakes_for(hq: &Hq<'_>, task: TaskId, reason: WakeReason) -> usize {
    hq.store()
        .wakes()
        .unwrap()
        .iter()
        .filter(|w| w.task == Some(task) && w.reason == reason)
        .count()
}

fn climbed(hq: &Hq<'_>) -> usize {
    hq.events_after(0)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "stuck.climbed")
        .count()
}

/// Linus takes his task, marks it blocked on what he named, and ends his turn; with a
/// check-back if `coming_back`.
fn blocked(hq: &Hq<'_>, dev: MemberId, task: TaskId, coming_back: bool) {
    let t = turn(hq, dev);
    hq.claim_in(t.run, task).unwrap();
    hq.set_status_in(
        t.run,
        task,
        TaskStatus::Blocked,
        "the indexer is down until its update lands tonight",
    )
    .unwrap();
    if coming_back {
        hq.check_back(t.run, 120, "see whether the indexer is back")
            .unwrap();
    }
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
}

/// Held is stopped on purpose: blocked with a named reason and a check-back still to come. It
/// is neither nudged nor climbed while that wake is live.
#[test]
fn blocked_work_with_a_reason_and_a_live_check_back_is_held_not_stranded() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, true);
    blocked(&hq, dev, task, true);
    for _ in 0..3 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
    }
    assert_eq!(wakes_for(&hq, task, WakeReason::AssignmentRecovery), 0);
    assert_eq!(climbed(&hq), 0, "held work does not climb the ladder");
}

/// The same task with no check-back to bring it back is parked: nudged, then up the ladder.
#[test]
fn blocked_work_with_nothing_to_bring_it_back_is_parked_and_climbs() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, true);
    blocked(&hq, dev, task, false);
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(wakes_for(&hq, task, WakeReason::AssignmentRecovery), 1);
    let nudge = turn(&hq, dev);
    hq.runner().finish(nudge.run, true);
    hq.tick().unwrap();
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(climbed(&hq), 1, "parked work climbs");
}

/// A turn that failed for good leaves HQ unable to tell whether its member stopped: the
/// area's lead looks once, and the member is not nudged as if it had dropped the work.
#[test]
fn work_whose_last_turn_failed_is_indeterminate_and_its_lead_looks_once() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, true);
    for _ in 0..3 {
        let t = turn(&hq, dev);
        hq.runner().finish(t.run, false);
        hq.tick().unwrap();
        later(&clock, 61_000);
    }
    assert!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .any(|r| r.task == Some(task) && r.status == crate::domain::RunStatus::Failed),
        "failed for good"
    );
    for _ in 0..3 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
    }
    assert_eq!(wakes_for(&hq, task, WakeReason::AssignmentRecovery), 0);
    assert_eq!(wakes_for(&hq, task, WakeReason::Guard), 1, "one look");
    assert_eq!(climbed(&hq), 0, "indeterminate never climbs");
    let thread = hq.store().thread(task).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.author == "endora" && p.text.contains("cannot tell")),
        "{thread:?}"
    );
}

/// A recovery wake is recorded with what it saw before it is sent; nothing new on the task
/// means it is never sent again, whatever reset the task's nudge.
#[test]
fn the_same_recovery_wake_is_never_sent_twice_while_nothing_changed() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, true);
    blocked(&hq, dev, task, false);
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(wakes_for(&hq, task, WakeReason::AssignmentRecovery), 1);
    // Its turn ends having done nothing, and something resets the nudge.
    let nudge = turn(&hq, dev);
    hq.runner().finish(nudge.run, true);
    hq.tick().unwrap();
    hq.store().set_nudged(task, false).unwrap();
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(
        wakes_for(&hq, task, WakeReason::AssignmentRecovery),
        1,
        "the same nudge is not sent again"
    );
    // Something new (the owner writes) makes a nudge worth sending again.
    hq.post(Author::Owner, task, "The indexer is back.")
        .unwrap();
    let woke = turn(&hq, dev);
    hq.runner().finish(woke.run, true);
    hq.tick().unwrap();
    hq.store().set_nudged(task, false).unwrap();
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    assert_eq!(wakes_for(&hq, task, WakeReason::AssignmentRecovery), 2);
}

/// With no lead to climb to, stuck work goes on the owner's desk once, and the item closes
/// itself when its member is back on it.
#[test]
fn a_stuck_desk_item_is_put_up_once_and_closes_itself_when_the_work_moves() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    blocked(&hq, dev, task, false);
    for _ in 0..2 {
        later(&clock, STRANDED_AFTER_MS + 1);
        hq.tick().unwrap();
        if let Some(t) = hq.runner().started().into_iter().rfind(|c| c.member == dev) {
            hq.runner().finish(t.run, true);
        }
        hq.tick().unwrap();
    }
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    let stuck = |status: &str| {
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .filter(|d| d.kind == "stuck" && d.task == Some(task) && d.status == status)
            .count()
    };
    assert_eq!(stuck("waiting"), 1);
    assert!(
        hq.store()
            .findings()
            .unwrap()
            .iter()
            .any(|f| f.key == format!("stuck:{}", task.0) && f.cleared_at.is_none()),
        "a stuck finding is open"
    );
    // The owner writes; Linus is back on it.
    hq.post(Author::Owner, task, "Try the indexer again.")
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(stuck("waiting"), 0, "closed itself");
    assert_eq!(stuck("cleared"), 1);
}

// ---- closure reasons ------------------------------------------------------------------------

/// A task that waited on work that was dropped stays blocked (what it waited for will not
/// happen), and its member is told at once why, and what carries the work on.
#[test]
fn work_waiting_on_a_dropped_task_stays_blocked_and_its_member_is_told_why() {
    use crate::domain::closure::{Closure, DropWhy};
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, first) = team(&clock, true);
    let goal = hq.store().task(first).unwrap().unwrap().goal;
    let new = |title: &str, waits_on: Vec<TaskId>| {
        hq.add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: title.to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on,
            },
        )
        .unwrap()
    };
    let after = new("Restore from the backup", vec![first]);
    let successor = new("Clear the downloads another way", Vec::new());
    hq.drop_task_because(first, Closure::Dropped(DropWhy::Obsolete), Some(successor))
        .unwrap();
    let closed = hq.store().closure(first).unwrap().expect("why it closed");
    assert_eq!(
        (closed.closure.name(), closed.successor, closed.by.as_str()),
        ("obsolete", Some(successor), "owner")
    );
    let waiting = hq.store().task(after).unwrap().unwrap();
    assert_eq!(
        waiting.status,
        TaskStatus::Blocked,
        "the blocker is not resolved"
    );
    let told = hq.store().thread(after).unwrap();
    assert!(
        told.iter().any(|p| p.author == "endora"
            && p.text.contains("was dropped: what it was for is gone")
            && p.text.contains(&format!("#{successor}"))),
        "{told:?}"
    );
    assert_eq!(wakes_for(&hq, after, WakeReason::WaitDropped), 1);
    assert_eq!(wakes_for(&hq, after, WakeReason::WaitFinished), 0);
    // The owner's plain drop, with no reason given, is work that will not be done.
    hq.drop_task(successor).unwrap();
    assert_eq!(
        hq.store()
            .closure(successor)
            .unwrap()
            .map(|c| c.closure.name()),
        Some("wont_do"),
        "the owner's plain drop"
    );
}
