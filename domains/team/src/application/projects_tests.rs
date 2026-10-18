//! Projects, budgets, proposals, planning and whom members report to (ADR 0113), end to end
//! over HQ's real store and the simulated runner.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Asked, Author, Engine, EngineError, PLANNING, Settings};
use super::ports::{Clock, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::domain::money::DOLLAR;
use crate::domain::routines::{DAY_MS, HOUR_MS};
use crate::domain::{
    GoalId, MemberId, Pause, Priority, ProjectId, ProjectStatus, Receipt, Refusal, Refused, Role,
    TaskId, TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// 17 October 2026, at noon UTC.
const OCT_17: i64 = 1_792_238_400_000;

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    )
}

struct Team {
    ceo: MemberId,
    lead: MemberId,
    dev: MemberId,
    goal: GoalId,
}

/// The CEO on Opus, a lead on Sonnet, a developer on the house's model, and the home lab's
/// goal.
fn team(hq: &Hq<'_>) -> Team {
    let ceo = hq.seat_ceo("Mara").unwrap().expect("seated");
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    Team {
        ceo,
        lead,
        dev,
        goal,
    }
}

fn task_in(
    hq: &Hq<'_>,
    goal: GoalId,
    project: Option<ProjectId>,
    to: MemberId,
    title: &str,
) -> Result<TaskId, EngineError> {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
}

/// The newest turn of `member`'s, once the tick has started what is queued.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(
    hq: &Hq<'_>,
    context: &RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

fn project(hq: &Hq<'_>, id: ProjectId) -> crate::domain::Project {
    hq.store()
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
}

fn waiting(hq: &Hq<'_>, kind: &str) -> Vec<crate::application::ports::DeskItem> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == kind && d.status == "waiting")
        .collect()
}

fn last_wake(hq: &Hq<'_>) -> (MemberId, Option<TaskId>, WakeReason, Receipt) {
    let w = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    (w.member, w.task, w.reason, w.receipt)
}

/// A turn of the lead's on `task` that spends `amount`, then ends.
fn spend(hq: &Hq<'_>, lead: MemberId, task: TaskId, amount: i64) {
    hq.wake(lead, Some(task), WakeReason::Assigned).unwrap();
    let context = turn(hq, lead);
    assert_eq!(context.task, Some(task));
    hq.report_usage(context.run, amount).unwrap();
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
}

#[test]
fn a_tasks_project_serves_its_goal_and_a_sub_task_stays_in_its_parents() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let other = hq.set_goal("Ship the tracker", "", Author::Owner).unwrap();
    let upkeep = hq
        .add_project(t.goal, "Home lab upkeep", "", t.lead, None)
        .unwrap();
    assert_eq!(
        task_in(&hq, other, Some(upkeep), t.dev, "Write it"),
        Err(EngineError::Refused(Refusal::OtherGoal))
    );
    let fix = task_in(&hq, t.goal, Some(upkeep), t.lead, "Fix sonarr").unwrap();
    let context = turn(&hq, t.lead);
    use_tool(&hq, &context, "claim", &json!({ "task": fix.0 })).unwrap();
    use_tool(
        &hq,
        &context,
        "create_task",
        &json!({ "title": "Restart sonarr", "description": "and watch it", "assignee": "Linus",
                 "criteria": [{ "text": "sonarr is running", "live": true }] }),
    )
    .unwrap();
    let sub = hq.store().tasks().unwrap().pop().unwrap();
    assert_eq!(
        (sub.parent, sub.project),
        (Some(fix), Some(upkeep)),
        "in its parent's project"
    );
    // A project that takes no work takes no task.
    hq.change_project(upkeep, "drop").unwrap();
    assert_eq!(
        task_in(&hq, t.goal, Some(upkeep), t.dev, "One more"),
        Err(EngineError::Refused(Refusal::ProjectClosed))
    );
    assert_eq!(
        hq.store().task(sub.id).unwrap().unwrap().status,
        TaskStatus::Dropped,
        "its open work went with it"
    );
}

