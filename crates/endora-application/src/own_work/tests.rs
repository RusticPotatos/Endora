use super::*;
use crate::GoalVerdict;

const DAY: i64 = 24 * 60 * 60 * 1000;

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

/// Four days in, long after anyone spoke.
const NOW: i64 = 4 * DAY;

fn situation(history: &[GoalRecord]) -> Situation<'_> {
    Situation {
        specimens: &[],
        focus: None,
        history,
        a_request_is_open: false,
        person_is_quiet: true,
        now: at(NOW),
    }
}

/// The goal `item` becomes, having reached `status` at `ended`.
fn ended(item: WorkItem, status: GoalStatus, ended: i64) -> GoalRecord {
    let record = item.into_record(at(1)).unwrap();
    let mut goal = record.goal().clone();
    match status {
        GoalStatus::Cancelled => goal.cancel("stopped by the person"),
        GoalStatus::Complete => {
            goal.start("x", "y");
            goal.commit("x", "something");
            goal.apply(GoalVerdict::Complete, "done");
        }
        _ => {
            goal.start("x", "y");
            goal.apply(GoalVerdict::Partial, "ran out");
        }
    }
    assert_eq!(goal.status(), status);
    record.with_goal(goal, at(ended))
}

fn open(item: WorkItem) -> GoalRecord {
    item.into_record(at(1)).unwrap()
}

use endora_understanding::{Specimen, SpecimenState};

fn specimen(id: &str, asked: &str, filed_ms: i64, replays: u32, last: Option<i64>) -> Specimen {
    Specimen {
        id: id.to_owned(),
        asked: asked.to_owned(),
        verdict: "not an answer".to_owned(),
        filed_ms,
        replays,
        last_replay_ms: last,
        state: SpecimenState::Open,
    }
}

fn with_specimens<'a>(specimens: &'a [Specimen], history: &'a [GoalRecord]) -> Situation<'a> {
    Situation {
        specimens,
        ..situation(history)
    }
}

#[test]
fn an_ask_that_failed_a_day_ago_is_asked_again() {
    let shelf = [specimen(
        "s1",
        "when is the next 41 bus?",
        NOW - DAY,
        0,
        None,
    )];
    let item = next_work(&with_specimens(&shelf, &[])).expect("a day is long enough");
    assert_eq!(item.request_key, "own:specimen:s1:1");
    assert_eq!(
        item.source,
        WorkSource::Specimen {
            id: "s1".to_owned()
        }
    );
    assert!(item.objective.contains("41 bus"), "{}", item.objective);
}

#[test]
fn an_ask_that_only_just_failed_is_left_until_something_could_have_changed() {
    let shelf = [specimen(
        "s1",
        "what lights are on?",
        NOW - DAY + 1,
        0,
        None,
    )];
    assert_eq!(next_work(&with_specimens(&shelf, &[])), None);
}

#[test]
fn the_wait_is_from_the_last_replay_when_there_has_been_one() {
    let recent = [specimen("s1", "q", 0, 3, Some(NOW - DAY + 1))];
    assert_eq!(next_work(&with_specimens(&recent, &[])), None);
    let due = [specimen("s1", "q", 0, 3, Some(NOW - DAY))];
    let item = next_work(&with_specimens(&due, &[])).unwrap();
    assert_eq!(
        item.request_key, "own:specimen:s1:4",
        "the replays so far are counted"
    );
}

#[test]
fn the_oldest_ask_goes_first() {
    let shelf = [
        specimen("newer", "q2", 2 * DAY, 0, None),
        specimen("older", "q1", DAY, 0, None),
    ];
    // Oldest by when it was last touched, which for a never-replayed ask is when it failed.
    let item = next_work(&with_specimens(&shelf, &[])).unwrap();
    assert_eq!(
        item.source,
        WorkSource::Specimen {
            id: "older".to_owned()
        }
    );
}

#[test]
fn an_ask_the_person_stopped_is_not_asked_again() {
    let shelf = [specimen("s1", "q", 0, 0, None)];
    let item = next_work(&with_specimens(&shelf, &[])).unwrap();
    let stopped = ended(item, GoalStatus::Cancelled, NOW - 5 * DAY);
    assert_eq!(next_work(&with_specimens(&shelf, &[stopped])), None);
}

