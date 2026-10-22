//! Addressed information does not schedule work or transfer authority (ADR 0137).

use std::sync::atomic::AtomicI64;

use super::engine::{Author, Called};
use super::history_tests::{Hands, NOON, engine, turn_of};
use super::ports::TeamStore;
use crate::domain::{Role, WakeReason};

#[test]
fn taskless_messages_keep_identity_and_create_no_work() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let a = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let b = hq.hire("Member 3", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq
        .set_goal("Synthetic investigation", "", Author::Owner)
        .unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    assert!(!hq.conversations_enabled().unwrap());
    assert!(hq.create_conversation(project, [a, b]).is_err());
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq.create_conversation(project, [a, b]).unwrap();
    hq.wake(a, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, a);
    assert!(turn.task.is_none());
    let before = (
        hq.store().tasks().unwrap().len(),
        hq.wakes().unwrap().len(),
        hq.store().runs().unwrap().len(),
    );
    let first = hq
        .send_message_in(
            turn.run,
            conversation,
            "message_key_1",
            "Which fixture failed?",
            &[],
        )
        .unwrap();
    let again = hq
        .send_message_in(
            turn.run,
            conversation,
            "message_key_1",
            "Which fixture failed?",
            &[],
        )
        .unwrap();
    assert_eq!(first, again);
    assert!(
        hq.send_message_in(turn.run, conversation, "message_key_1", "Different", &[])
            .is_err()
    );
    hq.rename(a, "Member 9").unwrap();
    let page = hq.conversation_messages(conversation, 0).unwrap();
    assert_eq!(page["messages"][0]["member"], a.0);
    assert_eq!(page["messages"][0]["run"], turn.run.0);
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    let audit = hq.conversation_operations(0).unwrap();
    let operations = audit["operations"].as_array().unwrap();
    assert!(
        operations
            .iter()
            .any(|o| o["member"] == a.0 && o["run"] == turn.run.0 && o["reason"] == "replayed")
    );
    assert!(
        !audit.to_string().contains("Which fixture failed"),
        "operation history never includes message bodies"
    );
    assert_eq!(
        before,
        (
            hq.store().tasks().unwrap().len(),
            hq.wakes().unwrap().len(),
            hq.store().runs().unwrap().len()
        )
    );
    hq.wake(b, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let source = turn_of(&hq, b);
    hq.record_receipt(
        &source.token,
        &Called {
            tool: "fixture_read".into(),
            args: String::new(),
            excerpt: "private fixture".into(),
            ok: true,
            act: false,
            confidential: true,
        },
    )
    .unwrap();
    hq.record_transcript(
        &source.token,
        "worker",
        "Synthetic confidential zebra archive investigation",
    )
    .unwrap();
    let history = hq
        .ask_record_in(turn.run, "zebra archive investigation", None, None, None)
        .unwrap();
    assert!(
        history.contains("Synthetic confidential zebra"),
        "{history}"
    );
    assert!(hq.store().call_receipts_of(turn.run).unwrap().is_empty());
    assert!(
        hq.send_message_in(turn.run, conversation, "history_key_1", &history, &[])
            .is_err(),
        "reading Confidential history must prevent relabeling it as Internal messaging"
    );
    let kept = hq
        .record_transcript(&turn.token, "worker", &history)
        .unwrap()
        .unwrap();
    assert_eq!(kept.tier, endora_kernel::tier::Tier::Confidential);
    hq.record_receipt(
        &turn.token,
        &Called {
            tool: "fixture_read".into(),
            args: String::new(),
            excerpt: "private fixture".into(),
            ok: true,
            act: false,
            confidential: true,
        },
    )
    .unwrap();
    assert!(
        hq.send_message_in(
            turn.run,
            conversation,
            "message_key_2",
            "Must not escape",
            &[]
        )
        .is_err()
    );
    hq.set_conversations_enabled(false).unwrap();
    assert!(hq.read_messages_in(turn.run, conversation, 0).is_err());
    assert_eq!(
        hq.conversation_messages(conversation, 0).unwrap()["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn conversation_participation_does_not_come_from_a_link_or_a_role() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let a = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let b = hq.hire("Member 3", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq
        .add_project(goal, "Fixture project", "", lead, None)
        .unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq.create_conversation(project, [a, b]).unwrap();
    hq.wake(lead, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, lead);
    assert!(hq.read_messages_in(turn.run, conversation, 0).is_err());
    assert!(
        hq.send_message_in(
            turn.run,
            conversation,
            "message_key_1",
            "Not a participant",
            &[]
        )
        .is_err()
    );
    hq.delete_conversation(conversation).unwrap();
    assert!(hq.conversation_messages(conversation, 0).is_err());
    hq.tick().unwrap();
    let next = hq.create_conversation(project, [a, b]).unwrap();
    assert!(next > conversation);
}

#[test]
fn messaging_protocol_refuses_forged_identity_and_enforces_message_bounds() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let peer = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq
        .add_project(goal, "Fixture project", "", lead, None)
        .unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq.create_conversation(project, [lead, peer]).unwrap();
    hq.wake(peer, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let context = turn_of(&hq, peer);
    let (run, member) = hq.turn_of(&context.token).unwrap().unwrap();
    let forged=super::protocol::call(&hq,&run,&member,"send_message",&serde_json::json!({
        "conversation":conversation,"key":"forged_key_1","text":"Claiming another sender","sender":lead.0,
    })).unwrap_err();
    assert!(forged.contains("identity"));
    assert_eq!(
        hq.store().conversation_audit_for_test(run.id),
        vec![(None, "send_message".into(), "protocol refused".into())],
        "a forged sender is attributed to the live run, without storing its claimed scope or body"
    );
    for args in [
        serde_json::json!({"after":"wrong"}),
        serde_json::json!({"conversation":null}),
        serde_json::json!({"after":-1}),
    ] {
        assert!(super::protocol::call(&hq, &run, &member, "read_messages", &args).is_err());
    }
    let directory: serde_json::Value = serde_json::from_str(
        &super::protocol::call(&hq, &run, &member, "read_messages", &serde_json::json!({}))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(directory["conversations"].as_array().unwrap().len(), 1);
    assert!(hq.store().conversation_audit_for_test(run.id).contains(&(
        None,
        "directory".into(),
        "accepted".into()
    )));
    hq.change_project(project, "pause").unwrap();
    let directory: serde_json::Value = serde_json::from_str(
        &super::protocol::call(&hq, &run, &member, "read_messages", &serde_json::json!({}))
            .unwrap(),
    )
    .unwrap();
    assert!(directory["conversations"].as_array().unwrap().is_empty());
    hq.change_project(project, "resume").unwrap();
    hq.pause(peer, true).unwrap();
    assert!(
        !super::protocol::tools_in(&hq, &run, &member)
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "read_messages"),
        "a paused participant must not be offered information tools"
    );
    assert!(
        super::protocol::call(&hq, &run, &member, "read_messages", &serde_json::json!({})).is_err(),
        "current member state overrides the protocol's earlier snapshot"
    );
    hq.pause(peer, false).unwrap();
    assert!(
        hq.send_message_in(
            run.id,
            conversation,
            "message_key_0",
            &"x".repeat(1201),
            &[]
        )
        .is_err()
    );
    assert!(
        hq.send_message_in(run.id, conversation, "short", "Fixture", &[])
            .is_err()
    );
    for i in 0..12 {
        hq.send_message_in(
            run.id,
            conversation,
            &format!("message_key_{i:02}"),
            "Fixture",
            &[],
        )
        .unwrap();
    }
    assert!(
        hq.send_message_in(run.id, conversation, "message_key_13", "Over limit", &[])
            .is_err()
    );
    assert!(
        hq.send_message_in(run.id, conversation, "message_key_00", "Fixture", &[])
            .is_ok(),
        "a retry does not spend quota"
    );
    let available = super::protocol::tools_in(&hq, &run, &member);
    assert!(
        available
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "read_messages")
    );
    hq.runner().finish(run.id, true);
    hq.tick().unwrap();
    assert!(
        super::protocol::call(
            &hq,
            &run,
            &member,
            "read_messages",
            &serde_json::json!({"conversation":conversation})
        )
        .is_err(),
        "ended turn cannot read"
    );
}

#[test]
fn notebook_input_cannot_launder_confidential_history_in_a_fresh_turn() {
    use super::history_tests::end;
    use crate::domain::Ref;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let peer = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq.create_conversation(project, [lead, peer]).unwrap();
    hq.wake(peer, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let source = turn_of(&hq, peer);
    hq.record_receipt(
        &source.token,
        &Called {
            tool: "fixture_read".into(),
            args: String::new(),
            excerpt: "Confidential fixture".into(),
            ok: true,
            act: false,
            confidential: true,
        },
    )
    .unwrap();
    end(&hq, &source);
    hq.wake(peer, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let clean = turn_of(&hq, peer);
    assert_ne!(clean.run, source.run);
    // A historical note becomes visible after briefing, so the explicit read must propagate it.
    hq.store()
        .add_note(
            peer,
            "Confidential notebook fixture",
            &[Ref::Task(crate::domain::TaskId(1))],
            false,
            Some(source.run),
            NOON,
        )
        .unwrap();
    assert!(
        hq.read_notebook_in(clean.run)
            .unwrap()
            .contains("Confidential notebook")
    );
    assert!(
        hq.send_message_in(
            clean.run,
            conversation,
            "notebook_key_1",
            "Confidential notebook fixture",
            &[]
        )
        .is_err()
    );
    end(&hq, &clean);
    hq.wake(peer, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let briefed = turn_of(&hq, peer);
    assert!(briefed.told.contains("Confidential notebook fixture"));
    assert!(
        hq.send_message_in(
            briefed.run,
            conversation,
            "notebook_key_2",
            "Confidential notebook fixture",
            &[]
        )
        .is_err(),
        "automatic notebook context must also propagate its tier"
    );
}

#[test]
fn enabling_messaging_does_not_authorize_a_turn_started_under_the_old_policy() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let a = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let b = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", a, None).unwrap();
    hq.wake(a, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let old = turn_of(&hq, a);
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq.create_conversation(project, [a, b]).unwrap();
    assert!(
        hq.send_message_in(
            old.run,
            conversation,
            "old_turn_key_1",
            "No proven classification",
            &[]
        )
        .unwrap_err()
        .to_string()
        .contains("this turn began before the current messaging policy")
    );
    hq.set_conversations_enabled(false).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    assert!(
        hq.send_message_in(
            old.run,
            conversation,
            "old_turn_key_2",
            "Still not classified",
            &[]
        )
        .unwrap_err()
        .to_string()
        .contains("this turn began before the current messaging policy")
    );
}

#[test]
fn an_assistance_request_is_durable_before_any_expert_turn_starts() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    let before = (hq.wakes().unwrap().len(), hq.store().runs().unwrap().len());
    let id = hq
        .request_assistance_in(
            turn.run,
            conversation,
            "request_key_1",
            "Which synthetic check is decisive?",
            &[],
        )
        .unwrap();
    assert_eq!(
        before,
        (hq.wakes().unwrap().len(), hq.store().runs().unwrap().len()),
        "acceptance commits the delivery intent without starting a model"
    );
    assert_eq!(
        hq.request_assistance_in(
            turn.run,
            conversation,
            "request_key_1",
            "Which synthetic check is decisive?",
            &[],
        )
        .unwrap(),
        id,
        "a lost response retries the same durable request"
    );
    assert!(
        hq.request_assistance_in(
            turn.run,
            conversation,
            "request_key_1",
            "A different objective",
            &[],
        )
        .is_err()
    );
    hq.cancel_assistance_in(turn.run, id).unwrap();
    let (live, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    let replay=super::protocol::call(&hq,&live,&member,"request_assistance",&serde_json::json!({"conversation":conversation,"key":"request_key_1","objective":"Which synthetic check is decisive?"})).unwrap();
    let replay: serde_json::Value = serde_json::from_str(&replay).unwrap();
    assert_eq!(
        replay["state"], "cancelled",
        "retry replies describe the actual retained disposition"
    );
    assert_eq!(replay["scheduled"], false);
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][0]["status"],
        "cancelled"
    );
    hq.tick().unwrap();
    assert_eq!(
        before,
        (hq.wakes().unwrap().len(), hq.store().runs().unwrap().len()),
        "cancelling accepted intent schedules nothing"
    );
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_2",
        "Another synthetic question?",
        &[],
    )
    .unwrap();
    hq.stop_all("Synthetic owner stop").unwrap();
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][1]["status"],
        "cancelled"
    );
    hq.resume().unwrap();
    hq.tick().unwrap();
    assert_eq!(
        before,
        (hq.wakes().unwrap().len(), hq.store().runs().unwrap().len()),
        "resume cannot revive cancelled delivery intent"
    );
}

