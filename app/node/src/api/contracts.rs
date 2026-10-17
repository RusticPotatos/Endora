//! The team's contracts on the node (ADR 0106): what Endora offers HQ's workers on the home
//! lab, served on the team's listener beside the model relays, and opened by a turn's own
//! token, checked with HQ as the relays check it. A worker proposes a call; the node decides
//! it by fixed rules (the owner's switches, Endora's own containers, the restart limits) and
//! does it through its own console adapter. No key and no raw access leave the node.
//!
//! Every call is written to Endora's audit log with the member and the run. A restart lands
//! on the owner's desk in HQ as a notice.

use super::relay::{Turn, token_of, turn_of};
use super::*;
use axum::body::Bytes;
use axum::http::HeaderMap;
use endora_capabilities::ansible_nas::{self, Docs};
use endora_capabilities::application::HomelabObserver as _;
use endora_capabilities::contracts::{
    self, ANSIBLE_NAS_DOC, CONTAINER_LOGS, Contract, HOMELAB_MAP, HOMELAB_STATUS,
    RESTART_CONTAINER, RestartLedger,
};
use endora_capabilities::lab_map;

/// Where the owner's switches are kept, among the node's settings.
const SWITCHES: &str = "hq-contracts";

/// How long the home lab's snapshot may go unseen before it is stale.
const STALE_AFTER_MS: i64 = 60_000;

/// A refusal, as a contract says one: a status and what went wrong, for the model to read.
fn refuse(status: StatusCode, _kind: &str, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// The owner's switches: every contract, and the restart alone. On unless switched off; off
/// when they cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Switches {
    all: bool,
    restart: bool,
}

fn switches_of(state: &AppState) -> Switches {
    let Ok(settings) = state.config.all_settings() else {
        return Switches {
            all: false,
            restart: false,
        };
    };
    let off = |key: &str| {
        settings
            .iter()
            .any(|(id, k, v)| id == SWITCHES && k == key && v == "off")
    };
    Switches {
        all: !off("all"),
        restart: !off(RESTART_CONTAINER),
    }
}

/// The contracts a turn is offered, as the owner has them switched.
fn offered(switches: Switches) -> Vec<Contract> {
    if !switches.all {
        return Vec::new();
    }
    contracts::all()
        .into_iter()
        .filter(|c| switches.restart || c.name != RESTART_CONTAINER)
        .collect()
}

/// Whether a turn may be given what a contract returns: a Confidential one (a container's
/// log) only to a turn on the house's model, or to the lead's on Claude, as the owner decided
/// (ADR 0109); and an act never to the CEO, which takes no step on the house (ADR 0111).
fn may_see(contract: &Contract, turn: &Turn) -> bool {
    contract.offered_to(&turn.role, turn.cloud)
}

/// The switches as the HQ screen shows them.
pub(super) fn switches_said(state: &AppState) -> serde_json::Value {
    let switches = switches_of(state);
    json!({ "all": switches.all, "restart_container": switches.restart })
}

/// Whose turn the request's token is, or the refusal: 401 for a token that opens no turn.
async fn the_turn(state: &AppState, headers: &HeaderMap) -> Result<Turn, Response> {
    let Some(hq) = state.hq.clone() else {
        return Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "",
            "HQ is not set up on this node",
        ));
    };
    turn_of(hq, token_of(headers), refuse).await
}

/// `GET /team-relay/contracts`: the contracts a turn may call, then the bridged tools its
/// role and its model may use (ADR 0108), as MCP lists tools, each with its kind.
pub(super) async fn list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let turn = match the_turn(&state, &headers).await {
        Ok(turn) => turn,
        Err(refused) => return refused,
    };
    let switches = switches_of(&state);
    let mut tools: Vec<serde_json::Value> = offered(switches)
        .iter()
        .filter(|c| may_see(c, &turn))
        .map(|c| {
            let mut tool = c.as_tool();
            tool["kind"] = if c.kind == contracts::Kind::Read {
                "read"
            } else {
                "act"
            }
            .into();
            tool
        })
        .collect();
    if switches.all {
        let looked = state.clone();
        let bridged =
            tokio::task::spawn_blocking(move || super::bridge::offered_to(&looked, &turn))
                .await
                .unwrap_or_default();
        tools.extend(
            bridged
                .iter()
                .map(endora_capabilities::bridge::Bridged::as_tool),
        );
    }
    Json(json!({ "tools": tools })).into_response()
}

/// What a contract did: what the model reads, and what the audit log says.
struct Done {
    text: String,
    said: String,
}

/// Why a contract was refused or failed: a status, and what the model reads.
type Refused = (StatusCode, String);

