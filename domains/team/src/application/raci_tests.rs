//! Roles as an enforced RACI (ADR 0120): the CEO sets direction and stays out of the areas'
//! work. It reads anything and acts on nothing in the lab; it is never woken by the lab's
//! alarms; on a task in a lead's area it does not claim, move, close, accept or send back, but
//! directs the lead, who decides. Leads propose; the CEO decides. The manager chain still acts
//! for a lead over its own members.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Called, Engine, EngineError, Settings};
use super::ports::{Clock, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::domain::routines::HOUR_MS;
use crate::domain::{
    GoalId, MemberId, Priority, ProjectId, Receipt, Refusal, Role, TaskId, TaskStatus, WakeReason,
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

struct Org {
    ceo: MemberId,
    ops: MemberId,
    dev: MemberId,
    goal: GoalId,
    upkeep: ProjectId,
}

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    )
}

/// Mara the CEO; Ada, who leads the home lab's upkeep, and Linus who reports to her; the
/// lab's routine for Ada in that project.
fn org(hq: &Hq<'_>) -> Org {
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    let ops = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.set_brain(ops, "claude-sonnet-5-5").unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let upkeep = hq
        .add_project(goal, "Home lab upkeep", "the lab", ops, None)
        .unwrap();
    hq.add_routine(
        Author::Endora,
        NewRoutine {
            title: "Check the home lab".to_owned(),
            description: String::new(),
            goal,
            project: Some(upkeep),
            assignee: ops,
            every_ms: 6 * HOUR_MS,
            on_lab_trouble: true,
        },
    )
    .unwrap();
    Org {
        ceo,
        ops,
        dev,
        goal,
        upkeep,
    }
}

