//! Addressed information does not schedule work or transfer authority (ADR 0137).

use std::sync::atomic::AtomicI64;

use super::engine::{Author, Called};
use super::history_tests::{Hands, NOON, engine, turn_of};
use super::ports::TeamStore;
use crate::domain::{Role, WakeReason};

fn canonical_hq(clock: &Hands) -> super::history_tests::Hq<'_> {
    super::engine::Engine::new(
        crate::infrastructure::SqliteTeamStore::in_memory().unwrap(),
        crate::infrastructure::SimulatedRunner::new(8),
        clock,
        super::engine::Settings::default(),
    )
}

#[test]
fn task_help_tool_discovery_does_not_depend_on_the_first_directory_page() {
    use super::ports::NewTask;
    use serde_json::json;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let other = hq
        .add_project(goal, "Other fixture", "", lead, None)
        .unwrap();
    let project = hq
        .add_project(goal, "Task fixture", "", lead, None)
        .unwrap();
    hq.set_conversations_enabled(true).unwrap();
    for _ in 0..50 {
        hq.create_conversation(other, [requester, expert]).unwrap();
    }
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: Some(project),
            parent: None,
            title: "Inspect fixture".into(),
            description: String::new(),
            assignee: Some(requester),
            priority: crate::domain::Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    hq.tick().unwrap();
    let caller = turn_of(&hq, requester);
    let (run, member) = hq.turn_of(&caller.token).unwrap().unwrap();
    let offered = super::protocol::tools_in(&hq, &run, &member);
    for tool in ["consult", "request_assistance", "send_message"] {
        assert!(
            offered
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == tool),
            "authorized task help must remain discoverable beyond page one: {tool}"
        );
    }
    super::protocol::call(
        &hq,
        &run,
        &member,
        "consult",
        &json!({
            "member":"Member 3", "conversation":conversation,
            "key":"later_scope", "question":"What does the fixture show?"
        }),
    )
    .unwrap();
}

#[test]
fn canonical_task_help_uses_one_assistance_lifecycle_and_keeps_task_authority() {
    canonical_task_help_with_finding(false);
}

#[test]
fn a_retained_requester_finding_does_not_restart_model_work_after_worker_loss() {
    canonical_task_help_with_finding(true);
}

fn canonical_task_help_with_finding(lose_worker_after_finding: bool) {
    use super::conversation_ports::ConversationStore;
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use serde_json::{Value, json};
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq
        .add_project(goal, "Fixture project", "", lead, None)
        .unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq
        .create_conversation(project, [requester, expert])
        .unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: "Inspect fixture".into(),
                description: "Find the observed fixture outcome".into(),
                assignee: Some(requester),
                priority: crate::domain::Priority::Normal,
                waits_on: vec![],
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let requester_turn = turn_of(&hq, requester);
    let (run, member) = hq.turn_of(&requester_turn.token).unwrap().unwrap();
    assert!(!hq.store().run_read_confidential(run.id).unwrap());
    // Both actual worker adapters fetch my_turn, then claim before starting a model.
    super::protocol::call(&hq, &run, &member, "my_turn", &json!({})).unwrap();
    let claimed =
        super::protocol::call(&hq, &run, &member, "claim", &json!({"task":task.0})).unwrap();
    assert_eq!(claimed, format!("Task {} is yours.", task.0));
    assert!(
        !hq.store().run_read_confidential(run.id).unwrap(),
        "claim acknowledgment contains no private input"
    );
    let args = json!({"member":"Member 3", "question":"What does the fixture show?", "conversation":conversation, "key":"task_help_01"});
    let accepted = super::protocol::call_v2(&hq, &run, &member, "consult", &args).unwrap();
    let accepted: Value = serde_json::from_str(&accepted).unwrap();
    let id = accepted["request"].as_i64().unwrap();
    let repeated: Value = serde_json::from_str(
        &super::protocol::call_v2(&hq, &run, &member, "consult", &args).unwrap(),
    )
    .unwrap();
    assert_eq!(repeated["request"], id);
    let direct = json!({"conversation":conversation, "key":"task_help_01", "objective":"What does the fixture show?", "evidence":[]});
    let alias: Value = serde_json::from_str(
        &super::protocol::call(&hq, &run, &member, "request_assistance", &direct).unwrap(),
    )
    .unwrap();
    assert_eq!(
        alias["request"], id,
        "the familiar entry point is a canonical alias"
    );
    let direct: Value = serde_json::from_str(
        &super::protocol::call_v2(&hq, &run, &member, "request_assistance", &direct).unwrap(),
    )
    .unwrap();
    assert_eq!(
        direct["request"], id,
        "both v2 entry points share the same durable request"
    );
    let request = hq.store().assistance_request(id).unwrap().unwrap();
    assert_eq!(request.project, project);
    assert_eq!(request.requester_run, run.id);
    assert!(hq.store().consults().unwrap().is_empty());
    assert!(
        super::protocol::call(
            &hq,
            &run,
            &member,
            "record_assistance_finding",
            &json!({"finding":"An ordinary turn cannot finish its own peer exchange."})
        )
        .is_err()
    );
    assert!(
        super::protocol::tools_in_v2(&hq, &run, &member)
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "consult")
    );
    hq.runner().finish(run.id, true);
    hq.tick().unwrap();
    let expert_turn = turn_of(&hq, expert);
    assert!(hq.in_assistance(expert_turn.run).unwrap());
    assert!(!hq.may_use_a_shell(expert_turn.run).unwrap());
    let (expert_run, expert_member) = hq.turn_of(&expert_turn.token).unwrap().unwrap();
    assert!(
        super::protocol::call(
            &hq,
            &expert_run,
            &expert_member,
            "record_assistance_finding",
            &json!({"finding":"An expert cannot write the requester finding."})
        )
        .is_err()
    );
    hq.answer_assistance_in(
        expert_turn.run,
        "The fixture returned its expected marker.",
        &[],
    )
    .unwrap();
    hq.runner().finish(expert_turn.run, true);
    hq.tick().unwrap();
    let result = turn_of(&hq, requester);
    let notice = hq
        .store()
        .assistance_notice_for_run(result.run)
        .unwrap()
        .unwrap();
    assert_eq!(notice.id, id);
    assert!(result.told.contains("What does the fixture show?"));
    assert!(result.told.contains("expected marker"));
    let (result_run, result_member) = hq.turn_of(&result.token).unwrap().unwrap();
    let offered = super::protocol::tools_in(&hq, &result_run, &result_member);
    assert!(
        offered
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "record_assistance_finding")
    );
    assert!(
        !offered
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "answer_assistance")
    );
    let finding =
        "Inconclusive: peer testimony reports the marker; no original evidence was granted.";
    super::protocol::call(
        &hq,
        &result_run,
        &result_member,
        "record_assistance_finding",
        &json!({"finding":finding,"evidence":[]}),
    )
    .unwrap();
    let retained = hq.assistance_requests_page(conversation, 0).unwrap();
    assert_eq!(retained["requests"][0]["finding"]["text"], finding);
    assert_eq!(retained["requests"][0]["finding"]["run"], result.run.0);
    // The exchange is also read where the work is: its task's page, in the same shape.
    let on_task = hq.task_assistance_page(task, 0).unwrap();
    assert_eq!(on_task["task"], task.0);
    assert_eq!(on_task["requests"].as_array().unwrap().len(), 1);
    assert_eq!(on_task["requests"][0]["id"], id);
    assert_eq!(on_task["requests"][0]["conversation"], conversation);
    assert_eq!(
        on_task["requests"][0]["answer"],
        "The fixture returned its expected marker."
    );
    assert_eq!(on_task["requests"][0]["finding"]["text"], finding);
    assert_eq!(on_task["has_more"], false);
    let unrelated = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: "Unrelated fixture".into(),
                description: String::new(),
                assignee: Some(lead),
                priority: crate::domain::Priority::Normal,
                waits_on: vec![],
            },
        )
        .unwrap();
    assert!(
        hq.task_assistance_page(unrelated, 0).unwrap()["requests"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        hq.task_assistance_page(crate::domain::TaskId(999_999), 0)
            .is_err()
    );
    assert!(
        !hq.events_after(0)
            .unwrap()
            .iter()
            .any(|event| event.data.to_string().contains(finding)),
        "finding bodies stay out of the general activity feed"
    );
    super::protocol::call(
        &hq,
        &result_run,
        &result_member,
        "record_assistance_finding",
        &json!({"finding":finding,"evidence":[]}),
    )
    .unwrap();
    assert!(
        super::protocol::call(
            &hq,
            &result_run,
            &result_member,
            "record_assistance_finding",
            &json!({"finding":"A conflicting replacement","evidence":[]})
        )
        .is_err()
    );
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][0]["finding"]["text"],
        finding
    );
    // The feed says who asked whom, the answer and the conclusion, once each despite the
    // retried calls above, and never the words themselves.
    let events = hq.events_after(0).unwrap();
    for kind in ["help.asked", "help.answered", "help.concluded"] {
        let seen: Vec<_> = events.iter().filter(|event| event.kind == kind).collect();
        assert_eq!(seen.len(), 1, "{kind}: {seen:?}");
        let data = &seen[0].data;
        assert_eq!(
            (data["member"].clone(), data["expert"].clone()),
            (json!(requester.0), json!(expert.0)),
            "{kind}"
        );
        assert_eq!(
            (
                data["task"].clone(),
                data["conversation"].clone(),
                data["request"].clone()
            ),
            (json!(task.0), json!(conversation), json!(id)),
            "{kind}"
        );
        for words in ["What does the fixture show?", "expected marker", finding] {
            assert!(!data.to_string().contains(words), "{kind} carried {words}");
        }
    }
    assert_ne!(
        hq.store().task(task).unwrap().unwrap().status,
        crate::domain::TaskStatus::Done
    );
    assert!(
        hq.store()
            .thread(task)
            .unwrap()
            .iter()
            .all(|post| !post.text.contains("expected marker")),
        "peer answers stay in canonical conversation history, not a second task-post lifecycle"
    );
    assert!(hq.store().consults().unwrap().is_empty());
    hq.record_transcript(&result.token, "worker", "Temporary raw transcript")
        .unwrap();
    if lose_worker_after_finding {
        let starts = hq
            .runner()
            .started()
            .iter()
            .filter(|turn| turn.run == result.run)
            .count();
        hq.runner().vanish(result.run);
        hq.tick().unwrap();
        clock
            .0
            .fetch_add(61_000, std::sync::atomic::Ordering::SeqCst);
        hq.tick().unwrap();
        assert_eq!(
            hq.runner()
                .started()
                .iter()
                .filter(|turn| turn.run == result.run)
                .count(),
            starts,
            "a durable finding must not require another model attempt"
        );
    } else {
        hq.runner().finish(result.run, true);
        hq.tick().unwrap();
    }
    clock.0.fetch_add(
        31 * 24 * 60 * 60 * 1000,
        std::sync::atomic::Ordering::SeqCst,
    );
    assert!(hq.prune_transcripts().unwrap() >= 1);
    assert!(hq.store().transcript(result.run).unwrap().is_none());
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"][0]["finding"]["text"],
        finding
    );
    hq.delete_conversation(conversation).unwrap();
    while hq.store().purge_conversation_batch().unwrap() > 0 {}
    assert!(hq.store().assistance_finding(id).unwrap().is_none());
}