/// `POST /team-relay/contracts/{name}`: one call of a contract, its arguments the JSON body.
pub(super) async fn call(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // The token first, so a stranger cannot make the node do anything at all.
    let turn = match the_turn(&state, &headers).await {
        Ok(turn) => turn,
        Err(refused) => return refused,
    };
    let args = if body.iter().all(u8::is_ascii_whitespace) {
        json!({})
    } else {
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
    };
    // Every call is in the audit log, these refusals too; a name the worker made up is
    // written as one short line.
    let refused_for = |why: &str| {
        record(
            &state,
            &format!(
                "{} (run {}) was refused {}: {why}",
                who(&turn),
                turn.run,
                one_line(&name)
            ),
        );
    };
    if !args.is_object() {
        refused_for("the arguments must be one JSON object");
        return refuse(
            StatusCode::BAD_REQUEST,
            "",
            "the arguments must be one JSON object",
        );
    }
    if !contracts::all().iter().any(|c| c.name == name) {
        return bridged(state, turn, name, args).await;
    }
    let Some(contract) = offered(switches_of(&state))
        .into_iter()
        .find(|c| c.name == name)
    else {
        record(
            &state,
            &format!(
                "{} (run {}) was refused {name}: switched off",
                who(&turn),
                turn.run
            ),
        );
        return refuse(
            StatusCode::FORBIDDEN,
            "",
            &format!("the owner has switched {name} off"),
        );
    };
    if !may_see(&contract, &turn) {
        let why = if contract.may_be_seen_by(&turn.role, turn.cloud) {
            "the CEO takes no step on the house itself: hand it to the lead with create_task"
                .to_owned()
        } else {
            super::bridge::confidential(&name)
        };
        refused_for(&why);
        return refuse(StatusCode::FORBIDDEN, "", &why);
    }
    let state2 = state.clone();
    let name2 = name.clone();
    let member = who(&turn);
    let run = turn.run;
    let done = tokio::task::spawn_blocking(move || match name2.as_str() {
        HOMELAB_STATUS => Ok(homelab_status(&state2)),
        HOMELAB_MAP => homelab_map(&state2),
        ANSIBLE_NAS_DOC => ansible_nas_doc(&state2, &args),
        CONTAINER_LOGS => container_logs(&state2, &args),
        RESTART_CONTAINER => restart_container(&state2, &turn, &args),
        _ => Err((
            StatusCode::NOT_FOUND,
            format!("no contract is called {name2}"),
        )),
    })
    .await;
    match done {
        Ok(Ok(done)) => {
            record(&state, &format!("{member} (run {run}) {}", done.said));
            Json(json!({ "text": done.text })).into_response()
        }
        Ok(Err((status, why))) => {
            record(
                &state,
                &format!("{member} (run {run}) was refused {name}: {why}"),
            );
            refuse(status, "", &why)
        }
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "", "the contract failed"),
    }
}

/// A call of a bridged tool (ADR 0108): offered to this turn now, or refused as no contract.
async fn bridged(state: AppState, turn: Turn, name: String, args: serde_json::Value) -> Response {
    let member = who(&turn);
    let run = turn.run;
    let worked = state.clone();
    let called_name = name.clone();
    let done = tokio::task::spawn_blocking(move || {
        let switched_on = switches_of(&worked).all;
        // Found among every bridged tool, so that one this turn may not use is refused with
        // the reason (its role, its model), not as unknown.
        let tool = super::bridge::catalog(&worked)
            .into_iter()
            .find(|b| b.name == called_name)
            .filter(|_| switched_on);
        let Some(tool) = tool else {
            return Err((
                StatusCode::NOT_FOUND,
                format!("no contract is called {called_name}"),
            ));
        };
        super::bridge::call(&worked, &turn, &tool, &args)
    })
    .await;
    match done {
        Ok(Ok(called)) => {
            record(&state, &format!("{member} (run {run}) {}", called.said));
            let mut answer = json!({ "text": called.text });
            if called.asked_owner {
                answer["asked_owner"] = true.into();
            }
            Json(answer).into_response()
        }
        Ok(Err((status, why))) => {
            let why = if status == StatusCode::NOT_FOUND {
                "there is no such contract".to_owned()
            } else {
                why
            };
            record(
                &state,
                &format!(
                    "{member} (run {run}) was refused {}: {why}",
                    one_line(&name)
                ),
            );
            let said = if status == StatusCode::NOT_FOUND {
                format!("no contract is called {name}")
            } else {
                why
            };
            refuse(status, "", &said)
        }
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "", "the contract failed"),
    }
}

/// The member a turn is of, as the owner reads it.
fn who(turn: &Turn) -> String {
    if turn.member.is_empty() {
        return "A team member".to_owned();
    }
    turn.member.clone()
}

/// `homelab_status`: the observer's snapshot, made compact.
fn homelab_status(state: &AppState) -> Done {
    let snapshot = state
        .homelab
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let seen = compact(&snapshot, state.clock.now().unix_millis());
    Done {
        text: seen.to_string(),
        said: "read the home lab's state".to_owned(),
    }
}

/// The home lab as a turn reads it: each service's state and its items, the containers by
/// name, state, status text, health and exit code, and whether it is all stale (unseen for a
/// minute, or never seen). Ids are left out: a contract names a container.
fn compact(snapshot: &serde_json::Value, now: i64) -> serde_json::Value {
    let observed = snapshot["observed_at_ms"].as_i64().unwrap_or(0);
    let stale = observed == 0 || now - observed > STALE_AFTER_MS;
    let services: Vec<serde_json::Value> = snapshot["services"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|service| {
            let docker = service["name"] == "docker";
            let observed = service["state"].as_str().unwrap_or("unavailable");
            let state = if stale && matches!(observed, "healthy" | "degraded") {
                "stale"
            } else {
                observed
            };
            let items: Vec<serde_json::Value> = service["items"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| {
                    if !docker {
                        return json!({
                            "title": item["title"], "status": item["status"],
                            "progress": item["progress"].as_f64().map(f64::round),
                            "stuck": item["stuck"],
                        });
                    }
                    let mut container = json!({
                        "name": item["name"], "state": item["state"],
                        "status": item["status"], "health": item["health"],
                    });
                    if let Some(code) = item["exit"].as_i64() {
                        container["exit_code"] = code.into();
                    }
                    container
                })
                .collect();
            let mut out = json!({ "name": service["name"], "state": state, "items": items });
            if let Some(total) = service["total"].as_u64() {
                out["total"] = total.into();
            }
            if let Some(error) = service["error"].as_str() {
                out["error"] = error.into();
            }
            out
        })
        .collect();
    json!({ "observed_at_ms": observed, "stale": stale, "services": services })
}

/// How the home lab is built, as the node knows it (ADR 0112): the lab's ansible-nas docs,
/// the owner's retired list, and the last map, kept [`lab_map::MAP_FOR_MS`].
pub(crate) struct LabKnowledge {
    pub(crate) docs: Docs,
    pub(crate) retired: Vec<(String, String)>,
    map: std::sync::Mutex<Option<(i64, serde_json::Value)>>,
}

