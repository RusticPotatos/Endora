//! The steps of `features/the-agents-map.feature` (the owner's decision M5): the Core team's
//! map, read as the owner's screen reads it, from HQ's public read.

use cucumber::{then, when};
use serde_json::Value;

use super::Office;

impl Office {
    /// The agents map now.
    fn agents_map(&self) -> Value {
        self.hq.team_map().expect("HQ reads the map")
    }
}

fn member<'a>(map: &'a Value, name: &str) -> &'a Value {
    map["members"]
        .as_array()
        .and_then(|m| m.iter().find(|x| x["name"] == name))
        .unwrap_or_else(|| panic!("{name} is on the map: {map:#}"))
}

fn held<'a>(map: &'a Value, name: &str, title: &str) -> (&'a Value, &'a Value) {
    let m = member(map, name);
    m["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|g| {
            g["items"]
                .as_array()
                .and_then(|items| items.iter().find(|i| i["title"] == title))
                .map(|i| (g, i))
        })
        .unwrap_or_else(|| panic!("{name} holds {title:?}: {map:#}"))
}

#[when("every running turn ends")]
fn every_turn_ends(w: &mut Office) {
    w.finish_running();
}

#[then(expr = "the agents map has {int} turn running, and only {word}'s line moves")]
fn only_one_moves(w: &mut Office, running: usize, name: String) {
    let map = w.agents_map();
    assert_eq!(map["running"], running, "{map:#}");
    for m in map["members"].as_array().expect("members") {
        let moving = m["live"]
            .as_array()
            .is_some_and(|l| l.iter().any(|x| x["moving"] == true));
        assert_eq!(moving, m["name"] == name.as_str(), "{map:#}");
    }
}

#[then(expr = "on the agents map {word} holds {string} under {string}")]
fn holds_under(w: &mut Office, name: String, title: String, project: String) {
    let map = w.agents_map();
    let (group, _) = held(&map, &name, &title);
    assert_eq!(group["project"], project, "{map:#}");
}

#[then(expr = "the agents map says {string}")]
fn says(w: &mut Office, said: String) {
    let map = w.agents_map();
    assert_eq!(map["said"], said, "{map:#}");
    let moving = map["members"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["live"].as_array().into_iter().flatten())
        .any(|l| l["moving"] == true);
    assert!(!moving, "{map:#}");
}

#[then(expr = "on the agents map {word}'s {string} asks the owner")]
fn asks_owner(w: &mut Office, name: String, title: String) {
    let map = w.agents_map();
    let (_, item) = held(&map, &name, &title);
    assert_eq!(item["asks_owner"], true, "{map:#}");
}

#[then(expr = "the agents map's desk has {int} card under {string}")]
fn desk_under(w: &mut Office, count: usize, project: String) {
    let map = w.agents_map();
    let group = map["desk"]
        .as_array()
        .and_then(|d| d.iter().find(|g| g["project"] == project.as_str()))
        .unwrap_or_else(|| panic!("a desk group for {project:?}: {map:#}"));
    assert_eq!(group["count"], count, "{map:#}");
}

#[then(expr = "the agents map shows {word} as {string}")]
fn shows_as(w: &mut Office, name: String, state: String) {
    let map = w.agents_map();
    assert_eq!(member(&map, &name)["state"], state, "{map:#}");
}
