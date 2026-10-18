//! The escalation ladder (ADR 0120, the owner's amendment of 2026-10-18): a lead decides its
//! own area; stuck work goes to its lead, then a huddle, then the CEO only when the lead says
//! it needs the CEO, and the owner's desk only for a board matter. Live, task #666 went in
//! circles and landed on the owner's desk.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, STRANDED_AFTER_MS, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::ladder::VIEWS_WAIT_MS;
use crate::domain::{
    GoalId, MemberId, Priority, ProjectId, Refusal, Role, TaskId, TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// 17 October 2026, at noon UTC.
const OCT_17: i64 = 1_792_238_400_000;

struct Wing {
    ceo: Option<MemberId>,
    kit: MemberId,
    devs: Vec<MemberId>,
    dee: MemberId,
    goal: GoalId,
    cleanup: ProjectId,
}

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings::default(),
    )
}

/// The wing's shape, as the night of 17 October had it: Kit leads the clean-up project, five
/// developers report to him, Dee tests, and Mara is the CEO when `ceo`.
fn wing(hq: &Hq<'_>, ceo: bool) -> Wing {
    let ceo = ceo.then(|| hq.seat_ceo("Mara").unwrap().unwrap());
    let kit = hq.hire("Kit", Role::Lead, 25, 3, 0).unwrap();
    let devs: Vec<MemberId> = ["Rae", "Zed", "Bo", "Cy", "Lu"]
        .iter()
        .map(|n| hq.hire(n, Role::Developer, 40, 2, 0).unwrap())
        .collect();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 0).unwrap();
    let goal = hq.set_goal("Improve the lab", "", Author::Owner).unwrap();
    let cleanup = hq
        .add_project(goal, "ansible-nas clean-up", "the fork", kit, None)
        .unwrap();
    Wing {
        ceo,
        kit,
        devs,
        dee,
        goal,
        cleanup,
    }
}

