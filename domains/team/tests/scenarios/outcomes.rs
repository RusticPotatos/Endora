//! The steps of `features/what-the-work-came-to.feature` (ADR 0132): the outcomes line on the HQ
//! screen's "How the team is doing" card, as the snapshot serves it, read after the owner, the
//! node and the members have done what the scenario says.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::domain::Brain;
use endora_team::domain::money::DAY_MS;
use serde_json::json;

use super::{Office, micros};

impl Office {
    /// The outcomes, as the HQ screen's card reads them.
    fn outcomes(&self) -> serde_json::Value {
        self.hq.snapshot().expect("the snapshot is built")["scorecard"]["outcomes"].clone()
    }

    /// The week's outcomes.
    fn outcomes_this_week(&self) -> serde_json::Value {
        self.outcomes()["week"].clone()
    }
}

#[then(regex = r"^the outcomes say (\d+) changes? landed this week$")]
fn changes_landed(w: &mut Office, count: u64) {
    let week = w.outcomes_this_week();
    assert_eq!(week["landed"], json!(count), "{week}");
}

#[then("the outcomes give no cost for a landed change")]
fn no_cost_for_a_landed_change(w: &mut Office) {
    let week = w.outcomes_this_week();
    assert_eq!(week["per_landed"], serde_json::Value::Null, "{week}");
}

#[then(
    regex = r"^the outcomes say (\d+) branch(?:es)? submitted and (\d+) checks? passed this week$"
)]
fn branches_and_checks(w: &mut Office, branches: u64, checks: u64) {
    let week = w.outcomes_this_week();
    assert_eq!(week["submitted"], json!(branches), "{week}");
    assert_eq!(week["checks_passed"], json!(checks), "{week}");
}

#[then(regex = r"^the outcomes say \$(\d+(?:\.\d+)?) was spent this week$")]
fn was_spent(w: &mut Office, dollars: f64) {
    let week = w.outcomes_this_week();
    assert_eq!(week["spent"], json!(micros(dollars)), "{week}");
}

#[then(
    regex = r"^(\d+) turns? waits? for budget and (\d+|none) for the Mac, the oldest for (\d+) minutes$"
)]
fn turns_wait(w: &mut Office, budget: u64, mac: String, minutes: u64) {
    let waiting = w.outcomes()["waiting"].clone();
    let mac: u64 = mac.parse().unwrap_or(0);
    assert_eq!(waiting["budget"], json!(budget), "{waiting}");
    assert_eq!(waiting["mac"], json!(mac), "{waiting}");
    assert_eq!(waiting["oldest_ms"], json!(minutes * 60_000), "{waiting}");
}

#[then(
    regex = r"^the outcomes count (\d+) asks? of the record, (\d+) consults? asked and (\d+) answered this week$"
)]
fn the_memory_is_used(w: &mut Office, asks: u64, consults: u64, answered: u64) {
    let memory = w.outcomes_this_week()["memory"].clone();
    assert_eq!(memory["record_asked"], json!(asks), "{memory}");
    assert_eq!(memory["consults"]["asked"], json!(consults), "{memory}");
    assert_eq!(memory["consults"]["answered"], json!(answered), "{memory}");
}

// --- What each model cost (the owner, 2026-10-31) --------------------------------------------

impl Office {
    /// What each model cost, as the Money card reads it.
    fn by_model(&self) -> serde_json::Value {
        self.hq.snapshot().expect("the snapshot is built")["scorecard"]["by_model"].clone()
    }

    /// The row of the brain the owner calls `label`, if a turn ran on it in either week.
    fn by_model_row(&self, label: &str) -> Option<serde_json::Value> {
        let brain = brain_called(label);
        self.by_model()["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|r| r["brain"] == brain.name())
            .cloned()
    }
}

/// The brain the owner calls `label`.
fn brain_called(label: &str) -> Brain {
    [
        Brain::Local,
        Brain::Mac,
        Brain::Opus,
        Brain::Sonnet,
        Brain::Haiku,
        Brain::DeepSeek,
    ]
    .into_iter()
    .find(|b| b.label() == label)
    .unwrap_or_else(|| panic!("no brain is called {label}"))
}

/// The key of a week in a row: `now` or `before`.
fn week_of(said: &str) -> &'static str {
    if said == "this week" { "now" } else { "before" }
}

#[given(regex = r"^(\d+) days pass$")]
#[when(regex = r"^(\d+) days pass$")]
fn days_pass(w: &mut Office, days: i64) {
    w.clock.0.fetch_add(days * DAY_MS, Ordering::SeqCst);
    w.tick();
}

#[then(regex = r"^the cost by model puts (.+) first, then (.+)$")]
fn by_model_order(w: &mut Office, first: String, second: String) {
    let shown = w.by_model();
    let names: Vec<&str> = shown["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter_map(|r| r["brain"].as_str())
        .collect();
    assert_eq!(
        names.get(..2),
        Some(&[brain_called(&first).name(), brain_called(&second).name()][..]),
        "{shown}"
    );
}

#[then(
    regex = r"^(this week|the week before) (.+?) ran (\d+) turns? for \$(\d+\.\d\d), \$(\d+\.\d\d) a turn(?:, (\d+)% of the spend)?$"
)]
fn brain_ran(
    w: &mut Office,
    week: String,
    label: String,
    turns: u64,
    spent: f64,
    per_turn: f64,
    share: String,
) {
    let row = w
        .by_model_row(&label)
        .unwrap_or_else(|| panic!("{label} has no row: {}", w.by_model()));
    let shown = &row[week_of(&week)];
    assert_eq!(shown["turns"], json!(turns), "{row}");
    assert_eq!(shown["spent"], json!(micros(spent)), "{row}");
    assert_eq!(shown["per_turn"], json!(micros(per_turn)), "{row}");
    if let Ok(share) = share.parse::<u64>() {
        assert_eq!(shown["share"], json!(share), "{row}");
    }
}

#[then(regex = r"^(this week|the week before) (.+?) ran no turns$")]
fn brain_ran_none(w: &mut Office, week: String, label: String) {
    let row = w
        .by_model_row(&label)
        .unwrap_or_else(|| panic!("{label} has no row: {}", w.by_model()));
    let shown = &row[week_of(&week)];
    assert_eq!(shown["turns"], json!(0), "{row}");
    assert_eq!(shown["per_turn"], serde_json::Value::Null, "{row}");
}

#[then(regex = r"^(.+?)'s cost went (up|down) and (.+?)'s went (up|down)$")]
fn costs_went(w: &mut Office, first: String, way: String, second: String, other: String) {
    for (label, way) in [(first, way), (second, other)] {
        let row = w.by_model_row(&label).expect("a row");
        assert_eq!(row["trend"], json!(way), "{row}");
    }
}
