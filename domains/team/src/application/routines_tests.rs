//! How the team keeps working (ADR 0107), end to end over HQ's real store and the simulated
//! runner: routines hand out work as tasks, alarms join a routine's open task, check-backs
//! bring a member back to its task, a clock with nothing to do starts no turn, and a task that
//! keeps moving nothing backs off.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, EngineError, Fired, Settings};
use super::ports::{Clock, NewRoutine, NewTask, TeamStore};
use crate::domain::routines::{HOUR_MS, MINUTE_MS};
use crate::domain::{
    CheckBackStatus, GoalId, MemberId, Priority, Receipt, Refusal, Refused, Role, RoutineId, RunId,
    RunStatus, TaskId, TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

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
    fn at(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000; // 2026-10-16 12:00 UTC

struct Team {
    lead: MemberId,
    dev: MemberId,
    goal: GoalId,
}

fn office(clock: &Hands) -> (Hq<'_>, Team) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq
        .set_goal(
            "Keep the home lab healthy",
            "it runs the house",
            Author::Owner,
        )
        .unwrap();
    (hq, Team { lead, dev, goal })
}

fn check_the_lab(hq: &Hq<'_>, t: &Team, on_lab_trouble: bool) -> RoutineId {
    hq.add_routine(
        Author::Owner,
        NewRoutine {
            title: "Check the home lab".to_owned(),
            description: "Look at the home lab with homelab_status.".to_owned(),
            goal: t.goal,
            project: None,
            assignee: t.lead,
            every_ms: 6 * HOUR_MS,
            on_lab_trouble,
        },
    )
    .unwrap()
}

fn tasks(hq: &Hq<'_>) -> Vec<crate::domain::Task> {
    hq.store().tasks().unwrap()
}

fn kinds(hq: &Hq<'_>) -> Vec<String> {
    hq.store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

fn routine(hq: &Hq<'_>, id: RoutineId) -> crate::domain::Routine {
    hq.store()
        .routines()
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
}

/// The turn running on a task now.
fn running_on(hq: &Hq<'_>, task: TaskId) -> RunId {
    hq.store()
        .runs()
        .unwrap()
        .into_iter()
        .find(|r| r.task == Some(task) && r.status == RunStatus::Running)
        .expect("a turn runs on the task")
        .id
}

#[test]
fn a_routine_hands_its_member_a_task_on_its_rhythm_and_wakes_them() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, false);
    assert_eq!(
        routine(&hq, id).next_due_at,
        NOON + 6 * HOUR_MS,
        "a new routine comes round after one period"
    );
    assert_eq!(hq.tick().unwrap().routines, 0);
    assert!(tasks(&hq).is_empty());

    clock.forward(6 * HOUR_MS);
    let report = hq.tick().unwrap();
    assert_eq!(report.routines, 1);
    assert_eq!(report.started, 1, "the assignment wakes the lead at once");
    let made = &tasks(&hq)[0];
    assert_eq!(made.title, "Check the home lab");
    assert_eq!(
        (made.goal, made.assignee, made.status),
        (t.goal, Some(t.lead), TaskStatus::Todo)
    );
    assert!(
        made.description
            .contains("Look at the home lab with homelab_status.")
    );
    assert!(
        made.description.contains("It comes round every 6 hours."),
        "{}",
        made.description
    );
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!(
        (wake.member, wake.task, wake.reason),
        (t.lead, Some(made.id), WakeReason::Assigned)
    );
    let r = routine(&hq, id);
    assert_eq!(r.last_task, Some(made.id));
    assert_eq!(r.last_fired_at, Some(clock.at()));
    assert!(kinds(&hq).contains(&"routine.fired".to_owned()));
}

#[test]
fn a_routine_has_one_open_task_and_a_late_tick_fires_once() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, false);
    clock.forward(20 * HOUR_MS); // late: three firings missed
    assert_eq!(hq.tick().unwrap().routines, 1);
    assert_eq!(tasks(&hq).len(), 1, "fired once, not caught up");
    assert_eq!(
        routine(&hq, id).next_due_at,
        clock.at() + 6 * HOUR_MS,
        "counted on from now"
    );
    let first = tasks(&hq)[0].id;
    hq.runner().finish(running_on(&hq, first), true);
    assert_eq!(hq.tick().unwrap().routines, 0);

    // Its task is still open: the next firing is skipped, and says so.
    clock.forward(6 * HOUR_MS);
    hq.tick().unwrap();
    assert_eq!(tasks(&hq).len(), 1);
    assert!(kinds(&hq).contains(&"routine.skipped".to_owned()));
    assert_eq!(routine(&hq, id).next_due_at, clock.at() + 6 * HOUR_MS);

    // The lead closes its check; the next firing hands out a new one.
    hq.wake(t.lead, Some(first), WakeReason::Mentioned).unwrap();
    hq.tick().unwrap();
    let run = running_on(&hq, first);
    hq.claim_in(run, first).unwrap();
    hq.set_status_in(run, first, TaskStatus::Done, "all containers healthy")
        .unwrap();
    assert_eq!(
        hq.store().task(first).unwrap().unwrap().status,
        TaskStatus::Done
    );
    clock.forward(6 * HOUR_MS);
    hq.tick().unwrap();
    assert_eq!(tasks(&hq).len(), 2);
    assert_eq!(routine(&hq, id).last_task, Some(tasks(&hq)[1].id));
}

