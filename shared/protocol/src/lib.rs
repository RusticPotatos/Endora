//! Endora's wire protocol (ADR 0099): the shapes of the HTTP responses its clients read.
//!
//! The node and its clients spoke untyped JSON: the server built objects field by field and the
//! handheld read fields by name, so a renamed field broke the handheld while both sides' checks
//! stayed green. It happened: the handheld waited for a `progress` event the server never sends,
//! and never showed the `step` and `error` events it does.
//!
//! This crate is the one place those shapes are written down. The server checks what it is
//! about to send against these types, and its tests prove each response fits. [`samples`] holds
//! one canonical value of each, which the clients' tests render, so a change here fails on the
//! side that did not follow. Serde only: no server or client code.

use serde::{Deserialize, Serialize};

/// A handheld page: a line of summary and a list of items (`GET /v1/terminal/{view}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleView {
    /// One line about the page.
    pub summary: String,
    /// What is on it.
    #[serde(default)]
    pub items: Vec<ConsoleItem>,
}

/// One row on a handheld page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleItem {
    /// What an action on the row names (an entity, a container, a title), if it can be acted on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the row is called.
    pub label: String,
    /// A line about it.
    #[serde(default)]
    pub detail: String,
    /// For media: `movie` or `tv`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// For the home catalog: whether it is a favorite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    /// For containers: whether it keeps the handheld's own path open (Endora, its tunnel), and
    /// so is never stopped from the handheld.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected: Option<bool>,
    /// For jobs: the latest run, whose output the handheld opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_id: Option<String>,
}

/// The handheld's home screen (`GET /v1/terminal/dashboard`): Homepage's groups with the live
/// state of each service, NAS figures and firing alerts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dashboard {
    /// NAS figures, one short line each.
    #[serde(default)]
    pub stats: Vec<String>,
    /// Firing alerts.
    #[serde(default)]
    pub alerts: Vec<serde_json::Value>,
    /// Homepage's groups.
    #[serde(default)]
    pub groups: Vec<DashboardGroup>,
}

/// One Homepage group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardGroup {
    /// The group's name.
    pub name: String,
    /// Its services.
    #[serde(default)]
    pub services: Vec<DashboardService>,
}

/// One service on the home screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardService {
    /// Its name.
    pub name: String,
    /// `running`, `stopped`, `unhealthy` or another container state.
    pub state: String,
    /// The container it runs as, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A few figures from its Homepage widget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What became of a handheld action (`POST /v1/terminal/action`, `GET
/// /v1/terminal/operations/{id}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// The id the client sent, which deduplicates a retry.
    pub request_id: String,
    /// `submitted`, `verified`, `uncertain` and the like.
    pub state: String,
    /// One line for the person.
    pub summary: String,
    /// What was read back afterwards, when something was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<serde_json::Value>,
    /// A safe category when the upstream did not confirm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Where a connection's sign-in stands (`GET /v1/connections/spotify`), the same for every
/// device: one started on the handheld shows as waiting on the phone, and the other way round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionStatus {
    /// `not_set_up`, `waiting` or `connected`.
    pub state: String,
    /// One line for the person.
    pub detail: String,
    /// Whether the app's client id is saved.
    #[serde(default)]
    pub client_id_set: bool,
    /// While waiting: the address to open to approve it.
    #[serde(default)]
    pub sign_in_url: Option<String>,
    /// While waiting: where the provider sends the person back.
    #[serde(default)]
    pub redirect: Option<String>,
    /// While waiting: when it was started.
    #[serde(default)]
    pub started_ms: Option<i64>,
    /// When connected: whose account.
    #[serde(default)]
    pub account: Option<String>,
}

/// An access token lent by the node (`POST /v1/connections/spotify/token`). The node keeps the
/// sign-in; a device borrows this and asks again when it runs out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LentToken {
    /// The token, sent to the provider as a bearer token.
    pub access_token: String,
    /// Seconds it has left.
    pub expires_in: i64,
}

