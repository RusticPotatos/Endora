//! The hub's API, for the Endora node (and anything else holding the token): the patchbay
//! and the groups it makes, and every change to it. A change is made whole or refused whole.
//! Which speaker is borrowed by another device is the hub's own to say, so it has no route.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Request, State as Extract};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use serde::Deserialize;
use serde_json::{Value, json};

use endora_sound::application::hub::{Hub, HubError};
use endora_sound::domain::patchbay::SENDERS;
use endora_sound::domain::{
    Level, Patchbay, Refusal, SourceId, SourceKind, SpeakerId, SpeakerKind,
};
use endora_sound::infrastructure::{FileStore, json as shape};

use crate::listen::Listeners;

/// What every handler shares.
#[derive(Clone)]
pub struct State {
    /// The hub.
    pub hub: Arc<Hub<FileStore>>,
    /// The token the node signs with.
    pub token: Arc<String>,
    /// The streams being listened to.
    pub listeners: Arc<Listeners>,
    /// A sender to ask which AirPlay speakers are on the network, when there are senders.
    pub found_from: Option<Arc<crate::senders::OwnTone>>,
}

/// The hub's routes.
pub fn app(state: State) -> Router {
    let signed = Router::new()
        .route("/v1/state", get(the_state))
        .route("/v1/sources", post(add_source))
        .route("/v1/sources/{id}", delete(remove_source))
        .route("/v1/sources/{id}/level", put(set_all))
        .route("/v1/speakers", post(add_speaker))
        .route("/v1/speakers/{id}", delete(remove_speaker))
        .route("/v1/speakers/{id}/volume", put(set_volume))
        .route("/v1/wires", put(wire))
        .route("/v1/wires/{source}/{speaker}", delete(unwire))
        .route("/v1/setups", post(save_setup))
        .route("/v1/setups/{name}", delete(remove_setup))
        .route("/v1/setups/{name}/apply", post(apply_setup))
        .route("/v1/listen/{speaker}", get(listen))
        .route("/v1/live/{speaker}", get(live))
        .route("/v1/speakers/{id}/probe", post(probe))
        .route("/v1/found", get(found))
        .route("/v1/speakers/{id}/free", post(free))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            signed_by_the_node,
        ));
    Router::new()
        .route("/health", get(health))
        .merge(signed)
        .with_state(state)
}

/// Equal in time whatever the input, so the token cannot be guessed a byte at a time.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn signed_by_the_node(
    Extract(state): Extract<State>,
    request: Request,
    next: Next,
) -> Response {
    let offered = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !same(offered.as_bytes(), state.token.as_bytes()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "not signed by the node" })),
        )
            .into_response();
    }
    next.run(request).await
}

/// A failure, as the node receives it.
struct Failed(StatusCode, String);