#[test]
fn the_projects_caps_together_never_pass_the_pools_month() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let a = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(50 * DOLLAR))
        .unwrap();
    assert_eq!(
        hq.add_project(t.goal, "Media", "", t.lead, Some(21 * DOLLAR)),
        Err(EngineError::Refused(Refusal::CapsOverPool {
            room: 20 * DOLLAR
        }))
    );
    let b = hq
        .add_project(t.goal, "Media", "", t.lead, Some(20 * DOLLAR))
        .unwrap();
    assert_eq!(
        hq.set_project_cap(a, Some(51 * DOLLAR)),
        Err(EngineError::Refused(Refusal::CapsOverPool {
            room: 50 * DOLLAR
        })),
        "its own cap is not counted twice"
    );
    // A paused project keeps its cap; a dropped one gives it back.
    hq.change_project(b, "pause").unwrap();
    assert!(hq.set_project_cap(a, Some(51 * DOLLAR)).is_err());
    hq.change_project(b, "drop").unwrap();
    hq.set_project_cap(a, Some(70 * DOLLAR)).unwrap();
    assert_eq!(project(&hq, a).cap, Some(70 * DOLLAR));
    assert_eq!(
        hq.add_project(t.goal, "Led by a developer", "", t.dev, None),
        Err(EngineError::Refused(Refusal::NotAProjectLead))
    );
}

#[test]
fn a_cloud_turn_on_a_capped_project_reserves_no_more_than_is_left_of_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 4))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    let context = turn(&hq, t.lead);
    assert_eq!(context.task, Some(task));
    let run = hq.store().run(context.run).unwrap().unwrap();
    assert_eq!(
        run.reserved,
        DOLLAR / 4,
        "the relay's `left` is this, less what it spent"
    );
}

#[test]
fn at_eighty_percent_the_owner_hears_once_a_month_and_nothing_is_blocked() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 2))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    hq.tick().unwrap();
    let first = hq.runner().started().pop().unwrap();
    hq.report_usage(first.run, 400_000).unwrap();
    let notes = waiting(&hq, "notice");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].project, Some(p));
    assert_eq!(
        notes[0].text,
        "Project #1 Upkeep has spent 80% of its $0.50 cap this month ($0.40)."
    );
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();
    // Still at work, and told no second time this month.
    hq.wake(t.lead, Some(task), WakeReason::Assigned).unwrap();
    let again = turn(&hq, t.lead);
    assert_ne!(again.run, first.run);
    hq.report_usage(again.run, 10_000).unwrap();
    assert_eq!(waiting(&hq, "notice").len(), 1, "once a month");
    assert_eq!(project(&hq, p).status, ProjectStatus::Active);
    // The CEO's next briefing says so.
    let review = hq
        .add_routine(
            Author::Owner,
            NewRoutine {
                title: "Daily review".to_owned(),
                description: String::new(),
                goal: t.goal,
                project: None,
                assignee: t.ceo,
                every_ms: DAY_MS,
                on_lab_trouble: false,
            },
        )
        .unwrap();
    hq.fire_routine(review).unwrap();
    let ceo = turn(&hq, t.ceo);
    assert!(
        ceo.told
            .contains("Projects at 80% of their cap or more this month (the owner was told):\n- #1 Upkeep (active, led by Ada): $0.41 of $0.50 this month"),
        "{}",
        ceo.told
    );
}

