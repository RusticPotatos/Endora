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
                project: None,
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
            .contains("Linus [unsupported]: Started on the list."),
        "a post citing nothing, in a turn with no reading"
    );

    use_tool(
        "set_status",
        json!({ "task": s.task.0, "status": "in_review", "reason": "ready for a look",
                "evidence": [format!("task {}", s.task.0)] }),
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
    let names: Vec<String> = tools(Role::Developer)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    // A huddle on stuck work asks every member's view (ADR 0120), and every member shapes the
    // office (ADR 0119), and may play chess (ADR 0121); every member reads the world and tells stories (ADR 0122).
    assert_eq!(names.len(), 20, "{names:?}");
    assert!(names.contains(&"read_world".to_owned()) && names.contains(&"tell_story".to_owned()));
    assert!(names.contains(&"huddle_view".to_owned()));
    assert!(names.contains(&"office".to_owned()));
    assert!(names.contains(&"chess".to_owned()));
    assert!(names.contains(&"read_document".to_owned()));
    // Every role keeps a notebook of its own (ADR 0118).
    assert!(
        ["note", "forget", "read_notebook", "notebook_checked"]
            .iter()
            .all(|n| names.contains(&(*n).to_owned()))
    );
    assert!(
        names.contains(&"read_doc".to_owned()),
        "every member reads the library"
    );
    assert!(
        !names.contains(&"review".to_owned()),
        "only the reviewer gives a verdict"
    );
    let reviewers: Vec<String> = tools(Role::Reviewer)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    // With the office (ADR 0119), a huddle's view (ADR 0120), chess (ADR 0121) and the world
    // (ADR 0122).
    assert_eq!(reviewers.len(), 20, "{reviewers:?}");
    assert!(
        !reviewers.contains(&"create_task".to_owned()),
        "the tester passes or sends back, and never makes work"
    );
    assert!(
        !reviewers.contains(&"note_lesson".to_owned())
            && !names.contains(&"note_lesson".to_owned()),
        "a lesson is a story now: tell_story"
    );
    assert!(reviewers.contains(&"tell_story".to_owned()));
    assert!(reviewers.contains(&"review".to_owned()));
    assert!(names.contains(&"check_back".to_owned()));
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

/// The member's newest turn, after a tick starts it.
fn next_turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn for the member")
}

#[test]
fn a_member_asks_one_question_at_a_time_and_writes_within_reason() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
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
    call(
        &hq,
        &run,
        &me,
        "ask_owner",
        &json!({ "text": "Steel or titanium first?" }),
    )
    .unwrap();

    // The owner writes to Linus without answering: he is woken, and asks again.
    hq.post(Author::Owner, s.task, "@Linus one moment.")
        .unwrap();
    let again = next_turn(&hq, s.dev);
    let (run, me) = turn(&hq, &again.token);
    let ask = |text: &str| call(&hq, &run, &me, "ask_owner", &json!({ "text": text }));
    let refused = ask("And the straps?").unwrap_err();
    assert!(refused.contains("waiting"), "{refused}");
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
}

#[test]
fn asking_the_owner_ends_the_turn_and_its_token_does_nothing_more() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    call(&hq, &run, &me, "claim", &json!({ "task": s.task.0 })).unwrap();
    let said = call(
        &hq,
        &run,
        &me,
        "ask_owner",
        &json!({ "text": "Steel first?" }),
    )
    .unwrap();
    assert!(said.contains("ended"), "{said}");
    let ended = hq.store().run(run.id).unwrap().unwrap();
    assert_eq!(
        (ended.status, ended.end_reason.as_str()),
        (crate::domain::RunStatus::Succeeded, "asked the owner")
    );
    assert!(hq.turn_of(&context.token).unwrap().is_none());
    assert_eq!(hq.runner().stopped(), [run.id], "its container is let go");
    // A call already on its way, with the turn as it was, is refused too.
    let late = call(
        &hq,
        &run,
        &me,
        "post",
        &json!({ "task": s.task.0, "text": "One more thing." }),
    )
    .unwrap_err();
    assert!(late.contains("ended"), "{late}");
    // Seeing it wakes the member, as before.
    hq.decide(question(&hq).id, "seen").unwrap();
    let woke = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    assert_eq!((woke.member, woke.task), (s.dev, Some(s.task)));
}

/// A goal's tasks as a tree, with Linus's turn on `own`: `parent` above it, `child` and
/// `grandchild` below, and `other` beside, in the same goal.
struct Tree {
    lead: MemberId,
    dev: MemberId,
    parent: TaskId,
    own: TaskId,
    child: TaskId,
    grandchild: TaskId,
    other: TaskId,
}

