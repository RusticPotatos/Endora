//! Evidence, acceptance criteria and the tester (ADR 0114): receipts written only by the node
//! for a running turn, done only on evidence per criterion (a live one from a fresh reading),
//! the home lab's standard criteria, unsupported posts, turn labels and what follows them, the
//! cap on review rounds, check-backs that wait, and the chain of command acting on a
//! subordinate's task.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Called, Engine, EngineError, Settings};
use super::ports::{Clock, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::evidence::NUDGE;
use crate::domain::routines::HOUR_MS;
use crate::domain::{
    Criterion, GoalId, Label, MemberId, Priority, Refusal, Role, RunId, RunStatus, TaskId,
    TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const OCT_17: i64 = 1_792_166_400_000;

struct Team {
    ceo: Option<MemberId>,
    lead: MemberId,
    dev: MemberId,
    tester: Option<MemberId>,
    goal: GoalId,
}

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    )
}

/// A team under the home lab's goal: the CEO when `ceo`, Ada the lead, Linus the developer and
/// Grace the tester when `tester`.
fn team(hq: &Hq<'_>, ceo: bool, tester: bool) -> Team {
    let ceo = ceo.then(|| hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap());
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let tester = tester.then(|| hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap());
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    Team {
        ceo,
        lead,
        dev,
        tester,
        goal,
    }
}

