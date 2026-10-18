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
        "tools/list" => json!({ "tools": protocol::tools(member.role) }),
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
    let found = tokio::task::spawn_blocking(move || {
        let Some((run, member)) = hq.turn_of(&req.token)? else {
            return Ok(None);
        };
        // What runs the turn (ADR 0117): the relay passes a Claude Code turn's own shape.
        let runtime = hq.runtime_of(member.id).unwrap_or_default();
        Ok::<_, endora_team::application::EngineError>(Some((run, member, runtime)))
    })
    .await;
    match found {
        Ok(Ok(Some((run, member, runtime)))) => Json(json!({
            "run": run.id.0,
            "member": member.name,
            "role": member.role.name(),
            "left": (run.reserved - run.spent).max(0),
            // The check point the node's relays, contracts and bridge ask (ADR 0109): a turn
            // on a cloud model is given no Confidential result beyond what the owner allowed,
            // and only such a turn reaches Anthropic.
            "cloud": member.brain.cloud(),
            "brain": member.brain.name(),
            "runtime": runtime.name(),
            // The task the turn is on (ADR 0115): the node names the wing's branches by it.
            "task": run.task.map(|t| t.0),
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
    /// The call's input tokens, as Anthropic reported them: fresh, read from the prompt cache
    /// and written to it. A relay that says none is taken at none.
    #[serde(default)]
    input: i64,
    #[serde(default)]
    cache_read: i64,
    #[serde(default)]
    cache_write: i64,
    /// The call's output tokens. A relay that says none is taken at none.
    #[serde(default)]
    output: i64,
}

/// For the node's model relay: what one model call of a turn cost. Crossing the turn's
/// budget stops it there.
pub(crate) async fn usage(
    Extract(state): Extract<State>,
    Json(req): Json<UsageReported>,
) -> Response {
    let hq = state.hq.clone();
    let reported = tokio::task::spawn_blocking(move || {
        let run = endora_team::domain::RunId(req.run);
        // The tokens first: a charge that stops the turn ends its record too.
        hq.note_tokens(run, req.input, req.cache_read, req.cache_write, req.output)?;
        hq.report_usage(run, req.micros.max(0))
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

#[derive(serde::Deserialize)]
pub(crate) struct ReceiptTold {
    /// The turn's own token, as its worker presented it to the node.
    token: String,
    tool: String,
    #[serde(default)]
    args: String,
    #[serde(default)]
    excerpt: String,
    ok: bool,
    #[serde(default)]
    act: bool,
    #[serde(default)]
    confidential: bool,
}

/// For the node's contracts and bridge (ADR 0114): one call a turn made of Endora's tools,
/// recorded as a receipt the turn can cite. Signed by the node, and naming the turn by its own
/// token: HQ records it only while that turn runs, so neither a model nor a turn that has ended
/// can make one.
pub(crate) async fn receipt(
    Extract(state): Extract<State>,
    Json(req): Json<ReceiptTold>,
) -> Response {
    let hq = state.hq.clone();
    let recorded = tokio::task::spawn_blocking(move || {
        hq.record_receipt(
            &req.token,
            &endora_team::application::engine::Called {
                tool: req.tool,
                args: req.args,
                excerpt: req.excerpt,
                ok: req.ok,
                act: req.act,
                confidential: req.confidential,
            },
        )
    })
    .await;
    match recorded {
        Ok(Ok(id)) => Json(json!({ "receipt": id })).into_response(),
        Ok(Err(e)) => refused(&e),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct NoticeTold {
    /// The turn it is from, if any.
    #[serde(default)]
    run: Option<i64>,
    text: String,
}

/// For the node's contracts (ADR 0106): something a member did that the owner should know of
/// but need not decide, such as a container restarted, as a notice on the owner's desk.
/// Signed by the node; nothing else may tell it.
pub(crate) async fn notice(
    Extract(state): Extract<State>,
    Json(req): Json<NoticeTold>,
) -> Response {
    if req.text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "a notice says something" })),
        )
            .into_response();
    }
    let hq = state.hq.clone();
    let told = tokio::task::spawn_blocking(move || {
        hq.notice(req.run.map(endora_team::domain::RunId), &req.text)
    })
    .await;
    match told {
        Ok(Ok(item)) => Json(json!({ "item": item.0 })).into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct ApprovalAsked {
    run: i64,
    text: String,
}

/// For the node's bridge (ADR 0108): an action a member proposed that waits for the owner's
/// yes, as an approval on the desk, saying exactly what would run. It ends the member's turn
/// as a question does. Signed by the node; nothing else may ask.
pub(crate) async fn approval(
    Extract(state): Extract<State>,
    Json(req): Json<ApprovalAsked>,
) -> Response {
    if req.text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "an approval says what would run" })),
        )
            .into_response();
    }
    let hq = state.hq.clone();
    let asked = tokio::task::spawn_blocking(move || {
        hq.ask_approval(endora_team::domain::RunId(req.run), &req.text)
    })
    .await;
    match asked {
        Ok(Ok(item)) => Json(json!({ "item": item.0 })).into_response(),
        Ok(Err(e)) => refused(&e),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct OutcomeTold {
    text: String,
}

/// For the node's bridge (ADR 0108): what came of an approved action the node ran, written in
/// the task's thread, which wakes the member who asked. Told once.
pub(crate) async fn approval_outcome(
    Extract(state): Extract<State>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(req): Json<OutcomeTold>,
) -> Response {
    let hq = state.hq.clone();
    let told = tokio::task::spawn_blocking(move || {
        hq.approval_outcome(endora_team::domain::DeskId(id), &req.text)
    })
    .await;
    match told {
        Ok(Ok(())) => Json(json!({ "told": true })).into_response(),
        Ok(Err(e)) => refused(&e),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct BranchTold {
    /// The turn's own token.
    token: String,
    branch: String,
    head: String,
    #[serde(default)]
    base: String,
    #[serde(default)]
    files: usize,
    #[serde(default)]
    plus: i64,
    #[serde(default)]
    minus: i64,
    #[serde(default)]
    stat: String,
    #[serde(default)]
    diff: String,
    #[serde(default)]
    can_push: bool,
}

/// For the node's `repo_submit` (ADR 0115): a branch it took from a turn, recorded with its
/// document and a check job queued. Signed by the node, naming the turn by its own token.
pub(crate) async fn branch(
    Extract(state): Extract<State>,
    Json(req): Json<BranchTold>,
) -> Response {
    let hq = state.hq.clone();
    let recorded = tokio::task::spawn_blocking(move || {
        hq.record_branch(
            &req.token,
            &endora_team::application::Submission {
                branch: req.branch,
                head: req.head,
                base: req.base,
                files: req.files,
                plus: req.plus,
                minus: req.minus,
                stat: req.stat,
                diff: req.diff,
                can_push: req.can_push,
            },
        )
    })
    .await;
    match recorded {
        Ok(Ok(r)) => Json(json!({
            "branch": r.branch, "document": r.document, "check": r.check,
        }))
        .into_response(),
        Ok(Err(e)) => refused(&e),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// For the node's team listener (ADR 0115): the check a job token names, so the node hands
/// that job's sandbox the right bundle and nothing else. 404 for a token that names none.
pub(crate) async fn check(Extract(state): Extract<State>, Json(req): Json<TokenAsked>) -> Response {
    let hq = state.hq.clone();
    let found = tokio::task::spawn_blocking(move || hq.check_by_token(&req.token)).await;
    match found {
        Ok(Ok(Some((check, branch)))) => Json(json!({
            "check": check.id, "task": check.task.0, "branch": branch.name, "head": branch.head,
        }))
        .into_response(),
        Ok(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no check has that token" })),
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct PushOutcome {
    ok: bool,
    text: String,
}

/// For the node (ADR 0115): what came of a push the owner approved, written in the task's
/// thread as a receipt, which wakes the branch's member. Told once.
pub(crate) async fn push_outcome(
    Extract(state): Extract<State>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(req): Json<PushOutcome>,
) -> Response {
    let hq = state.hq.clone();
    let told = tokio::task::spawn_blocking(move || {
        hq.push_outcome(endora_team::domain::DeskId(id), req.ok, &req.text)
    })
    .await;
    match told {
        Ok(Ok(())) => Json(json!({ "told": true })).into_response(),
        Ok(Err(e)) => refused(&e),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The engine's refusal, with its words, as the node reads it.
fn refused(e: &endora_team::application::EngineError) -> Response {
    use endora_team::application::EngineError;
    use endora_team::domain::Refusal;
    let status = match e {
        EngineError::Unknown(_) => StatusCode::NOT_FOUND,
        EngineError::Refused(Refusal::Finished) => StatusCode::CONFLICT,
        EngineError::Refused(_) => StatusCode::BAD_REQUEST,
        EngineError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "error": e.to_string() }))).into_response()
}
