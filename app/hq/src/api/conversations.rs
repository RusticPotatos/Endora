//! Owner-only scope, inspection and deletion for the information pilot (ADR 0137).

use axum::extract::{DefaultBodyLimit, Path, Query, State as Extract};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Failed, State, blocking};
use endora_team::domain::assistance::SourceRef;
use endora_team::domain::{MemberId, ProjectId};

pub(super) fn routes() -> Router<State> {
    Router::new()
        .route("/v1/conversations", get(list).post(create))
        .route("/v1/conversations/enabled", post(enable))
        .route("/v1/conversations/operations", get(operations))
        .route("/v1/conversations/{id}", get(page))
        .route("/v1/conversations/{id}/assistance", get(assistance))
        .route(
            "/v1/conversations/{id}/assistance/{request}/cancel",
            post(cancel_assistance),
        )
        .route("/v1/conversations/{id}/sources", post(grant_source))
        .route("/v1/conversations/{id}/sources/revoke", post(revoke_source))
        .route("/v1/conversations/{id}/delete", post(delete))
        .layer(DefaultBodyLimit::max(16 * 1024))
}

#[derive(Deserialize, Default)]
struct Cursor {
    #[serde(default)]
    after: i64,
}

async fn operations(
    Extract(state): Extract<State>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    Ok(Json(
        blocking(move || hq.conversation_operations(cursor.after)).await?,
    ))
}

async fn list(
    Extract(state): Extract<State>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    Ok(Json(
        blocking(move || hq.conversation_directory(None, cursor.after)).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enable {
    enabled: bool,
}

async fn enable(
    Extract(state): Extract<State>,
    Json(req): Json<Enable>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    blocking(move || hq.set_conversations_enabled(req.enabled)).await?;
    Ok(Json(json!({"enabled":req.enabled})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    project: i64,
    participants: [i64; 2],
}

async fn create(
    Extract(state): Extract<State>,
    Json(req): Json<Create>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    let id = blocking(move || {
        hq.create_conversation(ProjectId(req.project), req.participants.map(MemberId))
    })
    .await?;
    Ok(Json(json!({"conversation":id})))
}

async fn page(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    Ok(Json(
        blocking(move || hq.conversation_messages(id, cursor.after)).await?,
    ))
}

async fn assistance(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    Ok(Json(
        blocking(move || hq.assistance_requests_page(id, cursor.after)).await?,
    ))
}

async fn cancel_assistance(
    Extract(state): Extract<State>,
    Path((id, request)): Path<(i64, i64)>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    blocking(move || hq.cancel_assistance_request(id, request)).await?;
    Ok(Json(
        json!({"conversation":id,"request":request,"state":"cancelled"}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceGrant {
    source: String,
}

async fn grant_source(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<SourceGrant>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    let source = parse_source(&req.source)?;
    blocking(move || hq.grant_assistance_source(id, source)).await?;
    Ok(Json(
        json!({"conversation":id,"source":source.said(),"granted":true}),
    ))
}

async fn revoke_source(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
    Json(req): Json<SourceGrant>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    let source = parse_source(&req.source)?;
    blocking(move || hq.revoke_assistance_source(id, source)).await?;
    Ok(Json(
        json!({"conversation":id,"source":source.said(),"granted":false}),
    ))
}

fn parse_source(text: &str) -> Result<SourceRef, Failed> {
    SourceRef::parse(text).ok_or(Failed(
        endora_team::application::engine::EngineError::Refused(
            endora_team::domain::Refusal::Conversation("invalid source locator"),
        ),
    ))
}

async fn delete(
    Extract(state): Extract<State>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, Failed> {
    let hq = state.hq;
    blocking(move || hq.delete_conversation(id)).await?;
    Ok(Json(
        json!({"conversation":id,"unavailable":true,"purging":true}),
    ))
}
