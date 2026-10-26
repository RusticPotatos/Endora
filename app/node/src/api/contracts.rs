//! The team's contracts on the node (ADR 0106): what Endora offers HQ's workers on the home
//! lab, served on the team's listener beside the model relays, and opened by a turn's own
//! token, checked with HQ as the relays check it. A worker proposes a call; the node decides
//! it by fixed rules (the owner's switches, Endora's own containers, the restart limits) and
//! does it through its own console adapter. No key and no raw access leave the node.
//!
//! Every call is written to Endora's audit log with the member and the run. A restart lands
//! on the owner's desk in HQ as a notice. Every call a turn makes, done or refused, is also told
//! to HQ as a receipt, with the turn's own token (ADR 0114): the node writes it, never the
//! model, and the model reads its id (`receipt r12`) to cite as evidence.

use super::relay::{Turn, token_of, turn_of};
use super::*;
use axum::body::Bytes;
use axum::http::HeaderMap;
use endora_capabilities::ansible_nas::{self, Docs};
use endora_capabilities::application::HomelabObserver as _;
use endora_capabilities::contracts::{
    self, ANSIBLE_NAS_DOC, CONTAINER_LOGS, Contract, HOMELAB_MAP, HOMELAB_STATUS, REPO_CHECKOUT,
    REPO_SUBMIT, RESTART_CONTAINER, RestartLedger,
};
use endora_capabilities::lab_map;
use endora_capabilities::repo::{self, Mirror};
use std::path::PathBuf;

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
    // Changing the lab is the engineering wing's (ADR 0120): a lead outside it reads a branch
    // and submits none, so nothing is kept for it.
    if contract.name == REPO_SUBMIT && turn.wing == Some(false) {
        return false;
    }
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
    let only = turn.only.clone();
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
    // A turn HQ narrows is offered what it may use and nothing else (ADR 0115, amended).
    if let Some(only) = only {
        tools.retain(|t| {
            t["name"]
                .as_str()
                .is_some_and(|n| only.iter().any(|o| o == n))
        });
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

/// What a call came to, for its receipt.
pub(super) struct Came {
    /// The tool's name.
    pub(super) tool: String,
    /// Its arguments, in plain words.
    pub(super) args: String,
    /// What the turn read back: the result, or why it was refused.
    pub(super) said: String,
    /// Whether it did what was asked.
    pub(super) ok: bool,
    /// Whether the tool acts.
    pub(super) act: bool,
    /// Whether its result is Confidential.
    pub(super) confidential: bool,
}

/// Tells HQ what a call came to, as a receipt the turn can cite (ADR 0114), and returns what
/// the model reads after the result: ` (receipt r12)`, or nothing when HQ kept none. A receipt
/// HQ cannot keep is said in the log; the call's result stands, and the audit log has it.
pub(super) async fn receipt(state: &AppState, token: &str, came: Came) -> String {
    let Some(hq) = state.hq.clone() else {
        return String::new();
    };
    let body = json!({
        "token": token, "tool": came.tool, "args": came.args, "ok": came.ok, "act": came.act,
        "confidential": came.confidential,
        "excerpt": came.said.chars().take(1_000).collect::<String>(),
    });
    let kept = tokio::task::spawn_blocking(move || hq.post("/v1/team/receipt", &body)).await;
    match kept {
        Ok(Ok(answer)) => answer["receipt"]
            .as_i64()
            .map_or_else(String::new, |id| format!(" (receipt r{id})")),
        Ok(Err(e)) => {
            eprintln!("contracts: HQ kept no receipt of a call: {e}");
            String::new()
        }
        Err(_) => String::new(),
    }
}

/// A call's arguments in plain words, for its receipt: its keys and values, not its reason.
fn args_said(args: &serde_json::Value) -> String {
    let mut given = args.clone();
    if let Some(object) = given.as_object_mut() {
        object.remove("why");
    }
    endora_capabilities::bridge::plain_words(&given)
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `POST /team-relay/contracts/{name}`: one call of a contract, its arguments the JSON body,
/// with its receipt's id after what the model reads.
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
    let token = token_of(&headers);
    let args = if body.iter().all(u8::is_ascii_whitespace) {
        json!({})
    } else {
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
    };
    let said = args_said(&args);
    let (response, came) = called(state.clone(), turn, name, args).await;
    let Some(came) = came else {
        return response;
    };
    let receipt = receipt(&state, &token, Came { args: said, ..came }).await;
    if receipt.is_empty() {
        return response;
    }
    with_receipt(response, &receipt).await
}

/// A contract's answer with the receipt's id after its text, or its error.
async fn with_receipt(response: Response, receipt: &str) -> Response {
    let status = response.status();
    let Ok(bytes) = axum::body::to_bytes(response.into_body(), 1 << 24).await else {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "", "the contract failed");
    };
    let mut answer: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    for key in ["text", "error"] {
        if let Some(text) = answer[key].as_str() {
            answer[key] = format!("{text}{receipt}").into();
        }
    }
    (status, Json(answer)).into_response()
}

/// One call of a contract or a bridged tool, decided by the node's rules: the answer, and
/// what it came to for its receipt (`None` for a call that never reached a tool's rules: the
/// arguments unreadable, a name that is no tool, or an approval put on the desk).
async fn called(
    state: AppState,
    turn: Turn,
    name: String,
    args: serde_json::Value,
) -> (Response, Option<Came>) {
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
        let refused = refuse(
            StatusCode::BAD_REQUEST,
            "",
            "the arguments must be one JSON object",
        );
        return (refused, None);
    }
    // HQ's narrowing comes first, for a contract and a bridged tool alike: the check after a
    // deploy reads the lab's health, logs and metrics (ADR 0115, amended), a project that lists
    // its tools gives its work those (the Holodeck plan's phase 4), and nothing a turn is not
    // given runs. The refusal is kept as a receipt that failed.
    if !turn.may_use(&name) {
        let why = format!(
            "{name} is not among this turn's tools: HQ gives it {} of the node's and nothing \
             else (a post-deploy check, or a project that lists its tools)",
            match turn.only.as_deref().unwrap_or_default() {
                [] => "none".to_owned(),
                given => given.join(", "),
            }
        );
        refused_for(&why);
        let came = Came {
            tool: one_line(&name),
            args: String::new(),
            said: why.clone(),
            ok: false,
            act: false,
            confidential: false,
        };
        return (refuse(StatusCode::FORBIDDEN, "", &why), Some(came));
    }
    if !contracts::all().iter().any(|c| c.name == name) {
        return bridged(state, turn, name, args).await;
    }
    let found = contracts::all().into_iter().find(|c| c.name == name);
    let came = |said: &str, ok: bool| {
        found.as_ref().map(|c| Came {
            tool: c.name.to_owned(),
            args: String::new(),
            said: said.to_owned(),
            ok,
            act: c.kind != contracts::Kind::Read,
            confidential: c.tier == "confidential",
        })
    };
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
        let why = format!("the owner has switched {name} off");
        return (refuse(StatusCode::FORBIDDEN, "", &why), came(&why, false));
    };
    if !may_see(&contract, &turn) {
        let why = if name == REPO_SUBMIT && turn.wing == Some(false) && turn.role != "reviewer" {
            "changing the lab is the engineering wing's: its lead and developers submit the \
             branches. Ask for the change with propose_project, with its criteria and evidence; \
             the CEO decides and the wing builds it. Read a branch with repo_checkout"
                .to_owned()
        } else if name == REPO_SUBMIT && turn.role == "reviewer" {
            "a tester reads a branch by checking it out (repo_checkout) and submits none: only a \
             lead or a developer submits one"
                .to_owned()
        } else if name == REPO_CHECKOUT || name == REPO_SUBMIT {
            "the wing's git is a lead's or a developer's, and a tester may check a branch out: \
             read the branch's document (read_document) and the check's receipt instead"
                .to_owned()
        } else if contract.may_be_seen_by(&turn.role, turn.cloud) {
            "the CEO reads the house and takes no step on it itself: direct the lead of the \
             task's area with direct, who decides; or hand it to the lead"
                .to_owned()
        } else {
            super::bridge::confidential(&name)
        };
        refused_for(&why);
        return (refuse(StatusCode::FORBIDDEN, "", &why), came(&why, false));
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
        REPO_CHECKOUT => repo_checkout(&state2, &turn, &args),
        REPO_SUBMIT => repo_submit(&state2, &turn, &args),
        _ => Err((
            StatusCode::NOT_FOUND,
            format!("no contract is called {name2}"),
        )),
    })
    .await;
    match done {
        Ok(Ok(done)) => {
            record(&state, &format!("{member} (run {run}) {}", done.said));
            // A checkout's text is a bundle: its receipt says what was checked out instead.
            let excerpt = if name == REPO_CHECKOUT {
                &done.said
            } else {
                &done.text
            };
            let came = came(excerpt, true);
            (Json(json!({ "text": done.text })).into_response(), came)
        }
        Ok(Err((status, why))) => {
            record(
                &state,
                &format!("{member} (run {run}) was refused {name}: {why}"),
            );
            (refuse(status, "", &why), came(&why, false))
        }
        Err(_) => (
            refuse(StatusCode::INTERNAL_SERVER_ERROR, "", "the contract failed"),
            None,
        ),
    }
}

