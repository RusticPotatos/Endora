//! endora-sound through the node (ADR 0104). The console talks to the node only; the node
//! signs every request to the hub, passes on only the routes below (never an arbitrary path),
//! and writes the owner's lasting changes (sources, speakers, setups) to Endora's audit log.
//! Moving a level is not written down: a slider is dragged, not decided.
//!
//! Listening is the one thing a browser cannot sign: an `<audio>` element sends no header. So
//! the signed-in console asks for a ticket, and the audio element plays the stream with it. A
//! ticket is good once, for one speaker, for a minute. A long-lived token never goes in a URL.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use endora_infrastructure::{SoundClient, SoundError, SoundMethod};

/// How long a listen ticket is good for.
const TICKET_LIFE: Duration = Duration::from_secs(60);

use crate::auth::LISTEN_PREFIX;

/// The hub, and the tickets handed out for listening to it.
pub struct SoundRemote {
    client: SoundClient,
    tickets: Mutex<HashMap<String, (u32, Instant)>>,
}

impl SoundRemote {
    /// The remote for the hub this client reaches.
    #[must_use]
    pub fn new(client: SoundClient) -> Self {
        Self {
            client,
            tickets: Mutex::new(HashMap::new()),
        }
    }

    /// A new ticket to listen to `speaker`, forgetting any that have run out.
    fn ticket(&self, speaker: u32, id: u128) -> String {
        let ticket = format!("{id:032x}");
        let mut tickets = self
            .tickets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tickets.retain(|_, (_, at)| at.elapsed() < TICKET_LIFE);
        tickets.insert(ticket.clone(), (speaker, Instant::now()));
        ticket
    }

    /// Spends a ticket: true once, for the speaker it was made for, within its life.
    fn spend(&self, ticket: &str, speaker: u32) -> bool {
        let mut tickets = self
            .tickets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match tickets.remove(ticket) {
            Some((for_speaker, at)) => for_speaker == speaker && at.elapsed() < TICKET_LIFE,
            None => false,
        }
    }
}

fn remote(state: &AppState) -> Result<Arc<SoundRemote>, (StatusCode, Json<serde_json::Value>)> {
    state.sound.clone().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "configured": false, "error": "the sound hub is not set up on this node" })),
    ))
}

fn failed(error: SoundError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match &error {
        SoundError::Unreachable(_) => StatusCode::BAD_GATEWAY,
        SoundError::Answered { status, .. } => {
            StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
        }
    };
    (status, Json(json!({ "error": error.to_string() })))
}

type Answer = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;

async fn ask(
    state: &AppState,
    method: SoundMethod,
    path: String,
    body: Option<serde_json::Value>,
) -> Answer {
    let sound = remote(state)?;
    let answer =
        tokio::task::spawn_blocking(move || sound.client.send(method, &path, body.as_ref()))
            .await
            .map_err(|_| failed(SoundError::Unreachable("a worker thread failed".to_owned())))?;
    answer.map(Json).map_err(failed)
}

/// Writes one of the owner's lasting changes to the sound to Endora's audit log, and Activity.
fn record(state: &AppState, what: &str) {
    let now = state.clock.now();
    if let Ok(entry) = endora_application::AuditRecord::new(
        endora_application::AuditId::new(state.ids.new_id()),
        now,
        &format!("Sound: {what}"),
    ) {
        let _ = endora_application::AuditLog::append(state.audit.as_ref(), &entry);
    }
    let _ =
        endora_application::EventLog::record(state.events.as_ref(), now, &format!("Sound: {what}"));
    let _ = state.changes.send(());
}

