//! The providers and the ordered lists on the AI screen (ADR 0105). A provider's key goes in
//! once and never comes back out: the screen learns only whether one is set.

use super::*;
use endora_capabilities::model_lists::{Job, ModelListRepository as _};

type Answer = Result<Json<serde_json::Value>, ApiError>;

fn job(name: &str) -> Result<Job, AppError> {
    Job::from_name(name).ok_or_else(|| AppError::BadRequest {
        message: format!("no list called {name}"),
    })
}

/// Everything the AI screen shows about models: the providers, both lists in order, and
/// whether deep may be used on its own.
pub(super) async fn lists(State(state): State<AppState>) -> Answer {
    let config = state.config.clone();
    let brain = state.brain.clone();
    let out = blocking(move || {
        let providers = config.providers().map_err(AppError::Repository)?;
        let entries = |j: Job| -> Result<Vec<serde_json::Value>, AppError> {
            Ok(config
                .entries(j)
                .map_err(AppError::Repository)?
                .iter()
                .map(|e| {
                    let p = providers.iter().find(|p| p.id == e.provider);
                    json!({
                        "id": e.id,
                        "provider": e.provider,
                        "provider_name": p.map(|p| p.name.clone()).unwrap_or_default(),
                        "model": e.model,
                        "enabled": e.enabled,
                        "local": p.is_some_and(|p| endora_infrastructure::is_private_endpoint(&p.url)),
                    })
                })
                .collect())
        };
        let escalate = DeepModelRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .is_some_and(|d| d.escalate);
        let serving = brain.as_ref().map(|b| b.serving());
        Ok::<_, AppError>(json!({
            "providers": providers.iter().map(|p| json!({
                "id": p.id,
                "name": p.name,
                "url": p.url,
                "key_set": !p.api_key.is_empty(),
                "local": endora_infrastructure::is_private_endpoint(&p.url),
            })).collect::<Vec<_>>(),
            "everyday": entries(Job::Everyday)?,
            "deep": entries(Job::Deep)?,
            "escalate": escalate,
            "serving": serving.map(|(url, model)| json!({ "model": model, "local": endora_infrastructure::is_private_endpoint(&url) })),
        }))
    })
    .await?;
    Ok(Json(out))
}

#[derive(Deserialize)]
pub(super) struct ProviderRequest {
    #[serde(default)]
    name: String,
    url: String,
    /// A new key; absent or blank keeps the one stored.
    #[serde(default)]
    api_key: Option<String>,
}

fn checked(req: &ProviderRequest) -> Result<(String, String), AppError> {
    let url = req.url.trim().trim_end_matches('/').to_owned();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::BadRequest {
            message: "the endpoint must start with http:// or https://".to_owned(),
        });
    }
    let name = if req.name.trim().is_empty() {
        endora_capabilities::model_lists::name_for(&url)
    } else {
        req.name.trim().to_owned()
    };
    Ok((name, url))
}

/// Adds a provider.
pub(super) async fn add_provider(
    State(state): State<AppState>,
    Json(req): Json<ProviderRequest>,
) -> Answer {
    let (name, url) = checked(&req).map_err(ApiError)?;
    let key = req.api_key.unwrap_or_default().trim().to_owned();
    let config = state.config.clone();
    let said = format!("Added the provider {name}");
    let id = blocking(move || {
        config
            .add_provider(&name, &url, &key)
            .map_err(AppError::Repository)
    })
    .await?;
    record_event(state.events.as_ref(), state.clock.as_ref(), &said);
    let _ = state.changes.send(());
    Ok(Json(json!({ "id": id })))
}

