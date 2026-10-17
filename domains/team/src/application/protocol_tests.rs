//! The team protocol over HQ's real store and the simulated runner: a turn's token opens only
//! that turn, while it runs; the tools act as its member, under the engine's rules.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, Run, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::domain::{GoalId, Member, MemberId, Priority, Role, TaskId, TaskStatus};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

struct Setup {
    dev: MemberId,
    goal: GoalId,
    task: TaskId,
}

fn started(clock: &Hands) -> (Hq<'_>, Setup, RunContext) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap();
    let goal = hq
        .set_goal("Ship the watch tracker", "you asked for it", Author::Owner)
        .unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                parent: None,
                title: "Add a list of watches".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let context = hq.runner().started()[0].clone();
    (hq, Setup { dev, goal, task }, context)
}

fn turn(hq: &Hq<'_>, token: &str) -> (Run, Member) {
    hq.turn_of(token)
        .unwrap()
        .expect("the token opens its turn")
}

#[test]
fn a_turns_token_opens_only_that_turn_and_only_while_it_runs() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    assert_eq!(context.token.len(), 64, "32 random bytes, as hex");
    let (run, member) = turn(&hq, &context.token);
    assert_eq!((run.id, member.id), (context.run, s.dev));
    assert!(hq.turn_of("not-a-token").unwrap().is_none());
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    assert!(
        hq.turn_of(&context.token).unwrap().is_none(),
        "an ended turn's token opens nothing"
    );
}

#[test]
fn the_tools_act_as_the_turns_member_under_the_engines_rules() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    let use_tool = |tool: &str, args: serde_json::Value| call(&hq, &run, &me, tool, &args);

    assert!(
        use_tool("my_turn", json!({}))
            .unwrap()
            .contains("Add a list of watches")
    );
    use_tool("claim", json!({ "task": s.task.0 })).unwrap();
    assert_eq!(hq.store().task(s.task).unwrap().unwrap().owner, Some(s.dev));
    use_tool(
        "post",
        json!({ "task": s.task.0, "text": "Started on the list." }),
    )
    .unwrap();
    let thread = hq.store().thread(s.task).unwrap();
    assert_eq!(
        thread.last().unwrap().author,
        "Linus",
        "posted as the turn's member"
    );
    assert!(
        use_tool("my_turn", json!({}))
            .unwrap()
            .contains("Linus: Started on the list.")
    );

    use_tool(
        "set_status",
        json!({ "task": s.task.0, "status": "in_review", "reason": "ready for a look" }),
    )
    .unwrap();
    let refused = use_tool(
        "set_status",
        json!({ "task": s.task.0, "status": "done", "reason": "finished" }),
    );
    assert!(refused.is_err(), "only the gate sets done");
    assert_eq!(
        hq.store().task(s.task).unwrap().unwrap().status,
        TaskStatus::InReview
    );

    // Work goes only to members below: the developer may not hand the reviewer a task.
    let above = use_tool(
        "create_task",
        json!({ "title": "Review the list", "description": "check it", "assignee": "grace" }),
    );
    assert!(above.unwrap_err().contains("below you"));
    let made = use_tool(
        "create_task",
        json!({ "title": "Review the list", "description": "check it" }),
    )
    .unwrap();
    assert!(made.starts_with("Created task"), "{made}");
    let sub = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.title == "Review the list")
        .unwrap();
    assert_eq!((sub.parent, sub.goal), (Some(s.task), s.goal));
    assert!(
        use_tool(
            "create_task",
            json!({ "title": "x", "description": "y", "assignee": "Nobody" })
        )
        .is_err()
    );

    assert!(
        use_tool("run_gate", json!({ "task": s.task.0 }))
            .unwrap_err()
            .contains("not available yet")
    );
    assert!(use_tool("format_the_disk", json!({})).is_err());
    let names: Vec<String> = tools()
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names.len(), 8);
    assert!(
        !names.contains(&"report_usage".to_owned()),
        "the relay is the meter: a worker reports nothing"
    );
}

