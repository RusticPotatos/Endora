//! The guards (ADR 0124), through the engine: the stopped diagnosis that decides what climbs
//! the ladder, receipted recovery wakes, and stuck desk items that close themselves.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, STRANDED_AFTER_MS, Settings};
use super::guard_ports::GuardStore;
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use crate::domain::{Criterion, MemberId, Priority, Role, TaskId, TaskStatus, WakeReason};
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

pub(super) fn wakes_for(hq: &Hq<'_>, task: TaskId, reason: WakeReason) -> usize {
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
pub(super) fn blocked(hq: &Hq<'_>, dev: MemberId, task: TaskId, coming_back: bool) {
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

/// A task made to wait on work already done has nothing to wait for, and one made to wait on work
/// already dropped waits for what will not happen: neither is made.
#[test]
fn a_task_may_not_be_made_to_wait_on_work_that_is_already_done_or_dropped() {
    use super::engine::EngineError;
    use crate::domain::Refusal;
    use crate::domain::closure::{Closure, DropWhy};
    use crate::domain::guards::GuardRefusal;
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
    };
    let done = new("Read the logs", Vec::new()).unwrap();
    hq.store().set_task_status(done, TaskStatus::Done).unwrap();
    let dropped = new("Try the old way", Vec::new()).unwrap();
    hq.drop_task_because(dropped, Closure::Dropped(DropWhy::Obsolete), None)
        .unwrap();
    let tasks = hq.store().tasks().unwrap().len();
    for (wait, status) in [(done, TaskStatus::Done), (dropped, TaskStatus::Dropped)] {
        assert_eq!(
            new("Restore from the backup", vec![first, wait]),
            Err(EngineError::Refused(Refusal::Guard(
                GuardRefusal::WaitsOnFinished { task: wait, status }
            ))),
            "waiting on #{wait}, {}",
            status.name()
        );
    }
    assert_eq!(hq.store().tasks().unwrap().len(), tasks, "nothing was made");
    let said = GuardRefusal::WaitsOnFinished {
        task: dropped,
        status: TaskStatus::Dropped,
    }
    .to_string();
    assert!(
        said.contains("was dropped") && said.contains("carries its work on"),
        "{said}"
    );
    let said = GuardRefusal::WaitsOnFinished {
        task: done,
        status: TaskStatus::Done,
    }
    .to_string();
    assert!(said.contains("is done already"), "{said}");
    // Work still open is waited on, as ever; and a task that does not exist is not.
    let waiting = new("Restore from the backup", vec![first]).unwrap();
    assert_eq!(
        hq.store().task(waiting).unwrap().unwrap().waits_on,
        vec![first]
    );
    assert!(new("After nothing", vec![TaskId(9_999)]).is_err());
}

// ---- process health -------------------------------------------------------------------------

/// A lead on its own project's task, with no one above it (so no hand-up), and the task.
pub(super) fn project_work(clock: &Hands) -> (Hq<'_>, MemberId, TaskId) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let project = hq
        .add_project(goal, "Home lab upkeep", "", ada, None)
        .unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: "Plan the backups".to_owned(),
                description: String::new(),
                assignee: Some(ada),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let first = turn(&hq, ada);
    hq.claim_in(first.run, task).unwrap();
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();
    (hq, ada, task)
}

/// One turn of `member`'s on `task`, woken by a mention: it talks, or (with `act`) it also
/// restarts something and the node records the act.
pub(super) fn one_turn(hq: &Hq<'_>, member: MemberId, task: TaskId, act: bool) {
    hq.wake(member, Some(task), WakeReason::Mentioned).unwrap();
    let t = turn(hq, member);
    if act {
        hq.record_receipt(
            &t.token,
            &super::engine::Called {
                tool: "restart_container".to_owned(),
                args: "{\"name\":\"sonarr\"}".to_owned(),
                excerpt: "restarted".to_owned(),
                ok: true,
                act: true,
                confidential: false,
            },
        )
        .unwrap();
    }
    hq.post_in(
        t.run,
        task,
        "Still thinking about how the backups should go.",
    )
    .unwrap();
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
}

pub(super) fn guards(hq: &Hq<'_>) -> serde_json::Value {
    hq.snapshot().unwrap()["guards"].clone()
}

/// Talking about work isn't work: twelve turns that only talk, against no product change, is
/// a finding, a notice on the owner's desk, and the project's lead woken on the task.
#[test]
fn a_project_that_only_talks_about_its_work_is_a_finding_for_its_lead() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = project_work(&clock);
    for _ in 0..12 {
        one_turn(&hq, ada, task, false);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    let ceremony = guards(&hq)["ceremony"][0].clone();
    assert_eq!(ceremony["over"], true, "{ceremony}");
    assert_eq!(
        ceremony["windows"][0]["said"],
        "12 coordination turns, 0 product changes in 24 h"
    );
    let notice: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "guard" && d.status == "waiting")
        .collect();
    assert_eq!(notice.len(), 1, "one notice");
    assert!(
        notice[0].text.starts_with("Talk about work (finding "),
        "{}",
        notice[0].text
    );
    assert_eq!(
        wakes_for(&hq, task, WakeReason::Guard),
        1,
        "the project's lead"
    );
    // Ada, its lead, reads it in her thread and judges it.
    let looked = turn(&hq, ada);
    let thread = hq.store().thread(task).unwrap();
    let said = thread
        .iter()
        .rfind(|p| p.author == "endora")
        .expect("HQ says it in the thread");
    assert!(
        said.text
            .starts_with("Talking about work isn't work (finding "),
        "{}",
        said.text
    );
    let (run, member) = hq.turn_of(&looked.token).unwrap().unwrap();
    let id = hq.store().findings().unwrap()[0].id;
    super::protocol::call(
        &hq,
        &run,
        &member,
        "judge_finding",
        &serde_json::json!({ "finding": id, "verdict": "not_enough" }),
    )
    .unwrap();
    assert_eq!(
        hq.store().findings().unwrap()[0].judged_by,
        "Ada",
        "the lead of its area judged it"
    );
    hq.runner().finish(looked.run, true);
    hq.tick().unwrap();
    // Looked at again, it is the same finding: no second notice.
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    assert_eq!(
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .filter(|d| d.kind == "guard")
            .count(),
        1
    );
    // The owner says it was real: the guard's precision is measured.
    hq.decide(notice[0].id, "real").unwrap();
    let precision = guards(&hq)["precision"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["guard"] == "ceremony")
        .cloned()
        .unwrap();
    assert_eq!(
        (precision["real"].clone(), precision["precision"].clone()),
        (1.into(), 100.into())
    );
}

