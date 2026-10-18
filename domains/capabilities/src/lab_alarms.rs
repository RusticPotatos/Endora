//! The home lab's alarms (ADR 0107): when a container or a service goes from well to
//! trouble, Endora tells HQ, whose routine for the home lab hands its member the work. Only
//! new trouble: what was already wrong when Endora first saw it, and Endora's own containers,
//! never raise one, and a container raises at most one an hour.
//!
//! Coming back is news too: an alarm is a moment, and a container that alarmed and is running
//! and healthy again (a service that answers again) is told to HQ once, as long as its alarm
//! was within the day, so the member on the alarm's task reads that it recovered rather than
//! diagnosing what has already passed.

use std::collections::HashMap;

use serde_json::Value;

use crate::contracts::endoras_own;

/// At most one alarm a container (or service) this often.
pub const ALARM_EVERY_MS: i64 = 60 * 60 * 1000;

/// How long after its alarm a container's (or service's) coming back is still news.
pub const RECOVERY_WITHIN_MS: i64 = 24 * 60 * 60 * 1000;

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

/// Something that went from well to trouble, or, `recovered`, came back from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alarm {
    /// The container or service.
    pub name: String,
    /// What it became.
    pub became: String,
    /// Whether it is back up after an alarm, rather than in trouble.
    pub recovered: bool,
}

impl Alarm {
    /// The alarm as HQ is told it: a short factual line, at the owner's time of day.
    #[must_use]
    pub fn said(&self, hh_mm: &str) -> String {
        if self.recovered {
            return format!("{} is back up at {hh_mm}", self.name);
        }
        format!("{} became {} at {hh_mm}", self.name, self.became)
    }
}

/// What Endora has seen of the home lab since it started, in memory: a restart begins again,
/// taking what it then sees as how things are.
#[derive(Debug, Default)]
pub struct LabWatch {
    seen: HashMap<String, Seen>,
    alarmed: HashMap<String, i64>,
    /// What HQ was told is in trouble and has not yet been told is back, and when.
    unrecovered: HashMap<String, i64>,
}

impl LabWatch {
    /// A watch that has seen nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one snapshot of the observer's and returns what went from well to trouble since
    /// the last: new trouble only, never Endora's own, at most one an hour a name. Beside it,
    /// what alarmed within the day and is well again, running and no longer starting: once an
    /// alarm.
    pub fn observe(&mut self, snapshot: &Value, now: i64) -> Vec<Alarm> {
        self.unrecovered
            .retain(|_, at| now - *at < RECOVERY_WITHIN_MS);
        let mut alarms = Vec::new();
        for svc in snapshot["services"].as_array().into_iter().flatten() {
            let name = svc["name"].as_str().unwrap_or("");
            let state = svc["state"].as_str().unwrap_or("");
            if name == "docker" {
                // An observer that could not see the containers, or could not tell in time,
                // says nothing about them.
                if matches!(state, "unavailable" | "unconfigured" | "unverified") {
                    continue;
                }
                for item in svc["items"].as_array().into_iter().flatten() {
                    let it = item["name"].as_str().unwrap_or("").trim_start_matches('/');
                    if it.is_empty() || endoras_own(it, "") {
                        continue;
                    }
                    let health = item["health"].as_str().unwrap_or("");
                    let now_seen = container(item["state"].as_str().unwrap_or(""), health);
                    // Running but its health check not yet passed is not yet back.
                    let settled = health != "starting";
                    self.note(
                        &format!("container:{it}"),
                        it,
                        (now_seen, settled),
                        now,
                        &mut alarms,
                    );
                }
                continue;
            }
            if name.is_empty() || endoras_own(name, "") {
                continue;
            }
            // A read that timed out tells nothing (ADR 0124): what was last seen stands.
            if state == "unverified" {
                continue;
            }
            self.note(
                &format!("service:{name}"),
                name,
                (service(state), true),
                now,
                &mut alarms,
            );
        }
        alarms
    }

