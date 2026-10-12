//! Desired foundation behavior, deliberately separate from the green regression suite.
//! Run with `cargo test -p endora-infrastructure --test foundation_gate -- --ignored`.
//! Failures are release blockers, not expected successes. No network or user data.

use endora_application::{
    ChatMessage, ExecutionLeaseRepository, Goal, GoalDelivery, GoalRecord, GoalRepository,
    GoalStatus, GoalStep, GoalVerdict, MemoryStore, MessageId, MessageRole, Timestamp,
};
use endora_infrastructure::SqliteStore;

struct InventedSchedule;

impl endora_application::Butler for InventedSchedule {
    fn respond(
        &self,
        _history: &[ChatMessage],
        _preferences: &[endora_application::Preference],
        _context: &endora_application::ButlerContext,
    ) -> Result<endora_application::ButlerReply, endora_application::ProposalError> {
        Ok(endora_application::ButlerReply {
            text:
                "1. Invented opponent\n2. Another invention\n3. No dates checked\nSource: invented"
                    .to_owned(),
            ..Default::default()
        })
    }
}

struct NoTools;

impl endora_application::CapabilityRunner for NoTools {
    fn available(&self) -> Vec<endora_application::CapabilitySpec> {
        Vec::new()
    }

    fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
        panic!("this counterexample has no tools")
    }
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn formatted_invention_without_a_source_cannot_complete_a_goal() {
    let store = SqliteStore::open_in_memory().unwrap();
    let understanding = endora_understanding::UnderstandingStore::new(store.db());
    let audit = endora_platform::AuditStore::new(store.db());
    let record = GoalRecord::new(
        "invented-schedule",
        Goal::new("What are the next three Panthers games?").unwrap(),
        at(1000),
    )
    .unwrap();
    let after = endora_application::usecases::resume_goal(
        &record,
        &InventedSchedule,
        &NoTools,
        &audit,
        &understanding,
        &understanding,
        &endora_infrastructure::RandomIdSource,
        &endora_infrastructure::SystemClock,
        &endora_application::ButlerContext::default(),
    );
    assert_ne!(
        after.goal().status(),
        GoalStatus::Complete,
        "numbered model prose and a Source label are not verified schedule evidence"
    );
}

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