/// A call of a bridged tool (ADR 0108): offered to this turn now, or refused as no contract.
async fn bridged(
    state: AppState,
    turn: Turn,
    name: String,
    args: serde_json::Value,
) -> (Response, Option<Came>) {
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
            return (
                None,
                Err((
                    StatusCode::NOT_FOUND,
                    format!("no contract is called {called_name}"),
                )),
            );
        };
        let kind = Some((
            tool.kind == endora_capabilities::bridge::Kind::Act,
            tool.confidential(),
        ));
        (kind, super::bridge::call(&worked, &turn, &tool, &args))
    })
    .await;
    let (kind, done) = match done {
        Ok((kind, done)) => (kind, Ok(done)),
        Err(e) => (None, Err(e)),
    };
    let came = |said: &str, ok: bool| {
        kind.map(|(act, confidential)| Came {
            tool: name.clone(),
            args: String::new(),
            said: said.to_owned(),
            ok,
            act,
            confidential,
        })
    };
    match done {
        Ok(Ok(called)) => {
            record(&state, &format!("{member} (run {run}) {}", called.said));
            let mut answer = json!({ "text": called.text });
            if called.asked_owner {
                answer["asked_owner"] = true.into();
                // An approval put on the desk ran nothing: there is no reading to keep.
                return (Json(answer).into_response(), None);
            }
            let came = came(&called.text, true);
            (Json(answer).into_response(), came)
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
            let came = came(&said, false);
            (refuse(status, "", &said), came)
        }
        Err(_) => (
            refuse(StatusCode::INTERNAL_SERVER_ERROR, "", "the contract failed"),
            None,
        ),
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
/// the owner's retired list, and the last map, kept [`lab_map::MAP_FOR_MS`]; and the wing's
/// git (ADR 0115): the node's mirror of the fork, where it pushes to, and the deploy key.
pub(crate) struct LabKnowledge {
    pub(crate) docs: Docs,
    pub(crate) retired: Vec<(String, String)>,
    map: std::sync::Mutex<Option<(i64, serde_json::Value)>>,
    /// Where the mirror and the submissions are kept: the node's data folder.
    repo_dir: PathBuf,
    /// The mirror, once the fork's default branch is known.
    repo: std::sync::Mutex<Option<Arc<Mirror>>>,
    /// Where an approved branch is pushed: the fork over SSH, unless a test says a path.
    push_remote: Option<String>,
    /// The deploy key (`ENDORA_GIT_PUSH_KEY_FILE`), if one is set up.
    push_key: Option<PathBuf>,
}

impl LabKnowledge {
    /// The docs and the retired list as given; no mirror until a fork and a data folder are.
    pub(crate) fn new(docs: Docs, retired: Vec<(String, String)>) -> Self {
        Self {
            docs,
            retired,
            map: std::sync::Mutex::new(None),
            repo_dir: PathBuf::from("."),
            repo: std::sync::Mutex::new(None),
            push_remote: None,
            push_key: None,
        }
    }

    /// No fork, nothing retired and no deploy key: what a node has until `main` reads its
    /// environment, so a test never sees the developer's fork.
    pub(crate) fn unconfigured() -> Self {
        Self::new(Docs::new(None, Box::new(ansible_nas::Web)), Vec::new())
    }

    /// From the node's environment: `ENDORA_ANSIBLE_NAS_REPO`, `ENDORA_ANSIBLE_NAS_REF`,
    /// `ENDORA_LAB_RETIRED`, the data folder beside `ENDORA_DB`, and
    /// `ENDORA_GIT_PUSH_KEY_FILE`.
    pub(crate) fn from_env() -> Self {
        let retired = std::env::var("ENDORA_LAB_RETIRED").unwrap_or_default();
        let mut lab = Self::new(Docs::from_env(), lab_map::retired_of(&retired));
        lab.repo_dir = std::env::var("ENDORA_DB")
            .ok()
            .and_then(|db| PathBuf::from(db).parent().map(PathBuf::from))
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        lab.push_key = std::env::var("ENDORA_GIT_PUSH_KEY_FILE")
            .ok()
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        lab
    }

    /// With a mirror of the repository at `url` (worked from `branch`) kept under
    /// `dir`, pushed to `push_remote`: a test's local bare repository.
    #[cfg(test)]
    pub(crate) fn with_repo(
        mut self,
        dir: PathBuf,
        url: &str,
        branch: &str,
        push_remote: &str,
    ) -> Self {
        self.repo = std::sync::Mutex::new(Some(Arc::new(Mirror::new(&dir, url, branch))));
        self.repo_dir = dir;
        self.push_remote = Some(push_remote.to_owned());
        self
    }

    /// With a deploy key at `key`.
    #[cfg(test)]
    pub(crate) fn with_push_key(mut self, key: PathBuf) -> Self {
        self.push_key = Some(key);
        self
    }

    /// Whether the node can push a branch to the fork: a deploy key is set up.
    pub(crate) fn can_push(&self) -> bool {
        self.push_key.as_ref().is_some_and(|k| k.exists())
    }

    /// The node's mirror of the fork, made the first time it is asked for, worked from the base
    /// branch: `ENDORA_ANSIBLE_NAS_REF` when set (the fork's `develop`), else the fork's default
    /// branch, asked of GitHub then; `None` when no fork is configured.
    pub(crate) fn mirror(&self, now: i64) -> Option<Arc<Mirror>> {
        let mut kept = self
            .repo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(mirror) = kept.as_ref() {
            return Some(mirror.clone());
        }
        let fork = self.docs.fork()?;
        let branch = self.docs.branch(now)?;
        let mirror = Arc::new(Mirror::new(&self.repo_dir, &fork.https_url(), &branch));
        *kept = Some(mirror.clone());
        Some(mirror)
    }

    /// Where an approved branch is pushed: the fork over SSH.
    fn push_remote(&self) -> Option<String> {
        self.push_remote
            .clone()
            .or_else(|| self.docs.fork().map(ansible_nas::Fork::ssh_url))
    }
}

/// What the team is told when no fork is configured for the wing.
const NO_FORK: &str = "the lab's ansible-nas fork is not configured on the node (ENDORA_ANSIBLE_NAS_REPO): there is nothing to check out";

/// `repo_checkout` (ADR 0115): a bundle of the fork's default branch, or of one of the team's
/// `hq/` branches, as base64 in the text the worker reads (the worker unpacks it; the model
/// never sees the bundle). A team branch the fork has is handed out from the fork. One it
/// does not have is the team's own submission, kept on the node until the owner's push: HQ is
/// asked whether this turn may have it, and the kept branch is handed out at the head HQ
/// recorded (see [`kept_checkout`]).
fn repo_checkout(state: &AppState, turn: &Turn, args: &serde_json::Value) -> Result<Done, Refused> {
    let now = state.clock.now().unix_millis();
    let Some(mirror) = state.lab.mirror(now) else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, NO_FORK.to_owned()));
    };
    let branch = args["branch"]
        .as_str()
        .map(str::trim)
        .filter(|b| !b.is_empty());
    if let Some(named) = branch.and_then(|b| repo::hq_branch(b).ok()) {
        // Best effort: the fork may be unreachable while the node still keeps the work.
        let _ = mirror.refresh(now);
        if mirror.head_of(named).is_err() {
            return kept_checkout(state, &mirror, turn, named, now);
        }
    }
    let bundle = mirror
        .bundle(branch, now)
        .map_err(|why| (StatusCode::BAD_GATEWAY, why))?;
    let said = format!(
        "checked out {} at {} ({} KB)",
        bundle.branch,
        &bundle.head[..12.min(bundle.head.len())],
        bundle.bytes.len() >> 10
    );
    let text = json!({
        "branch": bundle.branch, "head": bundle.head, "base": mirror.base(),
        "bundle": repo::to_base64(&bundle.bytes),
    })
    .to_string();
    Ok(Done { text, said })
}

/// HQ's no to a kept checkout of `branch`, as the turn is told it. HQ answers 404 both for a
/// token that opens no running turn and for a branch it never recorded, and says which: a turn
/// that ended is told so (401, which ends it), not that the fork has no such branch. Whose the
/// branch is, HQ says, and that is passed on.
fn checkout_refused(e: endora_infrastructure::HqError, branch: &str, base: &str) -> Refused {
    use endora_infrastructure::HqError;
    match e {
        HqError::Answered {
            status: 404,
            message,
        } if message.contains("running turn") => (
            StatusCode::UNAUTHORIZED,
            "no running turn has that token: this turn is over".to_owned(),
        ),
        HqError::Answered { status: 404, .. } => (
            StatusCode::NOT_FOUND,
            format!(
                "the fork has no branch {branch}, and the team has not submitted one of that name: check out {base} to start from it"
            ),
        ),
        HqError::Answered { message, .. } => (StatusCode::FORBIDDEN, message),
        e @ HqError::Unreachable(_) => (
            StatusCode::BAD_GATEWAY,
            format!("HQ could not say whose {branch} is: {e}"),
        ),
    }
}