    /// Notes how `key` (`name`) is seen now, and whether it has settled there, and adds what
    /// is news to `out`.
    fn note(
        &mut self,
        key: &str,
        name: &str,
        (now_seen, settled): (Seen, bool),
        now: i64,
        out: &mut Vec<Alarm>,
    ) {
        let before = self.seen.insert(key.to_owned(), now_seen.clone());
        if now_seen == Seen::Well && settled && self.unrecovered.remove(key).is_some() {
            out.push(Alarm {
                name: name.to_owned(),
                became: "running".to_owned(),
                recovered: true,
            });
            return;
        }
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
        self.unrecovered.insert(key.to_owned(), now);
        out.push(Alarm {
            name: name.to_owned(),
            became,
            recovered: false,
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
                became: "unhealthy".to_owned(),
                recovered: false,
            }]
        );
        assert_eq!(alarms[0].said("14:02"), "sonarr became unhealthy at 14:02");
        assert!(
            watch.observe(&sick, 3 * MIN).is_empty(),
            "not every 30 seconds"
        );
    }

    /// A read that timed out is unverified (ADR 0124): never an alarm, and it changes nothing
    /// HQ believes, so the real refusal after it still alarms, and a recovery after it is news.
    #[test]
    fn an_unverified_reading_raises_nothing_and_changes_nothing() {
        let mut watch = LabWatch::new();
        let well = lab(&[("sonarr", "running", "healthy")], "healthy");
        watch.observe(&well, 0);
        let slow = lab(&[("sonarr", "running", "healthy")], "unverified");
        assert!(
            watch.observe(&slow, MIN).is_empty(),
            "a timeout is not down"
        );
        let blind =
            json!({ "services": [{ "name": "docker", "state": "unverified", "items": [] }] });
        assert!(watch.observe(&blind, 2 * MIN).is_empty());
        let down = lab(&[("sonarr", "running", "healthy")], "unavailable");
        assert_eq!(
            watch.observe(&down, 3 * MIN).len(),
            1,
            "well, then refused: an alarm"
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

    /// The lab of 2026-10-17: deluge, nzbget and prowlarr in a VPN container's network.
    fn vpn(state: &str, health: &str) -> Value {
        lab(
            &[
                ("deluge", state, health),
                ("nzbget", state, health),
                ("prowlarr", state, health),
                ("sonarr", "running", "healthy"),
            ],
            "healthy",
        )
    }

    fn said(alarms: &[Alarm]) -> Vec<String> {
        alarms.iter().map(|a| a.said("13:43")).collect()
    }

    #[test]
    fn what_alarmed_and_came_back_up_is_news_once() {
        let mut watch = LabWatch::new();
        watch.observe(&vpn("running", "unknown"), 0);
        let down = watch.observe(&vpn("exited", "unknown"), MIN);
        assert_eq!(down.len(), 3);
        assert!(down.iter().all(|a| !a.recovered));
        assert!(
            watch
                .observe(&vpn("running", "starting"), 2 * MIN)
                .is_empty(),
            "running, but not yet healthy: not yet back"
        );
        let back = watch.observe(&vpn("running", "healthy"), 3 * MIN);
        assert!(back.iter().all(|a| a.recovered));
        assert_eq!(
            said(&back),
            [
                "deluge is back up at 13:43",
                "nzbget is back up at 13:43",
                "prowlarr is back up at 13:43"
            ]
        );
        assert!(
            watch
                .observe(&vpn("running", "healthy"), 4 * MIN)
                .is_empty(),
            "said once"
        );
        // Down again within the hour raises no alarm, so its coming back is no news either.
        assert!(watch.observe(&vpn("exited", "unknown"), 5 * MIN).is_empty());
        assert!(
            watch
                .observe(&vpn("running", "unknown"), 6 * MIN)
                .is_empty()
        );
    }

    #[test]
    fn only_what_alarmed_in_the_last_day_comes_back_as_news() {
        let mut watch = LabWatch::new();
        // Down when Endora first looked: no alarm, so no news when it comes up.
        watch.observe(&vpn("exited", "unknown"), 0);
        assert!(watch.observe(&vpn("running", "unknown"), MIN).is_empty());
        // An alarm a day and more ago is old news: its recovery is not told.
        assert_eq!(watch.observe(&vpn("exited", "unknown"), 2 * MIN).len(), 3);
        assert!(
            watch
                .observe(&vpn("running", "unknown"), 3 * MIN + RECOVERY_WITHIN_MS)
                .is_empty()
        );
        // A service that answers again is back too.
        let mut watch = LabWatch::new();
        watch.observe(&lab(&[], "healthy"), 0);
        assert_eq!(watch.observe(&lab(&[], "unavailable"), MIN).len(), 1);
        let back = watch.observe(&lab(&[], "degraded"), 2 * MIN);
        assert_eq!(said(&back), ["sonarr is back up at 13:43"]);
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