/// The same talk beside enough real change (four acts to twelve turns of talk) is healthy.
#[test]
fn talk_beside_enough_product_change_is_healthy() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = project_work(&clock);
    for i in 0..16 {
        one_turn(&hq, ada, task, i % 4 == 0);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    let ceremony = guards(&hq)["ceremony"][0].clone();
    assert_eq!(ceremony["over"], false, "{ceremony}");
    assert!(hq.store().findings().unwrap().is_empty());
}

/// A lead on the owner's work outside any project, with no one above it: what "Give work" and a
/// routine make, and what the ceremony guard could not see (ADR 0133).
pub(super) fn loose_work(clock: &Hands) -> (Hq<'_>, MemberId, TaskId) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let task = loose_task(&hq, goal, "Plan the backups", ada, None);
    let first = turn(&hq, ada);
    hq.claim_in(first.run, task).unwrap();
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();
    (hq, ada, task)
}

/// A task in no project, handed to `to`: the owner's, or a sub-task of `parent` made by `to`.
pub(super) fn loose_task(
    hq: &Hq<'_>,
    goal: crate::domain::GoalId,
    title: &str,
    to: MemberId,
    parent: Option<TaskId>,
) -> TaskId {
    let by = if parent.is_some() {
        Author::Member(to)
    } else {
        Author::Owner
    };
    hq.add_task(
        by,
        NewTask {
            goal,
            project: None,
            parent,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

/// One turn of `member`'s on `task`, woken to decide: judgment, so on its Claude and paid for. It
/// only talks, and spends `cost`.
pub(super) fn paid_talk(hq: &Hq<'_>, member: MemberId, task: TaskId, cost: i64) {
    hq.wake(member, Some(task), WakeReason::Decide).unwrap();
    let t = turn(hq, member);
    hq.report_usage(t.run, cost).unwrap();
    hq.post_in(t.run, task, "Still weighing how it should go.")
        .unwrap();
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
}

/// The ceremony guard's notices on the owner's desk, waiting.
fn ceremony_notices(hq: &Hq<'_>) -> Vec<String> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "guard" && d.status == "waiting")
        .filter(|d| d.text.starts_with("Talk about work (finding "))
        .map(|d| d.text)
        .collect()
}

/// Live, 17 and 18 October 2026: one docs branch took 44 turns, 80 posts and $2.47, 28% of all the
/// spend, and the ceremony guard never saw it, because it counted only tasks inside a project and
/// the owner's work and a routine's are in none. Work outside a project is grouped by its task
/// tree, the unit its lead can act on, and the finding says what the talk cost.
#[test]
fn work_outside_a_project_that_only_talks_is_found_by_its_task_tree_and_says_what_it_cost() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = loose_work(&clock);
    for _ in 0..12 {
        paid_talk(&hq, ada, task, 50_000);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();

    let shown = guards(&hq)["ceremony"].clone();
    let tree = shown
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["title"]
                .as_str()
                .is_some_and(|t| t.contains("Plan the backups"))
        })
        .unwrap_or_else(|| panic!("the tree is on the card: {shown}"));
    assert_eq!(tree["over"], true, "{tree}");
    assert_eq!(
        tree["windows"][0]["said"],
        "12 coordination turns, 0 product changes in 24 h"
    );
    assert_eq!(tree["windows"][0]["spent"], 600_000);

    let found = hq.store().findings().unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].key, format!("ceremony:tree:{}", task.0));
    assert_eq!((found[0].task, found[0].project), (Some(task.0), None));
    let notices = ceremony_notices(&hq);
    assert_eq!(notices.len(), 1, "one notice: {notices:?}");
    assert!(
        notices[0].contains("12 coordination turns, 0 product changes in 24 h ($0.60 spent)"),
        "{}",
        notices[0]
    );
    assert_eq!(
        wakes_for(&hq, task, WakeReason::Guard),
        1,
        "the lead of its area is woken"
    );
    let said = hq.store().thread(task).unwrap();
    assert!(
        said.iter()
            .rfind(|p| p.author == "endora")
            .is_some_and(|p| {
                p.text
                    .starts_with("Talking about work isn't work (finding ")
                    && p.text.contains("this is in your area")
                    && p.text.contains("narrow its criteria (narrow_criteria)")
            }),
        "{said:?}"
    );
}