#[test]
fn a_durable_request_wakes_only_its_authorized_expert_on_the_next_tick() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_1",
        "Which synthetic check is decisive?",
        &[],
    )
    .unwrap();
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    assert!(expert_turn.task.is_none());
    assert!(
        expert_turn
            .told
            .contains("Which synthetic check is decisive?"),
        "the designated expert receives the stored question in a bounded briefing"
    );
    let (expert_run, expert_member) = hq.turn_of(&expert_turn.token).unwrap().unwrap();
    let offered = super::protocol::tools_in(&hq, &expert_run, &expert_member);
    assert!(
        offered
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "answer_assistance")
    );
    assert!(
        !offered
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "office")
    );
    assert!(
        super::protocol::call(
            &hq,
            &expert_run,
            &expert_member,
            "office",
            &serde_json::json!({})
        )
        .unwrap_err()
        .contains("assistance"),
        "tool hiding cannot be the authority boundary"
    );
    let expert_runs = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|run| run.member == expert)
        .count();
    hq.tick().unwrap();
    assert_eq!(
        hq.store()
            .runs()
            .unwrap()
            .into_iter()
            .filter(|run| run.member == expert)
            .count(),
        expert_runs,
        "an unchanged waiting request cannot create a second expert turn"
    );
}

#[test]
fn the_expert_answer_is_accepted_once_and_kept_for_the_owner() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let requester_turn = turn_of(&hq, requester);
    let id = hq
        .request_assistance_in(
            requester_turn.run,
            conversation,
            "request_key_1",
            "Which synthetic check is decisive?",
            &[],
        )
        .unwrap();
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    assert_eq!(
        hq.answer_assistance_in(expert_turn.run, "The synthetic check passed.", &[])
            .unwrap(),
        id
    );
    assert_eq!(
        hq.answer_assistance_in(expert_turn.run, "The synthetic check passed.", &[])
            .unwrap(),
        id,
        "an identical retry returns the accepted result"
    );
    assert!(
        hq.answer_assistance_in(expert_turn.run, "A second answer", &[])
            .is_err(),
        "the first terminal answer wins"
    );
    let page = hq.assistance_requests_page(conversation, 0).unwrap();
    assert_eq!(page["requests"][0]["id"], id);
    assert_eq!(page["requests"][0]["status"], "answered");
    assert_eq!(page["requests"][0]["answer"], "The synthetic check passed.");
    assert_eq!(page["requests"][0]["notified"], false);
    hq.set_conversations_enabled(false).unwrap();
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][0]["status"],
        "answered",
        "a committed answer remains historical evidence when delivery is cancelled"
    );
    assert!(
        !hq.store()
            .run(expert_turn.run)
            .unwrap()
            .unwrap()
            .status
            .live(),
        "disable also stops an expert that answered but has not exited"
    );
}

