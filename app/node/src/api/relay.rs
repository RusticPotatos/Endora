//! HQ's model relay (ADR 0102, phase 4): the only way a team member's turn reaches a model.
//! The worker presents its turn's token as its API key; the relay asks HQ whose turn that is,
//! its member's role and what it may still spend. It passes on only a call it can meter, with
//! a model the role may use; counts the prompt first (free) and takes its cost off; caps the
//! reply to what is left; adds the owner's Anthropic key (which never leaves the node); and
//! tells HQ what the call cost. A turn's calls go one at a time, each after the one before was
//! charged. A turn past its budget is stopped by HQ, and its token then opens nothing here
//! either. Only a turn HQ says is cloud (its member's brain is a Claude model, ADR 0109) is
//! passed on: a turn on the house's model may hold Confidential results, and stays home.
//!
//! Outside `/v1` on purpose: the owner's sign-in is not the credential here, a turn's token
//! is, and nothing else this node serves answers to one.
//!
//! Beside it, the local relay: the same token opens the house's own model, OpenAI's way. The
//! owner's list says which model, the call never leaves the house, and it costs nothing.

use super::*;
use axum::body::{Body, Bytes};
use axum::http::HeaderMap;
use endora_infrastructure::relay::{
    Call, StreamedUsage, Used, affordable_output, asks_to_cache, betas_for_claude_code,
    betas_passed, cost, counted, for_claude_code, may_use_as, most_a_turn_as, price_of,
    prompt_cost, send, send_local, send_metered, unmetered,
};

/// The most of a whole reply the relay holds at once.
const LARGEST_REPLY: u64 = 64 * 1024 * 1024;

/// The largest call the local relay takes: a long conversation is far smaller.
const LARGEST_LOCAL_CALL: usize = 4 * 1024 * 1024;

/// An error as Anthropic words one, so the worker's SDK reports it sensibly.
pub(super) fn refuse(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "type": "error", "error": { "type": kind, "message": message } })),
    )
        .into_response()
}

/// An error as OpenAI words one, for the local relay, which speaks OpenAI's API.
fn refuse_openai(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "message": message, "type": kind } })),
    )
        .into_response()
}

/// How a route words a refusal: [`refuse`] or [`refuse_openai`], or the contracts' own.
pub(super) type Wording = fn(StatusCode, &str, &str) -> Response;

pub(super) fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