#[test]
fn at_its_cap_a_project_pauses_its_cloud_turns_and_its_local_members_go_on() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 2))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    spend(&hq, t.lead, task, 400_000);
    spend(&hq, t.lead, task, 60_000);
    let paused = project(&hq, p);
    assert_eq!(
        (paused.status, paused.pause),
        (ProjectStatus::Paused, Some(Pause::Budget))
    );
    let items = waiting(&hq, "project_budget");
    assert_eq!(items.len(), 1);
    assert!(
        items[0]
            .text
            .starts_with("Project #1 Upkeep spent its $0.50 cap for the month ($0.46)"),
        "{}",
        items[0].text
    );
    // The lead thinks on Claude: turned away. The developer on the house's model goes on.
    hq.wake(t.lead, Some(task), WakeReason::Assigned).unwrap();
    assert_eq!(
        last_wake(&hq).3,
        Receipt::Refused(Refused::OutOfBudget),
        "no Claude turn on it"
    );
    let local = task_in(&hq, t.goal, Some(p), t.dev, "Read the logs").unwrap();
    let dev = turn(&hq, t.dev);
    assert_eq!(dev.task, Some(local), "a local turn costs nothing");
    let feed: Vec<String> = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "project.budget")
        .map(|e| e.data["level"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(feed, ["warned", "spent"]);
}

#[test]
fn a_budget_pause_lifts_when_the_owner_raises_the_cap_or_the_month_turns() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(DOLLAR / 2))
        .unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    spend(&hq, t.lead, task, 400_000);
    spend(&hq, t.lead, task, 60_000);
    let item = waiting(&hq, "project_budget").remove(0);
    // A raise is more than the month's spend, and within the pool.
    assert_eq!(
        hq.decide_saying(item.id, "raise", "0.40"),
        Err(EngineError::Refused(Refusal::CapNotAboveSpend {
            spent: 460_000
        }))
    );
    hq.decide_saying(item.id, "raise", "$1").unwrap();
    let raised = project(&hq, p);
    assert_eq!(
        (raised.status, raised.cap),
        (ProjectStatus::Active, Some(DOLLAR))
    );
    assert!(waiting(&hq, "project_budget").is_empty());
    // Spent again, and the owner waits: the month turns, and it goes on by itself.
    spend(&hq, t.lead, task, 400_000);
    spend(&hq, t.lead, task, 150_000);
    assert_eq!(project(&hq, p).status, ProjectStatus::Paused);
    let item = waiting(&hq, "project_budget").remove(0);
    hq.decide(item.id, "wait").unwrap();
    hq.tick().unwrap();
    assert_eq!(
        project(&hq, p).status,
        ProjectStatus::Paused,
        "still October"
    );
    clock.0.fetch_add(15 * DAY_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    let november = project(&hq, p);
    assert_eq!(
        (november.status, november.pause),
        (ProjectStatus::Active, None)
    );
}

#[test]
fn a_pause_the_owner_made_holds_every_turn_and_never_lifts_itself() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq.add_project(t.goal, "Upkeep", "", t.lead, None).unwrap();
    hq.change_project(p, "pause").unwrap();
    let task = task_in(&hq, t.goal, Some(p), t.dev, "Read the logs").unwrap();
    assert_eq!(
        last_wake(&hq),
        (
            t.dev,
            Some(task),
            WakeReason::Assigned,
            Receipt::Refused(Refused::ProjectPaused)
        ),
        "a local member waits too"
    );
    clock.0.fetch_add(20 * DAY_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(project(&hq, p).status, ProjectStatus::Paused, "a month on");
    assert!(hq.runner().started().is_empty());
    assert_eq!(hq.change_project(p, "resume"), Ok(ProjectStatus::Active));
    assert_eq!(
        hq.change_project(p, "resume"),
        Err(EngineError::Refused(Refusal::NotAProjectStep {
            from: "active",
            asked: "resume".to_owned()
        }))
    );
}

#[test]
fn a_member_reports_at_hire_the_owner_changes_it_and_never_into_a_circle() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    // Hired before the CEO: nobody to report to yet.
    let early = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    let second = hq.hire("Grace", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 0).unwrap();
    let reports = |id: MemberId| {
        hq.store()
            .members()
            .unwrap()
            .into_iter()
            .find(|m| m.id == id)
            .unwrap()
            .reports_to
    };
    assert_eq!(reports(early), None);
    assert_eq!(reports(second), Some(ceo));
    assert_eq!(reports(dev), Some(early), "the earliest lead");
    assert_eq!(reports(ceo), None, "the board");
    assert_eq!(hq.backfill_reports_to(), Ok(1));
    assert_eq!(reports(early), Some(ceo));
    assert_eq!(hq.backfill_reports_to(), Ok(0), "once");
    // The owner moves the developer to the second lead, then the second lead under the first.
    hq.set_reports_to(dev, second).unwrap();
    hq.set_reports_to(second, early).unwrap();
    assert_eq!(
        hq.set_reports_to(early, dev),
        Err(EngineError::Refused(Refusal::NotAManager)),
        "Linus reports to Grace, who reports to Ada"
    );
    assert_eq!(
        hq.set_reports_to(dev, dev),
        Err(EngineError::Refused(Refusal::NotAManager))
    );
    assert_eq!(
        hq.set_reports_to(ceo, early),
        Err(EngineError::Refused(Refusal::NotAManager))
    );
    let shown = hq.snapshot().unwrap();
    assert_eq!(shown["members"][3]["reports_to"], second.0);
}