/// The home lab's routine, for Ada: its goal's work changes no code, so needs no gate.
fn lab_routine(hq: &Hq<'_>, t: &Team) {
    hq.add_routine(
        Author::Endora,
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
}

fn owners_task(hq: &Hq<'_>, t: &Team, to: MemberId, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: None,
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

/// The newest turn of `member`'s, after a tick or two to start it. A tester's work is the Mac's
/// (ADR 0131), so the node says it is free.
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

/// Ends a turn as its worker would, and lets the engine see it.
fn end(hq: &Hq<'_>, clock: &Hands, context: &RunContext) {
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    clock.0.fetch_add(1_000, Ordering::SeqCst);
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

/// A call of Endora's tools the node recorded for the turn: its receipt's id.
fn reading(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &str, act: bool) -> i64 {
    hq.record_receipt(
        &context.token,
        &Called {
            tool: tool.to_owned(),
            args: args.to_owned(),
            excerpt: "sonarr: running, healthy".to_owned(),
            ok: true,
            act,
            confidential: false,
        },
    )
    .unwrap()
}

fn label(hq: &Hq<'_>, run: RunId) -> Option<Label> {
    hq.store().run(run).unwrap().unwrap().label
}

fn last_wake(hq: &Hq<'_>) -> (MemberId, Option<TaskId>, WakeReason) {
    let w = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    (w.member, w.task, w.reason)
}

fn thread(hq: &Hq<'_>, task: TaskId) -> Vec<String> {
    hq.store()
        .thread(task)
        .unwrap()
        .into_iter()
        .map(|p| p.text)
        .collect()
}

#[test]
fn receipts_are_written_only_for_a_turn_that_is_running_known_by_its_token() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    let called = Called {
        tool: "homelab_status".to_owned(),
        args: String::new(),
        excerpt: "x".repeat(1_000),
        ok: true,
        act: false,
        confidential: false,
    };
    assert!(hq.record_receipt("forged", &called).is_err());
    let id = hq.record_receipt(&linus.token, &called).unwrap();
    let kept = hq.store().call_receipt(id).unwrap().unwrap();
    assert_eq!(
        (kept.run, kept.member, kept.task),
        (linus.run, t.dev, Some(task))
    );
    assert_eq!(
        kept.excerpt.chars().count(),
        300,
        "an excerpt, not the whole"
    );
    end(&hq, &clock, &linus);
    assert!(
        hq.record_receipt(&linus.token, &called).is_err(),
        "a finished turn records nothing"
    );
}

#[test]
fn a_sandbox_report_is_cited_for_what_the_sandbox_did_but_never_for_the_lab_now() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let parent = owners_task(&hq, &t, t.lead, "Keep sonarr up");
    let ada = turn(&hq, t.lead);
    use_tool(&hq, &ada, "claim", &json!({ "task": parent.0 })).unwrap();
    use_tool(
        &hq,
        &ada,
        "create_task",
        &json!({ "title": "Tidy sonarr's role", "description": "and check it is up",
                 "assignee": "Linus", "criteria": [
                    { "text": "the role's tasks are named" },
                    { "text": "sonarr is running and healthy", "live": true }] }),
    )
    .unwrap();
    let sub = hq.store().tasks().unwrap().pop().unwrap();
    end(&hq, &clock, &ada);
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": sub.id.0 })).unwrap();
    let edit = hq
        .record_sandbox_receipt(
            &linus.token,
            &Called {
                tool: "Edit".to_owned(),
                args: "roles/sonarr/tasks/main.yml".to_owned(),
                excerpt: "edited".to_owned(),
                ok: true,
                act: true,
                confidential: false,
            },
        )
        .unwrap();
    let curl = hq
        .record_sandbox_receipt(
            &linus.token,
            &Called {
                tool: "Bash".to_owned(),
                args: "curl sonarr".to_owned(),
                excerpt: "sonarr: running, healthy".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    let kept = hq.store().call_receipt(edit).unwrap().unwrap();
    assert!(kept.sandbox && kept.line(true).contains("sandbox-reported"));
    let submit = |evidence: Value| {
        use_tool(
            &hq,
            &linus,
            "set_status",
            &json!({ "task": sub.id.0, "status": "in_review", "reason": "tidied",
                     "evidence": evidence }),
        )
    };
    let said = submit(json!([[format!("r{edit}")], [format!("r{curl}")]])).unwrap_err();
    assert!(said.contains("is about the lab now"), "{said}");
    let fresh = reading(&hq, &linus, "homelab_status", "", false);
    submit(json!([
        [format!("r{edit}")],
        [format!("r{curl}"), format!("r{fresh}")]
    ]))
    .unwrap();
    assert_eq!(
        hq.store().task(sub.id).unwrap().unwrap().status,
        TaskStatus::InReview
    );
}

#[test]
fn work_is_handed_on_with_criteria_and_submitted_only_with_evidence_for_each() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let parent = owners_task(&hq, &t, t.lead, "Keep sonarr up");
    let ada = turn(&hq, t.lead);
    use_tool(&hq, &ada, "claim", &json!({ "task": parent.0 })).unwrap();
    let hand = |criteria: Value| {
        use_tool(
            &hq,
            &ada,
            "create_task",
            &json!({ "title": "Find why sonarr stalls", "description": "and fix it",
                     "assignee": "Linus", "criteria": criteria }),
        )
    };
    assert_eq!(
        hand(json!([])).unwrap_err(),
        Refusal::NoCriteria.to_string()
    );
    hand(json!([{ "text": "the cause is in the thread" },
                { "text": "sonarr is running and healthy", "live": true }]))
    .unwrap();
    let sub = hq.store().tasks().unwrap().pop().unwrap();
    assert_eq!(
        sub.criteria,
        [
            Criterion::new("the cause is in the thread", false),
            Criterion::new("sonarr is running and healthy", true)
        ]
    );
    end(&hq, &clock, &ada);

    // Linus reads the lab in one turn, and submits in the next.
    let first = turn(&hq, t.dev);
    assert!(first.told.contains("What done means"), "{}", first.told);
    assert!(
        first
            .told
            .contains("2. sonarr is running and healthy [live")
    );
    use_tool(&hq, &first, "claim", &json!({ "task": sub.id.0 })).unwrap();
    let older = reading(&hq, &first, "homelab_status", "", false);
    end(&hq, &clock, &first);
    hq.post(Author::Owner, sub.id, "Go on.").unwrap();
    let linus = turn(&hq, t.dev);
    assert_ne!(linus.run, first.run);
    let submit = |evidence: Value| {
        use_tool(
            &hq,
            &linus,
            "set_status",
            &json!({ "task": sub.id.0, "status": "in_review", "reason": "found it",
                     "evidence": evidence }),
        )
    };
    let said = submit(json!([])).unwrap_err();
    assert!(
        said.contains("criterion 1 (the cause is in the thread) has no evidence"),
        "{said}"
    );
    let said = submit(json!([format!("task {}", sub.id.0)])).unwrap_err();
    assert!(
        said.contains("criterion 2 (sonarr is running and healthy)"),
        "{said}"
    );
    // A live criterion: an older turn's reading is memory, not the lab now.
    let said = submit(json!([format!("task {}", sub.id.0), format!("r{older}")])).unwrap_err();
    assert!(said.contains("is about the lab now"), "{said}");
    assert_eq!(
        submit(json!([format!("task {}", sub.id.0), "r999"])).unwrap_err(),
        "r999 is not in HQ's record"
    );
    assert_eq!(
        submit(json!([format!("task {}", parent.0), format!("r{older}")])).unwrap_err(),
        format!(
            "task {} is not from this task's work: cite this work's own",
            parent.0
        )
    );
    let fresh = reading(&hq, &linus, "homelab_status", "", false);
    submit(json!([
        [format!("task {}", sub.id.0)],
        [format!("r{older}"), format!("r{fresh}")]
    ]))
    .unwrap();
    assert_eq!(
        hq.store().task(sub.id).unwrap().unwrap().status,
        TaskStatus::InReview
    );
    assert_eq!(
        last_wake(&hq).2,
        WakeReason::ReviewRequested,
        "the tester is woken"
    );
    let cited = hq.store().cited(sub.id).unwrap();
    assert_eq!(
        cited.iter().map(|c| c.criterion).collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(
        thread(&hq, sub.id)
            .iter()
            .any(|p| p.contains(&format!("r{fresh} (homelab_status)"))),
    );
}

/// The work the owner hands out, as the Give work form and the API make it.
fn owners_ask(
    hq: &Hq<'_>,
    t: &Team,
    to: MemberId,
    criteria: &[Criterion],
) -> Result<TaskId, EngineError> {
    hq.add_task_with(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: None,
            parent: None,
            title: "Remove literal passwords from the role defaults".to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
        criteria,
    )
}

/// Live, 19 October: the owner's "no literal passwords in role defaults" went to a lead with no
/// definition of done and split into three blocked sub-tasks, each defensible and none tied to
/// what was asked. The owner's work says what done means now (ADR 0124, amended): at least one
/// line, or it is refused with the way to give one, and nothing is made or woken.
#[test]
fn the_owner_hands_out_work_only_with_what_done_means() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    assert_eq!(
        owners_ask(&hq, &t, t.lead, &[]),
        Err(EngineError::Refused(Refusal::OwnerNeedsCriteria))
    );
    assert!(hq.store().tasks().unwrap().is_empty(), "nothing was made");
    assert!(hq.store().wakes().unwrap().is_empty(), "nobody was woken");

    let many = vec![Criterion::new("x", false); crate::domain::evidence::MOST_CRITERIA + 1];
    assert_eq!(
        owners_ask(&hq, &t, t.lead, &many),
        Err(EngineError::Refused(Refusal::TooLong(
            crate::domain::evidence::MOST_CRITERIA
        )))
    );

    let ask = [
        Criterion::new("No role default holds a password", false),
        Criterion::new("Each role reads its secret from the vault", false),
    ];
    let made = owners_ask(&hq, &t, t.lead, &ask).unwrap();
    assert_eq!(hq.store().task(made).unwrap().unwrap().criteria, ask);
    assert_eq!(
        last_wake(&hq),
        (t.lead, Some(made), WakeReason::Assigned),
        "and the lead is woken for it as ever"
    );
}

/// Work already on the board with no criteria is untouched: nothing rewrites its row, and its
/// lead may give it some with `narrow_criteria`, a task with none being allowed some (ADR 0133).
#[test]
fn work_already_open_with_no_criteria_is_untouched_and_its_lead_may_give_it_some() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let legacy = owners_task(&hq, &t, t.lead, "Keep sonarr up");
    assert!(
        hq.store()
            .task(legacy)
            .unwrap()
            .unwrap()
            .criteria
            .is_empty()
    );
    let ada = turn(&hq, t.lead);
    use_tool(&hq, &ada, "claim", &json!({ "task": legacy.0 })).unwrap();
    use_tool(
        &hq,
        &ada,
        "narrow_criteria",
        &json!({ "task": legacy.0, "criteria": [{ "text": "sonarr has been up for a day" }] }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(legacy).unwrap().unwrap().criteria,
        [Criterion::new("sonarr has been up for a day", false)]
    );
}

/// A routine's task says what done means as its routine does (ADR 0124, amended): the home lab's
/// check, the two live criteria; any other routine of the owner's, a reading of what it is about,
/// cited. A lead's standing reviews and the CEO's are reports and decisions, closed by filing
/// them: they carry none, so closing one stays as it was.
#[test]
fn a_routines_task_carries_criteria_from_its_routine_but_a_standing_review_keeps_none() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, true, false);
    let routine = |title: &str, to: MemberId, lab: bool| {
        let id = hq
            .add_routine(
                Author::Owner,
                NewRoutine {
                    title: title.to_owned(),
                    description: String::new(),
                    goal: t.goal,
                    project: None,
                    assignee: to,
                    every_ms: 6 * HOUR_MS,
                    on_lab_trouble: lab,
                },
            )
            .unwrap();
        hq.fire_routine(id).unwrap();
        let task = hq
            .store()
            .routines()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .and_then(|r| r.last_task)
            .expect("the routine made a task");
        hq.store().task(task).unwrap().unwrap().criteria
    };
    assert_eq!(
        routine("Check the home lab", t.lead, true),
        Criterion::lab()
    );
    let others = routine("Look at the download queues", t.dev, false);
    assert_eq!(others.len(), 1, "{others:?}");
    assert!(
        others[0].text.contains("Look at the download queues") && others[0].text.contains("cited"),
        "{others:?}"
    );
    assert!(!others[0].live, "a routine need not be about the lab");
    // The CEO's review, and a lead's report and patterns, are not work to check against a
    // criterion: they close by being filed or decided.
    assert!(routine("Daily review", t.ceo.unwrap(), false).is_empty());
    hq.add_project(t.goal, "Home lab upkeep", "it runs the house", t.lead, None)
        .unwrap();
    for id in hq.seed_area_routines().unwrap() {
        hq.fire_routine(id).unwrap();
        let task = hq
            .store()
            .routines()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .and_then(|r| r.last_task)
            .expect("the lead's routine made a task");
        assert!(
            hq.store().task(task).unwrap().unwrap().criteria.is_empty(),
            "a lead's standing review carries no criterion"
        );
    }
}

