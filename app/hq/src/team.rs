//! The team protocol over MCP (spec section 6): the one door a worker has into HQ. A worker
//! presents its turn's token, never the node's; the token opens only its own turn, while it
//! runs. JSON-RPC over HTTP: `initialize`, `tools/list`, `tools/call`, and `ping`, one at a
//! time or in a batch; notifications and the worker's own responses take no answer.

use axum::Json;
use axum::extract::State as Extract;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use endora_team::application::protocol;

use crate::api::State;

/// The protocol versions this server speaks, newest first.
const VERSIONS: [&str; 2] = ["2025-06-18", "2025-03-26"];

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The answer to one JSON-RPC message, or `None` when it takes none: a notification (no
/// `id`), or the worker's own response to something (no `method`).
fn answer(
    hq: &crate::Hq,
    run: &endora_team::application::ports::Run,
    member: &endora_team::domain::Member,
    msg: &Value,
) -> Option<Value> {
    // A notification has no id, and no answer.
    let id = msg.get("id")?;
    let Some(method) = msg.get("method") else {
        if msg.get("result").is_some() || msg.get("error").is_some() {
            return None;
        }
        return Some(error(id, -32600, "not a JSON-RPC request"));
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method.as_str().unwrap_or("") {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&VERSIONS[0]);
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
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (text, failed) = match protocol::call(hq, run, member, name, &args) {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            json!({ "content": [{ "type": "text", "text": text }], "isError": failed })
        }
        _ => return Some(error(id, -32601, "no such method")),
    };
    Some(reply(id, result))
}

/// One JSON-RPC message from a worker, or a batch of them (protocol version 2025-03-26
/// allows one): answered with one answer, or a batch of answers, or 202 and nothing when
/// nothing in it takes an answer.
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
    let answered = tokio::task::spawn_blocking(move || {
        // A token that opens no running turn opens nothing at all.
        let Ok(Some((run, member))) = hq.turn_of(&token) else {
            return Err(StatusCode::UNAUTHORIZED);
        };
        let Value::Array(batch) = msg else {
            return Ok(answer(hq.as_ref(), &run, &member, &msg));
        };
        if batch.is_empty() {
            return Ok(Some(error(&Value::Null, -32600, "an empty batch")));
        }
        let answers: Vec<Value> = batch
            .iter()
            .filter_map(|m| answer(hq.as_ref(), &run, &member, m))
            .collect();
        if answers.is_empty() {
            return Ok(None);
        }
        Ok(Some(Value::Array(answers)))
    })
    .await;
    match answered {
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Ok(Err(status)) => (
            status,
            Json(json!({ "error": "no running turn has that token" })),
        )
            .into_response(),
        Ok(Ok(None)) => StatusCode::ACCEPTED.into_response(),
        Ok(Ok(Some(answer))) => Json(answer).into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct TokenAsked {
    token: String,
}

/// For the node's model relay: whose turn a token is, while it runs, and what it may still
/// spend. Signed by the node; nothing else may ask.
pub(crate) async fn turn(Extract(state): Extract<State>, Json(req): Json<TokenAsked>) -> Response {
    let hq = state.hq.clone();
    let found = tokio::task::spawn_blocking(move || hq.turn_of(&req.token)).await;
    match found {
        Ok(Ok(Some((run, member)))) => Json(json!({
            "run": run.id.0,
            "member": member.name,
            "role": member.role.name(),
            "left": (run.reserved - run.spent).max(0),
        }))
        .into_response(),
        Ok(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no running turn has that token" })),
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct UsageReported {
    run: i64,
    micros: i64,
}

/// For the node's model relay: what one model call of a turn cost. Crossing the turn's
/// budget stops it there.
pub(crate) async fn usage(
    Extract(state): Extract<State>,
    Json(req): Json<UsageReported>,
) -> Response {
    let hq = state.hq.clone();
    let reported = tokio::task::spawn_blocking(move || {
        hq.report_usage(endora_team::domain::RunId(req.run), req.micros.max(0))
    })
    .await;
    match reported {
        Ok(Ok(may_go_on)) => Json(json!({ "may_go_on": may_go_on })).into_response(),
        Ok(Err(e)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
