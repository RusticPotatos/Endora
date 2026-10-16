use super::*;

const DAY: i64 = 24 * 60 * 60 * 1000;
const MINUTE: i64 = 60 * 1000;

fn at(day: i64, hour: i64, minute: i64) -> i64 {
    day * DAY + (hour * 60 + minute) * MINUTE
}

fn moved(key: &str, to: &str, at_ms: i64) -> Transition {
    Transition {
        key: key.to_owned(),
        from: "other".to_owned(),
        to: to.to_owned(),
        at_ms,
    }
}

/// Fourteen days of a house whose front door opens at about the same time each day, plus one
/// unrelated event a day so every day counts as observed.
fn a_fortnight(door: impl Fn(i64) -> Option<i64>) -> Vec<Transition> {
    let mut out = Vec::new();
    for day in 0..14 {
        out.push(moved("home::sensor.heartbeat", "ok", at(day, 3, 0)));
        if let Some(when) = door(day) {
            out.push(moved("home::binary_sensor.front_door", "on", when));
        }
    }
    out
}

#[test]
fn something_that_happens_at_the_same_time_every_day_is_a_routine() {
    let log = a_fortnight(|day| Some(at(day, 7, 30 + day % 5)));
    let door = find_rhythms(&log, |_| 0)
        .into_iter()
        .find(|r| r.key.ends_with("front_door"))
        .expect("a door that opens every morning");
    assert_eq!(door.to, "on");
    assert_eq!(door.days_seen, 14);
    assert_eq!(door.days_observed, 14);
    assert!(
        (450..=455).contains(&door.minute),
        "around 07:30: {}",
        door.minute
    );
}

#[test]
fn something_with_no_usual_time_is_not() {
    let log = a_fortnight(|day| Some(at(day, (day * 5) % 24, 0)));
    assert!(
        find_rhythms(&log, |_| 0)
            .iter()
            .all(|r| !r.key.ends_with("front_door"))
    );
}

#[test]
fn a_weekday_routine_clears_the_bar() {
    // Five days in seven is 71% of the log's days.
    let log = a_fortnight(|day| (day % 7 < 5).then(|| at(day, 8, 10)));
    let door = find_rhythms(&log, |_| 0)
        .into_iter()
        .find(|r| r.key.ends_with("front_door"))
        .expect("weekdays only");
    assert_eq!(door.days_seen, 10);
}

#[test]
fn a_thing_that_happened_a_few_times_is_a_coincidence_not_a_routine() {
    let log = a_fortnight(|day| (day < 4).then(|| at(day, 7, 30)));
    assert!(
        find_rhythms(&log, |_| 0)
            .iter()
            .all(|r| !r.key.ends_with("front_door"))
    );
}

#[test]
fn too_little_log_says_nothing_at_all() {
    let log: Vec<Transition> = (0..4)
        .flat_map(|day| {
            vec![
                moved("home::sensor.heartbeat", "ok", at(day, 3, 0)),
                moved("home::light.hall", "on", at(day, 18, 0)),
            ]
        })
        .collect();
    assert!(find_rhythms(&log, |_| 0).is_empty());
}

#[test]
fn a_thing_that_flaps_all_day_has_no_time_of_day() {
    // Twenty changes a day lands in every hour; none of them is a routine.
    let mut log = a_fortnight(|_| None);
    for day in 0..14 {
        for n in 0..20 {
            log.push(moved("home::binary_sensor.motion", "on", at(day, n, 5)));
        }
    }
    assert!(
        find_rhythms(&log, |_| 0)
            .iter()
            .all(|r| !r.key.ends_with("motion"))
    );
}