#[test]
fn a_question_goes_to_whom_the_member_reports_and_past_one_who_is_paused() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let second = hq.hire("Grace", Role::Lead, 25, 2, 0).unwrap();
    hq.set_reports_to(t.dev, second).unwrap();
    let ask = |title: &str| {
        let task = task_in(&hq, t.goal, None, t.dev, title).unwrap();
        let context = turn(&hq, t.dev);
        assert_eq!(context.task, Some(task));
        hq.ask_owner(context.run, "May I restart it?").unwrap()
    };
    assert_eq!(ask("Fix sonarr"), Asked::Up(second));
    hq.pause(second, true).unwrap();
    assert_eq!(ask("Fix radarr"), Asked::Up(t.ceo), "Grace reports to Mara");
}

#[test]
fn a_task_handed_back_wakes_its_projects_lead_only() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let second = hq.hire("Grace", Role::Lead, 25, 2, 0).unwrap();
    let p = hq.add_project(t.goal, "Media", "", second, None).unwrap();
    let back = |project: Option<ProjectId>, title: &str| {
        let task = task_in(&hq, t.goal, project, t.dev, title).unwrap();
        let context = turn(&hq, t.dev);
        use_tool(&hq, &context, "claim", &json!({ "task": task.0 })).unwrap();
        use_tool(
            &hq,
            &context,
            "set_status",
            &json!({ "task": task.0, "status": "todo", "reason": "not mine" }),
        )
        .unwrap();
        hq.runner().finish(context.run, true);
        let woken: Vec<MemberId> = hq
            .store()
            .wakes()
            .unwrap()
            .into_iter()
            .filter(|w| w.task == Some(task) && w.reason == WakeReason::Assigned)
            .filter(|w| !matches!(w.receipt, Receipt::Refused(_)))
            .map(|w| w.member)
            .filter(|m| *m != t.dev)
            .collect();
        hq.tick().unwrap();
        woken
    };
    assert_eq!(back(Some(p), "Fix the player"), [second]);
    assert_eq!(back(None, "Fix the mail"), [t.lead, second], "every lead");
    hq.pause(second, true).unwrap();
    assert_eq!(
        back(Some(p), "Fix the tuner"),
        [t.lead],
        "its lead is paused"
    );
}

#[test]
fn a_leads_briefing_lists_its_projects_with_their_money() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let p = hq
        .add_project(t.goal, "Upkeep", "", t.lead, Some(10 * DOLLAR))
        .unwrap();
    task_in(&hq, t.goal, Some(p), t.lead, "Look at sonarr").unwrap();
    let context = turn(&hq, t.lead);
    assert!(
        context.told.contains(
            "Your projects (you lead them; the sub-tasks of their tasks stay in them; \
             propose_project for one more, with evidence):\n- #1 Upkeep (active, led by Ada): \
             $0.00 of $10.00 this month\n"
        ),
        "{}",
        context.told
    );
}