pub(super) fn token_of(headers: &HeaderMap) -> String {
    let bearer =
        header(headers, "authorization").and_then(|v| v.strip_prefix("Bearer ").map(str::to_owned));
    header(headers, "x-api-key")
        .or(bearer)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// The owner's Anthropic key: the provider whose endpoint is Anthropic's (ADR 0105).
pub(super) fn anthropic_key(config: &endora_capabilities::ConfigStore) -> Option<String> {
    use endora_capabilities::model_lists::ModelListRepository as _;
    config
        .providers()
        .ok()?
        .into_iter()
        .find(|p| p.url.contains("://api.anthropic.com"))
        .map(|p| p.api_key)
        .filter(|k| !k.is_empty())
}

/// Whose turn a token is: its run, its member's name and role, and what it may still spend.
pub(super) struct Turn {
    pub(super) run: i64,
    pub(super) member: String,
    pub(super) role: String,
    pub(super) left: i64,
    /// The task the turn is on, as HQ says (ADR 0115): what the wing's branches are named by.
    pub(super) task: Option<i64>,
    /// The turn's own token, as it presented it: what the node names the turn by to HQ.
    pub(super) token: String,
    /// Whether the member thinks on a cloud model, as HQ says (`"cloud": true`, ADR 0109):
    /// the check point for Confidential results (ADR 0105, 0108), and the only turns the
    /// Anthropic relay passes on. A turn HQ says nothing of counts as local.
    pub(super) cloud: bool,
    /// The member's brain, as HQ names it (`claude-haiku-5-5`, or `local`): a Claude Code
    /// turn's calls must name it.
    pub(super) brain: String,
    /// Whether Claude Code runs the turn (ADR 0117): its calls pass in Claude Code's own
    /// shape, held to the brain, and its sandbox may report its own tools. An older HQ says
    /// nothing: Endora's worker.
    pub(super) claude_code: bool,
    /// Whether the member changes the lab, as HQ says (ADR 0120): only the engineering wing's
    /// lead and developers are offered and run a branch's submission. An older HQ says nothing,
    /// and its role alone decides, as before.
    pub(super) wing: Option<bool>,
    /// The node's tools the turn may use, when HQ narrows them (ADR 0115, amended: the check
    /// after a deploy reads the lab's health, logs and metrics alone). `None`: what its role
    /// and brain give it, as before.
    pub(super) only: Option<Vec<String>>,
    /// The node's tools HQ refuses the turn whatever else it may use: `restart_container` on
    /// a task that says the lab is to be left as it is. Empty from an older HQ.
    pub(super) never: Vec<String>,
}

impl Turn {
    /// Whether HQ lets this turn use the node's tool `name` at all.
    pub(super) fn may_use(&self, name: &str) -> bool {
        if self.never.iter().any(|n| n == name) {
            return false;
        }
        self.only
            .as_ref()
            .is_none_or(|only| only.iter().any(|o| o == name))
    }
}

impl Turn {
    /// Where the turn thinks, as the bridge tells it apart (ADR 0131, amended): the house's own
    /// model on the NAS, the owner's Mac, or a Claude. A turn HQ says nothing of is the NAS's.
    pub(super) fn thinks(&self) -> endora_capabilities::bridge::Thinks {
        endora_capabilities::bridge::Thinks::of(self.cloud, &self.brain)
    }
}

/// A turn's calls to Anthropic, one at a time, by token. HQ knows only what has been
/// charged, so two calls side by side would each be capped by the whole of what is left; one
/// at a time, each is capped by what the one before left.
///
/// Beside that, what each turn's last call left in Anthropic's prompt cache, so the next
/// call's prompt is reckoned at the cache's price for the part it repeats.
#[derive(Default)]
pub(crate) struct TurnCalls {
    held: std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    cached: std::sync::Mutex<std::collections::HashMap<String, Cached>>,
}

/// How long after a call the relay trusts that what it cached is still there: inside the
/// cache's five minutes, with a minute to spare.
const CACHE_TRUSTED: std::time::Duration = std::time::Duration::from_secs(4 * 60);

/// What a turn's last call left in the cache: its prompt's fixed part (the model and all
/// that comes before the messages, or changes how they are read), its messages, and how many
/// tokens its reply said the cache holds (read or written), and when.
struct Cached {
    head: serde_json::Value,
    messages: Vec<serde_json::Value>,
    tokens: u64,
    at: std::time::Instant,
}

/// A call's fixed part, which must be the same for its cache to be read.
fn head_of(call: &serde_json::Value) -> serde_json::Value {
    let mut head = serde_json::Map::new();
    for field in [
        "model",
        "system",
        "tools",
        "tool_choice",
        "thinking",
        "output_config",
    ] {
        if let Some(v) = call.get(field) {
            head.insert(field.to_owned(), v.clone());
        }
    }
    serde_json::Value::Object(head)
}

impl TurnCalls {
    /// Notes what `call`, of the turn `token` opens, left in the cache, by its reply's usage.
    /// A call that cached nothing leaves nothing to count on.
    fn remember(&self, token: &str, call: &serde_json::Value, used: Used) {
        let mut cached = self
            .cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cached.retain(|_, c| c.at.elapsed() < CACHE_TRUSTED);
        let tokens = used.cache_read + used.cache_write;
        if tokens == 0 {
            cached.remove(token);
            return;
        }
        cached.insert(
            token.to_owned(),
            Cached {
                head: head_of(call),
                messages: call["messages"].as_array().cloned().unwrap_or_default(),
                tokens,
                at: std::time::Instant::now(),
            },
        );
    }

    /// How many of `call`'s prompt tokens the turn's last call left in the cache: all it
    /// cached, when `call` repeats its fixed part and goes on from its messages within
    /// [`CACHE_TRUSTED`]; none otherwise.
    fn cached_for(&self, token: &str, call: &serde_json::Value) -> u64 {
        let cached = self
            .cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(last) = cached.get(token) else {
            return 0;
        };
        let messages = call["messages"].as_array().map_or(&[][..], Vec::as_slice);
        let goes_on = messages.starts_with(&last.messages) && head_of(call) == last.head;
        if !goes_on || last.at.elapsed() >= CACHE_TRUSTED {
            return 0;
        }
        last.tokens
    }

    /// Waits until the turn `token` opens has no call in flight, and holds it while the
    /// guard lives: until the call has been charged.
    async fn one_at_a_time(&self, token: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let turn = {
            let mut turns = self
                .held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // A turn no call holds or waits for is forgotten.
            turns.retain(|_, held| Arc::strong_count(held) > 1);
            turns.entry(token.to_owned()).or_default().clone()
        };
        turn.lock_owned().await
    }
}

/// Asks HQ whose turn `token` is; a token that opens nothing is refused in the route's own
/// words.
pub(super) async fn turn_of(
    hq: Arc<endora_infrastructure::HqClient>,
    token: String,
    refuse: Wording,
) -> Result<Turn, Response> {
    if token.is_empty() {
        return Err(refuse(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "no turn token",
        ));
    }
    let presented = token.clone();
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
            member: v["member"].as_str().unwrap_or("").to_owned(),
            role: v["role"].as_str().unwrap_or("").to_owned(),
            left: v["left"].as_i64().unwrap_or(0),
            task: v["task"].as_i64(),
            token: presented,
            cloud: v["cloud"].as_bool().unwrap_or(false),
            brain: v["brain"].as_str().unwrap_or("local").to_owned(),
            claude_code: v["runtime"] == "claude-code",
            wing: v["wing"].as_bool(),
            only: v["only"].as_array().map(|only| {
                only.iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            }),
            never: v["never"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect(),
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
    // What is left with the provider (the owner, 2026-10-30): counted, and told when low.
    super::credits::after_charge(state, run, model, micros);
    let reported = json!({ "run": run, "micros": micros, "input": used.input,
                           "cache_read": used.cache_read, "cache_write": used.cache_write,
                           "output": used.output });
    if let Err(e) = hq.post("/v1/team/usage", &reported) {
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

/// Asks Anthropic how many tokens `asked` (a call's prompt, as [`counted`] makes it) comes to.
/// Counting is free, and tells what the call's input will cost before it is sent. Anthropic's
/// own refusal is passed on as it said it.
pub(super) async fn count(
    upstream: Arc<String>,
    key: String,
    version: Option<String>,
    beta: Option<String>,
    asked: serde_json::Value,
) -> Result<u64, Response> {
    let done = tokio::task::spawn_blocking(move || {
        let reply = send(
            &upstream,
            Call::CountTokens,
            &key,
            version.as_deref(),
            beta.as_deref(),
            asked.to_string().as_bytes(),
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
    let Ok(Ok((status, bytes))) = done else {
        return Err(refuse(
            StatusCode::BAD_GATEWAY,
            "api_error",
            "Anthropic did not answer",
        ));
    };
    if status >= 300 {
        return Err(whole(status, bytes));
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v["input_tokens"].as_u64())
        .ok_or_else(|| {
            refuse(
                StatusCode::BAD_GATEWAY,
                "api_error",
                "Anthropic did not count the prompt",
            )
        })
}

fn whole(status: u16, bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// The refusal of a call to Claude from a turn HQ says thinks on the house's model (ADR 0109):
/// such a turn may have been given Confidential results a cloud turn may not see, so nothing
/// of it leaves the house, whatever the worker sends.
fn stays_home() -> Response {
    refuse(
        StatusCode::FORBIDDEN,
        "permission_error",
        "this member thinks with the house's own model: its turns do not reach Claude",
    )
}

/// Whether `call` may pass on for `turn`, made fit to: Endora's worker's calls by the strict
/// metering rule ([`unmetered`]); a Claude Code turn's in Claude Code's own shape, held to the
/// member's brain and the fence ([`for_claude_code`], ADR 0117).
fn passable(turn: &Turn, call: &mut serde_json::Value) -> Result<(), String> {
    if turn.claude_code {
        return for_claude_code(call, &turn.brain);
    }
    unmetered(call).map_or(Ok(()), Err)
}

/// The betas passed on for `turn`: Endora's worker's known few, or Claude Code's own but
/// those that cost or reach out (ADR 0117).
fn betas_for(turn: &Turn, header: Option<&str>) -> Option<String> {
    if turn.claude_code {
        return betas_for_claude_code(header);
    }
    betas_passed(header)
}

/// `POST /team-relay/v1/messages`: a model call of a team member's turn.
pub(super) async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let token = token_of(&headers);
    // The bench's credential is no turn of HQ's (ADR 0123): its calls never reach HQ.
    if let Some(bench) = super::bench::opened_by(&state, &token) {
        return super::bench::messages(state, bench, headers, body).await;
    }
    let Some(hq) = state.hq.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "HQ is not set up on this node",
        );
    };
    // Held until this call is charged, so the turn's next call sees what this one cost.
    let one_at_a_time = state.relay_calls.one_at_a_time(&token).await;
    let turn = match turn_of(hq.clone(), token.clone(), refuse).await {
        Ok(turn) => turn,
        Err(refused) => return refused,
    };
    if !turn.cloud {
        return stays_home();
    }
    let Ok(mut call) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "the body is not JSON",
        );
    };
    if let Err(why) = passable(&turn, &mut call) {
        return refuse(StatusCode::BAD_REQUEST, "invalid_request_error", &why);
    }
    let model = call["model"].as_str().unwrap_or("").to_owned();
    let Some(cheapest) = price_of(&model, 0) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            &format!("{model} is not a model the team may use"),
        );
    };
    if let Err(why) = may_use_as(&turn.role, &turn.brain, &model) {
        return refuse(StatusCode::FORBIDDEN, "permission_error", &why);
    }
    // Never more than the role's turn may spend (ADR 0111), whatever HQ says is left.
    let left = turn.left.min(most_a_turn_as(&turn.role, &turn.brain));
    let spent = || {
        refuse(
            StatusCode::PAYMENT_REQUIRED,
            "billing_error",
            "this turn's budget is spent",
        )
    };
    if affordable_output(cheapest, left) == 0 {
        return spent();
    }
    let Some(key) = anthropic_key(state.config.as_ref()) else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "no Anthropic provider is set up on the AI screen",
        );
    };
    let version = header(&headers, "anthropic-version");
    let beta = betas_for(&turn, header(&headers, "anthropic-beta").as_deref());
    let upstream = state.relay_upstream.clone();
    // The prompt is paid for whatever the reply, so its cost comes off first, and the reply
    // may spend only what is left after it.
    let prompt = match count(
        upstream.clone(),
        key.clone(),
        version.clone(),
        beta.clone(),
        counted(&call),
    )
    .await
    {
        Ok(prompt) => prompt,
        Err(refused) => return refused,
    };
    let Some(price) = price_of(&model, prompt) else {
        return spent();
    };
    // A call that asks to cache reads what the turn's last call cached at the cache's price,
    // and writes the rest at the cache's (dearer) one. One that does not reads no cache.
    let caching = asks_to_cache(&call);
    let cached = if caching {
        state.relay_calls.cached_for(&token, &call)
    } else {
        0
    };
    let input = prompt_cost(price, prompt, cached, caching);
    let affordable = affordable_output(price, left - input);
    if affordable == 0 {
        return spent();
    }
    let cap = call["max_tokens"]
        .as_u64()
        .unwrap_or(affordable)
        .min(affordable);
    call["max_tokens"] = json!(cap);
    let state2 = state.clone();
    let sent = call.clone();
    let settle: Settle = Box::new(move |used| {
        // Held until this call is charged, so the turn's next call sees what this one cost.
        let _one_at_a_time = one_at_a_time;
        let Some(used) = used else { return };
        charge(&hq, &state2, turn.run, &model, used);
        state2.relay_calls.remember(&token, &call, used);
    });
    pass_on(upstream, key, version, beta, sent, settle).await
}

/// What is done once a call passed on has been answered: told what it used, or `None` when it
/// never reached Anthropic or Anthropic refused it (and so cost nothing). Called exactly once.
pub(super) type Settle = Box<dyn FnOnce(Option<Used>) + Send>;