fn name_of(body: &serde_json::Value) -> String {
    body["name"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(40)
        .collect()
}

/// The sources, the speakers, the wires, the groups they make and who is listening.
pub(super) async fn sound_state(State(state): State<AppState>) -> Answer {
    ask(&state, SoundMethod::Get, "/v1/state".to_owned(), None).await
}

pub(super) async fn sound_add_source(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Answer {
    let name = name_of(&body);
    let answer = ask(
        &state,
        SoundMethod::Post,
        "/v1/sources".to_owned(),
        Some(body),
    )
    .await?;
    record(&state, &format!("you added the source {name}"));
    Ok(answer)
}

pub(super) async fn sound_remove_source(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Answer {
    let answer = ask(
        &state,
        SoundMethod::Delete,
        format!("/v1/sources/{id}"),
        None,
    )
    .await?;
    record(&state, &format!("you removed source {id}"));
    Ok(answer)
}

pub(super) async fn sound_set_all(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Json(body): Json<serde_json::Value>,
) -> Answer {
    ask(
        &state,
        SoundMethod::Put,
        format!("/v1/sources/{id}/level"),
        Some(body),
    )
    .await
}

pub(super) async fn sound_add_speaker(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Answer {
    let name = name_of(&body);
    let answer = ask(
        &state,
        SoundMethod::Post,
        "/v1/speakers".to_owned(),
        Some(body),
    )
    .await?;
    record(&state, &format!("you added the speaker {name}"));
    Ok(answer)
}

pub(super) async fn sound_remove_speaker(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Answer {
    let answer = ask(
        &state,
        SoundMethod::Delete,
        format!("/v1/speakers/{id}"),
        None,
    )
    .await?;
    record(&state, &format!("you removed speaker {id}"));
    Ok(answer)
}

pub(super) async fn sound_set_volume(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Json(body): Json<serde_json::Value>,
) -> Answer {
    ask(
        &state,
        SoundMethod::Put,
        format!("/v1/speakers/{id}/volume"),
        Some(body),
    )
    .await
}

pub(super) async fn sound_wire(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Answer {
    ask(&state, SoundMethod::Put, "/v1/wires".to_owned(), Some(body)).await
}

pub(super) async fn sound_unwire(
    State(state): State<AppState>,
    Path((source, speaker)): Path<(u32, u32)>,
) -> Answer {
    ask(
        &state,
        SoundMethod::Delete,
        format!("/v1/wires/{source}/{speaker}"),
        None,
    )
    .await
}

/// A setup's name, as it may go in the hub's path: what a name may hold, encoded.
fn setup_path(name: &str) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "a name must be 1 to 40 characters" })),
        ));
    }
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(out)
}

