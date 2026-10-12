//! Desired foundation behavior, deliberately separate from the green regression suite.
//! Run with `cargo test -p endora-infrastructure --test foundation_gate -- --ignored`.
//! Failures are release blockers, not expected successes. No network or user data.

use endora_application::{
    ChatMessage, ExecutionLeaseRepository, Goal, GoalDelivery, GoalRecord, GoalRepository,
    GoalStatus, GoalStep, GoalVerdict, MemoryStore, MessageId, MessageRole, Timestamp,
};
use endora_infrastructure::SqliteStore;

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
    assert!(store.save_goal(&record).unwrap());
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
    store.purge().unwrap();
    let _ = store.save_goal(&stale.with_goal(stale.goal().clone(), at(2000)));
    assert!(store.get_goal(stale.request_key()).unwrap().is_none());
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn replaced_worker_cannot_overwrite_newer_evidence() {
    let store = SqliteStore::open_in_memory().unwrap();
    let stale = queued(&store);
    assert!(
        store
            .reserve_goal_attempt(stale.request_key(), at(2000))
            .unwrap()
    );
    assert!(
        store
            .reserve_goal_attempt(stale.request_key(), at(33000))
            .unwrap()
    );
    let mut newer = stale.goal().clone();
    newer.start("gathering", "alternative");
    newer.commit("alternative", "new worker evidence");
    assert!(
        store
            .save_goal(&stale.with_goal(newer.clone(), at(34000)))
            .unwrap()
    );
    let _ = store.save_goal(&stale.with_goal(stale.goal().clone(), at(35000)));
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
                .reserve_goal_attempt(record.request_key(), at(2000 + n * 31000))
                .unwrap()
        );
    }
    assert!(
        !store
            .reserve_goal_attempt(record.request_key(), at(300000))
            .unwrap()
    );
    let saved = store.get_goal(record.request_key()).unwrap().unwrap();
    assert!(
        saved.goal().status().terminal(),
        "spent budget must not strand active work"
    );
    assert!(!saved.goal().reason().is_empty());
}

#[test]
#[ignore = "foundation acceptance gate; failure blocks runtime adoption"]
fn cancellation_wins_against_late_success() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = queued(&store);
    store
        .cancel_goal(record.request_key(), "user cancelled", at(2000))
        .unwrap();
    let mut goal = record.goal().clone();
    goal.apply(GoalVerdict::Complete, "late success");
    assert!(!store.save_goal(&record.with_goal(goal, at(3000))).unwrap());
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
    let mut goal = record.goal().clone();
    goal.commit("fixture", "supported result");
    goal.apply(GoalVerdict::Complete, "verified");
    let record = record.with_goal(goal, at(2000));
    assert!(store.save_goal(&record).unwrap());
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
        let mut goal = record.goal().clone();
        goal.commit("fixture", "saved evidence");
        store.save_goal(&record.with_goal(goal, at(2000))).unwrap();
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
