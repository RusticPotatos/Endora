//! The steps of `features/the-live-work-map.feature` (the owner's decision M3): one item's map,
//! read as the owner's screen reads it, from HQ's public read.

use cucumber::then;
use serde_json::Value;

use super::Office;

impl Office {
    /// The live map of the task titled `title`.
    fn live_map(&self, title: &str) -> Value {
        self.hq
            .task_live(self.task(title))
            .expect("HQ reads the map")
    }
}

fn steps(map: &Value) -> &Vec<Value> {
    map["steps"].as_array().expect("steps")
}

#[then(expr = "the live map of {string} is {string}")]
fn is_state(w: &mut Office, title: String, state: String) {
    let map = w.live_map(&title);
    assert_eq!(map["state"], state, "{map:#}");
}

#[then(expr = "the live map of {string} is {string}: {string}")]
fn is_state_said(w: &mut Office, title: String, state: String, said: String) {
    let map = w.live_map(&title);
    assert_eq!(map["state"], state, "{map:#}");
    assert_eq!(map["said"], said, "{map:#}");
}

#[then(expr = "the live map of {string} puts the question {string} first for the owner")]
fn question_first(w: &mut Office, title: String, words: String) {
    let map = w.live_map(&title);
    let first = &map["attention"][0];
    assert_eq!(first["kind"], "question", "{map:#}");
    assert!(
        first["text"].as_str().unwrap_or_default().contains(&words),
        "{map:#}"
    );
}

#[then(expr = "the live map of {string} holds nothing for the owner")]
fn nothing_for_owner(w: &mut Office, title: String) {
    let map = w.live_map(&title);
    assert_eq!(
        map["attention"].as_array().map(Vec::len),
        Some(0),
        "{map:#}"
    );
}

#[then(expr = "the live map of {string} shows the owner's decision on its question")]
fn decision_shown(w: &mut Office, title: String) {
    let map = w.live_map(&title);
    let card = steps(&map)
        .iter()
        .find(|s| s["kind"] == "desk")
        .expect("the question is a step");
    assert_eq!(card["tone"], "owner", "{map:#}");
    let owner = card["owner"].as_str().unwrap_or_default();
    assert!(!owner.is_empty() && owner != "Waiting on you", "{map:#}");
}

#[then(expr = "the live map of {string} shows a failed check")]
fn failed_check(w: &mut Office, title: String) {
    let map = w.live_map(&title);
    assert!(
        steps(&map)
            .iter()
            .any(|s| s["kind"] == "check" && s["tone"] == "bad"),
        "{map:#}"
    );
}

#[then(expr = "the live map of {string} goes {string}")]
fn goes(w: &mut Office, title: String, kinds: String) {
    let map = w.live_map(&title);
    let seen: Vec<&str> = steps(&map)
        .iter()
        .filter_map(|s| s["kind"].as_str())
        .collect();
    let want: Vec<&str> = kinds.split(", ").collect();
    assert_eq!(seen, want, "{map:#}");
}

#[then(expr = "only {word} takes part in the live map of {string}")]
fn only_takes_part(w: &mut Office, name: String, title: String) {
    let map = w.live_map(&title);
    let people: Vec<&str> = map["people"]
        .as_array()
        .expect("people")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(people, [name.as_str()], "{map:#}");
}
