//! The agenda loop in the node (ADR 0103): every minute, what the home-lab observer, the mail
//! reader and the calendar last saw becomes the agenda's signals, and its items. The console
//! reads the items for the Now line and the pinned cards, and the owner answers them here.

use super::*;
use endora_agenda::application::{Agenda, Answer as Reply, Item};
use endora_agenda::domain::{Kind, Level, Response, Signal, Source, event_signals};
use endora_agenda::infrastructure::SqliteAgendaStore;
use std::collections::BTreeSet;

/// How often the loop looks.
const EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// Restarts through the console's allowlisted container action: the same checks as the
/// owner's own button, including the containers Endora may never touch.
struct Restart(AppState);

impl endora_agenda::application::Restarter for Restart {
    fn restart(&self, target: &str) -> Result<(), String> {
        let now = self.0.clock.now().unix_millis();
        let request = json!({
            "request_id": format!("agenda-{now}-{}", target.get(..12).unwrap_or(target)),
            "kind": "container",
            "action": "restart",
            "target": target,
            "confirmed": true,
        });
        // The action answers for what it could not confirm too: only "complete" is a restart.
        let outcome = self.0.console.act(&request).map_err(said)?;
        if outcome["state"] == "complete" {
            return Ok(());
        }
        Err(said(
            outcome["error"].as_str().unwrap_or("it did not confirm"),
        ))
    }
}

/// Why the container action refused, as the owner reads it.
fn said(code: &str) -> String {
    match code {
        "protected_control_path" => "it's one Endora may not touch",
        "target_not_allowed" => "Portainer doesn't list it",
        "unconfigured" | "configuration" => "the container controls aren't set up",
        other => other,
    }
    .replace('_', " ")
}

/// Pushes through the house's notification channels, the way the brief reaches the phone.
struct Push(AppState);

impl endora_agenda::application::Pusher for Push {
    fn push(&self, text: &str) -> Result<(), String> {
        let mut failures = Vec::new();
        let mut sent = false;
        for (server, channel) in native_channels(self.0.config.as_ref()) {
            match channel.notify("Endora", text) {
                Some(Ok(())) => sent = true,
                Some(Err(why)) => failures.push(format!("{server}: {why}")),
                None => {}
            }
        }
        if sent {
            return Ok(());
        }
        if failures.is_empty() {
            return Err("no phone is set up to push to".to_owned());
        }
        Err(failures.join("; "))
    }
}

/// Activity: the agenda's lines in Endora's own record.
struct Trail(AppState);

impl endora_agenda::application::Activity for Trail {
    fn record(&self, at: i64, line: &str) {
        let _ = endora_application::EventLog::record(
            self.0.events.as_ref(),
            endora_application::Timestamp::from_unix_millis(at),
            line,
        );
    }
}

/// The containers seen running since the loop started, the last home-lab snapshot it read, and
/// how many looks it has had (the monitoring is read every fifth).
#[derive(Default)]
pub(super) struct Memory {
    seen_running: BTreeSet<String>,
    homelab_at: i64,
    looks: u64,
}

/// The monitoring is read every this many looks: alerts move in minutes, not seconds.
const MONITORING_EVERY: u64 = 5;

/// Runs the loop for the life of the process.
pub(super) fn spawn(state: AppState) {
    tokio::spawn(async move {
        let memory = Arc::new(std::sync::Mutex::new(Memory::default()));
        let mut ticker = tokio::time::interval(EVERY);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let _ = once(&state, &memory).await;
        }
    });
}