/// Each tree stands alone: seven turns of talk in two pieces of work is not fourteen in one, and a
/// sub-task's talk counts for the tree it hangs from.
#[test]
fn talk_in_two_separate_pieces_of_work_is_not_added_together_and_a_sub_task_counts_for_its_root() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, first) = loose_work(&clock);
    let goal = hq.store().task(first).unwrap().unwrap().goal;
    for _ in 0..7 {
        one_turn(&hq, ada, first, false);
    }
    let second = loose_task(&hq, goal, "Look at the NAS fans", ada, None);
    for _ in 0..7 {
        one_turn(&hq, ada, second, false);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    assert!(
        hq.store().findings().unwrap().is_empty(),
        "fourteen turns in two trees is seven in each"
    );
    // Five turns on a sub-task of the first make twelve in its tree.
    let part = loose_task(&hq, goal, "Draft the plan", ada, Some(first));
    for _ in 0..5 {
        one_turn(&hq, ada, part, false);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    let found = hq.store().findings().unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].key,
        format!("ceremony:tree:{}", first.0),
        "keyed by the root, not by the sub-task"
    );
    assert_eq!(
        found[0].task,
        Some(first.0),
        "the busiest task in the tree: seven turns against five"
    );
}

/// Work outside a project with no lead at work to look at it is still found, and says so on the
/// owner's digest; nobody is woken, because nobody is there to decide it.
#[test]
fn a_tree_with_no_lead_at_work_is_found_and_wakes_nobody() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, linus, task) = team(&clock, false);
    for _ in 0..12 {
        one_turn(&hq, linus, task, false);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    assert_eq!(ceremony_notices(&hq).len(), 1);
    assert_eq!(wakes_for(&hq, task, WakeReason::Guard), 0);
}

/// Work that is over has nothing left for its lead to decide: the finding clears at the next
/// look, and its card in the digest with it.
#[test]
fn a_tree_whose_work_is_over_has_its_finding_and_its_card_cleared() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = loose_work(&clock);
    for _ in 0..12 {
        one_turn(&hq, ada, task, false);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    assert_eq!(ceremony_notices(&hq).len(), 1);
    hq.drop_task(task).unwrap();
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    let found = hq.store().findings().unwrap();
    assert!(found[0].cleared_at.is_some(), "{found:?}");
    assert!(ceremony_notices(&hq).is_empty(), "its card went with it");
    assert_eq!(
        guards(&hq)["ceremony"],
        serde_json::json!([]),
        "and it is off the Guards card"
    );
}

/// A project's finding says what its talk cost too (ADR 0133).
#[test]
fn a_projects_finding_states_the_spend_in_each_window() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, ada, task) = project_work(&clock);
    for _ in 0..12 {
        paid_talk(&hq, ada, task, 50_000);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    let notices = ceremony_notices(&hq);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].contains("in 24 h ($0.60 spent)") && notices[0].contains("in 7 d ($0.60 spent)"),
        "{}",
        notices[0]
    );
}

