use super::*;

fn error(category: &str) -> ApiError {
    ApiError(AppError::BadRequest {
        message: format!("Home console: {category}"),
    })
}
async fn read(
    state: AppState,
    view: &'static str,
    query: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    let console = state.console.clone();
    let result = tokio::task::spawn_blocking(move || console.read(view, &query))
        .await
        .map_err(|_| error("unavailable"))?;
    result.map(Json).map_err(error)
}
pub(super) async fn home(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "home-catalog", search.query).await
}
pub(super) async fn media(
    State(state): State<AppState>,
    Query(search): Query<Search>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "media", search.query).await
}
pub(super) async fn requests(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "requests", String::new()).await
}
pub(super) async fn containers(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "containers", String::new()).await
}
pub(super) async fn dashboard(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "dashboard", String::new()).await
}
pub(super) async fn logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "logs", id).await
}
pub(super) async fn jobs(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "jobs", String::new()).await
}
pub(super) async fn job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    read(state, "job", id).await
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
) -> Result<Json<serde_json::Value>, ApiError> {
    let console = state.console.clone();
    let request = serde_json::to_value(request).map_err(|_| error("invalid_request"))?;
    tokio::task::spawn_blocking(move || console.act(&request))
        .await
        .map_err(|_| error("unavailable"))?
        .map(Json)
        .map_err(error)
}
pub(super) async fn operation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let console = state.console.clone();
    tokio::task::spawn_blocking(move || console.operation(&id))
        .await
        .map_err(|_| error("unavailable"))?
        .map(Json)
        .map_err(error)
}
