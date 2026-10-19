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

#[derive(Deserialize)]
pub(in crate::api) struct HqGuardMode {
    mode: String,
}

/// The owner switches a guard that is tried in shadow on, or back to shadow (ADR 0133).
pub(in crate::api) async fn hq_guard_mode(
    State(state): State<AppState>,
    Path(guard): Path<String>,
    Json(req): Json<HqGuardMode>,
) -> Answer {
    // A guard is named in lowercase letters and underscores. Anything else is not one, and is not
    // made part of HQ's address.
    if guard.is_empty() || !guard.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no such guard" })),
        ));
    }
    let mode = req.mode;
    let answer = ask(
        &state,
        format!("/v1/guards/{guard}/mode"),
        Some(json!({ "mode": mode })),
    )
    .await?;
    let name = guard.replace('_', " ");
    record(
        &state,
        &if mode == "on" {
            format!("you switched the {name} guard on")
        } else {
            format!("you put the {name} guard back in shadow")
        },
    );
    Ok(answer)
}

#[derive(Deserialize)]
pub(in crate::api) struct HqRetract {
    target: String,
}

/// The owner says a note, a section or a story is not true (ADR 0124): HQ takes it out and
/// answers with what else says the same.
pub(in crate::api) async fn hq_retract(
    State(state): State<AppState>,
    Json(req): Json<HqRetract>,
) -> Answer {
    let answer = ask(
        &state,
        "/v1/retractions".to_owned(),
        Some(json!({ "target": req.target })),
    )
    .await?;
    record(&state, &format!("you said {} is not true", req.target));
    Ok(answer)
}

/// What else says what a retraction said.
pub(in crate::api) async fn hq_sweep(State(state): State<AppState>, Path(id): Path<i64>) -> Answer {
    ask(&state, format!("/v1/retractions/{id}/sweep"), None).await
}

#[derive(Deserialize)]
pub(in crate::api) struct HqForget {
    targets: Vec<String>,
}

/// The owner forgets, at once, what of a retraction's sweep they ticked.
pub(in crate::api) async fn hq_forget_swept(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<HqForget>,
) -> Answer {
    let answer = ask(
        &state,
        format!("/v1/retractions/{id}/forget"),
        Some(json!({ "targets": req.targets })),
    )
    .await?;
    record(
        &state,
        &format!(
            "you forgot what said retracted claim {id} again: {}",
            req.targets.join(", ")
        ),
    );
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::super::super::relay::tests::{fake, node};
    use super::*;

    /// Switching the budget of talk on, or back to shadow, goes to HQ signed and is in the audit
    /// log; a name that is not a guard's is not made part of HQ's address.
    #[tokio::test]
    async fn the_owners_mode_for_a_guard_passes_through_signed_and_is_recorded() {
        let (url, seen) = fake(Arc::new(|_, _| {
            (
                200,
                "application/json",
                json!({ "guard": "coordination", "mode": "on" }).to_string(),
            )
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let mode = |m: &str| HqGuardMode { mode: m.to_owned() };
        let _ = hq_guard_mode(
            State(state.clone()),
            Path("coordination".to_owned()),
            Json(mode("on")),
        )
        .await
        .unwrap();
        let _ = hq_guard_mode(
            State(state.clone()),
            Path("coordination".to_owned()),
            Json(mode("shadow")),
        )
        .await
        .unwrap();
        let sent = seen.lock().unwrap().clone();
        assert_eq!(sent[0].0, "/v1/guards/coordination/mode");
        let body = |i: usize| serde_json::from_str::<serde_json::Value>(&sent[i].2).unwrap();
        assert_eq!(body(0), json!({ "mode": "on" }));
        assert_eq!(body(1), json!({ "mode": "shadow" }));
        for odd in ["../stop", "a/b", "Coordination", ""] {
            let refused =
                hq_guard_mode(State(state.clone()), Path(odd.to_owned()), Json(mode("on")))
                    .await
                    .unwrap_err();
            assert_eq!(refused.0, StatusCode::NOT_FOUND, "{odd:?}");
        }
        assert_eq!(seen.lock().unwrap().len(), 2, "none of those was passed on");
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 10)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        assert!(
            audit.contains(&"HQ: you switched the coordination guard on".to_owned())
                && audit.contains(&"HQ: you put the coordination guard back in shadow".to_owned()),
            "{audit:?}"
        );
    }

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
