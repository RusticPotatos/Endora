//! The team protocol over HQ's real store and the simulated runner: a turn's token opens only
//! that turn, while it runs; the tools act as its member, under the engine's rules.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, Run, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::domain::{Member, MemberId, Priority, Role, TaskId};
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
    (hq, Setup { dev, task }, context)
}

fn turn(hq: &Hq<'_>, token: &str) -> (Run, Member) {
    hq.turn_of(token)
        .unwrap()
        .expect("the token opens its turn")
}

#[test]
fn asking_up_the_chain_is_for_decisions_and_colleagues_are_not_asked() {
    // ADR 0138 keeps management escalation for decisions; ADR 0142 retires asking colleagues.
    let ask = tools()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ask_owner")
        .unwrap()["description"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(ask.contains("decision"), "{ask}");
    assert!(!ask.contains("consult"), "{ask}");
    assert!(!ask.contains("Ask a question"), "{ask}");
}

#[test]
fn my_turn_keeps_the_latest_twelve_posts_in_both_protocol_versions() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, setup, context) = started(&clock);
    let (run, member) = turn(&hq, &context.token);
    for n in 0..13 {
        hq.post(Author::Owner, setup.task, &format!("Fixture post {n:03}"))
            .unwrap();
    }
    for canonical in [false, true] {
        let reply = if canonical {
            super::protocol::call_v2(&hq, &run, &member, "my_turn", &json!({}))
        } else {
            call(&hq, &run, &member, "my_turn", &json!({}))
        }
        .unwrap();
        let (_, thread) = reply.split_once("The thread, newest last:\n").unwrap();
        assert!(!thread.contains("Fixture post 000"));
        let positions: Vec<_> = (1..13)
            .map(|n| thread.find(&format!("Fixture post {n:03}")).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
    assert_eq!(
        hq.store().thread(setup.task).unwrap().len(),
        13,
        "bounded worker context must preserve all owner-visible history"
    );
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

/// `run_gate` and `request_action` were listed to every member and answered "not available yet"
/// to every call, since the first build: a member was offered two tools that could only fail, and
/// every turn paid for their listing. They are not offered to any role, not named in what a
/// member is told it can use, and a call to either is a call to a tool that is not there.
#[test]
fn tools_that_were_never_built_are_not_offered_told_of_or_known() {
    const NEVER_BUILT: [&str; 2] = ["run_gate", "request_action"];
    for role in [
        Role::Ceo,
        Role::Lead,
        Role::Developer,
        Role::Researcher,
        Role::Reviewer,
    ] {
        let offered: Vec<String> = tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        for tool in NEVER_BUILT {
            assert!(
                !offered.contains(&tool.to_owned()),
                "{role:?} is offered {tool}"
            );
        }
    }
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    for tool in NEVER_BUILT {
        let refused = call(&hq, &run, &me, tool, &json!({ "task": s.task.0 })).unwrap_err();
        assert_eq!(refused, format!("there is no tool called `{tool}`"));
    }
    let told = hq.runner().started()[0].told.clone();
    for tool in NEVER_BUILT {
        assert!(
            !told.contains(tool),
            "{tool} is in what the member is told: {told}"
        );
    }
    assert!(
        told.contains("Your tools at HQ: my_turn, claim, post, set_status, create_task, ask_owner, check_back,"),
        "{told}"
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
    assert!(
        !hq.store()
            .wakes()
            .unwrap()
            .iter()
            .any(|wake| wake.member != s.dev)
    );
    hq.post(Author::Owner, s.task, "@Grace please inspect the list")
        .unwrap();
    // The owner's explicit request waits for his turn to end. She is the tester, whose work is the
    // Mac's (ADR 0131), and the node says it is free.
    hq.tell_mac(true, "ready");
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
    let tool = tools()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "check_back")
        .cloned()
        .unwrap();
    assert_eq!(tool["inputSchema"]["required"], json!(["minutes", "why"]));
}

// Run 1831 was refused for evidence written as objects, `{"receipt": "r301", "task": "task
// 695"}`, and for a finding with its receipt in brackets after it, then cited the same ids
// plainly: four refusals for one status. What each holds is read as its references; an object
// or a line that names none is refused still.
#[test]
fn evidence_written_as_objects_or_as_findings_citing_an_id_reads_as_those_ids() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    let use_tool = |tool: &str, args: serde_json::Value| call(&hq, &run, &me, tool, &args);
    use_tool("claim", json!({ "task": s.task.0 })).unwrap();
    let cite = |evidence: serde_json::Value| {
        use_tool(
            "post",
            json!({ "task": s.task.0, "text": "Found it.", "evidence": evidence }),
        )
    };

    for read in [
        json!([{ "task": format!("task {}", s.task.0) }]),
        json!([{ "task": s.task.0, "criterion": "the list is there" }]),
        json!([format!("the list is not there yet (task {})", s.task.0)]),
        json!([format!("as task {} shows, the list is there", s.task.0)]),
    ] {
        cite(read.clone()).unwrap_or_else(|why| panic!("{read}: {why}"));
    }
    for refused in [
        json!([{ "criterion": "the list is there" }]),
        json!([{ "receipt": "the log" }]),
        json!(["the list is there"]),
        json!(["criterion #1 holds"]),
    ] {
        let why = cite(refused.clone()).unwrap_err();
        assert!(why.contains("is not evidence"), "{refused}: {why}");
    }
}

#[test]
#[ignore = "a measurement, run by hand"]
fn measure_tool_descriptions() {
    for role in [
        Role::Developer,
        Role::Lead,
        Role::Reviewer,
        Role::Researcher,
    ] {
        let all = tools();
        let mut sizes: Vec<(usize, String)> = all
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (t.to_string().len(), t["name"].as_str().unwrap().to_owned()))
            .collect();
        sizes.sort_by(|a, b| b.cmp(a));
        let total: usize = sizes.iter().map(|s| s.0).sum();
        println!(
            "{role:?}: {} tools, {total} chars; top: {:?}",
            sizes.len(),
            &sizes[..8]
        );
    }
}
