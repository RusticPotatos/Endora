//! The bridge on the node (ADR 0108): every capability Endora has, and every tool of the tool
//! servers it has installed, offered to HQ's team beside the contracts, under Endora's own
//! policy ([`endora_capabilities::bridge`]): never what is off, Restricted, Endora's
//! administration or the owner's own; Confidential only to a turn on a local model, but for
//! the download queues to the lead's turn on Claude (ADR 0109); acts only
//! for the roles that act; an act that is on runs at once within its limit and is told to the
//! owner; an act that asks waits on the owner's desk, and only the owner's yes runs it, exactly
//! as it was stored.
//!
//! Every call, done, asked or refused, is in Endora's audit log, as the contracts' are.

use super::relay::Turn;
use super::*;
use endora_capabilities::bridge::{
    self, APPROVALS_LAST_MS, Approval, BridgeStore, Bridged, Candidate, Kind, Way,
};

/// The most of a result a turn is given.
const MOST_OF_A_RESULT: usize = 16 * 1024;

/// What the member reads when its call waits for the owner.
pub(super) const ON_THE_DESK: &str =
    "On the owner's desk for approval; you are woken when they decide.";

/// Everything the bridge could offer, built-in skills first, then the tool servers' tools,
/// each as Endora knows it.
pub(super) fn candidates(state: &AppState) -> Vec<Candidate> {
    use endora_capabilities::{CapabilityRunner as _, McpServerRegistry as _, Stance};
    let stances: std::collections::HashMap<String, Stance> = state
        .config
        .stances()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let settings = settings_map(state.config.as_ref());
    let empty = endora_infrastructure::CapabilitySettings::new();
    let mut all: Vec<Candidate> = state
        .capabilities
        .iter()
        .map(|c| {
            let info = c.info();
            let own = settings
                .get(info.borrows_from.unwrap_or(info.id))
                .unwrap_or(&empty);
            Candidate {
                id: info.id.to_owned(),
                description: info.description.to_owned(),
                input_schema: info.input_schema.and_then(|s| serde_json::from_str(s).ok()),
                kind: Kind::of_skill(info.reversibility),
                stance: stances
                    .get(info.id)
                    .copied()
                    .unwrap_or_else(|| endora_capabilities::default_stance(info.reversibility)),
                from_a_server: false,
                ready: info.configured && endora_capabilities::settings_ready(&info, own),
                administration: endora_capabilities::infrastructure::terminal::settings_only(&info),
                third_party: info.third_party,
                owners_devices: false,
            }
        })
        .collect();
    // A tool server's tool reads only when the owner named it the server's reader; a tool the
    // owner has said nothing about asks them each time.
    let readers: std::collections::HashSet<String> = state
        .config
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.reader_tool.trim().is_empty())
        .map(|s| format!("{}.{}", s.name, s.reader_tool.trim()))
        .collect();
    // The server paired with Home Assistant, whose acts switch the owner's own devices: never
    // the team's (ADR 0108).
    let devices =
        endora_capabilities::paired_server(settings.get("home_assistant").unwrap_or(&empty));
    for spec in mcp_snapshot(state).available() {
        let kind = Kind::of_server_tool(readers.contains(&spec.id));
        all.push(Candidate {
            owners_devices: bridge::switches_the_owners_devices(&spec.id, kind, &devices),
            kind,
            stance: stances.get(&spec.id).copied().unwrap_or(Stance::Ask),
            input_schema: spec
                .input_schema
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok()),
            description: spec.description,
            from_a_server: true,
            ready: spec.configured,
            administration: false,
            third_party: spec.third_party,
            id: spec.id,
        });
    }
    all
}

/// The tools the bridge offers now, whoever asks: those [`bridge::bridge`] allows, and none
/// named like a contract.
pub(super) fn catalog(state: &AppState) -> Vec<Bridged> {
    let contracts: Vec<&str> = endora_capabilities::contracts::all()
        .iter()
        .map(|c| c.name)
        .collect();
    candidates(state)
        .iter()
        .filter_map(|c| bridge::bridge(c).ok())
        .filter(|b| !contracts.contains(&b.name.as_str()))
        .collect()
}

/// The tools a turn is offered: what its role may use, and what its model may see, less, when it
/// thinks on the house's own model on the NAS, whatever can end on the owner's desk (ADR 0131,
/// amended).
pub(super) fn offered_to(state: &AppState, turn: &Turn) -> Vec<Bridged> {
    bridge::offered(
        catalog(state),
        bridge::Asker {
            role: &turn.role,
            thinks: turn.thinks(),
        },
    )
}

/// Why a turn on the NAS's own model is refused a tool it names that can end on the owner's
/// desk, as the model reads it, and what to do instead: a small model is a poor judge of what to
/// ask the owner for, so the step is left to a member who thinks on the Mac, by saying what is
/// needed in the thread.
fn kept_from_the_bridge(name: &str, why: &str) -> String {
    format!(
        "{name} is not for this turn, because {why}: say what you need in the task's thread \
         (post), so that the lead can hand the step to a member who thinks on the Mac, rather \
         than leaving it undone"
    )
}

/// Why a cloud turn is refused a Confidential tool, as the model reads it, and what to do
/// instead: a member thinking on the house's own model may take the step, so the lead hands it
/// on rather than leaving it undone.
pub(super) fn confidential(name: &str) -> String {
    format!(
        "{name} gives back Confidential results, which stay with turns on the house's own \
         model: hand this step to a member who thinks on the house's model (create_task), \
         rather than leaving it undone"
    )
}