#[test]
fn only_a_routines_check_is_closed_by_its_member_and_only_once_taken() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, false);
    let Fired::Created(check) = hq.fire_routine(id).unwrap() else {
        panic!("a task");
    };
    hq.tick().unwrap();
    let run = running_on(&hq, check);
    assert_eq!(
        hq.set_status_in(run, check, TaskStatus::Done, "fine"),
        Err(EngineError::Refused(Refusal::NotYours)),
        "claimed first"
    );
    hq.claim_in(run, check).unwrap();
    hq.set_status_in(run, check, TaskStatus::Done, "fine")
        .unwrap();
    // Ordinary work is still done only by the gate and a review.
    let work = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                project: None,
                parent: None,
                title: "Add a dashboard".to_owned(),
                description: String::new(),
                assignee: Some(t.dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let run = running_on(&hq, work);
    hq.claim_in(run, work).unwrap();
    assert_eq!(
        hq.set_status_in(run, work, TaskStatus::Done, "done"),
        Err(EngineError::Refused(Refusal::OnlyTheGate))
    );
}

#[test]
fn an_alarm_hands_out_a_task_or_joins_the_open_one_and_wakes_its_member() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    assert_eq!(
        hq.alarm("sonarr became unhealthy at 14:02").unwrap(),
        None,
        "no routine takes the home lab's alarms"
    );
    let quiet = check_the_lab(&hq, &t, true);
    hq.switch_routine(quiet, false).unwrap();
    let id = check_the_lab(&hq, &t, true);
    let (took, fired) = hq
        .alarm("sonarr became unhealthy at 14:02")
        .unwrap()
        .unwrap();
    assert_eq!(took, id, "the first routine that is on takes it");
    let Fired::Created(task) = fired else {
        panic!("a new task: {fired:?}");
    };
    let made = hq.store().task(task).unwrap().unwrap();
    assert!(
        made.description
            .contains("Endora raised an alarm: sonarr became unhealthy at 14:02."),
        "{}",
        made.description
    );
    assert_eq!(made.priority, Priority::High);

    // The lead is working it; a second alarm is said in its thread, and joins the turn.
    hq.tick().unwrap();
    let (_, fired) = hq.alarm("radarr became exited at 14:05").unwrap().unwrap();
    assert_eq!(fired, Fired::Posted(task));
    assert_eq!(tasks(&hq).len(), 1, "still one task");
    let said = hq.store().thread(task).unwrap().pop().unwrap();
    assert_eq!(
        (said.author.as_str(), said.text.as_str()),
        ("endora", "Alarm: radarr became exited at 14:05")
    );
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!(
        (wake.member, wake.reason, wake.receipt),
        (t.lead, WakeReason::Alarm, Receipt::Folded)
    );
    assert!(kinds(&hq).contains(&"routine.posted".to_owned()));
}

