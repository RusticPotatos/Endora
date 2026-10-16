use super::*;

/// Serves the web console's HTML shell (embedded in the binary; see ADR 0050).
/// The styles and script are separate files (`/styles.css`, `/app.js`) so the
/// console is organized by responsibility, not one giant file — still embedded,
/// still no build step.
pub(super) async fn index() -> impl axum::response::IntoResponse {
    // Cache-bust the assets per build: the shell is always revalidated (no-cache),
    // and it points at `/app.js?v=<build>` / `/styles.css?v=<build>` so a deploy
    // serves fresh script/styles even to a browser that heuristically cached the
    // old ones (the assets themselves are then immutable per build, below).
    let v = endora_application::build_id();
    let html = include_str!("../web/index.html")
        .replace("/app.js", &format!("/app.js?v={v}"))
        .replace("/styles.css", &format!("/styles.css?v={v}"));
    (
        [(axum::http::header::CACHE_CONTROL, "no-cache")],
        Html(html),
    )
}

/// The console's stylesheet (embedded). Immutable per build — the shell versions
/// the URL, so a new build fetches a new URL rather than a stale cached one.
pub(super) async fn console_css() -> impl axum::response::IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable",
            ),
        ],
        include_str!("../web/styles.css"),
    )
}

/// The console's script, in the order it is joined. The parts share one global scope exactly
/// as the single file did (they are concatenated, not loaded as modules), so each is a slice of
/// the old `app.js`, cut at a top-level boundary. `scripts/check-console.mjs` joins the same
/// files in the same order.
const CONSOLE_SCRIPT_PARTS: [&str; 12] = [
    include_str!("../web/js/00-state.js"),
    include_str!("../web/js/10-core.js"),
    include_str!("../web/js/20-chat.js"),
    include_str!("../web/js/30-settings.js"),
    include_str!("../web/js/40-skills.js"),
    include_str!("../web/js/45-lab.js"),
    include_str!("../web/js/47-hq.js"),
    include_str!("../web/js/48-agenda.js"),
    include_str!("../web/js/50-views.js"),
    include_str!("../web/js/60-chat-actions.js"),
    include_str!("../web/js/70-dispatch.js"),
    include_str!("../web/js/80-boot.js"),
];

static CONSOLE_SCRIPT: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| CONSOLE_SCRIPT_PARTS.concat());

/// The console's script (embedded). Immutable per build (see [`console_css`]).
pub(super) async fn console_js() -> impl axum::response::IntoResponse {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable",
            ),
        ],
        CONSOLE_SCRIPT.as_str(),
    )
}

pub(super) async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let stt = super::ai::speech_url(state.config.as_ref()).is_some();
    // Whether a turn is being taken right now.
    //
    // The console decides whether to show a thinking bubble, and it could only ever ask
    // itself. So a turn that failed left the last stored message being the person's, with
    // nothing after it, and the screen sat on an animated ellipsis **forever** — through a
    // reload, through a restart, because the state it was rendering lived in a tab that had
    // gone. A butler that cannot be spoken to again is worse than one that answers badly.
    //
    // The lock the turn already takes is the honest answer, and it is the same answer on
    // every device rather than per-tab hope.
    let busy = state.turn_lock.try_lock().is_err();
    // Which brain is serving turns right now (ADR 0072) — so "which model answered
    // that?" is a fact on a screen. Read from config + the cached health verdict; a
    // fallback butler with no stored config reports nothing.
    let (serving_url, serving_model) = state
        .brain
        .as_ref()
        .map(|b| b.serving())
        .unwrap_or_default();
    Json(json!({
        "busy": busy,
        "serving_url": serving_url,
        "serving_model": serving_model,
        "status": "ok",
        "service": endora_application::platform_identity(),
        "version": endora_application::version(),
        "build": endora_application::build_id(),
        // Whether a speech-to-text server is configured, so the console can use
        // real transcription for push-to-talk instead of the browser's flaky one.
        "stt": stt,
    }))
}

/// Whisper (and similar STT) hallucinate a stock phrase repeated many times when
/// handed near-silent or non-speech audio — e.g. "Torsdagsfotografi" twenty times in
/// a row. Real speech varies; a hallucinated loop is one or two tokens over and over.
/// We treat such a transcript as nothing said (so conversation mode keeps listening
/// and push-to-talk inserts nothing) rather than send it as if the person spoke it.
pub(super) fn looks_like_stt_hallucination(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 6 {
        return false;
    }
    let distinct: std::collections::HashSet<String> =
        words.iter().map(|w| w.to_lowercase()).collect();
    (distinct.len() as f64) / (words.len() as f64) <= 0.34
}

