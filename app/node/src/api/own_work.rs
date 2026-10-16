//! Work Endora finds for itself, end to end through the node's workers (ADR 0094, Phase 3):
//! the starter that makes a goal from a stored record, the goal worker that re-reads the
//! source before spending a pass, and what the person sees when it ends. Deterministic: the
//! scripted butler, an in-memory store, no network.
use super::AppState;
use endora_application::{
    ChatMessage, ChatRepository, Clock, EventLog, Goal, GoalOrigin, GoalRecord, GoalRepository,
    GoalStatus, IdSource, MemoryStore, MessageId, MessageRole, Timestamp,
};
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
fn a_failed_ask_becomes_a_goal_in_the_lane_that_never_outranks_a_request() {
    let state = state();
    failed_ask(&state, "s1", "when is the next 41 bus?", 2);
    assert!(super::start_own_work_once(&state));

    let goals = own_goals(&state);
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0].origin(), GoalOrigin::Record);
    assert!(goals[0].goal().objective().contains("41 bus"));
    assert!(
        trail(&state)
            .iter()
            .any(|line| line.contains("41 bus") && line.contains("on my own")),
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
fn it_does_not_start_a_second_while_one_is_open() {
    let state = state();
    failed_ask(&state, "s1", "q1", 2);
    failed_ask(&state, "s2", "q2", 3);
    assert!(super::start_own_work_once(&state));
    assert!(!super::start_own_work_once(&state));
    assert_eq!(own_goals(&state).len(), 1);
}

#[test]
fn it_does_not_start_while_the_persons_own_request_is_open() {
    let state = state();
    failed_ask(&state, "s1", "q", 2);
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
    failed_ask(&state, "s1", "q", 2);
    wrote(&state, MessageRole::User, 2);
    assert!(!super::start_own_work_once(&state));
    assert!(own_goals(&state).is_empty());

    let quiet = state_with_quiet_person();
    assert!(super::start_own_work_once(&quiet));
}

fn state_with_quiet_person() -> AppState {
    let state = state();
    failed_ask(&state, "s1", "q", 2);
    wrote(&state, MessageRole::User, 30);
    state
}

#[test]
fn it_does_not_start_while_a_chat_turn_holds_the_turn_lock() {
    let state = state();
    failed_ask(&state, "s1", "q", 2);
    let _turn = state.turn_lock.clone().try_lock_owned().unwrap();
    assert!(!super::start_own_work_once(&state));
    assert!(own_goals(&state).is_empty());
}

#[test]
fn after_one_ends_the_next_waits_out_the_cooldown() {
    // The lane being empty is not enough: thirty minutes have to pass as well, so a row of
    // asks is looked into one at a time, spread out, not back to back.
    let state = state();
    failed_ask(&state, "s1", "q1", 3);
    failed_ask(&state, "s2", "q2", 2);
    assert!(super::start_own_work_once(&state));
    let key = own_goals(&state)[0].request_key().to_owned();
    // The first is stopped, so the only thing left to start is the second.
    state
        .store
        .cancel_goal(&key, "stopped from the console", state.clock.now())
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
    failed_ask(&state, "s1", "q", 2);
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
async fn work_on_a_device_stored_before_it_was_turned_off_is_withdrawn_quietly() {
    // Endora no longer looks into devices that stopped answering (2026-10-16). A goal it
    // started about one before is stopped by the worker: no pass, no chat message, a line in
    // the trail.
    let state = state();
    let goal = Goal::for_request(
        "Find out why sensor.garage_door has not answered for 4 days",
        endora_application::goal_request::GoalCriteria::Resolve {
            source: endora_application::goal_request::WorkSource::Retired,
        },
    )
    .unwrap();
    let key = "own:trouble:home:sensor.garage_door:1:1";
    let record = GoalRecord::new(key, goal, Timestamp::from_unix_millis(now(&state)))
        .unwrap()
        .with_origin(GoalOrigin::Record);
    state.store.create_goal(&record).unwrap();

    super::spawn_goal_worker(state.clone());
    let stopped = settled(&state, key, Duration::from_secs(3))
        .await
        .expect("the worker never stopped retired work");
    assert_eq!(stopped.goal().status(), GoalStatus::Cancelled);
    assert!(
        trail(&state)
            .iter()
            .any(|line| line.starts_with("Stopped looking into") && line.contains("no longer")),
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
fn an_ask_that_failed_two_days_ago_is_asked_again() {
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

    // With a failed ask present it would have come first: a fresh node, same belief.
    let busy = self::state();
    strongly_believes(&busy, "wants to sleep better");
    failed_ask(&busy, "s1", "q", 2);
    assert!(super::start_own_work_once(&busy));
    assert!(
        own_goals(&busy)[0]
            .request_key()
            .starts_with("own:specimen"),
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

// ---- the house's rhythms (ADR 0096) ----

use endora_capabilities::{Transition, TransitionLog};

/// A fortnight in which the front door opens at 07:30 UTC every day, with a heartbeat each day
/// so the log covers all of them.
fn a_house_with_a_routine(state: &AppState) {
    let start = now(state) - 14 * DAY;
    for day in 0..14 {
        for (key, to, minutes) in [
            ("home::sensor.heartbeat", "ok", 3 * 60),
            ("home::binary_sensor.front_door", "on", 7 * 60 + 30),
        ] {
            state
                .config
                .record(&Transition {
                    key: key.to_owned(),
                    from: "other".to_owned(),
                    to: to.to_owned(),
                    at_ms: start - start.rem_euclid(DAY) + day * DAY + minutes * 60_000 + DAY,
                })
                .unwrap();
        }
    }
}

fn rhythm_lines(state: &AppState) -> Vec<String> {
    trail(state)
        .into_iter()
        .filter(|line| line.starts_with("Regular patterns in the house"))
        .collect()
}

#[test]
fn a_routine_in_the_log_is_noticed_and_said_once() {
    let state = state();
    a_house_with_a_routine(&state);
    assert!(super::notice_rhythms_once(&state));
    let said = rhythm_lines(&state);
    assert_eq!(said.len(), 1, "{:?}", trail(&state));
    assert!(
        said[0].contains("binary_sensor.front_door goes on around 07:30"),
        "{}",
        said[0]
    );
    assert!(
        !super::notice_rhythms_once(&state),
        "the same thing, said again"
    );
    assert_eq!(rhythm_lines(&state).len(), 1);
}

#[test]
fn a_quiet_log_says_nothing() {
    let state = state();
    assert!(!super::notice_rhythms_once(&state));
    assert!(rhythm_lines(&state).is_empty());
}

#[test]
fn a_changed_picture_waits_hours_before_it_is_said_again() {
    let state = state();
    a_house_with_a_routine(&state);
    // An earlier, different line from a few hours ago, and then one from over six hours ago.
    endora_application::EventLog::record(
        state.events.as_ref(),
        Timestamp::from_unix_millis(now(&state) - 60 * 60 * 1000),
        "Regular patterns in the house: something else entirely",
    )
    .unwrap();
    assert!(
        !super::notice_rhythms_once(&state),
        "said an hour ago; the picture changing is not a reason to say it again so soon"
    );
}

#[test]
fn a_changed_picture_is_said_once_enough_time_has_passed() {
    let state = state();
    a_house_with_a_routine(&state);
    endora_application::EventLog::record(
        state.events.as_ref(),
        Timestamp::from_unix_millis(now(&state) - 7 * 60 * 60 * 1000),
        "Regular patterns in the house: something else entirely",
    )
    .unwrap();
    assert!(super::notice_rhythms_once(&state));
}

#[test]
fn the_persons_own_hours_are_used() {
    // 07:30 UTC is 02:30 five hours west of it.
    let state = state().with_time_zone(endora_infrastructure::TimeZone::fixed(-300));
    a_house_with_a_routine(&state);
    assert!(super::notice_rhythms_once(&state));
    assert!(
        rhythm_lines(&state)[0].contains("goes on around 02:30"),
        "{:?}",
        rhythm_lines(&state)
    );
}