#[test]
fn coming_back_up_is_said_in_the_alarms_open_task_and_wakes_its_owner() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    assert_eq!(
        hq.recovered("deluge is back up at 13:43").unwrap(),
        None,
        "no routine takes the home lab's news"
    );
    check_the_lab(&hq, &t, true);
    assert_eq!(
        hq.recovered("deluge is back up at 13:43").unwrap(),
        None,
        "no alarm's task is open: there is nothing to tell"
    );
    assert!(
        tasks(&hq).is_empty(),
        "news of a recovery hands out no work"
    );
    // The shape of 2026-10-17: three containers down in a minute, up again the next.
    for name in ["deluge", "nzbget", "prowlarr"] {
        hq.alarm(&format!("{name} became exited at 13:42")).unwrap();
    }
    let task = tasks(&hq)[0].id;
    for name in ["deluge", "nzbget", "prowlarr"] {
        assert_eq!(
            hq.recovered(&format!("{name} is back up at 13:43"))
                .unwrap(),
            Some(task)
        );
    }
    assert_eq!(tasks(&hq).len(), 1, "still one task");
    let said = hq.store().thread(task).unwrap().pop().unwrap();
    assert_eq!(
        (said.author.as_str(), said.text.as_str()),
        ("endora", "Recovered: prowlarr is back up at 13:43")
    );
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!((wake.member, wake.reason), (t.lead, WakeReason::Alarm));
    assert!(kinds(&hq).contains(&"routine.posted".to_owned()));
    // Its owner, woken, reads it beside the alarms in its thread.
    hq.tick().unwrap();
    let told = hq.runner().started().last().unwrap().told.clone();
    assert!(
        told.contains("endora: Recovered: deluge is back up at 13:43"),
        "{told}"
    );
    // HQ's tools are not all it has: live, a lead read that line as all it had, and said it
    // could not read the home lab.
    assert!(
        told.contains(
            "Your tools at HQ: my_turn, claim, post, set_status, create_task, ask_owner, \
             check_back, run_gate, request_action, propose_project. Endora's own tools on the home lab \
             (homelab_status and the like) come beside them: what your tool list offers, you \
             have.\n"
        ),
        "{told}"
    );
}

#[test]
fn a_firing_goes_through_the_kill_switch_a_pause_and_its_goal() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, true);
    hq.stop_all("testing").unwrap();
    assert!(matches!(hq.fire_routine(id).unwrap(), Fired::Skipped(_)));
    assert!(matches!(
        hq.alarm("sonarr became unhealthy at 14:02").unwrap(),
        Some((_, Fired::Skipped(_)))
    ));
    hq.resume().unwrap();
    hq.pause(t.lead, true).unwrap();
    let Fired::Skipped(why) = hq.fire_routine(id).unwrap() else {
        panic!("skipped");
    };
    assert!(why.contains("paused"), "{why}");
    hq.pause(t.lead, false).unwrap();
    hq.retire_goal(t.goal, "done with it").unwrap();
    let Fired::Skipped(why) = hq.fire_routine(id).unwrap() else {
        panic!("skipped");
    };
    assert!(why.contains("goal"), "{why}");
    assert!(tasks(&hq).is_empty(), "nothing was handed out");
    assert_eq!(routine(&hq, id).last_fired_at, None);
}

#[test]
fn a_routine_switched_off_waits_and_switched_on_counts_from_then() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, false);
    hq.switch_routine(id, false).unwrap();
    clock.forward(7 * HOUR_MS);
    assert_eq!(hq.tick().unwrap().routines, 0);
    hq.switch_routine(id, true).unwrap();
    assert_eq!(routine(&hq, id).next_due_at, clock.at() + 6 * HOUR_MS);
    assert_eq!(
        hq.tick().unwrap().routines,
        0,
        "the missed firing is skipped"
    );
    assert!(
        hq.switch_routine(RoutineId(99), true).is_err(),
        "no such routine"
    );
}

#[test]
fn a_routine_keeps_a_sensible_rhythm_and_a_real_goal_and_member() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let new = |every_ms, goal, assignee| NewRoutine {
        title: "Check".to_owned(),
        description: String::new(),
        goal,
        project: None,
        assignee,
        every_ms,
        on_lab_trouble: false,
    };
    assert_eq!(
        hq.add_routine(Author::Owner, new(MINUTE_MS, t.goal, t.lead)),
        Err(EngineError::Refused(Refusal::Rhythm))
    );
    assert!(
        hq.add_routine(Author::Owner, new(HOUR_MS, GoalId(9), t.lead))
            .is_err()
    );
    assert!(
        hq.add_routine(Author::Owner, new(HOUR_MS, t.goal, MemberId(9)))
            .is_err()
    );
    assert!(hq.store().routines().unwrap().is_empty());
}

#[test]
fn the_snapshot_shows_each_routine_its_rhythm_and_when_it_last_fired() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, true);
    hq.fire_routine(id).unwrap();
    let snap = hq.snapshot().unwrap();
    let shown = &snap["routines"][0];
    assert_eq!(shown["id"], id.0);
    assert_eq!(shown["title"], "Check the home lab");
    assert_eq!(shown["rhythm"], "every 6 hours");
    assert_eq!(shown["assignee"], t.lead.0);
    assert_eq!(shown["enabled"], true);
    assert_eq!(shown["on_lab_trouble"], true);
    assert_eq!(shown["last_fired_at"], NOON);
    assert_eq!(shown["last_task_open"], true);
    assert_eq!(snap["tasks"][0]["routine"], id.0);
}

