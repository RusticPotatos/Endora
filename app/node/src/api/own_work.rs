//! Work Endora finds for itself, end to end through the node's workers (ADR 0094, Phase 3):
//! the starter that makes a goal from a standing trouble, the goal worker that re-reads the
//! source before spending a pass, and what the person sees when it ends. Deterministic: the
//! scripted butler, an in-memory store, no network.
use super::AppState;
use endora_application::{
    ChatMessage, ChatRepository, Clock, EventLog, Goal, GoalOrigin, GoalRecord, GoalRepository,
    GoalStatus, IdSource, MemoryStore, MessageId, MessageRole, Timestamp,
};
use endora_capabilities::{StandingTrouble, StandingTroubleRepository};
use endora_infrastructure::{RandomIdSource, ScriptedButler, SqliteStore, SystemClock};
use std::sync::Arc;
use std::time::Duration;

const DAY: i64 = 24 * 60 * 60 * 1000;

fn state() -> AppState {
    AppState::new(
        Arc::new(SqliteStore::open_in_memory().unwrap()),
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
        Arc::new(ScriptedButler),
    )
}

fn now(state: &AppState) -> i64 {
    state.clock.now().unix_millis()
}

/// A thing that has not answered for `days`, as the house watcher would have recorded it.
fn in_trouble(state: &AppState, thing: &str, days: i64) {
    state
        .config
        .note_trouble(&StandingTrouble {
            server: "home".to_owned(),
            thing: thing.to_owned(),
            trouble: "unavailable".to_owned(),
            since_ms: now(state) - days * DAY,
            accepted: false,
        })
        .unwrap();
}

fn own_goals(state: &AppState) -> Vec<GoalRecord> {
    state
        .store
        .own_goals_since(Timestamp::from_unix_millis(0))
        .unwrap()
}

fn trail(state: &AppState) -> Vec<String> {
    state
        .events
        .recent(50)
        .unwrap()
        .into_iter()
        .map(|event| event.summary)
        .collect()
}

fn wrote(state: &AppState, role: MessageRole, minutes_ago: i64) {
    let when = Timestamp::from_unix_millis(now(state) - minutes_ago * 60_000);
    let message = ChatMessage::new(
        MessageId::new(state.ids.new_id()),
        role,
        "are you there?",
        when,
    )
    .unwrap();
    state.chat.append(&message).unwrap();
}

// ---- the starter ----

#[test]
fn a_standing_trouble_becomes_a_goal_in_the_lane_that_never_outranks_a_request() {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    assert!(super::start_own_work_once(&state));

    let goals = own_goals(&state);
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0].origin(), GoalOrigin::Record);
    assert!(goals[0].goal().objective().contains("sensor.garage_door"));
    assert!(
        trail(&state)
            .iter()
            .any(|line| line.contains("sensor.garage_door") && line.contains("on my own")),
        "starting is said in the trail: {:?}",
        trail(&state)
    );
}

#[test]
fn with_nothing_wrong_the_node_idles_and_makes_nothing_up() {
    // S9. No source yields work, so nothing is created, not even to look busy.
    let state = state();
    assert!(!super::start_own_work_once(&state));
    assert!(own_goals(&state).is_empty());
    assert!(trail(&state).is_empty());
}

#[test]
fn a_trouble_too_young_to_be_worth_raising_is_left_alone() {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 1);
    assert!(!super::start_own_work_once(&state));
}

#[test]
fn it_does_not_start_a_second_while_one_is_open() {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    in_trouble(&state, "sensor.other", 5);
    assert!(super::start_own_work_once(&state));
    assert!(!super::start_own_work_once(&state));
    assert_eq!(own_goals(&state).len(), 1);
}

#[test]
fn it_does_not_start_while_the_persons_own_request_is_open() {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    let asked = GoalRecord::new(
        "asked-for",
        Goal::new("Read a source").unwrap(),
        Timestamp::from_unix_millis(now(&state)),
    )
    .unwrap();
    state.store.create_goal(&asked).unwrap();
    assert!(!super::start_own_work_once(&state));
}

#[test]
fn it_does_not_start_while_the_person_is_talking_but_does_once_they_are_quiet() {
    // S2 at the starting end.
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    wrote(&state, MessageRole::User, 2);
    assert!(!super::start_own_work_once(&state));
    assert!(own_goals(&state).is_empty());

    let quiet = state_with_quiet_person();
    assert!(super::start_own_work_once(&quiet));
}

fn state_with_quiet_person() -> AppState {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    wrote(&state, MessageRole::User, 30);
    state
}

#[test]
fn it_does_not_start_while_a_chat_turn_holds_the_turn_lock() {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    let _turn = state.turn_lock.clone().try_lock_owned().unwrap();
    assert!(!super::start_own_work_once(&state));
    assert!(own_goals(&state).is_empty());
}

