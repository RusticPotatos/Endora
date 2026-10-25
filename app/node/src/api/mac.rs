//! The owner's Mac as a brain (ADR 0130). A small helper on the Mac says every thirty seconds
//! whether it is plugged in, how long the owner has been idle and whether its model is up
//! (`POST /v1/mac/heartbeat`, with a token of its own that opens that route and nothing else).
//! From that and the owner's "Mac busy" switch the node decides whether the Mac is free for a
//! team turn, tells HQ when that changes (`POST /v1/mac` on HQ), and sends a Mac-brain turn's
//! model calls to the Mac, and to nothing else, only while it is.
//!
//! The settings (the model's address, its key, its name, the switch, how long the owner must
//! be idle, the helper's token) are kept in the node's sealed settings table under one id, so
//! the key and the token are encrypted at rest like every other credential (ADR 0125) and are
//! never sent back to the console.

use super::*;
use std::sync::Mutex;

/// The id the Mac's settings are kept under in the capability settings table.
const ID: &str = "mac-brain";

/// How long after its last heartbeat the Mac still counts as heard from.
const HEARD_WITHIN_MS: i64 = 90_000;

/// How long the owner must have been idle before a team turn may run on the Mac, if the owner
/// has not said: no wait (the owner, 2026-10-25: their being at the Mac should not hold its
/// model). The owner may still ask for one, or switch "Mac busy" on.
const DEFAULT_IDLE_MINUTES: u64 = 0;

/// The longest idle time the owner may ask for: a day.
const MOST_IDLE_MINUTES: u64 = 24 * 60;

/// How often HQ is told the Mac's state when it has not changed: HQ trusts what it was told
/// for two and a half minutes.
const TELL_AGAIN_MS: i64 = 60_000;

/// The model name sent to the Mac's server when the owner has not named one.
const DEFAULT_MODEL: &str = "mac";

/// What the Mac's helper last said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Beat {
    /// When the node heard it, in milliseconds.
    pub(crate) at: i64,
    /// Whether the Mac is on mains power.
    pub(crate) plugged: bool,
    /// Seconds since the owner last touched the keyboard or mouse.
    pub(crate) idle_s: u64,
    /// Whether the model's server answers on the Mac.
    pub(crate) model_up: bool,
}

/// Whether the Mac is free for a team turn, and if not why, in the few words the console and
/// HQ show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Availability {
    pub(crate) ready: bool,
    pub(crate) why: &'static str,
}

/// Whether the Mac is free for a turn at `now`: heard from within ninety seconds, the owner's
/// "Mac busy" switch off, on mains power, its model up, and the owner idle for at least
/// `idle_needed_s`. The reasons are in that order.
pub(crate) fn availability(
    now: i64,
    beat: Option<&Beat>,
    busy: bool,
    idle_needed_s: u64,
) -> Availability {
    let no = |why| Availability { ready: false, why };
    let Some(beat) = beat.filter(|b| now - b.at <= HEARD_WITHIN_MS) else {
        return no("not heard from");
    };
    if busy {
        return no("busy");
    }
    if !beat.plugged {
        return no("on battery");
    }
    if !beat.model_up {
        return no("model loading");
    }
    if beat.idle_s < idle_needed_s {
        return no("owner active");
    }
    Availability {
        ready: true,
        why: "ready",
    }
}

/// What the node remembers about the Mac, in memory: the last heartbeat, and what HQ was last
/// told and when.
#[derive(Default)]
pub(crate) struct MacBrain {
    beat: Mutex<Option<Beat>>,
    told: Mutex<Option<(bool, String, i64)>>,
}

impl MacBrain {
    pub(super) fn hear(&self, beat: Beat) {
        *self
            .beat
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(beat);
    }

