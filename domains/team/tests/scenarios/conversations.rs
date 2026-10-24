//! Public-use-case steps for the information-only collaboration slice.

use cucumber::{given, then, when};
use endora_team::application::conversation_ports::ConversationStore;
use endora_team::application::history_ports::HistoryStore;
use endora_team::application::{Author, TeamStore};
use endora_team::domain::{Role, WakeReason};
use endora_team::infrastructure::Script;
use serde_json::json;

use super::Office;

const REQUESTER_FINDING: &str =
    "Inconclusive: no original source was granted; the expert answer remains testimony.";

#[when("the requester records its own inconclusive finding")]
fn requester_records_finding(w: &mut Office) {
    let caller = w.turn_of("Member1");
    let expert = w.turn_of("Member2");
    w.hq.runner().finish(caller.run, true);
    w.hq.runner().finish(expert.run, true);
    w.tick();
    let result =
        w.hq.runner()
            .started()
            .into_iter()
            .rfind(|turn| turn.member == w.member("Member1"))
            .unwrap();
    assert!(
        w.hq.store()
            .assistance_notice_for_run(result.run)
            .unwrap()
            .is_some()
    );
    w.turns.insert("Member1".into(), result.clone());
    assert!(
        w.use_tool(
            "Member1",
            "record_assistance_finding",
            &json!({"finding":REQUESTER_FINDING,"evidence":[]})
        )
        .is_some(),
        "{:?}",
        w.refused
    );
    w.hq.record_transcript(&result.token, "worker", "Temporary worker transcript")
        .unwrap();
    w.hq.runner().finish(result.run, true);
    w.tick();
}

#[then("the owner sees the retained requester finding")]
fn owner_sees_finding(w: &mut Office) {
    let page =
        w.hq.assistance_requests_page(w.conversation.unwrap(), 0)
            .unwrap();
    assert_eq!(page["requests"][0]["finding"]["text"], REQUESTER_FINDING);
    assert_eq!(
        page["requests"][0]["finding"]["run"],
        w.turns.get("Member1").unwrap().run.0
    );
}

#[when("the raw worker transcripts expire")]
fn raw_transcripts_expire(w: &mut Office) {
    w.clock.0.fetch_add(
        31 * 24 * 60 * 60 * 1000,
        std::sync::atomic::Ordering::SeqCst,
    );
    assert!(w.hq.prune_transcripts().unwrap() >= 1);
    assert!(
        w.hq.store()
            .transcript(w.turns.get("Member1").unwrap().run)
            .unwrap()
            .is_none()
    );
}

#[when("the owner deletes the conversation and its retained finding")]
fn delete_retained_finding(w: &mut Office) {
    let conversation = w.conversation.unwrap();
    let page = w.hq.assistance_requests_page(conversation, 0).unwrap();
    let request = page["requests"][0]["id"].as_i64().unwrap();
    w.hq.delete_conversation(conversation).unwrap();
    while w.hq.store().purge_conversation_batch().unwrap() > 0 {}
    assert!(w.hq.store().assistance_finding(request).unwrap().is_none());
}

#[then("the conversation history is unavailable")]
fn deleted_conversation_is_unavailable(w: &mut Office) {
    assert!(
        w.hq.assistance_requests_page(w.conversation.unwrap(), 0)
            .is_err()
    );
}

#[given("the owner has assigned classified task work in an authorized conversation project")]
fn classified_task(w: &mut Office) {
    use endora_team::application::{Engine, NewTask, Settings};
    use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};
    w.hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        w.clock.clone(),
        Settings::default(),
    );
    authorized(w);
    let previous = w.turn_of("Member1").run;
    w.hq.runner().finish(previous, true);
    w.tick();
    let scope =
        w.hq.store()
            .conversation(w.conversation.unwrap())
            .unwrap()
            .unwrap();
    let project =
        w.hq.store()
            .projects()
            .unwrap()
            .into_iter()
            .find(|p| p.id == scope.project)
            .unwrap();
    w.hq.add_task(
        Author::Owner,
        NewTask {
            goal: project.goal,
            project: Some(project.id),
            parent: None,
            title: "Fixture task".into(),
            description: "Inspect the synthetic check".into(),
            assignee: Some(w.member("Member1")),
            priority: endora_team::domain::Priority::Normal,
            waits_on: vec![],
        },
    )
    .unwrap();
    w.tick();
    let turn =
        w.hq.runner()
            .started()
            .into_iter()
            .rfind(|turn| turn.member == w.member("Member1"))
            .unwrap();
    assert!(turn.task.is_some());
    w.turns.insert("Member1".into(), turn);
    w.before_information = Some((
        w.hq.store().tasks().unwrap().len(),
        w.hq.wakes().unwrap().len(),
        w.hq.store().runs().unwrap().len(),
    ));
}