#[test]
fn a_check_back_brings_the_member_back_to_its_task_and_says_why() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, false);
    let Fired::Created(task) = hq.fire_routine(id).unwrap() else {
        panic!("a task");
    };
    hq.tick().unwrap();
    let run = running_on(&hq, task);
    let due = hq
        .check_back(run, 1, "see whether sonarr stayed up")
        .unwrap();
    assert_eq!(due, NOON + 5 * MINUTE_MS, "five minutes at the soonest");
    hq.runner().finish(run, true);
    hq.tick().unwrap();
    clock.forward(4 * MINUTE_MS);
    assert_eq!(hq.tick().unwrap().check_backs, 0);
    clock.forward(MINUTE_MS);
    let report = hq.tick().unwrap();
    assert_eq!(report.check_backs, 1);
    assert_eq!(report.started, 1);
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!(
        (wake.member, wake.task, wake.reason),
        (t.lead, Some(task), WakeReason::CheckBack)
    );
    let told = hq.runner().started().last().unwrap().told.clone();
    assert!(
        told.contains("You said you would check back: see whether sonarr stayed up"),
        "{told}"
    );
    assert!(told.contains("check_back"), "the tool is listed: {told}");
    assert_eq!(
        hq.store().check_backs().unwrap()[0].status,
        CheckBackStatus::Fired
    );
    assert!(kinds(&hq).contains(&"checkback.fired".to_owned()));
}

#[test]
fn a_check_back_waits_at_most_twelve_hours_replaces_its_own_and_not_for_finished_work() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let id = check_the_lab(&hq, &t, false);
    let Fired::Created(task) = hq.fire_routine(id).unwrap() else {
        panic!("a task");
    };
    hq.tick().unwrap();
    let run = running_on(&hq, task);
    let due = hq.check_back(run, 10_000, "tomorrow").unwrap();
    assert_eq!(due, NOON + 720 * MINUTE_MS);
    hq.check_back(run, 30, "sooner").unwrap();
    let pending: Vec<_> = hq
        .store()
        .check_backs()
        .unwrap()
        .into_iter()
        .filter(|c| c.status == CheckBackStatus::Pending)
        .collect();
    assert_eq!(pending.len(), 1, "the newer replaced the older");
    assert_eq!(pending[0].why, "sooner");
    assert!(
        hq.check_back(run, 30, "  ").is_err(),
        "a check-back says why"
    );

    // The task finishes first: the check-back wakes nobody.
    hq.drop_task(task).unwrap();
    clock.forward(31 * MINUTE_MS);
    hq.tick().unwrap();
    assert!(
        hq.wakes()
            .unwrap()
            .iter()
            .all(|w| w.reason != WakeReason::CheckBack)
    );
    let all = hq.store().check_backs().unwrap();
    assert_eq!(all.last().unwrap().status, CheckBackStatus::Dropped);
}

#[test]
fn a_timer_with_nothing_to_do_starts_no_turn() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    hq.store()
        .add_timer(t.lead, WakeReason::LeadReview, NOON, HOUR_MS)
        .unwrap();
    hq.tick().unwrap();
    let wake = hq.wakes().unwrap().pop().unwrap();
    assert_eq!(wake.receipt, Receipt::Refused(Refused::NothingToDo));
    assert!(hq.store().runs().unwrap().is_empty(), "no turn");

    // With an open task of its own, the look round goes ahead.
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: None,
            parent: None,
            title: "Plan the week".to_owned(),
            description: String::new(),
            assignee: Some(t.lead),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    clock.forward(HOUR_MS);
    hq.tick().unwrap();
    let looked = hq
        .wakes()
        .unwrap()
        .into_iter()
        .rfind(|w| w.reason == WakeReason::LeadReview)
        .unwrap();
    assert_ne!(looked.receipt, Receipt::Refused(Refused::NothingToDo));
}

/// Takes one turn on `task` for `member` that moves nothing, from `reason`.
fn empty_turn(hq: &Hq<'_>, member: MemberId, task: TaskId, reason: WakeReason) {
    hq.wake(member, Some(task), reason).unwrap();
    hq.tick().unwrap();
    let run = running_on(hq, task);
    hq.runner().finish(run, true);
    hq.tick().unwrap();
}