/// Sends `call` (its `max_tokens` already capped) to Anthropic with the owner's `key`, whole
/// or streamed as it asks, passes the reply back, and [`Settle`]s it once its usage is known: a
/// whole reply's at once, a stream's when it ends. A stream cut before its last usage said how
/// much it sent is settled as all it may have sent.
pub(super) async fn pass_on(
    upstream: Arc<String>,
    key: String,
    version: Option<String>,
    beta: Option<String>,
    call: serde_json::Value,
    settle: Settle,
) -> Response {
    let cap = call["max_tokens"].as_u64().unwrap_or(0);
    let streamed = call["stream"].as_bool().unwrap_or(false);
    let sent = call.to_string().into_bytes();

    if !streamed {
        let done = tokio::task::spawn_blocking(move || {
            let answered = send(
                &upstream,
                Call::Messages,
                &key,
                version.as_deref(),
                beta.as_deref(),
                &sent,
            )
            .and_then(|reply| {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(reply.body, LARGEST_REPLY),
                    &mut bytes,
                )
                .map_err(|e| e.to_string())?;
                Ok((reply.status, bytes))
            });
            let used = match &answered {
                Ok((status, bytes)) if *status < 300 => {
                    serde_json::from_slice::<serde_json::Value>(bytes)
                        .ok()
                        .map(|v| Used::from_usage(&v["usage"]))
                }
                _ => None,
            };
            settle(used);
            answered
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

    // Streamed: passed on as it arrives, read for its usage on the way, settled at the end.
    let (head_tx, head_rx) = tokio::sync::oneshot::channel::<Result<u16, String>>();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(32);
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
                settle(None);
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
        // Charged for what was used, even if the caller stopped listening part way.
        if reply.status >= 300 {
            settle(None);
            return;
        }
        let mut used = usage.used();
        if !usage.finished() {
            used.output = used.output.max(cap);
        }
        settle(Some(used));
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
    if let Some(bench) = super::bench::opened_by(&state, &token_of(&headers)) {
        return super::bench::count_tokens(state, bench, headers, body).await;
    }
    let Some(hq) = state.hq.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "HQ is not set up on this node",
        );
    };
    let turn = match turn_of(hq, token_of(&headers), refuse).await {
        Ok(turn) if !turn.cloud => return stays_home(),
        Ok(turn) => turn,
        Err(refused) => return refused,
    };
    let Ok(mut asked) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "the body is not JSON",
        );
    };
    // Free to count, but what reaches past the fence is refused here too.
    if let Err(why) = passable(&turn, &mut asked) {
        return refuse(StatusCode::BAD_REQUEST, "invalid_request_error", &why);
    }
    let beta = betas_for(&turn, header(&headers, "anthropic-beta").as_deref());
    count_on(&state, &headers, &asked, beta).await
}

/// Asks Anthropic to count `asked`, a call already made fit to pass on, with the owner's key
/// and the `beta` header allowed, and passes its answer back.
pub(super) async fn count_on(
    state: &AppState,
    headers: &HeaderMap,
    asked: &serde_json::Value,
    beta: Option<String>,
) -> Response {
    // Counted as it will be sent: a Claude Code call's hours made five minutes.
    let body = asked.to_string().into_bytes();
    let Some(key) = anthropic_key(state.config.as_ref()) else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "no Anthropic provider is set up on the AI screen",
        );
    };
    let version = header(headers, "anthropic-version");
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

/// Where a local call goes, and with which model and key.
pub(super) struct Local {
    pub(super) url: String,
    pub(super) model: String,
    key: String,
}

/// The first enabled Everyday entry on the house's network, else the model the node was
/// installed with. A list that cannot be read is an empty one: the installed model still
/// answers.
pub(super) fn local_target(state: &AppState) -> Local {
    use endora_capabilities::model_lists::{Job, ModelListRepository as _, targets};
    let providers = state.config.providers().unwrap_or_default();
    let everyday = state.config.entries(Job::Everyday).unwrap_or_default();
    let Some(entry) = targets(&providers, &everyday)
        .into_iter()
        .find(|t| endora_infrastructure::is_private_endpoint(&t.url))
    else {
        let (url, model) = state.installed_model.as_ref().clone();
        return Local {
            url,
            model,
            key: String::new(),
        };
    };
    Local {
        url: entry.url,
        model: entry.model,
        key: entry.api_key,
    }
}

/// Where a DeepSeek member's call goes: the first enabled Everyday entry that names `model`.
/// None when the owner's list has none: the turn is answered by nothing else.
fn deepseek_target(state: &AppState, model: &str) -> Option<Local> {
    use endora_capabilities::model_lists::{Job, ModelListRepository as _, targets};
    let providers = state.config.providers().unwrap_or_default();
    let everyday = state.config.entries(Job::Everyday).unwrap_or_default();
    let found = targets(&providers, &everyday)
        .into_iter()
        .find(|t| t.model == model)?;
    Some(Local {
        url: found.url,
        model: found.model,
        key: found.api_key,
    })
}

