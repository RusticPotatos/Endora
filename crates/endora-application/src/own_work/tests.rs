use super::*;

const DAY: i64 = 24 * 60 * 60 * 1000;
const MINUTE: i64 = 60 * 1000;

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

fn trouble(thing: &str, since_ms: i64) -> StandingTrouble {
    StandingTrouble {
        server: "home".to_owned(),
        thing: thing.to_owned(),
        trouble: "unavailable".to_owned(),
        since_ms,
        accepted: false,
    }
}

/// Four days into a trouble that began at time zero, long after anyone spoke.
const NOW: i64 = 4 * DAY;

fn situation<'a>(troubles: &'a [StandingTrouble], history: &'a [GoalRecord]) -> Situation<'a> {
    Situation {
        troubles,
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

fn first(troubles: &[StandingTrouble], history: &[GoalRecord]) -> Option<WorkItem> {
    next_work(&situation(troubles, history))
}

// ---- S9: nothing to do means nothing is made ----

#[test]
fn with_nothing_wrong_nothing_is_started() {
    assert_eq!(first(&[], &[]), None);
}

#[test]
fn a_trouble_too_new_to_be_worth_saying_is_not_worth_looking_into() {
    // Two days is under the three-day line `worth_raising` draws for the person.
    let recent = [trouble("sensor.garage_door", NOW - 2 * DAY)];
    assert_eq!(first(&recent, &[]), None);
}

#[test]
fn a_trouble_the_person_called_fine_is_never_started() {
    let mut fine = trouble("sensor.garage_door", 0);
    fine.accepted = true;
    assert_eq!(first(&[fine], &[]), None);
}

// ---- what is started ----

#[test]
fn a_standing_trouble_becomes_work_with_its_source_attached() {
    let item = first(&[trouble("sensor.garage_door", 0)], &[]).expect("a four-day-old trouble");
    assert_eq!(item.request_key, "own:trouble:home:sensor.garage_door:0:1");
    assert!(
        item.objective.contains("sensor.garage_door"),
        "{}",
        item.objective
    );
    assert_eq!(
        item.source,
        WorkSource::Trouble {
            server: "home".to_owned(),
            thing: "sensor.garage_door".to_owned(),
            since_ms: 0
        }
    );
}

#[test]
fn the_oldest_trouble_goes_first() {
    let troubles = [trouble("sensor.newer", DAY / 2), trouble("sensor.older", 0)];
    let item = first(&troubles, &[]).unwrap();
    assert!(
        item.request_key.contains("sensor.older"),
        "{}",
        item.request_key
    );
}

#[test]
fn work_becomes_a_goal_in_the_lane_that_never_outranks_a_request() {
    let record = first(&[trouble("sensor.garage_door", 0)], &[])
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
fn a_long_name_still_makes_a_valid_key() {
    let long = "sensor.".to_owned() + &"x".repeat(190);
    let item = first(&[trouble(&long, 0)], &[]).unwrap();
    assert!(
        item.request_key.chars().count() <= 200,
        "{}",
        item.request_key.len()
    );
    assert!(item.into_record(at(NOW)).is_ok());
}

#[test]
fn a_name_too_long_to_store_as_a_source_is_skipped_not_half_started() {
    let absurd = "sensor.".to_owned() + &"x".repeat(400);
    let troubles = [trouble(&absurd, 0), trouble("sensor.garage_door", DAY / 2)];
    let item = first(&troubles, &[]).expect("the next one is still looked into");
    assert!(
        item.request_key.contains("sensor.garage_door"),
        "{}",
        item.request_key
    );
}

// ---- the start gate ----

#[test]
fn nothing_starts_while_a_request_of_the_persons_is_open() {
    let troubles = [trouble("sensor.garage_door", 0)];
    let mut s = situation(&troubles, &[]);
    s.a_request_is_open = true;
    assert_eq!(next_work(&s), None);
}

#[test]
fn nothing_starts_while_the_person_is_talking() {
    let troubles = [trouble("sensor.garage_door", 0)];
    let mut s = situation(&troubles, &[]);
    s.person_is_quiet = false;
    assert_eq!(next_work(&s), None);
}

#[test]
fn nothing_starts_while_its_own_lane_is_occupied() {
    let troubles = [trouble("sensor.garage_door", 0), trouble("sensor.other", 0)];
    let running = open(first(&troubles[..1], &[]).unwrap());
    assert_eq!(first(&troubles, &[running]), None);
}

#[test]
fn nothing_starts_until_the_cooldown_after_the_last_ending_has_passed() {
    let troubles = [trouble("sensor.garage_door", 0), trouble("sensor.other", 0)];
    let item = first(&troubles[..1], &[]).unwrap();
    let just_ended = ended(
        item.clone(),
        GoalStatus::Partial,
        NOW - COOLDOWN_AFTER_ENDING_MS + 1,
    );
    let next = first(&troubles, &[just_ended]);
    assert_eq!(next, None, "one millisecond short of the cooldown");

    let long_ago = ended(item, GoalStatus::Partial, NOW - COOLDOWN_AFTER_ENDING_MS);
    let next = first(&troubles, &[long_ago]).expect("exactly the cooldown has passed");
    assert!(
        next.request_key.contains("sensor.other"),
        "{}",
        next.request_key
    );
}

// ---- S5 / S8: bounded per item ----

#[test]
fn an_item_the_person_stopped_is_not_started_again() {
    // S8. Stop is the person saying not this. The cooldown passing must not undo it.
    let troubles = [trouble("sensor.garage_door", 0)];
    let item = first(&troubles, &[]).unwrap();
    let stopped = ended(item, GoalStatus::Cancelled, NOW - 30 * DAY);
    assert_eq!(first(&troubles, &[stopped]), None);
}

#[test]
fn a_stopped_item_that_comes_back_as_a_new_situation_is_a_new_item() {
    // The same device failing again later has a later `since_ms`: not the thing that was
    // stopped.
    let before = [trouble("sensor.garage_door", 0)];
    let stopped = ended(first(&before, &[]).unwrap(), GoalStatus::Cancelled, 1);
    let again = [trouble("sensor.garage_door", DAY / 2)];
    let item = first(&again, &[stopped]).expect("a different episode");
    assert!(item.request_key.ends_with(":1"), "{}", item.request_key);
}

#[test]
fn an_item_is_left_alone_inside_its_own_cooldown() {
    let troubles = [trouble("sensor.garage_door", 0)];
    let item = first(&troubles, &[]).unwrap();
    let looked = ended(item, GoalStatus::Partial, NOW - DAY);
    assert_eq!(first(&troubles, &[looked]), None);
}

#[test]
fn an_item_is_looked_into_again_once_its_cooldown_has_passed_and_the_attempt_is_counted() {
    let troubles = [trouble("sensor.garage_door", 0)];
    let item = first(&troubles, &[]).unwrap();
    let looked = ended(item, GoalStatus::Partial, NOW - ITEM_COOLDOWN_MS);
    let again = first(&troubles, &[looked]).expect("a week later it is still wrong");
    assert!(again.request_key.ends_with(":2"), "{}", again.request_key);
}

#[test]
fn an_item_is_given_up_on_after_its_attempts() {
    let troubles = [trouble("sensor.garage_door", 0)];
    let one = ended(
        first(&troubles, &[]).unwrap(),
        GoalStatus::Partial,
        NOW - 30 * DAY,
    );
    let two_item = first(&troubles, std::slice::from_ref(&one)).unwrap();
    let two = ended(two_item, GoalStatus::Partial, NOW - 20 * DAY);
    assert_eq!(MAX_ATTEMPTS_PER_ITEM, 2);
    assert_eq!(first(&troubles, &[one, two]), None);
}

#[test]
fn another_item_is_still_looked_into_while_one_is_given_up_on() {
    let troubles = [trouble("sensor.stuck", 0), trouble("sensor.fresh", DAY / 2)];
    let one = ended(
        first(&troubles[..1], &[]).unwrap(),
        GoalStatus::Partial,
        NOW - 30 * DAY,
    );
    let two_item = first(&troubles[..1], std::slice::from_ref(&one)).unwrap();
    let two = ended(two_item, GoalStatus::Partial, NOW - 20 * DAY);
    let item = first(&troubles, &[one, two]).expect("the other one");
    assert!(
        item.request_key.contains("sensor.fresh"),
        "{}",
        item.request_key
    );
}

// ---- the source is the verifier ----

fn started(troubles: &[StandingTrouble]) -> GoalRecord {
    open(first(troubles, &[]).unwrap())
}

#[test]
fn while_the_trouble_stands_the_work_goes_on() {
    let troubles = [trouble("sensor.garage_door", 0)];
    let record = started(&troubles);
    assert_eq!(recheck(&record, &troubles, at(NOW)), Recheck::Unchanged);
}

#[test]
fn when_the_trouble_clears_the_source_completes_the_goal() {
    // S4's shape for troubles: done with no model verdict, because the record no longer says
    // it is wrong.
    let record = started(&[trouble("sensor.garage_door", 0)]);
    let Recheck::Resolved(done) = recheck(&record, &[], at(NOW + MINUTE)) else {
        panic!("a trouble that cleared should resolve the goal");
    };
    assert_eq!(done.goal().status(), GoalStatus::Complete);
    assert_eq!(done.origin(), GoalOrigin::Record, "the lane is unchanged");
    assert_eq!(done.updated_at(), at(NOW + MINUTE));
    assert!(
        done.goal().outcome_notice().contains("answering again"),
        "{}",
        done.goal().outcome_notice()
    );
}

#[test]
fn a_trouble_that_cleared_and_came_back_is_not_the_one_the_goal_was_for() {
    let record = started(&[trouble("sensor.garage_door", 0)]);
    let came_back = [trouble("sensor.garage_door", NOW - DAY)];
    assert!(matches!(
        recheck(&record, &came_back, at(NOW)),
        Recheck::Resolved(_)
    ));
}

#[test]
fn another_things_clearing_does_not_resolve_this() {
    let record = started(&[trouble("sensor.garage_door", 0)]);
    let others = [trouble("sensor.garage_door", 0), trouble("sensor.other", 0)];
    assert_eq!(recheck(&record, &others, at(NOW)), Recheck::Unchanged);
    let only_other = [trouble("sensor.other", 0)];
    assert!(matches!(
        recheck(&record, &only_other, at(NOW)),
        Recheck::Resolved(_)
    ));
}

#[test]
fn when_the_person_calls_it_fine_the_work_is_withdrawn() {
    let record = started(&[trouble("sensor.garage_door", 0)]);
    let mut fine = trouble("sensor.garage_door", 0);
    fine.accepted = true;
    let Recheck::Withdrawn(why) = recheck(&record, &[fine], at(NOW)) else {
        panic!("a trouble marked fine should withdraw the work");
    };
    assert!(why.contains("sensor.garage_door"), "{why}");
}

#[test]
fn calling_a_later_episode_fine_does_not_withdraw_the_work_for_the_earlier_one() {
    // The device recovered, failed again later, and the person said *that* one is fine. The
    // goal was for the first failure, which is over: it resolved, it was not "called fine".
    let record = started(&[trouble("sensor.garage_door", 0)]);
    let mut later = trouble("sensor.garage_door", NOW - DAY);
    later.accepted = true;
    assert!(matches!(
        recheck(&record, &[later], at(NOW)),
        Recheck::Resolved(_)
    ));
}

#[test]
fn a_goal_that_has_already_ended_is_left_as_it_is() {
    let item = first(&[trouble("sensor.garage_door", 0)], &[]).unwrap();
    let done = ended(item, GoalStatus::Partial, NOW);
    assert_eq!(recheck(&done, &[], at(NOW + MINUTE)), Recheck::Unchanged);
}

#[test]
fn a_goal_the_person_asked_for_is_never_touched_by_a_source_check() {
    let asked = GoalRecord::new("asked", Goal::new("Read a source").unwrap(), at(1)).unwrap();
    assert_eq!(recheck(&asked, &[], at(NOW)), Recheck::Unchanged);
}

// ---- what the person reads ----

#[test]
fn the_trail_line_says_what_was_looked_into_and_how_it_ended() {
    let item = first(&[trouble("sensor.garage_door", 0)], &[]).unwrap();
    let line = trail_line(&ended(item, GoalStatus::Partial, NOW));
    assert!(line.starts_with("On my own I looked into"), "{line}");
    assert!(line.contains("sensor.garage_door"), "{line}");
    assert!(line.contains("partial"), "{line}");
    assert!(!line.contains('\n'), "a trail line is one line: {line}");
    assert!(line.chars().count() < 1000, "{}", line.len());
}