/// Transcribes a recording (raw audio bytes in the body) via the configured
/// speech-to-text server (`ENDORA_STT_URL`, OpenAI-compatible). The node proxies
/// it so the STT host is never exposed to the page; 503 when none is configured.
pub(super) async fn transcribe(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    // The server chosen on the AI screen, or the deployment's.
    let Some(url) = super::ai::speech_url(state.config.as_ref()) else {
        return Err(ApiError(AppError::Model {
            message: "no speech-to-text server configured".to_owned(),
        }));
    };
    let text = blocking(move || {
        endora_infrastructure::transcribe_audio(url.trim(), &body)
            .map_err(|e| AppError::Model { message: e })
    })
    .await?;
    // Drop the classic silence-hallucination so it never reaches the conversation.
    let text = if looks_like_stt_hallucination(&text) {
        String::new()
    } else {
        text
    };
    Ok(Json(json!({ "text": text })))
}

#[derive(Deserialize)]
pub(super) struct AuditQuery {
    limit: Option<usize>,
}

#[derive(Serialize)]
pub(super) struct AuditResponse {
    id: String,
    at_ms: i64,
    summary: String,
}

impl From<&AuditRecord> for AuditResponse {
    fn from(r: &AuditRecord) -> Self {
        Self {
            id: r.id().value().to_string(),
            at_ms: r.at().unix_millis(),
            summary: r.summary().to_owned(),
        }
    }
}

pub(super) async fn audit(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditResponse>>, ApiError> {
    let limit = query.limit.unwrap_or(50);
    let audit = state.audit.clone();
    let records = blocking(move || usecases::recent_audit(audit.as_ref(), limit)).await?;
    Ok(Json(records.iter().map(AuditResponse::from).collect()))
}

#[derive(Serialize)]
pub(super) struct ActivityResponse {
    at_ms: i64,
    kind: String,
    summary: String,
}

impl From<&ActivityItem> for ActivityResponse {
    fn from(item: &ActivityItem) -> Self {
        Self {
            at_ms: item.at().unix_millis(),
            kind: item.kind().name().to_owned(),
            summary: item.summary().to_owned(),
        }
    }
}

/// The activity feed: a merged, newest-first timeline of what has happened.
pub(super) async fn activity(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<ActivityResponse>>, ApiError> {
    let limit = query.limit.unwrap_or(50);
    let audit = state.audit.clone();
    let events = state.events.clone();
    let items =
        blocking(move || usecases::recent_activity(audit.as_ref(), events.as_ref(), limit)).await?;
    Ok(Json(items.iter().map(ActivityResponse::from).collect()))
}

/// A server-sent event stream that emits a `changed` event whenever a write
/// succeeds. Clients re-read `/v1/activity` (and other state) on each event —
/// the stream carries a nudge, never the data itself.
pub(super) async fn activity_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.changes.subscribe();
    let stream = unfold(rx, |mut rx| async move {
        // A closed channel ends the stream; a lag still means "something
        // changed", so both a value and a lag emit one `changed` event.
        match rx.recv().await {
            Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                Some((Ok(Event::default().event("changed").data("changed")), rx))
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Serialize)]
pub(super) struct MessageResponse {
    pub(super) id: String,
    pub(super) role: String,
    pub(super) text: String,
    pub(super) at_ms: i64,
}

impl From<&ChatMessage> for MessageResponse {
    fn from(m: &ChatMessage) -> Self {
        Self {
            id: m.id().value().to_string(),
            role: m.role().name().to_owned(),
            text: m.text().to_owned(),
            at_ms: m.at().unix_millis(),
        }
    }
}

/// Builds a capability runner that honours the person's enable/disable choices
/// (ADR 0054) and their autonomy envelope (ADR 0051). Reading config is
/// best-effort: on failure it falls back to defaults, so a glitch never breaks the
/// butler.
/// Records one line to the butler's action log (best-effort — transparency, not
/// the critical path, so a failure never breaks the turn).
pub(super) fn record_event(
    events: &endora_platform::EventStore,
    clock: &SystemClock,
    summary: &str,
) {
    use endora_application::{Clock, EventLog};
    let _ = events.record(clock.now(), summary);
}