#[test]
fn claimed_task_waits_for_its_canonical_result_before_recovery() {
    use super::conversation_ports::ConversationStore;
    use super::ports::NewTask;
    for outcome in ["answered", "cancelled", "expired"] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = canonical_hq(&clock);
        let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
        let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
        let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
        let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
        let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
        hq.set_conversations_enabled(true).unwrap();
        let conversation = hq
            .create_conversation(project, [requester, expert])
            .unwrap();
        let task = hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: Some(project),
                    parent: None,
                    title: "Inspect fixture".into(),
                    description: String::new(),
                    assignee: Some(requester),
                    priority: crate::domain::Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        // Restored/ongoing work may already have spent its recovery nudge.
        hq.claim(requester, task).unwrap();
        hq.store().set_nudged(task, true).unwrap();
        clock.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        hq.tick().unwrap();
        let caller = turn_of(&hq, requester);
        hq.claim_in(caller.run, task).unwrap();
        let request = hq
            .request_assistance_in(
                caller.run,
                conversation,
                "waiting_fixture",
                "What does the fixture show?",
                &[],
            )
            .unwrap();
        hq.runner().finish(caller.run, true);
        hq.tick().unwrap();
        let live_work: Vec<_> = hq
            .store()
            .runs()
            .unwrap()
            .into_iter()
            .filter(|run| run.member == requester && run.task == Some(task) && run.status.live())
            .map(|run| {
                (
                    run.id.0,
                    run.status.name(),
                    hq.store().wake(run.wake).unwrap().unwrap().reason.name(),
                )
            })
            .collect();
        assert!(
            live_work.is_empty(),
            "ordinary recovery must wait for accepted help: {live_work:?}; request={request}"
        );

        match outcome {
            "answered" => {
                let expert_turn = turn_of(&hq, expert);
                hq.answer_assistance_in(
                    expert_turn.run,
                    "The fixture has its expected marker.",
                    &[],
                )
                .unwrap();
                hq.runner().finish(expert_turn.run, true);
                hq.tick().unwrap();
                let result = turn_of(&hq, requester);
                assert_eq!(
                    hq.store()
                        .assistance_notice_for_run(result.run)
                        .unwrap()
                        .unwrap()
                        .id,
                    request
                );
                assert!(
                    hq.store().task_awaits_assistance(task, requester).unwrap(),
                    "verification is still in progress"
                );
                assert!(
                    !hq.store()
                        .runs()
                        .unwrap()
                        .iter()
                        .any(|run| run.member == requester
                            && run.task == Some(task)
                            && run.status.live())
                );
                hq.runner().finish(result.run, true);
                hq.tick().unwrap();
            }
            "cancelled" => {
                hq.cancel_assistance_request(conversation, request).unwrap();
                hq.tick().unwrap();
            }
            "expired" => {
                let due = hq
                    .store()
                    .assistance_request(request)
                    .unwrap()
                    .unwrap()
                    .deadline_at;
                clock.0.store(due + 1, std::sync::atomic::Ordering::SeqCst);
                hq.tick().unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            !hq.store().task_awaits_assistance(task, requester).unwrap(),
            "{outcome}"
        );
        let kept = hq.store().task(task).unwrap().unwrap();
        assert_eq!(kept.owner, Some(requester));
        assert_eq!(kept.status, crate::domain::TaskStatus::Doing);
        assert_eq!(
            hq.store()
                .runs()
                .unwrap()
                .iter()
                .filter(|run| run.member == requester
                    && run.task == Some(task)
                    && run.status.live())
                .count(),
            1,
            "ordinary work resumes after {outcome}"
        );
    }
}

