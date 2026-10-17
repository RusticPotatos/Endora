//! The handheld's configured services, readable from chat.
//!
//! ADR 0093 gave the handheld console its own entries — "Docker", "Prometheus", now under
//! Settings → Connections → Home lab — so an address and a key could be entered once, in the
//! same place as every other integration. Those entries are settings-only on purpose: they are
//! never `configured`, so the model is never offered them. That kept the console's privileged
//! operations away from the model, and it also meant that a service the person had already set
//! up was invisible to the one place they would naturally ask about it.
//!
//! These skills close that gap for the **reads**, and only the reads. Each one *borrows* the
//! settings of the handheld entry it speaks for (`borrows_from`, the same seam the mail skill
//! uses to ride Home Assistant's connection), so there is nothing to configure twice: the
//! moment the entry is set up, the question can be asked in chat; until then the skill is not
//! offered. Mutations — container controls, media requests, jobs — stay on the handheld's
//! explicit, confirmed path, because a model that can restart containers on its own is a
//! different decision and was not the one made here. Sonarr's and Radarr's queues, and the
//! two actions on a stuck download the owner did decide on, are in [`super::arr`] (ADR 0108).
//!
//! What keeps this safe is structure, not instruction:
//!
//! - **The model chooses nothing.** No skill takes input, so there is no address, path or
//!   parameter for it to supply; the address read is the one the person configured. Each read
//!   uses the same hardened fetch as the Overview page: a validated `http(s)` origin, short
//!   timeout, no redirects, no proxy, a size cap, the key only ever in a header, and failures
//!   reported as a category — never what the service said, and never the key.
//! - **Projected, not passed through.** Container lists go through the Overview's own
//!   [`normalize`]: names, states, health. Ids, labels, environment and anything
//!   else the service returned are dropped before the model sees the result.
//! - **No logs, no search.** Container logs and media search are deliberately absent.
//! - **The turn stays on the device.** Every skill is `third_party`, so a turn that reads one
//!   is marked, may answer but may not act, and may not be escalated to the deep model (ADR
//!   0064/0070). It is a choice: container names and
//!   NAS figures describe the person's own infrastructure and should not leave it. Disguising
//!   them on the way out would be the alternative, but the escalation door disguises only the
//!   person's *messages* today, not what a skill returned, so there is nothing to hang it on.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::homelab::{normalize, validate_origin};
use super::terminal::metric_queries;
use super::{Capability, CapabilityError, CapabilityInfo, CapabilitySettings, Reversibility};

/// What a skill reads, and so how it asks and how it speaks.
#[derive(Clone, Copy)]
enum Source {
    /// A Docker-compatible container list, through the read-only address on the Overview page.
    Docker,
    /// The same container list, through a Portainer environment.
    Portainer,
    /// Prometheus figures about the NAS.
    Nas,
}

/// One chat-facing read over a handheld settings entry.
pub struct HomelabChatSkill {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    /// The handheld entry whose settings this reads (`borrows_from`).
    lender: &'static str,
    /// Where the person set it up, for the "not set up" message.
    needs: &'static str,
    source: Source,
}

/// The skills, registered beside the handheld's settings-only entries.
#[must_use]
pub fn chat_skills() -> Vec<Arc<dyn Capability>> {
    [
        HomelabChatSkill {
            id: "containers",
            name: "What the containers are doing",
            description: "Which of the house's Docker containers are running, stopped or \
                          unhealthy. Read-only: it cannot start, stop or restart anything, and \
                          it does not read logs.",
            lender: "homelab-docker",
            needs: "set up Docker under Settings → Connections",
            source: Source::Docker,
        },
        HomelabChatSkill {
            id: "portainer_containers",
            name: "What the containers are doing (Portainer)",
            description: "Which containers are running, stopped or unhealthy, as Portainer \
                          sees them. Read-only: it cannot start, stop or restart anything, and \
                          it does not read logs.",
            lender: "terminal-portainer",
            needs: "set up Portainer under Settings → Connections",
            source: Source::Portainer,
        },
        HomelabChatSkill {
            id: "nas_status",
            name: "How the NAS is doing",
            description: "Current figures for the NAS — load, memory, disk and the like — \
                          from Prometheus. Read-only.",
            lender: "terminal-metrics",
            needs: "set up Prometheus under Settings → Connections",
            source: Source::Nas,
        },
    ]
    .into_iter()
    .map(|skill| Arc::new(skill) as Arc<dyn Capability>)
    .collect()
}

impl Capability for HomelabChatSkill {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: self.id,
            name: self.name,
            description: self.description,
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            // Ready in principle. Whether it is usable is decided by the entry it borrows
            // from: the registry counts a borrower as configured exactly when its lender is.
            configured: true,
            needs: self.needs,
            // No settings of its own: nothing to configure twice.
            settings: &[],
            wants_place: false,
            borrows_from: Some(self.lender),
            // See the module comment: marks the turn, which is what keeps it on the device.
            third_party: true,
            // No input. The model has nothing to supply, and so nothing to misuse.
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        _input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let unavailable = |why: String| CapabilityError::Unavailable(why);
        let get = |key: &str| settings.get(key).map_or("", |v| v.trim());

