//! The worker takes the person's goal before one Endora started for itself (ADR 0094).
use super::AppState;
use endora_application::{
    ChatMessage, ChatRepository, Clock, Goal, GoalOrigin, GoalRecord, GoalRepository, IdSource,
    MessageId, MessageRole, Timestamp,
};
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

fn started_by_itself() -> GoalRecord {
    GoalRecord::new(
        "looked-into-it",
        Goal::new("A device has stopped answering").unwrap(),
        at(1),
    )
    .unwrap()
    .with_origin(GoalOrigin::Record)
}

/// Somebody writes in chat, `minutes_ago` minutes before now.
fn wrote(state: &AppState, role: MessageRole, minutes_ago: i64) {
    let when = Timestamp::from_unix_millis(state.clock.now().unix_millis() - minutes_ago * 60_000);
    let message = ChatMessage::new(
        MessageId::new(state.ids.new_id()),
        role,
        "are you there?",
        when,
    )
    .unwrap();
    state.chat.append(&message).unwrap();
}

fn person_spoke(state: &AppState, minutes_ago: i64) {
    wrote(state, MessageRole::User, minutes_ago);
}

/// Whether the worker took a pass on the goal within `wait`.
async fn advanced_within(state: &AppState, goal: &GoalRecord, wait: Duration) -> bool {
    tokio::time::timeout(wait, async {
        loop {
            let saved = state.store.get_goal(goal.request_key()).unwrap().unwrap();
            if saved.updated_at() != goal.updated_at() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn the_worker_works_on_a_goal_it_started_when_nobody_has_spoken() {
    // The control for the next test: without it, "did nothing" could just mean "never runs".
    let state = state();
    let own = started_by_itself();
    state.store.create_goal(&own).unwrap();
    super::spawn_goal_worker(state.clone());
    assert!(
        advanced_within(&state, &own, Duration::from_secs(3)).await,
        "the worker never took a pass on a goal it started, though nobody was talking"
    );
}

#[tokio::test]
async fn the_worker_leaves_a_goal_it_started_alone_while_the_person_is_talking() {
    // S2. The worker's first tick comes at once, so if the gate were missing the goal
    // would already have been claimed by the time this looks.
    let state = state();
    let own = started_by_itself();
    state.store.create_goal(&own).unwrap();
    person_spoke(&state, 1);
    super::spawn_goal_worker(state.clone());
    assert!(
        !advanced_within(&state, &own, Duration::from_millis(1500)).await,
        "a goal nobody asked for took a pass one minute after the person spoke"
    );
}

#[tokio::test]
async fn the_worker_goes_back_to_a_goal_it_started_once_the_quiet_window_has_passed() {
    let state = state();
    let own = started_by_itself();
    state.store.create_goal(&own).unwrap();
    person_spoke(&state, 11);
    super::spawn_goal_worker(state.clone());
    assert!(
        advanced_within(&state, &own, Duration::from_secs(3)).await,
        "eleven quiet minutes were not enough to resume"
    );
}

#[tokio::test]
async fn the_persons_own_goal_does_not_wait_for_quiet() {
    // The gate is for work nobody asked for. Someone who has just written, and asked for
    // something to be done, is exactly who the worker should be serving.
    let state = state();
    let asked = GoalRecord::new("asked-for", Goal::new("Read a source").unwrap(), at(1)).unwrap();
    state.store.create_goal(&asked).unwrap();
    person_spoke(&state, 0);
    super::spawn_goal_worker(state.clone());
    assert!(
        advanced_within(&state, &asked, Duration::from_secs(3)).await,
        "the worker held back a request because the person had just spoken"
    );
}

#[tokio::test]
async fn endoras_own_messages_do_not_count_as_the_person_speaking() {
    // A brief or a check-in posted a minute ago is Endora talking. If that held the lane
    // back, anything it said would keep it idle for ten minutes after.
    let state = state();
    let own = started_by_itself();
    state.store.create_goal(&own).unwrap();
    wrote(&state, MessageRole::Butler, 1);
    super::spawn_goal_worker(state.clone());
    assert!(
        advanced_within(&state, &own, Duration::from_secs(3)).await,
        "Endora's own message was taken for the person speaking"
    );
}

#[test]
fn a_chat_that_cannot_be_read_is_not_quiet() {
    // Not being able to tell is a reason to wait, never a reason to start.
    let now = at(100 * 60_000);
    assert!(!super::workers::quiet_given(
        now,
        Err(endora_application::RepositoryError::Backend(
            "the chat is unavailable".to_owned()
        ))
    ));
    assert!(
        super::workers::quiet_given(now, Ok(Vec::new())),
        "and an empty chat is the opposite: nobody has spoken"
    );
}