/// Closing the task of an owner's routine takes the reading it asks for, cited: the default
/// criterion is not decoration. A reading closes it, and the routine can come round again.
#[test]
fn an_owners_routine_task_is_closed_on_a_reading_of_what_it_is_about_cited() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let id = hq
        .add_routine(
            Author::Owner,
            NewRoutine {
                title: "Look at the download queues".to_owned(),
                description: "Is anything stuck?".to_owned(),
                goal: t.goal,
                project: None,
                assignee: t.dev,
                every_ms: 6 * HOUR_MS,
                on_lab_trouble: false,
            },
        )
        .unwrap();
    hq.fire_routine(id).unwrap();
    let task = hq.store().routines().unwrap()[0].last_task.unwrap();
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    let close = |evidence: Value| {
        use_tool(
            &hq,
            &linus,
            "set_status",
            &json!({ "task": task.0, "status": "done", "reason": "nothing is stuck",
                     "evidence": evidence }),
        )
    };
    let refused = close(json!([])).unwrap_err();
    assert!(
        refused.contains(
            "criterion 1 (A reading of \"Look at the download queues\" from this run, cited) has no evidence"
        ),
        "{refused}"
    );
    let read = reading(&hq, &linus, "sonarr_queue", "", false);
    close(json!([format!("r{read}")])).unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    // It is the routine's one open task that is closed, so the routine fires again.
    clock.0.fetch_add(6 * HOUR_MS, Ordering::SeqCst);
    assert_eq!(hq.tick().unwrap().routines, 1);
}

#[test]
fn the_home_labs_routine_and_alarm_tasks_say_what_done_means_and_close_on_a_fresh_reading() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let routine = hq
        .add_routine(
            Author::Endora,
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
    hq.alarm("sonarr became unhealthy at 11:58").unwrap();
    let task = hq.store().routines().unwrap()[0].last_task.unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().criteria,
        Criterion::lab()
    );
    let _ = routine;
    let ada = turn(&hq, t.lead);
    use_tool(&hq, &ada, "claim", &json!({ "task": task.0 })).unwrap();
    let close = |evidence: Value| {
        use_tool(
            &hq,
            &ada,
            "set_status",
            &json!({ "task": task.0, "status": "done", "reason": "recovered",
                     "evidence": evidence }),
        )
    };
    assert!(close(json!([])).unwrap_err().contains("criterion 1"));
    // Live, #667: closed two minutes after the restart, and the alarm came back. "No new
    // alarm since" waits an hour from the work's last change: here, the restart.
    clock.0.fetch_add(60_000, Ordering::SeqCst);
    reading(&hq, &ada, "restart_container", "container: sonarr", true);
    clock.0.fetch_add(2 * 60_000, Ordering::SeqCst);
    let now = reading(&hq, &ada, "homelab_status", "", false);
    let restarted = OCT_17 + 60_000;
    let refused = close(json!([format!("r{now}"), format!("r{now}")])).unwrap_err();
    assert_eq!(
        refused,
        Refusal::TooSoon {
            criterion: 2,
            text: crate::domain::evidence::LAB_CRITERIA[1].to_owned(),
            until: crate::domain::evidence::clock_time(restarted + 60 * 60_000, 0),
        }
        .to_string()
    );
    assert!(
        ada.told
            .contains("shown only 60 minutes after the work's last change")
    );
    clock.0.fetch_add(58 * 60_000, Ordering::SeqCst);
    let later = reading(&hq, &ada, "homelab_status", "", false);
    close(json!([format!("r{later}"), format!("r{later}")])).unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
}

#[test]
fn a_post_that_cites_nothing_in_a_turn_with_no_reading_is_marked_unsupported() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    let post = |text: &str, evidence: Value| {
        use_tool(
            &hq,
            &linus,
            "post",
            &json!({ "task": task.0, "text": text, "evidence": evidence }),
        )
    };
    post("sonarr is fine, I think.", json!([])).unwrap();
    post("As the task says.", json!([format!("task {}", task.0)])).unwrap();
    assert_eq!(
        post("See this.", json!(["r404"])).unwrap_err(),
        "r404 is not in HQ's record"
    );
    reading(&hq, &linus, "homelab_status", "", false);
    post("Read it: sonarr is running.", json!([])).unwrap();
    let marks: Vec<bool> = hq
        .store()
        .thread(task)
        .unwrap()
        .iter()
        .map(|p| p.unsupported)
        .collect();
    assert_eq!(marks, [true, false, false]);
    let thread = use_tool(&hq, &linus, "my_turn", &json!({})).unwrap();
    assert!(
        thread.contains(
            "- Linus [unsupported]: sonarr is fine, I think.\n- Linus: As the task says."
        ),
        "{thread}"
    );
}

#[test]
fn a_post_in_a_turn_with_receipts_and_the_last_word_are_never_unsupported() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    // The worker's own last word, in a turn with no receipt: it is not a claim of the turn's.
    let linus = turn(&hq, t.dev);
    hq.last_word_in(linus.run, task, "I looked at the task.")
        .unwrap();
    // A turn with a receipt (here one the node refused) is never unsupported: R1599 had two.
    hq.record_receipt(
        &linus.token,
        &Called {
            tool: "container_logs".to_owned(),
            args: "sonarr".to_owned(),
            excerpt: "no such container".to_owned(),
            ok: false,
            act: false,
            confidential: false,
        },
    )
    .unwrap();
    use_tool(
        &hq,
        &linus,
        "post",
        &json!({ "task": task.0, "text": "The logs say nothing.", "evidence": [] }),
    )
    .unwrap();
    hq.last_word_in(linus.run, task, "Closing: sonarr is fine.")
        .unwrap();
    let marks: Vec<bool> = hq
        .store()
        .thread(task)
        .unwrap()
        .iter()
        .map(|p| p.unsupported)
        .collect();
    assert_eq!(marks, [false, false, false]);
}

#[test]
fn each_turns_label_comes_from_what_it_did() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let mut labels = Vec::new();
    for step in ["claim", "talk", "nothing", "read", "act", "block"] {
        if step != "claim" {
            hq.post(Author::Owner, task, "Next.").unwrap();
        }
        let linus = turn(&hq, t.dev);
        match step {
            "claim" => {
                use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
            }
            "talk" => {
                use_tool(
                    &hq,
                    &linus,
                    "post",
                    &json!({ "task": task.0, "text": "Hm." }),
                )
                .unwrap();
            }
            "read" => {
                reading(&hq, &linus, "homelab_status", "", false);
            }
            "act" => {
                reading(&hq, &linus, "restart_container", "container: sonarr", true);
            }
            "block" => {
                use_tool(
                    &hq,
                    &linus,
                    "ask_owner",
                    &json!({ "text": "May I have the indexer's key?" }),
                )
                .unwrap();
            }
            _ => {}
        }
        end(&hq, &clock, &linus);
        labels.push(label(&hq, linus.run));
    }
    assert_eq!(
        labels,
        [
            Some(Label::Advanced),
            Some(Label::TalkOnly),
            Some(Label::Empty),
            Some(Label::Checked),
            Some(Label::Advanced),
            Some(Label::Blocked),
        ]
    );
    let ended = hq
        .events_after(0)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "run.ended")
        .unwrap();
    assert_eq!(ended.data["label"], "blocked", "the activity log has it");
}