/// What a bridged call came to: what the model reads, whether it ended the turn, and what the
/// audit log says.
pub(super) struct Called {
    pub(super) text: String,
    pub(super) asked_owner: bool,
    pub(super) said: String,
}

/// Why a bridged call was refused: a status and what the model reads.
pub(super) type Refused = (StatusCode, String);

/// One call of a bridged tool, decided in the policy's order. Blocking: run off the executor.
pub(super) fn call(
    state: &AppState,
    turn: &Turn,
    tool: &Bridged,
    args: &serde_json::Value,
) -> Result<Called, Refused> {
    // Where the turn thinks comes first: the model on the NAS is offered no tool that can end on
    // the owner's desk, so a name it made up or was shown elsewhere is refused if it is one
    // (ADR 0131, amended). Its trusted reads are called as any turn's are.
    if let Some(why) = turn.thinks().keeps_from(tool) {
        return Err((StatusCode::FORBIDDEN, kept_from_the_bridge(&tool.name, why)));
    }
    if !bridge::may_see(tool.tier, &tool.id, &turn.role, turn.cloud) {
        return Err((StatusCode::FORBIDDEN, confidential(&tool.name)));
    }
    let store = BridgeStore::new(state.store.db()).map_err(|_| broken())?;
    let marked = store.marked(turn.run).map_err(|_| broken())?;
    let way = bridge::way(tool, &turn.role, marked).map_err(|why| (StatusCode::FORBIDDEN, why))?;
    let why = one_line(args["why"].as_str().unwrap_or(""));
    // What the member says of the call, and not the tool's arguments: the reason, and which of
    // its task's criteria it serves.
    let serves = serves_said(&args["serves"]);
    let mut given = args.clone();
    if let Some(object) = given.as_object_mut() {
        for own in bridge::THE_BRIDGES_OWN {
            object.remove(own);
        }
    }
    let member = who(turn);
    if way != Way::Read && why.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "say why, in a sentence the owner reads".to_owned(),
        ));
    }
    match way {
        Way::Read => {
            let text = run(state, &tool.id, &given)?;
            if tool.third_party {
                store.mark(turn.run).map_err(|_| broken())?;
            }
            Ok(Called {
                text: marked_if(tool, text),
                asked_owner: false,
                said: format!("read {}{}", tool.name, with(&given)),
            })
        }
        Way::Act => {
            let now = state.clock.now().unix_millis();
            if let Err(why_not) = store.take_act(&tool.id, now).map_err(|_| broken())? {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("{} was not run: {why_not}.", tool.name),
                ));
            }
            let ran = run(state, &tool.id, &given);
            let (told, said) = match &ran {
                Ok(text) => (
                    format!(
                        "{member} ran {}{}: {why}. It said: {}",
                        tool.name,
                        with(&given),
                        short(text)
                    ),
                    format!("ran {}{}: {why}", tool.name, with(&given)),
                ),
                Err((_, failed)) => (
                    format!(
                        "{member} tried to run {}{}: {why}. It failed: {}",
                        tool.name,
                        with(&given),
                        short(failed)
                    ),
                    format!("tried to run {}{}: {why}", tool.name, with(&given)),
                ),
            };
            super::contracts::tell_owner(state, turn.run, &told);
            let text = ran?;
            if tool.third_party {
                store.mark(turn.run).map_err(|_| broken())?;
            }
            Ok(Called {
                text: marked_if(tool, format!("{text}\nThe owner has been told.")),
                asked_owner: false,
                said,
            })
        }
        Way::AskTheOwner => {
            let Some(hq) = state.hq.as_ref() else {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    "HQ is not set up on this node".to_owned(),
                ));
            };
            let now = state.clock.now().unix_millis();
            let id = store
                .add_approval(
                    &tool.id, &tool.name, &given, &why, &member, &turn.role, turn.run, now,
                )
                .map_err(|_| broken())?;
            let text = asked_as(&member, &tool.name, &given, &why);
            // HQ holds the task's criteria: it reads what the call serves, refuses one that
            // serves none with the reason, and shows the owner the one it serves (ADR 0124).
            let item = match hq.post(
                "/v1/team/approval",
                &json!({ "run": turn.run, "text": text, "serves": serves }),
            ) {
                Ok(answer) => answer["item"].as_i64(),
                Err(e) => {
                    let _ = store.settle(id, "waiting", "refused");
                    // A request HQ refuses for what it says is the member's to put right.
                    let status = match e {
                        endora_infrastructure::HqError::Answered { status: 400, .. } => {
                            StatusCode::BAD_REQUEST
                        }
                        _ => StatusCode::CONFLICT,
                    };
                    return Err((status, e.to_string()));
                }
            };
            let Some(item) = item else {
                let _ = store.settle(id, "waiting", "refused");
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "HQ did not say where it put it".to_owned(),
                ));
            };
            store.set_desk(id, item).map_err(|_| broken())?;
            Ok(Called {
                text: ON_THE_DESK.to_owned(),
                asked_owner: true,
                said: format!(
                    "asked the owner to approve {}{}: {why}",
                    tool.name,
                    with(&given)
                ),
            })
        }
    }
}

