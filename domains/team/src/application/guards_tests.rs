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
    // It is open sub-tasks that count: with one finished there is room for one more.
    hq.store()
        .set_task_status(TaskId(owners.0 + 2), TaskStatus::Done)
        .unwrap();
    use_tool(
        &hq,
        &t,
        "create_task",
        &serde_json::json!({ "title": "Part 5", "description": "a part" }),
    )
    .unwrap();
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
    assert!(
        hq.store().findings().unwrap().is_empty(),
        "it was not put with the answered one"
    );
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