fn outcomes(hq: &Hq<'_>) -> serde_json::Value {
    hq.snapshot().unwrap()["scorecard"]["outcomes"].clone()
}

/// Talk against work on the outcomes line is what the process-health look measured, with the
/// ceremony guard's own labels (ADR 0132): it agrees with the guard's card for the same window,
/// and says it has not been measured until the look has been made.
#[test]
fn talk_against_work_on_the_outcomes_is_the_ceremony_guards_own_count() {
    let clock = Hands(AtomicI64::new(NOON));
    let (unlooked, _, _) = team(&clock, true);
    assert_eq!(
        outcomes(&unlooked)["day"]["talk"],
        serde_json::Value::Null,
        "not measured before the first look"
    );
    assert_eq!(outcomes(&unlooked)["talk_at"], serde_json::Value::Null);
    let (hq, ada, task) = project_work(&clock);
    for i in 0..13 {
        one_turn(&hq, ada, task, i % 6 == 0);
    }
    later(&clock, crate::domain::guards::THRESHOLDS.look_every_ms);
    hq.tick().unwrap();
    let ceremony = guards(&hq)["ceremony"][0]["windows"].clone();
    let shown = outcomes(&hq);
    for (window, said) in [("day", &ceremony[0]), ("week", &ceremony[1])] {
        let talk = &shown[window]["talk"];
        assert_eq!(
            talk["coordination"], said["coordination"],
            "{window}: {talk}"
        );
        assert_eq!(talk["product"], said["product"], "{window}: {talk}");
    }
    let talk = &shown["day"]["talk"];
    assert_eq!(talk["product"], 3, "three of the thirteen turns acted");
    assert_eq!(talk["acts"], 3, "and each act counts once");
    let posts = hq
        .store()
        .thread(task)
        .unwrap()
        .iter()
        .filter(|p| p.author == "Ada")
        .count();
    assert_eq!(talk["posts"], posts, "a member's posts, not Endora's");
}

/// A member woken on a task again and again with nothing coming of it is not woken once more
/// for nothing: it is a finding, and the work is parked for its lead.
#[test]
fn a_member_woken_for_nothing_again_and_again_is_parked_not_woken() {
    let clock = Hands(AtomicI64::new(NOON));
    // No lead: nobody above Linus takes the work up after his third turn of nothing (ADR
    // 0114's hand-up), so he is the one woken again.
    let (hq, dev, task) = team(&clock, false);
    for _ in 0..3 {
        hq.wake(dev, Some(task), WakeReason::Mentioned).unwrap();
        let t = turn(&hq, dev);
        hq.runner().finish(t.run, true);
        hq.tick().unwrap();
    }
    assert_eq!(
        hq.wake(dev, Some(task), WakeReason::AssignmentRecovery)
            .unwrap(),
        crate::domain::Receipt::Refused(crate::domain::Refused::Redundant)
    );
    assert_eq!(
        hq.diagnosis(task).unwrap().map(|d| d.stopped),
        Some(crate::domain::guards::Stopped::Parked)
    );
    let found = hq.store().findings().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].guard, crate::domain::guards::Guard::RedundantWake);
    // News still wakes it; a turn that does something clears the finding.
    hq.post(Author::Owner, task, "The downloads are back.")
        .unwrap();
    let t = turn(&hq, dev);
    hq.claim_in(t.run, task).unwrap();
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
    assert!(hq.store().findings().unwrap()[0].cleared_at.is_some());
}

// ---- sub-task discipline and evidence that is enough -----------------------------------------

/// `member` uses a tool of the team protocol in its running turn `t`.
pub(super) fn use_tool(
    hq: &Hq<'_>,
    t: &RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&t.token).unwrap().expect("a running turn");
    super::protocol::call(hq, &run, &member, tool, args)
}