/// One look: every source read in full is an observation, then time moves the watches on.
/// Returns whether anything changed.
pub(super) async fn once(state: &AppState, memory: &Arc<std::sync::Mutex<Memory>>) -> bool {
    let state = state.clone();
    let memory = memory.clone();
    tokio::task::spawn_blocking(move || {
        let now = state.clock.now().unix_millis();
        let mut observations: Vec<(Source, Vec<Signal>)> = Vec::new();
        {
            let mut memory = memory
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let snapshot = state
                .homelab
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let at = snapshot["observed_at_ms"].as_i64().unwrap_or(0);
            if at > memory.homelab_at {
                memory.homelab_at = at;
                observations.extend(usecases::homelab_signals(
                    &snapshot,
                    &mut memory.seen_running,
                ));
            }
            memory.looks += 1;
            // The first look reads it too, so a restart does not wait five minutes to see.
            if memory.looks % MONITORING_EVERY == 1 {
                if let Ok(watched) = state.console.read("watch", "") {
                    observations.push((Source::Monitoring, usecases::monitoring_signals(&watched)));
                }
            }
        }
        let nightly = state
            .nightly_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        observations.push((
            Source::Endora,
            endora_agenda::domain::nightly_copy_signals(nightly.as_deref()),
        ));
        if state.mail_copy.is_some() {
            if let Ok(digests) = state
                .mail_store
                .digests_since(now - usecases::MAIL_RECENT_MS)
            {
                observations.push((Source::Mail, usecases::mail_signals(&digests, now)));
            }
        }
        let offset = state.clock.utc_offset_minutes();
        let (from, to, today, tomorrow) = usecases::calendar_window(now, offset);
        if let Ok(entries) =
            endora_application::Calendar::entries(state.calendar.as_ref(), from, to)
        {
            observations.push((Source::Calendar, event_signals(&entries, &today, &tomorrow)));
        }
        let (restart, push, trail) = (
            Restart(state.clone()),
            Push(state.clone()),
            Trail(state.clone()),
        );
        let agenda = Agenda::new(state.agenda.as_ref(), &restart, &push, &trail);
        let mut changed = false;
        for (source, signals) in &observations {
            match agenda.observe(*source, signals, now) {
                Ok(change) => changed |= change.any(),
                Err(error) => eprintln!("agenda: {} not taken in ({error})", source.name()),
            }
        }
        match agenda.tick(now) {
            Ok(moved) => changed |= moved,
            Err(error) => eprintln!("agenda: the watches did not move on ({error})"),
        }
        if changed {
            let _ = state.changes.send(());
        }
        changed
    })
    .await
    .unwrap_or(false)
}

/// The FYI items for the morning brief, other than mail (it has its own section).
pub(super) fn brief_lines(state: &AppState) -> Vec<String> {
    const MOST_LINES: usize = 6;
    let items: Vec<Item> =
        endora_agenda::application::AgendaStore::open_items(state.agenda.as_ref())
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.level == Level::Fyi && !i.kind.from_mail())
            .collect();
    if items.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!("Keeping an eye on {}:", items.len())];
    lines.extend(
        items
            .iter()
            .take(MOST_LINES)
            .map(|i| format!("{}.", i.sentence)),
    );
    lines
}

fn item_json(i: &Item) -> serde_json::Value {
    json!({
        "id": i.id,
        "kind": i.kind.name(),
        "kind_said": i.kind.said(),
        "level": i.level.name(),
        "level_said": i.level.said(),
        "sentence": i.sentence,
        "why": i.why,
        "opened_at": i.opened_at,
        "updated_at": i.updated_at,
        "watching_until": i.watch_until,
        "from_mail": i.kind.from_mail(),
        "stop_said": i.kind.family().rule_said(&i.about),
    })
}

type Answered = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;

fn refused(
    error: &endora_agenda::application::AgendaError,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": error.to_string() })),
    )
}

fn store(state: &AppState) -> Arc<SqliteAgendaStore> {
    state.agenda.clone()
}

