//! What recreated the lab's containers (D11, ADR 0106 amended 2026-10-31). The NAS's
//! Watchtower recreates containers on new images every night, and the team, seeing a container
//! up for four hours, could not tell an update from a crash.
//!
//! From Docker alone, through the observer Endora already reads: a container whose creation
//! time moved was created again (a restart keeps it), and one created again on a different
//! image was updated. Endora remembers each container's creation and image since it started,
//! in memory, so what happened before is not guessed: the line says since when it knows.
//! Endora's own containers are released by Endora and left out. Nothing of Watchtower's log is
//! read: it can hold notification URLs, and the observer has no route to it.

use std::collections::HashMap;

use serde_json::Value;

use crate::contracts::endoras_own;

/// How far back the line of updates looks.
pub const LATELY_MS: i64 = 24 * 60 * 60 * 1000;

/// At most this many names a line, then how many more.
const NAMES: usize = 12;

const MINUTE_MS: i64 = 60_000;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// What Endora has seen of the containers' creations and images since it started.
#[derive(Debug, Default)]
pub struct ImageUpdates {
    /// Each container's creation (ms) and image id, as last seen.
    seen: HashMap<String, (i64, String)>,
    /// When each container was last created again on a new image, if its last re-creation was
    /// one.
    updated: HashMap<String, i64>,
    /// When Endora first saw the containers.
    since: Option<i64>,
}

impl ImageUpdates {
    /// Nothing seen yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Notes one snapshot of the observer's and marks it: each container last created again on
    /// a new image gets `updated_at_ms`, and the docker service `updates_since_ms`, when Endora
    /// began to know. A snapshot whose docker service was not seen changes nothing.
    pub fn observe(&mut self, snapshot: &mut Value, now: i64) {
        let Some(docker) = snapshot["services"]
            .as_array_mut()
            .and_then(|services| services.iter_mut().find(|s| s["name"] == "docker"))
        else {
            return;
        };
        if !matches!(docker["state"].as_str(), Some("healthy" | "degraded")) {
            return;
        }
        let since = *self.since.get_or_insert(now);
        docker["updates_since_ms"] = since.into();
        for item in docker["items"].as_array_mut().into_iter().flatten() {
            let name = item["name"].as_str().unwrap_or("").to_owned();
            let created = item["created_ms"].as_i64();
            let image = item["image"].as_str().filter(|i| !i.is_empty());
            let (Some(created), Some(image)) = (created, image) else {
                continue;
            };
            if name.is_empty() || endoras_own(&name, "") {
                continue;
            }
            let before = self.seen.insert(name.clone(), (created, image.to_owned()));
            // Created again since last seen: on a new image, an update; on the same, not.
            match before {
                Some((was, ref was_image)) if created > was && was_image == image => {
                    self.updated.remove(&name);
                }
                Some((was, _)) if created > was => {
                    self.updated.insert(name.clone(), created);
                }
                _ => {}
            }
            if let Some(at) = self.updated.get(&name) {
                item["updated_at_ms"] = (*at).into();
            }
        }
    }
}

/// The owner's day (from 1970-01-01) and minute of the day at `at`.
fn local(at: i64, offset_minutes: i64) -> (i64, i64) {
    let minutes = at.div_euclid(MINUTE_MS) + offset_minutes;
    (minutes.div_euclid(24 * 60), minutes.rem_euclid(24 * 60))
}

fn hh_mm(minute: i64) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// A moment on the owner's clock, as the lab's lines say it: `31 Oct 03:24`.
#[must_use]
pub fn when(at: i64, offset_minutes: i64) -> String {
    let (day, minute) = local(at, offset_minutes);
    let (_, month, date) = crate::credits::civil_from_days(day);
    let month = MONTHS[(month as usize).saturating_sub(1) % 12];
    format!("{date} {month} {}", hh_mm(minute))
}

/// When a container, as [`ImageUpdates::observe`] marked it, was last created again on a new
/// image, on the owner's clock; `None` when it was not, or Endora does not know.
#[must_use]
pub fn updated_at(item: &Value, offset_minutes: i64) -> Option<String> {
    item["updated_at_ms"]
        .as_i64()
        .map(|at| when(at, offset_minutes))
}

/// When an update was, in the owner's words: last night (yesterday from 18:00 to today 06:00),
/// today, or yesterday.
fn day_word(at: i64, now: i64, offset_minutes: i64) -> &'static str {
    let (day, minute) = local(at, offset_minutes);
    let (today, _) = local(now, offset_minutes);
    let night = if day == today {
        minute < 6 * 60
    } else {
        minute >= 18 * 60
    };
    if night {
        return "last night";
    }
    if day == today {
        return "today";
    }
    "yesterday"
}