#[test]
fn the_sky_and_the_clock_are_not_the_houses_routine() {
    // Found on the first live run: "sun.sun goes above_horizon around 07:25, on 14 of the
    // last 15 days". True every day by definition, and nothing about the house.
    let mut log = a_fortnight(|day| Some(at(day, 7, 30)));
    for day in 0..14 {
        log.push(moved(
            "home-assistant::sun.sun",
            "above_horizon",
            at(day, 7, 25),
        ));
        log.push(moved(
            "home-assistant::sensor.date",
            "2026-10-15",
            at(day, 0, 0),
        ));
        log.push(moved(
            "home-assistant::sensor.moon_phase",
            "waxing_gibbous",
            at(day, 0, 1),
        ));
    }
    let found = find_rhythms(&log, |_| 0);
    assert!(
        found.iter().any(|r| r.key.ends_with("front_door")),
        "{found:?}"
    );
    assert!(
        found.iter().all(|r| !r.key.contains("sun.")
            && !r.key.contains("sensor.date")
            && !r.key.contains("moon")),
        "{found:?}"
    );
}

#[test]
fn readings_and_trouble_are_not_routines() {
    let mut log = a_fortnight(|_| None);
    for day in 0..14 {
        log.push(moved("home::sensor.temperature", "21.5", at(day, 6, 0)));
        log.push(moved("home::sensor.garage", "unavailable", at(day, 6, 0)));
        log.push(moved("home::sensor.hub", "unknown", at(day, 6, 0)));
    }
    let found = find_rhythms(&log, |_| 0);
    assert!(
        found.iter().all(|r| r.key.ends_with("heartbeat")),
        "{found:?}"
    );
}

#[test]
fn the_days_are_the_persons_not_greenwichs() {
    // 12:30 UTC is 07:30 five hours west of it.
    let log = a_fortnight(|day| Some(at(day, 12, 30)));
    let door = |offset| {
        find_rhythms(&log, |_| offset)
            .into_iter()
            .find(|r| r.key.ends_with("front_door"))
            .map(|r| r.minute)
    };
    assert_eq!(door(-300), Some(7 * 60 + 30));
    assert_eq!(door(0), Some(12 * 60 + 30));
}

#[test]
fn a_routine_is_kept_on_the_clock_across_a_clock_change() {
    // 07:30 every day by the person's clock, which goes back an hour after day 6: 11:30 UTC
    // while they are four hours west of it, 12:30 UTC once they are five.
    let change = 7 * DAY;
    let offset = |at_ms: i64| if at_ms < change { -240 } else { -300 };
    let log = a_fortnight(|day| Some(at(day, 11, 30) + if day >= 7 { 60 * MINUTE } else { 0 }));
    let door = find_rhythms(&log, offset)
        .into_iter()
        .find(|r| r.key.ends_with("front_door"))
        .expect("one routine, not two half ones");
    assert_eq!((door.minute, door.days_seen), (7 * 60 + 30, 14));
}

#[test]
fn a_routine_that_straddles_midnight_is_one_routine() {
    // Bed at 23:50 on some nights and 00:10 on others.
    let log = a_fortnight(|day| {
        Some(if day % 2 == 0 {
            at(day, 23, 50)
        } else {
            at(day + 1, 0, 10)
        })
    });
    let door = find_rhythms(&log, |_| 0)
        .into_iter()
        .find(|r| r.key.ends_with("front_door"))
        .expect("one routine across midnight");
    assert!(
        door.days_seen >= 13,
        "split in two by the clock changing date: {}",
        door.days_seen
    );
}

#[test]
fn only_the_first_time_each_day_counts() {
    // The door opens at 07:30 and then, every day, ten more times through the evening. The
    // routine is when it starts.
    let log = a_fortnight(|day| Some(at(day, 7, 30)));
    let mut busier = log.clone();
    for day in 0..14 {
        for n in 0..4 {
            busier.push(moved(
                "home::binary_sensor.front_door",
                "on",
                at(day, 17 + n, 0),
            ));
        }
    }
    let door = find_rhythms(&busier, |_| 0)
        .into_iter()
        .find(|r| r.key.ends_with("front_door"))
        .unwrap();
    assert!((449..=451).contains(&door.minute), "{}", door.minute);
}

