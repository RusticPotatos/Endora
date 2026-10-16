//! Spotify, held by the node and lent to every device (plan phase 4). The rules are in
//! `endora_capabilities::infrastructure::spotify`; this is the HTTP edge.

use super::*;
use endora_capabilities::infrastructure::spotify::{
    Accounts, CALLBACK_PATH, Connection, PASTE_REDIRECT,
};

fn refused(message: &str) -> ApiError {
    ApiError(AppError::BadRequest {
        message: message.to_owned(),
    })
}

fn as_status(
    value: serde_json::Value,
) -> Result<Json<endora_protocol::ConnectionStatus>, ApiError> {
    serde_json::from_value(value)
        .map(Json)
        .map_err(|_| refused("Spotify: protocol"))
}

/// Where the connection stands, for every device.
pub(super) async fn status(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::ConnectionStatus>, ApiError> {
    let config = state.config.clone();
    let now = state.clock.now().unix_millis();
    let value = blocking(move || {
        Ok::<_, AppError>(Connection::new(config.as_ref(), &Accounts).status(now))
    })
    .await?;
    as_status(value)
}

#[derive(Deserialize)]
pub(super) struct BeginRequest {
    /// The Spotify app's client id; blank keeps the saved one.
    #[serde(default)]
    client_id: String,
    /// `endora`: Spotify sends the person back to this node, which finishes by itself. Anything
    /// else: back to a loopback address whose address is pasted into a device.
    #[serde(default)]
    via: String,
}

/// The node's own callback address, as the person reaches it (the `Host` they used), which is
/// what must be registered in the Spotify app.
fn callback_address(headers: &axum::http::HeaderMap) -> Option<String> {
    let host = headers.get(axum::http::header::HOST)?.to_str().ok()?;
    if host.is_empty() || host.contains(['/', '@', ' ']) {
        return None;
    }
    let scheme = if std::env::var("ENDORA_TLS").is_ok_and(|v| !v.trim().is_empty() && v != "0") {
        "https"
    } else {
        "http"
    };
    Some(format!("{scheme}://{host}{CALLBACK_PATH}"))
}

/// Starts a sign-in any device can finish.
pub(super) async fn begin(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<BeginRequest>,
) -> Result<Json<endora_protocol::ConnectionStatus>, ApiError> {
    let redirect = if req.via == "endora" {
        callback_address(&headers)
            .ok_or_else(|| refused("Spotify: this address cannot be called back"))?
    } else {
        PASTE_REDIRECT.to_owned()
    };
    // Both random, both kept only here: the verifier never leaves the node.
    // 64 hex characters: within PKCE's 43 to 128.
    let verifier = format!("{:032x}{:032x}", state.ids.new_id(), state.ids.new_id());
    let nonce = format!("{:032x}", state.ids.new_id());
    let config = state.config.clone();
    let now = state.clock.now().unix_millis();
    let value = blocking(move || {
        Connection::new(config.as_ref(), &Accounts)
            .begin(&req.client_id, &redirect, &verifier, &nonce, now)
            .map_err(|m| AppError::BadRequest {
                message: m.to_owned(),
            })
    })
    .await?;
    as_status(value)
}

#[derive(Deserialize)]
pub(super) struct FinishRequest {
    /// The address Spotify sent the person to, or its query.
    returned: String,
}

/// Finishes the waiting sign-in with what came back from Spotify, pasted into any device.
pub(super) async fn finish(
    State(state): State<AppState>,
    Json(req): Json<FinishRequest>,
) -> Result<Json<endora_protocol::ConnectionStatus>, ApiError> {
    let config = state.config.clone();
    let now = state.clock.now().unix_millis();
    let value = blocking(move || {
        Connection::new(config.as_ref(), &Accounts)
            .finish(&req.returned, now)
            .map_err(|m| AppError::BadRequest {
                message: m.to_owned(),
            })
    })
    .await?;
    as_status(value)
}

/// Lends an access token, refreshed when nearly spent. The refresh token stays here.
pub(super) async fn token(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::LentToken>, ApiError> {
    let config = state.config.clone();
    let now = state.clock.now().unix_millis();
    let (access_token, expires_in) = blocking(move || {
        Connection::new(config.as_ref(), &Accounts)
            .lend(now)
            .map_err(|m| AppError::BadRequest {
                message: m.to_owned(),
            })
    })
    .await?;
    Ok(Json(endora_protocol::LentToken {
        access_token,
        expires_in,
    }))
}

#[derive(Deserialize)]
pub(super) struct TakeOverRequest {
    client_id: String,
    refresh_token: String,
}

/// Takes over a sign-in a device already has, once Spotify accepts it.
pub(super) async fn take_over(
    State(state): State<AppState>,
    Json(req): Json<TakeOverRequest>,
) -> Result<Json<endora_protocol::ConnectionStatus>, ApiError> {
    let config = state.config.clone();
    let now = state.clock.now().unix_millis();
    let value = blocking(move || {
        Connection::new(config.as_ref(), &Accounts)
            .take_over(&req.client_id, &req.refresh_token, now)
            .map_err(|m| AppError::BadRequest {
                message: m.to_owned(),
            })
    })
    .await?;
    as_status(value)
}

/// Forgets the sign-in on this node.
pub(super) async fn disconnect(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::ConnectionStatus>, ApiError> {
    let config = state.config.clone();
    let now = state.clock.now().unix_millis();
    let value = blocking(move || {
        Connection::new(config.as_ref(), &Accounts)
            .disconnect(now)
            .map_err(|m| AppError::BadRequest {
                message: m.to_owned(),
            })
    })
    .await?;
    as_status(value)
}

/// Where Spotify sends the person back when the sign-in was started with `via: endora`. Outside
/// `/v1` because a browser redirect carries no bearer token: the single-use `state` of the
/// waiting sign-in is what authorizes it, and a wrong or spent one finishes nothing.
pub(super) async fn callback(
    State(state): State<AppState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> axum::response::Html<String> {
    let config = state.config.clone();
    let now = state.clock.now().unix_millis();
    let query = query.unwrap_or_default();
    let outcome = blocking(move || {
        Ok::<_, AppError>(Connection::new(config.as_ref(), &Accounts).finish(&query, now))
    })
    .await;
    let (title, said) = match outcome {
        Ok(Ok(status)) => (
            "Spotify is connected",
            format!(
                "{}. You can close this and go back to Endora, on any of your devices.",
                status["detail"].as_str().unwrap_or("Connected")
            ),
        ),
        Ok(Err(message)) => ("Spotify is not connected", format!("{message}.")),
        Err(_) => (
            "Spotify is not connected",
            "Endora could not finish it.".to_owned(),
        ),
    };
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    axum::response::Html(format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>\
         <title>{t}</title><body style='font:16px system-ui;margin:2em;max-width:30em'>\
         <h1 style='font-size:1.3em'>{t}</h1><p>{s}</p><p><a href='/'>Back to Endora</a></p></body>",
        t = escape(title),
        s = escape(&said),
    ))
}