fn task_for(hq: &Hq<'_>, w: &Wing, to: MemberId, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: w.goal,
            project: Some(w.cleanup),
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn later(clock: &Hands, ms: i64) {
    clock.0.fetch_add(ms, Ordering::SeqCst);
}

/// Two ticks (what one queues, the next starts), every started turn ended, and a tick to see it.
fn settle(hq: &Hq<'_>) {
    hq.tick().unwrap();
    hq.tick().unwrap();
    for c in hq.runner().started() {
        hq.runner().finish(c.run, true);
    }
    hq.tick().unwrap();
}

/// The newest turn of `member`'s on `task`, once the ticks have started it.
fn turn_on(hq: &Hq<'_>, member: MemberId, task: TaskId) -> RunContext {
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member && c.task == Some(task))
        .expect("a turn of the member's on the task")
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

/// A task handed to `to` that nobody comes back to: its recovery, then ten minutes on, the
/// first rung.
fn stuck(hq: &Hq<'_>, clock: &Hands) {
    settle(hq);
    later(clock, STRANDED_AFTER_MS + 1);
    settle(hq);
    later(clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
}

fn wakes_of(hq: &Hq<'_>, member: MemberId, task: TaskId) -> Vec<WakeReason> {
    hq.wakes()
        .unwrap()
        .into_iter()
        .filter(|w| w.member == member && w.task == Some(task))
        .map(|w| w.reason)
        .collect()
}

fn desk(hq: &Hq<'_>) -> Vec<String> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.status == "waiting")
        .map(|d| d.kind)
        .collect()
}

#[test]
fn a_lead_decides_its_own_plan_with_no_tester_and_nobodys_leave() {
    // Live, #666: Kit's plan for his own approved project waited on the CEO's leave and a
    // tester who never passed it.
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let w = wing(&hq, true);
    let plan = task_for(&hq, &w, w.kit, "Plan the clean-up");
    let kit = turn_on(&hq, w.kit, plan);
    assert!(
        kit.told
            .contains("Your projects' plans are yours to decide"),
        "{}",
        kit.told
    );
    use_tool(&hq, &kit, "claim", &json!({ "task": plan.0 })).unwrap();
    assert!(
        use_tool(
            &hq,
            &kit,
            "set_status",
            &json!({ "task": plan.0, "status": "done", "reason": "planned" })
        )
        .unwrap_err()
        .contains("needs evidence")
    );
    use_tool(
        &hq,
        &kit,
        "set_status",
        &json!({ "task": plan.0, "status": "done", "reason": "three branches, one each",
                 "evidence": [format!("task {}", plan.0)] }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(plan).unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert!(wakes_of(&hq, w.dee, plan).is_empty(), "no tester's round");
    assert!(
        wakes_of(&hq, w.ceo.unwrap(), plan).is_empty(),
        "nobody's leave"
    );
    // A developer's work still goes through the tester.
    let fix = task_for(&hq, &w, w.devs[0], "Fix the lint");
    let rae = turn_on(&hq, w.devs[0], fix);
    use_tool(&hq, &rae, "claim", &json!({ "task": fix.0 })).unwrap();
    assert_eq!(
        use_tool(
            &hq,
            &rae,
            "set_status",
            &json!({ "task": fix.0, "status": "done", "reason": "fixed",
                     "evidence": [format!("task {}", fix.0)] })
        ),
        Err(Refusal::OnlyTheGate.to_string())
    );
}

#[test]
fn stuck_work_goes_to_its_lead_first_and_the_lead_decides_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let w = wing(&hq, true);
    let task = task_for(&hq, &w, w.devs[0], "Fix the lint");
    stuck(&hq, &clock);
    assert_eq!(wakes_of(&hq, w.kit, task), [WakeReason::Decide]);
    assert!(desk(&hq).is_empty(), "never straight to the board");
    assert!(
        wakes_of(&hq, w.ceo.unwrap(), task).is_empty(),
        "nor the CEO"
    );
    let kit = turn_on(&hq, w.kit, task);
    assert!(
        kit.told
            .contains("This is stuck in your area, and it is yours to decide"),
        "{}",
        kit.told
    );
    assert!(
        kit.told.contains("Its history, newest last:"),
        "{}",
        kit.told
    );
    // Only to a member of his team, and on evidence.
    let decide = |decision: &str, to: Option<&str>| {
        use_tool(
            &hq,
            &kit,
            "decide_stuck",
            &json!({ "task": task.0, "decision": decision, "detail": "Zed knows the role",
                     "to": to, "evidence": [format!("task {}", task.0)],
                     "handover": { "done": "the role is read", "not_done": "the lint fix",
                                   "next": "fix the lint", "watch_out": "the CI's version" } }),
        )
    };
    assert_eq!(
        decide("hand_to", Some("Mara")),
        Err(Refusal::NotBelow.to_string()),
        "only his own team"
    );
    assert!(decide("hand_to", Some("Zed")).is_ok());
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!(
        (now.status, now.assignee),
        (TaskStatus::Todo, Some(w.devs[1]))
    );
    assert_eq!(wakes_of(&hq, w.devs[1], task), [WakeReason::Assigned]);
    // Decided, it is not his to decide again until it is stuck again.
    assert_eq!(
        decide("drop", None),
        Err(Refusal::NotStuckForYou.to_string())
    );
}

#[test]
fn a_lead_that_says_it_needs_the_ceo_or_the_board_sends_it_up_and_only_then() {
    for (decision, with_ceo, to_the_desk) in [
        ("needs_ceo", true, false),
        ("needs_board", true, false),
        ("needs_board", false, true),
        ("drop", true, false),
    ] {
        let clock = Hands(AtomicI64::new(OCT_17));
        let hq = engine(&clock);
        let w = wing(&hq, with_ceo);
        let task = task_for(&hq, &w, w.devs[0], "Fix the lint");
        stuck(&hq, &clock);
        let kit = turn_on(&hq, w.kit, task);
        use_tool(
            &hq,
            &kit,
            "decide_stuck",
            &json!({ "task": task.0, "decision": decision,
                     "detail": "it needs a paid runner beyond the caps",
                     "evidence": [format!("task {}", task.0)],
                     "handover": { "done": "the lint is read", "not_done": "the fix",
                                   "next": "nothing", "watch_out": "the caps" } }),
        )
        .unwrap();
        let ceo_woken = w
            .ceo
            .is_some_and(|ceo| wakes_of(&hq, ceo, task) == [WakeReason::Decide]);
        assert_eq!(
            ceo_woken,
            with_ceo && decision != "drop",
            "{decision}, with a CEO: {with_ceo}"
        );
        assert_eq!(
            desk(&hq).contains(&"stuck".to_owned()),
            to_the_desk,
            "{decision}: {:?}",
            desk(&hq)
        );
        if decision == "drop" {
            assert_eq!(
                hq.store().task(task).unwrap().unwrap().status,
                TaskStatus::Dropped
            );
        }
        if decision == "needs_board" && with_ceo {
            hq.runner().finish(kit.run, true);
            hq.tick().unwrap();
            let mara = turn_on(&hq, w.ceo.unwrap(), task);
            assert!(
                mara.told.contains("Stuck work reached you up the ladder"),
                "{}",
                mara.told
            );
            assert!(
                hq.store()
                    .thread(task)
                    .unwrap()
                    .iter()
                    .any(|p| p.text.contains("the lead says this is the board's")),
            );
        }
    }
}

/// A member a huddle asked is not idle (ADR 0119 and 0120 together): while its huddle wake
/// waits behind another member's turn on the task, it is given no downtime turn.
#[test]
fn a_member_waiting_to_give_its_view_is_not_given_downtime() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq: Hq<'_> = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        &clock,
        Settings {
            downtime: true,
            ..Settings::default()
        },
    );
    let w = wing(&hq, false);
    let task = task_for(&hq, &w, w.devs[0], "Fix the lint");
    let members = hq.store().members().unwrap();
    for (i, m) in [w.devs[1], w.dee].iter().enumerate() {
        let name = &members.iter().find(|x| x.id == *m).unwrap().name;
        hq.store()
            .post(task, name, "I looked at it.", OCT_17 + i as i64)
            .unwrap();
    }
    stuck(&hq, &clock);
    let kit = turn_on(&hq, w.kit, task);
    hq.runner().finish(kit.run, true);
    hq.tick().unwrap();
    later(&clock, STRANDED_AFTER_MS + 1);
    for _ in 0..4 {
        hq.tick().unwrap();
    }
    let held: Vec<MemberId> = hq
        .wakes()
        .unwrap()
        .into_iter()
        .filter(|wk| {
            wk.task == Some(task)
                && wk.reason == WakeReason::Huddle
                && wk.receipt == crate::domain::Receipt::Held
        })
        .map(|wk| wk.member)
        .collect();
    assert!(!held.is_empty(), "one waits behind another's turn");
    let called = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "huddle.called")
        .expect("a huddle")
        .at;
    // The next day's look, with the huddle still waiting on that member.
    later(&clock, 24 * 60 * 60_000);
    for _ in 0..3 {
        hq.tick().unwrap();
    }
    for member in held {
        assert!(
            !hq.wakes().unwrap().iter().any(|wk| wk.member == member
                && wk.reason == WakeReason::Downtime
                && wk.at >= called),
            "member {member:?} waits for the huddle, not downtime"
        );
    }
}

