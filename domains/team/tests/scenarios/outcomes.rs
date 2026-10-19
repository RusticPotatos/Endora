//! The steps of `features/what-the-work-came-to.feature` (ADR 0132): the outcomes line on the HQ
//! screen's "How the team is doing" card, as the snapshot serves it, read after the owner, the
//! node and the members have done what the scenario says.

use cucumber::{then, when};
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

#[then(regex = r"^the outcomes say (\d+) turns? of talk against (\d+) turns? of work this week$")]
fn talk_against_work(w: &mut Office, talk: u64, work: u64) {
    let week = w.outcomes_this_week();
    assert_eq!(week["talk"]["coordination"], json!(talk), "{week}");
    assert_eq!(week["talk"]["product"], json!(work), "{week}");
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

/// The tester sends the work back, saying it already existed: the reason the memory builds are
/// measured by, kept apart from her notes.
#[when(expr = "{word} sends back {string} because it already exists: {string}")]
fn sends_back_as_a_repeat(w: &mut Office, name: String, title: String, notes: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "review",
        &json!({ "task": task.0, "verdict": "send_back", "reason": "exists_already",
                 "notes": notes }),
    )
    .expect("the tester sends it back");
}

#[then(regex = r"^the outcomes count (\d+) pieces? of work reinvented this week$")]
fn work_reinvented(w: &mut Office, count: u64) {
    let week = w.outcomes_this_week();
    assert_eq!(week["memory"]["reinvented"], json!(count), "{week}");
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
