//! The team protocol over MCP (spec section 6): the one door a worker has into HQ. A worker
//! presents its turn's token, never the node's; the token opens only its own turn, while it
//! runs. JSON-RPC over HTTP: `initialize`, `tools/list`, `tools/call`, and `ping`.

use axum::Json;
use axum::extract::State as Extract;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use endora_team::application::protocol;

use crate::api::State;

/// The protocol versions this server speaks, newest first.
const VERSIONS: [&str; 2] = ["2025-06-18", "2025-03-26"];

/// What a message comes to.
enum Outcome {
    /// A result for the request.
    Result(Value),
    /// A notification: nothing to answer.
    Nothing,
    /// A method this server does not have.
    NoSuchMethod,
}

fn reply(id: &Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn error(id: &Value, code: i64, message: &str) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }))
        .into_response()
}

/// One JSON-RPC message from a worker.
pub async fn mcp(
    Extract(state): Extract<State>,
    headers: HeaderMap,
    Json(msg): Json<Value>,
) -> Response {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    let hq = state.hq.clone();
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg["method"].as_str().unwrap_or("").to_owned();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let answer = tokio::task::spawn_blocking(move || {
        // A token that opens no running turn opens nothing at all.
        let Ok(Some((run, member))) = hq.turn_of(&token) else {
            return Err(StatusCode::UNAUTHORIZED);
        };
        Ok(Outcome::Result(match method.as_str() {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or("");
                let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "endora-hq", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Call my_turn first. You are one member of a team: use only these tools, and end your turn when your part is done.",
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": protocol::tools() }),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let (text, failed) = match protocol::call(hq.as_ref(), &run, &member, name, &args) {
                    Ok(text) => (text, false),
                    Err(text) => (text, true),
                };
                json!({ "content": [{ "type": "text", "text": text }], "isError": failed })
            }
            // A notification has no answer.
            m if m.starts_with("notifications/") => return Ok(Outcome::Nothing),
            _ => return Ok(Outcome::NoSuchMethod),
        }))
    })
    .await;
    match answer {
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Ok(Err(status)) => (
            status,
            Json(json!({ "error": "no running turn has that token" })),
        )
            .into_response(),
        Ok(Ok(Outcome::Nothing)) => StatusCode::ACCEPTED.into_response(),
        Ok(Ok(Outcome::NoSuchMethod)) => error(&id, -32601, "no such method"),
        Ok(Ok(Outcome::Result(result))) => reply(&id, result),
    }
}