#[test]
fn a_huddle_hears_at_most_four_who_touched_it_and_its_lead_decides_when_views_are_late() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let w = wing(&hq, false);
    let task = task_for(&hq, &w, w.devs[0], "Fix the lint");
    // All five developers and the tester wrote in its thread, oldest first.
    let members = hq.store().members().unwrap();
    for (i, m) in w.devs.iter().chain([&w.dee]).enumerate() {
        let name = &members.iter().find(|x| x.id == *m).unwrap().name;
        hq.store()
            .post(task, name, "I looked at it.", OCT_17 + i as i64)
            .unwrap();
    }
    stuck(&hq, &clock);
    // The lead does not decide: still stuck, a huddle.
    let kit = turn_on(&hq, w.kit, task);
    hq.runner().finish(kit.run, true);
    hq.tick().unwrap();
    later(&clock, STRANDED_AFTER_MS + 1);
    hq.tick().unwrap();
    let called = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "huddle.called")
        .expect("a huddle");
    let members = called.data["members"].as_array().unwrap().len();
    assert_eq!(members, 5, "the lead and four: {}", called.data);
    assert_eq!(called.data["members"][0], w.kit.0);
    // Nobody gives a view in time: its lead decides on what was said.
    let before = wakes_of(&hq, w.kit, task).len();
    later(&clock, VIEWS_WAIT_MS);
    hq.tick().unwrap();
    assert_eq!(wakes_of(&hq, w.kit, task).len(), before + 1);
    assert_eq!(wakes_of(&hq, w.kit, task).last(), Some(&WakeReason::Decide));
    assert!(desk(&hq).is_empty());
}
