//! The worker takes the person's goal before one Endora started for itself (ADR 0094).
use super::AppState;
use endora_application::{Goal, GoalOrigin, GoalRecord, GoalRepository, Timestamp};
use endora_infrastructure::{RandomIdSource, ScriptedButler, SqliteStore, SystemClock};
use std::sync::Arc;
use std::time::Duration;

fn state() -> AppState {
    AppState::new(
        Arc::new(SqliteStore::open_in_memory().unwrap()),
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
        Arc::new(ScriptedButler),
    )
}

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

#[tokio::test]
async fn the_worker_advances_the_persons_goal_before_an_older_one_it_started_itself() {
    // The case the lanes exist for. Endora's own goal is the older one, so the worker's old
    // rule — oldest-updated first, no notion of whose — would have taken it, and the request
    // somebody had just made would have waited behind work nobody asked for.
    let state = state();
    let own = GoalRecord::new(
        "looked-into-it",
        Goal::new("A device has stopped answering").unwrap(),
        at(1),
    )
    .unwrap()
    .with_origin(GoalOrigin::Record);
    let asked = GoalRecord::new("asked-for", Goal::new("Read a source").unwrap(), at(50)).unwrap();
    state.store.create_goal(&own).unwrap();
    state.store.create_goal(&asked).unwrap();

    super::spawn_goal_worker(state.clone());

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let saved = state.store.get_goal("asked-for").unwrap().unwrap();
            if saved.updated_at() != asked.updated_at() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the worker never advanced the person's goal");

    // The worker takes one goal per tick, five seconds apart, so by now it has not reached
    // the one it started itself.
    let untouched = state.store.get_goal("looked-into-it").unwrap().unwrap();
    assert_eq!(
        untouched.updated_at(),
        own.updated_at(),
        "the worker advanced its own goal ahead of a request"
    );
    assert_eq!(untouched.origin(), GoalOrigin::Record);
}

#[test]
fn the_api_says_which_lane_a_goal_is_in() {
    let asked = GoalRecord::new("asked-for", Goal::new("Read a source").unwrap(), at(1)).unwrap();
    assert_eq!(super::life::goal_json(&asked)["origin"], "person");
    let own = GoalRecord::new("looked-into-it", Goal::new("A device").unwrap(), at(1))
        .unwrap()
        .with_origin(GoalOrigin::Record);
    assert_eq!(super::life::goal_json(&own)["origin"], "record");
}