#[when("Member1 requests task help through the canonical protocol twice")]
fn task_help(w: &mut Office) {
    let turn = w.turn_of("Member1");
    let (run, member) = w.hq.turn_of(&turn.token).unwrap().unwrap();
    let args = json!({"member":"Member2","question":"Which synthetic check should be read?","conversation":w.conversation.unwrap(),"key":"scenario_task_help_1"});
    let mut accepted = None;
    for _ in 0..2 {
        let reply =
            endora_team::application::protocol::call_v2(&w.hq, &run, &member, "consult", &args)
                .unwrap();
        let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
        if let Some(id) = accepted {
            assert_eq!(reply["request"], id);
        }
        accepted = Some(reply["request"].clone());
    }
}

#[then("task help has no legacy request or automatic task completion")]
fn no_legacy_task_help(w: &mut Office) {
    assert!(w.hq.store().consults().unwrap().is_empty());
    let task = w.hq.store().task(w.task("Fixture task")).unwrap().unwrap();
    assert_ne!(task.status, endora_team::domain::TaskStatus::Done);
    assert!(w.hq.store().thread(task.id).unwrap().is_empty());
}

#[given("the owner authorizes two members to exchange information")]
fn authorized(w: &mut Office) {
    let lead = w.hq.hire("Member0", Role::Lead, 25, 2, 0).unwrap();
    let a = w.hq.hire("Member1", Role::Developer, 40, 2, 0).unwrap();
    let b = w.hq.hire("Member2", Role::Developer, 40, 2, 0).unwrap();
    w.members.insert("Member1".into(), a);
    w.members.insert("Member2".into(), b);
    let goal =
        w.hq.set_goal("Synthetic investigation", "", Author::Owner)
            .unwrap();
    let project =
        w.hq.add_project(goal, "Fixture project", "", lead, None)
            .unwrap();
    w.hq.set_conversations_enabled(true).unwrap();
    w.conversation = Some(w.hq.create_conversation(project, [a, b]).unwrap());
    w.hq.wake(a, None, WakeReason::Downtime).unwrap();
    w.tick();
    let turn =
        w.hq.runner()
            .started()
            .into_iter()
            .find(|r| r.member == a)
            .unwrap();
    assert!(turn.task.is_none());
    w.turns.insert("Member1".into(), turn);
    w.before_information = Some((
        w.hq.store().tasks().unwrap().len(),
        w.hq.wakes().unwrap().len(),
        w.hq.store().runs().unwrap().len(),
    ));
}

#[when(expr = "Member1 sends the information {string} with a stable retry key")]
fn send(w: &mut Office, text: String) {
    let conversation = w.conversation.unwrap();
    let sent = w.use_tool(
        "Member1",
        "send_message",
        &json!({"conversation":conversation,"key":"scenario_message_1","text":text}),
    );
    assert!(sent.is_some(), "{:?}", w.refused);
}

#[then("Member1's taskless briefing lists the authorized colleague tools")]
fn initial_briefing(w: &mut Office) {
    let turn = w.turn_of("Member1");
    assert!(turn.task.is_none());
    let summary = turn
        .told
        .lines()
        .find(|line| line.starts_with("Your tools at HQ:"))
        .unwrap();
    for tool in [
        "read_messages",
        "send_message",
        "consult",
        "request_assistance",
        "cancel_assistance",
    ] {
        assert!(summary.contains(tool), "{tool}: {summary}");
    }
    assert!(!turn.told.contains("nothing is due"), "{}", turn.told);
    assert!(turn.told.contains("create_task and hand_to are refused"));
}