fn tree(hq: &Hq<'_>) -> Tree {
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap();
    let goal = hq
        .set_goal("Ship the watch tracker", "you asked for it", Author::Owner)
        .unwrap();
    let add = |parent: Option<TaskId>, title: &str, assignee: Option<MemberId>| {
        hq.add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent,
                title: title.to_owned(),
                description: String::new(),
                assignee,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap()
    };
    let parent = add(None, "Plan the tracker", Some(lead));
    let own = add(Some(parent), "Add a list of watches", Some(dev));
    let child = add(Some(own), "Sort the list", None);
    let grandchild = add(Some(child), "Sort by brand", None);
    let other = add(None, "Write the help page", None);
    Tree {
        lead,
        dev,
        parent,
        own,
        child,
        grandchild,
        other,
    }
}

#[test]
fn a_turn_acts_on_its_own_task_and_below_and_writes_above_or_where_it_was_named() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        &clock,
        Settings::default(),
    );
    let t = tree(&hq);
    let mine = next_turn(&hq, t.dev);
    let (run, me) = turn(&hq, &mine.token);
    assert_eq!(run.task, Some(t.own));
    let use_tool = |tool: &str, args: serde_json::Value| call(&hq, &run, &me, tool, &args);
    let status = |task: TaskId, to: &str| {
        use_tool(
            "set_status",
            json!({ "task": task.0, "status": to, "reason": "on it",
                    "evidence": [format!("task {}", task.0)] }),
        )
    };
    let post = |task: TaskId| use_tool("post", json!({ "task": task.0, "text": "A note." }));

    use_tool("claim", json!({ "task": t.own.0 })).unwrap();
    use_tool("claim", json!({ "task": t.grandchild.0 })).expect("a sub-task, two down");
    status(t.grandchild, "in_review").unwrap();
    assert!(
        status(t.own, "done").unwrap_err().contains("gate"),
        "only the gate sets done, still"
    );
    post(t.child).unwrap();
    post(t.parent).expect("the task above, to report back");

    for refused in [
        use_tool("claim", json!({ "task": t.other.0 })),
        use_tool("claim", json!({ "task": t.parent.0 })),
        status(t.parent, "blocked"),
        post(t.other),
    ] {
        let why = refused.unwrap_err();
        assert!(why.contains("not part of this turn"), "{why}");
    }
    assert_eq!(hq.store().task(t.other).unwrap().unwrap().owner, None);

    // Named in its thread, Linus may write there, and still not take it.
    hq.post(Author::Owner, t.other, "@Linus could you help here?")
        .unwrap();
    post(t.other).unwrap();
    assert!(use_tool("claim", json!({ "task": t.other.0 })).is_err());
}

#[test]
fn the_lead_acts_on_any_task_in_its_goal_but_never_sets_done() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        &clock,
        Settings::default(),
    );
    let t = tree(&hq);
    let lead = next_turn(&hq, t.lead);
    let (run, me) = turn(&hq, &lead.token);
    let use_tool = |tool: &str, args: serde_json::Value| call(&hq, &run, &me, tool, &args);
    use_tool("claim", json!({ "task": t.other.0 })).unwrap();
    use_tool(
        "set_status",
        json!({ "task": t.other.0, "status": "blocked", "reason": "waiting on the owner" }),
    )
    .unwrap();
    use_tool(
        "post",
        json!({ "task": t.grandchild.0, "text": "Brand first." }),
    )
    .unwrap();
    let done = use_tool(
        "set_status",
        json!({ "task": t.other.0, "status": "done", "reason": "finished" }),
    );
    assert!(done.unwrap_err().contains("gate"));
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

#[test]
fn a_turn_asks_to_check_back_on_its_own_task_and_is_told_when() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    let use_tool = |tool: &str, args: serde_json::Value| call(&hq, &run, &me, tool, &args);
    let said = use_tool(
        "check_back",
        json!({ "minutes": 2, "why": "see whether the restart held" }),
    )
    .unwrap();
    assert!(said.contains("in 5 minutes"), "{said}");
    let backs = hq.store().check_backs().unwrap();
    assert_eq!((backs[0].member, backs[0].task), (s.dev, s.task));
    assert_eq!(backs[0].why, "see whether the restart held");
    assert!(
        use_tool("check_back", json!({ "why": "soon" })).is_err(),
        "minutes"
    );
    assert!(
        use_tool("check_back", json!({ "minutes": 10 })).is_err(),
        "why"
    );
    let tool = tools(Role::Lead)
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "check_back")
        .cloned()
        .unwrap();
    assert_eq!(tool["inputSchema"]["required"], json!(["minutes", "why"]));
}

