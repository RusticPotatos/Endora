//! Sonarr and Radarr: what their download queues hold and why an item is stuck, and two
//! actions on a stuck item (ADR 0108). The butler and the team (through the bridge) reach
//! them the same way, under the owner's stance for each.
//!
//! The apps share one API, so an app is a row of [`ARR_APPS`], not code: its name, the
//! handheld entry whose address and key it rides (`borrows_from`, as the chat reads do), the
//! base of its API, and the ids of its three skills. Another app of the same shape is one more
//! row.
//!
//! - **The queue** (a read): each item's number, title, progress, status, the app's own
//!   tracking state (`importBlocked`, `importPending`, …) and its status messages, which say
//!   why an item is stuck. The download client's ids and anything else the app returned are
//!   left out. Titles are release names from indexers, which anyone can write, so it is marked
//!   as from outside (`third_party`).
//! - **Retry the imports** (an act, on by default): the app's `RefreshMonitoredDownloads`
//!   command, which makes it check its download clients now and import what it can, as it does
//!   on its own every few minutes. It is given nothing, removes nothing and deletes nothing.
//! - **Remove, blocklist and search again** (an act that asks first by default): one queue
//!   item, named by the number the queue read gave it, is taken out of the queue and the
//!   download client, its release is blocklisted, and the app searches for another. A number
//!   the queue does not hold is refused before anything is changed.
//!
//! Every request uses the same hardened fetch as the chat reads: the configured origin only,
//! no redirects, no proxy, a short timeout, a size cap, the key in a header and nowhere else,
//! and failures said as a category, never what the app said.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::homelab::validate_origin;
use super::homelab_chat::{get_json, phrase};
use super::{Capability, CapabilityError, CapabilityInfo, CapabilitySettings, Reversibility};

/// One app of the shared API, as configuration.
#[derive(Debug, Clone, Copy)]
pub struct ArrApp {
    /// Its name, as the owner reads it.
    pub name: &'static str,
    /// The handheld entry whose address and key it rides.
    pub lender: &'static str,
    /// Where the owner sets that entry up, for the "not set up" message.
    pub needs: &'static str,
    /// The base of its API.
    pub api: &'static str,
    /// The id of its queue read.
    pub queue: &'static str,
    /// The id of its retry of the imports.
    pub retry: &'static str,
    /// The id of its remove, blocklist and search again.
    pub remove: &'static str,
}

/// The apps Endora reaches this way.
pub const ARR_APPS: [ArrApp; 2] = [
    ArrApp {
        name: "Sonarr",
        lender: "homelab-sonarr",
        needs: "set up Sonarr under Settings → Connections",
        api: "/api/v3",
        queue: "sonarr_queue",
        retry: "sonarr_retry_import",
        remove: "sonarr_remove_and_search",
    },
    ArrApp {
        name: "Radarr",
        lender: "homelab-radarr",
        needs: "set up Radarr under Settings → Connections",
        api: "/api/v3",
        queue: "radarr_queue",
        retry: "radarr_retry_import",
        remove: "radarr_remove_and_search",
    },
];

/// Whether `id` is one of these apps' skills.
#[must_use]
pub fn is_arr_skill(id: &str) -> bool {
    ARR_APPS
        .iter()
        .any(|a| [a.queue, a.retry, a.remove].contains(&id))
}

/// The most status messages kept for one item, and the most of each.
const MESSAGES_AN_ITEM: usize = 4;
const LONGEST_MESSAGE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Queue,
    Retry,
    Remove,
}

/// One of an app's three skills.
struct ArrSkill {
    app: &'static ArrApp,
    action: Action,
}

/// Every app's three skills, registered beside the chat reads.
#[must_use]
pub fn arr_skills() -> Vec<Arc<dyn Capability>> {
    ARR_APPS
        .iter()
        .flat_map(|app| {
            [Action::Queue, Action::Retry, Action::Remove]
                .map(|action| Arc::new(ArrSkill { app, action }) as Arc<dyn Capability>)
        })
        .collect()
}

impl ArrSkill {
    fn id(&self) -> &'static str {
        match self.action {
            Action::Queue => self.app.queue,
            Action::Retry => self.app.retry,
            Action::Remove => self.app.remove,
        }
    }
}