#[test]
fn a_task_that_moves_nothing_twice_backs_off_its_stale_wakes_but_never_news() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                project: None,
                parent: None,
                title: "Look at the backups".to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    empty_turn(&hq, t.dev, task, WakeReason::Assigned);
    empty_turn(&hq, t.dev, task, WakeReason::Recovery);

    // Two empty turns: a nudge waits two minutes.
    assert_eq!(
        hq.wake(t.dev, Some(task), WakeReason::Recovery).unwrap(),
        Receipt::Queued
    );
    let waiting = hq.store().runs().unwrap().pop().unwrap();
    assert_eq!(waiting.status, RunStatus::RetryWaiting);
    assert_eq!(waiting.retry_at, Some(clock.at() + 2 * MINUTE_MS));
    assert_eq!(hq.tick().unwrap().started, 0);
    clock.forward(2 * MINUTE_MS);
    assert_eq!(hq.tick().unwrap().started, 1);
    let run = running_on(&hq, task);
    hq.runner().finish(run, true);
    hq.tick().unwrap();

    // Three: four minutes. Then someone writes to it: it wakes at once.
    hq.wake(t.dev, Some(task), WakeReason::Recovery).unwrap();
    let waiting = hq.store().runs().unwrap().pop().unwrap();
    assert_eq!(waiting.retry_at, Some(clock.at() + 4 * MINUTE_MS));
    assert_eq!(
        hq.wake(t.dev, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Folded
    );
    assert_eq!(hq.tick().unwrap().started, 1, "news is never kept waiting");
}

/// Takes one turn on `task` for `member` as the real worker does when the model achieves
/// nothing: it claims the task before the model's first round, and posts the model's last
/// word with what the turn did. Neither is progress once the task is the member's.
fn workers_empty_turn(hq: &Hq<'_>, member: MemberId, task: TaskId, reason: WakeReason) {
    hq.wake(member, Some(task), reason).unwrap();
    hq.tick().unwrap();
    let id = running_on(hq, task);
    let run = hq.store().run(id).unwrap().unwrap();
    let me = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == member)
        .unwrap();
    let args = serde_json::json!({ "task": task.0 });
    super::protocol::call(hq, &run, &me, "claim", &args).unwrap();
    let last = serde_json::json!({
        "task": task.0,
        "text": "Nothing to add.\n\nThis turn made no tool calls.",
        "last_word": true,
    });
    super::protocol::call(hq, &run, &me, "post", &last).unwrap();
    hq.runner().finish(id, true);
    hq.tick().unwrap();
}

/// Found by checking HQ against the design it borrows (2026-10-17): the worker claims its task
/// before every turn and posts the model's last word after it, and both marked the turn as
/// progress, so the empty-turn back-off never once held a real turn back.
///
/// Given a task the member already owns,
/// when two turns only re-claim it and leave their last word,
/// then neither moved it forward, and the next stale wake backs off.
#[test]
fn turns_that_only_reclaim_and_leave_a_last_word_move_nothing() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, t) = office(&clock);
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                project: None,
                parent: None,
                title: "Look at the backups".to_owned(),
                description: String::new(),
                assignee: Some(t.dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    // The first turn takes the task: a real change, and progress.
    workers_empty_turn(&hq, t.dev, task, WakeReason::Assigned);
    let ended = |hq: &Hq<'_>| {
        hq.store()
            .runs()
            .unwrap()
            .into_iter()
            .filter(|r| r.task == Some(task) && r.status == RunStatus::Succeeded)
            .collect::<Vec<_>>()
    };
    let first = ended(&hq).pop().unwrap();
    assert!(first.moved_forward, "taking the task is progress");
    // Two more that only re-claim it and say their last word: not progress.
    workers_empty_turn(&hq, t.dev, task, WakeReason::Recovery);
    workers_empty_turn(&hq, t.dev, task, WakeReason::Recovery);
    let turns = ended(&hq);
    assert_eq!(turns.len(), 3);
    assert!(
        turns[1..].iter().all(|r| !r.moved_forward),
        "re-claiming and a last word are not progress"
    );
    assert_eq!(
        hq.wake(t.dev, Some(task), WakeReason::Recovery).unwrap(),
        Receipt::Queued
    );
    let waiting = hq.store().runs().unwrap().pop().unwrap();
    assert_eq!(
        waiting.status,
        RunStatus::RetryWaiting,
        "the back-off holds it"
    );
    assert_eq!(waiting.retry_at, Some(clock.at() + 2 * MINUTE_MS));
}