/// A member's sub-task of a member's task says which of its parent's criteria it serves; a task
/// with no criteria has none to name.
#[test]
fn a_sub_task_of_a_members_task_says_which_criterion_it_serves() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, linus_first) = team(&clock, true);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    // Linus's first task is put away, so his next turn is on what Ada hands him.
    hq.drop_task(linus_first).unwrap();
    let goal = hq.store().task(linus_first).unwrap().unwrap().goal;
    // The owner's task, with no criteria (work from before they were asked for), is split with
    // no `serves`: there is no criterion of it to name.
    let owners = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Keep the downloads moving".to_owned(),
                description: String::new(),
                assignee: Some(ada),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let a = turn(&hq, ada);
    assert_eq!(a.task, Some(owners));
    let made = use_tool(
        &hq,
        &a,
        "create_task",
        &serde_json::json!({ "title": "Find why the downloads stick", "description": "Why do they stick?",
            "assignee": "Linus",
            "criteria": [{ "text": "The cause is found" }, { "text": "The fix is written up" }] }),
    )
    .unwrap();
    let fix = TaskId(
        made.trim_start_matches("Created task ")
            .trim_end_matches('.')
            .parse()
            .unwrap(),
    );
    hq.runner().finish(a.run, true);
    hq.tick().unwrap();
    // Linus's sub-task of that (a member's task, with criteria) must say which it serves.
    let l = turn(&hq, dev);
    assert_eq!(l.task, Some(fix));
    let mine = |serves: Option<&str>| {
        use_tool(
            &hq,
            &l,
            "create_task",
            &serde_json::json!({ "title": "Read the queue", "description": "What is in it?", "serves": serves }),
        )
    };
    let refused = mine(None).unwrap_err();
    assert!(
        refused.contains(&format!("which of #{fix}'s criteria does this serve?"))
            && refused.contains("1. The cause is found; 2. The fix is written up"),
        "{refused}"
    );
    assert!(
        mine(Some("the moon base")).is_err(),
        "no criterion of the parent"
    );
    let made = mine(Some("1")).unwrap();
    let read = TaskId(
        made.trim_start_matches("Created task ")
            .trim_end_matches('.')
            .parse()
            .unwrap(),
    );
    let task = hq.store().task(read).unwrap().unwrap();
    assert_eq!(
        hq.serves_said(&task).unwrap(),
        format!("It serves #{fix}'s criterion 1: The cause is found. Keep to that.\n")
    );
}

/// The owner's task is split the way a member's is (ADR 0124, amended): each part says which
/// criterion of it that part serves. Live, the owner's "no literal passwords in role defaults"
/// went to a lead and split into three blocked sub-tasks, each defensible and none tied to what
/// was asked, because depth 0 (the owner's and a routine's tasks) was exempt.
#[test]
fn splitting_the_owners_task_says_which_criterion_each_part_serves() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _dev, linus_first) = team(&clock, true);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    hq.drop_task(linus_first).unwrap();
    let goal = hq.store().task(linus_first).unwrap().unwrap().goal;
    let ask = hq
        .add_task_with(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Remove literal passwords from the role defaults".to_owned(),
                description: String::new(),
                assignee: Some(ada),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
            &[
                Criterion::new("No role default holds a password", false),
                Criterion::new("Each role reads its secret from the vault", false),
            ],
        )
        .unwrap();
    let a = turn(&hq, ada);
    assert_eq!(a.task, Some(ask));
    let part = |title: &str, serves: Option<&str>| {
        use_tool(
            &hq,
            &a,
            "create_task",
            &serde_json::json!({ "title": title, "description": "a part", "assignee": "Linus",
                "criteria": [{ "text": "the part is done" }], "serves": serves }),
        )
    };
    let refused = part("Find the literal passwords", None).unwrap_err();
    assert!(
        refused.contains(&format!("which of #{ask}'s criteria does this serve?"))
            && refused.contains(
                "1. No role default holds a password; 2. Each role reads its secret from the vault"
            ),
        "{refused}"
    );
    assert!(
        part("Find the literal passwords", Some("the moon base")).is_err(),
        "no criterion of the task"
    );
    assert!(
        hq.store()
            .tasks()
            .unwrap()
            .iter()
            .all(|t| t.parent != Some(ask)),
        "nothing was made by a refusal"
    );
    part("Find the literal passwords", Some("1")).unwrap();
    // By its words too, case and spacing aside.
    part(
        "Move the secrets to the vault",
        Some("each role reads  its secret from the vault"),
    )
    .unwrap();
    let parts: Vec<_> = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.parent == Some(ask))
        .collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(
        hq.serves_said(&parts[0]).unwrap(),
        format!(
            "It serves #{ask}'s criterion 1: No role default holds a password. Keep to that.\n"
        )
    );
    assert!(
        hq.serves_said(&parts[1])
            .unwrap()
            .contains("criterion 2: Each role reads its secret from the vault")
    );
}

