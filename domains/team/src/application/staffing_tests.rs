//! Elastic staffing (ADR 0118): an idle member offered for furlough and never fired, a lead's
//! backlog bringing a furloughed member back before anyone is hired, the owner's own two
//! buttons, and what a furloughed member is left out of.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, DeskItem, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::{
    GoalId, Member, MemberId, Priority, ProjectId, Receipt, Refusal, Refused, Role, TaskId,
    WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// 16 Oct 2026, 16:00 UTC.
const START: i64 = 1_792_166_400_000;
const DAY: i64 = 24 * 60 * 60 * 1000;

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(6),
        clock,
        Settings::default(),
    )
}

/// The team as the owner has it: the CEO, Ada the lead, Dee the tester, and the wing seeded
/// (Kit leads, Rae and Zed are developers).
struct Team {
    ceo: MemberId,
    ada: MemberId,
    dee: MemberId,
    kit: MemberId,
    rae: MemberId,
    zed: MemberId,
    goal: GoalId,
    project: ProjectId,
    plan: TaskId,
}

fn team(hq: &Hq<'_>) -> Team {
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    hq.set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let seeded = hq.seed_wing().unwrap().expect("the wing is seeded");
    // Kit's first turn, on the plan, is over: the Mac takes one turn at a time (ADR 0131), and
    // these tests are about the staffing of the others.
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
    }
    hq.tick().unwrap();
    Team {
        ceo,
        ada,
        dee,
        kit: seeded.lead,
        rae: seeded.developers[0],
        zed: seeded.developers[1],
        goal: seeded.goal,
        project: seeded.project,
        plan: seeded.task,
    }
}

fn member(hq: &Hq<'_>, id: MemberId) -> Member {
    hq.store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

fn desk_of(hq: &Hq<'_>, kind: &str) -> Vec<DeskItem> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == kind && d.status == "waiting")
        .collect()
}

/// The newest turn of `member`'s, after a tick or two to start it. The wing's members build
/// with the Mac (ADR 0131), so the node says it is free.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn")
}

/// Every turn that has started is over, and what its end woke has started and is over too. The
/// Mac takes one turn at a time (ADR 0131), so a test about the next member's turn begins clear.
fn all_turns_over(hq: &Hq<'_>) {
    for _ in 0..3 {
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
        hq.tell_mac(true, "ready");
        hq.tick().unwrap();
    }
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
    }
}

/// A new turn of `member`'s on `task`: woken (a mention), then started. Kit's first turn, on
/// the plan, was over before a test began (see `team`), so a test that wants a briefing written
/// after it changed the world, or a turn it can still use tools in, wakes him again.
fn woken_turn(hq: &Hq<'_>, member: MemberId, task: TaskId) -> RunContext {
    hq.wake(member, Some(task), WakeReason::Mentioned).unwrap();
    turn(hq, member)
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, who) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &who, tool, args)
}