/// What the agenda has open, and what the Now line says.
pub(super) async fn agenda(State(state): State<AppState>) -> Answered {
    let store = store(&state);
    let (items, checked) = tokio::task::spawn_blocking(move || {
        use endora_agenda::application::AgendaStore as _;
        Ok::<_, endora_agenda::application::AgendaError>((store.open_items()?, store.checked()?))
    })
    .await
    .map_err(|_| {
        refused(&endora_agenda::application::AgendaError(
            "unavailable".to_owned(),
        ))
    })?
    .map_err(|e| refused(&e))?;
    let needs_you = items.iter().filter(|i| i.level >= Level::NeedsYou).count();
    Ok(Json(json!({
        "now": {
            "watching": items.len(),
            "needs_you": needs_you,
            "checked": checked.iter().map(|(s, at)| json!({ "source": s.name(), "at": at })).collect::<Vec<_>>(),
        },
        "items": items.iter().map(item_json).collect::<Vec<_>>(),
    })))
}

/// The owner answers an item: done, dismiss or stop.
pub(super) async fn answer(
    State(state): State<AppState>,
    Path((id, how)): Path<(i64, String)>,
) -> Answered {
    let now = state.clock.now().unix_millis();
    let (restart, push, trail) = (
        Restart(state.clone()),
        Push(state.clone()),
        Trail(state.clone()),
    );
    let store = store(&state);
    let done = tokio::task::spawn_blocking(move || {
        let agenda = Agenda::new(store.as_ref(), &restart, &push, &trail);
        match how.as_str() {
            "done" => agenda
                .answer(id, Reply::Done, now)
                .map(|()| json!({ "ok": true })),
            "dismiss" => agenda
                .answer(id, Reply::Dismiss, now)
                .map(|()| json!({ "ok": true })),
            "stop" => agenda
                .stop(id, now)
                .map(|rule| json!({ "ok": true, "rule": rule.family.rule_said(&rule.about) })),
            _ => Err(endora_agenda::application::AgendaError(
                "no such answer".to_owned(),
            )),
        }
    })
    .await
    .map_err(|_| {
        refused(&endora_agenda::application::AgendaError(
            "unavailable".to_owned(),
        ))
    })?
    .map_err(|e| refused(&e))?;
    let _ = state.changes.send(());
    Ok(Json(done))
}

/// What the owner taught it, and what each kind does: Settings.
pub(super) async fn rules(State(state): State<AppState>) -> Answered {
    let store = store(&state);
    let (restart, push, trail) = (
        Restart(state.clone()),
        Push(state.clone()),
        Trail(state.clone()),
    );
    let body = tokio::task::spawn_blocking(move || {
        use endora_agenda::application::AgendaStore as _;
        let agenda = Agenda::new(store.as_ref(), &restart, &push, &trail);
        let rules = store.rules()?;
        let mut kinds = Vec::new();
        for kind in Kind::ALL {
            let allowed: Vec<_> = Response::ALL
                .into_iter()
                .filter(|r| kind.allows(*r))
                .map(|r| json!({ "name": r.name(), "said": r.said() }))
                .collect();
            kinds.push(json!({
                "kind": kind.name(),
                "said": kind.said(),
                "response": agenda.response_for(kind)?.name(),
                "default": kind.default_response().name(),
                "allowed": allowed,
            }));
        }
        Ok::<_, endora_agenda::application::AgendaError>(json!({
            "rules": rules.iter().map(|r| json!({
                "id": r.id, "said": r.family.rule_said(&r.about), "at": r.at,
            })).collect::<Vec<_>>(),
            "kinds": kinds,
        }))
    })
    .await
    .map_err(|_| {
        refused(&endora_agenda::application::AgendaError(
            "unavailable".to_owned(),
        ))
    })?
    .map_err(|e| refused(&e))?;
    Ok(Json(body))
}

