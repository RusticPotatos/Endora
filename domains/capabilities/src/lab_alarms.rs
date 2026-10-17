//! The home lab's alarms (ADR 0107): when a container or a service goes from well to
//! trouble, Endora tells HQ, whose routine for the home lab hands its member the work. Only
//! new trouble: what was already wrong when Endora first saw it, and Endora's own containers,
//! never raise one, and a container raises at most one an hour.

use std::collections::HashMap;

use serde_json::Value;

use crate::contracts::endoras_own;

/// At most one alarm a container (or service) this often.
pub const ALARM_EVERY_MS: i64 = 60 * 60 * 1000;

/// How a container or service was last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    /// Running, and not unhealthy; or a service that answers.
    Well,
    /// In trouble, in this word: unhealthy, restarting, exited, dead, unavailable.
    Trouble(String),
    /// Neither: paused, created, starting, not set up.
    Other,
}

/// A container's state and health, as the observer has them.
fn container(state: &str, health: &str) -> Seen {
    if health == "unhealthy" {
        return Seen::Trouble("unhealthy".to_owned());
    }
    if matches!(state, "restarting" | "exited" | "dead") {
        return Seen::Trouble(state.to_owned());
    }
    if state == "running" {
        return Seen::Well;
    }
    Seen::Other
}

/// A service's state, as the observer has it.
fn service(state: &str) -> Seen {
    match state {
        "healthy" | "degraded" => Seen::Well,
        "unavailable" => Seen::Trouble("unavailable".to_owned()),
        _ => Seen::Other,
    }
}

/// Something that went from well to trouble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alarm {
    /// The container or service.
    pub name: String,
    /// What it became.
    pub became: String,
}

impl Alarm {
    /// The alarm as HQ is told it: a short factual line, at the owner's time of day.
    #[must_use]
    pub fn said(&self, hh_mm: &str) -> String {
        format!("{} became {} at {hh_mm}", self.name, self.became)
    }
}

/// What Endora has seen of the home lab since it started, in memory: a restart begins again,
/// taking what it then sees as how things are.
#[derive(Debug, Default)]
pub struct LabWatch {
    seen: HashMap<String, Seen>,
    alarmed: HashMap<String, i64>,
}

impl LabWatch {
    /// A watch that has seen nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one snapshot of the observer's and returns what went from well to trouble since
    /// the last: new trouble only, never Endora's own, at most one an hour a name.
    pub fn observe(&mut self, snapshot: &Value, now: i64) -> Vec<Alarm> {
        let mut alarms = Vec::new();
        for svc in snapshot["services"].as_array().into_iter().flatten() {
            let name = svc["name"].as_str().unwrap_or("");
            let state = svc["state"].as_str().unwrap_or("");
            if name == "docker" {
                // An observer that could not see the containers says nothing about them.
                if matches!(state, "unavailable" | "unconfigured") {
                    continue;
                }
                for item in svc["items"].as_array().into_iter().flatten() {
                    let it = item["name"].as_str().unwrap_or("").trim_start_matches('/');
                    if it.is_empty() || endoras_own(it, "") {
                        continue;
                    }
                    let now_seen = container(
                        item["state"].as_str().unwrap_or(""),
                        item["health"].as_str().unwrap_or(""),
                    );
                    self.note(&format!("container:{it}"), it, now_seen, now, &mut alarms);
                }
                continue;
            }
            if name.is_empty() || endoras_own(name, "") {
                continue;
            }
            self.note(
                &format!("service:{name}"),
                name,
                service(state),
                now,
                &mut alarms,
            );
        }
        alarms
    }

    fn note(&mut self, key: &str, name: &str, now_seen: Seen, now: i64, out: &mut Vec<Alarm>) {
        let before = self.seen.insert(key.to_owned(), now_seen.clone());
        let Some(Seen::Well) = before else {
            return;
        };
        let Seen::Trouble(became) = now_seen else {
            return;
        };
        if self
            .alarmed
            .get(name)
            .is_some_and(|at| now - at < ALARM_EVERY_MS)
        {
            return;
        }
        self.alarmed.insert(name.to_owned(), now);
        out.push(Alarm {
            name: name.to_owned(),
            became,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lab(containers: &[(&str, &str, &str)], sonarr: &str) -> Value {
        let items: Vec<Value> = containers
            .iter()
            .map(|(n, s, h)| json!({ "name": n, "state": s, "health": h }))
            .collect();
        json!({ "observed_at_ms": 1, "services": [
            { "name": "docker", "state": "healthy", "items": items },
            { "name": "sonarr", "state": sonarr, "items": [] },
            { "name": "radarr", "state": "unconfigured", "items": [] },
        ] })
    }

    const MIN: i64 = 60_000;

    #[test]
    fn only_new_trouble_raises_an_alarm_and_once_a_change() {
        let mut watch = LabWatch::new();
        let start = lab(
            &[
                ("sonarr", "running", "healthy"),
                ("lidarr", "running", "unhealthy"),
            ],
            "healthy",
        );
        assert!(
            watch.observe(&start, 0).is_empty(),
            "what was there at start"
        );
        assert!(watch.observe(&start, MIN).is_empty(), "nothing changed");
        let sick = lab(
            &[
                ("sonarr", "running", "unhealthy"),
                ("lidarr", "running", "unhealthy"),
            ],
            "healthy",
        );
        let alarms = watch.observe(&sick, 2 * MIN);
        assert_eq!(
            alarms,
            [Alarm {
                name: "sonarr".to_owned(),
                became: "unhealthy".to_owned()
            }]
        );
        assert_eq!(alarms[0].said("14:02"), "sonarr became unhealthy at 14:02");
        assert!(
            watch.observe(&sick, 3 * MIN).is_empty(),
            "not every 30 seconds"
        );
    }

    #[test]
    fn a_container_alarms_at_most_once_an_hour() {
        let mut watch = LabWatch::new();
        let well = lab(&[("sonarr", "running", "healthy")], "healthy");
        let down = lab(&[("sonarr", "exited", "unknown")], "healthy");
        watch.observe(&well, 0);
        assert_eq!(watch.observe(&down, MIN)[0].became, "exited");
        watch.observe(&well, 2 * MIN);
        assert!(watch.observe(&down, 3 * MIN).is_empty(), "within the hour");
        watch.observe(&well, 4 * MIN);
        let sonarr_api = lab(&[("sonarr", "running", "healthy")], "unavailable");
        assert!(
            watch.observe(&sonarr_api, 5 * MIN).is_empty(),
            "its service shares its hour"
        );
        watch.observe(&well, 6 * MIN);
        assert_eq!(watch.observe(&down, MIN + ALARM_EVERY_MS).len(), 1);
    }

    #[test]
    fn endoras_own_containers_and_an_unseen_lab_never_alarm() {
        let mut watch = LabWatch::new();
        watch.observe(
            &lab(
                &[
                    ("endora-node", "running", "healthy"),
                    ("radarr", "running", "unknown"),
                ],
                "healthy",
            ),
            0,
        );
        let blind =
            json!({ "services": [{ "name": "docker", "state": "unavailable", "items": [] }] });
        assert!(watch.observe(&blind, MIN).is_empty());
        let alarms = watch.observe(
            &lab(
                &[
                    ("endora-node", "dead", "unknown"),
                    ("radarr", "restarting", "unknown"),
                ],
                "unavailable",
            ),
            2 * MIN,
        );
        let said: Vec<String> = alarms.iter().map(|a| a.said("09:00")).collect();
        assert_eq!(
            said,
            [
                "radarr became restarting at 09:00",
                "sonarr became unavailable at 09:00"
            ]
        );
    }
}
