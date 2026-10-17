//! HQ's model relay (ADR 0102, phase 4): the only way a team member's turn reaches a model.
//! The worker presents its turn's token as its API key; the relay asks HQ whose turn that is
//! and what it may still spend, caps the reply to what it can afford, adds the owner's
//! Anthropic key (which never leaves the node), and tells HQ what the call cost. A turn past
//! its budget is stopped by HQ, and its token then opens nothing here either.
//!
//! Outside `/v1` on purpose: the owner's sign-in is not the credential here, a turn's token
//! is, and nothing else this node serves answers to one.

use super::*;
use axum::body::{Body, Bytes};
use axum::http::HeaderMap;
use endora_infrastructure::relay::{
    Call, StreamedUsage, Used, affordable_output, cost, price_of, send,
};

/// The most of a whole reply the relay holds at once.
const LARGEST_REPLY: u64 = 64 * 1024 * 1024;

/// An error as Anthropic words one, so the worker's SDK reports it sensibly.
fn refuse(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "type": "error", "error": { "type": kind, "message": message } })),
    )
        .into_response()
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn token_of(headers: &HeaderMap) -> String {
    let bearer =
        header(headers, "authorization").and_then(|v| v.strip_prefix("Bearer ").map(str::to_owned));
    header(headers, "x-api-key")
        .or(bearer)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// The owner's Anthropic key: the provider whose endpoint is Anthropic's (ADR 0105).
fn anthropic_key(config: &endora_capabilities::ConfigStore) -> Option<String> {
    use endora_capabilities::model_lists::ModelListRepository as _;
    config
        .providers()
        .ok()?
        .into_iter()
        .find(|p| p.url.contains("://api.anthropic.com"))
        .map(|p| p.api_key)
        .filter(|k| !k.is_empty())
}

/// Whose turn a token is, and what it may still spend.
struct Turn {
    run: i64,
    left: i64,
}

async fn turn_of(
    hq: Arc<endora_infrastructure::HqClient>,
    token: String,
) -> Result<Turn, Response> {
    if token.is_empty() {
        return Err(refuse(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "no turn token",
        ));
    }
    let asked =
        tokio::task::spawn_blocking(move || hq.post("/v1/team/turn", &json!({ "token": token })))
            .await
            .map_err(|_| {
                refuse(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "api_error",
                    "the relay failed",
                )
            })?;
    match asked {
        Ok(v) => Ok(Turn {
            run: v["run"].as_i64().unwrap_or(0),
            left: v["left"].as_i64().unwrap_or(0),
        }),
        Err(endora_infrastructure::HqError::Answered { status: 404, .. }) => Err(refuse(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "no running turn has that token",
        )),
        Err(_) => Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "HQ did not answer",
        )),
    }
}

/// Tells HQ what a call cost, and notes it in Endora's own use of models. A failure to tell
/// HQ is said in the log: it is never quietly charged to nobody.
fn charge(
    hq: &endora_infrastructure::HqClient,
    state: &AppState,
    run: i64,
    model: &str,
    used: Used,
) {
    let Some(price) = price_of(model, used.prompt()) else {
        return;
    };
    let micros = cost(price, used);
    if let Err(e) = hq.post("/v1/team/usage", &json!({ "run": run, "micros": micros })) {
        eprintln!("relay: could not charge run {run} {micros} micros: {e}");
    }
    endora_application::UsageLog::record(
        state.store.as_ref(),
        "team",
        model,
        endora_application::ModelUsage {
            prompt_tokens: used.prompt(),
            completion_tokens: used.output,
            cached_tokens: used.cache_read,
        },
    );
}

