//! The bench credential (ADR 0123): one token, from a file on the NAS, that opens the model
//! relay and nothing else, for measuring an external agent harness against HQ's own wing on
//! one real task. It is no turn of HQ's: HQ is never asked about it and never charged for it.
//! Its calls pass in Claude Code's own shape under the rules of a Claude Code turn (ADR 0117),
//! on Haiku or Sonnet only, each held to what is left of the bench's day, and are metered to
//! the node's usage log as `bench`.

use super::relay::{Settle, count, count_on, pass_on, refuse};
use super::*;
use axum::body::Bytes;
use axum::http::HeaderMap;
use endora_infrastructure::SqliteStore;
use endora_infrastructure::relay::{
    Used, affordable_output, asks_to_cache, betas_for_claude_code, cost, counted, for_claude_code,
    most_a_turn, price_of, prompt_cost,
};

/// The bench's token file is off until it holds a token at least this long.
const SHORTEST_TOKEN: usize = 32;

/// The day's cap when the owner names none: two dollars.
const DEFAULT_CAP_CENTS: i64 = 200;

/// The bench's own credential and its day's money.
pub(crate) struct Bench {
    file: std::path::PathBuf,
    cap: i64,
    held: std::sync::Mutex<Held>,
    refusals: std::sync::Mutex<Refusals>,
}

/// Anthropic's refusals of the bench's calls since the node started: how many, and the last,
/// in its own words, so a run that reached nothing says why without a shell on the NAS.
#[derive(Default)]
struct Refusals {
    count: u64,
    last: Option<serde_json::Value>,
}

/// Why a call cannot be held: the day is spent, or calls in flight hold what is left of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Short {
    /// What the store says was spent leaves nothing for one token out.
    DaySpent,
    /// Something is left, but calls in flight hold it until they end.
    HeldInFlight,
}

/// What calls in flight have set aside, on one day.
#[derive(Default)]
struct Held {
    day: i64,
    micros: i64,
}

/// What one call in flight has set aside, and on which day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hold {
    day: i64,
    micros: i64,
}

impl Bench {
    /// A bench opened by the token in `file`, spending at most `cap_cents` a day.
    #[must_use]
    pub(crate) fn new(file: impl Into<std::path::PathBuf>, cap_cents: i64) -> Self {
        Self {
            file: file.into(),
            cap: cap_cents.max(0).saturating_mul(10_000),
            held: std::sync::Mutex::default(),
            refusals: std::sync::Mutex::default(),
        }
    }

    /// From `ENDORA_BENCH_TOKEN_FILE` and `ENDORA_BENCH_CAP_CENTS` (two dollars when unset or
    /// unreadable); `None` when no file is named.
    #[must_use]
    pub(crate) fn from_env() -> Option<Self> {
        let file = std::env::var("ENDORA_BENCH_TOKEN_FILE")
            .ok()
            .filter(|f| !f.trim().is_empty())?;
        let cents = std::env::var("ENDORA_BENCH_CAP_CENTS")
            .ok()
            .and_then(|c| c.trim().parse::<i64>().ok())
            .filter(|c| *c >= 0)
            .unwrap_or(DEFAULT_CAP_CENTS);
        Some(Self::new(file.trim(), cents))
    }

    /// The most the bench may spend a day, in micros.
    #[must_use]
    pub(crate) const fn cap_micros(&self) -> i64 {
        self.cap
    }

    /// Whether `token` is the bench's, as its file says now: deleting the file turns the bench
    /// off at once. A file holding fewer than [`SHORTEST_TOKEN`] characters opens nothing.
    pub(crate) fn opens(&self, token: &str) -> bool {
        let Ok(kept) = std::fs::read_to_string(&self.file) else {
            return false;
        };
        let kept = kept.trim();
        if kept.len() < SHORTEST_TOKEN || token.len() != kept.len() {
            return false;
        }
        // Compared whole, whatever differs first, so the time taken says nothing of it.
        kept.bytes()
            .zip(token.bytes())
            .fold(0u8, |differ, (a, b)| differ | (a ^ b))
            == 0
    }

