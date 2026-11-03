//! The team protocol over HQ's real store and the simulated runner: a turn's token opens only
//! that turn, while it runs; the tools act as its member, under the engine's rules.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use serde_json::json;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, Run, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::application::desk_ports::DeskStore;
use crate::application::schedule_ports::ScheduleStore;
use crate::domain::{Member, MemberId, Priority, TaskId};
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
    hq.hire("Ada", 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", 40, 2, 4).unwrap();
    hq.hire("Grace", 20, 2, 1).unwrap();
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
        // The post left out is said to be, and where it is: an owner's early word that falls
        // out of the newest twelve was gone with no sign.
        assert!(
            thread.contains(&format!(
                "Left out here, kept in the record: the thread's 1 older post (ask_record with \
                 task {} finds it by its words).",
                setup.task.0
            )),
            "{thread}"
        );
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
    {
        let offered: Vec<String> = tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        for tool in NEVER_BUILT {
            assert!(
                !offered.contains(&tool.to_owned()),
                "a member is offered {tool}"
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
    dev: MemberId,
    parent: TaskId,
    own: TaskId,
    child: TaskId,
    grandchild: TaskId,
    other: TaskId,
}

fn tree(hq: &Hq<'_>) -> Tree {
    let lead = hq.hire("Ada", 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", 40, 2, 4).unwrap();
    hq.hire("Grace", 20, 2, 1).unwrap();
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

// HQ's record to 3 Nov (48 refused calls, 14 of them on 2 Nov's held-out set): a member gives a
// criterion's evidence as an id and a line saying what it shows, `[["r4031", "the :ro statement
// is in the thread"]]`, and the whole call was refused for the line, though its id was good.
// Words beside an id in one criterion's list are its gloss and are let go; a list, or a
// criterion, holding only words is refused as before, and so is a document.
#[test]
fn words_beside_an_id_in_one_criterions_list_are_let_go() {
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
    let id = format!("task {}", s.task.0);

    cite(json!([[id.clone(), "the list is there, in the thread"]])).unwrap();
    cite(json!([["what it shows comes first", id.clone()]])).unwrap();
    for refused in [
        json!([["the list is there", "and so is the rest"]]),
        json!([[id.clone()], ["the list is there"]]),
        json!([id, "the list is there"]),
    ] {
        let why = cite(refused.clone()).unwrap_err();
        assert!(why.contains("is not evidence"), "{refused}: {why}");
    }
}

// Set 4 and HQ's record before it (161 refusals in 116 turns): a member cites the document a
// submit or a check wrote (`d241`), which is not evidence, and was told only "cite a receipt as
// r12, a task as task 12", so it tried again, some turns several times, and some fell back to
// citing the task itself. The refusal names the receipt that wrote the document: the submit's,
// whose result names the document, or the check's. The rule of what counts is unchanged.
#[test]
fn a_document_cited_as_evidence_is_refused_naming_the_receipt_that_wrote_it() {
    use crate::application::receipt_ports::ReceiptStore;
    use crate::application::wing_ports::WingStore;
    use crate::domain::{CallReceipt, Document};
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, s, context) = started(&clock);
    let (run, me) = turn(&hq, &context.token);
    call(&hq, &run, &me, "claim", &json!({ "task": s.task.0 })).unwrap();
    let document = |title: &str| {
        hq.store()
            .add_document(&Document {
                id: 0,
                task: s.task,
                run: Some(run.id),
                title: title.to_owned(),
                text: "the diff".to_owned(),
                at: 1,
            })
            .unwrap()
    };
    let receipt = |tool: &str, args: &str, excerpt: &str| {
        hq.store()
            .add_call_receipt(&CallReceipt {
                id: 0,
                run: run.id,
                member: me.id,
                task: Some(s.task),
                tool: tool.to_owned(),
                args: args.to_owned(),
                excerpt: excerpt.to_owned(),
                ok: true,
                act: true,
                confidential: false,
                sandbox: false,
                at: 1,
            })
            .unwrap()
    };
    let diff = document("branch hq/task-1-watches (1 file, +1 −1)");
    // A document whose number begins another's is not that one's: d1 is not d12.
    receipt(
        "repo_checkout",
        "branch: null",
        &format!("checked out develop (document d{diff}0 is another task's)"),
    );
    let submit = receipt(
        "repo_submit",
        "branch: hq/task-1-watches",
        &format!(
            "Submitted hq/task-1-watches at a1b2c3d (1 file, +1 −1): document d{diff} on task"
        ),
    );
    let check = receipt("check", "c7 hq/task-1-watches@a1b2c3d", "syntax ok");
    let report = document("check c7: syntax ok");
    let cite = |evidence: serde_json::Value| {
        call(
            &hq,
            &run,
            &me,
            "post",
            &json!({ "task": s.task.0, "text": "Done.", "evidence": evidence }),
        )
        .unwrap_err()
    };

    let why = cite(json!([format!("d{diff}")]));
    assert!(why.contains(&format!("cite r{submit}")), "{why}");
    let why = cite(json!([[format!("r{submit}"), format!("d{report}")]]));
    assert!(why.contains(&format!("cite r{check}")), "{why}");
    let why = cite(json!([format!("the diff (document d{diff}) shows it")]));
    assert!(why.contains(&format!("cite r{submit}")), "{why}");
    // A document no receipt names, or none at all, is still said to be a document.
    let loose = document("notes");
    for unnamed in [format!("d{loose}"), "d999".to_owned()] {
        let why = cite(json!([unnamed]));
        assert!(why.contains("is a document, not a receipt"), "{why}");
    }
    // Words with no id are refused as before.
    assert!(cite(json!(["the list is there"])).contains("is not evidence"));
}

#[test]
#[ignore = "a measurement, run by hand"]
fn measure_tool_descriptions() {
    {
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
            "{} tools, {total} chars; top: {:?}",
            sizes.len(),
            &sizes[..8]
        );
    }
}

/// The tools a house turn starts with are read on every call by a slow model (ADR 0142, amended
/// 2026-10-30): each says its rules, briefly. Every rule is still said; the words around them
/// are fewer.
#[test]
fn the_tools_a_house_turn_starts_with_say_their_rules_briefly() {
    let all = tools();
    let tool = |name: &str| {
        all.as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no {name}"))
    };
    for (name, most, rules) in [
        ("my_turn", 300, &["first"][..]),
        ("claim", 300, &["Refused if another member has it"][..]),
        (
            "post",
            1050,
            &[
                "A mention hands no work on",
                "cites its evidence",
                "marked unsupported",
                "restart is coming",
                "1,200 characters",
                "ends your turn",
            ][..],
        ),
        (
            "set_status",
            1200,
            &[
                "in_review",
                "a reading this turn",
                "HQ's checks and the evidence decide it",
                "close as done yourself",
                "met_criteria",
                "superseded",
                "not_needed",
                "not evidence",
            ][..],
        ),
        (
            "ask_owner",
            650,
            &[
                "taste the owner kept",
                "found or named",
                "ends your turn",
                "HQ asks the owner itself",
            ][..],
        ),
        (
            "chess",
            700,
            &[
                "board",
                "san",
                "refused if illegal",
                "challenge",
                "One game each",
                "never on Claude",
            ][..],
        ),
    ] {
        let said = tool(name);
        let text = said.to_string();
        assert!(text.len() <= most, "{name} is {} bytes: {text}", text.len());
        for rule in rules {
            assert!(text.contains(rule), "{name} lost {rule:?}: {text}");
        }
    }
    // Roles, reviewers' re-checks, climbs and hand-offs to another member are retired (ADR
    // 0142, amended 2026-11-02; every live loop is worked by one member alone): no tool's
    // description promises one, or asks for one the engine refuses.
    for name in ["my_turn", "set_status", "create_task", "ask_owner"] {
        let text = tool(name).to_string();
        for retired in ["your role", "re-checks", "rank up", "for another member"] {
            assert!(!text.contains(retired), "{name} says {retired:?}: {text}");
        }
    }
}

/// Climb 2, S3: HQ's ask_owner says when to ask in the kernel's one statement, which the worker's
/// instructions also carry; the old list that contradicted the worker's rule is gone.
#[test]
fn ask_owner_says_when_to_ask_in_the_kernels_one_statement() {
    let all = tools();
    let ask = all
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ask_owner")
        .expect("the ask_owner tool");
    let said = ask["description"].as_str().unwrap();
    assert!(
        said.contains(&endora_kernel::escalation::when_to_ask_briefly()),
        "{said}"
    );
    assert!(
        !said.contains("only what the owner alone decides"),
        "{said}"
    );
}

/// Climb 2, S3 (round 33): the tool and the worker share one category clause, which keeps the
/// owner's reserved taste and names data-losing steps.
#[test]
fn ask_owner_carries_the_shared_category_clause_with_taste_and_data_loss() {
    let all = tools();
    let ask = all
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ask_owner")
        .expect("the ask_owner tool");
    let said = ask["description"].as_str().unwrap();
    assert!(
        said.contains(endora_kernel::escalation::DECISIONS),
        "{said}"
    );
    assert!(said.contains("taste the owner kept"), "{said}");
    assert!(said.contains("data-losing"), "{said}");
}

/// Correction-placement review: a revision left unsubmitted is recorded before the status
/// changes, and a record that cannot be written changes nothing. The store is made to refuse
/// that line (a trigger on the posts table), as a full disk or a failing database would.
#[test]
fn a_disposition_that_cannot_be_recorded_leaves_the_item_where_it_was() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let path = std::env::temp_dir().join(format!(
        "endora-disposition-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&path);
    let hq = Engine::new(
        SqliteTeamStore::open(path.to_str().unwrap()).unwrap(),
        SimulatedRunner::new(4),
        &clock,
        Settings::default(),
    );
    let dev = hq.hire("Linus", 40, 2, 4).unwrap();
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
    let (run, member) = turn(&hq, &context.token);
    call(&hq, &run, &member, "claim", &json!({ "task": task.0 })).unwrap();
    let head = "5faf6aed".repeat(5);
    let receipt = hq
        .record_receipt(
            &context.token,
            &super::engine::Called {
                tool: "repo_checkout".to_owned(),
                args: "branch: develop".to_owned(),
                excerpt: format!(
                    "checked out the base at {}: develop (4 KB)",
                    "01234567".repeat(5)
                ),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    let closing = json!({ "task": task.0, "status": "in_review", "reason": "Done.",
        "evidence": [format!("r{receipt}")],
        "sandbox": { "state": "read", "branches": [{ "branch": "hq/task-1-fix", "head": head }] },
        "unsubmitted": [{ "branch": "hq/task-1-fix", "head": head, "why": "not wanted" }] });
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TRIGGER fault BEFORE INSERT ON posts WHEN NEW.text LIKE '%unsubmitted%' \
             BEGIN SELECT RAISE(ABORT, 'the disk is full'); END;",
        )
        .unwrap();
    }
    let refused = call(&hq, &run, &member, "set_status", &closing);
    assert!(refused.is_err(), "{refused:?}");
    let status = hq.store().task(task).unwrap().unwrap().status;
    assert_eq!(
        status.name(),
        "doing",
        "a disposition not recorded closes nothing"
    );
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("DROP TRIGGER fault;").unwrap();
    }
    call(&hq, &run, &member, "set_status", &closing).unwrap();
    let thread = hq.store().thread(task).unwrap();
    let noted = thread
        .iter()
        .position(|p| {
            p.text
                .contains(&format!("leaves hq/task-1-fix at {head} unsubmitted"))
        })
        .expect("the disposition is in the thread");
    let moved = thread
        .iter()
        .position(|p| p.text.contains("→ in_review"))
        .expect("the change is in the thread");
    assert!(noted < moved, "recorded before the status changed");
    let _ = std::fs::remove_file(&path);
}

/// `ask_record` says what it searches, and only that: stories and notebooks were retired with
/// their tables (ADR 0142, 27 Oct), and the store no longer searches them.
#[test]
fn ask_record_names_only_the_records_it_searches() {
    let all = tools();
    let ask = all
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "ask_record")
        .unwrap();
    let said = ask["description"].as_str().unwrap();
    assert!(
        said.contains("handover notes") && said.contains("conversations of turns"),
        "{said}"
    );
    assert!(
        !said.contains("stories") && !said.contains("notebooks"),
        "{said}"
    );
}