#[test]
fn a_proposal_needs_a_lead_or_the_ceo_and_evidence_in_hqs_record() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    assert!(
        !tools(Role::Developer)
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "propose_project")
    );
    let names = |role: Role| -> Vec<String> {
        tools(role)
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_owned))
            .collect()
    };
    for (role, offered) in [
        (Role::Lead, ["propose_project"].as_slice()),
        (Role::Ceo, ["decide_project", "propose_goal"].as_slice()),
    ] {
        for tool in offered {
            assert!(names(role).iter().any(|n| n == tool), "{role:?} {tool}");
        }
    }
    assert!(!names(Role::Lead).iter().any(|n| n == "decide_project"));
    // Leads propose; the CEO decides (ADR 0120).
    assert!(!names(Role::Ceo).iter().any(|n| n == "propose_project"));
    let task = task_in(&hq, t.goal, None, t.dev, "Fix sonarr").unwrap();
    let dev = turn(&hq, t.dev);
    let args = json!({ "title": "Media stack", "reason": "it keeps failing", "tasks": [task.0] });
    assert_eq!(
        use_tool(&hq, &dev, "propose_project", &args),
        Err(Refusal::OnlyLeadsPropose.to_string()),
        "the rules refuse a tool nobody offered"
    );
    hq.runner().finish(dev.run, true);
    hq.tick().unwrap();
    let mine = task_in(&hq, t.goal, None, t.lead, "Look over the lab").unwrap();
    let lead = turn(&hq, t.lead);
    assert_eq!(lead.task, Some(mine));
    assert_eq!(
        use_tool(
            &hq,
            &lead,
            "propose_project",
            &json!({ "title": "Media stack", "reason": "it keeps failing" })
        ),
        Err(Refusal::NoEvidence.to_string())
    );
    assert_eq!(
        use_tool(
            &hq,
            &lead,
            "propose_project",
            &json!({ "title": "Media stack", "reason": "it keeps failing", "tasks": [task.0], "events": [99_999] })
        ),
        Err("event 99999 is not in HQ's record".to_owned())
    );
    assert_eq!(
        use_tool(
            &hq,
            &lead,
            "propose_project",
            &json!({ "title": "Media stack", "reason": "it keeps failing", "tasks": [task.0], "lead": "Linus" })
        ),
        Err(Refusal::NotAProjectLead.to_string())
    );
    let said = use_tool(&hq, &lead, "propose_project", &args).unwrap();
    assert!(said.starts_with("Project 1 is proposed"), "{said}");
    let proposed = project(&hq, ProjectId(1));
    assert_eq!(
        (proposed.status, proposed.lead, proposed.proposed_in),
        (ProjectStatus::Proposed, t.lead, Some(mine))
    );
    assert_eq!(proposed.evidence, [crate::domain::Evidence::Task(task)]);
    // Only the CEO decides it.
    assert_eq!(
        hq.decide_project(lead.run, ProjectId(1), true, "yes", None, &[]),
        Err(EngineError::Refused(Refusal::OnlyTheCeo))
    );
}