fn queued(store: &SqliteStore) -> GoalRecord {
    let record = GoalRecord::new(
        "fixture-request",
        Goal::new("Find future games").unwrap(),
        at(1000),
    )
    .unwrap();
    assert!(store.create_goal(&record).unwrap());
    record
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn source_failure_does_not_end_an_objective_with_an_available_alternative() {
    let mut goal = Goal::new("Find future games").unwrap();
    let steps = [
        GoalStep::new("gathering", "primary").unwrap(),
        GoalStep::new("gathering", "alternative").unwrap(),
    ];
    let mut attempted = Vec::new();
    goal.run_plan(
        &steps,
        |step| {
            attempted.push(step.name.clone());
            if step.name == "primary" {
                return Err("source refused this read: HTTP 403".to_owned());
            }
            Ok("verified fixture evidence".to_owned())
        },
        |_, _| GoalVerdict::Complete,
    );
    assert_eq!(attempted, ["primary", "alternative"]);
    assert_eq!(goal.status(), GoalStatus::Complete);
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn purge_wins_against_a_late_checkpoint() {
    let store = SqliteStore::open_in_memory().unwrap();
    let stale = queued(&store);
    let lease = store
        .claim_goal(stale.request_key(), at(1001))
        .unwrap()
        .unwrap();
    store.purge().unwrap();
    assert!(
        !store
            .checkpoint_goal(
                &stale.with_goal(stale.goal().clone(), at(2000)),
                &lease,
                at(2000)
            )
            .unwrap()
    );
    assert!(store.get_goal(stale.request_key()).unwrap().is_none());
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn replaced_worker_cannot_overwrite_newer_evidence() {
    let store = SqliteStore::open_in_memory().unwrap();
    let stale = queued(&store);
    let old = store
        .claim_goal(stale.request_key(), at(2000))
        .unwrap()
        .unwrap();
    let current = store
        .claim_goal(stale.request_key(), at(33000))
        .unwrap()
        .unwrap();
    let mut newer = stale.goal().clone();
    newer.start("gathering", "alternative");
    newer.commit("alternative", "new worker evidence");
    assert!(
        store
            .checkpoint_goal(
                &stale.with_goal(newer.clone(), at(34000)),
                &current,
                at(34000)
            )
            .unwrap()
    );
    assert!(
        !store
            .checkpoint_goal(
                &stale.with_goal(stale.goal().clone(), at(35000)),
                &old,
                at(35000)
            )
            .unwrap()
    );
    assert_eq!(
        store.get_goal(stale.request_key()).unwrap().unwrap().goal(),
        &newer
    );
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn exhausted_attempts_leave_a_visible_terminal_state() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = queued(&store);
    for n in 0..8 {
        assert!(
            store
                .claim_goal(record.request_key(), at(2000 + n * 31000))
                .unwrap()
                .is_some()
        );
    }
    assert!(
        store
            .claim_goal(record.request_key(), at(300000))
            .unwrap()
            .is_none()
    );
    let saved = store.get_goal(record.request_key()).unwrap().unwrap();
    assert!(
        saved.goal().status().terminal(),
        "spent budget must not strand active work"
    );
    assert!(!saved.goal().reason().is_empty());

    // Terminal is not yet visible: the person has to be told, with the reason.
    let pending = store.pending_goal_completions().unwrap();
    let delivered = pending
        .iter()
        .find(|p| p.request_key() == record.request_key())
        .expect("an exhausted goal must be queued for delivery, not left silent");
    assert!(
        delivered
            .goal()
            .outcome_notice()
            .contains(saved.goal().reason()),
        "the notice must carry the reason the goal stopped"
    );
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn cancellation_wins_against_late_success() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = queued(&store);
    let lease = store
        .claim_goal(record.request_key(), at(1001))
        .unwrap()
        .unwrap();
    store
        .cancel_goal(record.request_key(), "user cancelled", at(2000))
        .unwrap();
    let mut goal = record.goal().clone();
    goal.apply(GoalVerdict::Complete, "late success");
    assert!(
        !store
            .checkpoint_goal(&record.with_goal(goal, at(3000)), &lease, at(3000))
            .unwrap()
    );
    assert_eq!(
        store
            .get_goal(record.request_key())
            .unwrap()
            .unwrap()
            .goal()
            .status(),
        GoalStatus::Cancelled
    );
    assert!(store.pending_goal_completions().unwrap().is_empty());
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn completion_notice_is_committed_once() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = queued(&store);
    let lease = store
        .claim_goal(record.request_key(), at(1001))
        .unwrap()
        .unwrap();
    let mut goal = record.goal().clone();
    goal.commit("fixture", "supported result");
    goal.apply(GoalVerdict::Complete, "verified");
    let record = record.with_goal(goal, at(2000));
    assert!(store.checkpoint_goal(&record, &lease, at(2000)).unwrap());
    for id in [1, 2] {
        let notice = ChatMessage::new(
            MessageId::new(id),
            MessageRole::Butler,
            "Result ready",
            at(3000),
        )
        .unwrap();
        assert_eq!(
            store.publish_goal_completion(&record, &notice).unwrap(),
            id == 1
        );
    }
    assert_eq!(store.export().unwrap().messages.len(), 1);
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn restart_preserves_evidence_and_existing_lease() {
    let path =
        std::env::temp_dir().join(format!("endora-foundation-{}.sqlite", uuid::Uuid::new_v4()));
    {
        let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
        let record = queued(&store);
        let lease = store
            .claim_goal(record.request_key(), at(1001))
            .unwrap()
            .unwrap();
        let mut goal = record.goal().clone();
        goal.commit("fixture", "saved evidence");
        store
            .checkpoint_goal(&record.with_goal(goal, at(2000)), &lease, at(2000))
            .unwrap();
        store.release_goal(&lease).unwrap();
        assert!(
            store
                .reserve_execution_attempt("goal:fixture-request", at(3000), 8)
                .unwrap()
        );
    }
    let reopened = SqliteStore::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        reopened
            .get_goal("fixture-request")
            .unwrap()
            .unwrap()
            .goal()
            .evidence()[0]
            .output,
        "saved evidence"
    );
    assert!(
        !reopened
            .reserve_execution_attempt("goal:fixture-request", at(4000), 8)
            .unwrap()
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