/// One line of what was created again on a new image in the last day, from the docker service
/// as [`ImageUpdates::observe`] marked it: "Updated last night by Watchtower: sonarr, nzbget
/// (03:24)". By Watchtower when a container of that name is in the lab. With none, since when
/// Endora has known; `None` when it has not seen the containers.
#[must_use]
pub fn summary(docker: &Value, now: i64, offset_minutes: i64) -> Option<String> {
    let items = docker["items"].as_array()?;
    let mut lately: Vec<(i64, &str)> = items
        .iter()
        .filter_map(|item| Some((item["updated_at_ms"].as_i64()?, item["name"].as_str()?)))
        .filter(|(at, _)| now - at < LATELY_MS)
        .collect();
    if lately.is_empty() {
        let since = docker["updates_since_ms"].as_i64()?;
        if now - since >= LATELY_MS {
            return Some("No image updates in the last day".to_owned());
        }
        return Some(format!(
            "No image updates seen since {}",
            when(since, offset_minutes)
        ));
    }
    // By time; at one time, in the lab's own order.
    lately.sort_by_key(|(at, _)| *at);
    let watchtower = items.iter().any(|item| {
        item["name"]
            .as_str()
            .is_some_and(|n| n.to_ascii_lowercase().contains("watchtower"))
    });
    let by = if watchtower {
        "by Watchtower"
    } else {
        "on a new image"
    };
    // Consecutive updates said with the same day word go together.
    let mut groups: Vec<(&str, Vec<(i64, &str)>)> = Vec::new();
    for (at, name) in lately {
        let word = day_word(at, now, offset_minutes);
        match groups.last_mut() {
            Some((last, group)) if *last == word => group.push((at, name)),
            _ => groups.push((word, vec![(at, name)])),
        }
    }
    let said = |group: &[(i64, &str)]| {
        let names: Vec<&str> = group.iter().take(NAMES).map(|(_, n)| *n).collect();
        let more = group.len().saturating_sub(NAMES);
        let more = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        let first = hh_mm(local(group[0].0, offset_minutes).1);
        let last = hh_mm(local(group[group.len() - 1].0, offset_minutes).1);
        let time = if first == last {
            first
        } else {
            format!("{first} to {last}")
        };
        format!("{}{more} ({time})", names.join(", "))
    };
    if let [(word, group)] = groups.as_slice() {
        return Some(format!("Updated {word} {by}: {}", said(group)));
    }
    let parts: Vec<String> = groups
        .iter()
        .map(|(word, group)| format!("{word} {}", said(group)))
        .collect();
    Some(format!("Updated {by}: {}", parts.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 2026-10-31 00:00 UTC.
    const MIDNIGHT: i64 = 1_793_404_800_000;
    const HOUR: i64 = 60 * MINUTE_MS;

    fn lab(items: Value) -> Value {
        json!({ "services": [{ "name": "docker", "state": "healthy", "items": items }] })
    }

    #[test]
    fn a_moment_is_said_on_the_owners_clock() {
        assert_eq!(
            when(MIDNIGHT + 3 * HOUR + 24 * MINUTE_MS, 0),
            "31 Oct 03:24"
        );
        assert_eq!(when(MIDNIGHT + 2 * HOUR, -4 * 60), "30 Oct 22:00");
    }

    #[test]
    fn a_container_unseen_before_is_not_guessed_at() {
        let mut updates = ImageUpdates::new();
        let mut snapshot =
            lab(json!([{ "name": "sonarr", "created_ms": MIDNIGHT, "image": "a2" }]));
        updates.observe(&mut snapshot, MIDNIGHT + HOUR);
        let docker = &snapshot["services"][0];
        assert!(docker["items"][0]["updated_at_ms"].is_null());
        assert_eq!(docker["updates_since_ms"], MIDNIGHT + HOUR);
    }

    #[test]
    fn a_container_missing_for_a_moment_is_still_known_when_it_comes_back() {
        let mut updates = ImageUpdates::new();
        let mut first = lab(json!([{ "name": "sonarr", "created_ms": 1, "image": "a1" }]));
        updates.observe(&mut first, MIDNIGHT);
        // Watchtower removed the old container before it created the new one.
        updates.observe(&mut lab(json!([])), MIDNIGHT + MINUTE_MS);
        let mut back =
            lab(json!([{ "name": "sonarr", "created_ms": MIDNIGHT + MINUTE_MS, "image": "a2" }]));
        updates.observe(&mut back, MIDNIGHT + 2 * MINUTE_MS);
        assert_eq!(
            back["services"][0]["items"][0]["updated_at_ms"],
            MIDNIGHT + MINUTE_MS
        );
    }

    #[test]
    fn an_observer_that_saw_nothing_changes_nothing() {
        let mut updates = ImageUpdates::new();
        let mut down =
            json!({ "services": [{ "name": "docker", "state": "unavailable", "items": [] }] });
        updates.observe(&mut down, MIDNIGHT);
        assert!(down["services"][0]["updates_since_ms"].is_null());
        assert_eq!(summary(&down["services"][0], MIDNIGHT, 0), None);
    }

    #[test]
    fn a_long_line_names_twelve_and_counts_the_rest() {
        let items: Vec<Value> = (0..15)
            .map(|i| json!({ "name": format!("app{i:02}"), "updated_at_ms": MIDNIGHT + 3 * HOUR }))
            .collect();
        let line = summary(&json!({ "items": items }), MIDNIGHT + 8 * HOUR, 0).unwrap();
        assert!(
            line.starts_with("Updated last night on a new image: app00, "),
            "{line}"
        );
        assert!(line.ends_with("app11 and 3 more (03:00)"), "{line}");
    }
}