#[test]
fn a_trouble_the_person_called_fine_is_not_started() {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    state
        .config
        .accept_trouble("home", "sensor.garage_door")
        .unwrap();
    assert!(!super::start_own_work_once(&state));
}

#[test]
fn after_one_ends_the_next_waits_out_the_cooldown() {
    // The lane being empty is not enough: thirty minutes have to pass as well, so a row of
    // troubles is looked into one at a time, spread out, not back to back.
    let state = state();
    in_trouble(&state, "sensor.garage_door", 5);
    in_trouble(&state, "sensor.other", 4);
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    state
        .store
        .cancel_goal(&key, "stopped from the console", state.clock.now())
        .unwrap();
    // The first device is out of the picture, so the only thing left to start is the second.
    state
        .config
        .clear_trouble("home", "sensor.garage_door")
        .unwrap();

    assert!(
        !super::start_own_work_once(&state),
        "the next was started the moment the last one ended"
    );
    assert_eq!(own_goals(&state).len(), 1);
}

#[test]
fn a_goal_the_person_stopped_is_not_started_again() {
    // S8, through the real store: Stop cancels it, and the next look does not recreate it.
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    state
        .store
        .cancel_goal(&key, "stopped from the console", state.clock.now())
        .unwrap();

    assert!(!super::start_own_work_once(&state));
    assert_eq!(own_goals(&state).len(), 1, "nothing new was made");
}

// ---- the goal worker, with a goal of this kind ----

