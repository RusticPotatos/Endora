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

/// What every handler shares.
#[derive(Clone)]
pub struct State {
    /// The hub.
    pub hub: Arc<Hub<FileStore>>,
    /// The token the node signs with.
    pub token: Arc<String>,
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
        v
    }))
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
        let dir = std::env::temp_dir().join(format!(
            "endora-sound-api-{}-{:?}",
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
}