#[test]
fn canonical_task_help_refuses_a_missing_or_mismatched_scope_and_allows_confidential_asks() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use serde_json::json;
    for invalid in ["missing", "mismatch", "unknown", "confidential"] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = canonical_hq(&clock);
        let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
        let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
        let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
        let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
        let project = hq
            .add_project(goal, "Fixture project", "", lead, None)
            .unwrap();
        let task_project = match invalid {
            "missing" => None,
            "mismatch" => Some(
                hq.add_project(goal, "Other project", "", lead, None)
                    .unwrap(),
            ),
            _ => Some(project),
        };
        hq.set_conversations_enabled(true).unwrap();
        let conversation = hq
            .create_conversation(project, [requester, expert])
            .unwrap();
        let task = hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: task_project,
                    parent: None,
                    title: "Inspect fixture".into(),
                    description: "Find the fixture outcome".into(),
                    assignee: Some(requester),
                    priority: crate::domain::Priority::Normal,
                    waits_on: vec![],
                },
            )
            .unwrap();
        if invalid == "unknown" {
            hq.store()
                .post(task, "owner", "Unsourced fixture finding", NOON)
                .unwrap();
        }
        hq.tick().unwrap();
        let turn = turn_of(&hq, requester);
        let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
        if invalid == "confidential" {
            hq.store().mark_confidential_input(run.id).unwrap();
        }
        let private_before_claim = hq.store().run_read_confidential(run.id).unwrap();
        super::protocol::call(&hq, &run, &member, "claim", &json!({"task":task.0})).unwrap();
        assert_eq!(
            hq.store().run_read_confidential(run.id).unwrap(),
            private_before_claim,
            "claim cannot clear or invent private inputs: {invalid}"
        );
        let args = json!({"member":"Member 3","conversation":conversation,"key":"task_refusal_01","question":"What is the observed outcome?"});
        // ADR 0140: unknown or Confidential input no longer stops an ask; its colleague answers
        // on the house's models. A missing or mismatched scope still does.
        if matches!(invalid, "unknown" | "confidential") {
            super::protocol::call_v2(&hq, &run, &member, "consult", &args).unwrap();
            assert_eq!(
                hq.assistance_requests_page(conversation, 0).unwrap()["requests"]
                    .as_array()
                    .unwrap()
                    .len(),
                1,
                "{invalid}"
            );
        } else {
            assert!(
                super::protocol::call_v2(&hq, &run, &member, "consult", &args).is_err(),
                "{invalid}"
            );
            let direct = json!({"conversation":conversation,"key":"task_refusal_01","objective":"What is the observed outcome?"});
            assert!(
                super::protocol::call_v2(&hq, &run, &member, "request_assistance", &direct)
                    .is_err(),
                "{invalid}"
            );
            assert!(
                hq.assistance_requests_page(conversation, 0).unwrap()["requests"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(hq.store().consults().unwrap().is_empty());
        let tools = super::protocol::tools_in_v2(&hq, &run, &member);
        let offered = tools.as_array().unwrap().iter().any(|tool| {
            matches!(
                tool["name"].as_str(),
                Some("consult" | "request_assistance")
            )
        });
        // ADR 0139-0140: a task of an active project may ask any colleague, whatever it read, so
        // the tools stay offered; only a task with no project has no scope.
        assert_eq!(
            offered,
            invalid != "missing",
            "{invalid}: advertise assistance only where a scope can be used"
        );
    }
}

#[test]
fn a_fresh_owner_task_briefing_has_classified_inputs_without_clearing_taint() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let member = hq.hire("Member 1", Role::Developer, 40, 2, 0).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let goal = hq
        .set_goal("Fixture objective", "Owner supplied reason", Author::Owner)
        .unwrap();
    let task = hq
        .add_task_with(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Fixture task".into(),
                description: "Owner supplied work".into(),
                assignee: Some(member),
                priority: crate::domain::Priority::Normal,
                waits_on: vec![],
            },
            &[crate::domain::Criterion::new(
                "Owner supplied criterion",
                false,
            )],
        )
        .unwrap();
    hq.post(Author::Owner, task, "Owner supplied constraint")
        .unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, member);
    assert!(turn.told.contains("Owner supplied work"));
    assert!(turn.told.contains("Owner supplied criterion"));
    assert!(turn.told.contains("Owner supplied constraint"));
    assert!(turn.told.contains("Fixture objective"));
    assert!(
        !hq.store().run_read_confidential(turn.run).unwrap(),
        "typed owner inputs should not be blanket-classified as unknown"
    );
}