/// Which of its task's criteria a call says it serves: the member's words on one line, kept as
/// they are written (a criterion's own full stop is part of matching it), or a number as its
/// digits (models write `1` as often as `"1"`). Empty when it says none.
fn serves_said(said: &serde_json::Value) -> String {
    match said {
        serde_json::Value::String(words) => words
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .filter(|c| !c.is_control())
            .take(300)
            .collect(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// What the owner's desk says of an approval, made from what the node stores, so the node can
/// tell its own item from another that took its number.
fn asked_as(member: &str, name: &str, args: &serde_json::Value, why: &str) -> String {
    format!("{member} asks to run {name}{}. Why: {why}.", with(args))
}

/// What the owner's desk says of a stored approval.
pub(super) fn desk_text(approval: &Approval) -> String {
    let args: serde_json::Value = serde_json::from_str(&approval.args).unwrap_or_default();
    asked_as(&approval.member, &approval.name, &args, &approval.why)
}

/// Whether HQ's desk holds this approval, waiting, as the node asked it: an item of that
/// number that is an approval saying what the node stored. HQ's ids begin again if its store
/// is made anew, and a stored approval must never run on another item's yes.
fn on_the_desk(hq: &endora_infrastructure::HqClient, approval: &Approval) -> bool {
    let said = desk_text(approval);
    let Ok(snapshot) = hq.get("/v1/snapshot") else {
        return false;
    };
    snapshot["desk"].as_array().into_iter().flatten().any(|d| {
        d["id"] == approval.desk.unwrap_or(0)
            && d["kind"] == "approval"
            && d["text"] == said.as_str()
    })
}

/// Runs a capability or a tool server's tool through Endora's own path: a built-in skill as
/// the explicit invoke runs it, a tool server's tool through the connected servers. What it
/// said comes back as text, held to [`MOST_OF_A_RESULT`].
fn run(state: &AppState, id: &str, args: &serde_json::Value) -> Result<String, Refused> {
    use endora_capabilities::CapabilityRunner as _;
    if let Some(cap) = state.capabilities.iter().find(|c| c.info().id == id) {
        let value =
            invoke_now(state.config.as_ref(), cap.as_ref(), args.clone()).map_err(|e| match e {
                CapabilityError::BadInput(m) => (StatusCode::BAD_REQUEST, m),
                CapabilityError::Unavailable(m) => (StatusCode::BAD_GATEWAY, m),
            })?;
        return Ok(held(&cap.summarize(&value)));
    }
    mcp_snapshot(state)
        .run(id, &args.to_string())
        .map(|text| held(&text))
        .map_err(|why| (StatusCode::BAD_GATEWAY, why))
}

fn held(text: &str) -> String {
    text.chars().take(MOST_OF_A_RESULT).collect()
}

/// A stranger's words keep their mark (ADR 0064).
fn marked_if(tool: &Bridged, text: String) -> String {
    if tool.third_party {
        return format!("{}{text}", endora_application::egress::STRANGER_MARK);
    }
    text
}

/// Arguments in plain words after a tool's name: " with item: 102", or nothing.
fn with(args: &serde_json::Value) -> String {
    let words = bridge::plain_words(args);
    if words.is_empty() {
        return String::new();
    }
    let said: Vec<String> = words.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    format!(" with {}", said.join(", "))
}

fn short(text: &str) -> String {
    one_line(text).chars().take(200).collect()
}

/// A line as the owner reads it: one line, short, no control characters.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(300)
        .collect::<String>()
        .trim_end_matches('.')
        .to_owned()
}

fn who(turn: &Turn) -> String {
    if turn.member.is_empty() {
        return "A team member".to_owned();
    }
    turn.member.clone()
}

fn broken() -> Refused {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "the bridge's records could not be read".to_owned(),
    )
}

/// The approval behind a desk item, if one is.
pub(super) fn approval_at(state: &AppState, desk: i64) -> Option<Approval> {
    BridgeStore::new(state.store.db())
        .ok()?
        .by_desk(desk)
        .ok()
        .flatten()
}

/// What the HQ screen shows on an approval's card: the tool, its arguments in plain words,
/// the member's reason and who asked, from what the node stored, which is what will run.
pub(super) fn shown(approval: &Approval) -> serde_json::Value {
    let args: serde_json::Value = serde_json::from_str(&approval.args).unwrap_or_default();
    let words: Vec<serde_json::Value> = bridge::plain_words(&args)
        .into_iter()
        .map(|(k, v)| json!([k, v]))
        .collect();
    json!({ "tool": approval.name, "args": words, "why": approval.why,
            "member": approval.member })
}

/// The owner decides an approval: `approve` runs exactly what was stored, once, through
/// Endora's own path, and tells HQ what came of it; `decline` runs nothing. Blocking.
pub(super) fn decide(
    state: &AppState,
    approval: &Approval,
    decision: &str,
) -> Result<serde_json::Value, Refused> {
    let Some(hq) = state.hq.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "HQ is not set up on this node".to_owned(),
        ));
    };
    let store = BridgeStore::new(state.store.db()).map_err(|_| broken())?;
    let desk = approval.desk.unwrap_or(0);
    let args: serde_json::Value = serde_json::from_str(&approval.args).unwrap_or_default();
    let what = format!("{}{}", approval.name, with(&args));
    if approval.status != "waiting" {
        return Err((
            StatusCode::CONFLICT,
            format!("that approval was already {}", approval.status),
        ));
    }
    if !matches!(decision, "approve" | "decline") {
        return Err((
            StatusCode::BAD_REQUEST,
            "an approval is approved or declined".to_owned(),
        ));
    }
    if state.clock.now().unix_millis() - approval.at > APPROVALS_LAST_MS {
        let _ = store.settle(approval.id, "waiting", "expired");
        return Err((
            StatusCode::CONFLICT,
            "it waited more than a day, so it will not run".to_owned(),
        ));
    }
    if !on_the_desk(hq, approval) {
        let _ = store.settle(approval.id, "waiting", "refused");
        return Err((
            StatusCode::CONFLICT,
            "HQ's desk does not hold that approval any more, so nothing was run".to_owned(),
        ));
    }
    // HQ first: it holds the desk, decides each item once, and refuses one it has expired.
    hq.post(
        &format!("/v1/desk/{desk}"),
        &json!({ "decision": decision }),
    )
    .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    if decision == "decline" {
        store
            .settle(approval.id, "waiting", "declined")
            .map_err(|_| broken())?;
        record(
            state,
            &format!(
                "you declined {}'s {what} (run {}); it was not run",
                approval.member, approval.run
            ),
        );
        return Ok(json!({ "decided": true, "ran": false }));
    }
    if !store
        .settle(approval.id, "waiting", "approved")
        .map_err(|_| broken())?
    {
        return Err((
            StatusCode::CONFLICT,
            "that approval was already decided".to_owned(),
        ));
    }
    // The stance and the member's role, again, now: what is off now does not run.
    let now_offered = catalog(state).into_iter().find(|b| b.id == approval.tool);
    let ran = match now_offered {
        None => Err(format!(
            "{} is switched off or no longer offered, so it was not run",
            approval.name
        )),
        Some(tool) if !bridge::may(&approval.role, tool.kind) => Err(format!(
            "a {} may not run {}, so it was not run",
            approval.role, approval.name
        )),
        Some(tool) => run(state, &tool.id, &args)
            .map(|text| marked_if(&tool, text))
            .map_err(|(_, why)| format!("it failed: {why}")),
    };
    let (outcome, said) = match &ran {
        Ok(text) => (
            format!("The owner approved {what}, and it ran. It said: {text}"),
            format!(
                "you approved {}'s {what} (run {}), and it ran",
                approval.member, approval.run
            ),
        ),
        Err(why) => (
            format!("The owner approved {what}, but {why}."),
            format!(
                "you approved {}'s {what} (run {}), but {why}",
                approval.member, approval.run
            ),
        ),
    };
    record(state, &said);
    if let Err(e) = hq.post(
        &format!("/v1/team/approval/{desk}/outcome"),
        &json!({ "text": outcome }),
    ) {
        eprintln!("bridge: could not tell HQ what came of approval {desk}: {e}");
    }
    Ok(json!({ "decided": true, "ran": ran.is_ok() }))
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{Seen, fake, node};
    use super::*;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    /// HQ knowing one turn (token `good`, run 7, Linus's, in `role`, on a cloud model when
    /// `cloud`, else on the owner's Mac, which is offered the bridge), taking approvals as desk
    /// item 40.
    fn hq(role: &'static str, cloud: bool) -> (String, Seen) {
        hq_thinking(role, cloud, if cloud { "claude-haiku-5-5" } else { "mac" })
    }

    /// The same, the turn thinking on `brain` as HQ names it: `local` is the house's own model
    /// on the NAS (ADR 0131), which is kept from the tools that can end on the owner's desk. An
    /// approval is taken only when it says which criterion it serves, as HQ takes one (ADR 0124).
    fn hq_thinking(role: &'static str, cloud: bool, brain: &'static str) -> (String, Seen) {
        fake(Arc::new(move |path, body| {
            let reply = match path {
                "/v1/team/turn" if body.contains("\"good\"") => {
                    json!({ "run": 7, "member": "Linus", "role": role, "left": 40_000,
                            "cloud": cloud, "brain": brain })
                }
                "/v1/team/turn" => {
                    return (
                        404,
                        "application/json",
                        r#"{"error":"no such turn"}"#.to_owned(),
                    );
                }
                "/v1/team/approval"
                    if serde_json::from_str::<serde_json::Value>(body)
                        .is_ok_and(|asked| asked["serves"] == "1") =>
                {
                    json!({ "item": 40 })
                }
                "/v1/team/approval" => {
                    return (
                        400,
                        "application/json",
                        json!({ "error": "which of #3's criteria does asking the owner serve? \
                            Give serves (its number or its words): 1. The queue is clear. A \
                            request that serves none of them never reaches the owner's desk" })
                        .to_string(),
                    );
                }
                "/v1/snapshot" => {
                    let asked = |id: i64, why: &str| {
                        json!({ "id": id, "kind": "approval", "task": 3,
                        "text": format!("Linus asks to run sonarr_remove_and_search with item: 102. Why: {why}.") })
                    };
                    json!({ "desk": [
                        asked(40, "it is stuck importing"),
                        { "id": 41, "kind": "notice", "task": 3, "text": "Done." },
                        asked(50, "stuck"), asked(51, "stuck"), asked(52, "stuck"),
                        // Item 53 is a goal: HQ's store was made anew, and its ids began again.
                        { "id": 53, "kind": "goal", "text": "A goal: Tidy the lab" }] })
                }
                _ => json!({ "ok": true }),
            };
            (200, "application/json", reply.to_string())
        }))
    }

    /// Sonarr: a queue with item 102 stuck, its command and its delete.
    fn sonarr() -> (String, Seen) {
        fake(Arc::new(|path, _| {
            let reply = if path.starts_with("/api/v3/queue?") {
                json!({ "totalRecords": 1, "records": [
                    { "id": 102, "title": "Some.Show.S01E02", "size": 10.0, "sizeleft": 0.0,
                      "status": "completed", "trackedDownloadStatus": "warning",
                      "trackedDownloadState": "importBlocked",
                      "statusMessages": [{ "title": "Some.Show.S01E02.mkv", "messages": ["No files found are eligible for import"] }] }] })
            } else {
                json!({ "id": 1 })
            };
            (200, "application/json", reply.to_string())
        }))
    }

    /// The node with HQ as `hq` and Sonarr set up at `sonarr`.
    fn bridged(hq_url: &str, sonarr_url: &str) -> AppState {
        let state = node(hq_url, "http://127.0.0.1:9");
        state
            .config
            .set_setting("homelab-sonarr", "origin", sonarr_url)
            .unwrap();
        state
            .config
            .set_setting("homelab-sonarr", "api_key", "fictional-key")
            .unwrap();
        state
    }

    async fn send(
        state: &AppState,
        method: &str,
        uri: &str,
        body: &str,
    ) -> (StatusCode, serde_json::Value) {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer good")
            .body(axum::body::Body::from(body.to_owned()))
            .unwrap();
        let res = team_relay(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    async fn tools(state: &AppState) -> Vec<(String, String)> {
        let (_, listed) = send(state, "GET", "/team-relay/contracts", "").await;
        listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                (
                    t["name"].as_str().unwrap().to_owned(),
                    t["kind"].as_str().unwrap_or("").to_owned(),
                )
            })
            .collect()
    }

    fn paths(seen: &Seen) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .map(|(p, _, _)| p.clone())
            .collect()
    }

    fn sent_to(seen: &Seen, path: &str) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == path)
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect()
    }

    fn audit(state: &AppState) -> Vec<String> {
        endora_application::AuditLog::recent(state.audit.as_ref(), 50)
            .unwrap()
            .iter()
            .map(|r| r.summary().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn the_catalog_is_the_contracts_and_every_eligible_capability_with_its_kind() {
        let (hq_url, _) = hq("developer", false);
        let (sonarr_url, _) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let listed = tools(&state).await;
        let names: Vec<&str> = listed.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            &names[..3],
            ["homelab_status", "container_logs", "restart_container"],
            "the contracts first"
        );
        for (name, kind) in [
            ("sonarr_queue", "read"),
            ("sonarr_retry_import", "act"),
            ("sonarr_remove_and_search", "act"),
            ("weather", "read"),
        ] {
            assert!(
                listed.contains(&(name.to_owned(), kind.to_owned())),
                "{name} as {kind}: {listed:?}"
            );
        }
        for never in [
            "mail",
            "own_activity",
            "read_page",
            "brief",
            "image_review",
            "location",
            "terminal-portainer",
            "homelab-sonarr",
            "network-pihole",
            "radarr_queue",
        ] {
            assert!(
                !names.contains(&never),
                "{never} must not be offered: {names:?}"
            );
        }
        // Switched off by the owner: gone.
        state
            .config
            .set_stance("sonarr_retry_import", endora_capabilities::Stance::Off)
            .unwrap();
        assert!(
            !tools(&state)
                .await
                .iter()
                .any(|(n, _)| n == "sonarr_retry_import"),
            "off is never offered"
        );
    }

    #[tokio::test]
    async fn a_reviewer_reads_only_and_a_cloud_turn_sees_nothing_confidential() {
        let (hq_url, _) = hq("reviewer", false);
        let (sonarr_url, _) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let listed = tools(&state).await;
        assert!(listed.iter().any(|(n, _)| n == "sonarr_queue"));
        assert!(
            listed
                .iter()
                .all(|(n, k)| k != "act" || n == "restart_container"),
            "a reviewer is offered no bridged act: {listed:?}"
        );
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_retry_import",
            r#"{"why":"stuck"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");

        // A turn on a cloud model (as HQ will say one is): Confidential stays home.
        let (hq_url, _) = hq("developer", true);
        let state = bridged(&hq_url, &sonarr_url);
        let listed = tools(&state).await;
        assert!(
            !listed.iter().any(|(n, _)| n == "sonarr_queue"),
            "{listed:?}"
        );
        assert!(
            listed.iter().any(|(n, _)| n == "weather"),
            "Public still is"
        );
        let (status, said) = send(&state, "POST", "/team-relay/contracts/sonarr_queue", "{}").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains("Confidential"),
            "{said}"
        );
        let turn = Turn {
            run: 7,
            member: "Linus".to_owned(),
            role: "developer".to_owned(),
            left: 0,
            task: None,
            token: "good".to_owned(),
            cloud: true,
            brain: "claude-haiku-5-5".to_owned(),
            claude_code: false,
            wing: None,
        };
        let tool = catalog(&state)
            .into_iter()
            .find(|b| b.id == "sonarr_queue")
            .unwrap();
        let refused = call(&state, &turn, &tool, &json!({})).err().unwrap();
        assert_eq!(
            refused.0,
            StatusCode::FORBIDDEN,
            "the check point itself refuses"
        );
    }

    #[test]
    fn a_cloud_turns_refusal_says_to_hand_the_step_on() {
        let said = super::confidential("sonarr_retry_import");
        assert!(said.contains("hand this step"), "{said}");
        assert!(said.contains("create_task"), "{said}");
    }

    #[tokio::test]
    async fn the_cloud_lead_reads_the_download_queues_and_nothing_else_confidential() {
        // The owner's decision of 2026-10-17 (ADR 0109).
        let (sonarr_url, _) = sonarr();
        let (hq_url, _) = hq("lead", true);
        let state = bridged(&hq_url, &sonarr_url);
        let listed = tools(&state).await;
        assert!(
            listed.iter().any(|(n, _)| n == "sonarr_queue"),
            "{listed:?}"
        );
        assert!(
            !listed.iter().any(|(n, _)| n == "sonarr_retry_import"),
            "a Confidential act is not among what the owner allowed: {listed:?}"
        );
        let (status, said) = send(&state, "POST", "/team-relay/contracts/sonarr_queue", "{}").await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_retry_import",
            r#"{"why":"stuck"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
        assert!(
            said["error"].as_str().unwrap().contains("Confidential"),
            "{said}"
        );

        let (hq_url, _) = hq("reviewer", true);
        let state = bridged(&hq_url, &sonarr_url);
        assert!(
            !tools(&state).await.iter().any(|(n, _)| n == "sonarr_queue"),
            "a cloud reviewer reads no queue"
        );
        let (status, _) = send(&state, "POST", "/team-relay/contracts/sonarr_queue", "{}").await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // The CEO on Claude reads the queue as the cloud lead does, and acts on nothing
        // (ADR 0120, the owner's word of 2026-10-18).
        let (hq_url, _) = hq("ceo", true);
        let state = bridged(&hq_url, &sonarr_url);
        let listed = tools(&state).await;
        assert!(
            listed.iter().any(|(n, _)| n == "sonarr_queue"),
            "the CEO reads the queue: {listed:?}"
        );
        assert!(
            !listed.iter().any(|(n, _)| n == "sonarr_retry_import"),
            "and is offered no act: {listed:?}"
        );
        let (status, said) = send(&state, "POST", "/team-relay/contracts/sonarr_queue", "{}").await;
        assert_eq!(status, StatusCode::OK, "{said}");
    }

    #[tokio::test]
    async fn a_read_runs_through_endoras_own_path_and_keeps_a_strangers_mark() {
        let (hq_url, _) = hq("developer", false);
        let (sonarr_url, to_sonarr) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let (status, said) = send(&state, "POST", "/team-relay/contracts/sonarr_queue", "{}").await;
        assert_eq!(status, StatusCode::OK, "{said}");
        let text = said["text"].as_str().unwrap();
        assert!(text.starts_with("[from outside] "), "{text}");
        assert!(
            text.contains("No files found are eligible for import"),
            "{text}"
        );
        assert!(paths(&to_sonarr)[0].starts_with("/api/v3/queue?"));
        assert!(
            audit(&state)
                .iter()
                .any(|a| a.ends_with("Linus (run 7) read sonarr_queue")),
            "{:?}",
            audit(&state)
        );
    }

    #[tokio::test]
    async fn an_act_that_is_on_runs_within_its_limit_and_the_owner_is_told() {
        let (hq_url, to_hq) = hq("developer", false);
        let (sonarr_url, to_sonarr) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let retry = |why: &str| json!({ "why": why }).to_string();
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_retry_import",
            &retry(""),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "an act says why: {said}");
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_retry_import",
            &retry("two downloads are stuck"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert!(
            said["text"]
                .as_str()
                .unwrap()
                .contains("The owner has been told")
        );
        assert_eq!(paths(&to_sonarr), ["/api/v3/command"]);
        let notices = sent_to(&to_hq, "/v1/team/notice");
        assert_eq!(notices.len(), 1);
        let notice = notices[0]["text"].as_str().unwrap();
        assert!(
            notice.starts_with("Linus ran sonarr_retry_import: two downloads are stuck. It said: Sonarr is checking"),
            "{notice}"
        );
        for _ in 0..2 {
            let (status, _) = send(
                &state,
                "POST",
                "/team-relay/contracts/sonarr_retry_import",
                &retry("still stuck"),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_retry_import",
            &retry("still stuck"),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{said}");
        assert_eq!(paths(&to_sonarr).len(), 3, "three an hour, no more");
        let audit = audit(&state);
        assert!(
            audit
                .iter()
                .any(|a| a
                    .ends_with("Linus (run 7) ran sonarr_retry_import: two downloads are stuck")),
            "{audit:?}"
        );
        assert!(
            audit
                .iter()
                .any(|a| a.contains("was refused sonarr_retry_import")),
            "{audit:?}"
        );
    }

    #[tokio::test]
    async fn an_act_that_asks_waits_on_the_desk_and_runs_exactly_what_was_approved() {
        let (hq_url, to_hq) = hq("developer", false);
        let (sonarr_url, to_sonarr) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_remove_and_search",
            &json!({ "item": 102, "why": "it is stuck importing", "serves": "1" }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert_eq!(said["text"], ON_THE_DESK);
        assert_eq!(said["asked_owner"], true, "the worker ends its turn");
        assert!(to_sonarr.lock().unwrap().is_empty(), "nothing ran");
        // HQ is told which criterion it serves, and the desk's words say neither it nor why as
        // an argument of the call.
        let asked = sent_to(&to_hq, "/v1/team/approval");
        assert_eq!(
            asked,
            [
                json!({ "run": 7, "serves": "1", "text": "Linus asks to run sonarr_remove_and_search with item: 102. Why: it is stuck importing." })
            ]
        );

        // The owner's screen shows what will run, from what the node stored.
        let snap = hq_snapshot(State(state.clone())).await.unwrap().0;
        assert_eq!(
            snap["desk"][0]["approval"],
            json!({ "tool": "sonarr_remove_and_search", "args": [["item", "102"]],
                    "why": "it is stuck importing", "member": "Linus" })
        );
        assert!(snap["desk"][1]["approval"].is_null(), "a notice is not one");

        // Approved: HQ first, then exactly the stored call, then what came of it.
        let decided = hq_decide(
            State(state.clone()),
            Path(40),
            Json(HqDecision {
                decision: "approve".to_owned(),
                answer: String::new(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(decided.0["ran"], true);
        let to_sonarr_now = paths(&to_sonarr);
        assert_eq!(
            to_sonarr_now.last().unwrap(),
            "/api/v3/queue/102?removeFromClient=true&blocklist=true&skipRedownload=false"
        );
        assert_eq!(
            sent_to(&to_hq, "/v1/desk/40"),
            [json!({ "decision": "approve" })]
        );
        let outcome = sent_to(&to_hq, "/v1/team/approval/40/outcome");
        let outcome = outcome[0]["text"].as_str().unwrap();
        assert!(
            outcome.starts_with("The owner approved sonarr_remove_and_search with item: 102, and it ran. It said: [from outside] Removed Some.Show.S01E02"),
            "{outcome}"
        );
        // Once.
        let again = hq_decide(
            State(state.clone()),
            Path(40),
            Json(HqDecision {
                decision: "approve".to_owned(),
                answer: String::new(),
            }),
        )
        .await;
        assert!(again.is_err());
        assert_eq!(paths(&to_sonarr).len(), to_sonarr_now.len(), "run once");
        let audit = audit(&state);
        assert!(audit.iter().any(|a| a.ends_with("Linus (run 7) asked the owner to approve sonarr_remove_and_search with item: 102: it is stuck importing")), "{audit:?}");
        assert!(
            audit.iter().any(|a| a.ends_with(
                "you approved Linus's sonarr_remove_and_search with item: 102 (run 7), and it ran"
            )),
            "{audit:?}"
        );
    }

    #[tokio::test]
    async fn declined_switched_off_or_a_day_old_an_approval_never_runs() {
        let (hq_url, to_hq) = hq("developer", false);
        let (sonarr_url, to_sonarr) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let store = BridgeStore::new(state.store.db()).unwrap();
        let now = state.clock.now().unix_millis();
        let stored = |desk: i64, at: i64| {
            let id = store
                .add_approval(
                    "sonarr_remove_and_search",
                    "sonarr_remove_and_search",
                    &json!({ "item": 102 }),
                    "stuck",
                    "Linus",
                    "developer",
                    7,
                    at,
                )
                .unwrap();
            store.set_desk(id, desk).unwrap();
        };
        let decide_it = |desk: i64, decision: &str| {
            let state = state.clone();
            let decision = decision.to_owned();
            async move {
                hq_decide(
                    State(state),
                    Path(desk),
                    Json(HqDecision {
                        decision,
                        answer: String::new(),
                    }),
                )
                .await
            }
        };
        stored(50, now);
        let declined = decide_it(50, "decline").await.unwrap();
        assert_eq!(declined.0["ran"], false);
        assert_eq!(
            sent_to(&to_hq, "/v1/desk/50"),
            [json!({ "decision": "decline" })]
        );

        stored(51, now - APPROVALS_LAST_MS - 1);
        assert!(decide_it(51, "approve").await.is_err(), "a day old");
        assert!(
            sent_to(&to_hq, "/v1/desk/51").is_empty(),
            "not even asked of HQ"
        );

        stored(52, now);
        state
            .config
            .set_stance("sonarr_remove_and_search", endora_capabilities::Stance::Off)
            .unwrap();
        let approved = decide_it(52, "approve").await.unwrap();
        assert_eq!(approved.0["ran"], false);
        let outcome = sent_to(&to_hq, "/v1/team/approval/52/outcome");
        assert!(
            outcome[0]["text"]
                .as_str()
                .unwrap()
                .contains("switched off"),
            "{outcome:?}"
        );
        // A stored approval whose desk item is now something else is never run from it.
        stored(53, now);
        assert!(decide_it(53, "approve").await.is_err());
        assert!(
            sent_to(&to_hq, "/v1/desk/53").is_empty(),
            "the goal is not approved either"
        );
        assert!(to_sonarr.lock().unwrap().is_empty(), "nothing ran at all");
    }

    #[tokio::test]
    async fn after_a_strangers_words_an_act_given_arguments_asks_the_owner() {
        let (hq_url, to_hq) = hq("developer", false);
        let (sonarr_url, _) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        // The owner lets the removal run on its own; then the turn reads the queue's titles.
        state
            .config
            .set_stance(
                "sonarr_remove_and_search",
                endora_capabilities::Stance::Auto,
            )
            .unwrap();
        send(&state, "POST", "/team-relay/contracts/sonarr_queue", "{}").await;
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_remove_and_search",
            &json!({ "item": 102, "why": "stuck", "serves": 1 }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert_eq!(said["asked_owner"], true, "{said}");
        let asked = sent_to(&to_hq, "/v1/team/approval");
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0]["serves"], "1", "a number is as good as its digits");
    }

    /// Live: a developer on the house's model on the NAS asked the owner to approve a web map
    /// search, with coordinates it had made up, "to find the NAS backup location", and the
    /// request reached the owner's desk. A turn on that model is kept from what can end there; the
    /// reads the owner trusts stay, the Ops lead's download queues among them (ADR 0131,
    /// amended).
    #[tokio::test]
    async fn a_turn_on_the_nas_model_keeps_its_trusted_reads_and_is_refused_what_can_end_on_the_desk()
     {
        let (hq_url, to_hq) = hq_thinking("lead", false, "local");
        let (sonarr_url, to_sonarr) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let listed = tools(&state).await;
        let names: Vec<&str> = listed.iter().map(|(n, _)| n.as_str()).collect();
        // The lab's contracts are not the bridge, and stay.
        for contract in endora_capabilities::contracts::all()
            .iter()
            .filter(|c| c.offered_to("lead", false))
        {
            assert!(
                names.contains(&contract.name),
                "{}: {names:?}",
                contract.name
            );
        }
        // The trusted reads, and the act that runs on its own and takes nothing, stay.
        for kept in ["sonarr_queue", "weather", "sonarr_retry_import"] {
            assert!(names.contains(&kept), "{kept}: {names:?}");
        }
        // What asks the owner does not: the download removal starts out asking.
        assert!(!names.contains(&"sonarr_remove_and_search"), "{names:?}");
        let (status, read) = send(&state, "POST", "/team-relay/contracts/sonarr_queue", "{}").await;
        assert_eq!(status, StatusCode::OK, "the lead reads the queue: {read}");
        assert!(paths(&to_sonarr)[0].starts_with("/api/v3/queue?"));
        let (status, ran) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_retry_import",
            r#"{"why":"two downloads are stuck"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{ran}");
        let before = paths(&to_sonarr).len();
        let ask = r#"{"item":102,"why":"to find the NAS backup location","serves":"1"}"#;
        // Naming it anyway is refused, and so is the same act once the owner lets it run on its
        // own: it takes arguments, so it asks once a turn has read a stranger's words (0108),
        // and the queue just read was one.
        for stance in [None, Some(endora_capabilities::Stance::Auto)] {
            if let Some(stance) = stance {
                state
                    .config
                    .set_stance("sonarr_remove_and_search", stance)
                    .unwrap();
                assert!(
                    !tools(&state)
                        .await
                        .iter()
                        .any(|(n, _)| n == "sonarr_remove_and_search"),
                    "an act that asks after a stranger's words is kept too"
                );
            }
            let (status, said) = send(
                &state,
                "POST",
                "/team-relay/contracts/sonarr_remove_and_search",
                ask,
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
            assert!(
                said["error"].as_str().unwrap().contains("owner's desk"),
                "{said}"
            );
        }
        // A name that is no tool at all is still not found, as ever.
        let (status, _) = send(&state, "POST", "/team-relay/contracts/no_such_tool", "{}").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(paths(&to_sonarr).len(), before, "the refused ran nothing");
        assert!(
            sent_to(&to_hq, "/v1/team/approval").is_empty(),
            "nothing reached HQ, so nothing reached the desk"
        );
        // The refusals are in the audit log and told to HQ as receipts, like any refused call.
        assert!(
            audit(&state)
                .iter()
                .any(|a| a.contains("was refused sonarr_remove_and_search")),
            "{:?}",
            audit(&state)
        );
        assert!(
            !sent_to(&to_hq, "/v1/team/receipt").is_empty(),
            "a refused call is a receipt too"
        );
        // A turn HQ describes without a brain is not known to be a stronger model: the same.
        let (hq_url, _) = fake(Arc::new(|path, body| {
            let reply = match path {
                "/v1/team/turn" if body.contains("\"good\"") => json!({ "run": 7,
                    "member": "Linus", "role": "lead", "left": 40_000, "cloud": false }),
                _ => json!({ "ok": true }),
            };
            (200, "application/json", reply.to_string())
        }));
        let state = bridged(&hq_url, &sonarr_url);
        let unknown = tools(&state).await;
        assert!(
            unknown.iter().any(|(n, _)| n == "sonarr_queue")
                && !unknown.iter().any(|(n, _)| n == "sonarr_remove_and_search"),
            "kept from what asks, given what it trusts: {unknown:?}"
        );
    }

    /// An act that asks says which of its task's criteria it serves, or HQ refuses it to the
    /// member with the reason, and it is never on the desk (ADR 0124, amended).
    #[tokio::test]
    async fn an_act_that_asks_without_saying_what_it_serves_is_refused_and_never_reaches_the_desk()
    {
        let (hq_url, to_hq) = hq("developer", false);
        let (sonarr_url, to_sonarr) = sonarr();
        let state = bridged(&hq_url, &sonarr_url);
        let ask = |serves: serde_json::Value| {
            let mut call = json!({ "item": 102, "why": "it is stuck importing" });
            if !serves.is_null() {
                call["serves"] = serves;
            }
            call.to_string()
        };
        for serves in [
            serde_json::Value::Null,
            json!(""),
            json!("the moon base"),
            json!(7),
        ] {
            let (status, said) = send(
                &state,
                "POST",
                "/team-relay/contracts/sonarr_remove_and_search",
                &ask(serves.clone()),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{serves}: {said}");
            assert!(
                said["error"]
                    .as_str()
                    .unwrap()
                    .starts_with("which of #3's criteria does asking the owner serve?"),
                "the member reads HQ's reason: {said}"
            );
            assert!(said.get("asked_owner").is_none(), "{said}");
        }
        assert!(
            BridgeStore::new(state.store.db())
                .unwrap()
                .waiting()
                .unwrap()
                .is_empty(),
            "nothing is waiting for the owner's yes"
        );
        assert!(to_sonarr.lock().unwrap().is_empty(), "nothing ran");
        let snap = hq_snapshot(State(state.clone())).await.unwrap().0;
        assert!(
            snap["desk"]
                .as_array()
                .unwrap()
                .iter()
                .all(|d| d["approval"].is_null()),
            "no approval card was made from them"
        );
        // Said right, the same request is asked, and what HQ is told of it is that.
        let (status, said) = send(
            &state,
            "POST",
            "/team-relay/contracts/sonarr_remove_and_search",
            &ask(json!("1")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert_eq!(said["asked_owner"], true);
        assert_eq!(sent_to(&to_hq, "/v1/team/approval").len(), 5);
    }
}