#[test]
fn asking_up_the_chain_is_blocked_and_the_testers_send_back_is_a_finished_test() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, true, true);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    use_tool(
        &hq,
        &linus,
        "ask_owner",
        &json!({ "text": "Which sonarr?" }),
    )
    .unwrap();
    assert_eq!(label(&hq, linus.run), Some(Label::Blocked));
    let ada = turn(&hq, t.lead);
    end(&hq, &clock, &ada);

    hq.post(Author::Owner, task, "The main one.").unwrap();
    let again = turn(&hq, t.dev);
    use_tool(&hq, &again, "claim", &json!({ "task": task.0 })).unwrap();
    use_tool(
        &hq,
        &again,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    end(&hq, &clock, &again);
    let grace = turn(&hq, t.tester.unwrap());
    use_tool(
        &hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "send_back", "notes": "No reading shows it." }),
    )
    .unwrap();
    end(&hq, &clock, &grace);
    assert_eq!(label(&hq, grace.run), Some(Label::Advanced));
}

#[test]
fn the_back_off_holds_turns_that_only_talk_and_not_those_that_read() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    reading(&hq, &linus, "homelab_status", "", false);
    end(&hq, &clock, &linus);
    for _ in 0..2 {
        hq.post(Author::Owner, task, "Next.").unwrap();
        let talk = turn(&hq, t.dev);
        use_tool(
            &hq,
            &talk,
            "post",
            &json!({ "task": task.0, "text": "Thinking." }),
        )
        .unwrap();
        end(&hq, &clock, &talk);
    }
    hq.wake(t.dev, Some(task), WakeReason::Recovery).unwrap();
    let held = hq.store().runs().unwrap().pop().unwrap();
    assert_eq!(held.status, RunStatus::RetryWaiting, "two talk-only turns");
}

#[test]
fn turns_that_show_nothing_are_nudged_twice_and_then_the_task_goes_up_once() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let first = turn(&hq, t.dev);
    use_tool(&hq, &first, "claim", &json!({ "task": task.0 })).unwrap();
    end(&hq, &clock, &first);
    let mut told = Vec::new();
    for _ in 0..3 {
        hq.post(Author::Owner, task, "Next.").unwrap();
        let linus = turn(&hq, t.dev);
        told.push(linus.told.contains(NUDGE));
        use_tool(
            &hq,
            &linus,
            "post",
            &json!({ "task": task.0, "text": "Looking." }),
        )
        .unwrap();
        end(&hq, &clock, &linus);
    }
    assert_eq!(
        told,
        [false, true, true],
        "a nudge after each of the first two"
    );
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!(
        (now.assignee, now.owner, now.status),
        (Some(t.lead), None, TaskStatus::Todo),
        "the third goes up to whom Linus reports to"
    );
    let said: Vec<String> = thread(&hq, task)
        .into_iter()
        .filter(|p| p.contains("whom Linus reports to"))
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("Linus took 3 turns on this that showed nothing"));
    assert_eq!(last_wake(&hq), (t.lead, Some(task), WakeReason::Assigned));
}

#[test]
fn the_same_reading_again_shows_nothing_but_the_first_reading_does() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let read = |args: &str| {
        let linus = turn(&hq, t.dev);
        let nudged = linus.told.contains(NUDGE);
        reading(&hq, &linus, "homelab_status", args, false);
        end(&hq, &clock, &linus);
        hq.post(Author::Owner, task, "Next.").unwrap();
        nudged
    };
    assert!(!read(""));
    assert!(!read(""), "the first reading is a check");
    assert!(read(""), "the same reading again changed nothing");
    assert!(read("container: radarr"), "nudged for the turn before");
    assert!(!read("container: radarr"), "a new reading is a check");
}

#[test]
fn the_testers_check_of_a_draft_reads_the_receipts_it_cites_and_the_lead_goes_up_only_on_evidence()
{
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    lab_routine(&hq, &t);
    owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    let r = reading(&hq, &linus, "homelab_status", "", false);
    use_tool(
        &hq,
        &linus,
        "ask_owner",
        &json!({ "text": "sonarr is up again; shall I close it?", "evidence": [format!("r{r}")] }),
    )
    .unwrap();
    let grace = turn(&hq, t.tester.unwrap());
    assert!(
        grace.told.contains(&format!(
            "The receipts it cites, as the node recorded them:\n- r{r} homelab_status (ok): \
             sonarr: running, healthy"
        )),
        "{}",
        grace.told
    );

    // With a CEO at work, the lead's word up to her is refused without evidence.
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, true, false);
    owners_task(&hq, &t, t.lead, "Plan the week");
    let ada = turn(&hq, t.lead);
    let said = use_tool(&hq, &ada, "ask_owner", &json!({ "text": "Which first?" })).unwrap_err();
    assert!(said.contains("needs evidence"), "{said}");
}