#[test]
fn the_ceo_accepts_or_sends_back_a_proposal_and_its_proposer_hears_why() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let mine = task_in(&hq, t.goal, None, t.lead, "Look over the lab").unwrap();
    let lead = turn(&hq, t.lead);
    for title in ["Media stack", "Backups"] {
        use_tool(
            &hq,
            &lead,
            "propose_project",
            &json!({ "title": title, "reason": "it keeps failing", "tasks": [mine.0],
                     "cap": if title == "Backups" { json!(5) } else { json!(null) } }),
        )
        .unwrap();
    }
    hq.runner().finish(lead.run, true);
    hq.tick().unwrap();
    let asked = task_in(&hq, t.goal, None, t.ceo, "Weigh the proposals").unwrap();
    let ceo = turn(&hq, t.ceo);
    assert_eq!(ceo.task, Some(asked));
    assert!(
        ceo.told
            .contains("Project proposals waiting for your decision: 2"),
        "{}",
        ceo.told
    );
    // The CEO decides on evidence from HQ's record, as the proposal was made (ADR 0114).
    assert!(
        use_tool(
            &hq,
            &ceo,
            "decide_project",
            &json!({ "project": 1, "verdict": "send_back", "why": "one task is no pattern" })
        )
        .unwrap_err()
        .contains("needs evidence")
    );
    assert_eq!(
        use_tool(
            &hq,
            &ceo,
            "decide_project",
            &json!({ "project": 1, "verdict": "send_back", "why": "one task is no pattern",
                     "evidence": [format!("task {}", mine.0)] })
        ),
        Ok("Project 1 is dropped.".to_owned())
    );
    assert_eq!(
        use_tool(
            &hq,
            &ceo,
            "decide_project",
            &json!({ "project": 2, "verdict": "accept", "why": "backups failed thrice",
                     "evidence": [format!("task {}", mine.0)] })
        ),
        Ok("Project 2 is active.".to_owned())
    );
    let thread: Vec<String> = hq
        .store()
        .thread(mine)
        .unwrap()
        .into_iter()
        .map(|p| format!("{}: {}", p.author, p.text))
        .collect();
    assert!(
        thread.contains(&format!(
            "Mara: Project #1 Media stack: sent back. one task is no pattern (evidence: task {})",
            mine.0
        )) && thread.contains(&format!(
            "Mara: Project #2 Backups: accepted. backups failed thrice (evidence: task {})",
            mine.0
        )),
        "{thread:?}"
    );
    assert_eq!(
        last_wake(&hq).2,
        WakeReason::ProposalDecided,
        "the proposer woken"
    );
    // The accepted project's lead has its first task in it; its cap waits for the owner.
    let first = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.project == Some(ProjectId(2)));
    assert_eq!(first.map(|f| f.assignee), Some(Some(t.lead)));
    let backups = project(&hq, ProjectId(2));
    assert_eq!((backups.cap, backups.asked_cap), (None, Some(5 * DOLLAR)));
    let cap = waiting(&hq, "project_cap").remove(0);
    assert_eq!(
        cap.text,
        format!(
            "A cap for project #2 Backups: $5.00 a month.\nThe CEO recommends: backups failed \
             thrice (evidence: task {})",
            mine.0
        )
    );
    assert_eq!(cap.member, Some(t.ceo));
    // The owner grants less than asked.
    hq.decide_saying(cap.id, "approve", "4").unwrap();
    let backups = project(&hq, ProjectId(2));
    assert_eq!((backups.cap, backups.asked_cap), (Some(4 * DOLLAR), None));
    // Decided once.
    assert_eq!(
        use_tool(
            &hq,
            &ceo,
            "decide_project",
            &json!({ "project": 2, "verdict": "accept", "why": "again",
                     "evidence": [format!("task {}", mine.0)] })
        ),
        Err("no such proposed project with that number".to_owned())
    );
}

#[test]
fn the_ceo_proposes_a_goal_to_the_board_with_its_evidence() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let asked = task_in(&hq, t.goal, None, t.ceo, "Plan").unwrap();
    let ceo = turn(&hq, t.ceo);
    let alarm = hq.store().last_event().unwrap();
    assert_eq!(
        use_tool(
            &hq,
            &ceo,
            "propose_goal",
            &json!({ "title": "Keep the media stack up", "reason": "it fails weekly", "tasks": [asked.0], "events": [alarm] })
        ),
        Ok("Goal 2 is proposed: it waits on the board's desk.".to_owned())
    );
    let item = waiting(&hq, "goal").remove(0);
    assert_eq!(
        item.text,
        format!(
            "A goal: Keep the media stack up\nWhy: it fails weekly\nEvidence: task {}, event {alarm}",
            asked.0
        )
    );
    hq.decide(item.id, "approve").unwrap();
    assert_eq!(
        hq.store().goal(GoalId(2)).unwrap().unwrap().status,
        crate::domain::GoalStatus::Active
    );
}