    /// Sets aside the cost of a call on `day`: its `input` (micros) and as many tokens of
    /// output at `price` as it `asked` for, capped by what is left of the day (and never more
    /// than a developer's turn, ADR 0111). The output it may write, and the hold, or why not
    /// ([`Short`]) when the day cannot pay for a single token out. What is left is the day's cap, less what
    /// the store says was spent and what calls in flight have set aside, so the cap holds
    /// across a restart and for calls side by side.
    pub(crate) fn hold(
        &self,
        store: &SqliteStore,
        day: i64,
        price: endora_infrastructure::relay::Price,
        input: i64,
        asked: Option<u64>,
    ) -> Result<(u64, Hold), Short> {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.day != day {
            *held = Held { day, micros: 0 };
        }
        // An unreadable record is a spent day: the cap is never guessed open.
        let spent = store.bench_spent(day).unwrap_or(self.cap);
        let left = (self.cap - spent - held.micros).min(most_a_turn("developer"));
        let affordable = affordable_output(price, left - input);
        if affordable == 0 {
            let unheld = (self.cap - spent).min(most_a_turn("developer"));
            if affordable_output(price, unheld - input) == 0 {
                return Err(Short::DaySpent);
            }
            return Err(Short::HeldInFlight);
        }
        let out = asked.unwrap_or(affordable).min(affordable);
        let micros = input
            + cost(
                price,
                Used {
                    output: out,
                    ..Used::default()
                },
            );
        held.micros += micros;
        Ok((out, Hold { day, micros }))
    }

    /// Lets `hold` go, and keeps what its call `spent` (micros) on its day.
    pub(crate) fn settle(&self, store: &SqliteStore, hold: Hold, spent: Option<i64>) {
        if let Some(micros) = spent.filter(|m| *m > 0) {
            if let Err(e) = store.add_bench_spent(hold.day, micros) {
                eprintln!("bench: could not keep {micros} micros spent: {e}");
            }
        }
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.day == hold.day {
            held.micros = (held.micros - hold.micros).max(0);
        }
    }

    /// Keeps `refusal` as the last of Anthropic's refusals, and counts it.
    fn note_refusal(&self, refusal: serde_json::Value) {
        let mut refusals = self
            .refusals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        refusals.count += 1;
        refusals.last = Some(refusal);
    }

    /// How many of the bench's calls Anthropic refused since the node started, and the last.
    fn refusals(&self) -> (u64, serde_json::Value) {
        let refusals = self
            .refusals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            refusals.count,
            refusals.last.clone().unwrap_or(serde_json::Value::Null),
        )
    }
}

/// The most of a refusal's body read back: an error is a few hundred bytes.
const LARGEST_REFUSAL: usize = 64 * 1024;

/// A refusal of the bench's own, as Claude Code shows it truthfully: a 400 (never a 402 or a
/// `billing_error`, which Claude Code shows as "Credit balance too low", blaming the owner's
/// Anthropic account), its words starting `bench: `.
fn gate(why: &str) -> Response {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_request_error",
        &format!("bench: {why}"),
    )
}

/// What came back for a call at `step` passed on, as it is: a success untouched; a refusal
/// read whole, kept on the bench's record and in the node's log, and passed on with its status,
/// its kind and Anthropic's request id, its words saying it was Anthropic's, at which step, on
/// which model. A failure of the relay's own (no request id) says so instead.
async fn as_said(bench: &Bench, step: &str, model: &str, answer: Response) -> Response {
    let status = answer.status();
    if status.is_success() {
        return answer;
    }
    let bytes = axum::body::to_bytes(answer.into_body(), LARGEST_REFUSAL)
        .await
        .unwrap_or_default();
    let said: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let kind = said["error"]["type"]
        .as_str()
        .unwrap_or("api_error")
        .to_owned();
    let message = said["error"]["message"]
        .as_str()
        .unwrap_or("(no message)")
        .to_owned();
    let request_id = said["request_id"].clone();
    let Some(id) = request_id.as_str() else {
        return refuse(
            status,
            &kind,
            &format!("bench: the relay got no answer from Anthropic for {step}: {message}"),
        );
    };
    eprintln!(
        "bench: Anthropic refused {step} on {model}: {} {kind}: {message} ({id})",
        status.as_u16()
    );
    bench.note_refusal(json!({
        "at_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        "step": step, "model": model, "status": status.as_u16(), "type": kind,
        "message": message, "request_id": id,
    }));
    let words = format!(
        "bench: Anthropic refused {step} on {model} ({} {kind}): {message}",
        status.as_u16()
    );
    (
        status,
        Json(
            json!({ "type": "error", "error": { "type": kind, "message": words },
                     "request_id": request_id }),
        ),
    )
        .into_response()
}