/// Changes a provider; its key only when a new one is given.
pub(super) async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<ProviderRequest>,
) -> Answer {
    let (name, url) = checked(&req).map_err(ApiError)?;
    let key = req
        .api_key
        .map(|k| k.trim().to_owned())
        .filter(|k| !k.is_empty());
    let config = state.config.clone();
    let said = format!(
        "Changed the provider {name}{}",
        if key.is_some() {
            ", with a new key"
        } else {
            ""
        }
    );
    blocking(move || {
        config
            .update_provider(id, &name, &url, key.as_deref())
            .map_err(AppError::Repository)
    })
    .await?;
    record_event(state.events.as_ref(), state.clock.as_ref(), &said);
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// Removes a provider and every entry that uses it.
pub(super) async fn remove_provider(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    let config = state.config.clone();
    let name = blocking(move || {
        let name = config
            .providers()
            .map_err(AppError::Repository)?
            .into_iter()
            .find(|p| p.id == id)
            .map(|p| p.name)
            .ok_or(AppError::NotFound { entity: "provider" })?;
        config.remove_provider(id).map_err(AppError::Repository)?;
        Ok::<_, AppError>(name)
    })
    .await?;
    record_event(
        state.events.as_ref(),
        state.clock.as_ref(),
        &format!("Removed the provider {name}, and its places in the lists"),
    );
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

/// The models a provider offers, asked with its stored key.
pub(super) async fn discover(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    let config = state.config.clone();
    let models = blocking(move || {
        let p = config
            .providers()
            .map_err(AppError::Repository)?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(AppError::NotFound { entity: "provider" })?;
        endora_infrastructure::list_models(&p.url, &p.api_key)
            .map_err(|e| AppError::Model { message: e })
    })
    .await?;
    Ok(Json(json!({ "models": models })))
}

#[derive(Deserialize)]
pub(super) struct EntryRequest {
    job: String,
    provider: i64,
    model: String,
}

/// Adds an entry at the end of a list.
pub(super) async fn add_entry(
    State(state): State<AppState>,
    Json(req): Json<EntryRequest>,
) -> Answer {
    let job = job(&req.job).map_err(ApiError)?;
    let model = req.model.trim().to_owned();
    if model.is_empty() {
        return Err(ApiError(AppError::BadRequest {
            message: "which model?".to_owned(),
        }));
    }
    let config = state.config.clone();
    let said = format!("Added {model} to the {} list", job.name());
    let id = blocking(move || {
        if !config
            .providers()
            .map_err(AppError::Repository)?
            .iter()
            .any(|p| p.id == req.provider)
        {
            return Err(AppError::NotFound { entity: "provider" });
        }
        config
            .add_entry(job, req.provider, &model)
            .map_err(AppError::Repository)
    })
    .await?;
    record_event(state.events.as_ref(), state.clock.as_ref(), &said);
    let _ = state.changes.send(());
    Ok(Json(json!({ "id": id })))
}

/// Moves, switches, removes or tests one entry.
pub(super) async fn entry_action(
    State(state): State<AppState>,
    Path((id, action)): Path<(i64, String)>,
) -> Answer {
    let config = state.config.clone();
    let out = blocking(move || {
        let found = [Job::Everyday, Job::Deep]
            .into_iter()
            .find_map(|j| config.entries(j).ok()?.into_iter().find(|e| e.id == id))
            .ok_or(AppError::NotFound { entity: "entry" })?;
        let list = found.job.name();
        let said = match action.as_str() {
            "up" | "down" => {
                config
                    .move_entry(id, action == "up")
                    .map_err(AppError::Repository)?;
                format!("Moved {} {action} in the {list} list", found.model)
            }
            "on" | "off" => {
                config
                    .enable_entry(id, action == "on")
                    .map_err(AppError::Repository)?;
                format!("Switched {} {action} in the {list} list", found.model)
            }
            "remove" => {
                config.remove_entry(id).map_err(AppError::Repository)?;
                format!("Removed {} from the {list} list", found.model)
            }
            "test" => {
                let p = config
                    .providers()
                    .map_err(AppError::Repository)?
                    .into_iter()
                    .find(|p| p.id == found.provider)
                    .ok_or(AppError::NotFound { entity: "provider" })?;
                let result =
                    endora_infrastructure::test_connection(&p.url, &p.api_key, &found.model);
                // A failed test is an expected answer, not an error.
                return Ok::<_, AppError>((
                    None,
                    match result {
                        Ok(detail) => json!({ "ok": true, "detail": detail }),
                        Err(detail) => json!({ "ok": false, "detail": detail }),
                    },
                ));
            }
            _ => {
                return Err(AppError::BadRequest {
                    message: format!("no such action: {action}"),
                });
            }
        };
        Ok((Some(said), json!({ "ok": true })))
    })
    .await?;
    if let Some(said) = out.0 {
        record_event(state.events.as_ref(), state.clock.as_ref(), &said);
        let _ = state.changes.send(());
    }
    Ok(Json(out.1))
}

#[derive(Deserialize)]
pub(super) struct EscalateRequest {
    on: bool,
}

/// Whether Endora may use the deep list on its own (ADR 0055): one switch for the list.
pub(super) async fn set_escalate(
    State(state): State<AppState>,
    Json(req): Json<EscalateRequest>,
) -> Answer {
    let config = state.config.clone();
    blocking(move || {
        let current = DeepModelRepository::get(config.as_ref())
            .map_err(AppError::Repository)?
            .unwrap_or_default();
        DeepModelRepository::set(
            config.as_ref(),
            &endora_application::DeepModel {
                escalate: req.on,
                ..current
            },
        )
        .map_err(AppError::Repository)
    })
    .await?;
    record_event(
        state.events.as_ref(),
        state.clock.as_ref(),
        if req.on {
            "Let Endora use the deep list on its own"
        } else {
            "Stopped Endora using the deep list on its own"
        },
    );
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}