impl From<HubError> for Failed {
    fn from(e: HubError) -> Self {
        let status = match &e {
            HubError::Refused(
                Refusal::NoSuchSource | Refusal::NoSuchSpeaker | Refusal::NoSuchSetup,
            ) => StatusCode::NOT_FOUND,
            HubError::Refused(Refusal::NoFreeSender | Refusal::NameTaken) => StatusCode::CONFLICT,
            HubError::Refused(_) => StatusCode::BAD_REQUEST,
            HubError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(status, e.to_string())
    }
}

impl IntoResponse for Failed {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

/// Runs a change off the async threads: a save writes and syncs a file under the hub's lock.
async fn change<T: Send + 'static>(
    state: &State,
    f: impl FnOnce(&mut Patchbay) -> Result<T, Refusal> + Send + 'static,
) -> Result<T, Failed> {
    let hub = state.hub.clone();
    tokio::task::spawn_blocking(move || hub.change(f))
        .await
        .map_err(|_| {
            Failed(
                StatusCode::INTERNAL_SERVER_ERROR,
                "a worker thread failed".to_owned(),
            )
        })?
        .map_err(Failed::from)
}

fn a_level(n: u8) -> Result<Level, Failed> {
    Level::new(n).ok_or_else(|| Failed(StatusCode::BAD_REQUEST, "a level is 0 to 100".to_owned()))
}

/// Nothing was there to take away.
fn not_there(what: &str) -> Failed {
    Failed(StatusCode::NOT_FOUND, format!("there is no such {what}"))
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// The patchbay, the groups the wires make, and how many senders there are for them.
async fn the_state(Extract(state): Extract<State>) -> Json<Value> {
    Json(state.hub.read(|bay| {
        let mut v = shape::to_json(bay);
        v["senders"] = json!(SENDERS);
        v["groups"] = bay
            .groups()
            .iter()
            .map(|g| json!({
                "speakers": g.speakers.iter().map(|s| s.0).collect::<Vec<_>>(),
                "mix": g.mix.iter().map(|(s, l)| json!({ "source": s.0, "level": l.get() })).collect::<Vec<_>>(),
                "needs_sender": g.needs_sender,
            }))
            .collect();
        v["listening"] = json!(state.listeners.listening().iter().map(|s| s.0).collect::<Vec<_>>());
        v
    }))
}

/// The AirPlay speakers the senders can see on the network, for the owner to add by name.
async fn found(Extract(state): Extract<State>) -> Result<Json<Value>, Failed> {
    let Some(sender) = state.found_from.clone() else {
        return Err(Failed(
            StatusCode::SERVICE_UNAVAILABLE,
            "no AirPlay senders are set up".to_owned(),
        ));
    };
    let names = tokio::task::spawn_blocking(move || crate::senders::found(sender.as_ref()))
        .await
        .map_err(|_| {
            Failed(
                StatusCode::INTERNAL_SERVER_ERROR,
                "a worker thread failed".to_owned(),
            )
        })?
        .map_err(|e| {
            Failed(
                StatusCode::BAD_GATEWAY,
                format!("the sender did not answer: {e}"),
            )
        })?;
    Ok(Json(json!({ "speakers": names })))
}

/// The owner says a borrowed speaker is free again: it goes back to its wires.
async fn free(Extract(state): Extract<State>, Path(id): Path<u32>) -> Result<StatusCode, Failed> {
    change(&state, move |bay| bay.set_borrowed(SpeakerId(id), false)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A stream speaker's live stream, as MP3, for as long as the listener stays. The phone, the
/// tablet and the handheld play this; the node passes it on to them.
async fn listen(Extract(state): Extract<State>, Path(id): Path<u32>) -> Result<Response, Failed> {
    stream_speaker(&state, id)?;
    let receiver = state.listeners.listen(SpeakerId(id)).map_err(|e| {
        Failed(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the stream could not start: {e}"),
        )
    })?;
    let body = futures_util::stream::unfold(receiver, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(chunk) => return Some((Ok::<_, std::convert::Infallible>(chunk), rx)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "audio/mpeg"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
            (axum::http::header::ACCEPT_RANGES, "none"),
        ],
        axum::body::Body::from_stream(body),
    )
        .into_response())
}

/// Whether `id` is a stream speaker, which is what can be listened to.
fn stream_speaker(state: &State, id: u32) -> Result<(), Failed> {
    let kind = state
        .hub
        .read(|bay| bay.speaker(SpeakerId(id)).map(|s| s.kind));
    match kind {
        None => Err(not_there("speaker")),
        Some(SpeakerKind::AirPlay) => Err(Failed(
            StatusCode::BAD_REQUEST,
            "an AirPlay speaker is played by a sender, not listened to".to_owned(),
        )),
        Some(SpeakerKind::Stream) => Ok(()),
    }
}

/// A stream speaker's live stream: raw PCM in 20 ms frames (see [`crate::live`]), for a page
/// that wants the least delay. Each frame goes out as soon as it is mixed, as one chunk, and a
/// listener that falls behind skips ahead rather than carrying a growing delay.
async fn live(Extract(state): Extract<State>, Path(id): Path<u32>) -> Result<Response, Failed> {
    stream_speaker(&state, id)?;
    let receiver = state.listeners.listen_live(SpeakerId(id));
    let body = futures_util::stream::unfold(receiver, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(frame) => return Some((Ok::<_, std::convert::Infallible>(frame), rx)),
                // The sequence numbers tell the page what it missed.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/vnd.endora.live",
            ),
            (axum::http::header::CACHE_CONTROL, "no-store"),
            // Said plainly, so a proxy in front does not hold the frames back.
            (
                axum::http::HeaderName::from_static("x-accel-buffering"),
                "no",
            ),
        ],
        axum::body::Body::from_stream(body),
    )
        .into_response())
}