fn whole(status: u16, bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// `POST /team-relay/v1/messages`: a model call of a team member's turn.
pub(super) async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(hq) = state.hq.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "HQ is not set up on this node",
        );
    };
    let turn = match turn_of(hq.clone(), token_of(&headers)).await {
        Ok(turn) => turn,
        Err(refused) => return refused,
    };
    let Ok(mut call) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "the body is not JSON",
        );
    };
    let model = call["model"].as_str().unwrap_or("").to_owned();
    // The dearest tier of its price bounds what the reply may cost before the prompt is known.
    let Some(bound) = price_of(&model, u64::MAX) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            &format!("{model} is not a model the team may use"),
        );
    };
    let affordable = affordable_output(bound, turn.left);
    if affordable == 0 {
        return refuse(
            StatusCode::PAYMENT_REQUIRED,
            "billing_error",
            "this turn's budget is spent",
        );
    }
    let asked = call["max_tokens"].as_u64().unwrap_or(affordable);
    call["max_tokens"] = json!(asked.min(affordable));
    let Some(key) = anthropic_key(state.config.as_ref()) else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "no Anthropic provider is set up on the AI screen",
        );
    };
    let version = header(&headers, "anthropic-version");
    let beta = header(&headers, "anthropic-beta");
    let upstream = state.relay_upstream.clone();
    let streamed = call["stream"].as_bool().unwrap_or(false);
    let sent = call.to_string().into_bytes();

    if !streamed {
        let state2 = state.clone();
        let done = tokio::task::spawn_blocking(move || {
            let reply = send(
                &upstream,
                Call::Messages,
                &key,
                version.as_deref(),
                beta.as_deref(),
                &sent,
            )?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(
                &mut std::io::Read::take(reply.body, LARGEST_REPLY),
                &mut bytes,
            )
            .map_err(|e| e.to_string())?;
            if reply.status < 300 {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    charge(
                        &hq,
                        &state2,
                        turn.run,
                        &model,
                        Used::from_usage(&v["usage"]),
                    );
                }
            }
            Ok::<_, String>((reply.status, bytes))
        })
        .await;
        return match done {
            Ok(Ok((status, bytes))) => whole(status, bytes),
            Ok(Err(why)) => refuse(
                StatusCode::BAD_GATEWAY,
                "api_error",
                &format!("Anthropic did not answer: {why}"),
            ),
            Err(_) => refuse(StatusCode::BAD_GATEWAY, "api_error", "the relay failed"),
        };
    }

    // Streamed: passed on as it arrives, read for its usage on the way, charged at the end.
    let (head_tx, head_rx) = tokio::sync::oneshot::channel::<Result<u16, String>>();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);
    let state2 = state.clone();
    tokio::task::spawn_blocking(move || {
        let mut reply = match send(
            &upstream,
            Call::Messages,
            &key,
            version.as_deref(),
            beta.as_deref(),
            &sent,
        ) {
            Ok(reply) => reply,
            Err(why) => {
                let _ = head_tx.send(Err(why));
                return;
            }
        };
        let _ = head_tx.send(Ok(reply.status));
        let mut usage = StreamedUsage::default();
        let mut buf = [0u8; 8192];
        loop {
            match std::io::Read::read(&mut reply.body, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    usage.feed(&buf[..n]);
                    if tx
                        .blocking_send(Ok(Bytes::copy_from_slice(&buf[..n])))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    break;
                }
            }
        }
        // Charged for what was used, even if the worker stopped listening part way.
        if reply.status < 300 {
            charge(&hq, &state2, turn.run, &model, usage.used());
        }
    });
    let status = match head_rx.await {
        Ok(Ok(status)) => status,
        Ok(Err(why)) => {
            return refuse(
                StatusCode::BAD_GATEWAY,
                "api_error",
                &format!("Anthropic did not answer: {why}"),
            );
        }
        Err(_) => return refuse(StatusCode::BAD_GATEWAY, "api_error", "the relay failed"),
    };
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Response::builder()
        .status(status)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// `POST /team-relay/v1/messages/count_tokens`: free to ask, but only for a running turn.
