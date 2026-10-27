//! The team's world through the node (ADR 0122): the owner reads it, downloads it as Markdown,
//! and alone makes something canon or demotes it. Every change is in Endora's audit log.

use super::*;

/// The world as the HQ screen shows it.
pub(in crate::api) async fn hq_world(State(state): State<AppState>) -> Answer {
    ask(&state, "/v1/world".to_owned(), None).await
}

/// The world as one Markdown file, to keep.
pub(in crate::api) async fn hq_world_export(State(state): State<AppState>) -> Response {
    let answer = match ask(&state, "/v1/world/export".to_owned(), None).await {
        Ok(answer) => answer,
        Err(failed) => return failed.into_response(),
    };
    let markdown = answer.0["markdown"].as_str().unwrap_or_default().to_owned();
    (
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/markdown; charset=utf-8".to_owned(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"team-world.md\"".to_owned(),
            ),
        ],
        markdown,
    )
        .into_response()
}

#[derive(Deserialize)]
pub(in crate::api) struct HqSection {
    page: String,
    #[serde(default)]
    key: String,
    title: String,
    text: String,
}

/// The owner writes a section of a page, or replaces one.
pub(in crate::api) async fn hq_world_section(
    State(state): State<AppState>,
    Json(req): Json<HqSection>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/world/section".to_owned(),
        Some(json!({ "page": req.page, "key": req.key, "title": req.title, "text": req.text })),
    )
    .await?;
    record(
        &state,
        &format!("you wrote \"{}\" on world:{}", req.title, req.page),
    );
    Ok(answer)
}

#[derive(Deserialize)]
pub(in crate::api) struct HqTimeline {
    page: String,
    #[serde(default)]
    at: Option<i64>,
    text: String,
}

/// The owner adds an entry to a page's timeline.
pub(in crate::api) async fn hq_world_timeline(
    State(state): State<AppState>,
    Json(req): Json<HqTimeline>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/world/timeline".to_owned(),
        Some(json!({ "page": req.page, "at": req.at, "text": req.text })),
    )
    .await?;
    record(
        &state,
        &format!("you added to the timeline of world:{}", req.page),
    );
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::super::super::relay::tests::{fake, node};
    use super::*;

    #[tokio::test]
    async fn the_owners_world_passes_through_signed_and_each_change_is_recorded() {
        let (url, seen) = fake(Arc::new(|path, _| {
            let body = if path == "/v1/world/export" {
                json!({ "markdown": "# The team's world\n" })
            } else {
                json!({ "pages": [], "stories": [] })
            };
            (200, "application/json", body.to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let world = hq_world(State(state.clone())).await.unwrap();
        assert_eq!(world.0["stories"], json!([]));
        let download = hq_world_export(State(state.clone())).await;
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(
            download.headers()[axum::http::header::CONTENT_DISPOSITION],
            "attachment; filename=\"team-world.md\""
        );
        let _ = hq_world_section(
            State(state.clone()),
            Json(HqSection {
                page: "house".to_owned(),
                key: "nas".to_owned(),
                title: "The NAS".to_owned(),
                text: "The NAS has 16 GB.".to_owned(),
            }),
        )
        .await
        .unwrap();
        let _ = hq_world_timeline(
            State(state.clone()),
            Json(HqTimeline {
                page: "house".to_owned(),
                at: None,
                text: "Moved the router.".to_owned(),
            }),
        )
        .await
        .unwrap();
        let sent: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _, _)| p.clone())
            .collect();
        assert_eq!(
            sent,
            [
                "/v1/world",
                "/v1/world/export",
                "/v1/world/section",
                "/v1/world/timeline"
            ]
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        for line in [
            "HQ: you wrote \"The NAS\" on world:house",
            "HQ: you added to the timeline of world:house",
        ] {
            assert!(audit.iter().any(|a| a == line), "{line}: {audit:?}");
        }
        assert_eq!(
            audit.len(),
            2,
            "reading and downloading record nothing: {audit:?}"
        );
    }
}