/// An open task of the wing's project, handed to nobody (so nobody is woken).
fn open_task(hq: &Hq<'_>, t: &Team, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: Some(t.project),
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn names(hq: &Hq<'_>, items: &[DeskItem]) -> Vec<String> {
    let mut all: Vec<String> = items
        .iter()
        .map(|d| member(hq, d.member.unwrap()).name)
        .collect();
    all.sort();
    all
}

/// The first tick of a day looks the team over; a week on, the idle are offered.
fn a_quiet_week(hq: &Hq<'_>, clock: &Hands) {
    hq.tick().unwrap();
    clock.0.store(START + 8 * DAY, Ordering::SeqCst);
    hq.tick().unwrap();
}

#[test]
fn an_idle_member_is_offered_for_furlough_and_never_fired() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let before = hq.store().members().unwrap().len();
    a_quiet_week(&hq, &clock);
    let offers = desk_of(&hq, "furlough");
    // Not the CEO, not a lead; the developers and the tester have had nothing to do.
    assert_eq!(names(&hq, &offers), ["Dee", "Rae", "Zed"]);
    let rae = offers.iter().find(|d| d.member == Some(t.rae)).unwrap();
    assert!(
        rae.text.contains("Rae has had nothing to do since 16 Oct"),
        "{}",
        rae.text
    );
    // Once a day: another tick the same day offers nothing more.
    hq.tick().unwrap();
    assert_eq!(desk_of(&hq, "furlough").len(), 3);
    // Approving furloughs; nobody is deleted.
    hq.decide(rae.id, "approve").unwrap();
    let rae_now = member(&hq, t.rae);
    assert!(rae_now.furloughed());
    assert_eq!(rae_now.name, "Rae");
    assert_eq!(hq.store().members().unwrap().len(), before);
    let said: Vec<String> = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "member.furloughed")
        .map(|e| e.data["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(said, ["Rae"]);
}

#[test]
fn a_member_with_work_a_routine_or_a_recent_turn_is_not_offered() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    // Zed has a routine of his own.
    hq.add_routine(
        Author::Owner,
        NewRoutine {
            title: "Look over the roles".to_owned(),
            description: String::new(),
            goal: t.goal,
            project: None,
            assignee: t.zed,
            every_ms: 30 * DAY,
            on_lab_trouble: false,
        },
    )
    .unwrap();
    // Dee is woken (a mention) on day 4: that is not nothing to do.
    hq.tick().unwrap();
    clock.0.store(START + 4 * DAY, Ordering::SeqCst);
    hq.wake(t.dee, None, WakeReason::Mentioned).unwrap();
    hq.tick().unwrap();
    clock.0.store(START + 8 * DAY, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(names(&hq, &desk_of(&hq, "furlough")), ["Rae"]);
}

#[test]
fn keeping_a_member_asks_again_only_a_week_later() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    a_quiet_week(&hq, &clock);
    for item in desk_of(&hq, "furlough") {
        hq.decide(item.id, "decline").unwrap();
    }
    assert!(!member(&hq, t.rae).furloughed());
    // Three days on: still kept.
    clock.0.store(START + 11 * DAY, Ordering::SeqCst);
    hq.tick().unwrap();
    assert!(desk_of(&hq, "furlough").is_empty());
    // A week after the keep: asked again.
    clock.0.store(START + 16 * DAY, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(names(&hq, &desk_of(&hq, "furlough")), ["Dee", "Rae", "Zed"]);
}

#[test]
fn a_furloughed_member_is_never_woken_and_is_left_out_of_briefings_and_shares() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let share = |id: MemberId| {
        hq.snapshot().unwrap()["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == id.0)
            .unwrap()["per_day"]
            .as_i64()
            .unwrap()
    };
    let before = share(t.kit);
    hq.furlough(t.zed).unwrap();
    assert!(share(t.kit) > before, "the share is split among fewer");
    assert_eq!(
        hq.wake(t.zed, None, WakeReason::Mentioned).unwrap(),
        Receipt::Refused(Refused::Furloughed)
    );
    // Kit's turn: his team does not list Zed; the CEO's does not either.
    let kit = woken_turn(&hq, t.kit, t.plan);
    assert!(kit.told.contains("Rae"), "{}", kit.told);
    assert!(!kit.told.contains("Zed"), "{}", kit.told);
    hq.wake(t.ceo, None, WakeReason::QuestionAsked).unwrap();
    let ceo = turn(&hq, t.ceo);
    assert!(ceo.told.contains("Rae"), "{}", ceo.told);
    assert!(!ceo.told.contains("Zed"), "{}", ceo.told);
    // It keeps its record, its name and its place.
    let zed = member(&hq, t.zed);
    assert_eq!((zed.name.as_str(), zed.reports_to), ("Zed", Some(t.kit)));
}

#[test]
fn the_owner_furloughs_and_recalls_any_member_but_the_ceo() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    assert_eq!(
        hq.furlough(t.ceo).unwrap_err().to_string(),
        super::engine::EngineError::Refused(Refusal::IsTheCeo).to_string()
    );
    hq.furlough(t.dee).unwrap();
    assert!(member(&hq, t.dee).furloughed());
    assert!(hq.furlough(t.dee).is_err(), "already furloughed");
    assert!(hq.recall(t.rae).is_err(), "not furloughed");
    hq.recall(t.dee).unwrap();
    let dee = member(&hq, t.dee);
    assert!(!dee.furloughed());
    assert_eq!(dee.recalled_at, Some(START));
    // Open work is handed on first: the count is said.
    open_task(&hq, &t, "Tidy the roles");
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: None,
            parent: None,
            title: "Look at the proxy".to_owned(),
            description: String::new(),
            assignee: Some(t.rae),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    let refused = hq.furlough(t.rae).unwrap_err().to_string();
    assert!(refused.contains("1 open task"), "{refused}");
}

