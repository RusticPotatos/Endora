//! The brain over HTTP (ADR 0101): what Endora knows about the person, page by page, for
//! the console's About you.

use super::*;

/// Memory, and one line per page.
pub(super) async fn brain_overview(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let knowledge = state.knowledge.clone();
    let (memory, pages) = blocking(move || {
        let memory = endora_application::Brain::read(
            knowledge.as_ref(),
            &endora_application::PagePath::memory(),
        )?
        .unwrap_or_default();
        let pages = endora_application::Brain::pages(knowledge.as_ref())?;
        Ok::<_, AppError>((memory, pages))
    })
    .await?;
    Ok(Json(json!({
        "memory": memory,
        "pages": pages
            .iter()
            .filter(|p| !p.path.is_memory())
            .map(|p| json!({ "path": p.path.as_str(), "title": p.title, "line": p.line }))
            .collect::<Vec<_>>(),
    })))
}

fn page_path(raw: &str) -> Result<endora_application::PagePath, ApiError> {
    endora_application::PagePath::parse(raw).map_err(|e| {
        ApiError(AppError::BadRequest {
            message: e.to_string(),
        })
    })
}

/// One page, whole, with what happened to it.
pub(super) async fn read_brain_page(
    State(state): State<AppState>,
    Path(raw): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let path = page_path(&raw)?;
    let knowledge = state.knowledge.clone();
    let (text, history) = blocking(move || {
        let text = endora_application::Brain::read(knowledge.as_ref(), &path)?;
        let history = endora_application::Brain::history(knowledge.as_ref(), &path)?;
        Ok::<_, AppError>((text, history))
    })
    .await?;
    let Some(text) = text else {
        return Err(ApiError(AppError::NotFound { entity: "page" }));
    };
    Ok(Json(json!({
        "path": raw,
        "text": text,
        "history": history
            .iter()
            .map(|h| json!({ "at": h.at, "why": h.why }))
            .collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub(super) struct PageEdit {
    text: String,
}

/// The person's own edit of a page, as theirs in its history.
pub(super) async fn write_brain_page(
    State(state): State<AppState>,
    Path(raw): Path<String>,
    Json(req): Json<PageEdit>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let knowledge = state.knowledge.clone();
    let clock = state.clock.clone();
    let changed = blocking(move || {
        usecases::edit_page(
            knowledge.as_ref(),
            &raw,
            &req.text,
            &usecases::today(clock.as_ref()),
        )
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "changed": changed })))
}

/// Removes a page; its history keeps it.
pub(super) async fn delete_brain_page(
    State(state): State<AppState>,
    Path(raw): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let knowledge = state.knowledge.clone();
    let clock = state.clock.clone();
    let deleted = blocking(move || {
        usecases::delete_page(knowledge.as_ref(), &raw, &usecases::today(clock.as_ref()))
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "deleted": deleted })))
}

#[derive(Deserialize)]
pub(super) struct ForgetRequest {
    #[serde(default)]
    confirm: bool,
}

/// Forgets everything Endora learned, the history included, and every intention it formed
/// from it; keeps what the person told it and the research they asked for. Needs
/// `{"confirm": true}`, like the full purge.
pub(super) async fn forget_what_it_learned(
    State(state): State<AppState>,
    Json(req): Json<ForgetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !req.confirm {
        return Err(ApiError(AppError::BadRequest {
            message: r#"send {"confirm": true} to delete everything Endora learned"#.to_owned(),
        }));
    }
    let knowledge = state.knowledge.clone();
    let understanding = state.understanding.clone();
    let clock = state.clock.clone();
    let (pages, intentions) = blocking(move || {
        let pages =
            usecases::forget_what_it_learned(knowledge.as_ref(), &usecases::today(clock.as_ref()))?;
        let intentions =
            endora_application::IntentionRepository::forget_its_own(understanding.as_ref())?;
        Ok::<_, AppError>((pages, intentions))
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "forgot": pages, "intentions": intentions })))
}
