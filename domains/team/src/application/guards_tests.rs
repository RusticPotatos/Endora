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
    team_over(SqliteTeamStore::in_memory().unwrap(), clock, lead)
}

/// [`team`], over the team's `store`: a file, when a test is about HQ starting again.
pub(super) fn team_over(
    store: SqliteTeamStore,
    clock: &Hands,
    lead: bool,
) -> (Hq<'_>, MemberId, TaskId) {
    let hq = Engine::new(store, SimulatedRunner::new(4), clock, Settings::default());
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
    // Work already under way on what it waited for is blocked as well, not left running.
    let under_way = new("Check the restore", vec![first]);
    hq.store()
        .set_task_status(under_way, TaskStatus::Doing)
        .unwrap();
    let successor = new("Clear the downloads another way", Vec::new());
    hq.drop_task_because(first, Closure::Dropped(DropWhy::Obsolete), Some(successor))
        .unwrap();
    assert_eq!(
        hq.store().task(under_way).unwrap().unwrap().status,
        TaskStatus::Blocked,
        "work under way is blocked too"
    );
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

/// An ask the owner has already answered is not waiting, and does not take a new one in: the
/// same question asked after the answer is a new question, on its own card.
#[test]
fn an_ask_already_answered_does_not_take_the_same_ask_again_in() {
    let clock = Hands(AtomicI64::new(NOON));
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
    let text = "Install ansible-playbook, ansible-lint and yamllint on the dev node? The check exited 127.";
    let waiting = |hq: &Hq<'_>| -> Vec<super::ports::DeskItem> {
        hq.store()
            .desk()
            .unwrap()
            .into_iter()
            .filter(|d| d.kind == "question" && d.status == "waiting")
            .collect()
    };
    let ask_in = |task: TaskId| {
        let running = || {
            hq.tick().unwrap();
            hq.runner()
                .started()
                .into_iter()
                .rfind(|c| c.task == Some(task) && hq.turn_of(&c.token).unwrap().is_some())
        };
        let t = running().unwrap_or_else(|| {
            hq.wake(dev, Some(task), WakeReason::Mentioned).unwrap();
            running().expect("a turn on the task")
        });
        hq.ask_owner(t.run, text).unwrap();
        hq.tick().unwrap();
    };
    ask_in(a);
    let first = waiting(&hq);
    assert_eq!(first.len(), 1, "{first:?}");
    hq.answer(first[0].id, "Yes: install them.").unwrap();
    assert!(waiting(&hq).is_empty(), "answered");
    ask_in(b);
    let again = waiting(&hq);
    assert_eq!(again.len(), 1, "its own card: {again:?}");
    assert_ne!(again[0].id, first[0].id);
    assert_eq!(again[0].task, Some(b));
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