#[test]
fn an_undelivered_request_expires_without_starting_an_expert() {
    use std::sync::atomic::Ordering;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_1",
        "A synthetic question?",
        &[],
    )
    .unwrap();
    clock.0.fetch_add(16 * 60 * 1_000, Ordering::SeqCst);
    hq.tick().unwrap();
    let page = hq.assistance_requests_page(conversation, 0).unwrap();
    assert_eq!(page["requests"][0]["status"], "expired");
    assert!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .all(|run| run.member != expert)
    );
}

#[test]
fn an_answer_waits_for_the_requesters_running_turn_then_notifies_once() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let requester_turn = turn_of(&hq, requester);
    hq.request_assistance_in(
        requester_turn.run,
        conversation,
        "request_key_1",
        "Synthetic question?",
        &[],
    )
    .unwrap();
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    hq.answer_assistance_in(expert_turn.run, "Synthetic answer.", &[])
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .filter(|run| run.member == requester)
            .count(),
        1,
        "an answer does not add a turn while the requester is running"
    );
    hq.runner().finish(requester_turn.run, true);
    hq.tick().unwrap();
    let notified = turn_of(&hq, requester);
    assert_ne!(notified.run, requester_turn.run);
    assert!(notified.told.contains("Synthetic answer."));
    let count = hq
        .store()
        .runs()
        .unwrap()
        .iter()
        .filter(|run| run.member == requester)
        .count();
    hq.tick().unwrap();
    assert_eq!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .filter(|run| run.member == requester)
            .count(),
        count
    );
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][0]["notified"],
        true
    );
}

