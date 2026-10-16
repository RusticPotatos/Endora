use super::*;

#[derive(Serialize)]
pub(super) struct PreferenceResponse {
    id: String,
    text: String,
    kind: String,
    at_ms: i64,
}

impl From<&Preference> for PreferenceResponse {
    fn from(p: &Preference) -> Self {
        Self {
            id: p.id().value().to_string(),
            text: p.text().to_owned(),
            kind: p.kind().name().to_owned(),
            at_ms: p.at().unix_millis(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct CreatePreferenceRequest {
    text: String,
    #[serde(default)]
    kind: Option<String>,
}

pub(super) async fn create_preference(
    State(state): State<AppState>,
    Json(req): Json<CreatePreferenceRequest>,
) -> Result<Json<PreferenceResponse>, ApiError> {
    let kind = match req.kind.as_deref() {
        Some(k) => PreferenceKind::from_name(k).ok_or_else(|| {
            ApiError(AppError::BadRequest {
                message: format!(
                    "unknown preference kind {k:?}; expected taste, authority, or context"
                ),
            })
        })?,
        None => PreferenceKind::Taste,
    };
    let understanding = state.understanding.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let preference = blocking(move || {
        usecases::create_preference(
            understanding.as_ref(),
            ids.as_ref(),
            clock.as_ref(),
            &req.text,
            kind,
        )
    })
    .await?;
    Ok(Json(PreferenceResponse::from(&preference)))
}

pub(super) async fn list_preferences(
    State(state): State<AppState>,
) -> Result<Json<Vec<PreferenceResponse>>, ApiError> {
    let understanding = state.understanding.clone();
    let prefs = blocking(move || usecases::list_preferences(understanding.as_ref())).await?;
    Ok(Json(prefs.iter().map(PreferenceResponse::from).collect()))
}

pub(super) async fn delete_preference(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let pref_id = id.parse::<u128>().map(PreferenceId::new).map_err(|_| {
        ApiError(AppError::NotFound {
            entity: "preference",
        })
    })?;
    let understanding = state.understanding.clone();
    blocking(move || usecases::delete_preference(understanding.as_ref(), pref_id)).await?;
    Ok(Json(json!({ "deleted": true })))
}

/// The full export of the user's data — the "exportable" memory right.
#[derive(Serialize)]
pub(super) struct ExportResponse {
    audit: Vec<AuditResponse>,
    messages: Vec<MessageResponse>,
    preferences: Vec<PreferenceResponse>,
    beliefs: Vec<serde_json::Value>,
    outcomes: Vec<serde_json::Value>,
    intentions: Vec<serde_json::Value>,
    research: Vec<serde_json::Value>,
}

impl From<&MemorySnapshot> for ExportResponse {
    fn from(s: &MemorySnapshot) -> Self {
        Self {
            audit: s.audit.iter().map(AuditResponse::from).collect(),
            messages: s.messages.iter().map(MessageResponse::from).collect(),
            preferences: s.preferences.iter().map(PreferenceResponse::from).collect(),
            beliefs: s.beliefs.iter().map(stored_belief_json).collect(),
            outcomes: s.outcomes.iter().map(outcome_json).collect(),
            intentions: s.intentions.iter().map(intention_json).collect(),
            research: s.research.iter().map(research_json).collect(),
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
            message: r#"send {"confirm": true} to permanently delete all data"#.to_owned(),
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