/// Days since the epoch, UTC: the bench's day.
fn today() -> i64 {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    ms / 86_400_000
}

/// Whether the bench may call `model`: Claude Haiku or Sonnet, never Opus or anything else.
fn may_bench(model: &str) -> Result<(), String> {
    let is = |family: &str| model == family || model.starts_with(&format!("{family}-"));
    if is("claude-haiku-5-5") || is("claude-sonnet-5-5") {
        return Ok(());
    }
    Err(format!(
        "the bench thinks with claude-haiku-5-5 or claude-sonnet-5-5, not {model:?}"
    ))
}

/// The bench that `token` opens on this node, if any.
pub(super) fn opened_by(state: &AppState, token: &str) -> Option<Arc<Bench>> {
    state.bench.clone().filter(|bench| bench.opens(token))
}

/// A call with the bench's token, parsed and made fit to pass on in Claude Code's shape, held
/// to its own model (ADR 0117's rules, with the bench's two models as the brain), or the
/// refusal's words, which [`gate`] says.
fn fit(body: &[u8]) -> Result<(serde_json::Value, String), String> {
    let Ok(mut call) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Err("the body is not JSON".to_owned());
    };
    let model = call["model"].as_str().unwrap_or("").to_owned();
    may_bench(&model)?;
    for_claude_code(&mut call, &model)?;
    Ok((call, model))
}

/// `POST /team-relay/v1/messages` with the bench's token: counted, held to what is left of
/// the bench's day, passed on with the owner's key, and kept as the bench's spending and in
/// the node's usage log as `bench`. HQ is never asked or charged.
pub(super) async fn messages(
    state: AppState,
    bench: Arc<Bench>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (mut call, model) = match fit(&body) {
        Ok(fit) => fit,
        Err(why) => return gate(&why),
    };
    let Some(key) = super::relay::anthropic_key(state.config.as_ref()) else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "no Anthropic provider is set up on the AI screen",
        );
    };
    let version = super::relay::header(&headers, "anthropic-version");
    let beta = betas_for_claude_code(super::relay::header(&headers, "anthropic-beta").as_deref());
    let upstream = state.relay_upstream.clone();
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
        Err(refused) => return as_said(&bench, "counting the prompt", &model, refused).await,
    };
    #[allow(clippy::cast_precision_loss)]
    let dollars = bench.cap_micros() as f64 / 1_000_000.0;
    let Some(price) = price_of(&model, prompt) else {
        return gate(&format!("{model} has no price the relay knows"));
    };
    // Its calls share one token, so what one cached is never counted on for another: the
    // whole prompt is set aside at the dearer price, and the reply's usage is what is kept.
    let input = prompt_cost(price, prompt, 0, asks_to_cache(&call));
    let store = state.store.clone();
    let (cap, hold) = match bench.hold(&store, today(), price, input, call["max_tokens"].as_u64()) {
        Ok(held) => held,
        Err(Short::DaySpent) => {
            return gate(&format!(
                "today's cap of ${dollars:.2} is spent; it starts again at 00:00 UTC"
            ));
        }
        Err(Short::HeldInFlight) => {
            // A wait, not a spent day: the seats' other calls hold the rest until they end.
            let mut wait = refuse(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_error",
                &format!(
                    "bench: calls in flight hold what is left of today's ${dollars:.2}; try again shortly"
                ),
            );
            wait.headers_mut()
                .insert("retry-after", axum::http::HeaderValue::from_static("10"));
            return wait;
        }
    };
    call["max_tokens"] = json!(cap);
    let (bench2, model2) = (bench.clone(), model.clone());
    let paid = state.clone();
    let settle: Settle = Box::new(move |used| {
        let micros = used.map(|used| cost(price_of(&model, used.prompt()).unwrap_or(price), used));
        bench.settle(&store, hold, micros);
        // It is paid from the same Anthropic balance the owner watches (2026-10-30).
        if let Some(m) = micros {
            super::credits::after_bench_charge(&paid, m);
        }
        let Some(used) = used else { return };
        endora_application::UsageLog::record(
            store.as_ref(),
            "bench",
            &model,
            endora_application::ModelUsage {
                prompt_tokens: used.prompt(),
                completion_tokens: used.output,
                cached_tokens: used.cache_read,
            },
        );
    });
    let answer = pass_on(upstream, key, version, beta, call, settle).await;
    as_said(&bench2, "the call", &model2, answer).await
}