#[test]
fn canonical_task_briefings_preserve_unknown_constraints_and_fail_closed() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use crate::domain::{Criterion, GoalStatus, Priority};
    for unknown in [
        "body", "criteria", "goal", "parent", "post", "note", "strategy",
    ] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = canonical_hq(&clock);
        let member = hq.hire("Member 1", Role::Developer, 40, 2, 0).unwrap();
        hq.set_conversations_enabled(true).unwrap();
        let goal = if unknown == "goal" {
            hq.store()
                .add_goal("Fixture goal", "", GoalStatus::Active, None)
                .unwrap()
        } else {
            hq.set_goal("Fixture goal", "", Author::Owner).unwrap()
        };
        let new_task = NewTask {
            goal,
            project: None,
            parent: None,
            title: "Fixture task".into(),
            description: "Fixture body".into(),
            assignee: Some(member),
            priority: Priority::Normal,
            waits_on: vec![],
        };
        let parent = if unknown == "parent" {
            Some(
                hq.store()
                    .add_task(
                        &NewTask {
                            title: "Unclassified ancestor".into(),
                            assignee: None,
                            ..new_task.clone()
                        },
                        0,
                    )
                    .unwrap(),
            )
        } else {
            None
        };
        let new_task = NewTask { parent, ..new_task };
        let criteria = [Criterion::new("Fixture criterion", false)];
        let task = if unknown == "body" {
            let task = hq.store().add_task(&new_task, 0).unwrap();
            hq.wake(member, Some(task), WakeReason::Assigned).unwrap();
            task
        } else {
            hq.add_task_with(Author::Owner, new_task, &criteria)
                .unwrap()
        };
        match unknown {
            "criteria" => hq.store().set_criteria(task, &criteria).unwrap(),
            "post" => hq
                .store()
                .post(task, "owner", "Unclassified input", NOON)
                .unwrap(),
            "note" => hq.set_goal_note(goal, "Unclassified input").unwrap(),
            "strategy" => {
                hq.set_strategy(
                    goal,
                    &crate::domain::areas::StrategyEdit {
                        not_now: Some("Unclassified input".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
                // As one written before its writer was kept.
                hq.store()
                    .set_setting(&format!("not_now_origin:{}", goal.0), "")
                    .unwrap();
            }
            _ => {}
        }
        // A new owner post cannot launder older inputs that remain in the briefing.
        hq.post(Author::Owner, task, "New owner input").unwrap();
        hq.tick().unwrap();
        let turn = turn_of(&hq, member);
        let expected = match unknown {
            "body" => "Fixture body",
            "criteria" => "Fixture criterion",
            "goal" => "Fixture goal",
            "parent" => "Unclassified ancestor",
            _ => "Unclassified input",
        };
        assert!(
            turn.told.contains(expected),
            "{unknown}: its constraint must remain visible"
        );
        assert!(turn.told.contains("New owner input"));
        if unknown == "note" {
            let at = turn
                .told
                .lines()
                .position(|line| line.starts_with("Notes from the owner for this goal:"))
                .unwrap();
            assert!(
                at <= 4,
                "owner constraints must remain near the top: {}",
                turn.told
            );
        }
        assert!(
            hq.store().run_read_confidential(turn.run).unwrap(),
            "{unknown}: a clean post must not launder other rendered inputs"
        );
    }
}

#[test]
fn canonical_task_briefings_inherit_source_runs_without_using_display_authors() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use crate::domain::history::InputOrigin;
    for confidential in [false, true] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = canonical_hq(&clock);
        let source = hq.hire("Member 1", Role::Researcher, 30, 2, 0).unwrap();
        let reader = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
        hq.set_conversations_enabled(true).unwrap();
        hq.wake(source, None, WakeReason::Downtime).unwrap();
        hq.tick().unwrap();
        let source_run = turn_of(&hq, source).run;
        if confidential {
            hq.store().mark_confidential_input(source_run).unwrap();
        }
        hq.rename(source, "Member 3").unwrap();
        let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
        let task = hq
            .store()
            .add_task_from(
                &NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Source-authored task".into(),
                    description: "Source-authored work".into(),
                    assignee: Some(reader),
                    priority: crate::domain::Priority::Normal,
                    waits_on: vec![],
                },
                0,
                InputOrigin::Run(source_run),
            )
            .unwrap();
        hq.post(Author::Owner, task, "Later owner input").unwrap();
        hq.wake(reader, Some(task), WakeReason::Assigned).unwrap();
        hq.tick().unwrap();
        let received = turn_of(&hq, reader);
        assert!(received.told.contains("Source-authored work"));
        assert!(received.told.contains("Later owner input"));
        assert_eq!(
            hq.store().run_read_confidential(received.run).unwrap(),
            confidential,
            "source tier, not a later owner post or renamed display author, governs sharing"
        );
    }
}

#[test]
fn canonical_task_briefings_bound_context_while_retaining_older_posts() {
    use super::history_ports::HistoryStore;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let member = hq.hire("Member 1", Role::Developer, 40, 2, 0).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let task = super::history_tests::task_for(&hq, goal, member, "Fixture check");
    for n in 0..60 {
        hq.post(Author::Owner, task, &format!("Fixture post {n:03}"))
            .unwrap();
    }
    hq.tick().unwrap();
    let received = turn_of(&hq, member);
    assert!(!received.told.contains("Fixture post 047"));
    assert!(received.told.contains("Fixture post 048"));
    assert!(received.told.contains("Fixture post 059"));
    assert_eq!(received.told.matches("- owner: Fixture post").count(), 12);
    assert_eq!(hq.store().thread(task).unwrap().len(), 60);
    assert!(!hq.store().run_read_confidential(received.run).unwrap());
}

#[test]
fn returning_task_posts_inherits_their_sources_before_exposing_the_text() {
    use super::history_ports::HistoryStore;
    use super::ports::{NewTask, Post};
    use crate::domain::history::InputOrigin;
    for unknown in [true, false] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = engine(&clock);
        let reader = hq.hire("Member 1", Role::Developer, 40, 2, 0).unwrap();
        let source = hq.hire("Member 2", Role::Researcher, 30, 2, 0).unwrap();
        hq.set_conversations_enabled(true).unwrap();
        hq.wake(reader, None, WakeReason::Downtime).unwrap();
        hq.wake(source, None, WakeReason::Downtime).unwrap();
        hq.tick().unwrap();
        let reader = turn_of(&hq, reader).run;
        let source = turn_of(&hq, source).run;
        assert!(!hq.store().run_read_confidential(reader).unwrap());
        let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
        let task = hq
            .add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Fixture check".into(),
                    description: String::new(),
                    assignee: None,
                    priority: crate::domain::Priority::Normal,
                    waits_on: vec![],
                },
            )
            .unwrap();
        hq.post(Author::Owner, task, "Owner supplied finding")
            .unwrap();
        let owner = hq.latest_task_posts(reader, task, 12).unwrap();
        assert_eq!(owner.len(), 1);
        assert_eq!(owner[0].origin, InputOrigin::Owner);
        assert!(!hq.store().run_read_confidential(reader).unwrap());
        hq.store().mark_confidential_input(source).unwrap();
        hq.store()
            .post_from(
                task,
                &Post {
                    author: "owner".into(),
                    text: "Untrusted finding".into(),
                    at: NOON,
                    unsupported: true,
                },
                &[],
                if unknown {
                    InputOrigin::Unknown
                } else {
                    InputOrigin::Run(source)
                },
            )
            .unwrap();
        let received = hq.latest_task_posts(reader, task, 1).unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].post.text, "Untrusted finding");
        assert!(
            hq.store().run_read_confidential(reader).unwrap(),
            "unknown={unknown}: receiving a post must inherit its source tier"
        );
        hq.post(Author::Owner, task, "New owner finding").unwrap();
        hq.latest_task_posts(reader, task, 1).unwrap();
        assert!(
            hq.store().run_read_confidential(reader).unwrap(),
            "a subsequent owner input cannot lower the reader's tier"
        );
    }
}

#[test]
fn authoring_owner_work_records_typed_sources_without_display_name_inference() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use crate::domain::history::{InputOrigin, InputRef};
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let task = hq
        .add_task_with(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Fixture check".into(),
                description: "Inspect the fixture".into(),
                assignee: None,
                priority: crate::domain::Priority::Normal,
                waits_on: vec![],
            },
            &[crate::domain::Criterion::new(
                "Fixture output is inspected",
                false,
            )],
        )
        .unwrap();
    assert_eq!(
        hq.store().input_origin(InputRef::Goal(goal)).unwrap(),
        InputOrigin::Owner
    );
    assert_eq!(
        hq.store().input_origin(InputRef::TaskBody(task)).unwrap(),
        InputOrigin::Owner
    );
    assert_eq!(
        hq.store()
            .input_origin(InputRef::TaskCriteria(task))
            .unwrap(),
        InputOrigin::Owner
    );
    assert!(hq.store().thread(task).unwrap().is_empty());
    hq.post(Author::Owner, task, "Owner supplied finding")
        .unwrap();
    assert_eq!(
        hq.store().input_origin(InputRef::Post(1)).unwrap(),
        InputOrigin::Owner
    );
}