pub(super) async fn sound_save_setup(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Answer {
    let name = name_of(&body);
    let answer = ask(
        &state,
        SoundMethod::Post,
        "/v1/setups".to_owned(),
        Some(body),
    )
    .await?;
    record(&state, &format!("you saved the setup {name}"));
    Ok(answer)
}

pub(super) async fn sound_apply_setup(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Answer {
    let path = format!("/v1/setups/{}/apply", setup_path(&name)?);
    let answer = ask(&state, SoundMethod::Post, path, None).await?;
    record(&state, &format!("you played the setup {}", name.trim()));
    Ok(answer)
}

pub(super) async fn sound_remove_setup(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Answer {
    let path = format!("/v1/setups/{}", setup_path(&name)?);
    let answer = ask(&state, SoundMethod::Delete, path, None).await?;
    record(&state, &format!("you forgot the setup {}", name.trim()));
    Ok(answer)
}

/// A ticket to listen to one stream speaker, for an `<audio>` element to play with.
pub(super) async fn sound_listen_ticket(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Answer {
    let sound = remote(&state)?;
    let ticket = sound.ticket(id, state.ids.new_id());
    Ok(Json(
        json!({ "ticket": ticket, "url": format!("{LISTEN_PREFIX}{id}?ticket={ticket}") }),
    ))
}

#[derive(Deserialize)]
pub(super) struct ListenQuery {
    #[serde(default)]
    ticket: String,
}

/// A stream speaker's live MP3, passed on from the hub for as long as it is listened to.
/// Reached without a header, so it answers only a good ticket, spent here.
pub(super) async fn sound_listen(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(q): Query<ListenQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let sound = remote(&state)?;
    if !sound.spend(&q.ticket, id) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "that listen ticket is not good" })),
        ));
    }
    let opening = sound.clone();
    let reader = tokio::task::spawn_blocking(move || opening.client.listen(id))
        .await
        .map_err(|_| failed(SoundError::Unreachable("a worker thread failed".to_owned())))?
        .map_err(failed)?;
    // The hub's stream is read on a thread of its own and handed on as it comes; when the
    // listener goes, the next send fails and the thread lets the hub's stream go.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(64);
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if tx
                        .blocking_send(Ok(axum::body::Bytes::copy_from_slice(&buf[..n])))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    });
    let body = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|chunk| (chunk, rx))
    });
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "audio/mpeg"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        axum::body::Body::from_stream(body),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::tests::{get, json_body, post, test_state, unsigned};
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt as _;
    use std::io::{Read as _, Write as _};
    use tower::ServiceExt as _;

    /// A hub that answers every request with `status`, `kind` and `body`, and hands back the
    /// requests it was sent.
    fn a_hub(
        status: &'static str,
        kind: &'static str,
        body: &'static [u8],
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (seen, requests) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { continue };
                // The whole request, body and all, before answering: closing early would
                // look to the node like a hub that hung up.
                let mut request: Vec<u8> = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = socket.read(&mut buf).unwrap_or(0);
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request).into_owned();
                    let Some(end) = text.find("\r\n\r\n") else {
                        if n == 0 {
                            break;
                        }
                        continue;
                    };
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if n == 0 || request.len() >= end + 4 + length {
                        break;
                    }
                }
                let _ = seen.send(String::from_utf8_lossy(&request).into_owned());
                let head = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes());
                let _ = socket.write_all(body);
            }
        });
        (url, requests)
    }

    fn with_hub(url: &str) -> AppState {
        test_state().with_sound(SoundClient::new(
            url,
            "the-hubs-own-token-of-thirty-two-chars",
        ))
    }

    #[tokio::test]
    async fn a_node_with_no_hub_says_so() {
        let res = app(test_state()).oneshot(get("/v1/sound")).await.unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json_body(res).await["configured"], false);
    }

    #[tokio::test]
    async fn the_state_comes_from_the_hub_signed_with_the_hubs_token_not_the_owners() {
        let (url, requests) = a_hub(
            "200 OK",
            "application/json",
            br#"{"senders":4,"groups":[]}"#,
        );
        let res = app(with_hub(&url)).oneshot(get("/v1/sound")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await["senders"], 4);
        let request = requests.recv().unwrap();
        assert!(request.starts_with("GET /v1/state "), "{request}");
        assert!(
            request.to_lowercase().contains("bearer the-hubs-own-token"),
            "{request}"
        );
        assert!(
            !request.contains(crate::api::tests::TEST_TOKEN),
            "the owner's token went to the hub"
        );
    }

    #[tokio::test]
    async fn what_the_hub_refuses_reaches_the_console_as_it_said_it() {
        let (url, _requests) = a_hub(
            "409 Conflict",
            "application/json",
            br#"{"error":"every AirPlay sender is busy with another mix"}"#,
        );
        let req = Request::builder()
            .method("PUT")
            .uri("/v1/sound/wires")
            .header("content-type", "application/json")
            .header(
                "authorization",
                format!("Bearer {}", crate::api::tests::TEST_TOKEN),
            )
            .body(Body::from(r#"{"source":1,"speaker":2,"level":50}"#))
            .unwrap();
        let res = app(with_hub(&url)).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        assert_eq!(
            json_body(res).await["error"],
            "every AirPlay sender is busy with another mix"
        );
    }

    #[tokio::test]
    async fn a_lasting_change_is_written_down_and_a_slider_is_not() {
        let (url, _requests) = a_hub("200 OK", "application/json", br#"{"id":1}"#);
        let state = with_hub(&url);
        app(state.clone())
            .oneshot(post(
                "/v1/sound/speakers",
                r#"{"name":"Kitchen","kind":"airplay"}"#,
            ))
            .await
            .unwrap();
        let req = Request::builder()
            .method("PUT")
            .uri("/v1/sound/speakers/1/volume")
            .header("content-type", "application/json")
            .header(
                "authorization",
                format!("Bearer {}", crate::api::tests::TEST_TOKEN),
            )
            .body(Body::from(r#"{"level":40}"#))
            .unwrap();
        app(state.clone()).oneshot(req).await.unwrap();
        let audit = endora_application::AuditLog::recent(state.audit.as_ref(), 20).unwrap();
        let said: Vec<String> = audit.iter().map(|a| a.summary().to_owned()).collect();
        assert!(
            said.iter()
                .any(|s| s == "Sound: you added the speaker Kitchen"),
            "{said:?}"
        );
        assert!(
            !said.iter().any(|s| s.contains("volume")),
            "a slider was written down: {said:?}"
        );
    }

    #[tokio::test]
    async fn listening_takes_a_ticket_good_once_and_never_the_owners_token() {
        let (url, requests) = a_hub("200 OK", "audio/mpeg", b"ID3\x04\x00mp3 frames");
        let state = with_hub(&url);
        // No ticket, a made-up one, and the owner's own token in its place: all refused.
        for uri in [
            "/v1/sound/listen/2".to_owned(),
            "/v1/sound/listen/2?ticket=made-up".to_owned(),
            format!(
                "/v1/sound/listen/2?ticket={}",
                crate::api::tests::TEST_TOKEN
            ),
        ] {
            let res = app(state.clone()).oneshot(unsigned(&uri)).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
        }
        // A ticket is asked for signed, and only signed.
        let res = app(state.clone())
            .oneshot(unsigned_post("/v1/sound/listen/2/ticket"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let ticket = json_body(
            app(state.clone())
                .oneshot(post("/v1/sound/listen/2/ticket", "{}"))
                .await
                .unwrap(),
        )
        .await;
        let listen = ticket["url"].as_str().unwrap().to_owned();
        assert!(listen.starts_with("/v1/sound/listen/2?ticket="), "{listen}");
        // Not for another speaker.
        let elsewhere = listen.replacen("/listen/2?", "/listen/3?", 1);
        assert_eq!(
            app(state.clone())
                .oneshot(unsigned(&elsewhere))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        // That one was spent by trying; a fresh one plays.
        let ticket = json_body(
            app(state.clone())
                .oneshot(post("/v1/sound/listen/2/ticket", "{}"))
                .await
                .unwrap(),
        )
        .await;
        let listen = ticket["url"].as_str().unwrap().to_owned();
        let res = app(state.clone()).oneshot(unsigned(&listen)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "audio/mpeg");
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.starts_with(b"ID3"), "{bytes:?}");
        let request = requests.recv().unwrap();
        assert!(request.starts_with("GET /v1/listen/2 "), "{request}");
        // Spent.
        assert_eq!(
            app(state)
                .oneshot(unsigned(&listen))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    fn unsigned_post(uri: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn a_ticket_is_good_once_for_its_speaker_only() {
        let remote = SoundRemote::new(SoundClient::new("http://127.0.0.1:1", "t"));
        let ticket = remote.ticket(2, 0xabc);
        assert!(!remote.spend(&ticket, 3), "good for another speaker");
        let ticket = remote.ticket(2, 0xabd);
        assert!(remote.spend(&ticket, 2));
        assert!(!remote.spend(&ticket, 2), "good twice");
        assert!(!remote.spend("", 2));
        assert!(!remote.spend("made-up", 2));
    }

    #[test]
    fn a_setup_name_cannot_walk_out_of_its_path() {
        assert_eq!(
            setup_path("Kitchen mornings").unwrap(),
            "Kitchen%20mornings"
        );
        assert_eq!(setup_path("../../state").unwrap(), "..%2F..%2Fstate");
        assert_eq!(setup_path("a?b#c").unwrap(), "a%3Fb%23c");
        assert!(setup_path("  ").is_err());
        assert!(setup_path(&"x".repeat(41)).is_err());
    }
}