#[test]
fn the_start_gate_holds_for_asks_as_well() {
    let shelf = [specimen("s1", "q", 0, 0, None)];
    let mut s = with_specimens(&shelf, &[]);
    s.a_request_is_open = true;
    assert_eq!(next_work(&s), None);
    let mut s = with_specimens(&shelf, &[]);
    s.person_is_quiet = false;
    assert_eq!(next_work(&s), None);
}

// ---- the lane's own rules, on a failed ask ----

#[test]
fn work_becomes_a_goal_in_the_lane_that_never_outranks_a_request() {
    let shelf = [specimen("s1", "q", 0, 0, None)];
    let record = next_work(&with_specimens(&shelf, &[]))
        .unwrap()
        .into_record(at(NOW))
        .unwrap();
    assert_eq!(record.origin(), GoalOrigin::Record);
    assert!(matches!(
        record.goal().criteria(),
        Some(GoalCriteria::Resolve { .. })
    ));
    assert!(!record.goal().status().terminal());
}

#[test]
fn nothing_starts_while_its_own_lane_is_occupied() {
    let shelf = [
        specimen("s1", "q1", 0, 0, None),
        specimen("s2", "q2", 0, 0, None),
    ];
    let running = open(next_work(&with_specimens(&shelf[..1], &[])).unwrap());
    assert_eq!(next_work(&with_specimens(&shelf, &[running])), None);
}

#[test]
fn nothing_starts_until_the_cooldown_after_the_last_ending_has_passed() {
    let shelf = [
        specimen("s1", "q1", 0, 0, None),
        specimen("s2", "q2", DAY, 0, None),
    ];
    let item = next_work(&with_specimens(&shelf[..1], &[])).unwrap();
    let just_ended = ended(
        item.clone(),
        GoalStatus::Partial,
        NOW - COOLDOWN_AFTER_ENDING_MS + 1,
    );
    let next = next_work(&with_specimens(&shelf[1..], &[just_ended]));
    assert_eq!(next, None, "one millisecond short of the cooldown");
    let long_ago = ended(item, GoalStatus::Partial, NOW - COOLDOWN_AFTER_ENDING_MS);
    let next = next_work(&with_specimens(&shelf[1..], &[long_ago]))
        .expect("exactly the cooldown has passed");
    assert_eq!(next.request_key, "own:specimen:s2:1");
}

#[test]
fn work_on_a_device_that_stopped_answering_stored_before_is_withdrawn_not_pursued() {
    // Endora no longer looks into those (2026-10-16). A goal it started about one before
    // still reads, and the next check stops it.
    let stored = serde_json::json!({
        "kind": "resolve", "source": "trouble",
        "server": "home", "thing": "light.example", "since_ms": 1
    });
    let criteria = GoalCriteria::from_stored(&stored).expect("an old goal still reads");
    assert_eq!(
        criteria,
        GoalCriteria::Resolve {
            source: WorkSource::Retired
        }
    );
    let goal = Goal::for_request("find out why light.example is not answering", criteria).unwrap();
    let record = GoalRecord::new("own:trouble:home:light.example:1:1", goal, at(1))
        .unwrap()
        .with_origin(GoalOrigin::Record);
    let records = Records {
        open_specimens: &[],
        active_beliefs: &[],
    };
    assert!(matches!(recheck(&record, &records), Recheck::Withdrawn(_)));
}

// ---- beliefs (ADR 0095) ----

fn focus() -> Focus {
    Focus {
        id: "b-1".to_owned(),
        statement: "they want to run more often".to_owned(),
    }
}

fn with_focus<'a>(focus: &'a Focus, history: &'a [GoalRecord]) -> Situation<'a> {
    Situation {
        focus: Some(focus),
        ..situation(history)
    }
}