/// `POST /team-relay/v1/messages/count_tokens` with the bench's token: free, held to the
/// same rules.
pub(super) async fn count_tokens(
    state: AppState,
    bench: Arc<Bench>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (asked, model) = match fit(&body) {
        Ok(fit) => fit,
        Err(why) => return gate(&why),
    };
    let beta = betas_for_claude_code(super::relay::header(&headers, "anthropic-beta").as_deref());
    let answer = count_on(&state, &headers, &asked, beta).await;
    as_said(&bench, "counting the prompt", &model, answer).await
}

/// `GET /team-relay/bench` with the bench's token: what its day has cost so far, and its use
/// of models since the day began (UTC), so a run's numbers are read without a shell on the NAS.
pub(super) async fn today_so_far(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(bench) = opened_by(&state, &super::relay::token_of(&headers)) else {
        return refuse(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "only the bench's token reads the bench",
        );
    };
    let day = today();
    let refusals = bench.refusals();
    let spent = state.store.bench_spent(day);
    let used = state.store.model_usage_totals("bench", day * 86_400_000);
    let (Ok(spent), Ok(used)) = (spent, used) else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "the bench's record could not be read",
        );
    };
    Json(json!({
        "day": day, "cap_micros": bench.cap_micros(), "spent_micros": spent,
        "calls": used.calls, "prompt_tokens": used.prompt_tokens,
        "completion_tokens": used.completion_tokens, "cached_tokens": used.cached_tokens,
        "upstream_refusals": refusals.0, "last_refusal": refusals.1,
    }))
    .into_response()
}