    fn last(&self) -> Option<Beat> {
        self.beat
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// The owner's settings for the Mac.
#[derive(Debug, Clone, Default)]
pub(crate) struct Settings {
    /// The model server's base address, `http://<host>:8080/v1`; empty until set.
    pub(crate) url: String,
    /// The server's key; empty for none.
    pub(crate) api_key: String,
    /// The model name sent with every call.
    pub(crate) model: String,
    /// The owner's "Mac busy" switch.
    pub(crate) busy: bool,
    /// How many minutes the owner must have been idle.
    pub(crate) idle_minutes: u64,
    /// The helper's token; empty until the owner makes one.
    token: String,
}

impl Settings {
    /// What is kept; the defaults for what is not.
    pub(crate) fn load(config: &endora_capabilities::ConfigStore) -> Self {
        let mut found = Self {
            idle_minutes: DEFAULT_IDLE_MINUTES,
            model: DEFAULT_MODEL.to_owned(),
            ..Self::default()
        };
        for (id, key, value) in config.all_settings().unwrap_or_default() {
            if id != ID {
                continue;
            }
            match key.as_str() {
                "url" => found.url = value,
                "api_key" => found.api_key = value,
                "model" if !value.is_empty() => found.model = value,
                "busy" => found.busy = value == "true",
                "idle_minutes" => {
                    found.idle_minutes = value
                        .parse()
                        .ok()
                        .filter(|m| (0..=MOST_IDLE_MINUTES).contains(m))
                        .unwrap_or(DEFAULT_IDLE_MINUTES);
                }
                "token" => found.token = value,
                _ => {}
            }
        }
        found
    }

    fn idle_needed_s(&self) -> u64 {
        self.idle_minutes * 60
    }
}

fn set(config: &endora_capabilities::ConfigStore, key: &str, value: &str) -> Result<(), AppError> {
    config
        .set_setting(ID, key, value)
        .map_err(AppError::Repository)
}

fn refused(message: &str) -> ApiError {
    ApiError(AppError::BadRequest {
        message: message.to_owned(),
    })
}

/// The Mac's state as the node knows it now, for the console: never the key, never the token.
fn shown(state: &AppState, now: i64) -> serde_json::Value {
    let settings = Settings::load(&state.config);
    let beat = state.mac.last();
    let free = availability(now, beat.as_ref(), settings.busy, settings.idle_needed_s());
    let heard = beat.as_ref().filter(|b| now - b.at <= HEARD_WITHIN_MS);
    json!({
        "ready": free.ready,
        "why": free.why,
        "busy": settings.busy,
        "url": settings.url,
        "model": settings.model,
        "has_key": !settings.api_key.is_empty(),
        "has_token": !settings.token.is_empty(),
        "idle_minutes": settings.idle_minutes,
        "plugged": heard.map(|b| b.plugged),
        "idle_s": heard.map(|b| b.idle_s),
        "model_up": heard.map(|b| b.model_up),
        "heard_s_ago": beat.as_ref().map(|b| ((now - b.at) / 1000).max(0)),
    })
}

/// Tells HQ whether the Mac is free, when that has changed or HQ has not heard it for a minute.
/// Nothing when HQ is not set up. Returns whether HQ was told now.
pub(super) fn tell_hq(state: &AppState, now: i64) -> bool {
    let Some(hq) = state.hq.as_ref() else {
        return false;
    };
    let settings = Settings::load(&state.config);
    let free = availability(
        now,
        state.mac.last().as_ref(),
        settings.busy,
        settings.idle_needed_s(),
    );
    let mut told = state
        .mac
        .told
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((ready, why, at)) = told.as_ref() {
        if *ready == free.ready && why == free.why && now - at < TELL_AGAIN_MS {
            return false;
        }
    }
    match hq.post("/v1/mac", &json!({ "ready": free.ready, "why": free.why })) {
        Ok(_) => {
            *told = Some((free.ready, free.why.to_owned(), now));
            true
        }
        Err(e) => {
            eprintln!("mac: HQ was not told the Mac's state: {e}");
            false
        }
    }
}

/// Keeps HQ told, every twenty seconds: a Mac that stops reporting becomes "not heard from"
/// here, and so for HQ, without anyone calling the node.
pub(crate) fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(20));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let (state, now) = (state.clone(), state.clock.now().unix_millis());
            let _ = tokio::task::spawn_blocking(move || tell_hq(&state, now)).await;
        }
    });
}

/// Where a Mac turn's call goes.
pub(super) struct Target {
    pub(super) url: String,
    pub(super) model: String,
    pub(super) key: String,
}

