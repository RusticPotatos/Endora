//! The guards through the node (ADR 0124): the owner's verdict on a guard's finding. Every
//! change is in Endora's audit log.

use super::*;

#[derive(Deserialize)]
pub(in crate::api) struct HqVerdict {
    verdict: String,
    #[serde(default)]
    fix_worked: Option<bool>,
}

/// The owner says whether a guard's finding was real, and whether its fix worked.
pub(in crate::api) async fn hq_finding_verdict(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqVerdict>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/guards/findings/{id}/verdict"),
        Some(json!({ "verdict": req.verdict, "fix_worked": req.fix_worked })),
    )
    .await?;
    let fix = match req.fix_worked {
        Some(true) => ", and its fix worked",
        Some(false) => ", and its fix did not work",
        None => "",
    };
    record(
        &state,
        &format!("you judged guard finding {id} {}{fix}", req.verdict),
    );
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::super::super::relay::tests::{fake, node};
    use super::*;

    #[tokio::test]
    async fn the_owners_verdict_passes_through_signed_and_is_recorded() {
        let (url, seen) = fake(Arc::new(|_, _| {
            (200, "application/json", json!({ "finding": 3 }).to_string())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let _ = hq_finding_verdict(
            State(state.clone()),
            Path(3),
            Json(HqVerdict {
                verdict: "false_positive".to_owned(),
                fix_worked: None,
            }),
        )
        .await
        .unwrap();
        let sent = seen.lock().unwrap().clone();
        assert_eq!(sent[0].0, "/v1/guards/findings/3/verdict");
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        assert_eq!(audit, ["HQ: you judged guard finding 3 false_positive"]);
    }
}