pub(super) async fn count_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(hq) = state.hq.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "HQ is not set up on this node",
        );
    };
    if let Err(refused) = turn_of(hq, token_of(&headers)).await {
        return refused;
    }
    let Some(key) = anthropic_key(state.config.as_ref()) else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "no Anthropic provider is set up on the AI screen",
        );
    };
    let version = header(&headers, "anthropic-version");
    let beta = header(&headers, "anthropic-beta");
    let upstream = state.relay_upstream.clone();
    let done = tokio::task::spawn_blocking(move || {
        let reply = send(
            &upstream,
            Call::CountTokens,
            &key,
            version.as_deref(),
            beta.as_deref(),
            &body,
        )?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(reply.body, LARGEST_REPLY),
            &mut bytes,
        )
        .map_err(|e| e.to_string())?;
        Ok::<_, String>((reply.status, bytes))
    })
    .await;
    match done {
        Ok(Ok((status, bytes))) => whole(status, bytes),
        _ => refuse(
            StatusCode::BAD_GATEWAY,
            "api_error",
            "Anthropic did not answer",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use endora_infrastructure::{RandomIdSource, ScriptedButler, SqliteStore};
    use http_body_util::BodyExt as _;
    use std::io::{Read as _, Write as _};
    use std::sync::Mutex;
    use tower::ServiceExt as _;

    /// What a fake server was sent: the path, the headers (lower-cased) and the body.
    type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>, String)>>>;
    type Answer = Arc<dyn Fn(&str, &str) -> (u16, &'static str, String) + Send + Sync>;

    /// A tiny HTTP server on a local port that answers each request with `answer`.
    fn fake(answer: Answer) -> (String, Seen) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut head = Vec::new();
                let mut byte = [0u8];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let mut lines = head.lines();
                let path = lines
                    .next()
                    .unwrap_or("")
                    .split(' ')
                    .nth(1)
                    .unwrap_or("")
                    .to_owned();
                let headers: Vec<(String, String)> = lines
                    .filter_map(|l| l.split_once(": "))
                    .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
                    .collect();
                let length: usize = headers
                    .iter()
                    .find(|(k, _)| k == "content-length")
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0u8; length];
                let _ = stream.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body).into_owned();
                let (status, kind, reply) = answer(&path, &body);
                kept.lock().unwrap().push((path, headers, body));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        (url, seen)
    }

    /// HQ, knowing one turn (token `good`, run 7) with `left` micros to spend.
    fn hq(left: i64) -> (String, Seen) {
        fake(Arc::new(move |path, body| match path {
            "/v1/team/turn" if body.contains("\"good\"") => (
                200,
                "application/json",
                json!({ "run": 7, "left": left }).to_string(),
            ),
            "/v1/team/turn" => (
                404,
                "application/json",
                r#"{"error":"no such turn"}"#.to_owned(),
            ),
            _ => (200, "application/json", r#"{"may_go_on":true}"#.to_owned()),
        }))
    }

    /// Anthropic: a whole reply, or a streamed one.
    fn anthropic() -> (String, Seen) {
        fake(Arc::new(|_path, body| {
            if body.contains("\"stream\":true") {
                let sse = [
                    r#"event: message_start"#,
                    r#"data: {"type":"message_start","message":{"usage":{"input_tokens":1200,"cache_read_input_tokens":300,"output_tokens":1}}}"#,
                    "",
                    r#"event: message_delta"#,
                    r#"data: {"type":"message_delta","usage":{"output_tokens":57}}"#,
                    "",
                    r#"event: message_stop"#,
                    r#"data: {"type":"message_stop"}"#,
                    "",
                    "",
                ]
                .join("\n");
                return (200, "text/event-stream", sse);
            }
            (
                200,
                "application/json",
                json!({ "type": "message", "content": [{ "type": "text", "text": "Hello." }],
                        "usage": { "input_tokens": 1000, "output_tokens": 100 } })
                .to_string(),
            )
        }))
    }

    fn node(hq_url: &str, upstream: &str) -> AppState {
        use endora_capabilities::model_lists::ModelListRepository as _;
        let mut state = AppState::new(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
            Arc::new(ScriptedButler),
            Arc::new(endora_understanding::BrainFolder::temporary().unwrap()),
        )
        .with_hq(endora_infrastructure::HqClient::new(
            hq_url,
            &"n".repeat(32),
        ));
        state.relay_upstream = Arc::new(upstream.to_owned());
        state
            .config
            .add_provider(
                "Anthropic",
                "https://api.anthropic.com/v1",
                "sk-ant-the-owners-key",
            )
            .unwrap();
        state
    }

    fn ask(token: &str, body: serde_json::Value) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri("/team-relay/v1/messages")
            .header("content-type", "application/json")
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn charged(seen: &Seen) -> Vec<i64> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/usage")
            .filter_map(|(_, _, b)| {
                serde_json::from_str::<serde_json::Value>(b).ok()?["micros"].as_i64()
            })
            .collect()
    }

    #[tokio::test]
    async fn a_turn_reaches_the_model_with_the_owners_key_capped_and_charged() {
        let (hq_url, to_hq) = hq(40_000);
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::app(node(&hq_url, &upstream));
        let res = app
            .oneshot(ask(
                "good",
                json!({ "model": "claude-sonnet-5-5", "max_tokens": 64_000, "messages": [] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let sent = to_anthropic.lock().unwrap().clone();
        let (path, headers, body) = &sent[0];
        assert_eq!(path, "/v1/messages");
        let key = headers
            .iter()
            .find(|(k, _)| k == "x-api-key")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            key,
            Some("sk-ant-the-owners-key"),
            "the owner's key, never the turn's token"
        );
        let body: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            body["max_tokens"], 4_000,
            "4¢ left buys 4k tokens of Sonnet out"
        );
        assert_eq!(
            charged(&to_hq),
            vec![3_000],
            "1k in at $2 and 100 out at $10 a million"
        );
    }

    #[tokio::test]
    async fn a_streamed_turn_passes_through_and_is_charged_when_it_ends() {
        let (hq_url, to_hq) = hq(400_000);
        let (upstream, _) = anthropic();
        let app = super::super::app(node(&hq_url, &upstream));
        let res = app
            .oneshot(ask("good", json!({ "model": "claude-sonnet-5-5", "max_tokens": 1000, "stream": true, "messages": [] })))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("message_stop"));
        // The charge follows the stream's end.
        for _ in 0..100 {
            if !charged(&to_hq).is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(
            charged(&to_hq),
            vec![3_000],
            "1200 in, 300 read from the cache, 57 out"
        );
    }

    #[tokio::test]
    async fn no_token_no_model_and_no_money_mean_no_call() {
        let (hq_url, _) = hq(40_000);
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::app(node(&hq_url, &upstream));
        let call = |token: &str, model: &str| {
            ask(
                token,
                json!({ "model": model, "max_tokens": 10, "messages": [] }),
            )
        };
        let stranger = app
            .clone()
            .oneshot(call("guessed", "claude-sonnet-5-5"))
            .await
            .unwrap();
        assert_eq!(stranger.status(), StatusCode::UNAUTHORIZED);
        let unpriced = app
            .clone()
            .oneshot(call("good", "claude-fable-5-1"))
            .await
            .unwrap();
        assert_eq!(unpriced.status(), StatusCode::BAD_REQUEST);
        assert!(
            to_anthropic.lock().unwrap().is_empty(),
            "nothing was sent on"
        );
        let (spent_hq, _) = hq(0);
        let broke = super::super::app(node(&spent_hq, &upstream))
            .oneshot(call("good", "claude-sonnet-5-5"))
            .await
            .unwrap();
        assert_eq!(broke.status(), StatusCode::PAYMENT_REQUIRED);
        assert!(to_anthropic.lock().unwrap().is_empty());
    }
}