#[test]
fn a_lead_with_a_backlog_gets_a_furloughed_member_back_first() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.furlough(t.zed).unwrap();
    let kit = woken_turn(&hq, t.kit, t.plan);
    let tasks: Vec<TaskId> = (0..2)
        .map(|n| open_task(&hq, &t, &format!("Branch {n}")))
        .collect();
    let cite =
        |ids: &[TaskId]| -> Vec<String> { ids.iter().map(|i| format!("task {i}")).collect() };
    let ask = |args: Value| use_tool(&hq, &kit, "request_staff", &args);
    // One active developer (Rae): two open tasks are enough.
    let said = ask(
        json!({ "project": t.project.0, "why": "two branches wait", "evidence": cite(&tasks) }),
    )
    .unwrap();
    assert!(said.contains("owner's desk"), "{said}");
    let item = desk_of(&hq, "recall").pop().expect("a recall on the desk");
    assert_eq!(item.member, Some(t.kit));
    assert!(item.text.contains("Zed"), "{}", item.text);
    assert!(desk_of(&hq, "hire").is_empty());
    hq.decide(item.id, "approve").unwrap();
    let zed = member(&hq, t.zed);
    assert!(!zed.furloughed());
    assert_eq!(zed.reports_to, Some(t.kit));
    let events: Vec<String> = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(events.contains(&"member.recalled".to_owned()), "{events:?}");
}