#[then("the conversation has exactly one attributed message")]
fn one(w: &mut Office) {
    let page =
        w.hq.conversation_messages(w.conversation.unwrap(), 0)
            .unwrap();
    let messages = page["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["member"], w.member("Member1").0);
    assert_eq!(messages[0]["run"], w.turn_of("Member1").run.0);
}

#[then("sending information created no task wake or run")]
fn no_work(w: &mut Office) {
    assert_eq!(
        w.before_information.unwrap(),
        (
            w.hq.store().tasks().unwrap().len(),
            w.hq.wakes().unwrap().len(),
            w.hq.store().runs().unwrap().len()
        )
    );
}

#[when("the owner deletes the information conversation")]
fn delete(w: &mut Office) {
    w.hq.delete_conversation(w.conversation.unwrap()).unwrap();
}

#[then("the deleted information conversation cannot be read")]
fn unavailable(w: &mut Office) {
    assert!(
        w.hq.conversation_messages(w.conversation.unwrap(), 0)
            .is_err()
    );
}

#[then(expr = "the retained information still says {string}")]
fn retained(w: &mut Office, text: String) {
    let page =
        w.hq.conversation_messages(w.conversation.unwrap(), 0)
            .unwrap();
    assert_eq!(page["messages"][0]["text"], text);
}

#[when("Member1 explicitly asks Member2 for read-only assistance")]
fn ask_assistance(w: &mut Office) {
    let scope = w.conversation.unwrap();
    assert!(w.use_tool("Member1","request_assistance",&json!({"conversation":scope,"key":"scenario_request_1","objective":"Which synthetic check should be read?"})).is_some(),"{:?}",w.refused);
}

#[when("Member1 asks Member2 with the ordinary consult tool")]
fn ordinary_consult(w: &mut Office) {
    assert!(
        w.use_tool(
            "Member1",
            "consult",
            &json!({
                "member":"Member2", "question":"Which synthetic check should be read?"
            })
        )
        .is_some(),
        "{:?}",
        w.refused
    );
}

#[then("accepting the request has not started any work")]
fn accepted_only(w: &mut Office) {
    no_work(w);
}

#[when("the assistance delivery tick runs")]
fn assistance_tick(w: &mut Office) {
    w.tick();
    let member = w.member("Member2");
    let turn =
        w.hq.runner()
            .started()
            .into_iter()
            .rfind(|turn| turn.member == member)
            .unwrap();
    w.turns.insert("Member2".into(), turn);
}

#[then("only Member2 receives one assistance turn")]
fn one_expert(w: &mut Office) {
    let member = w.member("Member2");
    assert_eq!(
        w.hq.runner()
            .started()
            .iter()
            .filter(|turn| turn.member == member)
            .count(),
        1
    );
    let run = w
        .turns
        .get("Member2")
        .expect("the delivery step recorded the expert turn")
        .run;
    assert!(w.hq.in_assistance(run).unwrap());
    assert_eq!(
        w.hq.store().tasks().unwrap().len(),
        w.before_information.unwrap().0
    );
}

#[then("the assistance turn cannot delegate work")]
fn no_delegate(w: &mut Office) {
    assert!(
        w.use_tool("Member2", "consult", &json!({"question":"Nested request"}))
            .is_none()
    );
    assert!(
        w.refused
            .as_deref()
            .unwrap()
            .contains("read-only assistance")
    );
}

#[then("the assistance turn has no shell or node capabilities")]
fn no_execution_capabilities(w: &mut Office) {
    let run = w.turn_of("Member2").run;
    assert!(!w.hq.may_use_a_shell(run).unwrap());
    assert_eq!(w.hq.lab_tools_of(run).unwrap(), Some(Vec::new()));
}

#[when("Member2 answers the assistance request twice with the same answer")]
fn answer_twice(w: &mut Office) {
    for _ in 0..2 {
        assert!(
            w.use_tool(
                "Member2",
                "answer_assistance",
                &json!({"answer":"Read the original synthetic check; no source was granted."})
            )
            .is_some(),
            "{:?}",
            w.refused
        );
    }
}

#[when("Member2's worker disappears after answering")]
fn worker_disappears(w: &mut Office) {
    let run = w.turn_of("Member2").run;
    w.hq.runner().vanish(run);
    w.tick();
}

#[when("Member2's worker is unavailable")]
fn worker_unavailable(w: &mut Office) {
    let member = w.member("Member2");
    w.hq.runner().script(member, Script::WillNotStart);
}

#[when("three bounded delivery attempts have passed")]
fn three_attempts(w: &mut Office) {
    for attempt in 0..3 {
        if attempt > 0 {
            w.clock
                .0
                .fetch_add(61_000, std::sync::atomic::Ordering::SeqCst);
        }
        w.tick();
    }
}

#[then("the owner sees a failed assistance request with its cause")]
fn failed_with_cause(w: &mut Office) {
    let page =
        w.hq.assistance_requests_page(w.conversation.unwrap(), 0)
            .unwrap();
    let request = &page["requests"][0];
    assert_eq!(request["status"], "failed");
    assert_eq!(request["attempts"], 3);
    assert!(
        request["reason"]
            .as_str()
            .unwrap()
            .contains("runner said no")
    );
}

#[then("the owner sees one answered request with the original author and project")]
fn answered(w: &mut Office) {
    let page =
        w.hq.assistance_requests_page(w.conversation.unwrap(), 0)
            .unwrap();
    let rows = page["requests"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "answered");
    assert_eq!(rows[0]["requester"], w.member("Member1").0);
    assert_eq!(rows[0]["expert"], w.member("Member2").0);
    assert!(rows[0]["project"].as_i64().is_some_and(|id| id > 0));
}

#[when("the owner disables assistance")]
fn disable(w: &mut Office) {
    w.hq.set_conversations_enabled(false).unwrap();
}

#[then("the request is visibly cancelled")]
fn cancelled(w: &mut Office) {
    assert_eq!(
        w.hq.assistance_requests_page(w.conversation.unwrap(), 0)
            .unwrap()["requests"][0]["status"],
        "cancelled"
    );
}

#[when("the owner re-enables assistance")]
fn reenable(w: &mut Office) {
    w.hq.set_conversations_enabled(true).unwrap();
}

#[then("cancelled assistance starts no further turns")]
fn no_restart(w: &mut Office) {
    let before = w.hq.runner().started().len();
    w.tick();
    assert_eq!(w.hq.runner().started().len(), before);
}
