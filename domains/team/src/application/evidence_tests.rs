//! Evidence, acceptance criteria and the tester (ADR 0114): receipts written only by the node
//! for a running turn, done only on evidence per criterion (a live one from a fresh reading),
//! the home lab's standard criteria, unsupported posts, turn labels and what follows them, the
//! cap on review rounds, check-backs that wait, and the chain of command acting on a
//! subordinate's task.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use serde_json::{Value, json};

use super::engine::{Author, Called, Engine, EngineError, Settings};
use super::ports::{Clock, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::application::receipt_ports::ReceiptStore;
use crate::application::schedule_ports::ScheduleStore;
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
    lead: MemberId,
    dev: MemberId,
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
    if ceo {
        hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
    }
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    if tester {
        hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap();
    }
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    Team { lead, dev, goal }
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

/// Live: the owner's "no literal passwords in role defaults" went to a lead with no definition of
/// done and split into three blocked sub-tasks, each defensible and none tied to what was asked.
/// The owner's work says what done means now (ADR 0124, amended): at least one line, or it is
/// refused with the way to give one, and nothing is made or woken.
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

/// A routine's task says what done means as its routine does (ADR 0124, amended): the home lab's
/// check, the two live criteria; any other routine of the owner's, a reading of what it is about,
/// cited. A lead's standing reviews and the CEO's are reports and decisions, closed by filing
/// them: they carry none, so closing one stays as it was.
#[test]
fn a_routines_task_carries_criteria_from_its_routine() {
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

// ---- a lead narrows what done means (ADR 0133) --------------------------------------------------

// ---- why work was sent back (ADR 0132) --------------------------------------------------------