/// One event of a streamed chat reply (`POST /v1/chat/stream`, Server-Sent Events).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamEvent {
    /// The next piece of the reply's prose.
    Token {
        /// The text.
        text: String,
    },
    /// A skill running or finished, or the model thinking.
    Step {
        /// What it is doing, for the person.
        #[serde(default)]
        label: String,
        /// `running`, `ok`, `failed` and the like.
        #[serde(default)]
        status: String,
    },
    /// The reply is complete and saved.
    Done {
        /// The saved reply.
        reply: Reply,
        /// What the reply did, for the activity line.
        #[serde(default)]
        actions: serde_json::Value,
    },
    /// The exchange failed.
    Error {
        /// Why, for the person.
        message: String,
    },
    /// An event this version does not know; ignored rather than an error, so the server can add
    /// one without breaking older clients.
    #[serde(other)]
    Unknown,
}

/// A saved reply, as a stream's `done` event carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    /// What Endora said.
    pub text: String,
}

/// One canonical value of each shape, for both sides' tests. Fictional throughout.
pub mod samples {
    use super::{
        ConnectionStatus, ConsoleItem, ConsoleView, Dashboard, DashboardGroup, DashboardService,
        Operation, Reply, StreamEvent,
    };

    /// The home page: two favorites.
    #[must_use]
    pub fn home() -> ConsoleView {
        ConsoleView {
            summary: "Home favorites".to_owned(),
            items: vec![
                ConsoleItem {
                    id: Some("light.example_lamp".to_owned()),
                    label: "Example lamp".to_owned(),
                    detail: "on".to_owned(),
                    media_type: None,
                    favorite: None,
                    protected: None,
                    latest_id: None,
                },
                ConsoleItem {
                    id: Some("sensor.example_temperature".to_owned()),
                    label: "Example temperature".to_owned(),
                    detail: "21.5 °C".to_owned(),
                    media_type: None,
                    favorite: None,
                    protected: None,
                    latest_id: None,
                },
            ],
        }
    }

    /// A media search with one result.
    #[must_use]
    pub fn media() -> ConsoleView {
        ConsoleView {
            summary: "Up/Down to pick, Enter to request".to_owned(),
            items: vec![ConsoleItem {
                id: Some("101".to_owned()),
                label: "Example Film (2024)".to_owned(),
                detail: "movie | Availability: not requested".to_owned(),
                media_type: Some("movie".to_owned()),
                favorite: None,
                protected: None,
                latest_id: None,
            }],
        }
    }

    /// The home screen: one group, one service, one figure.
    #[must_use]
    pub fn dashboard() -> Dashboard {
        Dashboard {
            stats: vec!["CPU 12%".to_owned()],
            alerts: Vec::new(),
            groups: vec![DashboardGroup {
                name: "Media".to_owned(),
                services: vec![DashboardService {
                    name: "Example Server".to_owned(),
                    state: "running".to_owned(),
                    id: Some("example-container".to_owned()),
                    detail: Some("2 streams".to_owned()),
                }],
            }],
        }
    }

    /// The jobs page: one job, run before.
    #[must_use]
    pub fn jobs() -> ConsoleView {
        ConsoleView {
            summary: "Allowlisted Ansible jobs".to_owned(),
            items: vec![ConsoleItem {
                id: Some("check-example".to_owned()),
                label: "Check example".to_owned(),
                detail: "succeeded".to_owned(),
                media_type: None,
                favorite: None,
                protected: None,
                latest_id: Some("run-1".to_owned()),
            }],
        }
    }

    /// An action the upstream confirmed.
    #[must_use]
    pub fn operation() -> Operation {
        Operation {
            request_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            state: "verified".to_owned(),
            summary: "Request submitted".to_owned(),
            observed: None,
            error: None,
        }
    }