#[test]
fn work_sent_back_three_times_goes_to_a_huddle_on_its_fourth_submission_not_the_owner() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let grace = t.tester.unwrap();
    for round in 1..=4 {
        if round > 1 {
            // Sent back, Linus is woken to put it right.
        }
        let linus = turn(&hq, t.dev);
        use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
        // Each round cites something new (the newest event about the task), so this is the
        // rounds' cap at work, not the carousel (ADR 0124), which a round with nothing new
        // between would be.
        let newest = hq
            .events_after(0)
            .unwrap()
            .into_iter()
            .rev()
            .find(|e| e.data["task"].as_i64() == Some(task.0))
            .unwrap()
            .id;
        use_tool(
            &hq,
            &linus,
            "set_status",
            &json!({ "task": task.0, "status": "in_review", "reason": format!("round {round}"),
                     "evidence": [format!("task {}", task.0), format!("event {newest}")] }),
        )
        .unwrap();
        end(&hq, &clock, &linus);
        if round == 4 {
            break;
        }
        let tester = turn(&hq, grace);
        use_tool(
            &hq,
            &tester,
            "review",
            &json!({ "task": task.0, "verdict": "send_back", "notes": format!("not yet ({round})") }),
        )
        .unwrap();
        end(&hq, &clock, &tester);
    }
    // The owner's desk is not the next round (ADR 0120): a huddle of the area is.
    assert!(
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .all(|d| d.kind != "rounds"),
        "{:?}",
        hq.store().desk().unwrap()
    );
    let called = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "huddle.called")
        .expect("a huddle is called");
    assert_eq!(
        called.data["members"],
        json!([t.lead.0, t.dev.0, grace.0]),
        "the lead, then who touched it, newest first"
    );
    assert!(
        called.data["why"]
            .as_str()
            .unwrap()
            .contains("sent back 3 times")
    );
    // Each is heard once, in one short turn of its own. Linus was mid-turn, on the Mac, when it
    // was called, and a huddle is a decision, which his work turn may not make (ADR 0131): he is
    // asked as soon as that turn ends, and Grace after him.
    for (who, view) in [
        (
            t.dev,
            "The restarts come from its memory limit. Raising it would hold.",
        ),
        (
            grace,
            "It still fails the second criterion. Sonarr restarts every hour.",
        ),
    ] {
        let context = turn(&hq, who);
        assert!(
            context
                .told
                .contains("A huddle on this task, which is stuck"),
            "{}",
            context.told
        );
        use_tool(
            &hq,
            &context,
            "huddle_view",
            &json!({ "task": task.0, "view": view, "evidence": [format!("task {}", task.0)] }),
        )
        .unwrap();
        assert_eq!(
            use_tool(
                &hq,
                &context,
                "huddle_view",
                &json!({ "task": task.0, "view": "Again.", "evidence": [format!("task {}", task.0)] }),
            ),
            Err(Refusal::NotInTheHuddle.to_string()),
            "one view each"
        );
        end(&hq, &clock, &context);
    }
    // Then the lead decides, on what was said.
    let ada = turn(&hq, t.lead);
    for said in [
        "Your huddle's views:",
        "- Grace: It still fails the second criterion. Sonarr restarts every hour.",
        "- Linus: The restarts come from its memory limit.",
    ] {
        assert!(ada.told.contains(said), "{said}\n---\n{}", ada.told);
    }
    use_tool(
        &hq,
        &ada,
        "decide_stuck",
        &json!({ "task": task.0, "decision": "proceed",
                 "detail": "raise sonarr's memory limit, then resubmit",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    let docs = hq.store().documents_of(task).unwrap();
    let huddle = docs
        .iter()
        .find(|d| d.title == format!("Huddle on #{}", task.0))
        .unwrap();
    assert!(
        huddle.text.contains("- Grace: It still fails"),
        "{}",
        huddle.text
    );
    assert!(
        huddle
            .text
            .contains("Decided by Ada: proceed: raise sonarr's memory limit")
    );
    assert!(
        hq.store()
            .events_after(0)
            .unwrap()
            .iter()
            .any(|e| e.kind == "huddle.decided" && e.data["decision"] == "proceed")
    );
    let now = hq.store().task(task).unwrap().unwrap();
    assert_eq!((now.status, now.assignee), (TaskStatus::Todo, Some(t.dev)));
    assert_eq!(last_wake(&hq), (t.dev, Some(task), WakeReason::Assigned));
    assert!(
        hq.store().desk().unwrap().is_empty(),
        "nothing on the owner's desk"
    );
}

#[test]
fn with_no_other_lead_the_testers_pass_closes_the_work_and_says_so() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    lab_routine(&hq, &t);
    hq.pause(t.lead, true).unwrap();
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    use_tool(
        &hq,
        &linus,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    end(&hq, &clock, &linus);
    let grace = turn(&hq, t.tester.unwrap());
    let mine = reading(&hq, &grace, "homelab_status", "", false);
    use_tool(
        &hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "Holds.",
                 "evidence": [format!("r{mine}")] }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert!(thread(&hq, task).contains(
        &"No lead but the one who did this work: the tester's pass closes it.".to_owned()
    ));
}

#[test]
fn a_check_back_on_a_task_blocked_on_a_wait_is_replaced_by_the_wait() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.lead, "Fix the downloads");
    let ada = turn(&hq, t.lead);
    use_tool(&hq, &ada, "claim", &json!({ "task": task.0 })).unwrap();
    use_tool(
        &hq,
        &ada,
        "create_task",
        &json!({ "title": "Restart deluge", "description": "after the VPN", "assignee": "Linus",
                 "criteria": [{ "text": "deluge is running", "live": true }] }),
    )
    .unwrap();
    use_tool(
        &hq,
        &ada,
        "set_status",
        &json!({ "task": task.0, "status": "blocked", "reason": "waiting on Linus" }),
    )
    .unwrap();
    use_tool(
        &hq,
        &ada,
        "check_back",
        &json!({ "minutes": 30, "why": "see if Linus is done" }),
    )
    .unwrap();
    end(&hq, &clock, &ada);
    let before = hq.runner().started().len();
    clock.0.fetch_add(31 * 60_000, Ordering::SeqCst);
    hq.tick().unwrap();
    hq.tick().unwrap();
    let ada_turns = hq
        .runner()
        .started()
        .into_iter()
        .skip(before)
        .filter(|c| c.member == t.lead)
        .count();
    assert_eq!(ada_turns, 0, "no paid turn to say it still waits");
    let said: Vec<String> = thread(&hq, task)
        .into_iter()
        .filter(|p| p.contains("is replaced by the wait on"))
        .collect();
    assert_eq!(said.len(), 1);
}

/// Live, task #667: after restarting two containers, Ada took four turns ten seconds apart,
/// each one fresh homelab_status, a check-back and a post. The first of them read only what
/// her turn before had read, so it showed nothing; and its label (checked) reset the task's
/// recovery nudge, so the tick woke her again at once, check-back or not.
#[test]
fn a_reading_already_taken_the_turn_before_shows_nothing_whatever_its_trivial_differences() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at deluge");
    let first = turn(&hq, t.dev);
    use_tool(&hq, &first, "claim", &json!({ "task": task.0 })).unwrap();
    reading(&hq, &first, "homelab_status", "", false);
    reading(&hq, &first, "container_logs", "container: deluge", false);
    reading(&hq, &first, "restart_container", "container: deluge", true);
    end(&hq, &clock, &first);
    hq.post(Author::Owner, task, "Next.").unwrap();
    let again = turn(&hq, t.dev);
    assert!(!again.told.contains(NUDGE), "{}", again.told);
    reading(&hq, &again, "homelab_status", " { } ", false);
    end(&hq, &clock, &again);
    hq.post(Author::Owner, task, "Next.").unwrap();
    let third = turn(&hq, t.dev);
    assert!(
        third.told.contains(NUDGE),
        "the status it read the turn before, again: {}",
        third.told
    );
}

#[test]
fn a_member_with_a_check_back_set_is_not_woken_again_until_it_comes() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at deluge");
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    reading(&hq, &linus, "homelab_status", "", false);
    use_tool(
        &hq,
        &linus,
        "check_back",
        &json!({ "minutes": 10, "why": "see that deluge held" }),
    )
    .unwrap();
    end(&hq, &clock, &linus);
    let turns = || {
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .filter(|r| r.member == t.dev)
            .count()
    };
    for _ in 0..10 {
        hq.tick().unwrap();
        clock.0.fetch_add(30_000, Ordering::SeqCst);
    }
    assert_eq!(turns(), 1, "no recovery wake while its check-back waits");
    clock.0.fetch_add(6 * 60_000, Ordering::SeqCst);
    let back = turn(&hq, t.dev);
    assert_ne!(back.run, linus.run);
    assert!(
        back.told
            .contains("You said you would check back: see that deluge held")
    );
}