/// Undoes a taught rule.
pub(super) async fn forget(State(state): State<AppState>, Path(id): Path<i64>) -> Answered {
    let now = state.clock.now().unix_millis();
    let (restart, push, trail) = (
        Restart(state.clone()),
        Push(state.clone()),
        Trail(state.clone()),
    );
    let store = store(&state);
    tokio::task::spawn_blocking(move || {
        Agenda::new(store.as_ref(), &restart, &push, &trail).forget_rule(id, now)
    })
    .await
    .map_err(|_| {
        refused(&endora_agenda::application::AgendaError(
            "unavailable".to_owned(),
        ))
    })?
    .map_err(|e| refused(&e))?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(super) struct Relevel {
    response: Option<String>,
}

/// Re-levels a kind, or puts it back with `null`.
pub(super) async fn relevel(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Json(req): Json<Relevel>,
) -> Answered {
    let unknown = |what: &str| {
        refused(&endora_agenda::application::AgendaError(format!(
            "no such {what}"
        )))
    };
    let kind = Kind::from_name(&kind).ok_or_else(|| unknown("kind"))?;
    let response = match req.response.as_deref() {
        None => None,
        Some(name) => Some(Response::from_name(name).ok_or_else(|| unknown("response"))?),
    };
    let now = state.clock.now().unix_millis();
    let (restart, push, trail) = (
        Restart(state.clone()),
        Push(state.clone()),
        Trail(state.clone()),
    );
    let store = store(&state);
    tokio::task::spawn_blocking(move || {
        Agenda::new(store.as_ref(), &restart, &push, &trail).set_response(kind, response, now)
    })
    .await
    .map_err(|_| {
        refused(&endora_agenda::application::AgendaError(
            "unavailable".to_owned(),
        ))
    })?
    .map_err(|e| refused(&e))?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use endora_infrastructure::{RandomIdSource, ScriptedButler, SqliteStore};

    fn node() -> AppState {
        AppState::new(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
            Arc::new(ScriptedButler),
            Arc::new(endora_understanding::BrainFolder::temporary().unwrap()),
        )
    }

    fn containers(state: &AppState, at: i64, crashed: bool) {
        let item = if crashed {
            json!({"id": "c".repeat(64), "name": "sonarr", "state": "exited", "health": "unknown", "exit": 1})
        } else {
            json!({"id": "c".repeat(64), "name": "sonarr", "state": "running", "health": "healthy"})
        };
        *state.homelab.lock().unwrap() = json!({"observed_at_ms": at, "services": [
            {"name": "docker", "state": "degraded", "items": [item]}]});
    }

    #[tokio::test]
    async fn a_crash_the_node_sees_goes_through_the_container_action_and_is_told_when_refused() {
        let state = node();
        let memory = Arc::new(std::sync::Mutex::new(Memory::default()));
        containers(&state, 1, false);
        assert!(!once(&state, &memory).await, "running is nothing to say");
        containers(&state, 2, true);
        assert!(once(&state, &memory).await);
        let items =
            endora_agenda::application::AgendaStore::open_items(state.agenda.as_ref()).unwrap();
        assert_eq!(items.len(), 1);
        // No container controls in a test node: the action refuses, and the owner is told why.
        assert_eq!(items[0].level, Level::NeedsYou);
        assert!(
            items[0].why.starts_with("I couldn't restart it:"),
            "{}",
            items[0].why
        );
        // The same snapshot read again is not a new observation.
        assert!(!once(&state, &memory).await);
        let Json(body) = agenda(State(state.clone())).await.unwrap();
        assert_eq!(body["now"]["needs_you"], 1);
        assert_eq!(
            body["items"][0]["stop_said"],
            "Leave the container sonarr alone"
        );
        let id = items[0].id;
        let Json(stopped) = answer(State(state.clone()), Path((id, "stop".to_owned())))
            .await
            .unwrap();
        assert_eq!(stopped["rule"], "Leave the container sonarr alone");
        let Json(rules) = rules(State(state.clone())).await.unwrap();
        assert_eq!(
            rules["rules"][0]["said"],
            "Leave the container sonarr alone"
        );
        assert!(
            endora_agenda::application::AgendaStore::open_items(state.agenda.as_ref())
                .unwrap()
                .is_empty()
        );
    }
}
