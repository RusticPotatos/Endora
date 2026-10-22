//! The owner's bounded conversation views and explicit scope grants, through the node.

use super::*;

#[derive(Deserialize)]
pub(in crate::api) struct ConversationCursor {
    #[serde(default)]
    after: i64,
}

pub(in crate::api) async fn hq_conversation_operations(
    State(state): State<AppState>,
    Query(q): Query<ConversationCursor>,
) -> Answer {
    ask(
        &state,
        format!("/v1/conversations/operations?after={}", q.after.max(0)),
        None,
    )
    .await
}

pub(in crate::api) async fn hq_conversations(
    State(state): State<AppState>,
    Query(q): Query<ConversationCursor>,
) -> Answer {
    ask(
        &state,
        format!("/v1/conversations?after={}", q.after.max(0)),
        None,
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::api) struct ConversationCreate {
    project: i64,
    participants: [i64; 2],
}

pub(in crate::api) async fn hq_conversation_create(
    State(state): State<AppState>,
    Json(req): Json<ConversationCreate>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/conversations".into(),
        Some(json!({"project":req.project,"participants":req.participants})),
    )
    .await?;
    record(&state, "you authorized an information conversation");
    Ok(answer)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::api) struct ConversationEnable {
    enabled: bool,
}

pub(in crate::api) async fn hq_conversation_enable(
    State(state): State<AppState>,
    Json(req): Json<ConversationEnable>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/conversations/enabled".into(),
        Some(json!({"enabled":req.enabled})),
    )
    .await?;
    record(
        &state,
        if req.enabled {
            "you enabled information messaging"
        } else {
            "you disabled information messaging"
        },
    );
    Ok(answer)
}

pub(in crate::api) async fn hq_conversation_page(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ConversationCursor>,
) -> Answer {
    ask(
        &state,
        format!("/v1/conversations/{id}?after={}", q.after.max(0)),
        None,
    )
    .await
}

pub(in crate::api) async fn hq_conversation_delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/conversations/{id}/delete"),
        Some(json!({})),
    )
    .await?;
    record(
        &state,
        &format!("you deleted conversation {id}; its contents are being purged"),
    );
    Ok(answer)
}