/// The Mac's address, model and key for a turn's call at `now`, or why the call cannot go: the
/// Mac is not set up, or it is not free. The turn ends with that, and is tried again when the
/// Mac is free; it is never answered by another model.
pub(super) fn target(state: &AppState, now: i64) -> Result<Target, String> {
    let settings = Settings::load(&state.config);
    if settings.url.trim().is_empty() {
        return Err("the Mac is not set up on this node: no address for its model".to_owned());
    }
    let free = availability(
        now,
        state.mac.last().as_ref(),
        settings.busy,
        settings.idle_needed_s(),
    );
    if !free.ready {
        return Err(format!(
            "the Mac is not available ({}): the turn waits and is tried again when it is",
            free.why
        ));
    }
    Ok(Target {
        url: settings.url,
        model: settings.model,
        key: settings.api_key,
    })
}

/// The stored helper token, if one was made: what the gate compares the heartbeat's with.
pub(super) fn token(config: &endora_capabilities::ConfigStore) -> Option<String> {
    Some(Settings::load(config).token).filter(|t| !t.is_empty())
}

#[derive(Deserialize)]
pub(super) struct HeartbeatRequest {
    plugged: bool,
    idle_s: u64,
    #[serde(default)]
    model_up: bool,
}

/// `POST /v1/mac/heartbeat`: the Mac's helper says how it is. The reply is the owner's "Mac
/// busy" switch, which the helper obeys by unloading its model.
pub(super) async fn heartbeat(
    State(state): State<AppState>,
    Json(req): Json<HeartbeatRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let now = state.clock.now().unix_millis();
    state.mac.hear(Beat {
        at: now,
        plugged: req.plugged,
        idle_s: req.idle_s,
        model_up: req.model_up,
    });
    let told = state.clone();
    let busy = blocking(move || {
        tell_hq(&told, now);
        Ok(Settings::load(&told.config).busy)
    })
    .await?;
    Ok(Json(json!({ "busy": busy })))
}

/// `GET /v1/mac`: the Mac's state, for the console.
pub(super) async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let now = state.clock.now().unix_millis();
    Json(shown(&state, now))
}

#[derive(Deserialize)]
pub(super) struct SettingsRequest {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    busy: Option<bool>,
    #[serde(default)]
    idle_minutes: Option<u64>,
}

/// Whether every address is on the house's network (and there is at least one).
fn all_at_home(addrs: &[std::net::SocketAddr]) -> bool {
    !addrs.is_empty()
        && addrs
            .iter()
            .all(|a| endora_infrastructure::is_private_endpoint(&format!("http://{a}/")))
}

/// Whether a name in `url` (a laptop's name the router gives out, which follows it when its
/// address changes) resolves, now, only to addresses on the house's network.
async fn resolves_home(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split('/').next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let target = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    match tokio::net::lookup_host(target).await {
        Ok(found) => all_at_home(&found.collect::<Vec<_>>()),
        Err(_) => false,
    }
}

/// `POST /v1/mac/settings`: the owner changes what they sent; the rest stays. The address must
/// be on the house's network, so nothing of the team's can be sent anywhere else. A key left
/// out is kept; an empty one clears it.
pub(super) async fn change(
    State(state): State<AppState>,
    Json(req): Json<SettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(url) = req.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(refused("the Mac's address starts with http:// or https://"));
        }
        if !endora_infrastructure::is_private_endpoint(url) && !resolves_home(url).await {
            return Err(refused("the Mac's address must be on your own network"));
        }
    }
    if req.idle_minutes.is_some_and(|m| m > MOST_IDLE_MINUTES) {
        return Err(refused("the idle time is at most a day (0: no wait)"));
    }
    if req
        .model
        .as_deref()
        .is_some_and(|m| m.chars().count() > 120)
    {
        return Err(refused("the model's name is too long"));
    }
    let now = state.clock.now().unix_millis();
    let before = Settings::load(&state.config).busy;
    let (config, changed) = (state.config.clone(), (req.busy, before));
    blocking(move || {
        if let Some(url) = &req.url {
            set(&config, "url", url.trim().trim_end_matches('/'))?;
        }
        if let Some(key) = &req.api_key {
            set(&config, "api_key", key.trim())?;
        }
        if let Some(model) = &req.model {
            set(&config, "model", model.trim())?;
        }
        if let Some(busy) = req.busy {
            set(&config, "busy", if busy { "true" } else { "false" })?;
        }
        if let Some(minutes) = req.idle_minutes {
            set(&config, "idle_minutes", &minutes.to_string())?;
        }
        Ok(())
    })
    .await?;
    if let (Some(busy), before) = changed {
        if busy != before {
            record(
                &state,
                if busy {
                    "the owner switched the Mac to busy: the team's turns on it wait"
                } else {
                    "the owner switched the Mac off busy"
                },
            );
        }
    }
    // The switch matters at once: HQ is told now, not at the next look.
    let told = state.clone();
    blocking(move || {
        tell_hq(&told, now);
        Ok(())
    })
    .await?;
    Ok(Json(shown(&state, now)))
}