#[test]
fn a_lesson_noted_the_old_way_is_a_story_to_retell_and_the_owner_is_told() {
    use crate::application::world_ports::WorldStore;
    // 17 October 2026, at noon UTC.
    let clock = Hands(AtomicI64::new(1_792_238_400_000));
    let (hq, s, dev_turn) = started(&clock);
    let ada = hq.store().members().unwrap()[0].id;
    hq.set_goal_note(s.goal, "Owntone is being retired.")
        .unwrap();
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: s.goal,
            project: None,
            parent: None,
            title: "Check the home lab".to_owned(),
            description: String::new(),
            assignee: Some(ada),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    let lead = next_turn(&hq, ada);
    let (run, me) = turn(&hq, &lead.token);
    let lesson = "An RSS sync that finds releases and grabs none is normal.";
    // A worker told of note_lesson before stories (ADR 0122): kept as a story to retell, in
    // the lead's area, and the goal's notes are left as the owner wrote them.
    let said = call(&hq, &run, &me, "note_lesson", &json!({ "lesson": lesson })).unwrap();
    assert!(said.contains("story:an-rss-sync"), "{said}");
    assert_eq!(hq.goal_note(s.goal).unwrap(), "Owntone is being retired.");
    let stories = hq.store().stories().unwrap();
    assert_eq!(stories.len(), 1);
    assert!(stories[0].to_retell);
    assert_eq!(stories[0].instead, lesson);
    assert_eq!(stories[0].scope, crate::domain::world::Scope::Area(ada));
    let desk = hq.store().desk().unwrap();
    let told = desk.last().unwrap();
    assert_eq!(told.kind, "notice");
    assert_eq!(told.text, format!("Ada noted: {lesson}"));
    assert_eq!(told.member, Some(ada));
    assert_eq!(hq.store().count_events("lesson.noted").unwrap().0, 1);

    // At most 300 characters, and something said.
    assert!(
        call(
            &hq,
            &run,
            &me,
            "note_lesson",
            &json!({ "lesson": "x".repeat(301) })
        )
        .unwrap_err()
        .contains("300")
    );
    assert!(call(&hq, &run, &me, "note_lesson", &json!({ "lesson": "  " })).is_err());
    // The same lesson again is a second story, under its own handle.
    let again = call(&hq, &run, &me, "note_lesson", &json!({ "lesson": lesson })).unwrap();
    assert!(again.contains("story:an-rss-sync-2"), "{again}");

    // A developer keeps none.
    let (run, me) = turn(&hq, &dev_turn.token);
    let refused = call(&hq, &run, &me, "note_lesson", &json!({ "lesson": lesson })).unwrap_err();
    assert!(
        refused.contains("only the lead and the reviewer"),
        "{refused}"
    );
    assert!(
        !tools(Role::Lead)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "note_lesson"),
        "tell_story is offered instead"
    );
}

/// The lead is told who is on the team and to split the work: the developer fixes, the
/// researcher looks things up, the reviewer checks, and a step her brain may not take goes to
/// a member who may. Nobody else's briefing carries it.
#[test]
fn the_leads_briefing_names_the_team_and_says_to_hand_work_on() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, linus) = started(&clock);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "Ada")
        .unwrap()
        .id;
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: s.goal,
            project: None,
            parent: None,
            title: "Check the home lab".to_owned(),
            description: String::new(),
            assignee: Some(ada),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    hq.tick().unwrap();
    let lead = hq
        .runner()
        .started()
        .into_iter()
        .find(|c| c.member == ada)
        .expect("the lead's turn started");
    let (run, me) = turn(&hq, &lead.token);
    let told = call(&hq, &run, &me, "my_turn", &json!({})).unwrap();
    assert!(told.contains("Your team:"), "{told}");
    assert!(told.contains("Linus (developer, Local)"), "{told}");
    assert!(told.contains("Grace (reviewer, Local)"), "{told}");
    assert!(told.contains("hand"), "{told}");
    let (run, me) = turn(&hq, &linus.token);
    let told = call(&hq, &run, &me, "my_turn", &json!({})).unwrap();
    assert!(!told.contains("Your team:"), "only the lead's: {told}");
}
