use super::*;

#[derive(Deserialize)]
pub(super) struct Page {
    #[serde(default = "first")]
    page: usize,
}
fn first() -> usize {
    1
}

/// Locked displays receive counts, never container names or conversations.
pub(super) async fn overview(State(state): State<AppState>) -> Json<serde_json::Value> {
    let snapshot = state
        .homelab
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let age = state
        .clock
        .now()
        .unix_millis()
        .saturating_sub(snapshot["observed_at_ms"].as_i64().unwrap_or(0));
    let services: Vec<_> = snapshot["services"].as_array().into_iter().flatten().map(|service| {
        let observed = service["state"].as_str().unwrap_or("unavailable");
        json!({"name":service["name"],"state":if age > 60_000 && matches!(observed,"healthy"|"degraded") {"stale"} else {observed},"total":service["total"]})
    }).collect();
    Json(json!({"observed_at_ms":snapshot["observed_at_ms"],"services":services}))
}

pub(super) async fn dashboard(
    State(state): State<AppState>,
    Query(page): Query<Page>,
) -> Json<serde_json::Value> {
    let mut snapshot = state
        .homelab
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let stale =
        state.clock.now().unix_millis() - snapshot["observed_at_ms"].as_i64().unwrap_or(0) > 60_000;
    if let Some(services) = snapshot["services"].as_array_mut() {
        for service in services {
            if stale && matches!(service["state"].as_str(), Some("healthy" | "degraded")) {
                service["state"] = "stale".into();
            }
            if let Some(items) = service["items"].as_array() {
                let offset = page.page.clamp(1, 5).saturating_sub(1) * 20;
                let shown: Vec<_> = items.iter().skip(offset).take(20).cloned().collect();
                let cached = items.len();
                service["items"] = json!(shown);
                service["cached_items"] = cached.into();
                service["page"] = page.page.clamp(1, 5).into();
                service["page_size"] = 20.into();
            }
        }
    }
    Json(snapshot)
}

pub(crate) fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let settings = state.config.clone();
            let now = state.clock.now().unix_millis();
            let result = tokio::task::spawn_blocking(move || {
                let reader = endora_capabilities::infrastructure::homelab::Reader::new(settings);
                endora_capabilities::application::observe_homelab(&reader, now)
            })
            .await;
            if let Ok(snapshot) = result {
                *state
                    .homelab
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
            }
        }
    });
}