impl Capability for ArrSkill {
    fn info(&self) -> CapabilityInfo {
        // Static words for each app: the names are few, so each description is written once.
        let (name, description, reversibility, schema) = match self.action {
            Action::Queue => (
                "What it is downloading",
                "The download queue: what is downloading, how far along it is, and for anything \
                 stuck, the app's own words for why (import blocked, no eligible files, …). Each \
                 item has a number, for the actions on it. Read-only.",
                Reversibility::Observe,
                None,
            ),
            Action::Retry => (
                "Retry its imports",
                "Asks the app to check its downloads again now and import what it can, as it \
                 does on its own every few minutes. Takes nothing; removes and deletes nothing. \
                 It cannot clear an item the app has blocked for a reason: read the queue after \
                 a minute to see what is still stuck.",
                // On the device's own services, undone by nothing because it changes nothing
                // the app would not do itself: runs on its own by default.
                Reversibility::Reversible,
                None,
            ),
            Action::Remove => (
                "Remove a stuck download and search again",
                "Takes one item out of the download queue and its download client, blocklists \
                 that release so it is not grabbed again, and searches for another. Name the \
                 item by its number in the queue.",
                // Changes what the app and the download client hold; the blocklist can be
                // undone in the app: asks the owner first by default.
                Reversibility::OutwardReversible,
                Some(
                    r#"{"type":"object","properties":{"item":{"type":"integer","description":"The item's number, as the queue lists it (item 102)."}},"required":["item"]}"#,
                ),
            ),
        };
        CapabilityInfo {
            id: self.id(),
            name,
            description,
            category: "media",
            reaches_external: true,
            reversibility,
            // Ready in principle; usable once the entry it borrows from is set up.
            configured: true,
            needs: self.app.needs,
            settings: &[],
            wants_place: false,
            borrows_from: Some(self.app.lender),
            // A queue's titles and a removed item's title are release names from indexers.
            third_party: self.action != Action::Retry,
            input_schema: schema,
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let app = self.app;
        let unavailable = |why: String| CapabilityError::Unavailable(why);
        let get = |key: &str| settings.get(key).map_or("", |v| v.trim());
        let origin = get("origin").trim_end_matches('/');
        let key = get("api_key");
        if origin.is_empty() || key.is_empty() {
            return Err(unavailable(format!(
                "{} is not set up yet — {}",
                app.name, app.needs
            )));
        }
        validate_origin(origin).map_err(|_| {
            unavailable(format!(
                "{}: the address must look like http://host:port, with nothing after it",
                app.name
            ))
        })?;
        let failed =
            |category: &'static str| unavailable(format!("{}: {}", app.name, phrase(category)));
        let read_queue = || {
            get_json(
                origin,
                &format!("{}/queue?page=1&pageSize=100", app.api),
                Some(key),
                1024 * 1024,
            )
            .map_err(failed)
        };
        match self.action {
            Action::Queue => queue_of(&read_queue()?).map_err(failed),
            Action::Retry => {
                send(
                    "POST",
                    origin,
                    &format!("{}/command", app.api),
                    key,
                    Some(&json!({ "name": "RefreshMonitoredDownloads" })),
                )
                .map_err(failed)?;
                Ok(json!({ "app": app.name, "retried": true }))
            }
            Action::Remove => {
                let item = item_of(input).ok_or_else(|| {
                    CapabilityError::BadInput(
                        "name the item by its number, as the queue lists it".to_owned(),
                    )
                })?;
                let queue = read_queue()?;
                let found = queue["records"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|r| r["id"].as_u64() == Some(item))
                    .ok_or_else(|| {
                        CapabilityError::BadInput(format!(
                            "{}'s queue holds no item {item}: read the queue again",
                            app.name
                        ))
                    })?;
                let title = text(&found["title"], 160);
                send(
                    "DELETE",
                    origin,
                    &format!(
                        "{}/queue/{item}?removeFromClient=true&blocklist=true&skipRedownload=false",
                        app.api
                    ),
                    key,
                    None,
                )
                .map_err(failed)?;
                Ok(json!({ "app": app.name, "removed": title, "item": item }))
            }
        }
    }

    fn summarize(&self, output: &Value) -> String {
        let name = self.app.name;
        match self.action {
            Action::Retry => format!(
                "{name} is checking its downloads again, and imports what it can. Read the queue \
                 in a minute to see what is still stuck."
            ),
            Action::Remove => format!(
                "Removed {} (item {}) from {name}'s queue and its download client, blocklisted \
                 that release, and {name} is searching for another.",
                output["removed"].as_str().unwrap_or("the item"),
                output["item"]
            ),
            Action::Queue => {
                let items = output["items"].as_array().map_or(&[][..], Vec::as_slice);
                if items.is_empty() {
                    return format!("{name}'s download queue is empty.");
                }
                let mut lines = vec![if output["state"] == "degraded" {
                    format!(
                        "{} in {name}'s queue; at least one reports a warning or an error.",
                        output["total"]
                    )
                } else {
                    format!("{} in {name}'s queue.", output["total"])
                }];
                for item in items {
                    let field = |k: &str| item[k].as_str().unwrap_or("").to_owned();
                    let tracked = spaced(&field("tracked"));
                    let status = if tracked.is_empty() || tracked == field("status") {
                        field("status")
                    } else {
                        format!("{}, {tracked}", field("status"))
                    };
                    let mut line = format!(
                        "- {} (item {}): {:.0}% ({status})",
                        field("title"),
                        item["item"],
                        item["progress"].as_f64().unwrap_or(0.0)
                    );
                    let why: Vec<&str> = item["why"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect();
                    if !why.is_empty() {
                        line.push_str(&format!(". Stuck: {}", why.join("; ")));
                    }
                    lines.push(line);
                }
                lines.join("\n")
            }
        }
    }
}

/// The queue as a turn reads it: each item's number, title, progress, status, the app's
/// tracking state, whether it is stuck, and its status messages; nothing else.
fn queue_of(value: &Value) -> Result<Value, &'static str> {
    let records = value["records"].as_array().ok_or("protocol")?;
    let total = value["totalRecords"].as_u64().ok_or("protocol")?;
    let items: Vec<Value> = records
        .iter()
        .take(100)
        .map(|r| {
            let size = r["size"].as_f64().unwrap_or(0.0);
            let left = r["sizeleft"].as_f64().unwrap_or(size);
            let progress = if size > 0.0 {
                ((size - left) / size * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            let stuck = matches!(
                r["trackedDownloadStatus"].as_str(),
                Some("warning" | "error")
            );
            json!({
                "item": r["id"].as_u64(),
                "title": text(&r["title"], 160),
                "progress": progress,
                "status": text(&r["status"], 40),
                "tracked": text(&r["trackedDownloadState"], 40),
                "stuck": stuck,
                "why": why_of(r),
            })
        })
        .collect();
    let degraded = items.iter().any(|i| i["stuck"] == true);
    Ok(
        json!({ "state": if degraded { "degraded" } else { "healthy" }, "total": total, "items": items }),
    )
}

/// An item's status messages, each "title: message", then its error, bounded.
fn why_of(record: &Value) -> Vec<String> {
    let mut why: Vec<String> = record["statusMessages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| {
            let about = text(&m["title"], 120);
            m["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |said| {
                    let said = text(said, LONGEST_MESSAGE);
                    if about.is_empty() {
                        said
                    } else {
                        format!("{about}: {said}")
                    }
                })
                .collect::<Vec<_>>()
        })
        .filter(|s| !s.trim().is_empty() && !s.trim_end().ends_with(':'))
        .collect();
    let error = text(&record["errorMessage"], LONGEST_MESSAGE);
    if !error.is_empty() {
        why.push(error);
    }
    why.truncate(MESSAGES_AN_ITEM);
    why
}

/// Text as a model may read it: no control characters (and so no terminal escapes), held to
/// `most` characters.
fn text(value: &Value, most: usize) -> String {
    let raw = value.as_str().unwrap_or("");
    // An escape sequence's letters are not control characters: drop each sequence whole.
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if !c.is_control() {
            out.push(c);
        }
    }
    out.chars().take(most).collect()
}