#[test]
fn a_question_reaches_the_desk_and_seeing_it_wakes_the_member() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    call(&hq, &run, &me, "claim", &json!({ "task": s.task.0 })).unwrap();
    call(
        &hq,
        &run,
        &me,
        "ask_owner",
        &json!({ "text": "Steel or titanium first?" }),
    )
    .unwrap();
    let question = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.kind == "question")
        .unwrap();
    assert_eq!(question.text, "Linus asks: Steel or titanium first?");
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    hq.decide(question.id, "seen").unwrap();
    // Woken: a turn queued for it, or folded into the one already waiting for it.
    let woke = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    assert_eq!(
        (woke.member, woke.reason),
        (s.dev, crate::domain::WakeReason::ProposalDecided)
    );
    assert!(
        matches!(
            woke.receipt,
            crate::domain::Receipt::Queued | crate::domain::Receipt::Folded
        ),
        "{woke:?}"
    );
}

/// The question on the desk, the last one asked.
fn question(hq: &Hq<'_>) -> crate::application::ports::DeskItem {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .rfind(|d| d.kind == "question")
        .unwrap()
}

#[test]
fn seeing_a_question_wakes_the_member_who_asked_not_the_tasks_owner() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    call(&hq, &run, &me, "claim", &json!({ "task": s.task.0 })).unwrap();
    call(
        &hq,
        &run,
        &me,
        "post",
        &json!({ "task": s.task.0, "text": "@Grace can you look at the list?" }),
    )
    .unwrap();
    // Her wake waits for his turn on the task to end.
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    hq.tick().unwrap();
    let grace = hq
        .runner()
        .started()
        .into_iter()
        .find(|c| c.member != s.dev)
        .expect("Grace's turn on the task");
    let (her_run, her) = turn(&hq, &grace.token);
    assert_eq!(her_run.task, Some(s.task));
    call(
        &hq,
        &her_run,
        &her,
        "ask_owner",
        &json!({ "text": "Is the list for watches only?" }),
    )
    .unwrap();
    let asked = question(&hq);
    assert_eq!(asked.member, Some(her.id), "the desk knows who asked");
    hq.runner().finish(grace.run, true);
    hq.tick().unwrap();
    hq.decide(asked.id, "seen").unwrap();
    let woke = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    assert_eq!(
        (woke.member, woke.task),
        (her.id, Some(s.task)),
        "Grace asked, so Grace is woken, not Linus whose task it is"
    );
}

#[test]
fn the_owner_answers_a_question_in_its_thread_and_then_the_member_is_woken() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    call(
        &hq,
        &run,
        &me,
        "ask_owner",
        &json!({ "text": "Steel or titanium first?" }),
    )
    .unwrap();
    let asked = question(&hq);
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    hq.answer(asked.id, "  Steel first.  ").unwrap();
    let said = hq.store().thread(s.task).unwrap().pop().unwrap();
    assert_eq!(
        (said.author.as_str(), said.text.as_str()),
        ("owner", "Steel first.")
    );
    assert_eq!(question(&hq).status, "seen");
    let woke = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    assert_eq!(woke.member, s.dev);
    assert!(
        hq.answer(asked.id, "Again.").is_err(),
        "a question is answered once"
    );
    assert!(
        hq.answer(asked.id, "   ").is_err(),
        "an empty answer is not one"
    );
}

#[test]
fn a_member_asks_one_question_at_a_time_and_writes_within_reason() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    let ask = |text: &str| call(&hq, &run, &me, "ask_owner", &json!({ "text": text }));
    ask("Steel or titanium first?").unwrap();
    let again = ask("And the straps?").unwrap_err();
    assert!(again.contains("waiting"), "{again}");
    let waiting = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "question" && d.status == "waiting")
        .count();
    assert_eq!(waiting, 1, "one question at a time on the owner's desk");
    hq.decide(question(&hq).id, "dismiss").unwrap();
    ask("And the straps?").unwrap();

    let long = "x".repeat(5_000);
    let refused = call(
        &hq,
        &run,
        &me,
        "post",
        &json!({ "task": s.task.0, "text": long }),
    )
    .unwrap_err();
    assert!(refused.contains("4000"), "{refused}");
}

#[test]
fn a_worker_reports_no_usage_because_the_relay_already_charged_it() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, _s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    let said = call(
        &hq,
        &run,
        &me,
        "report_usage",
        &json!({ "micros": 10_000_000 }),
    )
    .unwrap();
    assert!(said.contains("relay"), "{said}");
    let (still, _) = turn(&hq, &context.token);
    assert_eq!(still.spent, 0, "nothing charged twice");
    assert!(
        !run.told.contains("report_usage"),
        "the briefing does not offer it: {}",
        run.told
    );
}