/// The team's own kept submission of `branch`, for the turn that asked (ADR 0115, amended): HQ
/// is asked, naming the turn by its token, whether the branch is this turn's (its work, or its
/// member's) and which head it recorded; the node hands out that head and no other, with the
/// base behind it. HQ's refusal is the turn's. A branch HQ never recorded is not there; a head
/// the node no longer keeps (the branch was submitted again) is refused, not swapped.
fn kept_checkout(
    state: &AppState,
    mirror: &Mirror,
    turn: &Turn,
    branch: &str,
    now: i64,
) -> Result<Done, Refused> {
    let Some(hq) = state.hq.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "HQ is not set up on this node".to_owned(),
        ));
    };
    let kept = hq
        .post(
            "/v1/team/branch/checkout",
            &json!({ "token": turn.token, "branch": branch }),
        )
        .map_err(|e| checkout_refused(e, branch, mirror.base()))?;
    let head = kept["head"].as_str().unwrap_or("");
    let bundle = mirror
        .kept_bundle(branch, head, now)
        .map_err(|why| (StatusCode::CONFLICT, why))?;
    let said = format!(
        "checked out the team's kept branch {} at {} (task #{}'s, on top of {}; {} KB)",
        bundle.branch,
        &bundle.head[..12.min(bundle.head.len())],
        kept["task"].as_i64().unwrap_or(0),
        mirror.base(),
        bundle.bytes.len() >> 10
    );
    let text = json!({
        "branch": bundle.branch, "head": bundle.head, "base": mirror.base(), "kept": true,
        "bundle": repo::to_base64(&bundle.bytes),
    })
    .to_string();
    Ok(Done { text, said })
}

/// `repo_submit` (ADR 0115): a bundle of one `hq/` branch from the sandbox, taken by the
/// mirror's rules (one branch, on top of the default branch, within the limits, every commit
/// HQ's), kept under the task, and told to HQ, which writes its document and queues its check.
fn repo_submit(state: &AppState, turn: &Turn, args: &serde_json::Value) -> Result<Done, Refused> {
    let Some(task) = turn.task else {
        return Err((
            StatusCode::CONFLICT,
            "this turn is on no task: a branch is submitted for a task".to_owned(),
        ));
    };
    let branch = args["branch"].as_str().unwrap_or("").trim();
    let why = one_line(args["why"].as_str().unwrap_or(""));
    if why.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "say what the branch does, in `why`".to_owned(),
        ));
    }
    let Some(bytes) = args["bundle"].as_str().and_then(repo::from_base64) else {
        return Err((
            StatusCode::BAD_REQUEST,
            "the bundle is sent as base64 in `bundle` (the worker does this)".to_owned(),
        ));
    };
    let now = state.clock.now().unix_millis();
    let Some(mirror) = state.lab.mirror(now) else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, NO_FORK.to_owned()));
    };
    let taken = mirror
        .submit(&bytes, branch, task, now)
        .map_err(|why| (StatusCode::UNPROCESSABLE_ENTITY, why))?;
    let Some(hq) = state.hq.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "HQ is not set up on this node".to_owned(),
        ));
    };
    let recorded = hq
        .post(
            "/v1/team/branch",
            &json!({
                "token": turn.token, "branch": taken.branch, "head": taken.head,
                "base": taken.base, "files": taken.files, "plus": taken.plus,
                "minus": taken.minus, "stat": taken.stat, "diff": taken.diff,
                "can_push": state.lab.can_push(),
            }),
        )
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("the branch is kept on the node, but HQ did not record it: {e}"),
            )
        })?;
    let short = &taken.head[..12.min(taken.head.len())];
    Ok(Done {
        text: format!(
            "Submitted {} at {short} ({} file{}, +{} −{}): document d{} on task #{task}, \
             check c{} queued on the dev node (its result comes back as a receipt in the \
             thread). Nothing is pushed: once the tester and the lead accept the task, the \
             owner decides. Now submit the task for review with set_status in_review, citing \
             this receipt.",
            taken.branch,
            taken.files,
            if taken.files == 1 { "" } else { "s" },
            taken.plus,
            taken.minus,
            recorded["document"].as_i64().unwrap_or(0),
            recorded["check"].as_i64().unwrap_or(0)
        ),
        said: format!(
            "submitted branch {} at {short} ({} files, +{} −{}) for task {task}: {why}",
            taken.branch, taken.files, taken.plus, taken.minus
        ),
    })
}