    /// A sign-in waiting to be approved.
    #[must_use]
    pub fn connection_waiting() -> ConnectionStatus {
        ConnectionStatus {
            state: "waiting".to_owned(),
            detail: "Waiting for you to approve Endora on Spotify".to_owned(),
            client_id_set: true,
            sign_in_url: Some(
                "https://accounts.spotify.com/authorize?client_id=example".to_owned(),
            ),
            redirect: Some("http://127.0.0.1:8888/callback".to_owned()),
            started_ms: Some(1_792_065_600_000),
            account: None,
        }
    }

    /// One of each chat stream event.
    #[must_use]
    pub fn stream() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Step {
                label: "Checking the weather".to_owned(),
                status: "running".to_owned(),
            },
            StreamEvent::Token {
                text: "It is sunny.".to_owned(),
            },
            StreamEvent::Done {
                reply: Reply {
                    text: "It is sunny.".to_owned(),
                },
                actions: serde_json::Value::Array(Vec::new()),
            },
            StreamEvent::Error {
                message: "The model did not answer.".to_owned(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_event_reads_as_the_server_writes_it_and_an_unknown_one_is_ignored() {
        let token: StreamEvent = serde_json::from_str(r#"{"type":"token","text":"Hi"}"#).unwrap();
        assert_eq!(
            token,
            StreamEvent::Token {
                text: "Hi".to_owned()
            }
        );
        let step: StreamEvent = serde_json::from_str(
            r#"{"type":"step","skill":"weather","status":"running","label":"Weather","output":""}"#,
        )
        .unwrap();
        assert!(matches!(step, StreamEvent::Step { ref label, .. } if label == "Weather"));
        let done: StreamEvent = serde_json::from_str(
            r#"{"type":"done","reply":{"id":"x","role":"butler","text":"Hello","at_ms":1},"activity":[],"actions":[]}"#,
        )
        .unwrap();
        assert!(matches!(done, StreamEvent::Done { ref reply, .. } if reply.text == "Hello"));
        let later: StreamEvent = serde_json::from_str(r#"{"type":"something-new","x":1}"#).unwrap();
        assert_eq!(later, StreamEvent::Unknown);
    }

    #[test]
    fn a_jobs_row_keeps_what_the_handheld_opens_with() {
        // Typing the wire must not drop a field a client uses: the jobs page opens a job's
        // latest output by `latest_id`, and containers are guarded by `protected`.
        let row: ConsoleItem = serde_json::from_str(
            r#"{"id":"apply-x","label":"Apply x","detail":"succeeded","latest_id":"run-9","protected":true}"#,
        )
        .unwrap();
        let back = serde_json::to_value(&row).unwrap();
        assert_eq!(back["latest_id"], "run-9");
        assert_eq!(back["protected"], true);
    }

    #[test]
    fn every_sample_survives_the_wire() {
        let jobs = serde_json::to_value(samples::jobs()).unwrap();
        assert_eq!(
            serde_json::from_value::<ConsoleView>(jobs).unwrap(),
            samples::jobs()
        );
        let home = serde_json::to_value(samples::home()).unwrap();
        assert_eq!(
            serde_json::from_value::<ConsoleView>(home).unwrap(),
            samples::home()
        );
        let dash = serde_json::to_value(samples::dashboard()).unwrap();
        assert_eq!(
            serde_json::from_value::<Dashboard>(dash).unwrap(),
            samples::dashboard()
        );
        let op = serde_json::to_value(samples::operation()).unwrap();
        assert_eq!(
            serde_json::from_value::<Operation>(op).unwrap(),
            samples::operation()
        );
        let waiting = serde_json::to_value(samples::connection_waiting()).unwrap();
        assert_eq!(
            serde_json::from_value::<ConnectionStatus>(waiting).unwrap(),
            samples::connection_waiting()
        );
        for event in samples::stream() {
            let wire = serde_json::to_value(&event).unwrap();
            assert_eq!(serde_json::from_value::<StreamEvent>(wire).unwrap(), event);
        }
    }
}