/// `POST /team-relay/local/v1/chat/completions`: a model call of a team member's turn, to the
/// house's own model, whole (never streamed). It is asked with the model the owner's list
/// names, whatever the worker named. Local costs nothing, so HQ is charged nothing; the use is
/// noted in Endora's own log as `team-local`.
///
/// The team's turns always think (ADR 0105): a member's round is worth the wait, whatever the
/// entry's switch says for the butler, so a worker's `"reasoning_effort": "none"` is taken off
/// and never added.
pub(super) async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Some(hq) = state.hq.clone() else {
        return refuse_openai(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "HQ is not set up on this node",
        );
    };
    // The token first, so a stranger cannot make the node read a body at all.
    let turn = match turn_of(hq.clone(), token_of(&headers), refuse_openai).await {
        Ok(turn) => turn,
        Err(refused) => return refused,
    };
    // A member who thinks with the owner's Mac (ADR 0130) is answered by the Mac, only while it
    // is free, and by nothing else: no other model, no Claude, not the house's own.
    let on_the_mac = turn.brain == "mac";
    // A member who thinks with DeepSeek (the owner, 2026-10-28) is answered by the DeepSeek
    // entry on the Everyday list alone, its output held to what the turn can afford, and each
    // call charged to HQ at DeepSeek's prices, as the Anthropic relay charges Claude's.
    let on_deepseek = turn.brain.starts_with("deepseek");
    let mut afford = None;
    if on_deepseek {
        let Some(price) = price_of(&turn.brain, 0) else {
            return refuse_openai(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &format!("{} is not a model the team may use", turn.brain),
            );
        };
        let left = turn.left.min(most_a_turn_as(&turn.role, &turn.brain));
        let most = affordable_output(price, left);
        if most == 0 {
            return refuse_openai(
                StatusCode::PAYMENT_REQUIRED,
                "billing_error",
                "this turn's budget is spent",
            );
        }
        afford = Some(most);
    }
    let Ok(body) = axum::body::to_bytes(body, LARGEST_LOCAL_CALL).await else {
        return refuse_openai(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request_error",
            "the call is larger than the relay takes",
        );
    };
    let Ok(serde_json::Value::Object(mut call)) = serde_json::from_slice(&body) else {
        return refuse_openai(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "the body is not a JSON object",
        );
    };
    if call.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        return refuse_openai(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "the local relay does not stream: ask with \"stream\": false",
        );
    }
    let (run, brain) = (turn.run, turn.brain.clone());
    let done = tokio::task::spawn_blocking(move || {
        let local = if on_deepseek {
            let Some(found) = deepseek_target(&state, &brain) else {
                let said = json!({ "error": { "message": format!("{brain} is not on the Everyday list"), "type": "api_error" } });
                return Ok::<_, String>((503, said.to_string().into_bytes()));
            };
            found
        } else if on_the_mac {
            let now = state.clock.now().unix_millis();
            match mac::target(&state, now) {
                Ok(mac) => Local {
                    url: mac.url,
                    model: mac.model,
                    key: mac.key,
                },
                // The turn ends with this, and is tried again when the Mac is free.
                Err(why) => {
                    let said = json!({ "error": { "message": why, "type": "api_error" } });
                    return Ok::<_, String>((503, said.to_string().into_bytes()));
                }
            }
        } else {
            local_target(&state)
        };
        call.insert("model".to_owned(), json!(local.model));
        if let Some(most) = afford {
            let asked = call
                .get("max_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(most);
            call.insert("max_tokens".to_owned(), json!(asked.min(most)));
        }
        if call
            .get("reasoning_effort")
            .and_then(serde_json::Value::as_str)
            == Some("none")
        {
            call.remove("reasoning_effort");
        }
        let sent = serde_json::Value::Object(call).to_string().into_bytes();
        // DeepSeek is metered and leaves the house; everything else here is free and stays.
        let (status, bytes) = if on_deepseek {
            send_metered(&local.url, &local.key, &sent)?
        } else {
            send_local(&local.url, &local.key, &sent)?
        };
        if status < 300 {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if on_deepseek {
                    charge(&hq, &state, run, &local.model, Used::from_openai(&v["usage"]));
                    return Ok((status, bytes));
                }
                let used = |k: &str| v["usage"][k].as_u64().unwrap_or(0);
                endora_application::UsageLog::record(
                    state.store.as_ref(),
                    if on_the_mac { "team-mac" } else { "team-local" },
                    &local.model,
                    endora_application::ModelUsage {
                        prompt_tokens: used("prompt_tokens"),
                        completion_tokens: used("completion_tokens"),
                        cached_tokens: 0,
                    },
                );
            }
        }
        Ok::<_, String>((status, bytes))
    })
    .await;
    match done {
        Ok(Ok((status, bytes))) => whole(status, bytes),
        Ok(Err(why)) => refuse_openai(
            StatusCode::BAD_GATEWAY,
            "api_error",
            &format!(
                "the {} did not answer: {why}",
                if on_the_mac {
                    "Mac"
                } else if on_deepseek {
                    "DeepSeek model"
                } else {
                    "local model"
                }
            ),
        ),
        Err(_) => refuse_openai(StatusCode::BAD_GATEWAY, "api_error", "the relay failed"),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use endora_infrastructure::{RandomIdSource, SqliteStore};
    use http_body_util::BodyExt as _;
    use std::io::{Read as _, Write as _};
    use std::sync::Mutex;
    use tower::ServiceExt as _;

    /// What a fake server was sent: the path, the headers (lower-cased) and the body.
    pub(in crate::api) type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>, String)>>>;
    pub(in crate::api) type Answer =
        Arc<dyn Fn(&str, &str) -> (u16, &'static str, String) + Send + Sync>;

    /// A tiny HTTP server on a local port that answers each request with `answer`.
    pub(in crate::api) fn fake(answer: Answer) -> (String, Seen) {
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

    /// HQ, knowing one turn (token `good`, run 7, a developer's thinking with Claude) with
    /// `left` micros to spend.
    fn hq(left: i64) -> (String, Seen) {
        hq_thinking(left, "developer", true)
    }

    /// HQ, knowing one turn (Linus's) of a member in `role`, on the house's model.
    pub(in crate::api) fn hq_as(left: i64, role: &'static str) -> (String, Seen) {
        hq_thinking(left, role, false)
    }

    /// HQ, knowing one turn (Linus's) of a member in `role`, thinking with Claude when
    /// `cloud`, as HQ says it (ADR 0109).
    pub(in crate::api) fn hq_thinking(
        left: i64,
        role: &'static str,
        cloud: bool,
    ) -> (String, Seen) {
        fake(Arc::new(move |path, body| match path {
            "/v1/team/turn" if body.contains("\"good\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Linus", "role": role, "left": left, "cloud": cloud })
                    .to_string(),
            ),
            "/v1/team/turn" => (
                404,
                "application/json",
                r#"{"error":"no such turn"}"#.to_owned(),
            ),
            _ => (200, "application/json", r#"{"may_go_on":true}"#.to_owned()),
        }))
    }

    /// HQ whose turn has `left` micros, less each charge as it lands, as the real one.
    fn hq_spending(left: i64) -> (String, Seen) {
        let left = Arc::new(std::sync::atomic::AtomicI64::new(left));
        fake(Arc::new(move |path, body| {
            use std::sync::atomic::Ordering::SeqCst;
            if path == "/v1/team/usage" {
                let micros = serde_json::from_str::<serde_json::Value>(body).unwrap()["micros"]
                    .as_i64()
                    .unwrap();
                left.fetch_sub(micros, SeqCst);
                return (200, "application/json", r#"{"may_go_on":true}"#.to_owned());
            }
            (
                200,
                "application/json",
                json!({ "run": 7, "role": "developer", "left": left.load(SeqCst), "cloud": true })
                    .to_string(),
            )
        }))
    }

    /// Anthropic: a whole reply, or a streamed one; a prompt counts 1,000 tokens.
    pub(in crate::api) fn anthropic() -> (String, Seen) {
        anthropic_counting(1_000)
    }

    /// Anthropic, counting every prompt as `prompt` tokens.
    pub(in crate::api) fn anthropic_counting(prompt: u64) -> (String, Seen) {
        fake(Arc::new(move |path, body| {
            if path == "/v1/messages/count_tokens" {
                return (
                    200,
                    "application/json",
                    json!({ "input_tokens": prompt }).to_string(),
                );
            }
            if body.contains("\"stop_sequences\":[\"cut\"]") {
                let sse = [
                    r#"event: message_start"#,
                    r#"data: {"type":"message_start","message":{"usage":{"input_tokens":1200,"cache_read_input_tokens":300,"output_tokens":1}}}"#,
                    "",
                    "",
                ]
                .join("\n");
                return (200, "text/event-stream", sse);
            }
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

    pub(in crate::api) fn node(hq_url: &str, upstream: &str) -> AppState {
        use endora_capabilities::model_lists::ModelListRepository as _;
        let mut state = AppState::new(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
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

    /// The bodies of the message calls Anthropic was sent, not the counting.
    pub(in crate::api) fn messages_sent(seen: &Seen) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/messages")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect()
    }

    /// Waits for `n` charges to reach HQ: a streamed call's follows its end.
    async fn charges(seen: &Seen, n: usize) -> Vec<i64> {
        for _ in 0..200 {
            if charged(seen).len() >= n {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        charged(seen)
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
    async fn the_relays_and_contracts_are_served_only_on_the_team_listener() {
        let (hq_url, _) = hq(40_000);
        let (upstream, to_anthropic) = anthropic();
        let main = super::super::app(node(&hq_url, &upstream));
        for (method, uri) in [
            ("POST", "/team-relay/v1/messages"),
            ("POST", "/team-relay/v1/messages/count_tokens"),
            ("POST", "/team-relay/local/v1/chat/completions"),
            ("GET", "/team-relay/contracts"),
            ("POST", "/team-relay/contracts/homelab_status"),
        ] {
            let req = axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .header("x-api-key", "good")
                .header("authorization", "Bearer good")
                .body(Body::from("{}"))
                .unwrap();
            let res = main.clone().oneshot(req).await.unwrap();
            assert_eq!(
                res.status(),
                StatusCode::NOT_FOUND,
                "{uri} on the main port"
            );
        }
        assert!(to_anthropic.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_turn_reaches_the_model_with_the_owners_key_capped_and_charged() {
        let (hq_url, to_hq) = hq(40_000);
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let res = app
            .oneshot(ask(
                "good",
                json!({ "model": "claude-sonnet-5-5", "max_tokens": 64_000, "messages": [] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let sent = to_anthropic.lock().unwrap().clone();
        let paths: Vec<&str> = sent.iter().map(|(p, _, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            ["/v1/messages/count_tokens", "/v1/messages"],
            "the prompt is counted first"
        );
        let counted: serde_json::Value = serde_json::from_str(&sent[0].2).unwrap();
        assert!(
            counted.get("max_tokens").is_none(),
            "counting takes no max_tokens: {counted}"
        );
        for (_, headers, _) in &sent {
            let key = headers
                .iter()
                .find(|(k, _)| k == "x-api-key")
                .map(|(_, v)| v.as_str());
            assert_eq!(
                key,
                Some("sk-ant-the-owners-key"),
                "the owner's key, never the turn's token"
            );
        }
        assert_eq!(
            messages_sent(&to_anthropic)[0]["max_tokens"],
            3_800,
            "4¢ left, less 1k tokens in at $2, buys 3.8k tokens of Sonnet out"
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
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let res = app
            .oneshot(ask("good", json!({ "model": "claude-sonnet-5-5", "max_tokens": 1000, "stream": true, "messages": [] })))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("message_stop"));
        assert_eq!(
            charges(&to_hq, 1).await,
            vec![3_000],
            "1200 in at $2, 300 read from the cache at 10¢, 57 out at $10"
        );
    }

    #[tokio::test]
    async fn a_stream_cut_before_its_output_is_counted_is_charged_its_whole_cap() {
        let (hq_url, to_hq) = hq(400_000);
        let (upstream, _) = anthropic();
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let res = app
            .oneshot(ask(
                "good",
                json!({ "model": "claude-sonnet-5-5", "max_tokens": 1000, "stream": true,
                "stop_sequences": ["cut"], "messages": [] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let _ = res.into_body().collect().await;
        assert_eq!(
            charges(&to_hq, 1).await,
            vec![12_430],
            "1200 in, 300 read, and all 1,000 tokens it may have sent"
        );
    }

    #[tokio::test]
    async fn a_big_prompt_on_little_budget_never_reaches_the_model() {
        let (hq_url, to_hq) = hq(100_000);
        let (upstream, to_anthropic) = anthropic_counting(200_000);
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let res = app
            .oneshot(ask(
                "good",
                json!({ "model": "claude-sonnet-5-5", "max_tokens": 10, "messages": [] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYMENT_REQUIRED);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let said: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(said["error"]["type"], "billing_error");
        assert!(
            messages_sent(&to_anthropic).is_empty(),
            "200k tokens in at $2 is 40¢, and 10¢ is left"
        );
        assert!(charged(&to_hq).is_empty(), "counting is free");
    }

    #[tokio::test]
    async fn a_turns_calls_go_one_at_a_time_each_capped_by_what_is_left() {
        let (hq_url, _) = hq_spending(40_000);
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let call = || {
            ask(
                "good",
                json!({ "model": "claude-sonnet-5-5", "max_tokens": 64_000, "messages": [] }),
            )
        };
        let (first, second) =
            tokio::join!(app.clone().oneshot(call()), app.clone().oneshot(call()));
        assert_eq!(first.unwrap().status(), StatusCode::OK);
        assert_eq!(second.unwrap().status(), StatusCode::OK);
        let mut caps: Vec<u64> = messages_sent(&to_anthropic)
            .iter()
            .map(|b| b["max_tokens"].as_u64().unwrap())
            .collect();
        caps.sort_unstable();
        assert_eq!(
            caps,
            [3_500, 3_800],
            "the second is capped by what the first left: 3.7¢, less 0.2¢ in"
        );
    }

    #[tokio::test]
    async fn only_what_the_relay_can_meter_is_passed_on() {
        let (hq_url, _) = hq(400_000);
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::team_relay(node(&hq_url, &upstream));
        for refused in [
            json!({ "model": "claude-opus-5-5", "max_tokens": 10, "messages": [], "speed": "fast" }),
            json!({ "model": "claude-sonnet-5-5", "max_tokens": 10, "messages": [],
                    "tools": [{ "type": "web_fetch_20260209", "name": "web_fetch" }] }),
            json!({ "model": "claude-sonnet-5-5", "max_tokens": 10, "messages": [],
                    "mcp_servers": [{ "type": "url", "url": "https://example.com/mcp", "name": "x" }] }),
        ] {
            let res = app
                .clone()
                .oneshot(ask("good", refused.clone()))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{refused}");
        }
        let mut counting = ask(
            "good",
            json!({ "model": "claude-sonnet-5-5", "messages": [],
                    "mcp_servers": [{ "type": "url", "url": "https://example.com/mcp", "name": "x" }] }),
        );
        *counting.uri_mut() = "/team-relay/v1/messages/count_tokens".parse().unwrap();
        let res = app.clone().oneshot(counting).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "counting too");
        assert!(
            to_anthropic.lock().unwrap().is_empty(),
            "nothing was sent on"
        );

        let mut fine = ask(
            "good",
            json!({ "model": "claude-sonnet-5-5", "max_tokens": 10, "messages": [] }),
        );
        fine.headers_mut().insert(
            "anthropic-beta",
            "fast-mode-2026-02-01,interleaved-thinking-2025-05-14"
                .parse()
                .unwrap(),
        );
        let res = app.oneshot(fine).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let sent = to_anthropic.lock().unwrap().clone();
        for (_, headers, _) in &sent {
            let beta = headers
                .iter()
                .find(|(k, _)| k == "anthropic-beta")
                .map(|(_, v)| v.as_str());
            assert_eq!(beta, Some("interleaved-thinking-2025-05-14"));
        }
    }

    #[tokio::test]
    async fn the_ceo_thinks_with_opus_a_dollar_a_turn_and_everyone_else_forty_cents() {
        let (upstream, to_anthropic) = anthropic();
        let call = |model: &str| {
            ask(
                "good",
                json!({ "model": model, "max_tokens": 200_000, "messages": [] }),
            )
        };
        // HQ says two dollars are left: the CEO's call is held to one, the lead's to forty
        // cents, each less its prompt (1k tokens in).
        let (ceo, to_hq) = hq_thinking(2_000_000, "ceo", true);
        let res = super::super::team_relay(node(&ceo, &upstream))
            .oneshot(call("claude-opus-5-5"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            messages_sent(&to_anthropic)[0]["max_tokens"],
            49_800,
            "$1 less 1k in at $4, at $20 a million out"
        );
        assert_eq!(
            charged(&to_hq),
            vec![6_000],
            "1k in at $4 and 100 out at $20 a million"
        );
        let (lead, _) = hq_thinking(2_000_000, "lead", true);
        let app = super::super::team_relay(node(&lead, &upstream));
        let res = app.clone().oneshot(call("claude-opus-5-5")).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::FORBIDDEN,
            "Opus is the CEO's alone"
        );
        let res = app.oneshot(call("claude-sonnet-5-5")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            messages_sent(&to_anthropic)[1]["max_tokens"],
            39_800,
            "40¢ less 1k in at $2, at $10 a million out"
        );
    }

    #[tokio::test]
    async fn opus_is_the_ceos_alone_and_a_researcher_thinks_with_haiku() {
        let (upstream, to_anthropic) = anthropic();
        let call = |model: &str| {
            ask(
                "good",
                json!({ "model": model, "max_tokens": 10, "messages": [] }),
            )
        };
        let (developer, _) = hq_thinking(400_000, "developer", true);
        let res = super::super::team_relay(node(&developer, &upstream))
            .oneshot(call("claude-opus-5-5"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let (researcher, _) = hq_thinking(400_000, "researcher", true);
        let app = super::super::team_relay(node(&researcher, &upstream));
        let res = app
            .clone()
            .oneshot(call("claude-sonnet-5-5"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert!(to_anthropic.lock().unwrap().is_empty());
        let res = app.oneshot(call("claude-haiku-5-5")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn no_token_no_model_and_no_money_mean_no_call() {
        let (hq_url, _) = hq(40_000);
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::team_relay(node(&hq_url, &upstream));
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
        let broke = super::super::team_relay(node(&spent_hq, &upstream))
            .oneshot(call("good", "claude-sonnet-5-5"))
            .await
            .unwrap();
        assert_eq!(broke.status(), StatusCode::PAYMENT_REQUIRED);
        assert!(to_anthropic.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_turn_on_the_houses_model_never_reaches_anthropic() {
        // HQ says the member thinks at home: what its turn has read (a log, a queue) may be
        // Confidential, so none of it goes to Claude, whatever the worker sends.
        let (local, _) = hq_as(400_000, "lead");
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::team_relay(node(&local, &upstream));
        let call = json!({ "model": "claude-sonnet-5-5", "max_tokens": 10, "messages": [] });
        let res = app
            .clone()
            .oneshot(ask("good", call.clone()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let said: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(said["error"]["type"], "permission_error");
        let mut counting = ask("good", call);
        *counting.uri_mut() = "/team-relay/v1/messages/count_tokens".parse().unwrap();
        let res = app.oneshot(counting).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "counting too");
        assert!(
            to_anthropic.lock().unwrap().is_empty(),
            "nothing was sent on"
        );
    }

    /// Anthropic counting every prompt as `prompt` tokens, whose every reply says it wrote
    /// `written` of them to the cache and read the rest from it, with 100 out.
    fn anthropic_caching(prompt: u64, written: u64) -> (String, Seen) {
        fake(Arc::new(move |path, _body| {
            if path == "/v1/messages/count_tokens" {
                return (
                    200,
                    "application/json",
                    json!({ "input_tokens": prompt }).to_string(),
                );
            }
            (
                200,
                "application/json",
                json!({ "type": "message", "content": [{ "type": "text", "text": "Hello." }],
                        "usage": { "input_tokens": 0, "cache_creation_input_tokens": written,
                                   "cache_read_input_tokens": prompt - written,
                                   "output_tokens": 100 } })
                .to_string(),
            )
        }))
    }

    #[tokio::test]
    async fn a_cached_call_passes_with_its_effort_and_is_charged_at_the_cache_prices() {
        let (hq_url, to_hq) = hq_thinking(1_000_000, "ceo", true);
        let (upstream, to_anthropic) = anthropic_caching(20_000, 2_000);
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let call = json!({ "model": "claude-opus-5-5", "max_tokens": 4096,
            "cache_control": { "type": "ephemeral" }, "output_config": { "effort": "high" },
            "system": "You are one member of a small team.",
            "messages": [{ "role": "user", "content": "You are Mara, the team's CEO." }] });
        let res = app
            .clone()
            .oneshot(ask("good", call.clone()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(messages_sent(&to_anthropic), [call], "passed on as sent");
        let counted: serde_json::Value =
            serde_json::from_str(&to_anthropic.lock().unwrap()[0].2).unwrap();
        assert!(
            counted.get("cache_control").is_none() && counted.get("output_config").is_none(),
            "counting is asked only the prompt: {counted}"
        );
        assert_eq!(
            charged(&to_hq),
            vec![2_000 * 5 + 18_000 / 5 + 100 * 20],
            "2k written at $5, 18k read at 20¢ and 100 out at $20 a million"
        );
        let told = to_hq
            .lock()
            .unwrap()
            .iter()
            .rfind(|(p, _, _)| p == "/v1/team/usage")
            .map(|(_, _, b)| serde_json::from_str::<serde_json::Value>(b).unwrap())
            .unwrap();
        assert_eq!(
            (
                &told["input"],
                &told["cache_read"],
                &told["cache_write"],
                &told["output"]
            ),
            (&json!(0), &json!(18_000), &json!(2_000), &json!(100)),
            "HQ hears the tokens too, in and out, for the turn's record"
        );

        let mut odd = json!({ "model": "claude-opus-5-5", "max_tokens": 10, "messages": [] });
        odd["output_config"] = json!({ "effort": "extreme" });
        let res = app.oneshot(ask("good", odd)).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "only the five efforts"
        );
    }

    #[tokio::test]
    async fn what_the_last_call_cached_is_reckoned_at_the_cache_price_and_nothing_else() {
        // A developer's turn on Sonnet with 10¢ left; every prompt counts 20k tokens.
        let (hq_url, _) = hq(100_000);
        let (upstream, to_anthropic) = anthropic_caching(20_000, 1_000);
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let first = json!({ "role": "user", "content": "You are Linus, the team's developer." });
        let more = [
            first.clone(),
            json!({ "role": "assistant", "content": [{ "type": "text", "text": "Looking." }] }),
            json!({ "role": "user", "content": "Go on." }),
        ];
        let round = |system: &str, messages: &[serde_json::Value], caching: bool| {
            let mut call = json!({ "model": "claude-sonnet-5-5", "max_tokens": 64_000,
                                   "system": system, "messages": messages });
            if caching {
                call["cache_control"] = json!({ "type": "ephemeral" });
            }
            call
        };
        for (call, cap, why) in [
            (
                round("Be brief.", &[first.clone()], true),
                5_000,
                "nothing cached yet: 20k written at $2.50 leaves 5¢, 5k out",
            ),
            (
                round("Be brief.", &more, true),
                9_800,
                "the 20k the call before cached, read at 10¢, leave 9.8¢",
            ),
            (
                round("Be short.", &more, true),
                5_000,
                "another system prompt: nothing of the cache is read",
            ),
            (
                round("Be short.", &more, false),
                6_000,
                "asking no cache: 20k fresh at $2",
            ),
        ] {
            let res = app.clone().oneshot(ask("good", call)).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{why}");
            let sent = messages_sent(&to_anthropic);
            assert_eq!(sent.last().unwrap()["max_tokens"], cap, "{why}");
        }
    }

    /// HQ, knowing one Claude Code turn (ADR 0117): token `cc`, Rae's, a developer on Haiku;
    /// and one of Endora's worker's (`good`), the same but for its runtime.
    fn hq_claude_code(left: i64) -> (String, Seen) {
        fake(Arc::new(move |path, body| match path {
            "/v1/team/turn" if body.contains("\"cc\"") || body.contains("\"good\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Rae", "role": "developer", "left": left,
                        "cloud": true, "brain": "claude-haiku-5-5",
                        "runtime": if body.contains("\"cc\"") { "claude-code" } else { "worker" } })
                .to_string(),
            ),
            "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
            _ => (200, "application/json", "{}".to_owned()),
        }))
    }

    #[tokio::test]
    async fn a_claude_code_turns_call_passes_in_its_own_shape_held_to_the_brain_and_the_cap() {
        let (hq_url, to_hq) = hq_claude_code(20_000);
        let (upstream, to_anthropic) = anthropic();
        let state = node(&hq_url, &upstream);
        // As Claude Code 2.1.296 sends a round (recorded, cut down): Bearer, its betas, its
        // own fields, an hour's cache mark, and `max_tokens` far past what the turn can pay.
        let round = json!({
            "model": "claude-haiku-5-5", "max_tokens": 128_000, "stream": true,
            "system": [
                { "type": "text", "text": "You are Claude Code." },
                { "type": "text", "text": "Endora's rules.", "cache_control": { "type": "ephemeral", "ttl": "1h" } }],
            "tools": [{ "name": "Bash", "description": "Run a command", "input_schema": { "type": "object" } },
                      { "name": "mcp__endora__post", "description": "Post", "input_schema": { "type": "object" } }],
            "messages": [{ "role": "user", "content": [
                { "type": "text", "text": "Your task: tidy a role", "cache_control": { "type": "ephemeral" } }] }],
            "metadata": { "user_id": "{\"session_id\":\"s\"}" },
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "low" },
        });
        let send = |token: &str, body: &serde_json::Value| {
            axum::http::Request::builder()
                .method("POST")
                .uri("/team-relay/v1/messages?beta=true")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .header("anthropic-version", "2023-06-01")
                .header(
                    "anthropic-beta",
                    "interleaved-thinking-2025-05-14,claude-code-20250219,effort-2025-11-24,fast-mode-2026-02-01",
                )
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let res = super::super::team_relay(state.clone())
            .oneshot(send("cc", &round))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let _ = res.into_body().collect().await.unwrap();
        let sent = messages_sent(&to_anthropic);
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0]["system"][1]["cache_control"],
            json!({ "type": "ephemeral", "ttl": "5m" }),
            "an hour's cache is asked for five minutes"
        );
        assert_eq!(
            sent[0]["metadata"], round["metadata"],
            "its own fields as sent"
        );
        assert_eq!(sent[0]["thinking"], round["thinking"]);
        // 2¢ left, less 1k tokens written to the cache at 12.5¢ a million, at 50¢ a million
        // out: the reply is capped far below Claude Code's 128k.
        assert_eq!(sent[0]["max_tokens"], 39_750);
        let betas: Vec<String> = to_anthropic
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, h, _)| {
                h.iter()
                    .find(|(k, _)| k == "anthropic-beta")
                    .map(|(_, v)| v.clone())
            })
            .collect();
        assert!(!betas.is_empty());
        for beta in &betas {
            assert_eq!(
                beta, "interleaved-thinking-2025-05-14,claude-code-20250219,effort-2025-11-24",
                "Claude Code's betas pass, fast mode does not"
            );
        }
        assert!(!charged(&to_hq).is_empty(), "charged when the stream ends");
        // Another model than the member's brain is refused, and so is a server-side tool.
        for change in [
            json!({ "model": "claude-sonnet-5-5" }),
            json!({ "tools": [{ "type": "web_search_20260209", "name": "web_search" }] }),
            json!({ "mcp_servers": [] }),
        ] {
            let mut odd = round.clone();
            for (k, v) in change.as_object().unwrap() {
                odd[k] = v.clone();
            }
            let res = super::super::team_relay(state.clone())
                .oneshot(send("cc", &odd))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{change}");
        }
        // Endora's worker's turn is held to its strict rule: the same call is refused.
        let res = super::super::team_relay(state)
            .oneshot(send("good", &round))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_workers_own_request_passes_as_it_sends_it_capped_and_charged() {
        // As `endora-worker` sends a round on a cloud brain: its bearer token, the version,
        // its instructions as `system`, the team's tools in Anthropic's form, and a tool's
        // use and result in the conversation.
        let (hq_url, to_hq) = hq_thinking(400_000, "lead", true);
        let (upstream, to_anthropic) = anthropic();
        let app = super::super::team_relay(node(&hq_url, &upstream));
        let round = json!({
            "model": "claude-sonnet-5-5",
            "max_tokens": 4096,
            "system": "You are one member of a small team at Endora HQ.",
            "tools": [{ "name": "homelab_status", "description": "Read the home lab's state.",
                        "input_schema": { "type": "object", "properties": {} } }],
            "messages": [
                { "role": "user", "content": "You are Jane Doe, the team's lead." },
                { "role": "assistant", "content": [
                    { "type": "text", "text": "Looking." },
                    { "type": "tool_use", "id": "toolu_1", "name": "homelab_status", "input": {} }] },
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "content": "{}" }] },
            ],
        });
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/team-relay/v1/messages")
            .header("content-type", "application/json")
            .header("authorization", "Bearer good")
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(round.to_string()))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let sent = messages_sent(&to_anthropic);
        assert_eq!(sent.len(), 1);
        let mut expected = round.clone();
        expected["max_tokens"] = json!(4096);
        assert_eq!(
            sent[0], expected,
            "passed on whole: 40¢ left covers 4,096 out"
        );
        assert_eq!(charged(&to_hq), vec![3_000]);
    }

    /// A model on the house's network, answering OpenAI's way; its base is `{url}/v1`.
    fn local_model() -> (String, Seen) {
        let (url, seen) = fake(Arc::new(|_path, _body| {
            (
                200,
                "application/json",
                json!({ "choices": [{ "message": { "role": "assistant", "content": "Hello." } }],
                        "usage": { "prompt_tokens": 120, "completion_tokens": 30 } })
                .to_string(),
            )
        }));
        (format!("{url}/v1"), seen)
    }

    /// A node with HQ, whose installed model is `installed`, and an Everyday list of
    /// `(provider url, model, key, enabled)`, in order.
    fn local_node(
        hq_url: &str,
        installed: &str,
        everyday: &[(&str, &str, &str, bool)],
    ) -> AppState {
        use endora_capabilities::model_lists::{Job, ModelListRepository as _};
        let mut state = AppState::new(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
        )
        .with_hq(endora_infrastructure::HqClient::new(
            hq_url,
            &"n".repeat(32),
        ));
        state.installed_model = Arc::new((installed.to_owned(), "qwen2.5:7b".to_owned()));
        for (i, (url, model, key, enabled)) in everyday.iter().enumerate() {
            let provider = state
                .config
                .add_provider(&format!("Provider {i}"), url, key)
                .unwrap();
            let entry = state
                .config
                .add_entry(Job::Everyday, provider, model)
                .unwrap();
            state.config.enable_entry(entry, *enabled).unwrap();
        }
        state
    }

    fn ask_local(token: &str, body: serde_json::Value) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri("/team-relay/local/v1/chat/completions")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn a_call(model: &str) -> serde_json::Value {
        json!({ "model": model, "messages": [{ "role": "user", "content": "Hi." }] })
    }

    /// The model a fake was asked for, call by call.
    fn models_asked(seen: &Seen) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .filter_map(|(_, _, b)| {
                let v = serde_json::from_str::<serde_json::Value>(b).ok()?;
                v["model"].as_str().map(str::to_owned)
            })
            .collect()
    }

    /// HQ knowing one turn of a member who thinks with DeepSeek, with `left` micros.
    fn hq_on_deepseek(left: i64) -> (String, Seen) {
        fake(Arc::new(move |path, body| match path {
            "/v1/team/turn" if body.contains("\"good\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Sol", "role": "developer", "left": left,
                        "cloud": true, "brain": "deepseek-flash" })
                .to_string(),
            ),
            "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
            _ => (200, "application/json", r#"{"may_go_on":true}"#.to_owned()),
        }))
    }

    /// The owner, 2026-10-28: DeepSeek Flash as a brain. A member who thinks with it is
    /// answered by the DeepSeek entry on the Everyday list and nothing else, with its output
    /// held to what the turn can afford, and the call is charged to HQ at DeepSeek's prices.
    #[tokio::test]
    async fn a_deepseek_turn_is_answered_by_the_deepseek_entry_capped_and_charged() {
        let (hq_url, to_hq) = hq_on_deepseek(1_200);
        let (house, to_house) = local_model();
        let (deepseek, to_deepseek) = fake(Arc::new(|_path, _body| {
            (
                200,
                "application/json",
                json!({ "choices": [{ "message": { "role": "assistant", "content": "Done." } }],
                        "usage": { "prompt_tokens": 1_000, "completion_tokens": 100,
                                   "prompt_cache_hit_tokens": 600,
                                   "prompt_cache_miss_tokens": 400 } })
                .to_string(),
            )
        }));
        // DeepSeek is not on the house's network: an entry like it is passed over for a house
        // turn, and the only one a DeepSeek turn is given.
        let state = local_node(
            &hq_url,
            &house,
            &[
                (&house, "qwen3.5:9b", "", true),
                (
                    &format!("{deepseek}/v1"),
                    "deepseek-flash",
                    "a-deepseek-key",
                    true,
                ),
            ],
        );
        let res = super::super::team_relay(state)
            .oneshot(ask_local("good", a_call("local")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            to_house.lock().unwrap().is_empty(),
            "never the house's model"
        );
        let asked = to_deepseek.lock().unwrap().clone();
        assert_eq!(asked.len(), 1);
        let call: serde_json::Value = serde_json::from_str(&asked[0].2).unwrap();
        assert_eq!(call["model"], "deepseek-flash");
        // $0.0012 left at $1.20 a million out: 1,000 tokens at most.
        assert_eq!(call["max_tokens"], 1_000, "{call}");
        assert!(
            asked[0]
                .1
                .iter()
                .any(|(k, v)| k == "authorization" && v == "Bearer a-deepseek-key")
        );
        let charged: Vec<serde_json::Value> = to_hq
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/usage")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(charged.len(), 1);
        assert_eq!(charged[0]["micros"], 244);
        assert_eq!(charged[0]["cache_read"], 600);
    }

    #[tokio::test]
    async fn a_deepseek_turn_with_nothing_left_or_no_entry_is_answered_by_nothing() {
        let (hq_url, _) = hq_on_deepseek(0);
        let (house, to_house) = local_model();
        let (deepseek, to_deepseek) = local_model();
        let state = local_node(&hq_url, &house, &[(&deepseek, "deepseek-flash", "k", true)]);
        let res = super::super::team_relay(state)
            .oneshot(ask_local("good", a_call("local")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYMENT_REQUIRED);
        let (hq_url, _) = hq_on_deepseek(400_000);
        let state = local_node(&hq_url, &house, &[(&house, "qwen3.5:9b", "", true)]);
        let res = super::super::team_relay(state)
            .oneshot(ask_local("good", a_call("local")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(to_house.lock().unwrap().is_empty() && to_deepseek.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_turn_asks_the_first_local_everyday_entry_with_that_entrys_model() {
        let (hq_url, to_hq) = hq(0);
        let (installed, to_installed) = local_model();
        let (off, to_off) = local_model();
        let (first, to_first) = local_model();
        let (second, to_second) = local_model();
        let state = local_node(
            &hq_url,
            &installed,
            &[
                (&off, "llama3.1:8b", "", false),
                (&first, "qwen2.5:14b", "the-macs-key", true),
                (&second, "llama3.1:8b", "", true),
            ],
        );
        let store = state.store.clone();
        let res = super::super::team_relay(state)
            .oneshot(ask_local("good", a_call("claude-sonnet-5-5")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(
            String::from_utf8_lossy(&body).contains("Hello."),
            "the reply passes through"
        );
        assert_eq!(
            models_asked(&to_first),
            ["qwen2.5:14b"],
            "the owner's model, whatever the worker named"
        );
        let sent = to_first.lock().unwrap().clone();
        let (path, headers, _) = &sent[0];
        assert_eq!(path, "/v1/chat/completions");
        let auth = headers
            .iter()
            .find(|(k, _)| k == "authorization")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            auth,
            Some("Bearer the-macs-key"),
            "the entry's key, never the turn's token"
        );
        for other in [&to_off, &to_second, &to_installed] {
            assert!(other.lock().unwrap().is_empty(), "only the first is asked");
        }
        assert!(
            to_hq
                .lock()
                .unwrap()
                .iter()
                .all(|(p, _, _)| p != "/v1/team/usage"),
            "local is free: HQ is charged nothing, even with nothing left"
        );
        let used = store.model_usage_totals("team-local", 0).unwrap();
        assert_eq!(
            (used.calls, used.prompt_tokens, used.completion_tokens),
            (1, 120, 30)
        );
    }

    #[tokio::test]
    async fn a_team_turn_always_thinks_whatever_the_worker_or_the_entry_says() {
        let (hq_url, _) = hq(0);
        let (installed, _) = local_model();
        let (local, to_local) = local_model();
        let state = local_node(&hq_url, &installed, &[(&local, "qwen3:8b", "", true)]);
        let mut quiet = a_call("local");
        quiet["reasoning_effort"] = json!("none");
        for call in [a_call("local"), quiet] {
            let res = super::super::team_relay(state.clone())
                .oneshot(ask_local("good", call))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }
        let sent = to_local.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
        for (_, _, body) in &sent {
            let v: serde_json::Value = serde_json::from_str(body).unwrap();
            assert!(
                v.get("reasoning_effort").is_none(),
                "never told not to think: {v}"
            );
        }
    }

    #[tokio::test]
    async fn a_cloud_entry_is_passed_over_for_one_on_the_houses_network() {
        let (hq_url, _) = hq(40_000);
        let (installed, to_installed) = local_model();
        let (local, to_local) = local_model();
        let state = local_node(
            &hq_url,
            &installed,
            &[
                ("https://api.example.com/v1", "big-model", "sk-cloud", true),
                (&local, "qwen2.5:14b", "", true),
            ],
        );
        let res = super::super::team_relay(state)
            .oneshot(ask_local("good", a_call("big-model")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(models_asked(&to_local), ["qwen2.5:14b"]);
        let sent = to_local.lock().unwrap().clone();
        assert!(
            sent[0].1.iter().all(|(k, _)| k != "authorization"),
            "a keyless endpoint is sent no key"
        );
        assert!(to_installed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn with_nothing_local_on_the_list_the_installed_model_answers() {
        let (hq_url, _) = hq(40_000);
        let (installed, to_installed) = local_model();
        let (off, to_off) = local_model();
        let state = local_node(
            &hq_url,
            &installed,
            &[
                ("https://api.example.com/v1", "big-model", "sk-cloud", true),
                (&off, "qwen2.5:14b", "", false),
            ],
        );
        let res = super::super::team_relay(state)
            .oneshot(ask_local("good", a_call("big-model")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(models_asked(&to_installed), ["qwen2.5:7b"]);
        assert!(to_off.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_stranger_never_reaches_the_local_model() {
        let (hq_url, _) = hq(40_000);
        let (installed, to_installed) = local_model();
        let (local, to_local) = local_model();
        let app = super::super::team_relay(local_node(
            &hq_url,
            &installed,
            &[(&local, "qwen2.5:14b", "", true)],
        ));
        for token in ["guessed", ""] {
            let res = app
                .clone()
                .oneshot(ask_local(token, a_call("qwen2.5:14b")))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
            let body = res.into_body().collect().await.unwrap().to_bytes();
            let said: serde_json::Value = serde_json::from_slice(&body).unwrap();
            // Worded OpenAI's way, not Anthropic's: no top-level `type`.
            assert!(
                said["error"]["message"].is_string() && said["error"]["type"].is_string(),
                "worded OpenAI's way: {said}"
            );
            assert!(said.get("type").is_none(), "not Anthropic's shape: {said}");
        }
        assert!(to_local.lock().unwrap().is_empty());
        assert!(to_installed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_streamed_or_oversized_call_is_refused() {
        let (hq_url, _) = hq(40_000);
        let (installed, to_installed) = local_model();
        let (local, to_local) = local_model();
        let app = super::super::team_relay(local_node(
            &hq_url,
            &installed,
            &[(&local, "qwen2.5:14b", "", true)],
        ));
        let mut streamed = a_call("qwen2.5:14b");
        streamed["stream"] = json!(true);
        let res = app
            .clone()
            .oneshot(ask_local("good", streamed))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let mut huge = a_call("qwen2.5:14b");
        huge["messages"][0]["content"] = json!("x".repeat(LARGEST_LOCAL_CALL));
        let res = app.oneshot(ask_local("good", huge)).await.unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(to_local.lock().unwrap().is_empty());
        assert!(to_installed.lock().unwrap().is_empty());
    }

    /// HQ, knowing two turns of a developer on the house's side: `mac-turn`, whose member thinks
    /// with the owner's Mac (ADR 0130), and `home-turn`, whose member thinks with the house's
    /// own model. Neither is cloud.
    fn hq_with_a_mac_member() -> (String, Seen) {
        fake(Arc::new(|path, body| match path {
            "/v1/team/turn" if body.contains("\"mac-turn\"") => (
                200,
                "application/json",
                json!({ "run": 8, "member": "Linus", "role": "developer", "left": 0,
                        "cloud": false, "brain": "mac" })
                .to_string(),
            ),
            "/v1/team/turn" if body.contains("\"home-turn\"") => (
                200,
                "application/json",
                json!({ "run": 9, "member": "Rae", "role": "developer", "left": 0,
                        "cloud": false, "brain": "local" })
                .to_string(),
            ),
            _ => (
                404,
                "application/json",
                r#"{"error":"no such turn"}"#.to_owned(),
            ),
        }))
    }

    /// A node whose Mac is set up at `mac` with a key and a model, and whose helper last said it
    /// was plugged in with the owner away for `idle_s` seconds.
    fn node_with_a_mac(hq_url: &str, installed: &str, mac: &str, idle_s: u64) -> AppState {
        use endora_application::CapabilitySettingsRepository as _;
        let state = local_node(hq_url, installed, &[]);
        for (key, value) in [
            ("url", mac),
            ("api_key", "the-macs-key"),
            ("model", "coder"),
        ] {
            state.config.set_setting("mac-brain", key, value).unwrap();
        }
        state.mac.hear(super::mac::Beat {
            at: state.clock.now().unix_millis(),
            plugged: true,
            idle_s,
            model_up: true,
        });
        state
    }

    #[tokio::test]
    async fn a_mac_turn_is_answered_by_the_mac_with_its_key_and_model_and_by_nothing_else() {
        let (hq_url, to_hq) = hq_with_a_mac_member();
        let (installed, to_installed) = local_model();
        let (mac, to_mac) = local_model();
        let state = node_with_a_mac(&hq_url, &installed, &mac, 3_600);
        let store = state.store.clone();
        let res = super::super::team_relay(state)
            .oneshot(ask_local("mac-turn", a_call("claude-sonnet-5-5")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("Hello."));
        assert_eq!(models_asked(&to_mac), ["coder"], "the owner's model name");
        let sent = to_mac.lock().unwrap().clone();
        let (path, headers, _) = &sent[0];
        assert_eq!(path, "/v1/chat/completions");
        let auth = headers
            .iter()
            .find(|(k, _)| k == "authorization")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            auth,
            Some("Bearer the-macs-key"),
            "the Mac's key, not the turn's token"
        );
        assert!(
            to_installed.lock().unwrap().is_empty(),
            "not the house's model"
        );
        assert!(
            to_hq
                .lock()
                .unwrap()
                .iter()
                .all(|(p, _, _)| p != "/v1/team/usage"),
            "the Mac costs nothing"
        );
        let used = store.model_usage_totals("team-mac", 0).unwrap();
        assert_eq!(
            (used.calls, used.prompt_tokens, used.completion_tokens),
            (1, 120, 30)
        );
    }

    #[tokio::test]
    async fn a_mac_turn_whose_mac_is_not_free_ends_with_a_clear_error_and_no_other_model_answers() {
        use endora_application::CapabilitySettingsRepository as _;
        let (hq_url, _) = hq_with_a_mac_member();
        let (installed, to_installed) = local_model();
        let (mac, to_mac) = local_model();
        // The owner asked for ten idle minutes, and came back: seconds idle.
        let state = node_with_a_mac(&hq_url, &installed, &mac, 4);
        state
            .config
            .set_setting("mac-brain", "idle_minutes", "10")
            .unwrap();
        let res = super::super::team_relay(state.clone())
            .oneshot(ask_local("mac-turn", a_call("claude-sonnet-5-5")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        let said = String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes())
            .into_owned();
        assert!(
            said.contains("owner active") && said.contains("tried again"),
            "{said}"
        );
        // The owner's switch holds it too, however long they have been away.
        state.mac.hear(super::mac::Beat {
            at: state.clock.now().unix_millis(),
            plugged: true,
            idle_s: 3_600,
            model_up: true,
        });
        state
            .config
            .set_setting("mac-brain", "busy", "true")
            .unwrap();
        let res = super::super::team_relay(state)
            .oneshot(ask_local("mac-turn", a_call("claude-sonnet-5-5")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(to_mac.lock().unwrap().is_empty(), "the Mac was not asked");
        assert!(
            to_installed.lock().unwrap().is_empty(),
            "no fallback to the house's model"
        );
    }

    #[tokio::test]
    async fn a_mac_turn_with_no_mac_set_up_is_not_answered_by_the_houses_model() {
        let (hq_url, _) = hq_with_a_mac_member();
        let (installed, to_installed) = local_model();
        let (local, to_local) = local_model();
        let state = local_node(&hq_url, &installed, &[(&local, "qwen2.5:14b", "", true)]);
        let res = super::super::team_relay(state)
            .oneshot(ask_local("mac-turn", a_call("claude-sonnet-5-5")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(to_installed.lock().unwrap().is_empty());
        assert!(to_local.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_house_turn_never_reaches_the_mac_and_a_mac_turn_never_reaches_claude() {
        let (hq_url, _) = hq_with_a_mac_member();
        let (installed, to_installed) = local_model();
        let (mac, to_mac) = local_model();
        let (anthropic, to_anthropic) = anthropic();
        let mut state = node_with_a_mac(&hq_url, &installed, &mac, 3_600);
        state.relay_upstream = Arc::new(anthropic);
        let app = super::super::team_relay(state);
        let res = app
            .clone()
            .oneshot(ask_local("home-turn", a_call("local")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            to_mac.lock().unwrap().is_empty(),
            "the Mac is for its members alone"
        );
        assert_eq!(to_installed.lock().unwrap().len(), 1);
        let res = app
            .oneshot(ask(
                "mac-turn",
                json!({ "model": "claude-sonnet-5-5", "max_tokens": 100,
                        "messages": [{ "role": "user", "content": "Hi." }] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "it stays in the house");
        assert!(to_anthropic.lock().unwrap().is_empty());
    }
}