#[test]
fn a_day_the_log_has_nothing_for_does_not_count_against_a_routine() {
    // The node was off for four days. The routine happened on every day that was recorded.
    let mut log = Vec::new();
    for day in (0..14).filter(|d| !(5..9).contains(d)) {
        log.push(moved(
            "home::binary_sensor.front_door",
            "on",
            at(day, 7, 30),
        ));
    }
    let door = find_rhythms(&log, |_| 0).into_iter().next().unwrap();
    assert_eq!((door.days_seen, door.days_observed), (10, 10));
}

#[test]
fn the_strongest_comes_first_and_only_a_dozen_are_kept() {
    let mut log = a_fortnight(|day| Some(at(day, 7, 30)));
    for n in 0..20 {
        for day in 0..14 {
            log.push(moved(&format!("home::light.l{n}"), "on", at(day, 18, 0)));
        }
    }
    let found = find_rhythms(&log, |_| 0);
    assert_eq!(found.len(), MOST_RHYTHMS);
    assert!(found.windows(2).all(|pair| {
        let share = |r: &Rhythm| r.days_seen as f64 / r.days_observed as f64;
        share(&pair[0]) >= share(&pair[1])
    }));
}

#[test]
fn it_says_it_as_a_count_a_person_can_check() {
    let rhythm = Rhythm {
        key: "home::binary_sensor.front_door".to_owned(),
        to: "on".to_owned(),
        minute: 7 * 60 + 32,
        days_seen: 11,
        days_observed: 14,
    };
    assert_eq!(
        rhythm.describe(),
        "binary_sensor.front_door goes on around 07:30, on 11 of the last 14 days"
    );
}

#[test]
fn five_times_in_a_fortnight_at_the_same_hour_is_still_not_most_days() {
    // Five days clears the minimum, and 36% of the log does not clear the bar.
    let log = a_fortnight(|day| (day < 5).then(|| at(day, 7, 30)));
    assert!(
        find_rhythms(&log, |_| 0)
            .iter()
            .all(|r| !r.key.ends_with("front_door"))
    );
}

#[test]
fn most_of_a_short_log_is_still_not_enough_days() {
    // Five days of log, three of them with the event: exactly the 60% bar, and only three
    // days, which is not enough to call anything a habit.
    let log: Vec<Transition> = (0..5)
        .flat_map(|day| {
            let mut day_log = vec![moved("home::sensor.heartbeat", "ok", at(day, 3, 0))];
            if day % 2 == 0 {
                day_log.push(moved(
                    "home::binary_sensor.front_door",
                    "on",
                    at(day, 7, 30),
                ));
            }
            day_log
        })
        .collect();
    assert!(
        find_rhythms(&log, |_| 0)
            .iter()
            .all(|r| !r.key.ends_with("front_door"))
    );
}

#[test]
fn a_routine_on_every_day_comes_before_one_on_most() {
    let mut log = a_fortnight(|day| Some(at(day, 7, 30)));
    for day in (0..14).filter(|d| d % 3 != 0) {
        log.push(moved("home::light.porch", "on", at(day, 19, 0)));
    }
    let found = find_rhythms(&log, |_| 0);
    let door = found
        .iter()
        .position(|r| r.key.ends_with("front_door"))
        .unwrap();
    let porch = found.iter().position(|r| r.key.ends_with("porch")).unwrap();
    assert!(door < porch, "{found:?}");
}

#[test]
fn a_state_that_is_a_date_or_a_time_is_a_clock_whatever_it_is_called() {
    let mut log = a_fortnight(|_| None);
    for day in 0..14 {
        log.push(moved(
            "home::sensor.phone_next_alarm",
            &format!("2026-10-{:02}T07:00:00", day + 1),
            at(day, 0, 5),
        ));
        log.push(moved(
            "home::sensor.kettle_last_on",
            "07:12",
            at(day, 7, 12),
        ));
    }
    let found = find_rhythms(&log, |_| 0);
    assert!(
        found.iter().all(|r| r.key.ends_with("heartbeat")),
        "{found:?}"
    );
}