async fn settled(state: &AppState, key: &str, wait: Duration) -> Option<GoalRecord> {
    tokio::time::timeout(wait, async {
        loop {
            let saved = state.store.get_goal(key).unwrap().unwrap();
            if saved.goal().status().terminal() {
                return saved;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .ok()
}

#[tokio::test]
async fn when_the_trouble_clears_the_worker_finishes_the_goal_without_a_model_and_without_a_chat_message()
 {
    // S4 and S7 together. The source says it is over; no pass is spent, nothing is posted to
    // chat, and the trail says what happened.
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    state
        .config
        .clear_trouble("home", "sensor.garage_door")
        .unwrap();

    super::spawn_goal_worker(state.clone());
    let done = settled(&state, &key, Duration::from_secs(3))
        .await
        .expect("the worker never finished a goal whose trouble had cleared");
    assert_eq!(done.goal().status(), GoalStatus::Complete);
    assert_eq!(done.origin(), GoalOrigin::Record);

    // Published quietly: the one-time notice is spent without a chat message.
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if state.store.pending_goal_completions().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the ending was never settled");
    assert!(
        state.store.export().unwrap().messages.is_empty(),
        "self-started work posted to chat: {:?}",
        state.store.export().unwrap().messages
    );
    assert!(
        trail(&state)
            .iter()
            .any(|line| line.starts_with("On my own I looked into") && line.contains("complete")),
        "the ending is in the trail: {:?}",
        trail(&state)
    );
}

#[tokio::test]
async fn when_the_person_calls_the_trouble_fine_the_worker_stops_the_goal() {
    let state = state();
    in_trouble(&state, "sensor.garage_door", 4);
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    state
        .config
        .accept_trouble("home", "sensor.garage_door")
        .unwrap();

    super::spawn_goal_worker(state.clone());
    let stopped = settled(&state, &key, Duration::from_secs(3))
        .await
        .expect("the worker never stopped work on a trouble the person called fine");
    assert_eq!(stopped.goal().status(), GoalStatus::Cancelled);
    assert!(
        trail(&state)
            .iter()
            .any(|line| line.contains("fine") && line.contains("sensor.garage_door")),
        "{:?}",
        trail(&state)
    );
    assert!(state.store.export().unwrap().messages.is_empty());
}

#[tokio::test]
async fn a_person_goal_that_ends_still_posts_to_chat() {
    // The other half of S7: only this lane is quiet. A request the person made is announced
    // exactly as before (ADR 0082).
    let state = state();
    let mut goal = Goal::new("Compare the supplied fictional products").unwrap();
    for n in 0..4 {
        goal.commit("read", &format!("distinct source finding {n}"));
    }
    let asked = GoalRecord::new("asked-for", goal, Timestamp::from_unix_millis(1)).unwrap();
    state.store.create_goal(&asked).unwrap();
    super::spawn_goal_worker(state.clone());

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if !state.store.export().unwrap().messages.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("a request's ending must still reach chat");
}

// ---- specimens and beliefs (ADR 0095) ----

use endora_application::{BeliefKind, Confidence};
use endora_understanding::SpecimenRepository;

/// An ask that failed `days_ago` days ago and has not been replayed.
fn failed_ask(state: &AppState, id: &str, asked: &str, days_ago: i64) {
    state
        .understanding
        .file_specimen(id, asked, "not an answer", now(state) - days_ago * DAY)
        .unwrap();
}

fn strongly_believes(state: &AppState, statement: &str) {
    endora_application::BeliefRepository::save(
        state.understanding.as_ref(),
        &endora_application::Belief::new(
            endora_application::BeliefId::new(7),
            statement,
            BeliefKind::Intent,
            Confidence::High,
            "said so twice",
            Timestamp::from_unix_millis(now(state)),
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn an_ask_that_failed_two_days_ago_is_asked_again_when_nothing_is_wrong_in_the_house() {
    let state = state();
    failed_ask(&state, "s1", "when is the next 41 bus?", 2);
    assert!(super::start_own_work_once(&state));
    let goals = own_goals(&state);
    assert_eq!(goals.len(), 1);
    assert!(goals[0].request_key().starts_with("own:specimen:s1"));
    assert_eq!(goals[0].origin(), GoalOrigin::Record);
}

#[test]
fn an_ask_that_failed_an_hour_ago_is_left_alone() {
    let state = state();
    state
        .understanding
        .file_specimen("s1", "q", "not an answer", now(&state) - 60 * 60 * 1000)
        .unwrap();
    assert!(!super::start_own_work_once(&state));
}

#[test]
fn a_belief_is_looked_into_only_when_nothing_else_needs_it() {
    let state = state();
    strongly_believes(&state, "wants to sleep better");
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    assert!(key.starts_with("own:belief:7:"), "{key}");
    assert!(
        own_goals(&state)[0]
            .goal()
            .objective()
            .contains("sleep better")
    );

    // With a trouble present it would have come first: a fresh node, same belief.
    let busy = self::state();
    strongly_believes(&busy, "wants to sleep better");
    in_trouble(&busy, "sensor.garage_door", 4);
    assert!(super::start_own_work_once(&busy));
    assert!(
        own_goals(&busy)[0].request_key().starts_with("own:trouble"),
        "{}",
        own_goals(&busy)[0].request_key()
    );
}

#[tokio::test]
async fn a_replay_the_worker_runs_ends_the_goal_and_leaves_the_result_in_the_trail_only() {
    // S4. The lane asks the question again, judges it by the verdict that filed it, and the
    // ending reaches the activity trail in the words the night used, never chat.
    let state = state();
    failed_ask(&state, "s1", "what lights are on?", 2);
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();

    super::spawn_goal_worker(state.clone());
    let done = settled(&state, &key, Duration::from_secs(5))
        .await
        .expect("the worker never replayed the ask");
    assert!(
        matches!(
            done.goal().status(),
            GoalStatus::Complete | GoalStatus::Partial
        ),
        "{:?}",
        done.goal().status()
    );
    // The specimen recorded the attempt: replayed once, whichever way it went.
    let after = state
        .understanding
        .open_specimens()
        .unwrap()
        .into_iter()
        .chain(state.understanding.gave_up_specimens().unwrap())
        .find(|s| s.id == "s1");
    if let Some(open) = after {
        assert_eq!(open.replays, 1, "{open:?}");
    }

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if state.store.pending_goal_completions().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the ending was never settled");
    assert!(state.store.export().unwrap().messages.is_empty());
    assert!(
        trail(&state)
            .iter()
            .any(|line| line.contains("what lights are on")),
        "{:?}",
        trail(&state)
    );
}

#[tokio::test]
async fn an_ask_that_is_taken_off_the_shelf_withdraws_its_goal() {
    let state = state();
    failed_ask(&state, "s1", "what lights are on?", 2);
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    // Answered some other way before the worker got to it.
    state
        .understanding
        .record_replay("s1", true, now(&state))
        .unwrap();

    super::spawn_goal_worker(state.clone());
    let gone = settled(&state, &key, Duration::from_secs(3))
        .await
        .expect("the worker never withdrew the goal");
    assert_eq!(gone.goal().status(), GoalStatus::Cancelled);
}

#[tokio::test]
async fn a_belief_goal_whose_belief_has_gone_is_withdrawn() {
    let state = state();
    strongly_believes(&state, "wants to sleep better");
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    let mut belief = endora_application::BeliefRepository::get(
        state.understanding.as_ref(),
        endora_application::BeliefId::new(7),
    )
    .unwrap()
    .unwrap();
    belief.expire();
    endora_application::BeliefRepository::save(state.understanding.as_ref(), &belief).unwrap();

    super::spawn_goal_worker(state.clone());
    let gone = settled(&state, &key, Duration::from_secs(3))
        .await
        .expect("the worker never withdrew the goal");
    assert_eq!(gone.goal().status(), GoalStatus::Cancelled);
}