#[test]
fn authoring_a_member_post_keeps_the_authenticated_source_run() {
    use super::history_ports::HistoryStore;
    use crate::domain::history::{InputOrigin, InputRef};
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let member = hq.hire("Member 1", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let task = super::history_tests::task_for(&hq, goal, member, "Fixture check");
    hq.tick().unwrap();
    let turn = turn_of(&hq, member);
    assert!(hq.store().thread(task).unwrap().is_empty());
    hq.post_in(turn.run, task, "Source-backed fixture finding")
        .unwrap();
    hq.rename(member, "Member 2").unwrap();
    assert_eq!(
        hq.store().input_origin(InputRef::Post(1)).unwrap(),
        InputOrigin::Run(turn.run)
    );
    let created = hq
        .create_task_in(
            turn.run,
            super::ports::NewTask {
                goal,
                project: None,
                parent: None,
                title: "Additional fixture result".into(),
                description: "Another synthetic responsibility".into(),
                assignee: None,
                priority: crate::domain::Priority::Normal,
                waits_on: vec![],
            },
            &[],
            None,
        )
        .unwrap();
    assert_eq!(
        hq.store()
            .input_origin(InputRef::TaskBody(created))
            .unwrap(),
        InputOrigin::Run(turn.run)
    );
    assert_eq!(
        hq.store()
            .input_origin(InputRef::TaskCriteria(created))
            .unwrap(),
        InputOrigin::Run(turn.run)
    );
}

#[test]
fn canonical_protocol_aliases_hide_disabled_help_without_suggesting_a_task_detour() {
    use serde_json::json;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let requester = hq.hire("Member 1", Role::Developer, 40, 2, 0).unwrap();
    hq.hire("Member 2", Role::Researcher, 30, 2, 0).unwrap();
    hq.wake(requester, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    let briefing = super::protocol::call(&hq, &run, &member, "my_turn", &json!({})).unwrap();
    assert!(
        briefing.starts_with("You are Member 1,"),
        "worker identity is parsed from the first line: {briefing}"
    );
    let offered = super::protocol::tools_in(&hq, &run, &member);
    for tool in [
        "consult",
        "send_message",
        "request_assistance",
        "answer_consult",
    ] {
        assert!(
            !offered
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == tool)
        );
    }
    let error = super::protocol::call(
        &hq,
        &run,
        &member,
        "consult",
        &json!({
            "member":"Member 2", "question":"What does the fixture show?"
        }),
    )
    .unwrap_err();
    assert!(error.contains("disabled"), "{error}");
    assert!(
        !error.contains("name one with"),
        "must not recommend a retired task detour: {error}"
    );
}

#[test]
fn retained_legacy_requests_still_count_toward_the_canonical_daily_allowance() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use crate::domain::consult::{Consult, Status};
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let asker = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq.create_conversation(project, [asker, expert]).unwrap();
    hq.tick().unwrap();
    hq.wake(asker, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let caller = turn_of(&hq, asker);
    assert!(caller.task.is_none());
    for n in 0..2 {
        let request = hq
            .request_assistance_in(
                caller.run,
                conversation,
                &format!("shared_limit_{n}"),
                "Which fixture failed?",
                &[],
            )
            .unwrap();
        hq.cancel_assistance_in(caller.run, request).unwrap();
    }
    hq.runner().finish(caller.run, true);
    hq.tick().unwrap();
    let fresh_task = |title: &str| {
        hq.add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: title.into(),
                description: String::new(),
                assignee: Some(asker),
                priority: crate::domain::Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap()
    };
    for round in 0..2 {
        let task = fresh_task(&format!("Historical fixture {round}"));
        hq.tick().unwrap();
        let current = turn_of(&hq, asker);
        assert_eq!(current.task, Some(task));
        for n in 0..2 {
            // Retained pre-upgrade history; no old execution engine is called.
            hq.store()
                .seed_legacy_consultation(&Consult {
                    id: 0,
                    asker,
                    asker_run: current.run,
                    expert,
                    topic: Some(task),
                    thread: task,
                    question: format!("Which fixture failed in case {round}-{n}?"),
                    status: Status::Cancelled,
                    answer_run: None,
                    why: "Retired before cutover".into(),
                    asker_woken: false,
                    at: NOON,
                })
                .unwrap();
        }
        hq.runner().finish(current.run, true);
        hq.drop_task(task).unwrap();
        hq.tick().unwrap();
    }
    fresh_task("Current canonical fixture");
    hq.tick().unwrap();
    let current = turn_of(&hq, asker);
    assert!(hq.turn_of(&current.token).unwrap().is_some());
    let error = hq
        .consult_peer_in(
            current.run,
            "Member 3",
            "A distinct seventh question?",
            None,
            None,
            &[],
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("two per turn and six per day"),
        "{error}"
    );
}

#[test]
fn a_turn_without_the_help_tools_is_not_told_to_use_them() {
    use super::ports::NewTask;
    // Production, 2026-10-24: every task turn on the older briefing read unclassified thread
    // history, so the help tools were withheld, yet its briefing said to use consult.
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let developer = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    hq.create_conversation(project, [lead, developer]).unwrap();
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: Some(project),
            parent: None,
            title: "Check the fixture".into(),
            description: String::new(),
            assignee: Some(lead),
            priority: crate::domain::Priority::Normal,
            waits_on: vec![],
        },
    )
    .unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, lead);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    let offered = super::protocol::tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "consult");
    // ADR 0138 A2c-d: a lead's task turn showing only the owner's and HQ's words is no longer
    // locked whole, and its briefing tells of help exactly when the tools are offered.
    assert!(offered, "a lead's clean task turn is offered help");
    assert_eq!(
        turn.told.contains("Colleagues: you may ask anyone"),
        offered,
        "{}",
        turn.told
    );
}