        let origin = get("origin").trim_end_matches('/');
        if origin.is_empty() {
            return Err(unavailable(format!(
                "{} is not set up yet — {}",
                self.name, self.needs
            )));
        }
        validate_origin(origin).map_err(|_| {
            unavailable(format!(
                "{}: the address must look like http://host:port, with nothing after it",
                self.name
            ))
        })?;
        let keyed = |what: &str| {
            let key = get("api_key");
            if key.is_empty() {
                return Err(unavailable(format!(
                    "{} is not set up yet — add its key ({what}) in settings",
                    self.name
                )));
            }
            Ok(key)
        };
        let failed =
            |category: &'static str| unavailable(format!("{}: {}", self.name, phrase(category)));

        match self.source {
            Source::Docker => {
                let list = get_json(origin, "/containers/json?all=true", None, 1024 * 1024)
                    .map_err(failed)?;
                normalize("docker", &list).map_err(failed)
            }
            Source::Portainer => {
                let key = keyed("Portainer API key")?;
                let environment = get("endpoint");
                if environment.is_empty()
                    || environment.len() > 9
                    || !environment.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(unavailable(format!(
                        "{} is not set up yet — {}",
                        self.name, self.needs
                    )));
                }
                let path = format!("/api/endpoints/{environment}/docker/containers/json?all=true");
                let list = get_json(origin, &path, Some(key), 1024 * 1024).map_err(failed)?;
                normalize("docker", &list).map_err(failed)
            }
            Source::Nas => {
                let instance = get("instance");
                if instance.is_empty() {
                    return Err(unavailable(format!(
                        "{} is not set up yet — {}",
                        self.name, self.needs
                    )));
                }
                let queries = metric_queries(instance).map_err(failed)?;
                let items: Vec<Value> = queries
                    .into_iter()
                    .map(|(label, query)| {
                        let path = format!("/api/v1/query?query={}", percent_encode(&query));
                        let detail = get_json(origin, &path, None, 256 * 1024)
                            .ok()
                            .and_then(|v| {
                                v["data"]["result"][0]["value"][1]
                                    .as_str()
                                    .map(str::to_owned)
                            })
                            .unwrap_or_else(|| "unavailable".to_owned());
                        json!({"label": label, "detail": detail})
                    })
                    .collect();
                Ok(json!({"summary": "NAS figures", "items": items}))
            }
        }
    }

    fn summarize(&self, output: &Value) -> String {
        let items = output["items"].as_array().map_or(&[][..], Vec::as_slice);
        let field = |item: &Value, key: &str| item[key].as_str().unwrap_or("").to_owned();
        match self.source {
            Source::Docker | Source::Portainer => {
                let needs_attention =
                    |item: &&Value| item["state"] != "running" || item["health"] == "unhealthy";
                let trouble = items.iter().filter(needs_attention).count();
                let mut lines = vec![if trouble == 0 {
                    format!("{} containers, all running and healthy.", items.len())
                } else {
                    format!("{} containers; {trouble} need attention.", items.len())
                }];
                // The ones that need attention first, so they are not buried in a long list.
                let (bad, good): (Vec<&Value>, Vec<&Value>) =
                    items.iter().partition(needs_attention);
                for item in bad.into_iter().chain(good) {
                    let health = field(item, "health");
                    let extra = if health == "unhealthy" || health == "starting" {
                        format!(", {health}")
                    } else {
                        String::new()
                    };
                    lines.push(format!(
                        "- {}: {}{extra}",
                        field(item, "name"),
                        field(item, "state")
                    ));
                }
                lines.join("\n")
            }
            Source::Nas => {
                let mut lines = vec!["NAS figures:".to_owned()];
                for item in items {
                    lines.push(format!(
                        "- {}: {}",
                        field(item, "label"),
                        field(item, "detail")
                    ));
                }
                lines.join("\n")
            }
        }
    }
}

/// A category, in words a person can act on. Never what the service said, and never the key.
pub(super) fn phrase(category: &str) -> &'static str {
    match category {
        "authentication" => "the key was refused — check it in settings",
        "connection" => "could not connect — is it running, and is the address right?",
        "upstream" => "the service answered with an error",
        "protocol" => "the answer was not in the form expected",
        _ => "the settings are not valid",
    }
}

/// One bounded, authenticated-if-keyed GET. Mirrors the Overview reader's settings: no
/// redirects (a configured address must not become a pivot to somewhere else), no proxy, a
/// short timeout, and a cap on how much of an answer is read. The key goes in a header and
/// nowhere else.
pub(super) fn get_json(
    origin: &str,
    path: &str,
    key: Option<&str>,
    limit: u64,
) -> Result<Value, &'static str> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .max_redirects(0)
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent.get(format!("{origin}{path}"));
    if let Some(key) = key {
        request = request.header("X-Api-Key", key);
    }
    let mut response = request.call().map_err(|_| "connection")?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err("authentication"),
        _ => return Err("upstream"),
    }
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_json()
        .map_err(|_| "protocol")
}

/// Percent-encodes a query value: unreserved characters pass, everything else becomes `%XX`.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::percent_encode;

    #[test]
    fn a_query_is_encoded_so_it_cannot_break_out_of_its_parameter() {
        assert_eq!(percent_encode("a-b_c.d~E9"), "a-b_c.d~E9");
        assert_eq!(
            percent_encode("up{host=\"x\"} & 1"),
            "up%7Bhost%3D%22x%22%7D%20%26%201"
        );
        assert!(!percent_encode("a&b=c#d").contains(['&', '=', '#']));
    }
}