/// In front of every route of the team listener but the model relay: the bench's token is
/// refused there before anything else is asked, HQ above all (ADR 0123). It opens no contract,
/// no bridge, no receipt, no check and no door of HQ's.
pub(super) async fn fenced(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let token = super::relay::token_of(request.headers());
    if opened_by(&state, &token).is_some() {
        return refuse(
            StatusCode::FORBIDDEN,
            "permission_error",
            "the bench's token opens the model relay and nothing else",
        );
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{Seen, anthropic, fake, messages_sent, node};
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    const TOKEN: &str = "the-benchs-own-token-for-these-tests-only";

    /// HQ, which knows no turn at all: whatever it is asked shows here.
    fn hq() -> (String, Seen) {
        fake(Arc::new(|path, _| match path {
            "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
            _ => (200, "application/json", "{}".to_owned()),
        }))
    }

    /// A folder of a test's own, removed when it is dropped.
    struct Folder(std::path::PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A token file holding `token`, in a folder of its own that lives as long as the guard.
    fn token_file(token: &str) -> (Folder, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("endora-bench-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token");
        std::fs::write(&file, format!("{token}\n")).unwrap();
        (Folder(dir), file)
    }

    fn with_bench(state: AppState, file: &std::path::Path, cap_cents: i64) -> AppState {
        state.with_bench(Bench::new(file, cap_cents))
    }

    fn ask(path: &str, token: &str, body: &serde_json::Value) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .header("anthropic-version", "2023-06-01")
            .header(
                "anthropic-beta",
                "claude-code-20250219,fast-mode-2026-02-01,effort-2025-11-24",
            )
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn a_round(model: &str) -> serde_json::Value {
        json!({
            "model": model, "max_tokens": 32_000,
            "system": [{ "type": "text", "text": "You are Claude Code.",
                         "cache_control": { "type": "ephemeral", "ttl": "1h" } }],
            "tools": [{ "name": "Bash", "description": "Run a command",
                        "input_schema": { "type": "object" } }],
            "messages": [{ "role": "user", "content": "Write the AGENTS.md." }],
            "metadata": { "user_id": "seat" },
        })
    }

    fn today() -> i64 {
        super::today()
    }

    async fn said_json(res: axum::response::Response) -> serde_json::Value {
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }

    async fn said(res: axum::response::Response) -> serde_json::Value {
        said_json(res).await
    }

    /// Whether a refusal says it is the bench's.
    fn said_bench(said: &serde_json::Value) -> bool {
        said["error"]["message"]
            .as_str()
            .is_some_and(|m| m.starts_with("bench: "))
    }

    #[tokio::test]
    async fn the_bench_token_reaches_claude_without_hq_and_is_metered_as_bench() {
        let (hq_url, to_hq) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        let res = team_relay(state.clone())
            .oneshot(ask(
                "/team-relay/v1/messages?beta=true",
                TOKEN,
                &a_round("claude-haiku-5-5"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let _ = res.into_body().collect().await.unwrap();
        assert!(
            to_hq.lock().unwrap().is_empty(),
            "HQ is neither asked nor charged"
        );
        for (_, headers, _) in to_anthropic.lock().unwrap().iter() {
            let key = headers.iter().find(|(k, _)| k == "x-api-key");
            assert_eq!(
                key.map(|(_, v)| v.as_str()),
                Some("sk-ant-the-owners-key"),
                "the owner's key upstream, never the bench's token"
            );
            let beta = headers.iter().find(|(k, _)| k == "anthropic-beta");
            assert_eq!(
                beta.map(|(_, v)| v.as_str()),
                Some("claude-code-20250219,effort-2025-11-24"),
                "Claude Code's betas pass, fast mode does not"
            );
        }
        let sent = messages_sent(&to_anthropic);
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0]["system"][0]["cache_control"]["ttl"], "5m",
            "an hour's cache is asked for five minutes, as for a Claude Code turn"
        );
        assert_eq!(sent[0]["metadata"], json!({ "user_id": "seat" }));
        // 1k in and 100 out on Haiku, at 10¢ and 50¢ a million.
        assert_eq!(state.store.bench_spent(today()).unwrap(), 150);
        let usage = state.store.model_usage_totals("bench", 0).unwrap();
        assert_eq!(
            (usage.calls, usage.prompt_tokens, usage.completion_tokens),
            (1, 1_000, 100)
        );
        assert_eq!(
            state.store.model_usage_totals("team", 0).unwrap().calls,
            0,
            "not the team's"
        );

        // Counting is free, and the bench's token may ask it.
        let res = team_relay(state)
            .oneshot(ask(
                "/team-relay/v1/messages/count_tokens",
                TOKEN,
                &a_round("claude-sonnet-5-5"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(to_hq.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_bench_thinks_with_haiku_or_sonnet_and_nothing_else() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        for model in ["claude-sonnet-5-5", "claude-haiku-5-5"] {
            let res = team_relay(state.clone())
                .oneshot(ask("/team-relay/v1/messages", TOKEN, &a_round(model)))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{model}");
        }
        for model in ["claude-opus-5-5", "gpt-5", ""] {
            for path in [
                "/team-relay/v1/messages",
                "/team-relay/v1/messages/count_tokens",
            ] {
                let res = team_relay(state.clone())
                    .oneshot(ask(path, TOKEN, &a_round(model)))
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{model} on {path}");
                let said = said(res).await;
                assert!(
                    said_bench(&said),
                    "a refusal Claude Code shows as the bench's own: {said}"
                );
            }
        }
        assert_eq!(messages_sent(&to_anthropic).len(), 2, "only the two passed");
    }

    #[tokio::test]
    async fn the_bench_keeps_a_claude_code_turns_rules() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        for change in [
            json!({ "tools": [{ "type": "web_search_20260209", "name": "web_search" }] }),
            json!({ "mcp_servers": [] }),
            json!({ "speed": "fast" }),
            json!({ "container": "c" }),
        ] {
            let mut odd = a_round("claude-haiku-5-5");
            for (k, v) in change.as_object().unwrap() {
                odd[k] = v.clone();
            }
            let res = team_relay(state.clone())
                .oneshot(ask("/team-relay/v1/messages", TOKEN, &odd))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{change}");
        }
        assert!(messages_sent(&to_anthropic).is_empty());
    }

    #[tokio::test]
    async fn the_benchs_day_is_capped_and_the_cap_outlives_a_restart() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        // A cent a day, nine tenths of it spent before the node restarted: the new process's
        // bench holds nothing in memory, and still sees it.
        let state = with_bench(node(&hq_url, &upstream), &file, 1);
        state.store.add_bench_spent(today(), 9_000).unwrap();
        let mut round = a_round("claude-haiku-5-5");
        round["system"] = json!("You are Claude Code.");
        let res = team_relay(state.clone())
            .oneshot(ask("/team-relay/v1/messages", TOKEN, &round))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        // 1,000 micros left, less 1k in at 10¢ a million: 900, or 1,800 tokens out at 50¢.
        assert_eq!(messages_sent(&to_anthropic)[0]["max_tokens"], 1_800);
        assert_eq!(state.store.bench_spent(today()).unwrap(), 9_150);
    }

    #[tokio::test]
    async fn a_spent_day_sends_nothing() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        state.store.add_bench_spent(today(), 2_000_000).unwrap();
        let res = team_relay(state)
            .oneshot(ask(
                "/team-relay/v1/messages",
                TOKEN,
                &a_round("claude-haiku-5-5"),
            ))
            .await
            .unwrap();
        // Not 402 or `billing_error`: Claude Code shows those as "Credit balance too low",
        // which would blame the owner's Anthropic account for the bench's own cap.
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let said = said(res).await;
        assert_eq!(said["error"]["type"], "invalid_request_error");
        assert!(said_bench(&said), "{said}");
        assert!(
            said["error"]["message"].as_str().unwrap().contains("$2.00"),
            "{said}"
        );
        assert!(messages_sent(&to_anthropic).is_empty());
    }

    #[tokio::test]
    async fn calls_in_flight_holding_the_rest_are_a_wait_not_a_spent_day() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 1);
        // Another seat's call in flight holds the whole cent.
        let haiku = endora_infrastructure::relay::price_of("claude-haiku-5-5", 0).unwrap();
        let bench = state.bench.clone().unwrap();
        let _held = bench.hold(&state.store, today(), haiku, 0, None).unwrap();
        let res = team_relay(state)
            .oneshot(ask(
                "/team-relay/v1/messages",
                TOKEN,
                &a_round("claude-haiku-5-5"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(res.headers().contains_key("retry-after"));
        let said = said(res).await;
        assert_eq!(said["error"]["type"], "rate_limit_error");
        assert!(said_bench(&said), "{said}");
        assert!(messages_sent(&to_anthropic).is_empty());
    }

    /// A round as Claude Code 2.1.296 sent it in the harness's seats (seen by the stand-in
    /// relay before the run): streamed, `max_tokens` 128,000, adaptive thinking, an effort,
    /// cache marks, and 22 tools, on a prompt of about 27,000 tokens.
    fn a_seats_round(model: &str) -> serde_json::Value {
        let tools: Vec<_> = (0..22)
            .map(|i| {
                json!({ "name": format!("Tool{i}"), "description": "a tool",
                             "input_schema": { "type": "object" } })
            })
            .collect();
        json!({
            "model": model, "max_tokens": 128_000, "stream": true,
            "system": [
                { "type": "text", "text": "You are Claude Code." },
                { "type": "text", "text": "The seat's role.",
                  "cache_control": { "type": "ephemeral", "ttl": "1h" } }],
            "tools": tools,
            "messages": [{ "role": "user", "content": [
                { "type": "text", "text": "orch-lead@bench",
                  "cache_control": { "type": "ephemeral" } }] }],
            "metadata": { "user_id": "{\"session_id\":\"s\"}" },
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "high" },
        })
    }

    #[tokio::test]
    async fn a_seats_real_first_round_passes_the_gate_on_either_model() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = super::super::relay::tests::anthropic_counting(27_000);
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        for model in ["claude-sonnet-5-5", "claude-haiku-5-5"] {
            let res = team_relay(state.clone())
                .oneshot(ask(
                    "/team-relay/v1/messages?beta=true",
                    TOKEN,
                    &a_seats_round(model),
                ))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{model}");
            let _ = res.into_body().collect().await.unwrap();
        }
        let sent = messages_sent(&to_anthropic);
        assert_eq!(sent.len(), 2, "both reached Anthropic");
        // Sonnet: 40¢ a call at most, less 27k written to the cache at $2.50 a million.
        assert_eq!(sent[0]["max_tokens"], 33_250);
        assert_eq!(
            sent[1]["max_tokens"], 128_000,
            "Haiku can pay for all it asked"
        );
    }

    /// Anthropic refusing, as it answered the bench's first run (2026-10-18): a 400 whose words
    /// Claude Code shows as "Credit balance is too low", with Anthropic's request id.
    fn anthropic_refusing(at: &'static str) -> (String, Seen) {
        fake(Arc::new(move |path, _| {
            if path == at {
                return (
                    400,
                    "application/json",
                    json!({ "type": "error", "error": { "type": "invalid_request_error",
                            "message": "Your credit balance is too low to access the Anthropic API." },
                            "request_id": "req_test_0001" })
                    .to_string(),
                );
            }
            if path == "/v1/messages/count_tokens" {
                return (
                    200,
                    "application/json",
                    json!({ "input_tokens": 100 }).to_string(),
                );
            }
            (500, "application/json", "{}".to_owned())
        }))
    }

    #[tokio::test]
    async fn anthropics_refusal_is_passed_on_as_anthropics_and_kept_on_the_record() {
        for (at, step) in [
            ("/v1/messages/count_tokens", "counting the prompt"),
            ("/v1/messages", "the call"),
        ] {
            let (hq_url, _) = hq();
            let (upstream, _) = anthropic_refusing(at);
            let (_dir, file) = token_file(TOKEN);
            let state = with_bench(node(&hq_url, &upstream), &file, 200);
            let res = team_relay(state.clone())
                .oneshot(ask(
                    "/team-relay/v1/messages?beta=true",
                    TOKEN,
                    &a_seats_round("claude-haiku-5-5"),
                ))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{at}");
            let said = said(res).await;
            let words = said["error"]["message"].as_str().unwrap();
            assert!(
                words.starts_with(&format!("bench: Anthropic refused {step}")),
                "{words}"
            );
            assert!(words.contains("credit balance is too low"), "{words}");
            assert_eq!(said["request_id"], "req_test_0001");
            // The record says who refused, at which step, in Anthropic's own words.
            let res = team_relay(state)
                .oneshot(
                    axum::http::Request::builder()
                        .uri("/team-relay/bench")
                        .header("authorization", format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let day = said_json(res).await;
            assert_eq!(day["upstream_refusals"], 1, "{day}");
            let last = &day["last_refusal"];
            assert_eq!(last["step"], step);
            assert_eq!(last["status"], 400);
            assert_eq!(last["model"], "claude-haiku-5-5");
            assert_eq!(last["request_id"], "req_test_0001");
            assert_eq!(
                last["message"],
                "Your credit balance is too low to access the Anthropic API."
            );
            assert_eq!(day["calls"], 0, "nothing was charged");
        }
    }

    #[tokio::test]
    async fn the_reply_is_capped_by_what_is_left_of_the_day() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        // A dime a day, all but 3,000 micros spent: 1k in on Haiku with no cache mark costs
        // 100, which leaves 2,900, or 5,800 tokens out at 50¢ a million.
        let state = with_bench(node(&hq_url, &upstream), &file, 10);
        state.store.add_bench_spent(today(), 97_000).unwrap();
        let mut round = a_round("claude-haiku-5-5");
        round["system"] = json!("You are Claude Code.");
        let res = team_relay(state)
            .oneshot(ask("/team-relay/v1/messages", TOKEN, &round))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(messages_sent(&to_anthropic)[0]["max_tokens"], 5_800);
    }

    #[test]
    fn calls_in_flight_never_set_aside_more_than_the_day_has() {
        let store = SqliteStore::open_in_memory().unwrap();
        let (_dir, file) = token_file(TOKEN);
        let bench = Bench::new(&file, 1);
        let haiku = endora_infrastructure::relay::price_of("claude-haiku-5-5", 0).unwrap();
        // 10,000 micros a day: a call asking 16,000 out (8,000 micros) and 1,000 in is held.
        let (cap, first) = bench.hold(&store, 5, haiku, 1_000, Some(16_000)).unwrap();
        assert_eq!(cap, 16_000);
        // 1,000 left: the next may write 1,000 tokens less its own input of 500.
        let (cap, second) = bench.hold(&store, 5, haiku, 500, Some(16_000)).unwrap();
        assert_eq!(cap, 1_000);
        assert_eq!(
            bench.hold(&store, 5, haiku, 0, Some(10)).unwrap_err(),
            Short::HeldInFlight,
            "nothing is left while they are in flight"
        );
        // The first spent less than it set aside: the rest comes back.
        bench.settle(&store, first, Some(2_000));
        bench.settle(&store, second, None);
        assert_eq!(store.bench_spent(5).unwrap(), 2_000);
        let (cap, _) = bench.hold(&store, 5, haiku, 0, None).unwrap();
        assert_eq!(cap, 16_000, "8,000 micros left, at 50¢ a million");
        // A new day starts with the whole cap, whatever the old one still holds.
        let (cap, _) = bench.hold(&store, 6, haiku, 0, Some(100_000)).unwrap();
        assert_eq!(cap, 20_000);
    }

    #[test]
    fn the_bench_is_opened_only_by_its_files_whole_token() {
        let (_dir, file) = token_file(TOKEN);
        let bench = Bench::new(&file, 200);
        assert!(bench.opens(TOKEN));
        for other in ["", "bench", &TOKEN[..TOKEN.len() - 1], &format!("{TOKEN}x")] {
            assert!(!bench.opens(other), "{other:?}");
        }
        let (_short, short) = token_file("too-short");
        assert!(
            !Bench::new(&short, 200).opens("too-short"),
            "a token under 32 characters leaves the bench off"
        );
        let gone = Bench::new(file.with_file_name("missing"), 200);
        assert!(!gone.opens(TOKEN), "no file, no bench");
        // Deleting the file turns it off at once, without a restart.
        std::fs::remove_file(&file).unwrap();
        assert!(!bench.opens(TOKEN));
    }

    #[tokio::test]
    async fn the_bench_token_opens_nothing_but_the_model_relay() {
        let (hq_url, to_hq) = hq();
        let (upstream, _) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        for (method, uri) in [
            ("GET", "/team-relay/contracts"),
            ("POST", "/team-relay/contracts/homelab_status"),
            ("POST", "/team-relay/contracts/repo_checkout"),
            ("POST", "/team-relay/receipt"),
            ("GET", "/team-relay/check/1/bundle"),
            ("POST", "/team-relay/local/v1/chat/completions"),
            ("POST", "/team-gateway/mcp"),
            ("POST", "/team-gateway/v2/mcp"),
            ("POST", "/team-gateway/runner/next"),
        ] {
            let req = axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::from("{}"))
                .unwrap();
            let res = team_relay(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        }
        assert!(
            to_hq.lock().unwrap().is_empty(),
            "the bench's token never reaches HQ"
        );
    }

    #[tokio::test]
    async fn the_bench_reads_its_own_day_and_nobody_else_may() {
        let (hq_url, _) = hq();
        let (upstream, _) = anthropic();
        let (_dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        let res = team_relay(state.clone())
            .oneshot(ask(
                "/team-relay/v1/messages",
                TOKEN,
                &a_round("claude-haiku-5-5"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let read = |token: &str| {
            axum::http::Request::builder()
                .method("GET")
                .uri("/team-relay/bench")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };
        let res = team_relay(state.clone())
            .oneshot(read(TOKEN))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let said: serde_json::Value =
            serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(
            said,
            json!({ "day": today(), "cap_micros": 2_000_000, "spent_micros": 150,
                    "calls": 1, "prompt_tokens": 1_000, "completion_tokens": 100,
                    "cached_tokens": 0, "upstream_refusals": 0, "last_refusal": null })
        );
        for stranger in ["", "good", "bench"] {
            let res = team_relay(state.clone())
                .oneshot(read(stranger))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{stranger:?}");
        }
    }

    #[tokio::test]
    async fn without_its_file_the_bench_token_is_a_stranger() {
        let (hq_url, _) = hq();
        let (upstream, to_anthropic) = anthropic();
        let (dir, file) = token_file(TOKEN);
        let state = with_bench(node(&hq_url, &upstream), &file, 200);
        drop(dir);
        let res = team_relay(state)
            .oneshot(ask(
                "/team-relay/v1/messages",
                TOKEN,
                &a_round("claude-haiku-5-5"),
            ))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "HQ knows no such turn"
        );
        assert!(messages_sent(&to_anthropic).is_empty());
    }
}