#[test]
fn downtime_briefing_lists_authorized_peer_tools_and_keeps_task_creation_refused() {
    use serde_json::json;
    for (enabled, has_scope) in [(true, true), (true, false), (false, true)] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = engine(&clock);
        let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
        let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
        let expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
        let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
        let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
        if has_scope {
            hq.set_conversations_enabled(true).unwrap();
            hq.create_conversation(project, [requester, expert])
                .unwrap();
        }
        hq.set_conversations_enabled(enabled).unwrap();
        hq.wake(requester, None, WakeReason::Downtime).unwrap();
        hq.tick().unwrap();
        let turn = turn_of(&hq, requester);
        let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
        let summary = turn
            .told
            .lines()
            .find(|line| line.starts_with("Your tools at HQ:"))
            .unwrap();
        let eligible = enabled && has_scope;
        let advertised = super::protocol::tools_in(&hq, &run, &member);
        let canonical = super::protocol::tools_in_v2(&hq, &run, &member);
        assert_eq!(
            canonical
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "consult"),
            eligible,
            "v2 must not advertise a legacy fallback when conversation scope is absent"
        );
        for tool in [
            "read_messages",
            "send_message",
            "consult",
            "request_assistance",
            "cancel_assistance",
        ] {
            assert_eq!(summary.contains(tool), eligible, "{tool}: {summary}");
            if eligible {
                assert!(
                    advertised
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|entry| entry["name"] == tool)
                );
            }
        }
        if eligible {
            assert!(!turn.told.contains("nothing is due"), "{}", turn.told);
            assert!(turn.told.contains("no task is due"), "{}", turn.told);
        }
        assert_eq!(
            turn.told.contains("Colleagues: you may ask anyone"),
            eligible,
            "the briefing tells of help exactly when the tools are offered"
        );
        assert!(turn.task.is_none());
        let refused = super::protocol::call(
            &hq,
            &run,
            &member,
            "create_task",
            &json!({
                "title":"Synthetic task", "description":"Fixture", "assignee":"Member 3"
            }),
        )
        .unwrap_err();
        assert!(refused.contains("downtime"), "{refused}");
        assert!(hq.store().tasks().unwrap().is_empty());
    }
}

#[test]
fn normal_taskless_consult_uses_the_authorized_assistance_path_and_retries_once() {
    use super::history_ports::HistoryStore;
    use serde_json::json;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let a = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let b = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq
        .set_goal("Synthetic diagnostic", "", Author::Owner)
        .unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let conversation = hq.create_conversation(project, [a, b]).unwrap();
    hq.wake(a, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, a);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    let args = json!({"member":"Member 3","question":"Which synthetic dependency failed?"});
    let before = hq.store().tasks().unwrap().len();
    let first = super::protocol::call(&hq, &run, &member, "consult", &args).unwrap();
    assert!(turn.told.contains("consult"));
    assert!(turn.told.contains("read_messages"));
    let retry = super::protocol::call(&hq, &run, &member, "consult", &args).unwrap();
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    let retry: serde_json::Value = serde_json::from_str(&retry).unwrap();
    assert_eq!(first["request"], retry["request"]);
    assert_eq!(first["conversation"], conversation);
    assert!(!hq.store().run_read_confidential(run.id).unwrap());
    assert_eq!(hq.store().tasks().unwrap().len(), before);
    assert!(hq.store().consults().unwrap().is_empty());
    hq.tick().unwrap();
    let expert = turn_of(&hq, b);
    assert!(hq.in_assistance(expert.run).unwrap());
    assert!(
        !hq.may_use_a_shell(expert.run).unwrap(),
        "read-only expert must not be handed a shell"
    );
    assert_eq!(
        hq.lab_tools_of(expert.run).unwrap(),
        Some(Vec::new()),
        "read-only expert has no node capabilities"
    );
    hq.answer_assistance_in(expert.run, "The fixture cache was unavailable.", &[])
        .unwrap();
    hq.runner().finish(turn.run, true);
    hq.runner().finish(expert.run, true);
    hq.tick().unwrap();
    let result = turn_of(&hq, a);
    assert!(result.told.contains("fixture cache"));
    assert!(
        result.told.contains("Which synthetic dependency failed?"),
        "verification must retain the original question: {}",
        result.told
    );
    for briefing in [&expert.told, &result.told] {
        for guidance in [
            "Separate what the originals establish, what you infer, and what remains unknown.",
            "Label inference explicitly and cite its basis.",
            "Do not rule out other causes or predict a repair will succeed without supporting evidence.",
            "Finish with a concise finding; if the evidence is insufficient, say so.",
            "In your final finding, explicitly restate the key evidence that answers the question and cite the original locators; agreement with a colleague's summary is insufficient.",
        ] {
            assert!(
                briefing.contains(guidance),
                "missing {guidance}: {briefing}"
            );
        }
    }
    assert!(
        !hq.may_use_a_shell(result.run).unwrap(),
        "source-verification result must not be handed a shell"
    );
    assert_eq!(
        hq.lab_tools_of(result.run).unwrap(),
        Some(Vec::new()),
        "source verification has no node capabilities"
    );
    let (result_run, result_member) = hq.turn_of(&result.token).unwrap().unwrap();
    let offered = super::protocol::tools_in(&hq, &result_run, &result_member);
    assert!(
        offered
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "read_assistance_source")
    );
    for tool in ["office", "create_task", "consult", "answer_assistance"] {
        assert!(
            !offered
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == tool)
        );
        assert!(
            super::protocol::call(&hq, &result_run, &result_member, tool, &json!({}))
                .unwrap_err()
                .contains("read-only assistance"),
            "{tool}"
        );
    }
    for _ in 0..crate::domain::assistance::READ_ROUNDS {
        super::protocol::call(&hq, &result_run, &result_member, "my_turn", &json!({})).unwrap();
    }
    let exhausted =
        super::protocol::call(&hq, &result_run, &result_member, "my_turn", &json!({})).unwrap_err();
    assert!(exhausted.contains("six read rounds"), "{exhausted}");
    assert!(exhausted.contains("finish now"), "{exhausted}");
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(hq.store().tasks().unwrap().len(), before);
}