#[test]
fn a_member_is_not_woken_again_on_its_task_within_two_minutes_unless_something_new_came() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at deluge");
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    reading(&hq, &linus, "homelab_status", "", false);
    end(&hq, &clock, &linus);
    // A stale wake (a recovery nudge) seconds after its turn ended waits out the two minutes.
    hq.wake(t.dev, Some(task), WakeReason::Recovery).unwrap();
    let held = hq.store().runs().unwrap().pop().unwrap();
    assert_eq!(held.status, RunStatus::RetryWaiting);
    let ended = hq
        .store()
        .run(linus.run)
        .unwrap()
        .unwrap()
        .ended_at
        .unwrap();
    assert_eq!(held.retry_at, Some(ended + 2 * 60_000));
    // The owner's word is new: it brings the turn forward at once.
    hq.post(Author::Owner, task, "Deluge is back, look now.")
        .unwrap();
    let now = hq.store().run(held.id).unwrap().unwrap();
    assert_eq!(now.status, RunStatus::Queued);
}

#[test]
fn the_chain_of_command_closes_a_subordinates_check_on_evidence_and_hands_on_its_work() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, true, false);
    let ceo = t.ceo.unwrap();
    hq.add_routine(
        Author::Endora,
        NewRoutine {
            title: "Check the home lab".to_owned(),
            description: String::new(),
            goal: t.goal,
            project: None,
            assignee: t.dev,
            every_ms: 6 * HOUR_MS,
            on_lab_trouble: true,
        },
    )
    .unwrap();
    hq.alarm("deluge exited at 12:01").unwrap();
    let task = hq.store().routines().unwrap()[0].last_task.unwrap();
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    end(&hq, &clock, &linus);
    // Nudged once (two minutes after his turn) and still not moving, it is stuck and blocked,
    // as #663 was live.
    hq.tick().unwrap();
    clock.0.fetch_add(2 * 60_000, Ordering::SeqCst);
    let nudged = turn(&hq, t.dev);
    end(&hq, &clock, &nudged);
    clock
        .0
        .fetch_add(super::engine::STRANDED_AFTER_MS + 1, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Blocked
    );
    // Stuck, it climbs to its area's lead, never the CEO (ADR 0120): Ada, above Linus,
    // confirms the recovery herself, done on her own fresh reading.
    assert_eq!(last_wake(&hq), (t.lead, Some(task), WakeReason::Decide));
    assert!(
        hq.store().wakes().unwrap().iter().all(|w| w.member != ceo),
        "the CEO is not woken for it"
    );
    let ada = turn(&hq, t.lead);
    let close = |evidence: Value| {
        use_tool(
            &hq,
            &ada,
            "set_status",
            &json!({ "task": task.0, "status": "done", "reason": "deluge is back",
                     "evidence": evidence }),
        )
    };
    assert!(close(json!([])).unwrap_err().contains("criterion 1"));
    // An hour on from the alarm, "no new alarm since" may be shown.
    clock.0.fetch_add(60 * 60_000, Ordering::SeqCst);
    let now = reading(&hq, &ada, "homelab_status", "", false);
    close(json!([format!("r{now}"), format!("r{now}")])).unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert!(
        thread(&hq, task)
            .iter()
            .any(|p| p.ends_with("deluge is back (for Linus, as one above them)")),
        "{:?}",
        thread(&hq, task)
    );
    end(&hq, &clock, &ada);

    // Ada hands Linus's task to herself; Linus may not hand hers.
    let mine = owners_task(&hq, &t, t.dev, "Look at radarr");
    let lead_task = owners_task(&hq, &t, t.lead, "Plan");
    let ada = turn(&hq, t.lead);
    use_tool(
        &hq,
        &ada,
        "hand_to",
        &json!({ "task": mine.0, "to": "Ada", "why": "I will take it",
                 "handover": { "done": "nothing yet", "not_done": "the look", "next": "look",
                               "watch_out": "radarr restarts slowly" } }),
    )
    .unwrap();
    let moved = hq.store().task(mine).unwrap().unwrap();
    assert_eq!((moved.assignee, moved.owner), (Some(t.lead), None));
    assert!(
        thread(&hq, mine)
            .contains(&"Handed from Linus to Ada by Ada, above them: I will take it".to_owned())
    );
    assert_eq!(
        hq.hand_to_in(
            ada.run,
            lead_task,
            t.dev,
            "yours",
            &crate::domain::world::HandoverNote::new("a", "b", "c", "d").unwrap()
        ),
        Err(super::engine::EngineError::Refused(Refusal::NotYours)),
        "her own task is nobody's below her"
    );
}

/// The snapshot carries the scorecard ("How the team is doing"): today and the last seven
/// days, from HQ's own record, the screen only rendering it.
#[test]
fn the_snapshot_scores_how_the_team_is_doing() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    end(&hq, &clock, &linus);
    hq.post(Author::Owner, task, "Next.").unwrap();
    let linus = turn(&hq, t.dev);
    use_tool(
        &hq,
        &linus,
        "post",
        &json!({ "task": task.0, "text": "Hm." }),
    )
    .unwrap();
    hq.report_usage(linus.run, 30_000).unwrap();
    end(&hq, &clock, &linus);

    let snap = hq.snapshot().unwrap();
    let today = &snap["scorecard"]["today"];
    assert_eq!(today["turns"]["total"], json!(2));
    assert_eq!(today["turns"]["nothing"], json!(1), "the talk-only turn");
    assert_eq!(today["turns"]["nothing_share"], json!(50));
    assert_eq!(today["turns"]["unlabelled"], json!(0));
    assert_eq!(today["turns"]["members"][0]["member"], json!(t.dev.0));
    assert_eq!(today["turns"]["members"][0]["spent"], json!(30_000));
    assert_eq!(today["review"]["submissions"], json!(0));
    assert_eq!(today["finished"]["done"], json!(0));
    // HQ was started at noon: the day began before its record did, and the card says so.
    assert_eq!(today["since"], json!(OCT_17));
    let week = &snap["scorecard"]["week"];
    assert_eq!(week["turns"]["total"], json!(2));
    assert_eq!(week["until"], today["until"]);
    assert!(week["from"].as_i64().unwrap() < today["from"].as_i64().unwrap());
    assert!(
        week["since"].is_i64(),
        "the record is younger than the week"
    );
}

