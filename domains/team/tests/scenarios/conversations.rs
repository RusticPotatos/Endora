//! Public-use-case steps for the information-only collaboration slice.

use cucumber::{given, then, when};
use endora_team::application::{Author, TeamStore};
use endora_team::domain::{Role, WakeReason};
use endora_team::infrastructure::Script;
use serde_json::json;

use super::Office;

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