impl LabKnowledge {
    /// The docs and the retired list as given.
    pub(crate) fn new(docs: Docs, retired: Vec<(String, String)>) -> Self {
        Self {
            docs,
            retired,
            map: std::sync::Mutex::new(None),
        }
    }

    /// From the node's environment: `ENDORA_ANSIBLE_NAS_REPO`, `ENDORA_ANSIBLE_NAS_REF` and
    /// `ENDORA_LAB_RETIRED`.
    pub(crate) fn from_env() -> Self {
        let retired = std::env::var("ENDORA_LAB_RETIRED").unwrap_or_default();
        Self::new(Docs::from_env(), lab_map::retired_of(&retired))
    }
}

/// Why Endora's Docker observer could not be used, as the model reads it.
fn observer(code: &str) -> Refused {
    if code == "unconfigured" || code == "configuration" {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Endora's Docker observer is not set up, so the home lab's map is not available. Tell the owner if you need it.".to_owned(),
        );
    }
    (
        StatusCode::BAD_GATEWAY,
        format!("Docker did not answer ({})", code.replace('_', " ")),
    )
}

/// `homelab_map`: how the home lab is built, from Docker's own metadata through the observer's
/// fixed routes (its list, then each container's inspection), with who manages each container
/// from the lab's ansible-nas roles. Kept a minute.
fn homelab_map(state: &AppState) -> Result<Done, Refused> {
    let now = state.clock.now().unix_millis();
    let said = "read how the home lab is built".to_owned();
    let mut kept = state
        .lab
        .map
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, map)) = kept.as_ref() {
        if now - at < lab_map::MAP_FOR_MS {
            return Ok(Done {
                text: map.to_string(),
                said,
            });
        }
    }
    let reader = endora_capabilities::infrastructure::homelab::Reader::new(state.config.clone());
    let listed = reader.observe("docker").map_err(observer)?;
    let mut inspected = 0;
    let containers: Vec<serde_json::Value> = listed["items"]
        .as_array()
        .into_iter()
        .flatten()
        .take(100)
        .map(|item| {
            let name = item["name"].as_str().unwrap_or("");
            match reader.inspect(name) {
                Ok(seen) if seen["name"] != "" => {
                    inspected += 1;
                    seen
                }
                _ => json!({ "name": name, "state": item["state"] }),
            }
        })
        .collect();
    let roles = state.lab.docs.roles(now);
    let mut map = lab_map::build(&containers, roles.as_deref(), &state.lab.retired);
    map["observed_at_ms"] = now.into();
    map["ansible_nas"] = match (&roles, state.lab.docs.configured()) {
        (Some(roles), _) => format!("{} roles in the lab's ansible-nas", roles.len()),
        (None, false) => {
            "not configured: who manages a container is unknown (ansible_nas_doc says how)"
                .to_owned()
        }
        (None, true) => {
            "the role list could not be read from GitHub: who manages a container is unknown"
                .to_owned()
        }
    }
    .into();
    if inspected == 0 && !containers.is_empty() {
        map["note"] =
            "Docker's inspection was not available: no wiring and no restart policies".into();
    }
    *kept = Some((now, map.clone()));
    Ok(Done {
        text: map.to_string(),
        said,
    })
}

/// `ansible_nas_doc`: one of the lab's ansible-nas docs, or a folder of them, by the
/// allowlist.
fn ansible_nas_doc(state: &AppState, args: &serde_json::Value) -> Result<Done, Refused> {
    let path = match &args["path"] {
        serde_json::Value::Null => "",
        serde_json::Value::String(path) => path.as_str(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "the path is a string, e.g. roles/deluge/README.md".to_owned(),
            ));
        }
    };
    let now = state.clock.now().unix_millis();
    let shown = one_line(path);
    let shown = if shown.is_empty() {
        "the top"
    } else {
        shown.as_str()
    };
    match state.lab.docs.read(path, now) {
        Ok(text) => Ok(Done {
            text,
            said: format!("read {shown} of the lab's ansible-nas"),
        }),
        Err(why) => {
            let status = if why == ansible_nas::NOT_CONFIGURED {
                StatusCode::SERVICE_UNAVAILABLE
            } else if why == ansible_nas::ONLY_THE_DOCS {
                StatusCode::FORBIDDEN
            } else if why.starts_with("there is no") || why.contains("is not there") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_GATEWAY
            };
            Err((status, why))
        }
    }
}

/// Why Portainer could not be used, as the model reads it.
fn portainer(code: &str) -> Refused {
    match code {
        "unconfigured" | "configuration" => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Portainer is not set up in Endora's settings, so container logs and restarts are not available. Tell the owner if you need them.".to_owned(),
        ),
        "authentication" => (
            StatusCode::BAD_GATEWAY,
            "Portainer refused Endora's key".to_owned(),
        ),
        other => (
            StatusCode::BAD_GATEWAY,
            format!("Portainer did not answer ({})", other.replace('_', " ")),
        ),
    }
}

