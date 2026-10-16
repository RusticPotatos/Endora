use super::*;

fn error(category: &str) -> ApiError {
    ApiError(AppError::BadRequest {
        message: format!("Home console: {category}"),
    })
}
/// What the console built, as the protocol says the handheld reads it (ADR 0099). A response
/// that does not fit its type is refused here rather than sent: the handheld would read it by
/// field name and quietly show nothing.
fn as_protocol<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<Json<T>, ApiError> {
    serde_json::from_value(value)
        .map(Json)
        .map_err(|_| error("protocol"))
}

async fn read_value(
    state: AppState,
    view: &'static str,
    query: String,
) -> Result<serde_json::Value, ApiError> {
    let console = state.console.clone();
    let result = tokio::task::spawn_blocking(move || console.read(view, &query))
        .await
        .map_err(|_| error("unavailable"))?;
    result.map_err(error)
}

async fn read(
    state: AppState,
    view: &'static str,
    query: String,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    as_protocol(read_value(state, view, query).await?)
}
pub(super) async fn home(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    read(state, "home", String::new()).await
}
#[derive(Deserialize)]
pub(super) struct Search {
    #[serde(default)]
    query: String,
}
pub(super) async fn catalog(
    State(state): State<AppState>,
    Query(search): Query<Search>,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    read(state, "home-catalog", search.query).await
}
pub(super) async fn media(
    State(state): State<AppState>,
    Query(search): Query<Search>,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    read(state, "media", search.query).await
}
pub(super) async fn requests(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    read(state, "requests", String::new()).await
}
pub(super) async fn containers(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    read(state, "containers", String::new()).await
}
pub(super) async fn dashboard(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::Dashboard>, ApiError> {
    as_protocol(read_value(state, "dashboard", String::new()).await?)
}
pub(super) async fn logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    read(state, "logs", id).await
}
pub(super) async fn jobs(
    State(state): State<AppState>,
) -> Result<Json<endora_protocol::ConsoleView>, ApiError> {
    read(state, "jobs", String::new()).await
}
// One job's run: the job agent's own output and state, passed through as it gave them.
pub(super) async fn job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read_value(state, "job", id).await.map(Json)
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Action {
    request_id: String,
    kind: String,
    action: String,
    target: String,
    confirmed: bool,
}
pub(super) async fn action(
    State(state): State<AppState>,
    Json(request): Json<Action>,
) -> Result<Json<endora_protocol::Operation>, ApiError> {
    let console = state.console.clone();
    let request = serde_json::to_value(request).map_err(|_| error("invalid_request"))?;
    let outcome = tokio::task::spawn_blocking(move || console.act(&request))
        .await
        .map_err(|_| error("unavailable"))?
        .map_err(error)?;
    as_protocol(outcome)
}
pub(super) async fn operation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<endora_protocol::Operation>, ApiError> {
    let console = state.console.clone();
    let found = tokio::task::spawn_blocking(move || console.operation(&id))
        .await
        .map_err(|_| error("unavailable"))?
        .map_err(error)?;
    as_protocol(found)
}