#[test]
fn deleting_a_conversation_cancels_its_queued_request_without_a_wake() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_1",
        "Synthetic question?",
        &[],
    )
    .unwrap();
    hq.delete_conversation(conversation).unwrap();
    hq.tick().unwrap();
    assert!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .all(|run| run.member != expert)
    );
    assert!(hq.conversation_messages(conversation, 0).is_err());
}

#[test]
fn deleting_a_conversation_stops_its_live_assistance_before_purging_the_link() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_1",
        "Synthetic question?",
        &[],
    )
    .unwrap();
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    hq.delete_conversation(conversation).unwrap();
    assert!(
        !hq.store()
            .run(expert_turn.run)
            .unwrap()
            .unwrap()
            .status
            .live()
    );
    hq.tick().unwrap();
    assert!(!hq.in_assistance(expert_turn.run).unwrap());
}

#[test]
fn only_an_owner_granted_original_document_can_be_read_for_an_assistance_run() {
    use super::ports::NewTask;
    use crate::domain::assistance::SourceRef;
    use crate::domain::{Document, Priority};
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: "Synthetic check".into(),
                description: String::new(),
                assignee: Some(lead),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let source = turn_of(&hq, lead);
    let document = hq
        .store()
        .add_document(&Document {
            id: 0,
            task,
            run: Some(source.run),
            title: "Synthetic report".into(),
            text: "The independent check passed.".into(),
            at: NOON,
        })
        .unwrap();
    let receipt = hq
        .record_receipt(
            &source.token,
            &Called {
                tool: "fixture_check".into(),
                args: String::new(),
                excerpt: "Synthetic receipt passed".into(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    assert!(
        hq.request_assistance_in(
            turn.run,
            conversation,
            "request_key_1",
            "What did the check show?",
            &[SourceRef::Document(document)]
        )
        .is_err()
    );
    hq.grant_assistance_document(conversation, document)
        .unwrap();
    hq.grant_assistance_receipt(conversation, receipt).unwrap();
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_1",
        "What did the check show?",
        &[SourceRef::Document(document), SourceRef::Receipt(receipt)],
    )
    .unwrap();
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    assert!(
        hq.read_document(expert_turn.run, document, 1)
            .unwrap()
            .contains("independent check passed")
    );
    assert!(
        hq.read_assistance_source(expert_turn.run, SourceRef::Receipt(receipt), 1)
            .unwrap()
            .contains("Synthetic receipt passed")
    );
    assert!(
        hq.read_document(turn.run, document, 1).is_err(),
        "the grant does not give an ordinary requester cross-task access"
    );
    let other_project = hq
        .add_project(goal, "Other fixture", "", lead, None)
        .unwrap();
    let other_scope = hq
        .create_conversation(other_project, [requester, expert])
        .unwrap();
    assert!(
        hq.grant_assistance_document(other_scope, document).is_err(),
        "source project must match the immutable conversation project"
    );
    hq.revoke_assistance_source(conversation, SourceRef::Document(document))
        .unwrap();
    assert!(
        hq.read_document(expert_turn.run, document, 1).is_err(),
        "revocation is checked on every read"
    );
    assert!(
        hq.answer_assistance_in(
            expert_turn.run,
            "An unverified result",
            &[SourceRef::Document(document)]
        )
        .is_err()
    );
    hq.grant_assistance_document(conversation, document)
        .unwrap();
    hq.answer_assistance_in(
        expert_turn.run,
        "The check passed.",
        &[SourceRef::Document(document)],
    )
    .unwrap();
    super::history_tests::end(&hq, &turn);
    let result_turn = turn_of(&hq, requester);
    assert_ne!(result_turn.run, turn.run);
    assert!(
        hq.read_document(result_turn.run, document, 1)
            .unwrap()
            .contains("independent check passed"),
        "the assigned result turn checks the original, without broadening ordinary access"
    );
    clock.0.store(
        NOON + 24 * 60 * 60 * 1000,
        std::sync::atomic::Ordering::SeqCst,
    );
    assert!(
        hq.read_document(result_turn.run, document, 1).is_err(),
        "an expired grant or request cannot keep an expert's reach alive"
    );
}

#[test]
fn disabling_assistance_cancels_pending_and_live_requests() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_1",
        "Synthetic question?",
        &[],
    )
    .unwrap();
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    hq.set_conversations_enabled(false).unwrap();
    assert!(
        !hq.store()
            .run(expert_turn.run)
            .unwrap()
            .unwrap()
            .status
            .live()
    );
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][0]["status"],
        "cancelled"
    );
    hq.set_conversations_enabled(true).unwrap();
    hq.tick().unwrap();
    assert!(
        hq.answer_assistance_in(expert_turn.run, "Late answer", &[])
            .is_err()
    );
    assert_eq!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .filter(|run| run.member == expert)
            .count(),
        1
    );
}

#[test]
fn an_expert_turn_ending_without_an_answer_has_a_visible_outcome() {
    use super::history_tests::end;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    hq.request_assistance_in(
        turn.run,
        conversation,
        "request_key_1",
        "Synthetic question?",
        &[],
    )
    .unwrap();
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    end(&hq, &expert_turn);
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][0]["status"],
        "declined"
    );
}
