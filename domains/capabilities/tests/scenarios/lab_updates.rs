//! The steps of `features/what-watchtower-did.feature`: the lab's containers as the observer
//! sees them (`app/node/src/api/homelab.rs` every 30 seconds), noted by
//! [`ImageUpdates`], and what `homelab_status` says of them
//! (`app/node/src/api/contracts.rs`).

use cucumber::{given, then, when};
use endora_capabilities::credits::DAY_MS;
use endora_capabilities::lab_updates::{ImageUpdates, summary, updated_at};
use serde_json::{Value, json};

use super::{Bridge, credit::day_of};

/// The lab's containers and what Endora has noted of them.
#[derive(Debug, Default)]
pub struct Lab {
    /// Each container: its name, when it was created and its image.
    containers: Vec<(String, i64, String)>,
    /// Whether Watchtower runs in the lab.
    watchtower: bool,
    /// What the node keeps between observations.
    updates: ImageUpdates,
    /// The docker service as `homelab_status` last read it, and when.
    docker: Value,
    now: i64,
}

impl Lab {
    /// One observation at `at`, as the node's loop takes it.
    fn observe(&mut self, at: i64) {
        let mut items: Vec<Value> = self
            .containers
            .iter()
            .map(|(name, created, image)| {
                json!({ "name": name, "state": "running", "health": "unknown",
                        "created_ms": created, "image": image })
            })
            .collect();
        if self.watchtower {
            items.push(
                json!({ "name": "watchtower", "state": "running", "health": "unknown",
                               "created_ms": 0, "image": "w1" }),
            );
        }
        let mut snapshot = json!({ "observed_at_ms": at, "services": [
            { "name": "docker", "state": "healthy", "items": items },
        ] });
        self.updates.observe(&mut snapshot, at);
        self.docker = snapshot["services"][0].clone();
        self.now = at;
    }

    fn item(&self, name: &str) -> &Value {
        self.docker["items"]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["name"] == name))
            .unwrap_or_else(|| panic!("{name} is not in the lab: {}", self.docker))
    }
}

/// `date` (`YYYY-MM-DD`) at `hh:mm` on the owner's clock, in UTC milliseconds.
fn at(w: &Bridge, date: &str, hh_mm: &str) -> i64 {
    let (hour, minute) = hh_mm.split_once(':').expect("a time is hh:mm");
    let minutes: i64 = hour.parse::<i64>().expect("an hour") * 60
        + minute.parse::<i64>().expect("a minute")
        - w.credit.offset();
    day_of(date) * DAY_MS + minutes * 60_000
}

/// Long before the scenario: when a container Endora first sees was made.
const LONG_AGO: i64 = 1_790_000_000_000;

#[given("Watchtower runs in the lab")]
fn watchtower_runs(w: &mut Bridge) {
    w.lab.watchtower = true;
}

#[given("Watchtower does not run in the lab")]
fn watchtower_does_not_run(w: &mut Bridge) {
    w.lab.watchtower = false;
}

#[given(
    regex = r#"^at (\S+) (\d\d:\d\d) Endora sees "([^"]+)" on image "([^"]+)", "([^"]+)" on image "([^"]+)" and "([^"]+)" on image "([^"]+)"$"#
)]
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn endora_sees_three(
    w: &mut Bridge,
    date: String,
    time: String,
    a: String,
    a_image: String,
    b: String,
    b_image: String,
    c: String,
    c_image: String,
) {
    for (name, image) in [(a, a_image), (b, b_image), (c, c_image)] {
        w.lab.containers.push((name, LONG_AGO, image));
    }
    let when = at(w, &date, &time);
    w.lab.observe(when);
}

#[given(regex = r#"^at (\S+) (\d\d:\d\d) Endora sees "([^"]+)" on image "([^"]+)"$"#)]
#[allow(clippy::needless_pass_by_value)]
fn endora_sees_one(w: &mut Bridge, date: String, time: String, name: String, image: String) {
    w.lab.containers.push((name, LONG_AGO, image));
    let when = at(w, &date, &time);
    w.lab.observe(when);
}

#[when(regex = r#"^at (\S+) (\d\d:\d\d) "([^"]+)" is created again on image "([^"]+)"$"#)]
#[allow(clippy::needless_pass_by_value)]
fn created_again(w: &mut Bridge, date: String, time: String, name: String, image: String) {
    let when = at(w, &date, &time);
    let container = w
        .lab
        .containers
        .iter_mut()
        .find(|(n, _, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not in the lab"));
    container.1 = when;
    container.2 = image;
    w.lab.observe(when);
}

/// A restart keeps the container, and so when it was created and its image.
#[when(regex = r#"^at (\S+) (\d\d:\d\d) "([^"]+)" restarts$"#)]
#[allow(clippy::needless_pass_by_value)]
fn restarts(w: &mut Bridge, date: String, time: String, _name: String) {
    let when = at(w, &date, &time);
    w.lab.observe(when);
}

#[when(regex = r"^a member reads homelab_status at (\S+) (\d\d:\d\d)$")]
#[allow(clippy::needless_pass_by_value)]
fn reads(w: &mut Bridge, date: String, time: String) {
    let when = at(w, &date, &time);
    w.lab.observe(when);
}

#[then(expr = "it says {string}")]
fn it_says(w: &mut Bridge, line: String) {
    let said = summary(&w.lab.docker, w.lab.now, w.credit.offset());
    assert_eq!(said.as_deref(), Some(line.as_str()), "{}", w.lab.docker);
}

#[then(expr = "{string} is said to be updated {string}")]
fn said_updated(w: &mut Bridge, name: String, when: String) {
    let said = updated_at(w.lab.item(&name), w.credit.offset());
    assert_eq!(said.as_deref(), Some(when.as_str()));
}

#[then(expr = "{string} is not said to be updated")]
fn not_said_updated(w: &mut Bridge, name: String) {
    let said = updated_at(w.lab.item(&name), w.credit.offset());
    assert_eq!(said, None);
}