/// A routine's task has criteria now, and is split the same way (the home lab's, with its two
/// live ones). Work already open with none, and the plan HQ hands a project's lead, have no
/// criterion to name, so their parts need no `serves`: the one exception, because there is
/// nothing to serve.
#[test]
fn a_routines_task_is_split_the_same_way_and_a_task_with_no_criteria_has_none_to_serve() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, linus_first) = team(&clock, true);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    // Linus's task came from the owner before criteria were asked for: it has none.
    assert!(
        hq.store()
            .task(linus_first)
            .unwrap()
            .unwrap()
            .criteria
            .is_empty()
    );
    let l = turn(&hq, dev);
    assert_eq!(l.task, Some(linus_first));
    use_tool(
        &hq,
        &l,
        "create_task",
        &serde_json::json!({ "title": "Read the queue", "description": "What is in it?" }),
    )
    .expect("a task with no criteria has none to serve");
    hq.runner().finish(l.run, true);
    hq.tick().unwrap();

    // The home lab's routine: its task has the two live criteria, so a part names one.
    let goal = hq.store().task(linus_first).unwrap().unwrap().goal;
    hq.add_routine(
        Author::Endora,
        super::ports::NewRoutine {
            title: "Check the home lab".to_owned(),
            description: String::new(),
            goal,
            project: None,
            assignee: ada,
            every_ms: 6 * 60 * 60 * 1000,
            on_lab_trouble: true,
        },
    )
    .unwrap();
    hq.alarm("sonarr became unhealthy").unwrap();
    let a = turn(&hq, ada);
    let check = a.task.expect("the lead is on the routine's task");
    let part = |serves: Option<&str>| {
        use_tool(
            &hq,
            &a,
            "create_task",
            &serde_json::json!({ "title": "Look at sonarr", "description": "Why is it unhealthy?",
                "assignee": "Linus", "criteria": [{ "text": "the cause is in the thread" }],
                "serves": serves }),
        )
    };
    let refused = part(None).unwrap_err();
    assert!(
        refused.contains(&format!("which of #{check}'s criteria does this serve?"))
            && refused.contains("1. Every container that alarmed is running and healthy"),
        "{refused}"
    );
    part(Some("1")).unwrap();
}

/// No task holds more than five open sub-tasks, and work goes no deeper than three; the
/// refusals say the counts.
#[test]
fn a_task_holds_five_open_sub_tasks_and_work_goes_three_deep() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, owners) = team(&clock, true);
    let t = turn(&hq, dev);
    for i in 0..5 {
        use_tool(
            &hq,
            &t,
            "create_task",
            &serde_json::json!({ "title": format!("Part {i}"), "description": "a part" }),
        )
        .unwrap();
    }
    let refused = use_tool(
        &hq,
        &t,
        "create_task",
        &serde_json::json!({ "title": "Part 5", "description": "a part" }),
    )
    .unwrap_err();
    assert!(
        refused.contains(&format!(
            "#{owners} already has 5 open sub-tasks, and 5 is the most"
        )),
        "{refused}"
    );
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
    // Three deep below the owner's task, and no deeper.
    let goal = hq.store().task(owners).unwrap().unwrap().goal;
    let mut parent = TaskId(owners.0 + 1);
    for depth in 2..=3 {
        parent = hq
            .add_task(
                Author::Member(dev),
                NewTask {
                    goal,
                    project: None,
                    parent: Some(parent),
                    title: format!("Depth {depth}"),
                    description: String::new(),
                    assignee: Some(dev),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
    }
    assert_eq!(hq.store().task(parent).unwrap().unwrap().depth, 3);
    hq.wake(dev, Some(parent), WakeReason::Mentioned).unwrap();
    let deep = turn(&hq, dev);
    assert_eq!(deep.task, Some(parent));
    let refused = use_tool(
        &hq,
        &deep,
        "create_task",
        &serde_json::json!({ "title": "Depth 4", "description": "too deep" }),
    )
    .unwrap_err();
    assert!(
        refused.contains("would be 4 deep, and 3 is the deepest"),
        "{refused}"
    );
}

/// Enough evidence means submit: told so, then three turns that only check the same again
/// are talk only, and on the third HQ submits the work for its member.
#[test]
fn evidence_already_enough_is_submitted_by_hq_after_three_re_checks() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, _) = team(&clock, true);
    let goal = hq.store().goals().unwrap()[0].id;
    let task = hq
        .add_task_with(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Find why sonarr restarts".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
            &[crate::domain::Criterion::new("The cause is found", false)],
        )
        .unwrap();
    let read = |t: &RunContext| {
        hq.record_receipt(
            &t.token,
            &super::engine::Called {
                tool: "container_logs".to_owned(),
                args: "{\"name\":\"sonarr\"}".to_owned(),
                excerpt: "killed: out of memory".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap()
    };
    // Linus's turns on it, each started by the tick after the previous (the first by its
    // handing-on, the next by a mention).
    let mut turns = Vec::new();
    for i in 0..4 {
        if i > 0 {
            hq.wake(dev, Some(task), WakeReason::Mentioned).unwrap();
        }
        let t = turn(&hq, dev);
        if i == 0 {
            hq.claim_in(t.run, task).unwrap();
        }
        turns.push(t.clone());
        read(&t);
        hq.runner().finish(t.run, true);
        hq.tick().unwrap();
    }
    assert!(
        !turns[0].told.contains("already meets the criteria"),
        "no evidence before its first reading"
    );
    assert!(
        turns[1]
            .told
            .contains("Your evidence already meets the criteria: submit now"),
        "{}",
        turns[1].told
    );
    let labels: Vec<_> = turns
        .iter()
        .map(|t| hq.store().run(t.run).unwrap().unwrap().label)
        .collect();
    use crate::domain::Label;
    assert_eq!(
        labels,
        [
            Some(Label::Advanced),
            Some(Label::TalkOnly),
            Some(Label::TalkOnly),
            Some(Label::TalkOnly)
        ],
        "re-checking enough evidence is talk only"
    );
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::InReview,
        "HQ submitted it"
    );
    let thread = hq.store().thread(task).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.author == "endora" && p.text.contains("HQ submits it for Linus")),
        "{thread:?}"
    );
    assert!(
        !hq.store().cited(task).unwrap().is_empty(),
        "on that evidence"
    );
}

