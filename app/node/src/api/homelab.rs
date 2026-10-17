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

/// The owner's time of day, as an alarm says it: 14:02.
fn hh_mm(now_ms: i64, offset_minutes: i64) -> String {
    let minutes = (now_ms.div_euclid(60_000) + offset_minutes).rem_euclid(24 * 60);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Tells HQ of new trouble in the home lab (ADR 0107), signed with HQ's token: one call a
/// container or service that went from well to trouble, which HQ's routine for the home lab
/// takes. Nothing when HQ is not set up (and HQ itself refuses while its demo is on). Each
/// alarm is in Endora's audit log. Returns how many HQ was told of.
pub(super) fn raise_alarms(
    state: &AppState,
    watch: &mut endora_capabilities::lab_alarms::LabWatch,
    snapshot: &serde_json::Value,
    now: i64,
    offset_minutes: i64,
) -> usize {
    let Some(hq) = state.hq.as_ref() else {
        return 0;
    };
    let mut told = 0;
    for alarm in watch.observe(snapshot, now) {
        let said = alarm.said(&hh_mm(now, offset_minutes));
        match hq.post("/v1/alarms", &json!({ "text": said })) {
            Ok(_) => {
                told += 1;
                super::hq::record(state, &format!("Endora raised an alarm: {said}"));
            }
            Err(e) => eprintln!("homelab: HQ was not told that {said}: {e}"),
        }
    }
    told
}

/// How often the node says the home lab's containers to HQ when they have not changed: HQ
/// keeps them in memory, so one restarted learns them again within this.
const NAMES_AGAIN_AFTER_MS: i64 = 10 * 60_000;

/// The names of the containers in a snapshot of the home lab, sorted, each once.
fn container_names(snapshot: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = snapshot["services"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["name"] == "docker")
        .flat_map(|s| s["items"].as_array().into_iter().flatten())
        .filter_map(|i| i["name"].as_str())
        .map(|n| n.trim_start_matches('/').to_owned())
        .filter(|n| !n.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Tells HQ which containers the home lab has (`POST /v1/lab/names`, signed with HQ's token),
/// so a question to the owner can be checked against them before it reaches the desk: when
/// they change, and every [`NAMES_AGAIN_AFTER_MS`]. Nothing when HQ is not set up, or when the
/// snapshot names no container (an observer that could not look says nothing is there).
/// `last` is what HQ was told, and when. Returns whether HQ was told now.
pub(super) fn tell_lab_names(
    state: &AppState,
    last: &mut Option<(Vec<String>, i64)>,
    snapshot: &serde_json::Value,
    now: i64,
) -> bool {
    let Some(hq) = state.hq.as_ref() else {
        return false;
    };
    let names = container_names(snapshot);
    if names.is_empty() {
        return false;
    }
    if let Some((told, at)) = last.as_ref() {
        if *told == names && now - at < NAMES_AGAIN_AFTER_MS {
            return false;
        }
    }
    match hq.post("/v1/lab/names", &json!({ "names": names })) {
        Ok(_) => {
            *last = Some((names, now));
            true
        }
        Err(e) => {
            eprintln!("homelab: HQ was not told the containers' names: {e}");
            false
        }
    }
}

pub(crate) fn spawn(state: AppState) {
    // What Endora has seen of the home lab, for its alarms: in memory, begun again on restart.
    let watch = Arc::new(std::sync::Mutex::new(
        endora_capabilities::lab_alarms::LabWatch::new(),
    ));
    // The containers' names as HQ was last told them, and when.
    let told = Arc::new(std::sync::Mutex::new(None::<(Vec<String>, i64)>));
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
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
                if state.hq.is_some() {
                    let (state, watch, told) = (state.clone(), watch.clone(), told.clone());
                    let offset = state.clock.utc_offset_minutes();
                    let _ = tokio::task::spawn_blocking(move || {
                        let mut watch = watch
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        raise_alarms(&state, &mut watch, &snapshot, now, offset);
                        let mut told = told
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        tell_lab_names(&state, &mut told, &snapshot, now);
                    })
                    .await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{fake, node};
    use super::*;

    fn lab(sonarr: &str) -> serde_json::Value {
        json!({ "services": [{ "name": "docker", "state": "healthy", "items": [
            { "name": "sonarr", "state": "running", "health": sonarr },
            { "name": "endora-hq", "state": "running", "health": sonarr },
        ] }] })
    }

    #[test]
    fn the_time_of_day_is_the_owners() {
        let noon = 1_792_152_000_000; // 2026-10-16 12:00 UTC
        assert_eq!(hh_mm(noon, 0), "12:00");
        assert_eq!(hh_mm(noon + 2 * 60_000, 120), "14:02");
        assert_eq!(hh_mm(noon, -13 * 60), "23:00");
    }

    #[test]
    fn new_trouble_is_told_to_hq_signed_once_and_recorded() {
        let (url, seen) = fake(Arc::new(|_, _| {
            (
                200,
                "application/json",
                r#"{"fired":"created","task":3}"#.to_owned(),
            )
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let mut watch = endora_capabilities::lab_alarms::LabWatch::new();
        let noon = 1_792_152_000_000; // 2026-10-16 12:00 UTC
        assert_eq!(
            raise_alarms(&state, &mut watch, &lab("healthy"), noon, 0),
            0
        );
        let then = noon + 2 * 60_000;
        assert_eq!(
            raise_alarms(&state, &mut watch, &lab("unhealthy"), then, 120),
            1
        );
        assert_eq!(
            raise_alarms(&state, &mut watch, &lab("unhealthy"), then + 30_000, 120),
            0
        );
        let sent = seen.lock().unwrap().clone();
        assert_eq!(sent.len(), 1, "endora-hq is Endora's own");
        assert_eq!(sent[0].0, "/v1/alarms");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent[0].2).unwrap(),
            json!({ "text": "sonarr became unhealthy at 14:02" })
        );
        assert!(
            sent[0]
                .1
                .iter()
                .any(|(k, v)| k == "authorization" && v.starts_with("Bearer "))
        );
        let audit: Vec<String> = endora_application::AuditLog::recent(state.audit.as_ref(), 5)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect();
        assert!(
            audit
                .iter()
                .any(|a| a == "HQ: Endora raised an alarm: sonarr became unhealthy at 14:02"),
            "{audit:?}"
        );
    }

    #[test]
    fn hq_is_told_the_containers_names_when_they_change_and_now_and_then() {
        let (url, seen) = fake(Arc::new(|_, _| {
            (200, "application/json", r#"{"ok":true}"#.to_owned())
        }));
        let state = node(&url, "http://127.0.0.1:9");
        let mut told = None;
        let noon = 1_792_152_000_000;
        assert!(tell_lab_names(&state, &mut told, &lab("healthy"), noon));
        assert!(
            !tell_lab_names(&state, &mut told, &lab("unhealthy"), noon + 30_000),
            "the same names: not again yet"
        );
        assert!(tell_lab_names(
            &state,
            &mut told,
            &lab("healthy"),
            noon + NAMES_AGAIN_AFTER_MS
        ));
        let fewer = json!({ "services": [{ "name": "docker", "state": "healthy", "items": [
            { "name": "/sonarr", "state": "running" }] }] });
        assert!(tell_lab_names(
            &state,
            &mut told,
            &fewer,
            noon + NAMES_AGAIN_AFTER_MS + 1
        ));
        let blind = json!({ "services": [{ "name": "docker", "state": "unavailable" }] });
        assert!(
            !tell_lab_names(&state, &mut told, &blind, noon + 2 * NAMES_AGAIN_AFTER_MS),
            "an observer that saw nothing says nothing"
        );
        let sent = seen.lock().unwrap().clone();
        let bodies: Vec<serde_json::Value> = sent
            .iter()
            .map(|(path, _, body)| {
                assert_eq!(path, "/v1/lab/names");
                serde_json::from_str(body).unwrap()
            })
            .collect();
        assert_eq!(
            bodies,
            [
                json!({ "names": ["endora-hq", "sonarr"] }),
                json!({ "names": ["endora-hq", "sonarr"] }),
                json!({ "names": ["sonarr"] }),
            ]
        );
        assert!(
            sent[0]
                .1
                .iter()
                .any(|(k, v)| k == "authorization" && v.starts_with("Bearer "))
        );
    }

    #[test]
    fn without_hq_nothing_is_raised() {
        let mut state = node("http://127.0.0.1:9", "http://127.0.0.1:9");
        state.hq = None;
        let mut watch = endora_capabilities::lab_alarms::LabWatch::new();
        raise_alarms(&state, &mut watch, &lab("healthy"), 0, 0);
        assert_eq!(
            raise_alarms(&state, &mut watch, &lab("unhealthy"), 60_000, 0),
            0
        );
    }
}