/// The owner asks to measure a speaker's delay: for a moment its mix is replaced by a tone the
/// page listens for. The answer says, by the hub's clock, when the request arrived, when the
/// tone went into the mix and when this answer was made (so the page can take the network out
/// of what it measures), and how long each source's sound waits in the hub before it is mixed.
async fn probe(Extract(state): Extract<State>, Path(id): Path<u32>) -> Result<Json<Value>, Failed> {
    stream_speaker(&state, id)?;
    let received = crate::listen::now_us();
    let told = state.listeners.probe(SpeakerId(id)).map_err(|_| {
        Failed(
            StatusCode::CONFLICT,
            "nobody is listening to that speaker".to_owned(),
        )
    })?;
    let injected = tokio::task::spawn_blocking(move || {
        told.recv_timeout(std::time::Duration::from_secs(2)).ok()
    })
    .await
    .ok()
    .flatten()
    .ok_or_else(|| {
        Failed(
            StatusCode::GATEWAY_TIMEOUT,
            "the tone did not get into the mix".to_owned(),
        )
    })?;
    let replied = crate::listen::now_us();
    let intake = state.listeners.intake();
    println!(
        "sound: delay probe on speaker {id}: mixed {} us after it arrived; sound waits in the hub before the mix: {}",
        injected.saturating_sub(received),
        if intake.is_empty() {
            "nothing is playing".to_owned()
        } else {
            intake
                .iter()
                .map(|(name, ms)| format!("{name} {ms} ms"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    Ok(Json(json!({
        "received_at_us": received,
        "injected_at_us": injected,
        "replied_at_us": replied,
        "rate": endora_sound::domain::mix::RATE,
        "tone_hz": crate::live::PROBE_HZ,
        "tone_ms": crate::live::PROBE_MS,
        "tone_amplitude": crate::live::PROBE_AMPLITUDE,
        "intake": intake.iter().map(|(name, ms)| json!({ "source": name, "ms": ms })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct NewSource {
    name: String,
    kind: String,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    device: Option<String>,
}

async fn add_source(
    Extract(state): Extract<State>,
    Json(req): Json<NewSource>,
) -> Result<Json<Value>, Failed> {
    let kind = match (req.kind.as_str(), req.address) {
        ("airplay", _) => SourceKind::AirPlay,
        ("spotify", _) => SourceKind::Spotify,
        ("voice", _) => SourceKind::Voice,
        ("radio", Some(address)) => SourceKind::Radio(address.trim().to_owned()),
        ("radio", None) => {
            return Err(Failed(
                StatusCode::BAD_REQUEST,
                "a radio station needs an address".to_owned(),
            ));
        }
        _ => {
            return Err(Failed(
                StatusCode::BAD_REQUEST,
                "a source is airplay, spotify, radio or voice".to_owned(),
            ));
        }
    };
    let id = change(&state, move |bay| {
        bay.add_source(&req.name, kind, req.device.as_deref())
    })
    .await?;
    Ok(Json(json!({ "id": id.0 })))
}

async fn remove_source(
    Extract(state): Extract<State>,
    Path(id): Path<u32>,
) -> Result<StatusCode, Failed> {
    if !change(&state, move |bay| Ok(bay.remove_source(SourceId(id)))).await? {
        return Err(not_there("source"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct LevelOf {
    level: u8,
}

/// Sets every wire of one source at once ("classical at 10 everywhere").
async fn set_all(
    Extract(state): Extract<State>,
    Path(id): Path<u32>,
    Json(req): Json<LevelOf>,
) -> Result<Json<Value>, Failed> {
    let level = a_level(req.level)?;
    let count = change(&state, move |bay| bay.set_all(SourceId(id), level)).await?;
    Ok(Json(json!({ "wires": count })))
}

#[derive(Deserialize)]
struct NewSpeaker {
    name: String,
    kind: String,
    #[serde(default)]
    device: Option<String>,
}

async fn add_speaker(
    Extract(state): Extract<State>,
    Json(req): Json<NewSpeaker>,
) -> Result<Json<Value>, Failed> {
    let kind = SpeakerKind::from_name(&req.kind).ok_or_else(|| {
        Failed(
            StatusCode::BAD_REQUEST,
            "a speaker is airplay or stream".to_owned(),
        )
    })?;
    let id = change(&state, move |bay| {
        bay.add_speaker(&req.name, kind, req.device.as_deref())
    })
    .await?;
    Ok(Json(json!({ "id": id.0 })))
}

async fn remove_speaker(
    Extract(state): Extract<State>,
    Path(id): Path<u32>,
) -> Result<StatusCode, Failed> {
    if !change(&state, move |bay| Ok(bay.remove_speaker(SpeakerId(id)))).await? {
        return Err(not_there("speaker"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn set_volume(
    Extract(state): Extract<State>,
    Path(id): Path<u32>,
    Json(req): Json<LevelOf>,
) -> Result<StatusCode, Failed> {
    let level = a_level(req.level)?;
    change(&state, move |bay| bay.set_volume(SpeakerId(id), level)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NewWire {
    source: u32,
    speaker: u32,
    level: u8,
}

async fn wire(
    Extract(state): Extract<State>,
    Json(req): Json<NewWire>,
) -> Result<StatusCode, Failed> {
    let level = a_level(req.level)?;
    change(&state, move |bay| {
        bay.wire(SourceId(req.source), SpeakerId(req.speaker), level)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unwire(
    Extract(state): Extract<State>,
    Path((source, speaker)): Path<(u32, u32)>,
) -> Result<StatusCode, Failed> {
    if !change(&state, move |bay| {
        Ok(bay.unwire(SourceId(source), SpeakerId(speaker)))
    })
    .await?
    {
        return Err(not_there("wire"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SetupName {
    name: String,
}

async fn save_setup(
    Extract(state): Extract<State>,
    Json(req): Json<SetupName>,
) -> Result<StatusCode, Failed> {
    change(&state, move |bay| bay.save_setup(&req.name)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn apply_setup(
    Extract(state): Extract<State>,
    Path(name): Path<String>,
) -> Result<StatusCode, Failed> {
    change(&state, move |bay| bay.apply_setup(&name)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_setup(
    Extract(state): Extract<State>,
    Path(name): Path<String>,
) -> Result<StatusCode, Failed> {
    if !change(&state, move |bay| Ok(bay.remove_setup(&name))).await? {
        return Err(not_there("setup"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    const TOKEN: &str = "a-token-of-at-least-thirty-two-characters";

    fn state() -> (State, std::path::PathBuf) {
        // Each test its own state file: tests run side by side, and the clock alone can give
        // two of them the same name.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "endora-sound-api-{}-{n}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sound.json");
        let hub = Hub::open(FileStore::new(&path)).unwrap();
        (
            State {
                hub: Arc::new(hub),
                token: Arc::new(TOKEN.to_owned()),
                listeners: Arc::new(Listeners::new("ffmpeg".into())),
                found_from: None,
            },
            path,
        )
    }

    fn call(method: &str, uri: &str, token: Option<&str>, body: &str) -> HttpRequest<Body> {
        let mut b = HttpRequest::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            b = b.header("authorization", format!("Bearer {token}"));
        }
        b.body(Body::from(body.to_owned())).unwrap()
    }

    async fn send(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
        let res = app
            .clone()
            .oneshot(call(method, uri, Some(TOKEN), body))
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn only_the_node_is_answered() {
        let (state, _) = state();
        let app = app(state);
        for token in [
            None,
            Some("wrong"),
            Some("a-token-of-at-least-thirty-two-characterz"),
        ] {
            let res = app
                .clone()
                .oneshot(call("GET", "/v1/state", token, ""))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let res = app
            .clone()
            .oneshot(call("GET", "/health", None, ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "health needs no token");
    }

    #[tokio::test]
    async fn wires_make_groups_and_the_state_says_so() {
        let (state, path) = state();
        let app = app(state);
        let (_, radio) = send(
            &app,
            "POST",
            "/v1/sources",
            r#"{"name":"Classical","kind":"radio","address":"https://radio.example.com/c"}"#,
        )
        .await;
        let (_, kitchen) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Kitchen","kind":"airplay"}"#,
        )
        .await;
        let (_, office) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Office","kind":"airplay"}"#,
        )
        .await;
        let (radio, kitchen, office) = (&radio["id"], &kitchen["id"], &office["id"]);
        for speaker in [kitchen, office] {
            let (status, _) = send(
                &app,
                "PUT",
                "/v1/wires",
                &format!(r#"{{"source":{radio},"speaker":{speaker},"level":10}}"#),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
        }
        let (_, s) = send(&app, "GET", "/v1/state", "").await;
        assert_eq!(s["senders"], 4);
        assert_eq!(s["groups"].as_array().unwrap().len(), 1, "{s}");
        assert_eq!(s["groups"][0]["speakers"], json!([kitchen, office]));
        assert_eq!(
            s["groups"][0]["mix"],
            json!([{ "source": radio, "level": 10 }])
        );
        // Kitchen mornings: the station louder in the kitchen makes it a mix of its own.
        let (status, set) = send(
            &app,
            "PUT",
            &format!("/v1/sources/{radio}/level"),
            r#"{"level":20}"#,
        )
        .await;
        assert_eq!((status, set["wires"].clone()), (StatusCode::OK, json!(2)));
        send(
            &app,
            "PUT",
            "/v1/wires",
            &format!(r#"{{"source":{radio},"speaker":{kitchen},"level":60}}"#),
        )
        .await;
        let (_, s) = send(&app, "GET", "/v1/state", "").await;
        assert_eq!(s["groups"].as_array().unwrap().len(), 2);
        // And all of it was kept.
        let kept = std::fs::read_to_string(&path).unwrap();
        assert!(
            kept.contains("Classical") && kept.contains("\"level\": 60"),
            "{kept}"
        );
    }

    #[tokio::test]
    async fn what_the_patchbay_refuses_reaches_the_node_as_what_it_is() {
        let (state, _) = state();
        let app = app(state);
        let (_, phone) = send(
            &app,
            "POST",
            "/v1/sources",
            r#"{"name":"Endora · iPhone","kind":"airplay","device":"iphone"}"#,
        )
        .await;
        let (_, phone_out) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"iPhone","kind":"stream","device":"iphone"}"#,
        )
        .await;
        let (phone, phone_out) = (&phone["id"], &phone_out["id"]);
        let cases = [
            (
                "PUT",
                "/v1/wires".to_owned(),
                format!(r#"{{"source":{phone},"speaker":{phone_out},"level":50}}"#),
                StatusCode::BAD_REQUEST,
            ),
            (
                "PUT",
                "/v1/wires".to_owned(),
                format!(r#"{{"source":99,"speaker":{phone_out},"level":50}}"#),
                StatusCode::NOT_FOUND,
            ),
            (
                "PUT",
                "/v1/wires".to_owned(),
                format!(r#"{{"source":{phone},"speaker":99,"level":50}}"#),
                StatusCode::NOT_FOUND,
            ),
            (
                "PUT",
                format!("/v1/speakers/{phone_out}/volume"),
                r#"{"level":101}"#.to_owned(),
                StatusCode::BAD_REQUEST,
            ),
            (
                "POST",
                "/v1/sources".to_owned(),
                r#"{"name":"Bad","kind":"radio","address":"file:///etc/passwd"}"#.to_owned(),
                StatusCode::BAD_REQUEST,
            ),
            (
                "POST",
                "/v1/sources".to_owned(),
                r#"{"name":"Bad","kind":"radio"}"#.to_owned(),
                StatusCode::BAD_REQUEST,
            ),
            (
                "POST",
                "/v1/sources".to_owned(),
                r#"{"name":"Bad","kind":"vinyl"}"#.to_owned(),
                StatusCode::BAD_REQUEST,
            ),
            (
                "POST",
                "/v1/speakers".to_owned(),
                r#"{"name":"iPhone","kind":"stream"}"#.to_owned(),
                StatusCode::CONFLICT,
            ),
            (
                "POST",
                "/v1/setups/Night/apply".to_owned(),
                String::new(),
                StatusCode::NOT_FOUND,
            ),
            (
                "DELETE",
                format!("/v1/wires/{phone}/{phone_out}"),
                String::new(),
                StatusCode::NOT_FOUND,
            ),
            (
                "DELETE",
                "/v1/speakers/99".to_owned(),
                String::new(),
                StatusCode::NOT_FOUND,
            ),
        ];
        for (method, uri, body, expected) in cases {
            let (status, said) = send(&app, method, &uri, &body).await;
            assert_eq!(status, expected, "{method} {uri} {body} said {said}");
        }
        let (_, s) = send(&app, "GET", "/v1/state", "").await;
        assert!(
            s["wires"].as_array().unwrap().is_empty(),
            "a refusal changed something: {s}"
        );
    }

    #[tokio::test]
    async fn a_setup_is_saved_applied_and_forgotten() {
        let (state, _) = state();
        let app = app(state);
        let (_, radio) = send(
            &app,
            "POST",
            "/v1/sources",
            r#"{"name":"Classical","kind":"radio","address":"https://radio.example.com/c"}"#,
        )
        .await;
        let (_, kitchen) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Kitchen","kind":"airplay"}"#,
        )
        .await;
        let (radio, kitchen) = (&radio["id"], &kitchen["id"]);
        send(
            &app,
            "PUT",
            "/v1/wires",
            &format!(r#"{{"source":{radio},"speaker":{kitchen},"level":60}}"#),
        )
        .await;
        assert_eq!(
            send(&app, "POST", "/v1/setups", r#"{"name":"Kitchen mornings"}"#)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        send(&app, "DELETE", &format!("/v1/wires/{radio}/{kitchen}"), "").await;
        assert_eq!(
            send(&app, "POST", "/v1/setups/Kitchen%20mornings/apply", "")
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let (_, s) = send(&app, "GET", "/v1/state", "").await;
        assert_eq!(
            s["wires"],
            json!([{ "source": radio, "speaker": kitchen, "level": 60 }])
        );
        assert_eq!(
            send(&app, "DELETE", "/v1/setups/Kitchen%20mornings", "")
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            send(&app, "DELETE", "/v1/setups/Kitchen%20mornings", "")
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn only_a_stream_speaker_can_be_listened_to() {
        let (state, _) = state();
        let app = app(state);
        let (_, kitchen) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Kitchen","kind":"airplay"}"#,
        )
        .await;
        let kitchen = &kitchen["id"];
        assert_eq!(
            send(&app, "GET", &format!("/v1/listen/{kitchen}"), "")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(&app, "GET", "/v1/listen/99", "").await.0,
            StatusCode::NOT_FOUND
        );
        let res = app
            .clone()
            .oneshot(call("GET", "/v1/listen/99", None, ""))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "a stream is signed like everything else"
        );
    }

    /// A web server on this machine that answers every request with two seconds of a tone,
    /// as a WAV file.
    fn a_station() -> String {
        use std::io::{Read as _, Write as _};
        let rate = 44_100usize;
        let mut data = Vec::new();
        for i in 0..2 * rate {
            let v =
                ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12_000.0) as i16;
            data.extend_from_slice(&v.to_le_bytes());
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for (v, n) in [
            (16u32, 4),
            (1, 2),
            (2, 2),
            (44_100, 4),
            (176_400, 4),
            (4, 2),
            (16, 2),
        ] {
            wav.extend_from_slice(&v.to_le_bytes()[..n]);
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.read(&mut [0u8; 2048]);
                let head = format!(
                    "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\n\r\n",
                    wav.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&wav);
            }
        });
        format!("http://127.0.0.1:{port}/classical.wav")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_phone_listening_hears_its_groups_mix_as_mp3() {
        let found = std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if !found.is_ok_and(|s| s.success()) {
            eprintln!(
                "no ffmpeg here: the live stream test is skipped (it runs in the image and on the Mac)"
            );
            return;
        }
        let (state, path) = state();
        crate::player::start(
            state.hub.clone(),
            state.listeners.clone(),
            endora_sound::infrastructure::Places {
                ffmpeg: "ffmpeg".into(),
                pipes: path.parent().unwrap().to_owned(),
            },
            Box::new(crate::player::NoSenders),
        );
        let app = app(state);
        let body = format!(
            r#"{{"name":"Classical","kind":"radio","address":"{}"}}"#,
            a_station()
        );
        let (_, radio) = send(&app, "POST", "/v1/sources", &body).await;
        let (_, phone) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"iPhone","kind":"stream","device":"iphone"}"#,
        )
        .await;
        let (radio, phone) = (&radio["id"], &phone["id"]);
        send(
            &app,
            "PUT",
            "/v1/wires",
            &format!(r#"{{"source":{radio},"speaker":{phone},"level":80}}"#),
        )
        .await;
        let res = app
            .clone()
            .oneshot(call("GET", &format!("/v1/listen/{phone}"), Some(TOKEN), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "audio/mpeg");
        let mut body = res.into_body().into_data_stream();
        let mut heard: Vec<u8> = Vec::new();
        let got = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            use futures_util::StreamExt as _;
            while heard.len() < 16_384 {
                match body.next().await {
                    Some(Ok(chunk)) => heard.extend_from_slice(&chunk),
                    _ => break,
                }
            }
        })
        .await;
        assert!(
            got.is_ok(),
            "the stream gave only {} bytes in 20 s",
            heard.len()
        );
        // MP3: an ID3 tag first, or straight into a frame (eleven set bits).
        let frame = heard
            .windows(2)
            .any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0);
        assert!(
            heard.starts_with(b"ID3") || frame,
            "not MP3: {:?}",
            &heard[..8.min(heard.len())]
        );
        let (_, s) = send(&app, "GET", "/v1/state", "").await;
        assert_eq!(s["listening"], json!([phone]));
    }

    async fn a_phone(app: &Router) -> u64 {
        let (_, phone) = send(
            app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Test phone","kind":"stream","device":"test-phone"}"#,
        )
        .await;
        phone["id"].as_u64().unwrap()
    }

    #[tokio::test]
    async fn only_a_signed_request_for_a_stream_speaker_gets_the_live_stream() {
        let (state, _) = state();
        let app = app(state);
        let (_, kitchen) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Kitchen","kind":"airplay"}"#,
        )
        .await;
        let kitchen = &kitchen["id"];
        assert_eq!(
            send(&app, "GET", &format!("/v1/live/{kitchen}"), "")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(&app, "GET", "/v1/live/99", "").await.0,
            StatusCode::NOT_FOUND
        );
        for uri in ["/v1/live/99", "/v1/speakers/99/probe"] {
            let method = if uri.ends_with("probe") {
                "POST"
            } else {
                "GET"
            };
            let res = app
                .clone()
                .oneshot(call(method, uri, None, ""))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri} is signed");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_live_stream_is_pcm_frames_that_arrive_as_they_are_mixed() {
        use futures_util::StreamExt as _;
        let (state, _) = state();
        let listeners = state.listeners.clone();
        let app = app(state);
        let phone = a_phone(&app).await;
        let res = app
            .clone()
            .oneshot(call("GET", &format!("/v1/live/{phone}"), Some(TOKEN), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "application/vnd.endora.live");
        assert_eq!(res.headers()["cache-control"], "no-store");
        let (_, s) = send(&app, "GET", "/v1/state", "").await;
        assert_eq!(
            s["listening"],
            json!([phone]),
            "a live listener is a listener"
        );
        // The mixer feeds a block every tick; each frame reaches the listener as a chunk of
        // its own, not held back to fill a buffer.
        let speaker = SpeakerId(u32::try_from(phone).unwrap());
        let feeder = std::thread::spawn(move || {
            for n in 0..10i16 {
                listeners.feed(speaker, &vec![n + 1; crate::live::FRAME_FRAMES * 2]);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        let mut body = res.into_body().into_data_stream();
        let mut seen = Vec::new();
        for _ in 0..10 {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
                .await
                .expect("a frame in five seconds")
                .expect("the stream goes on")
                .unwrap();
            let (frame, rest) = crate::live::parse(&chunk).expect("one whole frame per chunk");
            assert!(rest.is_empty());
            seen.push((frame.seq, frame.samples[0]));
        }
        feeder.join().unwrap();
        assert_eq!(seen, (0..10u32).zip(1..=10i16).collect::<Vec<_>>());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_delay_probe_is_refused_without_a_listener_and_answers_with_the_hubs_clock() {
        use futures_util::StreamExt as _;
        let (state, _) = state();
        let listeners = state.listeners.clone();
        let app = app(state);
        let phone = a_phone(&app).await;
        let (status, _) = send(&app, "POST", &format!("/v1/speakers/{phone}/probe"), "").await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "nobody listens, nobody would hear it"
        );
        let res = app
            .clone()
            .oneshot(call("GET", &format!("/v1/live/{phone}"), Some(TOKEN), ""))
            .await
            .unwrap();
        let speaker = SpeakerId(u32::try_from(phone).unwrap());
        listeners.note_intake(vec![("Endora Mixer".to_owned(), 160)]);
        let feeder = std::thread::spawn(move || {
            for _ in 0..30 {
                listeners.feed(speaker, &vec![5i16; crate::live::FRAME_FRAMES * 2]);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        let (_, before) = send(&app, "GET", "/v1/state", "").await;
        let (status, answer) = send(&app, "POST", &format!("/v1/speakers/{phone}/probe"), "").await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        // A measurement touches the sound, never the patchbay: no source, wire or level moves.
        let (_, after) = send(&app, "GET", "/v1/state", "").await;
        for key in ["sources", "speakers", "wires", "setups", "groups"] {
            assert_eq!(before[key], after[key], "a delay probe changed the {key}");
        }
        let (got, injected, replied) = (
            answer["received_at_us"].as_u64().unwrap(),
            answer["injected_at_us"].as_u64().unwrap(),
            answer["replied_at_us"].as_u64().unwrap(),
        );
        assert!(got <= replied && replied - got < 1_000_000, "{answer}");
        assert!(
            injected + 25_000 >= got,
            "the tone is mixed after the request: {answer}"
        );
        assert_eq!(answer["rate"], 44_100);
        assert_eq!(answer["tone_hz"], 2000.0);
        assert_eq!(
            answer["intake"],
            json!([{ "source": "Endora Mixer", "ms": 160 }])
        );
        // The frame the tone began in carries the clock the answer gave.
        let mut body = res.into_body().into_data_stream();
        let mut flagged = None;
        while let Some(Ok(chunk)) =
            tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
                .await
                .unwrap()
        {
            let (frame, _) = crate::live::parse(&chunk).unwrap();
            if frame.flags & crate::live::FLAG_PROBE != 0 {
                flagged = Some(frame.at_us);
                break;
            }
        }
        feeder.join().unwrap();
        assert_eq!(flagged, Some(injected));
    }

    #[tokio::test]
    async fn a_borrowed_speaker_is_taken_back_when_the_owner_says_it_is_free() {
        let (state, _) = state();
        let hub = state.hub.clone();
        let app = app(state);
        let (_, kitchen) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Kitchen","kind":"airplay"}"#,
        )
        .await;
        let id = kitchen["id"].as_u64().unwrap() as u32;
        hub.change(|bay| bay.set_borrowed(SpeakerId(id), true))
            .unwrap();
        assert_eq!(
            send(&app, "POST", &format!("/v1/speakers/{id}/free"), "")
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert!(!hub.read(|bay| bay.speaker(SpeakerId(id)).unwrap().borrowed));
        assert_eq!(
            send(&app, "POST", "/v1/speakers/99/free", "").await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn deleting_a_speaker_takes_its_wires_and_nothing_else() {
        let (state, _) = state();
        let app = app(state);
        let (_, station) = send(
            &app,
            "POST",
            "/v1/sources",
            &format!(
                r#"{{"name":"Test station","kind":"radio","address":"{}"}}"#,
                a_station()
            ),
        )
        .await;
        let (_, pad) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Pad","kind":"stream","device":"pad"}"#,
        )
        .await;
        let (_, kitchen) = send(
            &app,
            "POST",
            "/v1/speakers",
            r#"{"name":"Kitchen","kind":"airplay"}"#,
        )
        .await;
        for speaker in [&pad, &kitchen] {
            let wire = format!(
                r#"{{"source":{},"speaker":{},"level":60}}"#,
                station["id"], speaker["id"]
            );
            send(&app, "PUT", "/v1/wires", &wire).await;
        }
        let uri = format!("/v1/speakers/{}", pad["id"]);
        assert_eq!(
            send(&app, "DELETE", &uri, "").await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            send(&app, "DELETE", &uri, "").await.0,
            StatusCode::NOT_FOUND
        );
        let (_, s) = send(&app, "GET", "/v1/state", "").await;
        assert_eq!(s["sources"].as_array().unwrap().len(), 1);
        assert_eq!(s["wires"].as_array().unwrap().len(), 1);
        assert_eq!(s["wires"][0]["speaker"], kitchen["id"]);
    }

    #[tokio::test]
    async fn with_no_senders_there_is_nothing_found_and_it_says_why() {
        let (state, _) = state();
        let (status, said) = send(&app(state), "GET", "/v1/found", "").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(said["error"], "no AirPlay senders are set up");
    }
}