/// The review carousel (ADR 0124): work sent back a third time with nothing new submitted
/// between goes straight to a huddle of its area, and is not reviewed again before the lead
/// decides it.
#[test]
fn a_task_bounced_three_times_with_nothing_new_goes_to_a_huddle_not_another_round() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let grace = t.tester.unwrap();
    let huddles = |hq: &Hq<'_>| {
        hq.store()
            .events_after(0)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "huddle.called")
            .collect::<Vec<_>>()
    };
    for round in 1..=3 {
        let linus = turn(&hq, t.dev);
        use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
        use_tool(
            &hq,
            &linus,
            "set_status",
            &json!({ "task": task.0, "status": "in_review", "reason": format!("round {round}"),
                     "evidence": [format!("task {}", task.0)] }),
        )
        .unwrap();
        end(&hq, &clock, &linus);
        assert!(huddles(&hq).is_empty(), "not before the third return");
        let tester = turn(&hq, grace);
        use_tool(
            &hq,
            &tester,
            "review",
            &json!({ "task": task.0, "verdict": "send_back", "notes": format!("not yet ({round})") }),
        )
        .unwrap();
        end(&hq, &clock, &tester);
    }
    let called = huddles(&hq);
    assert_eq!(called.len(), 1, "at the third return");
    assert!(
        called[0].data["why"]
            .as_str()
            .unwrap()
            .contains("sent back 3 times with no new evidence between"),
        "{}",
        called[0].data
    );
    let found = crate::application::guard_ports::GuardStore::findings(hq.store()).unwrap();
    assert!(
        found
            .iter()
            .any(|f| f.guard == crate::domain::guards::Guard::Carousel && f.cleared_at.is_none())
    );
    // Linus submits again while the huddle decides: no fourth round for the tester.
    let reviews = |hq: &Hq<'_>| {
        hq.store()
            .wakes()
            .unwrap()
            .iter()
            .filter(|w| w.member == grace && w.reason == WakeReason::ReviewRequested)
            .count()
    };
    let before = reviews(&hq);
    let linus = turn(&hq, t.dev);
    let _ = use_tool(&hq, &linus, "claim", &json!({ "task": task.0 }));
    use_tool(
        &hq,
        &linus,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "round 4",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    assert_eq!(reviews(&hq), before, "it waits for the lead's decision");
}

/// A tester's pass is bound to the criteria it judged (ADR 0124): changed since, it reads
/// "needs re-check", and the lead cannot accept on it.
#[test]
fn a_pass_on_criteria_that_changed_since_needs_re_checking_and_does_not_count() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let task = owners_task(&hq, &t, t.dev, "Look at sonarr");
    let linus = turn(&hq, t.dev);
    use_tool(&hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    use_tool(
        &hq,
        &linus,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    end(&hq, &clock, &linus);
    let grace = turn(&hq, t.tester.unwrap());
    let mine = reading(&hq, &grace, "homelab_status", "", false);
    use_tool(
        &hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "Holds.",
                 "evidence": [format!("r{mine}")] }),
    )
    .unwrap();
    end(&hq, &clock, &grace);
    let tested = |hq: &Hq<'_>| {
        let snap = hq.snapshot().unwrap();
        snap["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == task.0)
            .unwrap()["evidence"]["cited"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["stage"] == "tested")
            .unwrap()["needs_recheck"]
            .clone()
    };
    assert_eq!(tested(&hq), json!(false));
    hq.set_criteria(task, &[Criterion::new("sonarr stays up for a day", true)])
        .unwrap();
    assert_eq!(tested(&hq), json!(true), "needs re-check");
    let ada = turn(&hq, t.lead);
    let refused = use_tool(
        &hq,
        &ada,
        "accept",
        &json!({ "task": task.0, "verdict": "accept", "notes": "fine",
                 "evidence": [[format!("r{mine}")]] }),
    )
    .unwrap_err();
    assert_eq!(
        refused,
        Refusal::NotTested.to_string(),
        "the pass does not count"
    );
}

// ---- a lead narrows what done means (ADR 0133) --------------------------------------------------

/// The owner's task for Linus with two things that must hold, handed in by Linus and passed by
/// Grace on her own reading.
fn passed_on_two_criteria(hq: &Hq<'_>, clock: &Hands, t: &Team) -> TaskId {
    let task = hq
        .add_task_with(
            Author::Owner,
            NewTask {
                goal: t.goal,
                project: None,
                parent: None,
                title: "Tidy the docs".to_owned(),
                description: String::new(),
                assignee: Some(t.dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
            &[
                Criterion::new("The links work", false),
                Criterion::new("The headings are consistent", false),
            ],
        )
        .unwrap();
    let linus = turn(hq, t.dev);
    use_tool(hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    use_tool(
        hq,
        &linus,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done",
                 "evidence": [format!("task {}", task.0), format!("task {}", task.0)] }),
    )
    .unwrap();
    end(hq, clock, &linus);
    let grace = turn(hq, t.tester.unwrap());
    let mine = reading(hq, &grace, "homelab_status", "", false);
    use_tool(
        hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "Both hold.",
                 "evidence": [format!("r{mine}"), format!("r{mine}")] }),
    )
    .unwrap();
    end(hq, clock, &grace);
    // Her pass asks Ada to accept it: that turn is over before the narrowing is tried.
    let asked = turn(hq, t.lead);
    end(hq, clock, &asked);
    task
}

/// What the feed says of criteria that were changed.
fn criteria_events(hq: &Hq<'_>) -> Vec<Value> {
    hq.events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "task.criteria")
        .map(|e| e.data)
        .collect()
}

/// The ceremony finding tells a lead to narrow a task's criteria, and the engine had the means and
/// no tool to use it (ADR 0124 deferred it). The lead of a task's area narrows or rewords them:
/// it is posted in the thread and recorded in the feed, and a tester's pass on the old ones needs
/// checking again.
#[test]
fn a_lead_narrows_its_areas_criteria_and_it_is_posted_recorded_and_the_old_pass_stops_counting() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let task = passed_on_two_criteria(&hq, &clock, &t);
    let tested = |hq: &Hq<'_>| {
        hq.snapshot().unwrap()["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == task.0)
            .unwrap()["evidence"]["cited"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["stage"] == "tested")
            .unwrap()["needs_recheck"]
            .clone()
    };
    assert_eq!(tested(&hq), json!(false));

    hq.wake(t.lead, Some(task), WakeReason::Guard).unwrap();
    let ada = turn(&hq, t.lead);
    let said = use_tool(
        &hq,
        &ada,
        "narrow_criteria",
        &json!({ "task": task.0,
                 "criteria": [{ "text": "The docs build with no broken link" }],
                 "why": "the headings are a taste, not a need" }),
    )
    .unwrap();
    assert!(said.contains("narrowed"), "{said}");

    let now = hq.store().task(task).unwrap().unwrap().criteria;
    assert_eq!(
        now,
        [Criterion::new("The docs build with no broken link", false)]
    );
    let thread = hq.store().thread(task).unwrap();
    assert!(
        thread.iter().any(|p| p.author == "endora"
            && p.text.contains("Ada narrowed what done means")
            && p.text.contains("the headings are a taste, not a need")
            && p.text
                .contains("Was: 1. The links work; 2. The headings are consistent")
            && p.text
                .contains("Now: 1. The docs build with no broken link")),
        "{thread:?}"
    );
    let recorded = criteria_events(&hq);
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(
        (
            recorded[0]["task"].clone(),
            recorded[0]["by"].clone(),
            recorded[0]["before"].clone(),
            recorded[0]["after"].clone()
        ),
        (
            json!(task.0),
            json!(t.lead.0),
            json!(["The links work", "The headings are consistent"]),
            json!(["The docs build with no broken link"])
        )
    );
    assert_eq!(tested(&hq), json!(true), "the pass judged the old criteria");
}