#[test]
fn with_nothing_else_to_do_it_looks_into_what_it_understands_best() {
    let f = focus();
    let item = next_work(&with_focus(&f, &[])).unwrap();
    assert_eq!(item.request_key, "own:belief:b-1:1");
    assert_eq!(
        item.source,
        WorkSource::Belief {
            id: "b-1".to_owned()
        }
    );
    assert!(
        item.objective.contains("run more often"),
        "{}",
        item.objective
    );
}

#[test]
fn a_belief_is_the_last_thing_looked_into() {
    let f = focus();
    let shelf = [specimen("s1", "q", 0, 0, None)];
    let both = Situation {
        specimens: &shelf,
        focus: Some(&f),
        ..situation(&[])
    };
    assert!(
        next_work(&both)
            .unwrap()
            .request_key
            .starts_with("own:specimen")
    );
    let only_the_belief = Situation {
        specimens: &[],
        ..both
    };
    assert!(
        next_work(&only_the_belief)
            .unwrap()
            .request_key
            .starts_with("own:belief")
    );
}

#[test]
fn a_belief_is_looked_into_at_most_twice_a_week_apart_and_never_after_a_stop() {
    let f = focus();
    let first = next_work(&with_focus(&f, &[])).unwrap();
    let one = ended(first.clone(), GoalStatus::Partial, NOW - DAY);
    assert_eq!(
        next_work(&with_focus(&f, &[one])),
        None,
        "inside the cooldown"
    );

    let one = ended(first.clone(), GoalStatus::Partial, NOW - ITEM_COOLDOWN_MS);
    let second = next_work(&with_focus(&f, std::slice::from_ref(&one))).unwrap();
    assert!(second.request_key.ends_with(":2"), "{}", second.request_key);
    let two = ended(second, GoalStatus::Partial, NOW - ITEM_COOLDOWN_MS);
    assert_eq!(
        next_work(&with_focus(&f, &[one, two])),
        None,
        "two attempts is all"
    );

    let stopped = ended(first, GoalStatus::Cancelled, NOW - 30 * DAY);
    assert_eq!(next_work(&with_focus(&f, &[stopped])), None);
}

#[test]
fn nothing_to_look_into_still_means_nothing() {
    assert_eq!(next_work(&situation(&[])), None);
}

// ---- re-reading the source, for the others ----

fn started_from(item: WorkItem) -> GoalRecord {
    item.into_record(at(1)).unwrap()
}

#[test]
fn an_ask_that_is_no_longer_on_the_shelf_withdraws_its_goal() {
    let shelf = [specimen("s1", "q", 0, 0, None)];
    let record = started_from(next_work(&with_specimens(&shelf, &[])).unwrap());
    let still = Records {
        open_specimens: &shelf,
        active_beliefs: &[],
    };
    assert_eq!(recheck(&record, &still), Recheck::Unchanged);
    let gone = Records {
        open_specimens: &[],
        ..still
    };
    assert!(matches!(recheck(&record, &gone), Recheck::Withdrawn(_)));
}

#[test]
fn a_belief_nothing_holds_any_more_withdraws_its_goal_and_never_completes_it() {
    let f = focus();
    let record = started_from(next_work(&with_focus(&f, &[])).unwrap());
    let held = vec!["b-1".to_owned()];
    let still = Records {
        open_specimens: &[],
        active_beliefs: &held,
    };
    assert_eq!(
        recheck(&record, &still),
        Recheck::Unchanged,
        "its source cannot say it is done"
    );
    let gone = Records {
        active_beliefs: &[],
        ..still
    };
    assert!(matches!(recheck(&record, &gone), Recheck::Withdrawn(_)));
}

#[test]
fn the_trail_says_a_replay_in_the_words_the_night_used() {
    let shelf = [specimen("s1", "q", 0, 0, None)];
    let record = started_from(next_work(&with_specimens(&shelf, &[])).unwrap());
    let mut goal = record.goal().clone();
    goal.start("replay", "asked again");
    goal.commit(
        "replay",
        "A question that once stumped me answers now: \"q\"",
    );
    goal.apply(GoalVerdict::Complete, "answered");
    let line = trail_line(&record.with_goal(goal, at(NOW)));
    assert_eq!(line, "A question that once stumped me answers now: \"q\"");
}