// ---- retract and sweep ----------------------------------------------------------------------

/// A claim the owner retracts is taken out, what else says it is swept for the owner to
/// forget at once, and a member writing it again is refused, saying when it was retracted.
#[test]
fn a_retracted_claim_is_swept_and_cannot_come_back() {
    use crate::domain::Ref;
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, true);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    let on = [Ref::Task(task)];
    let t = turn(&hq, dev);
    let wrong = hq
        .note_in(t.run, "Sonarr needs a restart every night.", &on)
        .unwrap();
    let built = hq
        .note_in(t.run, &format!("Per n{wrong}, restart it at 02:00."), &on)
        .unwrap();
    let other = hq
        .note_in(t.run, "Radarr's queue is empty on Sundays.", &on)
        .unwrap();
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
    hq.wake(ada, Some(task), WakeReason::Mentioned).unwrap();
    let a = turn(&hq, ada);
    let copied = hq
        .note_in(
            a.run,
            "As we found, sonarr needs a restart every night or the queue sticks",
            &on,
        )
        .unwrap();
    hq.runner().finish(a.run, true);
    hq.tick().unwrap();

    let said = hq.retract(&format!("note:{wrong}")).unwrap();
    assert_eq!(
        said["built_on_it"],
        serde_json::json!([format!("note:{built}")])
    );
    let swept: Vec<String> = said["sweep"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["target"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(swept, [format!("note:{copied}")], "only what says it again");
    assert!(hq.store().notes(dev).unwrap().iter().all(|n| n.id != wrong));
    let id = said["retraction"].as_i64().unwrap();
    assert_eq!(
        hq.forget_swept(id, &[format!("note:{copied}"), format!("note:{other}")])
            .unwrap(),
        1,
        "only what the sweep found"
    );
    assert!(hq.store().notes(ada).unwrap().is_empty());
    assert_eq!(hq.store().notes(dev).unwrap().len(), 2, "the rest stay");
    // Written again, in other words around it: refused, with the day it was retracted.
    hq.wake(dev, Some(task), WakeReason::Mentioned).unwrap();
    let again = turn(&hq, dev);
    let refused = hq
        .note_in(
            again.run,
            "Reminder: sonarr needs a restart every night (r9).",
            &on,
        )
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("this says again what was retracted on")
            && refused.contains(&format!("note:{wrong}")),
        "{refused}"
    );
    assert!(hq.retract("note:9999").is_err());
}

// ---- the same ask, once on the desk ---------------------------------------------------------