/// `GET /team-relay/check/{id}/bundle` (ADR 0115): the whole history of the branch a check
/// job is for, to the job's sandbox, opened by the job's own token, which HQ checks. Nothing
/// else of the mirror is served.
pub(super) async fn check_bundle(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    let token = token_of(&headers);
    if token.len() < 32 {
        return refuse(StatusCode::UNAUTHORIZED, "", "no check token");
    }
    let Some(hq) = state.hq.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "",
            "HQ is not set up on this node",
        );
    };
    let asked =
        tokio::task::spawn_blocking(move || hq.post("/v1/team/check", &json!({ "token": token })))
            .await;
    let named = match asked {
        Ok(Ok(named)) if named["check"].as_i64() == Some(id) => named,
        Ok(Ok(_)) => {
            return refuse(
                StatusCode::UNAUTHORIZED,
                "",
                "that token is another check's",
            );
        }
        Ok(Err(endora_infrastructure::HqError::Answered { status: 404, .. })) => {
            return refuse(StatusCode::UNAUTHORIZED, "", "no check has that token");
        }
        _ => return refuse(StatusCode::SERVICE_UNAVAILABLE, "", "HQ did not answer"),
    };
    let branch = named["branch"].as_str().unwrap_or("").to_owned();
    let head = named["head"].as_str().unwrap_or("").to_owned();
    let now = state.clock.now().unix_millis();
    let looked = state.clone();
    let bundled = tokio::task::spawn_blocking(move || {
        let mirror = looked.lab.mirror(now).ok_or_else(|| NO_FORK.to_owned())?;
        mirror.submission_bundle(&branch, &head)
    })
    .await;
    match bundled {
        Ok(Ok(Some(bytes))) => {
            record(
                &state,
                &format!(
                    "check c{id} fetched the bundle of {} at {}",
                    named["branch"], named["head"]
                ),
            );
            (
                StatusCode::OK,
                [
                    (
                        axum::http::header::CONTENT_TYPE,
                        "application/octet-stream".to_owned(),
                    ),
                    (
                        axum::http::HeaderName::from_static("x-endora-branch"),
                        named["branch"].as_str().unwrap_or("").to_owned(),
                    ),
                    // The default branch's head the submission sits on: what the job judges
                    // the branch against ("no worse than develop").
                    (
                        axum::http::HeaderName::from_static("x-endora-base"),
                        named["base"].as_str().unwrap_or("").to_owned(),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(Ok(None)) => refuse(
            StatusCode::NOT_FOUND,
            "",
            "no submission of that branch is stored",
        ),
        Ok(Err(why)) => refuse(StatusCode::BAD_GATEWAY, "", &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "", "the bundle failed"),
    }
}

/// `GET /v1/hq/bundles/{task}/{slug}` (ADR 0115): the newest stored submission of
/// `hq/<slug>` for a task, whole, for the owner to fetch into their own clone when the node
/// cannot push.
pub(super) async fn bundle_download(
    State(state): State<AppState>,
    Path((task, slug)): Path<(i64, String)>,
) -> Response {
    let branch = format!("hq/{slug}");
    if repo::hq_branch(&branch).is_err() {
        return refuse(StatusCode::NOT_FOUND, "", "not one of the team's branches");
    }
    let now = state.clock.now().unix_millis();
    let looked = state.clone();
    let bundled = tokio::task::spawn_blocking(move || {
        let mirror = looked.lab.mirror(now).ok_or_else(|| NO_FORK.to_owned())?;
        let Some(stored) = mirror.stored_latest(task, &branch) else {
            return Ok(None);
        };
        let head = stored
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit('-').next())
            .unwrap_or("")
            .to_owned();
        mirror.submission_bundle(&branch, &head)
    })
    .await;
    match bundled {
        Ok(Ok(Some(bytes))) => {
            record(
                &state,
                &format!("you downloaded the bundle of hq/{slug} (task {task})"),
            );
            (
                StatusCode::OK,
                [
                    (
                        axum::http::header::CONTENT_TYPE,
                        "application/octet-stream".to_owned(),
                    ),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{slug}.bundle\""),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(Ok(None)) => refuse(
            StatusCode::NOT_FOUND,
            "",
            "no submission of that branch is stored",
        ),
        Ok(Err(why)) => refuse(StatusCode::BAD_GATEWAY, "", &why),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "", "the bundle failed"),
    }
}

/// Pushes the branch of a push the owner approved (ADR 0115), as HQ named it: to the fork over
/// SSH with the deploy key, as that branch alone, never force. Tells HQ what came of it and
/// writes the audit line. Returns what the owner reads.
pub(super) fn push_approved(state: &AppState, push: &serde_json::Value) -> Result<String, String> {
    let item = push["item"]
        .as_i64()
        .ok_or_else(|| "HQ named no desk item".to_owned())?;
    let branch = push["branch"].as_str().unwrap_or("").to_owned();
    let head = push["head"].as_str().unwrap_or("").to_owned();
    let now = state.clock.now().unix_millis();
    let pushed = state
        .lab
        .mirror(now)
        .ok_or_else(|| NO_FORK.to_owned())
        .and_then(|mirror| {
            let remote = state.lab.push_remote().ok_or_else(|| NO_FORK.to_owned())?;
            mirror.push(&branch, &head, &remote, state.lab.push_key.as_deref())
        });
    let (ok, text) = match &pushed {
        Ok(said) => (
            true,
            format!(
                "{said}. The branch is on the fork as {branch}: merge it there when you are ready; HQ opens no pull request and merges nothing."
            ),
        ),
        Err(why) => (
            false,
            format!("Pushing {branch} failed: {why}. It stays stored on the node."),
        ),
    };
    record(
        state,
        &format!(
            "you approved pushing {branch} ({}) to the fork: {}",
            &head[..12.min(head.len())],
            if ok { "pushed" } else { "it failed" }
        ),
    );
    if let Some(hq) = state.hq.as_ref() {
        if let Err(e) = hq.post(
            &format!("/v1/team/push/{item}/outcome"),
            &json!({ "ok": ok, "text": text }),
        ) {
            eprintln!("repo: could not tell HQ what came of push {item}: {e}");
        }
    }
    if ok {
        return Ok(text);
    }
    Err(text)
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

    #[test]
    fn hqs_no_to_a_kept_checkout_says_whether_the_turn_or_the_branch_is_missing() {
        use endora_infrastructure::HqError;
        let answered = |status: u16, message: &str| HqError::Answered {
            status,
            message: message.to_owned(),
        };
        // HQ answers 404 for both, and says which (the engine's `Unknown`).
        let (status, said) = checkout_refused(
            answered(404, "no such running turn with that token"),
            "hq/task-7-x",
            "main",
        );
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(said.contains("no running turn has that token"), "{said}");
        assert!(!said.contains("no branch"), "{said}");
        let (status, said) = checkout_refused(
            answered(404, "no such branch of that name"),
            "hq/task-7-x",
            "main",
        );
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            said,
            "the fork has no branch hq/task-7-x, and the team has not submitted one of that \
             name: check out main to start from it"
        );
        // Whose it is, said by HQ, is passed on; HQ out of reach is said so.
        let (status, said) = checkout_refused(
            answered(400, "hq/task-7-x is task #7's"),
            "hq/task-7-x",
            "main",
        );
        assert_eq!(
            (status, said.as_str()),
            (StatusCode::FORBIDDEN, "hq/task-7-x is task #7's")
        );
        let (status, said) = checkout_refused(
            HqError::Unreachable("refused".to_owned()),
            "hq/task-7-x",
            "main",
        );
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            said.starts_with("HQ could not say whose hq/task-7-x is"),
            "{said}"
        );
    }

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

    /// One call of a contract as the turn whose token is `token`.
    async fn as_turn(
        state: &AppState,
        token: &str,
        name: &str,
        args: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        answer(
            state,
            ask(
                "POST",
                &format!("/team-relay/contracts/{name}"),
                Some(token),
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

    /// The check after a deploy (ADR 0115, amended): HQ names the lab's three reads as all the
    /// turn may use, and the node offers and runs those alone. An act, a branch, the fork's
    /// docs, the lab's map or any bridged tool is refused here, before it runs, whatever the
    /// model was told; a refusal is kept as a receipt that failed.
    #[tokio::test]
    async fn a_deploy_checks_turn_is_given_the_lab_reads_and_refused_everything_else() {
        use super::super::relay::tests::fake;
        let (hq_url, seen) = fake(Arc::new(move |path, body| match path {
            "/v1/team/turn" if body.contains("\"good\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Ada", "role": "lead", "left": 0, "cloud": false,
                        "brain": "local", "task": 9, "wing": false,
                        "only": ["homelab_status", "container_logs", "nas_status"] })
                .to_string(),
            ),
            "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
            "/v1/team/receipt" => (200, "application/json", r#"{"receipt":41}"#.to_owned()),
            _ => (200, "application/json", "{}".to_owned()),
        }));
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab();
        let names = listed(&state).await;
        assert!(!names.is_empty());
        assert!(
            names
                .iter()
                .all(|n| ["homelab_status", "container_logs", "nas_status"].contains(&n.as_str())),
            "the lab's reads alone: {names:?}"
        );
        assert!(names.contains(&"homelab_status".to_owned()), "{names:?}");
        for name in [
            "restart_container",
            "repo_checkout",
            "repo_submit",
            "ansible_nas_doc",
            "homelab_map",
            "sonarr_queue",
            "read_file",
        ] {
            let (status, said) = use_contract(&state, name, json!({ "why": "look" })).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {said}");
            assert!(
                said["error"]
                    .as_str()
                    .unwrap()
                    .contains("is not among this turn's tools"),
                "{name}: {said}"
            );
        }
        let refusals: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/receipt")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(refusals.len(), 7, "each refusal is kept: {refusals:?}");
        assert!(
            refusals
                .iter()
                .all(|r| r["ok"] == false && r["act"] == false)
        );
        let (status, said) = use_contract(&state, "homelab_status", json!({})).await;
        assert_eq!(status, StatusCode::OK, "{said}");
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
    async fn the_cloud_ceo_reads_the_home_lab_as_the_lead_does_and_never_restarts_anything() {
        // The owner's word of 2026-10-18 (ADR 0120): reading is open to the CEO, whose system
        // it is; acting on the lab is the areas' own.
        let state = on_claude("ceo");
        let offered = listed(&state).await;
        for read in ["homelab_status", "container_logs", "homelab_map"] {
            assert!(offered.contains(&read.to_owned()), "{read}: {offered:?}");
        }
        assert!(
            !offered.contains(&"restart_container".to_owned()),
            "{offered:?}"
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
                .contains("direct the lead of the task's area"),
            "{said}"
        );
    }

    #[tokio::test]
    async fn a_cloud_turn_reads_logs_whatever_its_role() {
        // ADR 0141: Confidential is open to Claude; a log is read by any member on it.
        for role in ["reviewer", "developer", "researcher"] {
            let state = on_claude(role);
            assert!(
                listed(&state).await.contains(&"container_logs".to_owned()),
                "a cloud {role} is offered logs"
            );
            let (status, said) =
                use_contract(&state, "container_logs", json!({ "container": "sonarr" })).await;
            assert_eq!(status, StatusCode::OK, "{role}: {said}");
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
    async fn every_call_a_turn_makes_is_told_to_hq_as_a_receipt_with_the_turns_token() {
        use super::super::relay::tests::fake;
        let (hq_url, seen) = fake(Arc::new(|path, body| match path {
            "/v1/team/turn" if body.contains("\"good\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Linus", "role": "developer", "left": 0,
                        "cloud": false })
                .to_string(),
            ),
            "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
            "/v1/team/receipt" => (200, "application/json", r#"{"receipt":41}"#.to_owned()),
            _ => (200, "application/json", "{}".to_owned()),
        }));
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab();
        let (status, said) =
            use_contract(&state, "container_logs", json!({ "container": "sonarr" })).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert!(
            said["text"].as_str().unwrap().ends_with(" (receipt r41)"),
            "{said}"
        );
        let (status, said) = use_contract(
            &state,
            "container_logs",
            json!({ "container": "endora-hq" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(said["error"].as_str().unwrap().ends_with(" (receipt r41)"));
        let told: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/receipt")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(told.len(), 2);
        assert_eq!(
            (
                &told[0]["token"],
                &told[0]["tool"],
                &told[0]["args"],
                &told[0]["ok"],
                &told[0]["confidential"]
            ),
            (
                &json!("good"),
                &json!("container_logs"),
                &json!("container: sonarr"),
                &json!(true),
                &json!(true)
            ),
            "the node writes it, with the turn's own token"
        );
        assert!(
            !told[0]["excerpt"].as_str().unwrap().contains("hunter2"),
            "scrubbed"
        );
        assert_eq!(told[1]["ok"], false, "a refusal is a receipt too");
        // A stranger's token makes no receipt at all.
        let (status, _) = answer(
            &state,
            ask(
                "POST",
                "/team-relay/contracts/homelab_status",
                Some("bad"),
                "{}",
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            seen.lock()
                .unwrap()
                .iter()
                .filter(|(p, _, _)| p == "/v1/team/receipt")
                .count(),
            2
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

    /// One `git` in a test's repository.
    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A fork as a bare repository with one commit on `main`, in a folder of its own.
    fn a_fork(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        a_fork_on(name, "main", "---\n- hosts: all\n")
    }

    /// A fork as a bare repository with one commit on `base`, whose `nas.yml` says `yaml`, in a
    /// folder of its own.
    fn a_fork_on(name: &str, base: &str, yaml: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "endora-node-wing-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        git_in(&root, &["init", "-q", "--bare", "-b", base, "fork.git"]);
        git_in(&root, &["clone", "-q", "fork.git", "seed"]);
        let seed = root.join("seed");
        std::fs::write(seed.join("nas.yml"), yaml).unwrap();
        git_in(&seed, &["add", "."]);
        git_in(
            &seed,
            &[
                "-c",
                "user.name=John Doe",
                "-c",
                "user.email=john@example.com",
                "commit",
                "-q",
                "-m",
                "the lab",
            ],
        );
        git_in(&seed, &["push", "-q", "origin", &format!("HEAD:{base}")]);
        (root.clone(), root.join("fork.git"))
    }

    /// One docs-only branch, with real git and a local bare fork, from the checkout of the fork's
    /// `develop` to the fork's `hq/` branch after the owner's approval: what the diagnosis of the
    /// wing's first 39 hours (six branches, none reached the owner) asked to have proved where
    /// it can be, in the node. The sandbox is played with plain git. The worker's checkout and
    /// bundling and the check job's unpacking and judging keep their own tests in
    /// `app/worker`; HQ's rules are played by a fake and held by the engine's own tests and the
    /// scenarios.
    #[tokio::test]
    async fn a_docs_only_branch_goes_from_the_forks_develop_to_the_forks_hq_branch_with_real_git() {
        use super::super::relay::tests::fake;
        // Develop has a lint finding of its own (trailing spaces, no document start).
        let (root, fork) = a_fork_on("docs", "develop", "- hosts: all   \n");
        let develop = git_in(&fork, &["rev-parse", "refs/heads/develop"]);
        let recorded: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let told = recorded.clone();
        let (hq_url, seen) = fake(Arc::new(move |path, body| {
            let body: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let said =
                |status: u16, v: serde_json::Value| (status, "application/json", v.to_string());
            let on_task_7 = |token: &str| matches!(token, "rae" | "dee");
            match (path, body["token"].as_str().unwrap_or("")) {
                ("/v1/team/turn", "rae") => said(
                    200,
                    json!({ "run": 7, "member": "Rae", "role": "developer", "left": 40_000,
                            "cloud": true, "task": 7 }),
                ),
                ("/v1/team/turn", "dee") => said(
                    200,
                    json!({ "run": 8, "member": "Dee", "role": "reviewer", "left": 40_000,
                            "cloud": true, "task": 7 }),
                ),
                ("/v1/team/turn", "zed") => said(
                    200,
                    json!({ "run": 9, "member": "Zed", "role": "developer", "left": 40_000,
                            "cloud": true, "task": 9 }),
                ),
                ("/v1/team/turn", _) => said(404, json!({})),
                ("/v1/team/branch", _) => {
                    *told.lock().unwrap() = Some(body);
                    said(200, json!({ "branch": 1, "document": 5, "check": 9 }))
                }
                ("/v1/team/branch/checkout", token) if on_task_7(token) => {
                    let branch = told.lock().unwrap().clone().unwrap_or_default();
                    said(
                        200,
                        json!({ "head": branch["head"], "base": branch["base"], "task": 7 }),
                    )
                }
                ("/v1/team/branch/checkout", _) => said(
                    400,
                    json!({ "error": "hq/task-7-add-pages is task #7's: check out the branch of your own work" }),
                ),
                ("/v1/team/check", "a-check-jobs-token-of-thirty-two-chars-x") => {
                    let branch = told.lock().unwrap().clone().unwrap_or_default();
                    said(
                        200,
                        json!({ "check": 9, "task": 7, "branch": branch["branch"],
                                "head": branch["head"], "base": branch["base"] }),
                    )
                }
                ("/v1/team/check", _) => said(404, json!({})),
                _ => {
                    // The owner's approval of push item 4 names the branch and the head HQ
                    // recorded; every other call gets its receipt or an empty answer.
                    if path == "/v1/desk/4" {
                        let branch = told.lock().unwrap().clone().unwrap_or_default();
                        return said(
                            200,
                            json!({ "decided": true, "push": { "item": 4, "task": 7,
                                    "branch": branch["branch"], "head": branch["head"] } }),
                        );
                    }
                    if path == "/v1/team/receipt" {
                        return said(200, json!({ "receipt": 41 }));
                    }
                    said(200, json!({}))
                }
            }
        }));
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab();
        state.lab = Arc::new(
            LabKnowledge::new(Docs::new(None, Box::new(ansible_nas::Web)), Vec::new()).with_repo(
                root.join("data"),
                fork.to_str().unwrap(),
                "develop",
                fork.to_str().unwrap(),
            ),
        );
        // 1. Rae's turn starts in the fork: a bundle of develop, which her sandbox clones.
        let (status, said) = as_turn(&state, "rae", REPO_CHECKOUT, json!({ "why": "start" })).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let (payload, _) = said["text"]
            .as_str()
            .unwrap()
            .rsplit_once(" (receipt ")
            .unwrap();
        let handed: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(
            (handed["branch"].as_str(), handed["base"].as_str()),
            (Some("develop"), Some("develop"))
        );
        let file = root.join("checkout.bundle");
        std::fs::write(
            &file,
            repo::from_base64(handed["bundle"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        git_in(
            &root,
            &[
                "clone",
                "-q",
                "-b",
                "develop",
                file.to_str().unwrap(),
                "work",
            ],
        );
        let work = root.join("work");
        // 2. A docs-only commit on her task's branch, HQ-authored with the trailer.
        git_in(
            &work,
            &["checkout", "-q", "-b", "hq/task-7-add-pages", "develop"],
        );
        std::fs::create_dir_all(work.join("website/docs/applications")).unwrap();
        std::fs::write(work.join("website/docs/applications/foo.md"), "# Foo\n").unwrap();
        git_in(&work, &["add", "."]);
        git_in(
            &work,
            &[
                "-c",
                "user.name=Endora HQ",
                "-c",
                "user.email=hq@endora.invalid",
                "commit",
                "-q",
                "-m",
                "add the foo page\n\nMade-by: Endora HQ (Rae) for task #7",
            ],
        );
        let head = git_in(&work, &["rev-parse", "HEAD"]);
        let delta = root.join("delta.bundle");
        git_in(
            &work,
            &[
                "bundle",
                "create",
                delta.to_str().unwrap(),
                "develop..hq/task-7-add-pages",
            ],
        );
        let delta = repo::to_base64(&std::fs::read(&delta).unwrap());
        // 3. The node takes it and tells HQ the branch, its head and the base it sits on.
        let (status, said) = as_turn(
            &state,
            "rae",
            REPO_SUBMIT,
            json!({ "branch": "hq/task-7-add-pages", "why": "adds the foo page", "bundle": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let told_hq = recorded.lock().unwrap().clone().unwrap();
        assert_eq!(told_hq["branch"], "hq/task-7-add-pages");
        assert_eq!(told_hq["head"], head.as_str());
        assert_eq!(
            told_hq["base"],
            develop.as_str(),
            "the base it sits on: develop's head"
        );
        assert_eq!(told_hq["files"], 1);
        // 4. The check job fetches the branch's whole history with its token, and the base HQ
        //    recorded rides in the header and is in that history: what "no worse than develop"
        //    needs to run the same checks there.
        let job = "a-check-jobs-token-of-thirty-two-chars-x";
        let res = team_relay(state.clone())
            .oneshot(ask("GET", "/team-relay/check/9/bundle", Some(job), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-endora-base"].to_str().unwrap(), develop);
        let whole = res.into_body().collect().await.unwrap().to_bytes();
        let file = root.join("whole.bundle");
        std::fs::write(&file, &whole).unwrap();
        git_in(&root, &["init", "-q", "check"]);
        let check = root.join("check");
        git_in(
            &check,
            &[
                "fetch",
                "-q",
                file.to_str().unwrap(),
                "refs/submissions/hq/task-7-add-pages:refs/heads/check",
            ],
        );
        assert_eq!(git_in(&check, &["rev-parse", "check"]), head);
        git_in(
            &check,
            &["cat-file", "-e", &format!("{develop}^{{commit}}")],
        );
        // 5. Dee, the tester of the same task, has the kept branch for her own reading, with
        //    develop behind it; another task's developer is refused; she submits nothing.
        let (status, said) = as_turn(
            &state,
            "dee",
            REPO_CHECKOUT,
            json!({ "branch": "hq/task-7-add-pages", "why": "to read it" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        let (payload, receipt) = text.rsplit_once(" (receipt ").unwrap();
        assert!(receipt.starts_with("r41"), "her receipt: {receipt}");
        let kept: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(
            (
                kept["kept"].as_bool(),
                kept["head"].as_str(),
                kept["base"].as_str()
            ),
            (Some(true), Some(head.as_str()), Some("develop"))
        );
        let kept_file = root.join("kept.bundle");
        std::fs::write(
            &kept_file,
            repo::from_base64(kept["bundle"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let heads = git_in(
            &root,
            &["bundle", "list-heads", kept_file.to_str().unwrap()],
        );
        assert!(
            heads.contains("refs/heads/develop")
                && heads.contains("refs/submissions/hq/task-7-add-pages"),
            "{heads}"
        );
        git_in(
            &root,
            &[
                "clone",
                "-q",
                "-b",
                "develop",
                kept_file.to_str().unwrap(),
                "reading",
            ],
        );
        git_in(
            &root.join("reading"),
            &[
                "fetch",
                "-q",
                kept_file.to_str().unwrap(),
                "refs/submissions/hq/task-7-add-pages:refs/heads/hq/task-7-add-pages",
            ],
        );
        let diff = git_in(
            &root.join("reading"),
            &["diff", "--name-only", "develop..hq/task-7-add-pages"],
        );
        assert_eq!(
            diff, "website/docs/applications/foo.md",
            "what she reads is the docs-only change"
        );
        let (status, said) = as_turn(
            &state,
            "zed",
            REPO_CHECKOUT,
            json!({ "branch": "hq/task-7-add-pages", "why": "to look" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains("task #7's"),
            "{said}"
        );
        let (status, _) = as_turn(
            &state,
            "dee",
            REPO_SUBMIT,
            json!({ "branch": "hq/task-7-add-pages", "why": "x", "bundle": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // 6. The owner approves the push (desk item 4): the node sends the stored ref to the
        //    fork as the `hq/` branch, at that head, and develop is untouched.
        let mut signed = state.clone();
        signed.token = Arc::new(super::super::tests::TEST_TOKEN.to_owned());
        let decided = super::super::hq::hq_decide(
            State(signed.clone()),
            Path(4),
            Json(super::super::hq::HqDecision {
                decision: "approve".to_owned(),
                answer: String::new(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(decided.0["pushed"], true, "{}", decided.0);
        assert_eq!(
            git_in(&fork, &["rev-parse", "refs/heads/hq/task-7-add-pages"]),
            head
        );
        assert_eq!(
            git_in(&fork, &["rev-parse", "refs/heads/develop"]),
            develop,
            "develop untouched"
        );
        let outcome: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/push/4/outcome")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(outcome.len(), 1);
        assert_eq!(outcome[0]["ok"], true);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn the_wing_checks_out_the_fork_submits_a_branch_and_the_check_and_the_push_follow() {
        use super::super::relay::tests::fake;
        let (root, fork) = a_fork("flow");
        // HQ, faked: Kit's turn (token `good`, a lead on task 3), Dee's (`dee`, the tester),
        // and what the node tells it: the branch, the check a job token names, the push.
        let last_branch: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let told = last_branch.clone();
        let (hq_url, seen) = fake(Arc::new(move |path, body| {
            let body: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            match path {
                "/v1/team/turn" if body["token"] == "good" => (
                    200,
                    "application/json",
                    json!({ "run": 7, "member": "Kit", "role": "lead", "left": 40_000,
                            "cloud": true, "task": 3 })
                    .to_string(),
                ),
                "/v1/team/turn" if body["token"] == "dee" => (
                    200,
                    "application/json",
                    json!({ "run": 8, "member": "Dee", "role": "reviewer", "left": 0,
                            "cloud": false, "task": 3 })
                    .to_string(),
                ),
                // The Ops lead: a lead, outside the engineering wing (ADR 0120).
                "/v1/team/turn" if body["token"] == "ada" => (
                    200,
                    "application/json",
                    json!({ "run": 10, "member": "Ada", "role": "lead", "left": 40_000,
                            "cloud": true, "task": 4, "wing": false })
                    .to_string(),
                ),
                "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
                "/v1/team/branch" => {
                    *told.lock().unwrap() = Some(body);
                    (
                        200,
                        "application/json",
                        json!({ "branch": 1, "document": 5, "check": 9 }).to_string(),
                    )
                }
                "/v1/team/check" if body["token"] == "a-check-jobs-token-of-thirty-two-chars-x" => {
                    let branch = told.lock().unwrap().clone().unwrap_or_default();
                    (
                        200,
                        "application/json",
                        json!({ "check": 9, "task": 3, "branch": branch["branch"],
                                "head": branch["head"], "base": branch["base"] })
                        .to_string(),
                    )
                }
                "/v1/team/check" => (404, "application/json", "{}".to_owned()),
                "/v1/desk/4" => {
                    let branch = told.lock().unwrap().clone().unwrap_or_default();
                    (
                        200,
                        "application/json",
                        json!({ "decided": true, "push": { "item": 4, "task": 3,
                                "branch": branch["branch"], "head": branch["head"] } })
                        .to_string(),
                    )
                }
                "/v1/team/receipt" => (200, "application/json", r#"{"receipt":41}"#.to_owned()),
                _ => (200, "application/json", "{}".to_owned()),
            }
        }));
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab();
        state.lab = Arc::new(
            LabKnowledge::new(Docs::new(None, Box::new(ansible_nas::Web)), Vec::new()).with_repo(
                root.join("data"),
                fork.to_str().unwrap(),
                "main",
                fork.to_str().unwrap(),
            ),
        );
        // A lead and a developer are offered the wing's git; the tester is offered the checkout
        // alone (it is her reading of a branch), never the submission.
        let (_, listed) = answer(
            &state,
            ask("GET", "/team-relay/contracts", Some("good"), ""),
        )
        .await;
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(
            names.contains(&REPO_CHECKOUT) && names.contains(&REPO_SUBMIT),
            "{names:?}"
        );
        assert_eq!(
            listed["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == REPO_SUBMIT)
                .unwrap()["kind"],
            "act"
        );
        let (_, listed) =
            answer(&state, ask("GET", "/team-relay/contracts", Some("dee"), "")).await;
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(
            names.contains(&REPO_CHECKOUT) && !names.contains(&REPO_SUBMIT),
            "a tester is offered checkout, never submit: {names:?}"
        );
        // Dee's refusal of a submission says the tester's rule, not the CEO's.
        let (refused, why) = answer(
            &state,
            ask(
                "POST",
                "/team-relay/contracts/repo_submit",
                Some("dee"),
                r#"{"branch":"hq/task-3-x","why":"look","bundle":"AAAA"}"#,
            ),
        )
        .await;
        assert_eq!(refused, StatusCode::FORBIDDEN, "{why}");
        assert!(
            why["error"]
                .as_str()
                .unwrap()
                .contains("reads a branch by checking it out"),
            "{why}"
        );
        // A lead outside the wing is offered no submission, and one she asks for is refused
        // before the node keeps anything or tells HQ of a branch (task 687, ADR 0120).
        let (_, listed) =
            answer(&state, ask("GET", "/team-relay/contracts", Some("ada"), "")).await;
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(
            names.contains(&REPO_CHECKOUT) && !names.contains(&REPO_SUBMIT),
            "outside the wing, checkout and never submit: {names:?}"
        );
        let (refused, why) = answer(
            &state,
            ask(
                "POST",
                "/team-relay/contracts/repo_submit",
                Some("ada"),
                r#"{"branch":"hq/task-4-x","why":"drift check","bundle":"AAAA"}"#,
            ),
        )
        .await;
        assert_eq!(refused, StatusCode::FORBIDDEN, "{why}");
        assert!(
            why["error"].as_str().unwrap().contains("engineering wing"),
            "{why}"
        );
        assert!(
            last_branch.lock().unwrap().is_none(),
            "HQ was told of no branch"
        );
        // The tester checks the fork out like anyone: a bundle, and a receipt of hers.
        let (status, said) = answer(
            &state,
            ask(
                "POST",
                "/team-relay/contracts/repo_checkout",
                Some("dee"),
                r#"{"why":"look"}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        // The checkout: a bundle of main the sandbox can clone, said in the receipt by its head.
        let (status, said) = use_contract(&state, REPO_CHECKOUT, json!({ "why": "x" })).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        let (bundle_json, receipt) = text.rsplit_once(" (receipt ").unwrap();
        assert!(receipt.starts_with("r41"), "{receipt}");
        let checkout: serde_json::Value = serde_json::from_str(bundle_json).unwrap();
        assert_eq!(checkout["branch"], "main");
        assert_eq!(checkout["base"], "main");
        let bytes = repo::from_base64(checkout["bundle"].as_str().unwrap()).unwrap();
        let file = root.join("checkout.bundle");
        std::fs::write(&file, &bytes).unwrap();
        git_in(
            &root,
            &["clone", "-q", "-b", "main", file.to_str().unwrap(), "work"],
        );
        let work = root.join("work");
        assert_eq!(
            git_in(&work, &["rev-parse", "HEAD"]),
            checkout["head"].as_str().unwrap()
        );
        let receipts = |seen: &super::super::relay::tests::Seen| -> Vec<serde_json::Value> {
            seen.lock()
                .unwrap()
                .iter()
                .filter(|(p, _, _)| p == "/v1/team/receipt")
                .map(|(_, _, b)| serde_json::from_str(b).unwrap())
                .collect()
        };
        let checked_out = receipts(&seen).pop().unwrap();
        assert!(
            checked_out["excerpt"]
                .as_str()
                .unwrap()
                .starts_with("checked out main at "),
            "{checked_out}"
        );
        assert_eq!(checked_out["act"], false);
        // Kit's work: one commit, authored HQ with its trailer, bundled from main.
        git_in(
            &work,
            &["checkout", "-q", "-b", "hq/task-3-tidy-roles", "main"],
        );
        std::fs::write(work.join("nas.yml"), "---\n- hosts: nas\n").unwrap();
        git_in(&work, &["add", "."]);
        git_in(
            &work,
            &[
                "-c",
                "user.name=Endora HQ",
                "-c",
                "user.email=hq@endora.invalid",
                "commit",
                "-q",
                "-m",
                "tidy\n\nMade-by: Endora HQ (Kit) for task #3",
            ],
        );
        let head = git_in(&work, &["rev-parse", "HEAD"]);
        let delta = root.join("delta.bundle");
        git_in(
            &work,
            &[
                "bundle",
                "create",
                delta.to_str().unwrap(),
                "main..hq/task-3-tidy-roles",
            ],
        );
        let delta = repo::to_base64(&std::fs::read(&delta).unwrap());
        let (status, said) = use_contract(
            &state,
            REPO_SUBMIT,
            json!({ "branch": "main", "why": "x", "bundle": delta }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "never main: {said}"
        );
        let (status, said) = use_contract(
            &state,
            REPO_SUBMIT,
            json!({ "branch": "hq/task-3-tidy-roles", "bundle": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "a why: {said}");
        let (status, said) = use_contract(
            &state,
            REPO_SUBMIT,
            json!({ "branch": "hq/task-3-tidy-roles", "why": "tidies the play", "bundle": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        assert!(
            text.contains("document d5")
                && text.contains("check c9")
                && text.contains("Nothing is pushed"),
            "{text}"
        );
        let told_hq = last_branch.lock().unwrap().clone().unwrap();
        assert_eq!(told_hq["token"], "good", "the turn's own token");
        assert_eq!(told_hq["branch"], "hq/task-3-tidy-roles");
        assert_eq!(told_hq["head"], head);
        assert_eq!(told_hq["files"], 1);
        assert_eq!(told_hq["can_push"], false, "no deploy key on this node");
        assert!(told_hq["diff"].as_str().unwrap().contains("+- hosts: nas"));
        let submitted = receipts(&seen).pop().unwrap();
        assert_eq!(submitted["act"], true);
        assert!(
            submitted["excerpt"]
                .as_str()
                .unwrap()
                .starts_with("Submitted hq/task-3-tidy-roles")
        );
        // The check's sandbox fetches the whole branch with the job's token, and nothing else opens it.
        let job = "a-check-jobs-token-of-thirty-two-chars-x";
        let res = team_relay(state.clone())
            .oneshot(ask("GET", "/team-relay/check/9/bundle", Some(job), ""))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-endora-branch"], "hq/task-3-tidy-roles");
        assert_eq!(
            res.headers()["x-endora-base"].to_str().unwrap(),
            told_hq["base"].as_str().unwrap(),
            "the base HQ recorded rides with the bundle"
        );
        let whole = res.into_body().collect().await.unwrap().to_bytes();
        let file = root.join("whole.bundle");
        std::fs::write(&file, &whole).unwrap();
        git_in(&root, &["init", "-q", "check"]);
        git_in(
            &root.join("check"),
            &[
                "fetch",
                "-q",
                file.to_str().unwrap(),
                "refs/submissions/hq/task-3-tidy-roles:refs/heads/check",
            ],
        );
        assert_eq!(git_in(&root.join("check"), &["rev-parse", "check"]), head);
        for (id, token) in [
            (8, Some(job)),
            (9, Some("good")),
            (9, None),
            (9, Some("short")),
        ] {
            let (status, _) = answer(
                &state,
                ask("GET", &format!("/team-relay/check/{id}/bundle"), token, ""),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{id} {token:?}");
        }
        // The owner downloads the bundle from the node's own port.
        let owner = super::super::tests::get("/v1/hq/bundles/3/task-3-tidy-roles");
        let mut signed = state.clone();
        signed.token = Arc::new(super::super::tests::TEST_TOKEN.to_owned());
        let res = app(signed.clone()).oneshot(owner).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()["content-disposition"]
                .to_str()
                .unwrap()
                .contains("task-3-tidy-roles.bundle")
        );
        let res = app(signed.clone())
            .oneshot(super::super::tests::get("/v1/hq/bundles/3/nothing-here"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        // The owner approves the push (desk item 4): the node pushes that branch to the fork,
        // as itself, and tells HQ; main is untouched.
        let decided = super::super::hq::hq_decide(
            State(signed.clone()),
            Path(4),
            Json(super::super::hq::HqDecision {
                decision: "approve".to_owned(),
                answer: String::new(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(decided.0["pushed"], true, "{}", decided.0);
        assert_eq!(
            git_in(&fork, &["rev-parse", "refs/heads/hq/task-3-tidy-roles"]),
            head
        );
        assert_eq!(
            git_in(&fork, &["rev-parse", "refs/heads/main"]),
            checkout["head"].as_str().unwrap()
        );
        let outcome: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/push/4/outcome")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(outcome.len(), 1);
        assert_eq!(outcome[0]["ok"], true);
        assert!(
            outcome[0]["text"]
                .as_str()
                .unwrap()
                .contains("merges nothing")
        );
        let audit = audit(&signed);
        assert!(
            audit
                .iter()
                .any(|a| a.contains("you approved pushing hq/task-3-tidy-roles")
                    && a.ends_with("pushed")),
            "{audit:?}"
        );
        assert!(
            audit
                .iter()
                .any(|a| a.contains("submitted branch hq/task-3-tidy-roles")),
            "{audit:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Live (2026-10-18): `repo_checkout` of a submitted `hq/` branch was refused, "the fork has
    /// no branch", three times, though the node was keeping it: a submission lives under
    /// `refs/submissions/`, and only the owner's push puts a branch on the fork. Real git, a
    /// local bare fork, and HQ played by a fake that applies its rule: a kept branch is its own
    /// task's.
    #[tokio::test]
    async fn the_team_checks_out_its_own_kept_branch_and_only_its_own() {
        use super::super::relay::tests::fake;
        let (root, fork) = a_fork("kept");
        let recorded: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let first_head: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let (told, first) = (recorded.clone(), first_head.clone());
        let (hq_url, seen) = fake(Arc::new(move |path, body| {
            let body: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let said =
                |status: u16, v: serde_json::Value| (status, "application/json", v.to_string());
            match (path, body["token"].as_str()) {
                // Kit leads task 7; Zed is a developer on task 9.
                ("/v1/team/turn", Some("kit" | "late")) => said(
                    200,
                    json!({ "run": 7, "member": "Kit", "role": "lead", "left": 40_000,
                            "cloud": true, "task": 7 }),
                ),
                ("/v1/team/turn", Some("zed")) => said(
                    200,
                    json!({ "run": 9, "member": "Zed", "role": "developer", "left": 40_000,
                            "cloud": true, "task": 9 }),
                ),
                // Dee, the tester, is on task 7 too, on Haiku: a turn of judgment.
                ("/v1/team/turn", Some("dee")) => said(
                    200,
                    json!({ "run": 8, "member": "Dee", "role": "reviewer", "left": 40_000,
                            "cloud": true, "task": 7 }),
                ),
                ("/v1/team/turn", _) => said(404, json!({})),
                ("/v1/team/branch", _) => {
                    let mut first = first.lock().unwrap();
                    if first.is_empty() {
                        *first = body["head"].as_str().unwrap_or("").to_owned();
                    }
                    *told.lock().unwrap() = Some(body);
                    said(200, json!({ "branch": 1, "document": 5, "check": 9 }))
                }
                // HQ's rule, played here (the engine's tests hold the real one): the head it
                // recorded, to the branch's own task, and to no other.
                ("/v1/team/branch/checkout", token) => {
                    let branch = told.lock().unwrap().clone().unwrap_or_default();
                    if body["branch"] != branch["branch"] {
                        return said(404, json!({ "error": "no branch of that name" }));
                    }
                    match token {
                        Some("kit" | "dee") => said(
                            200,
                            json!({ "head": branch["head"], "base": branch["base"], "task": 7 }),
                        ),
                        // HQ's record names a head the node no longer keeps.
                        Some("late") => said(
                            200,
                            json!({ "head": first.lock().unwrap().clone(),
                                    "base": branch["base"], "task": 7 }),
                        ),
                        _ => said(
                            400,
                            json!({ "error": "hq/task-7-add-pages is task #7's: check out the branch of your own work" }),
                        ),
                    }
                }
                ("/v1/team/receipt", _) => said(200, json!({ "receipt": 41 })),
                _ => said(200, json!({})),
            }
        }));
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab();
        state.lab = Arc::new(
            LabKnowledge::new(Docs::new(None, Box::new(ansible_nas::Web)), Vec::new()).with_repo(
                root.join("data"),
                fork.to_str().unwrap(),
                "main",
                fork.to_str().unwrap(),
            ),
        );
        // Kit checks out the fork, makes the docs-only branch, and submits it.
        let (status, said) = as_turn(&state, "kit", REPO_CHECKOUT, json!({ "why": "x" })).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        let (payload, _) = text.rsplit_once(" (receipt ").unwrap();
        let handed: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert!(
            handed["kept"].is_null() || handed["kept"] == false,
            "the fork's own branch is not a kept one"
        );
        let file = root.join("checkout.bundle");
        std::fs::write(
            &file,
            repo::from_base64(handed["bundle"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        git_in(
            &root,
            &["clone", "-q", "-b", "main", file.to_str().unwrap(), "work"],
        );
        let work = root.join("work");
        git_in(
            &work,
            &["checkout", "-q", "-b", "hq/task-7-add-pages", "main"],
        );
        std::fs::create_dir_all(work.join("website/docs/applications")).unwrap();
        std::fs::write(work.join("website/docs/applications/foo.md"), "# Foo\n").unwrap();
        git_in(&work, &["add", "."]);
        git_in(
            &work,
            &[
                "-c",
                "user.name=Endora HQ",
                "-c",
                "user.email=hq@endora.invalid",
                "commit",
                "-q",
                "-m",
                "add the page\n\nMade-by: Endora HQ (Kit) for task #7",
            ],
        );
        let head = git_in(&work, &["rev-parse", "HEAD"]);
        let delta = root.join("delta.bundle");
        git_in(
            &work,
            &[
                "bundle",
                "create",
                delta.to_str().unwrap(),
                "main..hq/task-7-add-pages",
            ],
        );
        let delta = repo::to_base64(&std::fs::read(&delta).unwrap());
        // A branch is named for the turn's task: task 7's turn cannot overwrite task 9's.
        let (status, said) = as_turn(
            &state,
            "kit",
            REPO_SUBMIT,
            json!({ "branch": "hq/task-9-add-pages", "why": "x", "bundle": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains("hq/task-7-<slug>"),
            "{said}"
        );
        let (status, said) = as_turn(
            &state,
            "kit",
            REPO_SUBMIT,
            json!({ "branch": "hq/task-7-add-pages", "why": "adds a page", "bundle": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        // The fork has no such branch, but the team's own task may have it back: the kept
        // submission, with the base behind it.
        let (status, said) = as_turn(
            &state,
            "kit",
            REPO_CHECKOUT,
            json!({ "branch": "hq/task-7-add-pages", "why": "to go on" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        let (payload, receipt) = text.rsplit_once(" (receipt ").unwrap();
        assert!(receipt.starts_with("r41"), "{receipt}");
        let handed: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(handed["kept"], true);
        assert_eq!(
            (
                handed["branch"].as_str(),
                handed["head"].as_str(),
                handed["base"].as_str()
            ),
            (
                Some("hq/task-7-add-pages"),
                Some(head.as_str()),
                Some("main")
            )
        );
        let kept = root.join("kept.bundle");
        std::fs::write(
            &kept,
            repo::from_base64(handed["bundle"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let heads = git_in(&root, &["bundle", "list-heads", kept.to_str().unwrap()]);
        assert!(
            heads.contains("refs/heads/main")
                && heads.contains("refs/submissions/hq/task-7-add-pages"),
            "{heads}"
        );
        git_in(
            &root,
            &["clone", "-q", "-b", "main", kept.to_str().unwrap(), "again"],
        );
        git_in(
            &root.join("again"),
            &[
                "fetch",
                "-q",
                kept.to_str().unwrap(),
                "refs/submissions/hq/task-7-add-pages:refs/heads/hq/task-7-add-pages",
            ],
        );
        git_in(
            &root.join("again"),
            &["checkout", "-q", "hq/task-7-add-pages"],
        );
        assert_eq!(git_in(&root.join("again"), &["rev-parse", "HEAD"]), head);
        assert!(root.join("again/website/docs/applications/foo.md").exists());
        // HQ was asked, with the turn's own token and the branch, and the receipt says what
        // was checked out: the branch, its head, and that it is the team's kept one.
        let asked: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/branch/checkout")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(
            asked.len(),
            1,
            "asked once, for the kept one only: {asked:?}"
        );
        assert_eq!(
            asked[0],
            json!({ "token": "kit", "branch": "hq/task-7-add-pages" })
        );
        let receipt: serde_json::Value = serde_json::from_str(
            &seen
                .lock()
                .unwrap()
                .iter()
                .rfind(|(p, _, _)| p == "/v1/team/receipt")
                .unwrap()
                .2,
        )
        .unwrap();
        let excerpt = receipt["excerpt"].as_str().unwrap();
        assert!(
            excerpt.starts_with("checked out the team's kept branch hq/task-7-add-pages at ")
                && excerpt.contains(&head[..12]),
            "{excerpt}"
        );
        assert_eq!(receipt["act"], false);
        assert!(
            receipt["args"]
                .as_str()
                .unwrap()
                .contains("hq/task-7-add-pages"),
            "{receipt}"
        );
        // The tester of the same task reads it too: a reading of her own, whose receipt names
        // the branch and its head (she cites it with the check's). She submits nothing.
        let (status, said) = as_turn(
            &state,
            "dee",
            REPO_CHECKOUT,
            json!({ "branch": "hq/task-7-add-pages", "why": "to read it" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert!(
            said["text"].as_str().unwrap().contains("\"kept\":true"),
            "{said}"
        );
        let (status, said) = as_turn(
            &state,
            "dee",
            REPO_SUBMIT,
            json!({ "branch": "hq/task-7-add-pages", "why": "x", "bundle": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
        // Another task's turn is refused, with HQ's words; a branch nobody submitted is not
        // there; and a head HQ recorded that the node no longer keeps is not handed out.
        let (status, said) = as_turn(
            &state,
            "zed",
            REPO_CHECKOUT,
            json!({ "branch": "hq/task-7-add-pages", "why": "to look" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains("task #7's"),
            "{said}"
        );
        let (status, said) = as_turn(
            &state,
            "kit",
            REPO_CHECKOUT,
            json!({ "branch": "hq/task-7-nothing", "why": "x" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{said}");
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("has not submitted"),
            "{said}"
        );
        git_in(&work, &["checkout", "-q", "hq/task-7-add-pages"]);
        std::fs::write(work.join("website/docs/applications/bar.md"), "# Bar\n").unwrap();
        git_in(&work, &["add", "."]);
        git_in(
            &work,
            &[
                "-c",
                "user.name=Endora HQ",
                "-c",
                "user.email=hq@endora.invalid",
                "commit",
                "-q",
                "-m",
                "another page\n\nMade-by: Endora HQ (Kit) for task #7",
            ],
        );
        git_in(
            &work,
            &[
                "bundle",
                "create",
                delta_path(&root).as_str(),
                "main..hq/task-7-add-pages",
            ],
        );
        let again = repo::to_base64(&std::fs::read(delta_path(&root)).unwrap());
        let (status, said) = as_turn(
            &state,
            "kit",
            REPO_SUBMIT,
            json!({ "branch": "hq/task-7-add-pages", "why": "one more page", "bundle": again }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let (status, said) = as_turn(
            &state,
            "late",
            REPO_CHECKOUT,
            json!({ "branch": "hq/task-7-add-pages", "why": "x" }),
        )
        .await;
        assert!(
            status == StatusCode::CONFLICT
                && said["error"].as_str().unwrap().contains("submitted again"),
            "{status}: {said}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Where a test puts a second bundle of a branch.
    fn delta_path(root: &std::path::Path) -> String {
        root.join("delta-two.bundle").to_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn without_a_fork_the_wings_git_says_so_and_a_turn_on_no_task_submits_nothing() {
        use super::super::relay::tests::fake;
        let (hq_url, _) = fake(Arc::new(|path, body| match path {
            "/v1/team/turn" if body.contains("\"good\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Kit", "role": "lead", "left": 0, "cloud": false })
                    .to_string(),
            ),
            _ => (404, "application/json", "{}".to_owned()),
        }));
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.console = lab();
        let (status, said) = use_contract(&state, REPO_CHECKOUT, json!({ "why": "x" })).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("ENDORA_ANSIBLE_NAS_REPO"),
            "{said}"
        );
        let (status, said) = use_contract(
            &state,
            REPO_SUBMIT,
            json!({ "branch": "hq/task-1-x", "why": "x", "bundle": "AAAA" }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "no task: {said}");
        assert!(!state.lab.can_push(), "no key file named");
        let missing = LabKnowledge::new(Docs::new(None, Box::new(ansible_nas::Web)), Vec::new())
            .with_push_key(std::env::temp_dir().join("endora-no-such-key"));
        assert!(!missing.can_push(), "a key file named but absent");
        let present = std::env::temp_dir().join(format!("endora-key-{}", std::process::id()));
        std::fs::write(&present, "not a real key\n").unwrap();
        assert!(
            LabKnowledge::new(Docs::new(None, Box::new(ansible_nas::Web)), Vec::new())
                .with_push_key(present.clone())
                .can_push()
        );
        let _ = std::fs::remove_file(present);
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
                "ansible_nas_doc",
                "repo_checkout",
                "repo_submit"
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