#[test]
fn staff_are_requested_on_evidence_of_a_backlog_by_a_lead_alone() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let kit = woken_turn(&hq, t.kit, t.plan);
    let ask = |args: Value| use_tool(&hq, &kit, "request_staff", &args);
    let tasks: Vec<TaskId> = (0..3)
        .map(|n| open_task(&hq, &t, &format!("Branch {n}")))
        .collect();
    let cite =
        |ids: &[TaskId]| -> Vec<String> { ids.iter().map(|i| format!("task {i}")).collect() };
    let p = t.project.0;
    // Evidence is needed, and must exist.
    let refused = ask(json!({ "project": p, "why": "x", "evidence": [] })).unwrap_err();
    assert!(refused.contains("evidence"), "{refused}");
    let refused = ask(json!({ "project": p, "why": "x", "evidence": ["task 999"] })).unwrap_err();
    assert!(refused.contains("not in HQ's record"), "{refused}");
    // Two active developers: three open tasks are not twice as many.
    let refused = ask(json!({ "project": p, "why": "x", "evidence": cite(&tasks) })).unwrap_err();
    assert!(
        refused.contains("3 open tasks")
            && refused.contains("2 active developers")
            && refused.contains("at least 4"),
        "{refused}"
    );
    // A task of another project, or one that is over, does not count.
    let other = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                project: None,
                parent: None,
                title: "Elsewhere".to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let mut with_other = cite(&tasks);
    with_other.push(format!("task {other}"));
    let refused = ask(json!({ "project": p, "why": "x", "evidence": with_other })).unwrap_err();
    assert!(
        refused.contains(&format!("task {other}")) && refused.contains("not an open task of"),
        "{refused}"
    );
    // Four open tasks: no furloughed developer, so it becomes the hire request.
    let mut four = tasks.clone();
    four.push(open_task(&hq, &t, "Branch 3"));
    let said =
        ask(json!({ "project": p, "why": "four branches wait", "evidence": cite(&four) })).unwrap();
    assert!(said.contains("owner's desk"), "{said}");
    let hire = desk_of(&hq, "hire").pop().expect("a hire request");
    assert_eq!(hire.member, Some(t.kit));
    // One at a time.
    let again = ask(json!({ "project": p, "why": "four branches wait", "evidence": cite(&four) }))
        .unwrap_err();
    assert!(again.contains("waiting"), "{again}");
    // Only a lead is offered it, and only a lead may call it.
    assert!(
        super::protocol::tools(Role::Lead)
            .to_string()
            .contains("request_staff")
    );
    for role in [Role::Developer, Role::Reviewer, Role::Researcher, Role::Ceo] {
        assert!(
            !super::protocol::tools(role)
                .to_string()
                .contains("request_staff"),
            "{role:?}"
        );
    }
    // Kit's turn is over, and the Mac, which takes one turn at a time, is free for Rae's.
    all_turns_over(&hq);
    hq.wake(t.rae, Some(four[0]), WakeReason::Mentioned)
        .unwrap();
    let rae = turn(&hq, t.rae);
    let refused = use_tool(
        &hq,
        &rae,
        "request_staff",
        &json!({ "project": p, "why": "x", "evidence": cite(&four) }),
    )
    .unwrap_err();
    assert!(refused.contains("only a lead"), "{refused}");
    // Nor a lead for a project that is not hers.
    all_turns_over(&hq);
    hq.wake(t.ada, Some(four[1]), WakeReason::Mentioned)
        .unwrap();
    let ada = turn(&hq, t.ada);
    let refused = use_tool(
        &hq,
        &ada,
        "request_staff",
        &json!({ "project": p, "why": "x", "evidence": cite(&four) }),
    )
    .unwrap_err();
    assert!(refused.contains("lead"), "{refused}");
    let _ = t.ada;
}

#[test]
fn the_wings_seats_still_hold_when_staff_are_requested() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    // Three more developers fill the wing's five seats.
    for name in ["Sam", "Lee", "Max"] {
        let id = hq
            .hire_by_owner(name, Role::Developer, "claude-haiku-5-5", Some(t.kit))
            .unwrap();
        assert!(member(&hq, id).reports_to == Some(t.kit));
    }
    let kit = woken_turn(&hq, t.kit, t.plan);
    let tasks: Vec<String> = (0..10)
        .map(|n| format!("task {}", open_task(&hq, &t, &format!("Branch {n}"))))
        .collect();
    let refused = use_tool(
        &hq,
        &kit,
        "request_staff",
        &json!({ "project": t.project.0, "why": "x", "evidence": tasks }),
    )
    .unwrap_err();
    assert!(refused.contains("developers"), "{refused}");
}

#[test]
fn the_scorecard_counts_each_teams_seats_active_and_furloughed() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.furlough(t.zed).unwrap();
    let card = hq.snapshot().unwrap()["scorecard"]["teams"].clone();
    let kits = card
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["lead"] == t.kit.0)
        .unwrap()
        .clone();
    assert_eq!(
        (
            kits["seats"].clone(),
            kits["active"].clone(),
            kits["furloughed"].clone()
        ),
        (json!(3), json!(2), json!(1))
    );
    let _ = t.dee;
}

#[test]
fn no_engine_method_or_query_ever_deletes_a_member() {
    let store = include_str!("../infrastructure/sqlite.rs").to_ascii_lowercase();
    let marker = ["delete", "from", "members"].join(" ");
    assert!(!store.contains(&marker), "the store deletes members");
    let ports = include_str!("ports.rs").to_ascii_lowercase();
    for word in ["fn delete_member", "fn remove_member", "fn fire_member"] {
        assert!(!ports.contains(word), "{word}");
    }
}