#[test]
fn the_ceos_planning_is_made_once_and_shows_the_patterns_to_propose_from() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let other = hq.set_goal("Ship the tracker", "", Author::Owner).unwrap();
    hq.set_lab_names(
        ["deluge", "nzbget", "prowlarr", "sonarr"]
            .map(str::to_owned)
            .to_vec(),
    );
    let lab = hq
        .add_routine(
            Author::Owner,
            NewRoutine {
                title: "Check the home lab".to_owned(),
                description: String::new(),
                goal: t.goal,
                project: None,
                assignee: t.lead,
                every_ms: 6 * HOUR_MS,
                on_lab_trouble: true,
            },
        )
        .unwrap();
    let upkeep = hq.migrate_projects().unwrap().expect("made");
    assert_eq!(hq.migrate_projects(), Ok(None), "once");
    // Three containers down twice, and their tasks.
    for _ in 0..2 {
        hq.alarm("deluge became exited").unwrap();
    }
    for title in ["Fix deluge and nzbget", "deluge and nzbget again"] {
        task_in(&hq, t.goal, Some(upkeep), t.dev, title).unwrap();
    }
    task_in(&hq, t.goal, None, t.dev, "Restart prowlarr").unwrap();
    let made = hq.seed_planning(t.goal).unwrap().expect("made");
    assert_eq!(hq.seed_planning(t.goal), Ok(None), "once");
    let planning = hq
        .store()
        .routines()
        .unwrap()
        .into_iter()
        .find(|r| r.id == made)
        .unwrap();
    assert_eq!(
        (
            planning.title.as_str(),
            planning.assignee,
            planning.every_ms
        ),
        ("Planning", t.ceo, 7 * DAY_MS)
    );
    assert_eq!(planning.description, PLANNING);
    assert_ne!(planning.id, lab);
    hq.fire_routine(made).unwrap();
    let ceo = turn(&hq, t.ceo);
    let alarms: Vec<i64> = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| {
            (e.kind == "routine.fired" && e.data["firing"] == "alarm") || e.kind == "routine.posted"
        })
        .map(|e| e.id)
        .collect();
    for said in [
        "This is your weekly planning",
        "- #1 Keep the home lab healthy (active)\n  - #1 Home lab upkeep (active, led by Ada): $0.00 this month, no cap\n",
        "- deluge, nzbget: 2 tasks (#2, #3), 2 alarms\n",
        "- prowlarr: 1 task (#4), 0 alarms\n",
        &format!("- deluge: 2 alarms (events {}, {})\n", alarms[0], alarms[1]),
        "Project proposals waiting for your decision:\n- none\n",
        &format!("Goals with no project: #{} Ship the tracker.", other.0),
    ] {
        assert!(ceo.told.contains(said), "{said}\n---\n{}", ceo.told);
    }
    for gone in ["Open tasks:", "The board's desk in the last 7 days"] {
        assert!(!ceo.told.contains(gone), "lean: {gone}\n---\n{}", ceo.told);
    }
}

#[test]
fn a_question_on_a_projects_task_goes_to_the_projects_lead() {
    // The live shape: the tester reports to Ada, and asked about a task of Kit's project.
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let kit = hq.hire("Kit", Role::Lead, 25, 2, 0).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 0).unwrap();
    hq.set_reports_to(dee, t.lead).unwrap();
    let wing = hq
        .add_project(t.goal, "The wing", "the lab's code", kit, None)
        .unwrap();
    let ask = |member: MemberId, task: TaskId| {
        let context = turn(&hq, member);
        assert_eq!(context.task, Some(task));
        use_tool(
            &hq,
            &context,
            "ask_owner",
            &json!({ "text": "Which branch is this on?", "evidence": [format!("task {}", task.0)] }),
        )
        .unwrap();
        context
    };
    let in_wing = task_in(&hq, t.goal, Some(wing), dee, "Test the deluge role").unwrap();
    let context = ask(dee, in_wing);
    assert!(
        context
            .told
            .contains("Your questions (ask_owner) go to Kit, the lead, in the task's thread"),
        "{}",
        context.told
    );
    assert_eq!(
        last_wake(&hq),
        (
            kit,
            Some(in_wing),
            WakeReason::QuestionAsked,
            Receipt::Queued
        ),
        "to the project's lead, not the one she reports to"
    );
    // Off any project, up her own chain as before.
    let elsewhere = task_in(&hq, t.goal, None, dee, "Test the backups").unwrap();
    ask(dee, elsewhere);
    assert_eq!(
        last_wake(&hq),
        (
            t.lead,
            Some(elsewhere),
            WakeReason::QuestionAsked,
            Receipt::Queued
        )
    );
    // The project's lead asking about its own work asks up its chain: the CEO.
    let his = task_in(&hq, t.goal, Some(wing), kit, "Plan the wing").unwrap();
    hq.tick().unwrap();
    ask(kit, his);
    assert_eq!(
        last_wake(&hq),
        (t.ceo, Some(his), WakeReason::QuestionAsked, Receipt::Queued)
    );
}