/// Linus's two tasks, and a question asked on each in its own turn: the desk items after.
fn two_asks(first: &str, second: &str) -> Vec<super::ports::DeskItem> {
    let clock = Hands(AtomicI64::new(NOON));
    {
        let (hq, dev, a) = team(&clock, false);
        let goal = hq.store().task(a).unwrap().unwrap().goal;
        let b = hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Check the other branch".to_owned(),
                    description: String::new(),
                    assignee: Some(dev),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        for (task, text) in [(a, first), (b, second)] {
            // Linus's running turn on the task: started by its handing-on, or a mention.
            let running = |hq: &Hq<'_>| {
                hq.tick().unwrap();
                hq.runner()
                    .started()
                    .into_iter()
                    .rfind(|c| c.task == Some(task) && hq.turn_of(&c.token).unwrap().is_some())
            };
            let t = match running(&hq) {
                Some(t) => t,
                None => {
                    hq.wake(dev, Some(task), WakeReason::Mentioned).unwrap();
                    running(&hq).expect("a turn on the task")
                }
            };
            hq.ask_owner(t.run, text).unwrap();
            hq.tick().unwrap();
        }
        let desk: Vec<_> = hq
            .store()
            .desk()
            .unwrap()
            .into_iter()
            .filter(|d| d.kind == "question" && d.status == "waiting")
            .collect();
        // One answer for one card reaches every task put with it.
        if desk.len() == 1 {
            hq.answer(desk[0].id, "Yes: install them on the dev node.")
                .unwrap();
            for task in [a, b] {
                assert!(
                    hq.store()
                        .thread(task)
                        .unwrap()
                        .iter()
                        .any(|p| p.author == "owner"
                            && p.text.contains("install them on the dev node")),
                    "#{task} hears the answer"
                );
                assert!(
                    hq.store()
                        .wakes()
                        .unwrap()
                        .iter()
                        .any(|w| w.task == Some(task)
                            && w.member == dev
                            && w.reason == WakeReason::ProposalDecided),
                    "#{task}'s asker is woken"
                );
            }
            let found = hq.store().findings().unwrap();
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].guard, crate::domain::guards::Guard::SameAsk);
        }
        desk
    }
}

/// Live, 18 October 2026: the same board decision twice, once a task. The same ask from a
/// second task is put with the first card, and the owner's one answer reaches both.
#[test]
fn the_same_ask_from_two_tasks_is_one_card_and_one_answer_reaches_both() {
    let desk = two_asks(
        "Install ansible-playbook, ansible-lint and yamllint on the dev node? The check exited 127.",
        "Install ansible-playbook, ansible-lint and yamllint on the dev node? Its check exited 127 too.",
    );
    assert_eq!(desk.len(), 1, "one card: {desk:?}");
}

#[test]
fn two_different_asks_are_two_cards() {
    let desk = two_asks(
        "Install ansible-playbook, ansible-lint and yamllint on the dev node? The check exited 127.",
        "May sonarr have 2 GB of memory? It was killed for memory three times.",
    );
    assert_eq!(desk.len(), 2, "{desk:?}");
}

// ---- pushing at the fence -------------------------------------------------------------------

fn refused_act(hq: &Hq<'_>, t: &RunContext, tool: &str) {
    hq.record_receipt(
        &t.token,
        &super::engine::Called {
            tool: tool.to_owned(),
            args: String::new(),
            excerpt: "refused: not in the contract".to_owned(),
            ok: false,
            act: true,
            confidential: false,
        },
    )
    .unwrap();
}

fn fence_notices(hq: &Hq<'_>) -> Vec<String> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "guard" && d.text.starts_with("Pushing at the fence"))
        .map(|d| d.text)
        .collect()
}

/// Refused acts again and again are a notice naming the tool and who tried; and each briefing
/// carries the goal's "not now".
#[test]
fn pushing_at_the_fence_is_a_notice_and_the_briefing_says_what_is_not_now() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, true);
    let goal = hq.store().task(task).unwrap().unwrap().goal;
    hq.set_strategy(
        goal,
        &crate::domain::areas::StrategyEdit {
            purpose: None,
            success: None,
            measures: None,
            bets: None,
            not_now: Some("moving the media stack off the NAS".to_owned()),
        },
    )
    .unwrap();
    let t = turn(&hq, dev);
    assert!(
        t.told
            .contains("Not now, for this goal: moving the media stack off the NAS."),
        "{}",
        t.told
    );
    refused_act(&hq, &t, "remove_container");
    refused_act(&hq, &t, "remove_container");
    assert!(fence_notices(&hq).is_empty(), "twice is not yet");
    refused_act(&hq, &t, "edit_compose");
    let notices = fence_notices(&hq);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].contains("Linus was refused 3 acts in one turn: remove_container, edit_compose"),
        "{}",
        notices[0]
    );
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
    // Another member refused the same act within the day: the team keeps trying it.
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    hq.wake(ada, Some(task), WakeReason::Mentioned).unwrap();
    let a = turn(&hq, ada);
    refused_act(&hq, &a, "remove_container");
    let notices = fence_notices(&hq);
    assert_eq!(notices.len(), 2, "{notices:?}");
    assert!(
        notices[1].contains("remove_container was refused for Linus, Ada within a day"),
        "{}",
        notices[1]
    );
}