/// Only the lead of a task's area narrows its criteria, and only to fewer, never to none: the
/// tool is offered to leads alone, and the engine holds the rest to it.
#[test]
fn only_the_lead_of_the_tasks_area_narrows_and_never_to_more_or_to_none() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, true, true);
    let bea = hq.hire("Bea", Role::Lead, 25, 2, 0).unwrap();
    let task = passed_on_two_criteria(&hq, &clock, &t);
    let one = json!([{ "text": "The docs build with no broken link" }]);
    // What `who` is told when it tries to narrow the task in a turn that `why` woke.
    let tries = |who: MemberId, why: WakeReason| {
        hq.wake(who, Some(task), why).unwrap();
        let context = turn(&hq, who);
        let told = use_tool(
            &hq,
            &context,
            "narrow_criteria",
            &json!({ "task": task.0, "criteria": one }),
        )
        .unwrap_err();
        end(&hq, &clock, &context);
        told
    };
    let only = "only the lead of the task's area narrows its criteria";
    // The worker, the tester, the CEO and a lead of another area are refused.
    assert!(tries(t.dev, WakeReason::OwnerReplied).contains(only));
    assert!(tries(t.tester.unwrap(), WakeReason::ReviewRequested).contains(only));
    assert!(tries(t.ceo.unwrap(), WakeReason::Guard).contains(only));
    assert!(
        tries(bea, WakeReason::Guard).contains(only),
        "not Bea's area"
    );
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().criteria.len(),
        2,
        "nothing changed"
    );
    assert!(criteria_events(&hq).is_empty());

    // Ada, whose area it is, may not widen it, nor leave it with nothing to meet.
    hq.wake(t.lead, Some(task), WakeReason::Guard).unwrap();
    let ada = turn(&hq, t.lead);
    let three = json!([{ "text": "a" }, { "text": "b" }, { "text": "c" }]);
    let widened = use_tool(
        &hq,
        &ada,
        "narrow_criteria",
        &json!({ "task": task.0, "criteria": three }),
    )
    .unwrap_err();
    assert!(
        widened.contains("the task has 2 and you gave 3"),
        "{widened}"
    );
    let none = use_tool(
        &hq,
        &ada,
        "narrow_criteria",
        &json!({ "task": task.0, "criteria": [] }),
    )
    .unwrap_err();
    assert!(none.contains("at least one"), "{none}");
    // Rewording to as many is allowed.
    use_tool(
        &hq,
        &ada,
        "narrow_criteria",
        &json!({ "task": task.0,
                 "criteria": [{ "text": "Every link resolves" }, { "text": "Headings read alike" }] }),
    )
    .unwrap();
    assert_eq!(criteria_events(&hq).len(), 1);

    // Finished work has nothing to narrow.
    hq.drop_task(task).unwrap();
    let over = use_tool(
        &hq,
        &ada,
        "narrow_criteria",
        &json!({ "task": task.0, "criteria": one }),
    )
    .unwrap_err();
    assert_eq!(over, Refusal::Finished.to_string());
}

/// The tool is a lead's: no other role is offered it, the CEO included.
#[test]
fn narrow_criteria_is_offered_to_leads_only() {
    use crate::application::protocol::tools;
    let offered = |role: Role| {
        tools(role)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "narrow_criteria")
    };
    assert!(offered(Role::Lead));
    for role in [Role::Developer, Role::Researcher, Role::Reviewer, Role::Ceo] {
        assert!(!offered(role), "{role:?}");
    }
    let tool = tools(Role::Lead)
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "narrow_criteria")
        .cloned()
        .unwrap();
    assert_eq!(
        tool["inputSchema"]["required"],
        json!(["task", "criteria"]),
        "narrow_criteria {{task, criteria}}"
    );
}

// ---- why work was sent back (ADR 0132) --------------------------------------------------------

/// Linus's task, handed in for review.
fn handed_in(hq: &Hq<'_>, clock: &Hands, t: &Team) -> TaskId {
    let task = owners_task(hq, t, t.dev, "Look at sonarr");
    let linus = turn(hq, t.dev);
    use_tool(hq, &linus, "claim", &json!({ "task": task.0 })).unwrap();
    use_tool(
        hq,
        &linus,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    end(hq, clock, &linus);
    task
}

/// What the feed says of work sent back.
fn sent_back_events(hq: &Hq<'_>) -> Vec<Value> {
    hq.events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "task.sent_back")
        .map(|e| e.data)
        .collect()
}

/// A tester who sends work back because the team already had it says so, and the feed keeps the
/// reason: it is how the memory builds are measured, work reinvented against work found.
#[test]
fn a_tester_can_send_work_back_because_it_already_exists_and_the_feed_keeps_the_reason() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let task = handed_in(&hq, &clock, &t);
    let grace = turn(&hq, t.tester.unwrap());
    use_tool(
        &hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "send_back", "reason": "exists_already",
                 "notes": "The restart role is in roles/deluge already." }),
    )
    .unwrap();
    let sent = sent_back_events(&hq);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["reason"], "exists_already");
    assert_eq!(sent[0]["task"], task.0);
    assert_eq!(sent[0]["by"], t.tester.unwrap().0);
    assert_eq!(sent[0]["via"], "tester");
    assert!(
        thread(&hq, task)
            .iter()
            .any(|p| p == "Sent back: The restart role is in roles/deluge already."),
        "the thread keeps the notes as they were: {:?}",
        thread(&hq, task)
    );
}

/// A send-back with no reason named is still recorded, with none: the feed counts every one.
#[test]
fn a_send_back_with_no_reason_is_kept_with_none() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let task = handed_in(&hq, &clock, &t);
    let grace = turn(&hq, t.tester.unwrap());
    use_tool(
        &hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "send_back", "notes": "No reading shows it." }),
    )
    .unwrap();
    let sent = sent_back_events(&hq);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["reason"], Value::Null);
}

/// Only a reason HQ counts is taken, and a reason belongs to a send-back alone.
#[test]
fn a_reason_must_be_one_hq_knows_and_goes_with_a_send_back_alone() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, true);
    let task = handed_in(&hq, &clock, &t);
    let grace = turn(&hq, t.tester.unwrap());
    let unknown = use_tool(
        &hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "send_back", "reason": "boring",
                 "notes": "Not for me." }),
    )
    .unwrap_err();
    assert!(
        unknown.contains("`reason` must be exists_already"),
        "{unknown}"
    );
    let mine = reading(&hq, &grace, "homelab_status", "", false);
    let on_a_pass = use_tool(
        &hq,
        &grace,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "reason": "exists_already",
                 "notes": "Holds.", "evidence": [format!("r{mine}")] }),
    )
    .unwrap_err();
    assert!(
        on_a_pass.contains("only goes with send_back"),
        "{on_a_pass}"
    );
    assert!(sent_back_events(&hq).is_empty(), "nothing was sent back");
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::InReview,
        "both were refused, so the work stayed where it was"
    );
}

/// The lead's send-back says why in the same words, and the feed says it was the lead's.
#[test]
fn a_lead_can_send_work_back_because_it_already_exists_too() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false, false);
    let task = handed_in(&hq, &clock, &t);
    let ada = turn(&hq, t.lead);
    use_tool(
        &hq,
        &ada,
        "accept",
        &json!({ "task": task.0, "verdict": "send_back", "reason": "exists_already",
                 "notes": "This is the media stack's restart job again." }),
    )
    .unwrap();
    let sent = sent_back_events(&hq);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["reason"], "exists_already");
    assert_eq!(sent[0]["via"], "lead");
    assert_eq!(sent[0]["by"], t.lead.0);
}