fn task_for(hq: &Hq<'_>, o: &Org, to: MemberId, title: &str, project: Option<ProjectId>) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: o.goal,
            project,
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

fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn end(hq: &Hq<'_>, clock: &Hands, context: &RunContext) {
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    clock.0.fetch_add(3 * 60_000, Ordering::SeqCst);
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

fn reading(hq: &Hq<'_>, context: &RunContext) -> i64 {
    hq.record_receipt(
        &context.token,
        &Called {
            tool: "homelab_status".to_owned(),
            args: String::new(),
            excerpt: "deluge: running".to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
}

fn names(role: Role) -> Vec<String> {
    tools(role)
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect()
}

fn wakes_of(hq: &Hq<'_>, member: MemberId) -> Vec<(WakeReason, Receipt)> {
    hq.store()
        .wakes()
        .unwrap()
        .into_iter()
        .filter(|w| w.member == member)
        .map(|w| (w.reason, w.receipt))
        .collect()
}

fn thread(hq: &Hq<'_>, task: TaskId) -> Vec<String> {
    hq.store()
        .thread(task)
        .unwrap()
        .into_iter()
        .map(|p| format!("{}: {}", p.author, p.text))
        .collect()
}

#[test]
fn the_ceo_directs_decides_and_reads_and_proposes_accepts_and_acts_on_nothing() {
    let ceo = names(Role::Ceo);
    for kept in [
        "direct",
        "decide_project",
        "propose_goal",
        "ask_owner",
        "read_doc",
        "read_document",
        "note",
        "hand_to",
        "note_house",
    ] {
        assert!(ceo.iter().any(|n| n == kept), "{kept}: {ceo:?}");
    }
    for gone in ["propose_project", "accept", "review"] {
        assert!(!ceo.iter().any(|n| n == gone), "{gone}: {ceo:?}");
    }
    // Leads propose and accept; nobody else directs.
    let lead = names(Role::Lead);
    assert!(lead.iter().any(|n| n == "propose_project") && lead.iter().any(|n| n == "accept"));
    for role in [
        Role::Lead,
        Role::Developer,
        Role::Reviewer,
        Role::Researcher,
    ] {
        assert!(!names(role).iter().any(|n| n == "direct"), "{role:?}");
    }
}

#[test]
fn an_alarm_or_a_recovery_wakes_the_areas_lead_and_never_the_ceo() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    hq.alarm("deluge, prowlarr exited at 23:43").unwrap();
    let task = hq.store().routines().unwrap()[0].last_task.unwrap();
    // As live: the alarm's task had gone up to the CEO, who then held it.
    hq.store().set_assignee(task, Some(o.ceo)).unwrap();
    hq.alarm("deluge, nzbget, prowlarr exited at 01:44")
        .unwrap();
    hq.recovered("deluge is running again").unwrap();
    assert!(
        wakes_of(&hq, o.ceo).is_empty(),
        "the CEO is never woken by the lab: {:?}",
        wakes_of(&hq, o.ceo)
    );
    let ada: Vec<WakeReason> = wakes_of(&hq, o.ops).into_iter().map(|w| w.0).collect();
    // The alarm that made the task (an emergency, ADR 0131), the one passed into it, and the
    // recovery: three, all Ada's.
    assert!(
        ada.iter().filter(|r| **r == WakeReason::Alarm).count() == 3,
        "{ada:?}"
    );
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!(now.assignee, Some(o.ops), "the area's lead has it again");
    assert!(
        thread(&hq, task)
            .iter()
            .any(|p| p.contains("This is Ada's area: it goes to Ada")),
        "{:?}",
        thread(&hq, task)
    );
}

#[test]
fn on_an_areas_task_the_ceo_directs_and_the_lead_decides() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    hq.alarm("deluge exited at 23:43").unwrap();
    let task = hq.store().routines().unwrap()[0].last_task.unwrap();
    let ada = turn(&hq, o.ops);
    use_tool(&hq, &ada, "claim", &json!({ "task": task.0 })).unwrap();
    use_tool(
        &hq,
        &ada,
        "ask_owner",
        &json!({ "text": "Deluge keeps exiting. Plan a gluetun change?",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    end(&hq, &clock, &ada);

    let mara = turn(&hq, o.ceo);
    assert_eq!(
        mara.task,
        Some(task),
        "a question up the chain is the CEO's"
    );
    assert!(
        mara.told.contains("This task is in Ada's area"),
        "{}",
        mara.told
    );
    let refused = Refusal::CeoDirects("Ada".to_owned()).to_string();
    // Reading is open to the CEO: it may look at the lab itself.
    let read = reading(&hq, &mara);
    for (tool, args) in [
        ("claim", json!({ "task": task.0 })),
        (
            "set_status",
            json!({ "task": task.0, "status": "done", "reason": "it held",
                    "evidence": [format!("r{read}"), format!("r{read}")] }),
        ),
        (
            "set_status",
            json!({ "task": task.0, "status": "blocked", "reason": "wait" }),
        ),
        (
            "create_task",
            json!({ "title": "Change gluetun", "description": "x", "assignee": "Ada",
                    "criteria": [{ "text": "gluetun is pinned" }] }),
        ),
    ] {
        assert_eq!(
            use_tool(&hq, &mara, tool, &args),
            Err(refused.clone()),
            "{tool}"
        );
    }
    assert_eq!(
        hq.accept_in(mara.run, task, false, "no", &[], None),
        Err(EngineError::Refused(Refusal::CeoDirects("Ada".to_owned())))
    );
    // A direction needs evidence, and then goes to the lead, who is woken.
    assert!(
        use_tool(
            &hq,
            &mara,
            "direct",
            &json!({ "task": task.0, "direction": "Propose the gluetun change." })
        )
        .unwrap_err()
        .contains("needs evidence")
    );
    let said = use_tool(
        &hq,
        &mara,
        "direct",
        &json!({ "task": task.0, "direction": "Propose the gluetun change, with its evidence.",
                 "evidence": [format!("r{read}")] }),
    )
    .unwrap();
    assert!(said.contains("Ada is woken: Ada decides"), "{said}");
    assert!(
        thread(&hq, task).iter().any(|p| p.starts_with(
            "Mara: Direction for Ada: Propose the gluetun change, with its evidence.\nEvidence: r"
        )),
        "{:?}",
        thread(&hq, task)
    );
    let ada_wakes = wakes_of(&hq, o.ops);
    assert_eq!(ada_wakes.last().unwrap().0, WakeReason::Directed);
    // The CEO reaching into Ada's task is a rescue on her roster line (ADR 0124), once, and her
    // other wakes before it are not.
    let rescues = hq.snapshot().unwrap()["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == o.ops.0)
        .unwrap()["rescues"]
        .clone();
    assert_eq!(rescues, json!(1), "one direction, one rescue");
    end(&hq, &clock, &mara);
    let ada = turn(&hq, o.ops);
    assert!(
        ada.told
            .contains("Mara, the CEO, directs you: Propose the gluetun change, with its evidence."),
        "{}",
        ada.told
    );
    assert!(ada.told.contains("you decide"), "{}", ada.told);
    // The task is still Ada's, as it was.
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!((now.owner, now.status), (Some(o.ops), TaskStatus::Doing));
    // A week and a day on, with the feed whole, the direction is no longer this week's.
    clock.0.fetch_add(8 * 24 * 60 * 60 * 1000, Ordering::SeqCst);
    let rescues = hq.snapshot().unwrap()["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == o.ops.0)
        .unwrap()["rescues"]
        .clone();
    assert_eq!(rescues, json!(0), "counted over the week, not for ever");
}

#[test]
fn the_ceo_delegates_to_leads_only_and_proposes_no_project() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    let review = task_for(&hq, &o, o.ceo, "Daily review", None);
    let mara = turn(&hq, o.ceo);
    assert_eq!(mara.task, Some(review));
    assert!(!mara.told.contains("This task is in"), "her own work");
    let new = |to: &str| {
        use_tool(
            &hq,
            &mara,
            "create_task",
            &json!({ "title": "Look at the backups", "description": "x", "assignee": to,
                     "criteria": [{ "text": "the backups are planned" }] }),
        )
    };
    assert_eq!(new("Linus"), Err(Refusal::CeoToLeads.to_string()));
    assert!(new("Ada").unwrap().starts_with("Created task"));
    let linus_task = task_for(&hq, &o, o.dev, "Fix sonarr", Some(o.upkeep));
    assert_eq!(
        hq.hand_to_in(
            mara.run,
            linus_task,
            o.dev,
            "yours",
            &crate::domain::world::HandoverNote::new("a", "b", "c", "d").unwrap()
        ),
        Err(EngineError::Refused(Refusal::CeoToLeads))
    );
    assert_eq!(
        use_tool(
            &hq,
            &mara,
            "propose_project",
            &json!({ "title": "Backups", "reason": "they fail", "tasks": [review.0] })
        ),
        Err(Refusal::OnlyLeadsPropose.to_string())
    );
    // Her own review she takes and moves herself.
    use_tool(&hq, &mara, "claim", &json!({ "task": review.0 })).unwrap();
    use_tool(
        &hq,
        &mara,
        "set_status",
        &json!({ "task": review.0, "status": "in_review", "reason": "done",
                 "evidence": [format!("task {}", review.0)] }),
    )
    .unwrap();
}

/// A second area beside Ada's: Kit leads the clean-up project, and Rae reports to him.
fn second_area(hq: &Hq<'_>, o: &Org) -> (MemberId, MemberId, ProjectId) {
    let kit = hq.hire("Kit", Role::Lead, 25, 2, 0).unwrap();
    let rae = hq.hire("Rae", Role::Developer, 40, 2, 0).unwrap();
    hq.set_reports_to(rae, kit).unwrap();
    let cleanup = hq
        .add_project(o.goal, "Clean-up", "the fork", kit, None)
        .unwrap();
    (kit, rae, cleanup)
}

fn note() -> crate::domain::world::HandoverNote {
    crate::domain::world::HandoverNote::new("nothing yet", "the role", "tidy it", "the check")
        .unwrap()
}

#[test]
fn the_ceo_cannot_hand_one_areas_work_to_another_areas_lead() {
    // Live, the CEO handed an engineering task to the Ops lead: the CEO directs and does not
    // move an area's work (ADR 0120, amended).
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    let (kit, rae, cleanup) = second_area(&hq, &o);
    let task = task_for(&hq, &o, rae, "Tidy the deluge role", Some(cleanup));
    task_for(&hq, &o, o.ceo, "Daily review", None);
    let mara = turn(&hq, o.ceo);
    let wakes = wakes_of(&hq, o.ops).len();
    assert_eq!(
        hq.hand_to_in(mara.run, task, o.ops, "Ops has the time", &note()),
        Err(EngineError::Refused(Refusal::OutsideTheArea {
            lead: "Kit".to_owned(),
            to: "Ada".to_owned()
        }))
    );
    let said = use_tool(
        &hq,
        &mara,
        "hand_to",
        &json!({ "task": task.0, "to": "Ada", "why": "Ops has the time",
                 "handover": { "done": "nothing yet", "not_done": "the role", "next": "tidy it",
                               "watch_out": "the check" } }),
    )
    .unwrap_err();
    assert!(
        said.starts_with("Ada is outside Kit's area: only Kit, who leads it, or the owner"),
        "{said}"
    );
    let still = hq.store().task(task).unwrap().unwrap();
    assert_eq!(still.assignee, Some(rae), "nothing moved");
    assert_eq!(wakes_of(&hq, o.ops).len(), wakes, "Ada was not woken");
    // The area's own lead is inside it: handing the task up to Kit stays the CEO's to do.
    hq.hand_to_in(mara.run, task, kit, "Kit decides", &note())
        .unwrap();
    assert_eq!(hq.store().task(task).unwrap().unwrap().assignee, Some(kit));
}

#[test]
fn only_an_areas_own_lead_hands_its_work_out_of_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    let (kit, rae, cleanup) = second_area(&hq, &o);
    let bo = hq.hire("Bo", Role::Developer, 40, 2, 0).unwrap();
    let task = task_for(&hq, &o, rae, "Tidy the deluge role", Some(cleanup));
    let kept = task_for(&hq, &o, o.dev, "Look at radarr", Some(cleanup));
    task_for(&hq, &o, kit, "Plan the clean-up", Some(cleanup));
    task_for(&hq, &o, o.ops, "Plan the week", Some(o.upkeep));
    let kit_turn = turn(&hq, kit);
    // Kit leads the area the task is in, so he may hand it to a developer of Ada's.
    hq.hand_to_in(kit_turn.run, task, o.dev, "Linus knows the role", &note())
        .unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().assignee,
        Some(o.dev)
    );
    // Ada manages Linus, but the clean-up project is Kit's area: she does not hand its work to
    // another of her own.
    let ada_turn = turn(&hq, o.ops);
    assert_eq!(
        hq.hand_to_in(ada_turn.run, kept, bo, "Bo is free", &note()),
        Err(EngineError::Refused(Refusal::OutsideTheArea {
            lead: "Kit".to_owned(),
            to: "Bo".to_owned()
        }))
    );
    assert_eq!(
        hq.store().task(kept).unwrap().unwrap().assignee,
        Some(o.dev),
        "nothing moved"
    );
}

#[test]
fn a_leads_stalled_task_stays_in_its_area_and_never_goes_up_to_the_ceo() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    let task = task_for(&hq, &o, o.ops, "Look at deluge", Some(o.upkeep));
    let first = turn(&hq, o.ops);
    use_tool(&hq, &first, "claim", &json!({ "task": task.0 })).unwrap();
    end(&hq, &clock, &first);
    for _ in 0..3 {
        hq.post(Author::Owner, task, "Next.").unwrap();
        let ada = turn(&hq, o.ops);
        use_tool(
            &hq,
            &ada,
            "post",
            &json!({ "task": task.0, "text": "Looking." }),
        )
        .unwrap();
        end(&hq, &clock, &ada);
    }
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!(now.assignee, Some(o.ops), "it stays with Ada");
    assert!(
        wakes_of(&hq, o.ceo).is_empty(),
        "{:?}",
        wakes_of(&hq, o.ceo)
    );
    // ADR 0139: Ada takes it a model tier up herself.
    assert!(
        thread(&hq, task)
            .iter()
            .any(|p| p.contains("Ada takes it a model tier up")),
        "{:?}",
        thread(&hq, task)
    );
}

#[test]
fn the_ceo_writes_each_goals_strategy_on_evidence_and_the_owner_edits_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    let review = task_for(&hq, &o, o.ceo, "Daily review", None);
    let mara = turn(&hq, o.ceo);
    assert!(
        mara.told
            .contains("No strategy yet: write it with set_strategy"),
        "{}",
        mara.told
    );
    let write = |args: Value| use_tool(&hq, &mara, "set_strategy", &args);
    assert!(
        write(json!({ "goal": o.goal.0, "purpose": "The house runs itself." }))
            .unwrap_err()
            .contains("needs evidence")
    );
    write(
        json!({ "goal": o.goal.0, "purpose": "The house runs itself.",
                  "measures": "unplanned outages over 10 min",
                  "evidence": [format!("task {}", review.0)] }),
    )
    .unwrap();
    let written = hq.strategy_of(o.goal).unwrap();
    assert_eq!(
        (written.purpose.as_str(), written.by.as_str()),
        ("The house runs itself.", "Mara")
    );
    // Only the CEO's turn writes it; the owner writes it from the goal card.
    end(&hq, &clock, &mara);
    let lead_task = task_for(&hq, &o, o.ops, "Plan", Some(o.upkeep));
    let ada = turn(&hq, o.ops);
    assert_eq!(ada.task, Some(lead_task));
    assert_eq!(
        use_tool(
            &hq,
            &ada,
            "set_strategy",
            &json!({ "goal": o.goal.0, "bets": "mine", "evidence": [format!("task {}", lead_task.0)] })
        ),
        Err(Refusal::OnlyTheCeo.to_string())
    );
    hq.set_strategy(
        o.goal,
        &crate::domain::areas::StrategyEdit {
            not_now: Some("No new containers this month.".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    let snap = hq.snapshot().unwrap();
    let goal = &snap["goals"][0]["strategy"];
    assert_eq!(goal["purpose"], "The house runs itself.");
    assert_eq!(goal["not_now"], "No new containers this month.");
    assert_eq!(goal["by"], "owner");
}

#[test]
fn a_leads_daily_report_is_its_areas_report_and_never_work_on_the_board() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    let made = hq.seed_area_routines().unwrap();
    assert_eq!(made.len(), 2);
    let report = hq
        .store()
        .routines()
        .unwrap()
        .into_iter()
        .find(|r| r.id == made[0])
        .unwrap();
    assert_eq!(report.title, "State of my area");
    assert_eq!(report.assignee, o.ops);
    assert_eq!(
        report.next_due_at,
        OCT_17 + 6 * HOUR_MS,
        "due at the owner's 18:00 (UTC here)"
    );
    hq.alarm("deluge exited at 11:00").unwrap();
    hq.fire_routine(report.id).unwrap();
    let task = hq
        .store()
        .routines()
        .unwrap()
        .into_iter()
        .find(|r| r.id == report.id)
        .and_then(|r| r.last_task)
        .unwrap();
    for c in hq.runner().started() {
        hq.runner().finish(c.run, true);
    }
    hq.tick().unwrap();
    hq.tick().unwrap();
    let ada = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.task == Some(task))
        .expect("Ada's report turn");
    for said in [
        "Report on your area to the CEO",
        "Your area's open work (1):",
        "Alarms in your area in the last 24 hours: 1.",
        "Your area's measures, the last 7 days: the lab: 1 alarms;",
    ] {
        assert!(ada.told.contains(said), "{said}\n---\n{}", ada.told);
    }
    use_tool(
        &hq,
        &ada,
        "report_area",
        &json!({ "moved": "#1 deluge restarted", "stuck": "nothing",
                 "need": "a gluetun change from the wing", "proposals": "pin gluetun" }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    let snap = hq.snapshot().unwrap();
    assert!(
        !snap["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == task.0),
        "a report is not work on the board"
    );
    assert_eq!(snap["scorecard"]["today"]["finished"]["done"], 0);
    assert_eq!(
        snap["area_reports"][0]["need"],
        "a gluetun change from the wing"
    );
    let area = &snap["scorecard"]["areas"][0];
    assert_eq!(
        (
            area["name"].as_str(),
            area["week"]["lab"]["alarms"].as_i64()
        ),
        (Some("Ada"), Some(1))
    );
    // The CEO's review reads it.
    end(&hq, &clock, &ada);
    task_for(&hq, &o, o.ceo, "Daily review", None);
    let mara = turn(&hq, o.ceo);
    assert!(
        mara.told.contains(
            "- Ada, 17 Oct:\n  Moved: #1 deluge restarted\n  Stuck: nothing\n  Needs: a gluetun change from the wing\n  Proposes: pin gluetun\n"
        ),
        "{}",
        mara.told
    );
    // A report is a lead's, in its own report's turn.
    assert_eq!(
        use_tool(
            &hq,
            &mara,
            "report_area",
            &json!({ "moved": "x", "stuck": "", "need": "", "proposals": "" })
        ),
        Err(Refusal::NotAReport.to_string())
    );
}

#[test]
fn a_lead_still_acts_on_its_own_members_work() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let o = org(&hq);
    let task = task_for(&hq, &o, o.dev, "Look at radarr", Some(o.upkeep));
    task_for(&hq, &o, o.ops, "Plan the week", Some(o.upkeep));
    let ada = turn(&hq, o.ops);
    use_tool(
        &hq,
        &ada,
        "hand_to",
        &json!({ "task": task.0, "to": "Ada", "why": "I will take it",
                 "handover": { "done": "nothing yet", "not_done": "the look", "next": "look",
                               "watch_out": "radarr restarts slowly" } }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().assignee,
        Some(o.ops)
    );
}
