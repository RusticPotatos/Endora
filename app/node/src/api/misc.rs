use super::*;

/// The full export of Endora's record — the "exportable" memory right. Since the chat was
/// retired (ADR 0136) that is what Activity shows: the audit trail and the event log.
#[derive(Serialize)]
pub(super) struct ExportResponse {
    audit: Vec<AuditResponse>,
    events: Vec<serde_json::Value>,
}

impl From<&MemorySnapshot> for ExportResponse {
    fn from(s: &MemorySnapshot) -> Self {
        Self {
            audit: s.audit.iter().map(AuditResponse::from).collect(),
            events: s
                .events
                .iter()
                .map(|e| json!({ "at_ms": e.at.unix_millis(), "summary": e.summary }))
                .collect(),
        }
    }
}

pub(super) async fn export(
    State(state): State<AppState>,
) -> Result<Json<ExportResponse>, ApiError> {
    let store = state.store.clone();
    let snapshot = blocking(move || usecases::export_memory(store.as_ref())).await?;
    Ok(Json(ExportResponse::from(&snapshot)))
}

#[derive(Deserialize)]
pub(super) struct PurgeRequest {
    #[serde(default)]
    confirm: bool,
}

pub(super) async fn purge(
    State(state): State<AppState>,
    Json(req): Json<PurgeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !req.confirm {
        return Err(ApiError(AppError::BadRequest {
            message: r#"send {"confirm": true} to permanently delete Endora's record"#.to_owned(),
        }));
    }
    let store = state.store.clone();
    blocking(move || usecases::purge_memory(store.as_ref())).await?;
    Ok(Json(json!({ "purged": true })))
}

/// Runs blocking use-case work on the blocking thread pool, mapping a task
/// failure to a backend error.
pub(super) async fn blocking<T>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, ApiError>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => result.map_err(ApiError),
        Err(_) => Err(ApiError(AppError::Repository(RepositoryError::Backend(
            "worker task failed".to_owned(),
        )))),
    }
}

/// Wraps [`AppError`] so it can be turned into an HTTP response.
pub(super) struct ApiError(pub(super) AppError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message, code) = match &self.0 {
            AppError::Domain(e) => (StatusCode::BAD_REQUEST, e.to_string(), None),
            AppError::BadRequest { message } => (StatusCode::BAD_REQUEST, message.clone(), None),
            AppError::Conflict { code, message } => {
                (StatusCode::CONFLICT, message.clone(), Some(*code))
            }
            AppError::NotFound { .. } => (StatusCode::NOT_FOUND, self.0.to_string(), None),
            AppError::Model { message } => (StatusCode::SERVICE_UNAVAILABLE, message.clone(), None),
            // Don't leak backend detail to clients.
            AppError::Repository(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".to_owned(),
                None,
            ),
        };
        (status, Json(json!({ "error": message, "code": code }))).into_response()
    }
}
