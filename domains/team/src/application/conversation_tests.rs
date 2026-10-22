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
