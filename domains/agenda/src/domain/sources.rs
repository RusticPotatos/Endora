//! How what the readers saw becomes signals. A container is only Endora's business once it has
//! been seen running: one already stopped when the loop starts was most likely stopped on
//! purpose, and so is one that stops cleanly.

use super::{Kind, Signal, Source};
use std::collections::BTreeSet;

/// A container as the home-lab observer reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// Docker's id: what a restart acts on.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Docker's state: running, exited, restarting, dead, paused, created.
    pub state: String,
    /// Its health check: healthy, unhealthy, starting or unknown.
    pub health: String,
    /// For an exited container, its exit code when Docker said it.
    pub exit: Option<i32>,
}

/// The exit codes a deliberate stop leaves: a clean exit, and the signals `docker stop` sends.
const DELIBERATE: [i32; 3] = [0, 137, 143];

/// The containers' signals. `seen_running` remembers every container seen running since the
/// loop started; it is how a crash is told from a container that was never on.
pub fn container_signals(seen_running: &mut BTreeSet<String>, list: &[Container]) -> Vec<Signal> {
    let mut signals = Vec::new();
    for c in list {
        let signal = |kind: Kind, detail: String| Signal {
            kind,
            subject: c.name.clone(),
            about: c.name.clone(),
            name: c.name.clone(),
            detail,
            target: Some(c.id.clone()),
            cautious: false,
            at: None,
        };
        match c.state.as_str() {
            "running" => {
                seen_running.insert(c.name.clone());
                if c.health == "unhealthy" {
                    signals.push(signal(Kind::ContainerUnhealthy, String::new()));
                }
            }
            "restarting" => signals.push(signal(Kind::ContainerLooping, String::new())),
            "exited" | "dead" if seen_running.contains(&c.name) => {
                // Not knowing how it ended is treated as deliberate: a restart is the one thing
                // the loop does unasked, so it needs to be sure.
                let crashed = c.state == "dead" || c.exit.is_some_and(|e| !DELIBERATE.contains(&e));
                let detail = c
                    .exit
                    .map_or_else(|| c.state.clone(), |code| format!("exit {code}"));
                let kind = if crashed {
                    Kind::ContainerCrashed
                } else {
                    Kind::ContainerStopped
                };
                signals.push(signal(kind, detail));
            }
            _ => {}
        }
    }
    signals
}

/// A download in Sonarr's or Radarr's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedDownload {
    /// What it is.
    pub title: String,
    /// Whether the queue flags it (a warning or an error).
    pub stuck: bool,
}

/// The stuck downloads' signals.
#[must_use]
pub fn download_signals(source: Source, queue: &[QueuedDownload]) -> Vec<Signal> {
    let service = match source {
        Source::Radarr => "Radarr",
        _ => "Sonarr",
    };
    queue
        .iter()
        .filter(|d| d.stuck)
        .map(|d| Signal {
            kind: Kind::DownloadStuck,
            subject: d.title.clone(),
            about: d.title.clone(),
            name: d.title.clone(),
            detail: service.to_owned(),
            target: None,
            cautious: false,
            at: None,
        })
        .collect()
}

/// Today's and tomorrow's calendar entries' signals, from `(day, title)` pairs. An entry
/// keeps its identity from tomorrow to today, so it is one item that changes its words.
#[must_use]
pub fn event_signals(entries: &[(String, String)], today: &str, tomorrow: &str) -> Vec<Signal> {
    entries
        .iter()
        .filter_map(|(day, title)| {
            let when = if day == today {
                "Today"
            } else if day == tomorrow {
                "Tomorrow"
            } else {
                return None;
            };
            Some(Signal {
                kind: Kind::Event,
                subject: format!("{day} {title}"),
                about: title.clone(),
                name: title.clone(),
                detail: when.to_owned(),
                target: None,
                cautious: false,
                at: None,
            })
        })
        .collect()
}

/// A firing alert, as Prometheus reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    /// The rule's name: `FilesystemFull`.
    pub name: String,
    /// `critical` or `warning`.
    pub severity: String,
    /// What it is about, from its labels: a drive, a path, a plan.
    pub target: String,
    /// Its summary, as the rule words it.
    pub summary: String,
}

/// The alerts the containers already speak for: the Docker source tells those, once.
const TOLD_BY_DOCKER: [&str; 2] = ["ContainerRestartLoop", "ContainerUnhealthy"];

/// The firing alerts' signals. The rule's name decides the kind: a full disk, a backup that
/// has not succeeded, a failing drive; anything else by its severity.
#[must_use]
pub fn alert_signals(alerts: &[Alert]) -> Vec<Signal> {
    alerts
        .iter()
        .filter(|a| !TOLD_BY_DOCKER.contains(&a.name.as_str()))
        .map(|a| {
            let kind = match a.name.as_str() {
                "FilesystemFull" => Kind::DiskFull,
                "FilesystemAlmostFull" => Kind::DiskAlmostFull,
                "BackupStale" | "BackupNeverSucceeded" | "DatabaseDumpStale" => Kind::BackupFailing,
                "DiskFailingSmart" | "DiskBadSectors" => Kind::DriveFailing,
                _ if a.severity == "critical" => Kind::AlertCritical,
                _ => Kind::AlertWarning,
            };
            let about = if a.target.is_empty() {
                a.name.clone()
            } else {
                format!("{} on {}", a.name, a.target)
            };
            let detail = if a.summary.trim().is_empty() {
                about.clone()
            } else {
                a.summary.trim().to_owned()
            };
            Signal {
                kind,
                subject: about.clone(),
                about,
                name: a.name.clone(),
                detail,
                target: None,
                cautious: false,
                at: None,
            }
        })
        .collect()
}

/// The failing uptime checks' signals, by the check's name.
#[must_use]
pub fn check_signals(failing: &[String]) -> Vec<Signal> {
    failing
        .iter()
        .map(|name| Signal {
            kind: Kind::CheckFailing,
            subject: name.clone(),
            about: format!("the uptime check {name}"),
            name: name.clone(),
            detail: String::new(),
            target: None,
            cautious: false,
            at: None,
        })
        .collect()
}

/// Endora's nightly copy: a signal while its last attempt failed, none once one succeeds.
#[must_use]
pub fn nightly_copy_signals(last_failure: Option<&str>) -> Vec<Signal> {
    last_failure
        .map(|why| Signal {
            kind: Kind::NightlyCopyFailed,
            subject: "nightly".to_owned(),
            about: "nightly".to_owned(),
            name: "Endora".to_owned(),
            detail: why.to_owned(),
            target: None,
            cautious: false,
            at: None,
        })
        .into_iter()
        .collect()
}