#[test]
fn normal_consult_requires_explicit_scope_when_a_pair_has_two_conversations() {
    use serde_json::json;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let a = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let b = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq
        .add_project(goal, "First project", "", lead, None)
        .unwrap();
    let other = hq
        .add_project(goal, "Second project", "", lead, None)
        .unwrap();
    hq.set_conversations_enabled(true).unwrap();
    let c1 = hq.create_conversation(project, [a, b]).unwrap();
    let c2 = hq.create_conversation(other, [b, a]).unwrap();
    hq.wake(a, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, a);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    let args = json!({"member":"Member 3","question":"Which fixture failed?"});
    let error = super::protocol::call(&hq, &run, &member, "consult", &args).unwrap_err();
    assert!(error.contains("conversation"), "{error}");
    let selected = json!({"member":"Member 3","question":"Which fixture failed?","conversation":c2,"key":"normal_consult_1"});
    let accepted: serde_json::Value = serde_json::from_str(
        &super::protocol::call(&hq, &run, &member, "consult", &selected).unwrap(),
    )
    .unwrap();
    assert_eq!(accepted["conversation"], c2);
    assert!(
        hq.assistance_requests_page(c1, 0).unwrap()["requests"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        hq.assistance_requests_page(c2, 0).unwrap()["requests"][0]["project"],
        other.0
    );
}

#[test]
fn normal_consult_preserves_scope_source_and_confidentiality_boundaries() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use crate::domain::Priority;
    use crate::domain::assistance::SourceRef;
    use serde_json::json;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let a = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let b = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.add_task(
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
    let receipt = hq
        .record_receipt(
            &source.token,
            &Called {
                tool: "fixture_check".into(),
                args: String::new(),
                excerpt: "Synthetic dependency failed".into(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    hq.wake(a, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, a);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    let args = json!({"member":"Member 3","question":"Which fixture failed?"});
    assert!(super::protocol::call(&hq, &run, &member, "consult", &args).is_err());
    hq.set_conversations_enabled(true).unwrap();
    assert!(super::protocol::call(&hq, &run, &member, "consult", &args).is_err());
    let foreign = hq.create_conversation(project, [lead, b]).unwrap();
    let forged =
        json!({"member":"Member 3","question":"Which fixture failed?","conversation":foreign});
    assert!(super::protocol::call(&hq, &run, &member, "consult", &forged).is_err());
    let conversation = hq.create_conversation(project, [a, b]).unwrap();
    // A turn started before enabling messaging must stay refused. Start a fresh turn
    // to test source admission under the current policy rather than that stale-turn guard.
    hq.runner().finish(turn.run, true);
    hq.tick().unwrap();
    hq.wake(a, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, a);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    assert!(super::protocol::call(&hq, &run, &member, "consult", &forged).is_err());
    let evidence = json!({"member":"Member 3","question":"Which fixture failed?","conversation":conversation,"key":"normal_sources_1","evidence":[format!("r{receipt}")]});
    assert!(super::protocol::call(&hq, &run, &member, "consult", &evidence).is_err());
    assert!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    hq.grant_assistance_receipt(conversation, receipt).unwrap();
    super::protocol::call(&hq, &run, &member, "consult", &evidence).unwrap();
    hq.tick().unwrap();
    let expert = turn_of(&hq, b);
    assert!(
        hq.read_assistance_source(expert.run, SourceRef::Receipt(receipt), 1)
            .unwrap()
            .contains("Synthetic dependency failed")
    );
    hq.revoke_assistance_source(conversation, SourceRef::Receipt(receipt))
        .unwrap();
    assert!(
        hq.read_assistance_source(expert.run, SourceRef::Receipt(receipt), 1)
            .is_err()
    );
    hq.store().mark_confidential_input(run.id).unwrap();
    assert!(super::protocol::call(&hq, &run, &member, "consult", &args).is_err());
    assert_eq!(
        hq.assistance_requests_page(conversation, 0).unwrap()["requests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(hq.store().consults().unwrap().is_empty());
}

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

/// ADR 0139: one team. A member on a project's task asks any colleague with consult, and no
/// owner-made pair is needed: the pair's conversation is made on that project when first asked.
/// A task with no project still has nothing to charge, and the CEO is not asked this way.
#[test]
fn any_member_asks_any_colleague_from_a_projects_task_without_an_owner_made_pair() {
    use super::ports::NewTask;
    use serde_json::{Value, json};
    let clock = Hands(AtomicI64::new(NOON));
    let hq = canonical_hq(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let requester = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let _expert = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let _ceo = hq.hire("Member 4", Role::Ceo, 35, 1, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    assert!(
        hq.conversation_directory(None, 0).unwrap()["conversations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: "Inspect fixture".into(),
                description: String::new(),
                assignee: Some(requester),
                priority: crate::domain::Priority::Normal,
                waits_on: vec![],
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, requester);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    super::protocol::call(&hq, &run, &member, "claim", &json!({"task": task.0})).unwrap();
    assert!(
        super::protocol::tools_in(&hq, &run, &member)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "consult"),
        "a project's task may ask a colleague before any conversation exists"
    );
    // The CEO is not asked as a peer: its word is a decision, through ask_owner.
    assert!(
        hq.consult_peer_in(
            run.id,
            "Member 4",
            "Is the fixture healthy?",
            None,
            None,
            &[]
        )
        .is_err()
    );
    let asked: Value = serde_json::from_str(
        &hq.consult_peer_in(
            run.id,
            "Member 3",
            "What does the fixture show?",
            None,
            None,
            &[],
        )
        .unwrap(),
    )
    .unwrap();
    let made = asked["conversation"].as_i64().unwrap();
    let directory = hq.conversation_directory(None, 0).unwrap();
    let rows = directory["conversations"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{directory}");
    assert_eq!(rows[0]["id"], made);
    assert_eq!(rows[0]["project"], project.0);
    // Asked again (refused while the first is open), no second conversation is made.
    let _ = hq.consult_peer_in(
        run.id,
        "Member 3",
        "And the second fixture?",
        None,
        Some("second_q"),
        &[],
    );
    assert_eq!(
        hq.conversation_directory(None, 0).unwrap()["conversations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

/// ADR 0138 A2c: the older task briefing (every turn the classified one does not serve) is
/// classified section by section instead of locked whole: owner and HQ inputs leave the turn
/// free to ask, and each unknown or Confidential input it shows still locks it.
#[test]
fn the_older_task_briefing_is_classified_by_section_and_fails_closed_on_what_it_cannot_trace() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use crate::domain::Criterion;
    for case in [
        "clean",
        "unknown post",
        "confidential post",
        "note",
        "strategy",
        "owner's strategy",
        "document, clean run",
        "document, no run",
        "old unknown post, not shown",
        "old unknown post, a question reads it",
    ] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = canonical_hq(&clock);
        let source = hq.hire("Member 1", Role::Researcher, 30, 2, 0).unwrap();
        let member = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
        hq.set_conversations_enabled(true).unwrap();
        let goal = hq.set_goal("Fixture goal", "", Author::Owner).unwrap();
        let task = hq
            .add_task_with(
                Author::Owner,
                NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: "Fixture task".into(),
                    description: "Fixture body".into(),
                    assignee: Some(member),
                    priority: crate::domain::Priority::Normal,
                    waits_on: vec![],
                },
                &[Criterion::new("Fixture criterion", false)],
            )
            .unwrap();
        if case.starts_with("old unknown post") {
            hq.store()
                .post(task, "owner", "Unclassified older input", NOON)
                .unwrap();
        }
        hq.tick().unwrap();
        let first = turn_of(&hq, member);
        hq.runner().finish(first.run, true);
        hq.tick().unwrap();
        clock
            .0
            .fetch_add(60_000, std::sync::atomic::Ordering::SeqCst);
        match case {
            "unknown post" => {
                hq.store()
                    .post(task, "owner", "Unclassified input", NOON + 60_000)
                    .unwrap();
            }
            "confidential post" => {
                hq.wake(source, None, WakeReason::Downtime).unwrap();
                hq.tick().unwrap();
                let source_run = turn_of(&hq, source).run;
                hq.store().mark_confidential_input(source_run).unwrap();
                hq.store()
                    .post_from(
                        task,
                        &super::ports::Post {
                            author: "Member 1".into(),
                            text: "Confidential-derived input".into(),
                            at: NOON + 60_000,
                            unsupported: false,
                        },
                        &[],
                        crate::domain::history::InputOrigin::Run(source_run),
                    )
                    .unwrap();
                hq.runner().finish(source_run, true);
                hq.tick().unwrap();
            }
            "note" => hq.set_goal_note(goal, "Unclassified input").unwrap(),
            "strategy" | "owner's strategy" => {
                hq.set_strategy(
                    goal,
                    &crate::domain::areas::StrategyEdit {
                        not_now: Some("Unclassified input".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
                if case == "strategy" {
                    // As one written before its writer was kept.
                    hq.store()
                        .set_setting(&format!("not_now_origin:{}", goal.0), "")
                        .unwrap();
                }
            }
            "document, clean run" | "document, no run" => {
                hq.wake(source, None, WakeReason::Downtime).unwrap();
                hq.tick().unwrap();
                let source_run = turn_of(&hq, source).run;
                hq.store()
                    .add_document(&crate::domain::wing::Document {
                        id: 0,
                        task,
                        run: (case == "document, clean run").then_some(source_run),
                        title: "Fixture notes".into(),
                        text: "Fixture document".into(),
                        at: NOON,
                    })
                    .unwrap();
                hq.runner().finish(source_run, true);
                hq.tick().unwrap();
            }
            _ => {}
        }
        // A mention or a question: not the classified briefing's reasons, so the older one
        // serves them.
        let reason = if case.ends_with("a question reads it") {
            WakeReason::QuestionAsked
        } else {
            WakeReason::Mentioned
        };
        hq.wake(member, Some(task), reason).unwrap();
        hq.tick().unwrap();
        let turn = turn_of(&hq, member);
        assert_ne!(turn.run, first.run);
        assert!(turn.told.contains("Fixture body"), "{case}");
        assert!(
            turn.told.contains("Your tools at HQ"),
            "the older briefing: {case}"
        );
        assert_eq!(
            hq.store().run_read_confidential(turn.run).unwrap(),
            !matches!(
                case,
                "clean"
                    | "old unknown post, not shown"
                    | "owner's strategy"
                    | "document, clean run"
            ),
            "{case}: {}",
            turn.told
        );
    }
}

/// ADR 0138 A2d: the house page a lead is briefed is classified by the turns that wrote it: a
/// section or timeline line from a clean turn leaves the lead free to ask; one from a
/// Confidential turn, or with no recorded turn, locks it.
#[test]
fn a_leads_house_page_is_classified_by_the_turns_that_wrote_it() {
    use super::history_ports::HistoryStore;
    use super::ports::NewTask;
    use super::world_ports::WorldStore;
    use crate::domain::world::{HOUSE, Section, Stage};
    use crate::domain::world_migration::TimelineLine;
    for case in [
        "clean section",
        "confidential section",
        "section, no run",
        "line, no run",
    ] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = canonical_hq(&clock);
        let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
        let source = hq.hire("Member 2", Role::Researcher, 30, 2, 0).unwrap();
        let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
        let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
        hq.set_conversations_enabled(true).unwrap();
        hq.wake(source, None, WakeReason::Downtime).unwrap();
        hq.tick().unwrap();
        let source_run = turn_of(&hq, source).run;
        if case == "confidential section" {
            hq.store().mark_confidential_input(source_run).unwrap();
        }
        hq.runner().finish(source_run, true);
        hq.tick().unwrap();
        hq.store()
            .save_section(
                &Section {
                    id: 0,
                    page: HOUSE.slug.into(),
                    anchor: "nas".into(),
                    heading: "The NAS".into(),
                    headline: "The NAS holds the media.".into(),
                    body: String::new(),
                    stage: Stage::Canon,
                    warrant: "read".into(),
                    evidence: Vec::new(),
                    verified_at: Some(NOON),
                    verified_by: "Member 2".into(),
                    superseded_by: None,
                    why: String::new(),
                    written_by: "Member 2".into(),
                    written_in_run: (case != "section, no run").then_some(source_run),
                    revision: 1,
                    updated_at: NOON,
                    last_used_at: NOON,
                },
                "fixture",
            )
            .unwrap();
        if case == "line, no run" {
            // A line on its timeline since the lead's last turn, which no turn wrote.
            hq.wake(lead, None, WakeReason::Downtime).unwrap();
            hq.tick().unwrap();
            let before = turn_of(&hq, lead).run;
            hq.runner().finish(before, true);
            hq.tick().unwrap();
            clock
                .0
                .fetch_add(60_000, std::sync::atomic::Ordering::SeqCst);
            hq.store()
                .add_timeline(
                    &TimelineLine {
                        page: HOUSE.slug.into(),
                        at: NOON + 30_000,
                        by: "HQ".into(),
                        line: "The router was replaced.".into(),
                    },
                    None,
                )
                .unwrap();
        }
        hq.add_task(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: "Check the fixture".into(),
                description: String::new(),
                assignee: Some(lead),
                priority: crate::domain::Priority::Normal,
                waits_on: vec![],
            },
        )
        .unwrap();
        hq.tick().unwrap();
        let turn = turn_of(&hq, lead);
        assert!(turn.told.contains("The NAS holds the media."), "{case}");
        if case == "line, no run" {
            assert!(
                turn.told.contains("The router was replaced."),
                "{}",
                turn.told
            );
        }
        assert_eq!(
            hq.store().run_read_confidential(turn.run).unwrap(),
            case != "clean section",
            "{case}: {}",
            turn.told
        );
    }
}

/// ADR 0140: a turn that read Confidential lab data may ask a colleague, as long as both work
/// on the house's own models (work always does: ADR 0131): the colleague's turn and the asker's
/// result turn carry the mark.
#[test]
fn a_confidential_turn_asks_a_colleague_who_answers_on_the_houses_models() {
    use super::history_ports::HistoryStore;
    use serde_json::json;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let lead = hq.hire("Member 1", Role::Lead, 25, 2, 0).unwrap();
    let a = hq.hire("Member 2", Role::Developer, 40, 2, 0).unwrap();
    let b = hq.hire("Member 3", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Synthetic lab", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Fixture", "", lead, None).unwrap();
    hq.set_conversations_enabled(true).unwrap();
    hq.create_conversation(project, [a, b]).unwrap();
    hq.wake(a, None, WakeReason::Downtime).unwrap();
    hq.tick().unwrap();
    let turn = turn_of(&hq, a);
    let (run, member) = hq.turn_of(&turn.token).unwrap().unwrap();
    // It read the lab: Confidential.
    hq.store().mark_confidential_input(run.id).unwrap();
    let offered = super::protocol::tools_in(&hq, &run, &member);
    assert!(
        offered
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "consult"),
        "a Confidential turn is offered help: {offered}"
    );
    super::protocol::call(
        &hq,
        &run,
        &member,
        "consult",
        &json!({"member":"Member 3","question":"Why does gluetun stop?"}),
    )
    .unwrap();
    hq.tick().unwrap();
    let expert = turn_of(&hq, b);
    assert!(expert.told.contains("Why does gluetun stop?"));
    assert!(
        hq.store().run_read_confidential(expert.run).unwrap(),
        "the colleague's turn carries the mark"
    );
    hq.answer_assistance_in(expert.run, "Its health check restarts it.", &[])
        .unwrap();
    hq.runner().finish(turn.run, true);
    hq.runner().finish(expert.run, true);
    hq.tick().unwrap();
    let result = turn_of(&hq, a);
    assert!(result.told.contains("health check"), "{}", result.told);
    assert!(
        hq.store().run_read_confidential(result.run).unwrap(),
        "the asker's result turn carries the mark"
    );
    // Sending information stays closed to a Confidential turn.
    assert!(
        hq.send_message_in(result.run, 1, "fixture_send_1", "x", &[])
            .is_err()
    );
}
