//! The steps of `features/the-macs-brain*.feature` (ADR 0130, ADR 0131): work that needs the
//! owner's Mac, whose turn waits until the node says the Mac is free.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::application::{Engine, TeamStore};
use endora_team::domain::RunStatus;
use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};

use super::Office;

impl Office {
    /// Whether `name` has a turn running now.
    fn has_a_turn_running(&self, name: &str) -> bool {
        let id = self.member(name);
        self.hq
            .store()
            .runs()
            .expect("the store answers")
            .iter()
            .any(|r| r.member == id && r.status == RunStatus::Running)
    }
}

#[given("the node says the Mac is ready")]
#[when("the node says the Mac is ready")]
fn the_node_says_ready(w: &mut Office) {
    w.mac_said = true;
    w.hq.tell_mac(true, "ready");
}

#[given(expr = "the node says the Mac is not ready: {string}")]
#[when(expr = "the node says the Mac is not ready: {string}")]
fn the_node_says_not_ready(w: &mut Office, why: String) {
    w.mac_said = true;
    w.hq.tell_mac(false, &why);
}

/// The scenario is about the node saying nothing: until it does, the Mac is not assumed free.
#[given("the node has said nothing about the Mac")]
fn the_node_has_said_nothing(w: &mut Office) {
    w.mac_said = true;
}

#[given(expr = "{int} minutes pass without the node speaking")]
#[when(expr = "{int} minutes pass without the node speaking")]
fn minutes_pass(w: &mut Office, minutes: i64) {
    w.clock.0.fetch_add(minutes * 60_000, Ordering::SeqCst);
}

#[then(expr = "{word} has no turn running")]
fn has_no_turn_running(w: &mut Office, name: String) {
    assert!(
        !w.has_a_turn_running(&name),
        "{name} has a turn running on the Mac"
    );
}

#[then(expr = "{word}'s turn on {string} is waiting for the Mac: {string}")]
fn turn_is_waiting(w: &mut Office, name: String, title: String, why: String) {
    let member = w.member(&name);
    let task = w.task(&title);
    let waiting =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .find(|r| r.member == member && r.task == Some(task) && r.status == RunStatus::Queued)
            .unwrap_or_else(|| panic!("{name} has no turn waiting on {title:?}"));
    assert_eq!(
        waiting.end_reason,
        format!("waiting for the Mac: {why}"),
        "{name}'s turn says something else"
    );
    w.wakes_when_waiting = w.hq.wakes().expect("the store answers").len();
    // The owner's screen says the same.
    let shown = w.hq.snapshot().expect("the snapshot is built");
    let run = shown["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .find(|r| r["id"] == waiting.id.0)
        .expect("the waiting turn is on screen");
    assert_eq!(run["why"], format!("waiting for the Mac: {why}"));
    assert!(shown["mac"]["waiting"].as_u64().unwrap_or(0) >= 1);
}

#[then("the pool has paid nothing")]
fn the_pool_paid_nothing(w: &mut Office) {
    let shown = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(shown["pool"]["committed"], 0);
    assert_eq!(shown["pool"]["spent"], 0);
}

// --- HQ restarting ----------------------------------------------------------------------------

/// A file for this scenario's team, new each time.
fn the_teams_file() -> String {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir()
        .join(format!(
            "endora-mac-brain-{}-{unique}.db",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

/// An HQ over the file at `path`, as the service starts one, with the clock the steps move.
pub(super) fn hq_over(w: &Office, path: &str) -> super::Hq {
    Engine::new(
        SqliteTeamStore::open(path).expect("the team's file opens"),
        SimulatedRunner::new(4),
        w.clock.clone(),
        w.settings(),
    )
}

#[given("HQ keeps its team in a file")]
fn keeps_its_team_in_a_file(w: &mut Office) {
    let path = the_teams_file();
    w.hq = hq_over(w, &path);
    w.file = Some(path);
}

#[when("HQ restarts")]
fn hq_restarts(w: &mut Office) {
    let path = w.file.clone().expect("HQ keeps its team in a file");
    // The old HQ is dropped first: nothing of its memory survives, only the file.
    w.hq = Engine::new(
        SqliteTeamStore::in_memory().expect("an in-memory store"),
        SimulatedRunner::new(4),
        w.clock.clone(),
        w.settings(),
    );
    w.hq = hq_over(w, &path);
}

// --- The Mac going away under a turn ------------------------------------------------------------

#[when(expr = "the owner comes back to the Mac under {word}'s turn {int} times")]
fn the_owner_comes_back(w: &mut Office, name: String, times: u32) {
    // The node's word about the Mac is the scenario's from here.
    w.mac_said = true;
    for _ in 0..times {
        w.hq.tell_mac(true, "ready");
        let context = w.turn_of(&name);
        // The owner touches the keyboard: the node says so, and the turn's next call to the
        // Mac is refused, so the turn fails.
        w.hq.tell_mac(false, "owner active");
        w.hq.runner().finish(context.run, false);
        w.tick();
        w.clock.0.fetch_add(61_000, Ordering::SeqCst);
    }
}
