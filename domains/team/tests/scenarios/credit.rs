//! The steps of `features/the-pool-follows-the-credit.feature` (ADR 0113, amended 2026-10-31):
//! the node says what is left of the Claude grant and when the plan renews, and the day's pool
//! is sized from it.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::domain::money::{DAY_MS, DOLLAR};

use super::{Office, micros};

#[given(regex = r"^a pool of \$(\d+(?:\.\d+)?) a month$")]
fn a_pool_a_month(w: &mut Office, dollars: f64) {
    w.pool_per_month = (dollars * DOLLAR as f64).round() as i64;
    w.restart_with_settings();
}

/// Noon UTC on the day, as the owner's clock in these scenarios is UTC.
#[given(
    regex = r"^it is (\d+) (January|February|March|April|May|June|July|August|September|October|November|December) (\d{4})$"
)]
fn it_is(w: &mut Office, day: u32, month: String, year: i64) {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month = MONTHS.iter().position(|m| *m == month).expect("a month") + 1;
    let date = format!("{year:04}-{month:02}-{day:02}");
    let days = endora_team::domain::credit_pool::day_of_date(&date).expect("a date");
    w.clock
        .0
        .store(days * DAY_MS + DAY_MS / 2, Ordering::SeqCst);
}

#[given(
    regex = r"^the node says the Claude grant has \$(\d+(?:\.\d+)?) left and renews on (\d{4}-\d\d-\d\d)$"
)]
#[when(
    regex = r"^the node says the Claude grant has \$(\d+(?:\.\d+)?) left and renews on (\d{4}-\d\d-\d\d)$"
)]
fn node_says_the_grant(w: &mut Office, dollars: f64, renews: String) {
    w.hq.tell_claude_credit(Some(micros(dollars)), Some(&renews))
        .expect("HQ keeps what the node says");
}

#[when("the node says no plan grant is set")]
fn node_says_no_grant(w: &mut Office) {
    w.hq.tell_claude_credit(None, None)
        .expect("HQ keeps what the node says");
}

#[then(regex = r"^today's pool is \$(\d+\.\d\d)$")]
fn todays_pool(w: &mut Office, dollars: f64) {
    let snap = w.hq.snapshot().expect("the snapshot is built");
    let per_day = snap["pool"]["per_day"].as_i64().expect("today's pool");
    // To the cent, as the owner reads it.
    assert_eq!(
        (per_day + 5_000) / 10_000,
        micros(dollars) / 10_000,
        "{}",
        snap["pool"]
    );
}

#[then(expr = "the pool says {string}")]
fn the_pool_says(w: &mut Office, said: String) {
    let snap = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(
        snap["pool"]["sized"]["why"].as_str(),
        Some(said.as_str()),
        "{}",
        snap["pool"]
    );
}