/// `importBlocked` as "import blocked".
fn spaced(state: &str) -> String {
    let mut out = String::new();
    for c in state.chars() {
        if c.is_ascii_uppercase() {
            out.push(' ');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The item a call names: a whole number, given as one or as digits.
fn item_of(input: &Value) -> Option<u64> {
    match &input["item"] {
        Value::Number(n) => n.as_u64(),
        Value::String(s)
            if !s.is_empty() && s.len() <= 12 && s.bytes().all(|b| b.is_ascii_digit()) =>
        {
            s.parse().ok()
        }
        _ => None,
    }
    .filter(|n| *n > 0)
}

/// One bounded request that changes something, as [`get_json`] reads: the configured origin
/// only, no redirects, no proxy, a short timeout, the key in a header.
fn send(
    method: &str,
    origin: &str,
    path: &str,
    key: &str,
    body: Option<&Value>,
) -> Result<(), &'static str> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .max_redirects(0)
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("{origin}{path}");
    let response = match (method, body) {
        ("POST", Some(body)) => agent.post(&url).header("X-Api-Key", key).send_json(body),
        ("DELETE", _) => agent.delete(&url).header("X-Api-Key", key).call(),
        _ => return Err("configuration"),
    }
    .map_err(|_| "connection")?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err("authentication"),
        _ => Err("upstream"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_a_whole_number_and_nothing_else() {
        assert_eq!(item_of(&json!({"item": 102})), Some(102));
        assert_eq!(item_of(&json!({"item": "102"})), Some(102));
        for bad in [
            json!({"item": "102/../1"}),
            json!({"item": -1}),
            json!({"item": 0}),
            json!({"item": 1.5}),
            json!({}),
        ] {
            assert_eq!(item_of(&bad), None, "{bad}");
        }
    }

    #[test]
    fn a_state_is_said_in_words() {
        assert_eq!(spaced("importBlocked"), "import blocked");
        assert_eq!(spaced("downloading"), "downloading");
    }
}