/// `POST /v1/mac/token`: makes the helper's token, replacing any before it, and shows it this
/// once. It opens `POST /v1/mac/heartbeat` and nothing else.
pub(super) async fn make_token(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = format!("{:032x}{:032x}", state.ids.new_id(), state.ids.new_id());
    let (config, kept) = (state.config.clone(), token.clone());
    blocking(move || set(&config, "token", &kept)).await?;
    record(&state, "the owner made a token for the Mac's helper");
    Ok(Json(json!({ "token": token })))
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{Seen, fake};
    use super::super::tests::{get, json_body, post, test_state};
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt as _;

    const NOW: i64 = 1_000_000;

    fn beat(plugged: bool, idle_s: u64, model_up: bool, at: i64) -> Beat {
        Beat {
            at,
            plugged,
            idle_s,
            model_up,
        }
    }

    #[test]
    fn the_mac_is_free_only_when_heard_plugged_in_up_and_the_owner_idle() {
        let ten_min = 600;
        let free = |b: &Beat, busy| availability(NOW, Some(b), busy, ten_min);
        let ok = beat(true, 600, true, NOW - 10_000);
        assert_eq!(free(&ok, false).why, "ready");
        assert!(free(&ok, false).ready);
        assert_eq!(free(&ok, true).why, "busy", "the owner's switch holds it");
        assert_eq!(
            free(&beat(false, 9_000, true, NOW), false).why,
            "on battery"
        );
        assert_eq!(
            free(&beat(true, 9_000, false, NOW), false).why,
            "model loading"
        );
        assert_eq!(
            free(&beat(true, 599, true, NOW), false).why,
            "owner active",
            "a second short of ten minutes"
        );
        assert_eq!(
            availability(NOW, None, false, ten_min).why,
            "not heard from"
        );
        let stale = beat(true, 9_000, true, NOW - 90_001);
        assert_eq!(free(&stale, false).why, "not heard from", "ninety seconds");
        let just = beat(true, 9_000, true, NOW - 90_000);
        assert!(free(&just, false).ready);
    }

    #[test]
    fn busy_is_said_before_battery_and_not_heard_before_both() {
        let b = beat(false, 0, false, NOW);
        assert_eq!(availability(NOW, Some(&b), true, 600).why, "busy");
        assert_eq!(
            availability(NOW + 100_000, Some(&b), true, 600).why,
            "not heard from"
        );
    }

    fn signed(token: &str, method: &str, uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    /// HQ, which takes what the node tells it about the Mac.
    fn hq() -> (String, Seen) {
        fake(std::sync::Arc::new(|_path, _body| {
            (200, "application/json", r#"{"ok":true}"#.to_owned())
        }))
    }

    fn told_hq(seen: &Seen) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/mac")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn the_helpers_token_is_shown_once_and_opens_the_heartbeat_and_nothing_else() {
        let router = super::super::app(test_state());
        let made = json_body(
            router
                .clone()
                .oneshot(post("/v1/mac/token", "{}"))
                .await
                .unwrap(),
        )
        .await;
        let helper = made["token"].as_str().unwrap().to_owned();
        assert_eq!(helper.len(), 64);
        let beat = r#"{"plugged":true,"idle_s":900,"model_up":true}"#;
        let ok = router
            .clone()
            .oneshot(signed(&helper, "POST", "/v1/mac/heartbeat", beat))
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(json_body(ok).await["busy"], false);
        for (method, uri) in [
            ("GET", "/v1/mac"),
            ("POST", "/v1/mac/settings"),
            ("POST", "/v1/mac/token"),
            ("GET", "/v1/chat"),
            ("POST", "/v1/hq/stop"),
        ] {
            let res = router
                .clone()
                .oneshot(signed(&helper, method, uri, "{}"))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
        let wrong = router
            .clone()
            .oneshot(signed(&"0".repeat(64), "POST", "/v1/mac/heartbeat", beat))
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
        let none = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mac/heartbeat")
                    .header("content-type", "application/json")
                    .body(Body::from(beat))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(none.status(), StatusCode::UNAUTHORIZED);
        // A new token replaces the old one.
        let again = json_body(
            router
                .clone()
                .oneshot(post("/v1/mac/token", "{}"))
                .await
                .unwrap(),
        )
        .await;
        assert_ne!(again["token"], made["token"]);
        let old = router
            .clone()
            .oneshot(signed(&helper, "POST", "/v1/mac/heartbeat", beat))
            .await
            .unwrap();
        assert_eq!(
            old.status(),
            StatusCode::UNAUTHORIZED,
            "the old token is dead"
        );
        // The console never gets the token or the key back.
        let console = json_body(router.clone().oneshot(get("/v1/mac")).await.unwrap()).await;
        assert_eq!(console["has_token"], true);
        assert!(
            !console
                .to_string()
                .contains(again["token"].as_str().unwrap())
        );
    }

    #[tokio::test]
    async fn the_node_reads_the_helper_and_answers_with_the_owners_switch() {
        let (hq_url, to_hq) = hq();
        let state = test_state().with_hq(endora_infrastructure::HqClient::new(
            &hq_url,
            &"n".repeat(32),
        ));
        let router = super::super::app(state);
        let start = json_body(router.clone().oneshot(get("/v1/mac")).await.unwrap()).await;
        assert_eq!(start["why"], "not heard from");
        assert_eq!(start["ready"], false);
        assert_eq!(
            start["idle_minutes"], 0,
            "no wait for the owner to be idle unless they ask for one"
        );
        let beat = json!({ "plugged": true, "idle_s": 601, "model_up": true }).to_string();
        let made = json_body(
            router
                .clone()
                .oneshot(post("/v1/mac/token", "{}"))
                .await
                .unwrap(),
        )
        .await;
        let helper = made["token"].as_str().unwrap();
        let reply = json_body(
            router
                .clone()
                .oneshot(signed(helper, "POST", "/v1/mac/heartbeat", &beat))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(reply["busy"], false);
        let now = json_body(router.clone().oneshot(get("/v1/mac")).await.unwrap()).await;
        assert_eq!(now["ready"], true);
        assert_eq!(now["plugged"], true);
        assert_eq!(now["idle_s"], 601);
        assert_eq!(
            told_hq(&to_hq).last().unwrap(),
            &json!({ "ready": true, "why": "ready" })
        );
        // The owner flips the switch from their phone: HQ is told at once, and the next
        // heartbeat's reply carries it.
        let flipped = json_body(
            router
                .clone()
                .oneshot(post("/v1/mac/settings", r#"{"busy":true}"#))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (flipped["busy"].clone(), flipped["why"].clone()),
            (json!(true), json!("busy"))
        );
        assert_eq!(
            told_hq(&to_hq).last().unwrap(),
            &json!({ "ready": false, "why": "busy" })
        );
        let reply = json_body(
            router
                .clone()
                .oneshot(signed(helper, "POST", "/v1/mac/heartbeat", &beat))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(reply["busy"], true);
        // A repeat of the same news is not said to HQ again within the minute.
        let told = told_hq(&to_hq).len();
        router
            .clone()
            .oneshot(signed(helper, "POST", "/v1/mac/heartbeat", &beat))
            .await
            .unwrap();
        assert_eq!(told_hq(&to_hq).len(), told);
    }

    #[test]
    fn a_name_is_at_home_only_when_every_address_it_resolves_to_is() {
        let at = |s: &str| s.parse::<std::net::SocketAddr>().unwrap();
        assert!(super::all_at_home(&[at("192.168.1.10:8080")]));
        assert!(super::all_at_home(&[
            at("192.168.1.10:8080"),
            at("[fd00::10]:8080")
        ]));
        assert!(!super::all_at_home(&[
            at("192.168.1.10:8080"),
            at("8.8.8.8:8080")
        ]));
        assert!(!super::all_at_home(&[]));
    }

    #[tokio::test]
    async fn the_address_must_be_on_the_owners_network_and_the_key_is_kept_sealed_and_unshown() {
        let state = test_state();
        let router = super::super::app(state.clone());
        for url in [
            "https://example.com/v1",
            "http://8.8.8.8:8080/v1",
            "ftp://192.168.1.10/v1",
            // A name that resolves to nothing is not known to be at home.
            "http://no-such-mac.invalid:8080/v1",
        ] {
            let res = router
                .clone()
                .oneshot(post("/v1/mac/settings", &json!({ "url": url }).to_string()))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{url}");
        }
        // No wait at all (0) is the default and may be chosen; more than a day may not.
        for bad in [r#"{"idle_minutes":2000}"#] {
            let res = router
                .clone()
                .oneshot(post("/v1/mac/settings", bad))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        let res = router
            .clone()
            .oneshot(post(
                "/v1/mac/settings",
                r#"{"url":"http://192.168.1.10:8080/v1/","api_key":"the-macs-key","model":"coder","idle_minutes":5}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let shown = json_body(res).await;
        assert_eq!(shown["url"], "http://192.168.1.10:8080/v1");
        assert_eq!(shown["model"], "coder");
        assert_eq!(shown["has_key"], true);
        assert_eq!(shown["idle_minutes"], 5);
        assert!(!shown.to_string().contains("the-macs-key"));
        // Sealed at rest: the stored value is not the key.
        let raw: String = state
            .store
            .db()
            .lock()
            .unwrap()
            .query_row(
                "SELECT value FROM capability_settings WHERE capability_id = 'mac-brain' AND key = 'api_key'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!raw.contains("the-macs-key") && !raw.is_empty(), "{raw}");
        // Left out is kept; empty clears.
        router
            .clone()
            .oneshot(post("/v1/mac/settings", r#"{"busy":false}"#))
            .await
            .unwrap();
        assert_eq!(Settings::load(&state.config).api_key, "the-macs-key");
        router
            .clone()
            .oneshot(post("/v1/mac/settings", r#"{"api_key":""}"#))
            .await
            .unwrap();
        assert_eq!(Settings::load(&state.config).api_key, "");
    }

    #[test]
    fn a_mac_turns_call_goes_only_where_the_owner_set_it_and_only_while_it_is_free() {
        let state = test_state();
        let now = state.clock.now().unix_millis();
        let err = |s: &AppState| target(s, now).err().unwrap_or_default();
        assert!(err(&state).contains("not set up"), "{}", err(&state));
        set(&state.config, "url", "http://192.168.1.10:8080/v1").unwrap();
        set(&state.config, "api_key", "k").unwrap();
        assert!(err(&state).contains("not heard from"));
        state.mac.hear(beat(true, 3_000, true, now));
        let ok = target(&state, now).ok().unwrap();
        assert_eq!(
            (ok.url.as_str(), ok.model.as_str(), ok.key.as_str()),
            ("http://192.168.1.10:8080/v1", "mac", "k")
        );
        // The owner at the Mac does not hold it unless they ask for a wait (the owner,
        // 2026-10-25): by default it is free with them at the keyboard.
        state.mac.hear(beat(true, 3, true, now));
        assert!(target(&state, now).is_ok(), "{}", err(&state));
        set(&state.config, "idle_minutes", "10").unwrap();
        assert!(err(&state).contains("owner active"));
        set(&state.config, "idle_minutes", "0").unwrap();
        state.mac.hear(beat(false, 3_000, true, now));
        assert!(err(&state).contains("on battery"));
        state.mac.hear(beat(true, 3_000, true, now));
        set(&state.config, "busy", "true").unwrap();
        assert!(err(&state).contains("busy"));
    }
}