/// The container a call names, as Portainer lists it: its id and its name. An unknown name is
/// refused, and so is a container that is Endora's own.
fn container_named(
    state: &AppState,
    args: &serde_json::Value,
) -> Result<(String, String), Refused> {
    let asked = args["container"]
        .as_str()
        .unwrap_or("")
        .trim()
        .trim_start_matches('/');
    if asked.is_empty() || asked.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            "name the container, as homelab_status lists it".to_owned(),
        ));
    }
    let listed = state
        .console
        .read("team-containers", "")
        .map_err(portainer)?;
    let items = listed["items"].as_array().map_or(&[][..], Vec::as_slice);
    let found = items
        .iter()
        .find(|i| i["name"] == asked)
        .or_else(|| {
            items.iter().find(|i| {
                i["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(asked))
            })
        })
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("there is no container called {asked}: homelab_status lists them"),
            )
        })?;
    let name = found["name"].as_str().unwrap_or(asked).to_owned();
    let service = found["service"].as_str().unwrap_or("");
    if found["protected"] == true || contracts::endoras_own(&name, service) {
        return Err((
            StatusCode::FORBIDDEN,
            format!("{name} is Endora's own: no contract reads or restarts it"),
        ));
    }
    let id = found["id"].as_str().unwrap_or("");
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Portainer gave no usable id for {name}"),
        ));
    }
    Ok((id.to_owned(), name))
}

/// `container_logs`: the newest lines of a container's log, scrubbed of secrets.
fn container_logs(state: &AppState, args: &serde_json::Value) -> Result<Done, Refused> {
    let (id, name) = container_named(state, args)?;
    let read = state.console.read("logs", &id).map_err(portainer)?;
    let scrubbed = endora_capabilities::scrub_secrets(read["summary"].as_str().unwrap_or(""));
    let log = contracts::newest_of(&scrubbed);
    let text = if log.trim().is_empty() {
        format!("{name}'s log is empty.")
    } else {
        format!("The newest lines of {name}'s log, with secrets replaced by [redacted]:\n{log}")
    };
    Ok(Done {
        text,
        said: format!("read {name}'s log"),
    })
}

/// A reason as the owner reads it: one line, short.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>()
        .trim_end_matches('.')
        .to_owned()
}

/// `restart_container`: a safe fix, within the limits, told to the owner.
fn restart_container(
    state: &AppState,
    turn: &Turn,
    args: &serde_json::Value,
) -> Result<Done, Refused> {
    let why = one_line(args["why"].as_str().unwrap_or(""));
    if why.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "say why it needs a restart, in a sentence the owner reads".to_owned(),
        ));
    }
    let (id, name) = container_named(state, args)?;
    let now = state.clock.now().unix_millis();
    // What a restart cannot fix (already restarting, out of memory, crash-looping) is the
    // owner's, and costs none of the team's restarts.
    let inspected = state.console.read("team-inspect", &id).map_err(portainer)?;
    if contracts::restart_would_not_fix(&inspected, now) {
        return Err((
            StatusCode::CONFLICT,
            format!("{name} was not restarted: {}.", contracts::RESTART_WONT_FIX),
        ));
    }
    // Portainer's restart starts a stopped container, and one may be stopped on purpose.
    if contracts::restart_would_start(&inspected) {
        return Err((
            StatusCode::CONFLICT,
            format!("{name} was not restarted: {}.", contracts::RESTART_STOPPED),
        ));
    }
    let ledger = RestartLedger::new(state.store.db()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "the restart limits could not be read".to_owned(),
        )
    })?;
    match ledger.take(&name, now) {
        Ok(Ok(())) => {}
        Ok(Err(why_not)) => {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                format!("{name} was not restarted: {why_not}."),
            ));
        }
        Err(_) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "the restart limits could not be read".to_owned(),
            ));
        }
    }
    // The console's own container action: its allowlist, its protection, its journal and
    // its read-back, the same as the owner's button on the handheld.
    let request = json!({
        "request_id": format!("team-{}-{now}", turn.run),
        "kind": "container",
        "action": "restart",
        "target": id,
        "confirmed": true,
    });
    let member = who(turn);
    let failed = match state.console.act(&request) {
        Ok(outcome) if outcome["state"] == "complete" => {
            let now_is = outcome["observed"]["state"].as_str().unwrap_or("unknown");
            tell_owner(
                state,
                turn.run,
                &format!("{member} restarted {name}: {why}. It is now {now_is}."),
            );
            return Ok(Done {
                text: format!("Restarted {name}. It is now {now_is}. The owner has been told."),
                said: format!("restarted {name}: {why}. It is now {now_is}"),
            });
        }
        Ok(outcome) => outcome["error"]
            .as_str()
            .unwrap_or("it did not confirm")
            .replace('_', " "),
        Err(code) => portainer(code).1,
    };
    tell_owner(
        state,
        turn.run,
        &format!("{member} tried to restart {name}: {why}. It did not confirm: {failed}."),
    );
    Err((
        StatusCode::BAD_GATEWAY,
        format!(
            "the restart of {name} did not confirm ({failed}). Check homelab_status before anything else; the owner has been told."
        ),
    ))
}

/// Puts a notice on the owner's desk in HQ. One that cannot be put there is said in the log:
/// the audit log has the restart either way.
pub(super) fn tell_owner(state: &AppState, run: i64, text: &str) {
    let Some(hq) = state.hq.as_ref() else {
        return;
    };
    if let Err(e) = hq.post("/v1/team/notice", &json!({ "run": run, "text": text })) {
        eprintln!("contracts: could not tell the owner of run {run}'s restart: {e}");
    }
}

#[derive(Deserialize)]
pub(super) struct Switch {
    #[serde(default)]
    all: Option<bool>,
    #[serde(default)]
    restart_container: Option<bool>,
}

/// The owner's switches, as the HQ screen shows them.
pub(super) async fn switches(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(switches_said(&state))
}

