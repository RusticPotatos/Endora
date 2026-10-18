//! Evidence, acceptance criteria and the tester (ADR 0114): receipts written only by the node
//! for a running turn, done only on evidence per criterion (a live one from a fresh reading),
//! the home lab's standard criteria, unsupported posts, turn labels and what follows them, the
//! cap on review rounds, check-backs that wait, and the chain of command acting on a
//! subordinate's task.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Called, Engine, Settings};
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

/// The newest turn of `member`'s, after a tick or two to start it.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
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
    let now = reading(&hq, &ada, "homelab_status", "", false);
    close(json!([format!("r{now}"), format!("r{now}")])).unwrap();
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
fn work_sent_back_three_times_is_the_owners_on_its_fourth_submission() {
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
        use_tool(
            &hq,
            &linus,
            "set_status",
            &json!({ "task": task.0, "status": "in_review", "reason": format!("round {round}"),
                     "evidence": [format!("task {}", task.0)] }),
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
    let rounds: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "rounds")
        .collect();
    assert_eq!(rounds.len(), 1);
    assert!(
        rounds[0].text.contains("Sent back 3 times"),
        "{}",
        rounds[0].text
    );
    assert!(rounds[0].text.contains("- Grace: not yet (3)"));
    assert_ne!(last_wake(&hq).0, grace, "no fourth round for the tester");
    hq.decide(rounds[0].id, "accept").unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
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
            assignee: t.lead,
            every_ms: 6 * HOUR_MS,
            on_lab_trouble: true,
        },
    )
    .unwrap();
    hq.alarm("deluge exited at 12:01").unwrap();
    let task = hq.store().routines().unwrap()[0].last_task.unwrap();
    let ada = turn(&hq, t.lead);
    use_tool(&hq, &ada, "claim", &json!({ "task": task.0 })).unwrap();
    end(&hq, &clock, &ada);
    // Mara, above Ada, confirms the recovery herself: done on her own fresh reading.
    // Nudged once and still not moving, it is stuck and blocked, as #663 was live.
    let nudged = turn(&hq, t.lead);
    end(&hq, &clock, &nudged);
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Blocked
    );
    hq.wake(ceo, Some(task), WakeReason::Assigned).unwrap();
    let mara = turn(&hq, ceo);
    let close = |evidence: Value| {
        use_tool(
            &hq,
            &mara,
            "set_status",
            &json!({ "task": task.0, "status": "done", "reason": "deluge is back",
                     "evidence": evidence }),
        )
    };
    assert!(close(json!([])).unwrap_err().contains("criterion 1"));
    let now = reading(&hq, &mara, "homelab_status", "", false);
    close(json!([format!("r{now}"), format!("r{now}")])).unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert!(
        thread(&hq, task)
            .iter()
            .any(|p| p.ends_with("deluge is back (for Ada, as one above them)")),
        "{:?}",
        thread(&hq, task)
    );
    end(&hq, &clock, &mara);

    // Ada hands Linus's task to herself; Linus may not hand hers.
    let mine = owners_task(&hq, &t, t.dev, "Look at radarr");
    let lead_task = owners_task(&hq, &t, t.lead, "Plan");
    let ada = turn(&hq, t.lead);
    use_tool(
        &hq,
        &ada,
        "hand_to",
        &json!({ "task": mine.0, "to": "Ada", "why": "I will take it" }),
    )
    .unwrap();
    let moved = hq.store().task(mine).unwrap().unwrap();
    assert_eq!((moved.assignee, moved.owner), (Some(t.lead), None));
    assert!(
        thread(&hq, mine)
            .contains(&"Handed from Linus to Ada by Ada, above them: I will take it".to_owned())
    );
    assert_eq!(
        hq.hand_to_in(ada.run, lead_task, t.dev, "yours"),
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