/// The owner switches the team's contracts, or the restart alone, on or off. Recorded.
pub(super) async fn switch(
    State(state): State<AppState>,
    Json(req): Json<Switch>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    for (key, on, what) in [
        ("all", req.all, "the team's contracts"),
        (
            RESTART_CONTAINER,
            req.restart_container,
            "the team's restarts",
        ),
    ] {
        let Some(on) = on else {
            continue;
        };
        state
            .config
            .set_setting(SWITCHES, key, if on { "on" } else { "off" })
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "the switch could not be kept" })),
                )
            })?;
        record(
            &state,
            &format!("you switched {what} {}", if on { "on" } else { "off" }),
        );
    }
    Ok(Json(switches_said(&state)))
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{hq_as, hq_thinking, node};
    use super::*;
    use http_body_util::BodyExt as _;
    use std::sync::Mutex;
    use tower::ServiceExt as _;

    /// A home lab behind a fake console: Portainer's containers, one log, and the actions
    /// asked of it. `None` containers is Portainer not set up.
    struct Lab {
        containers: Option<serde_json::Value>,
        log: String,
        acted: Mutex<Vec<serde_json::Value>>,
    }

    impl endora_capabilities::application::HomeConsole for Lab {
        fn read(&self, view: &str, query: &str) -> Result<serde_json::Value, &'static str> {
            let Some(containers) = &self.containers else {
                return Err("unconfigured");
            };
            match view {
                "team-containers" => Ok(json!({ "items": containers })),
                // lidarr is crash-looping out of memory; the rest are well.
                "team-inspect" if query == "07".repeat(32) => Ok(json!({
                    "state": "restarting", "restart_count": 14, "oom_killed": true,
                    "exit_code": 137, "started_at": "2026-10-17T10:00:00Z" })),
                // owntone is stopped, on purpose.
                "team-inspect" if query == "08".repeat(32) => Ok(json!({
                    "state": "exited", "restart_count": 0, "oom_killed": false,
                    "exit_code": 0, "started_at": "2026-10-10T10:00:00Z" })),
                "team-inspect" => Ok(json!({
                    "state": "running", "restart_count": 0, "oom_killed": false,
                    "exit_code": 0, "started_at": "2026-10-15T10:00:00Z" })),
                "logs" => Ok(json!({ "summary": self.log, "items": [] })),
                _ => Err("unknown_view"),
            }
        }
        fn act(&self, request: &serde_json::Value) -> Result<serde_json::Value, &'static str> {
            self.acted.lock().unwrap().push(request.clone());
            Ok(
                json!({ "request_id": request["request_id"], "state": "complete",
                       "summary": "Observed container state: running",
                       "observed": { "state": "running" } }),
            )
        }
        fn operation(&self, _id: &str) -> Result<serde_json::Value, &'static str> {
            Err("unknown_operation")
        }
    }

    fn container(name: &str, n: u8, protected: bool) -> serde_json::Value {
        json!({ "id": format!("{n:02x}").repeat(32), "name": name, "state": "running",
                "status": "Up 2 days (unhealthy)", "service": "", "protected": protected })
    }

    fn lab() -> Arc<Lab> {
        Arc::new(Lab {
            containers: Some(json!([
                container("sonarr", 1, false),
                container("radarr", 2, false),
                container("jellyfin", 3, false),
                container("bazarr", 4, false),
                container("lidarr", 7, false),
                container("owntone", 8, false),
                container("endora-hq", 5, true),
                container("quirky_hopper", 6, true),
            ])),
            log: "10:00 starting\n10:01 DB_PASSWORD=hunter2 connecting\n10:02 Authorization: Bearer abc.def\n10:03 failed".to_owned(),
            acted: Mutex::new(Vec::new()),
        })
    }

    /// The node with HQ knowing one turn (token `good`, run 7, Linus's) and `lab` as its
    /// console. What HQ was sent is the second.
    fn with(lab: Arc<Lab>) -> (AppState, super::super::relay::tests::Seen) {
        let (hq_url, seen) = hq_as(40_000, "developer");
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab;
        (state, seen)
    }

    fn ask(
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: &str,
    ) -> axum::http::Request<axum::body::Body> {
        let mut req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        req.body(axum::body::Body::from(body.to_owned())).unwrap()
    }

    async fn answer(
        state: &AppState,
        req: axum::http::Request<axum::body::Body>,
    ) -> (StatusCode, serde_json::Value) {
        let res = team_relay(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
    }

    async fn use_contract(
        state: &AppState,
        name: &str,
        args: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        answer(
            state,
            ask(
                "POST",
                &format!("/team-relay/contracts/{name}"),
                Some("good"),
                &args.to_string(),
            ),
        )
        .await
    }

    fn audit(state: &AppState) -> Vec<String> {
        endora_application::AuditLog::recent(state.audit.as_ref(), 50)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect()
    }

    fn told(seen: &super::super::relay::tests::Seen) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/notice")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn a_turn_lists_the_contracts_and_a_stranger_gets_nothing() {
        let (state, _) = with(lab());
        for token in [None, Some("bad")] {
            let (status, _) = answer(&state, ask("GET", "/team-relay/contracts", token, "")).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{token:?}");
            let (status, _) = answer(
                &state,
                ask("POST", "/team-relay/contracts/homelab_status", token, "{}"),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let (status, listed) = answer(
            &state,
            ask("GET", "/team-relay/contracts", Some("good"), ""),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(
            names[..3],
            ["homelab_status", "container_logs", "restart_container"],
            "the contracts first, then what the bridge offers (ADR 0108)"
        );
        assert!(listed["tools"][2]["inputSchema"]["required"].is_array());
        assert_eq!(listed["tools"][2]["kind"], "act");
        let (status, _) = use_contract(&state, "shell", json!({})).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "nothing but the contracts");
        let (status, _) = answer(
            &state,
            ask(
                "POST",
                "/team-relay/contracts/homelab_status",
                Some("good"),
                "[1, 2]",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Every call is in the audit log, refused ones too (ADR 0106).
        let audit = audit(&state);
        for line in [
            "Linus (run 7) was refused shell: there is no such contract",
            "Linus (run 7) was refused homelab_status: the arguments must be one JSON object",
        ] {
            assert!(audit.iter().any(|a| a.ends_with(line)), "{line}: {audit:?}");
        }
    }

    #[tokio::test]
    async fn the_home_lab_is_read_compact_and_said_stale_when_it_is() {
        let (state, _) = with(lab());
        let now = state.clock.now().unix_millis();
        *state.homelab.lock().unwrap() = json!({ "observed_at_ms": now - 120_000, "services": [
            { "name": "docker", "state": "degraded", "total": 1, "observed_at_ms": now - 120_000, "items": [
                { "name": "sonarr", "state": "running", "health": "unhealthy", "id": "ab".repeat(32),
                  "exit": null, "status": "Up 2 days (unhealthy)" }] },
            { "name": "radarr", "state": "unconfigured", "error": "unconfigured", "items": [] },
        ] });
        let (status, said) = use_contract(&state, "homelab_status", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        let seen: serde_json::Value = serde_json::from_str(said["text"].as_str().unwrap()).unwrap();
        assert_eq!(seen["stale"], true);
        assert_eq!(seen["services"][0]["state"], "stale");
        assert_eq!(
            seen["services"][0]["items"][0],
            json!({ "name": "sonarr", "state": "running", "status": "Up 2 days (unhealthy)", "health": "unhealthy" })
        );
        assert!(!said.to_string().contains("abab"), "no ids");
        assert_eq!(seen["services"][1]["state"], "unconfigured");
        assert!(
            audit(&state)
                .iter()
                .any(|a| a.contains("Linus (run 7) read the home lab's state"))
        );
    }

    /// The node with HQ knowing one turn of a member in `role`, thinking with Claude.
    fn on_claude(role: &'static str) -> AppState {
        let (hq_url, _) = hq_thinking(40_000, role, true);
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab();
        state
    }

    async fn listed(state: &AppState) -> Vec<String> {
        let (_, listed) =
            answer(state, ask("GET", "/team-relay/contracts", Some("good"), "")).await;
        listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_owned))
            .collect()
    }

    #[tokio::test]
    async fn the_cloud_ceo_reads_the_home_labs_state_and_neither_its_logs_nor_its_restarts() {
        let state = on_claude("ceo");
        let offered = listed(&state).await;
        assert!(
            offered.contains(&"homelab_status".to_owned()),
            "{offered:?}"
        );
        for refused in [
            "container_logs",
            "restart_container",
            "sonarr_queue",
            "radarr_queue",
        ] {
            assert!(
                !offered.contains(&refused.to_owned()),
                "{refused}: {offered:?}"
            );
        }
        let (status, said) =
            use_contract(&state, "container_logs", json!({ "container": "sonarr" })).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains("Confidential"),
            "{said}"
        );
        let (status, said) = use_contract(
            &state,
            "restart_container",
            json!({ "container": "sonarr", "why": "it is unhealthy" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("hand it to the lead"),
            "{said}"
        );
    }

    #[tokio::test]
    async fn a_cloud_turn_reads_logs_only_when_it_is_the_leads() {
        // The owner's decision of 2026-10-17 (ADR 0109): the lead thinking with Claude may read
        // a scrubbed log; no other member on a cloud model may.
        for role in ["reviewer", "developer", "researcher"] {
            let state = on_claude(role);
            assert!(
                !listed(&state).await.contains(&"container_logs".to_owned()),
                "a cloud {role} is not offered logs"
            );
            let (status, said) =
                use_contract(&state, "container_logs", json!({ "container": "sonarr" })).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{role}: {said}");
            assert!(
                said["error"].as_str().unwrap().contains("Confidential"),
                "{said}"
            );
            assert!(
                audit(&state)
                    .iter()
                    .any(|a| a.contains("Linus (run 7) was refused container_logs")),
                "{:?}",
                audit(&state)
            );
        }
        let state = on_claude("lead");
        let offered = listed(&state).await;
        assert!(
            offered.contains(&"container_logs".to_owned()),
            "{offered:?}"
        );
        assert!(offered.contains(&"homelab_status".to_owned()));
        let (status, said) =
            use_contract(&state, "container_logs", json!({ "container": "sonarr" })).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        assert!(
            !text.contains("hunter2") && text.contains("[redacted]"),
            "scrubbed: {text}"
        );
    }

    #[tokio::test]
    async fn a_log_is_read_scrubbed_and_never_endoras_own() {
        let (state, _) = with(lab());
        let (status, said) =
            use_contract(&state, "container_logs", json!({ "container": "sonarr" })).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        assert!(text.contains("10:03 failed"), "{text}");
        assert!(
            !text.contains("hunter2") && !text.contains("abc.def"),
            "{text}"
        );
        assert!(text.contains("[redacted]"));
        for (name, status) in [
            ("endora-hq", StatusCode::FORBIDDEN),
            ("quirky_hopper", StatusCode::FORBIDDEN),
            ("nothing-like-it", StatusCode::NOT_FOUND),
            ("", StatusCode::BAD_REQUEST),
        ] {
            let (got, said) =
                use_contract(&state, "container_logs", json!({ "container": name })).await;
            assert_eq!(got, status, "{name}: {said}");
            assert!(said["error"].is_string());
        }
        let (state, _) = with(Arc::new(Lab {
            containers: None,
            log: String::new(),
            acted: Mutex::new(Vec::new()),
        }));
        let (status, said) =
            use_contract(&state, "container_logs", json!({ "container": "sonarr" })).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("Portainer is not set up"),
            "{said}"
        );
    }

    #[tokio::test]
    async fn a_restart_runs_within_its_limits_and_lands_on_the_owners_desk() {
        let lab = lab();
        let (state, to_hq) = with(lab.clone());
        let restart = |name: &str| json!({ "container": name, "why": "it is unhealthy" });
        let (status, said) = use_contract(&state, "restart_container", restart("sonarr")).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert!(said["text"].as_str().unwrap().contains("It is now running"));
        assert_eq!(lab.acted.lock().unwrap()[0]["target"], "01".repeat(32));
        assert_eq!(lab.acted.lock().unwrap()[0]["action"], "restart");
        let notices = told(&to_hq);
        assert_eq!(
            notices,
            [
                json!({ "run": 7, "text": "Linus restarted sonarr: it is unhealthy. It is now running." })
            ]
        );
        assert!(
            audit(&state)
                .iter()
                .any(|a| a.contains("Linus (run 7) restarted sonarr"))
        );

        // Once an hour a container, three an hour in all, never Endora's own, always a reason.
        let (status, _) = use_contract(&state, "restart_container", restart("sonarr")).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        let (status, _) = use_contract(&state, "restart_container", restart("endora-hq")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // Crash-looping out of memory: a restart would not fix it, and costs no restart.
        let (status, said) = use_contract(&state, "restart_container", restart("lidarr")).await;
        assert_eq!(status, StatusCode::CONFLICT, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains(
                "restarting will not fix this; put it on the owner's desk with ask_owner (e.g. raise its memory)"
            ),
            "{said}"
        );
        // Stopped, perhaps on purpose: a restart would start it, so it is refused, and costs
        // no restart.
        let (status, said) = use_contract(&state, "restart_container", restart("owntone")).await;
        assert_eq!(status, StatusCode::CONFLICT, "{said}");
        assert_eq!(
            said["error"],
            "owntone was not restarted: it is stopped, perhaps on purpose: restart only fixes a running container; ask the owner with ask_owner if it should run."
        );
        assert!(
            lab.acted
                .lock()
                .unwrap()
                .iter()
                .all(|a| a["target"] != "08".repeat(32)),
            "nothing was asked of Portainer for it"
        );
        let (status, _) = use_contract(
            &state,
            "restart_container",
            json!({ "container": "radarr", "why": " " }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        for name in ["radarr", "jellyfin"] {
            let (status, _) = use_contract(&state, "restart_container", restart(name)).await;
            assert_eq!(status, StatusCode::OK, "{name}");
        }
        let (status, said) = use_contract(&state, "restart_container", restart("bazarr")).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{said}");
        assert_eq!(
            lab.acted.lock().unwrap().len(),
            3,
            "three restarts, no more"
        );
        assert_eq!(told(&to_hq).len(), 3);
        assert!(
            audit(&state)
                .iter()
                .any(|a| a.contains("was refused restart_container"))
        );
    }

    /// GitHub, faked for the lab's ansible-nas: the role list, and nothing else there.
    struct Hub;
    impl ansible_nas::Fetch for Hub {
        fn get(&self, url: &str) -> Result<(u16, String), &'static str> {
            if url.contains("/contents/roles?") {
                let roles: Vec<serde_json::Value> = ["deluge", "gluetun", "nzbget", "prowlarr"]
                    .iter()
                    .map(|r| json!({ "name": r, "path": format!("roles/{r}"), "type": "dir" }))
                    .collect();
                return Ok((200, serde_json::Value::Array(roles).to_string()));
            }
            Ok((404, "{}".to_owned()))
        }
    }

    /// Docker, as the observer's origin answers it: gluetun with three in its network,
    /// Endora's node, and a container nobody knows. Each inspection carries what the map must
    /// never pass on.
    fn docker() -> (String, super::super::relay::tests::Seen) {
        let id = |n: u8| format!("{n:02x}").repeat(32);
        let lab: Vec<(u8, &str, String)> = vec![
            (1, "gluetun", "bridge".to_owned()),
            (2, "deluge", format!("container:{}", id(1))),
            (3, "nzbget", format!("container:{}", id(1))),
            (4, "prowlarr", format!("container:{}", id(1))),
            (5, "endora-node", "endora_default".to_owned()),
            (6, "mystery", "bridge".to_owned()),
        ];
        super::super::relay::tests::fake(Arc::new(move |path, _| {
            if path == "/containers/json?all=true" {
                let rows: Vec<serde_json::Value> = lab
                    .iter()
                    .map(|(n, name, _)| json!({ "Id": id(*n), "Names": [format!("/{name}")], "State": "running", "Status": "Up 1 minute" }))
                    .collect();
                return (
                    200,
                    "application/json",
                    serde_json::Value::Array(rows).to_string(),
                );
            }
            let Some((n, name, network)) = lab
                .iter()
                .find(|(_, name, _)| path == format!("/containers/{name}/json"))
            else {
                return (404, "application/json", "{}".to_owned());
            };
            let seen = json!({
                "Id": id(*n), "Name": format!("/{name}"),
                "Config": { "Image": format!("example/{name}:latest@sha256:{}", id(9)),
                            "Env": ["WIREGUARD_PRIVATE_KEY=fictional-secret"] },
                "State": { "Status": "running" },
                "HostConfig": { "NetworkMode": network, "Binds": ["/srv/fictional:/config"],
                                "RestartPolicy": { "Name": "unless-stopped" } },
                "Mounts": [{ "Source": "/srv/fictional" }],
            });
            (200, "application/json", seen.to_string())
        }))
    }

    #[tokio::test]
    async fn the_map_says_what_runs_in_gluetuns_network_and_who_manages_each_container() {
        let (mut state, _) = with(lab());
        let (origin, asked) = docker();
        state
            .config
            .set_setting("homelab-docker", "origin", &origin)
            .unwrap();
        state.lab = Arc::new(LabKnowledge::new(
            Docs::new(
                ansible_nas::Fork::of("owner/ansible-nas", ""),
                Box::new(Hub),
            ),
            vec![(
                "mystery".to_owned(),
                "retired: kept for its settings".to_owned(),
            )],
        ));
        let (status, said) = use_contract(&state, "homelab_map", json!({})).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        for never in [
            "fictional-secret",
            "PRIVATE_KEY",
            "/srv",
            "Binds",
            "Mounts",
            "sha256",
        ] {
            assert!(!text.contains(never), "{never} in {text}");
        }
        let map: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            map["wiring"],
            json!(["gluetun → deluge, nzbget, prowlarr (run in its network)"])
        );
        let of = |name: &str| {
            map["containers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == name)
                .cloned()
                .unwrap()
        };
        assert_eq!(
            of("deluge"),
            json!({ "name": "deluge", "state": "running", "managed_by": "ansible-nas role deluge",
                    "image": "example/deluge:latest", "restart": "unless-stopped",
                    "depends_on": ["gluetun"], "in_network_of": "gluetun" })
        );
        assert_eq!(
            of("gluetun")["needed_by"],
            json!(["deluge", "nzbget", "prowlarr"])
        );
        assert_eq!(of("endora-node")["managed_by"], "Endora");
        assert_eq!(of("mystery")["managed_by"], "unknown");
        assert_eq!(of("mystery")["note"], "retired: kept for its settings");
        assert_eq!(map["ansible_nas"], "4 roles in the lab's ansible-nas");
        assert!(
            audit(&state)
                .iter()
                .any(|a| a.ends_with("Linus (run 7) read how the home lab is built"))
        );
        // Kept a minute: asked again, Docker is not.
        let before = asked.lock().unwrap().len();
        assert_eq!(before, 7, "the list and six inspections");
        let (status, again) = use_contract(&state, "homelab_map", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(again, said);
        assert_eq!(asked.lock().unwrap().len(), before);
    }

    #[tokio::test]
    async fn without_the_observer_or_the_ansible_nas_fork_the_map_and_the_docs_say_so() {
        let (mut state, _) = with(lab());
        state.lab = Arc::new(LabKnowledge::new(
            Docs::new(None, Box::new(Hub)),
            Vec::new(),
        ));
        let (status, said) = use_contract(&state, "homelab_map", json!({})).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{said}");
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("Docker observer is not set up")
        );
        let (status, said) =
            use_contract(&state, "ansible_nas_doc", json!({ "path": "roles" })).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{said}");
        assert_eq!(said["error"], ansible_nas::NOT_CONFIGURED);
        // The fork set up, the map's managers come from it; with none, they are unknown.
        let (origin, _) = docker();
        state
            .config
            .set_setting("homelab-docker", "origin", &origin)
            .unwrap();
        let (_, said) = use_contract(&state, "homelab_map", json!({})).await;
        let map: serde_json::Value = serde_json::from_str(said["text"].as_str().unwrap()).unwrap();
        assert!(
            map["ansible_nas"]
                .as_str()
                .unwrap()
                .starts_with("not configured")
        );
        assert_eq!(
            map["containers"][0]["managed_by"], "unknown",
            "deluge, unknown"
        );
        state.lab = Arc::new(LabKnowledge::new(
            Docs::new(
                ansible_nas::Fork::of("owner/ansible-nas", ""),
                Box::new(Hub),
            ),
            Vec::new(),
        ));
        for path in [
            "group_vars/all.yml",
            "inventories/my-ansible-nas/inventory",
            "../nas.yml",
        ] {
            let (status, said) =
                use_contract(&state, "ansible_nas_doc", json!({ "path": path })).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {said}");
        }
        let (status, said) =
            use_contract(&state, "ansible_nas_doc", json!({ "path": "roles" })).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert!(
            said["text"]
                .as_str()
                .unwrap()
                .contains("deluge, gluetun, nzbget, prowlarr")
        );
        let (status, _) = use_contract(&state, "ansible_nas_doc", json!({ "path": 7 })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Every role on any brain is offered both (names and wiring, Internal).
        for role in ["ceo", "lead", "developer", "reviewer", "researcher"] {
            let offered = listed(&on_claude(role)).await;
            for name in ["homelab_map", "ansible_nas_doc"] {
                assert!(
                    offered.contains(&name.to_owned()),
                    "{name} to a cloud {role}"
                );
            }
        }
    }

    #[tokio::test]
    async fn the_owner_switches_the_restart_or_every_contract_off() {
        let lab = lab();
        let (state, _) = with(lab.clone());
        let names = |listed: &serde_json::Value| -> Vec<String> {
            listed["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_owned())
                .filter(|n| contracts::all().iter().any(|c| c.name == n))
                .collect()
        };
        let _ = switch(
            State(state.clone()),
            Json(Switch {
                all: None,
                restart_container: Some(false),
            }),
        )
        .await
        .unwrap();
        let (_, listed) = answer(
            &state,
            ask("GET", "/team-relay/contracts", Some("good"), ""),
        )
        .await;
        assert_eq!(
            names(&listed),
            [
                "homelab_status",
                "container_logs",
                "homelab_map",
                "ansible_nas_doc"
            ]
        );
        let (status, said) = use_contract(
            &state,
            "restart_container",
            json!({ "container": "sonarr", "why": "unwell" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("switched restart_container off")
        );
        assert!(lab.acted.lock().unwrap().is_empty());
        let shown = switch(
            State(state.clone()),
            Json(Switch {
                all: Some(false),
                restart_container: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(shown.0, json!({ "all": false, "restart_container": false }));
        let (_, listed) = answer(
            &state,
            ask("GET", "/team-relay/contracts", Some("good"), ""),
        )
        .await;
        assert!(
            listed["tools"].as_array().unwrap().is_empty(),
            "every contract off is the bridge off too: {listed}"
        );
        let (status, _) = use_contract(&state, "homelab_status", json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            audit(&state)
                .iter()
                .any(|a| a.contains("you switched the team's restarts off"))
        );
    }
}
