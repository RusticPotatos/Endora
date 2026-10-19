//! The team's contracts (ADR 0106): the narrow abilities Endora offers HQ's workers on the
//! home lab, and the deterministic policy that decides each use. A worker proposes; this
//! decides. Reads are free; a safe fix runs on its own, within limits kept in the node's own
//! database, and is told to the owner; anything riskier is not a contract at all yet.
//!
//! What a contract is and what it may touch lives here; the node serves them to a turn and
//! reaches the home lab through its own console adapter.

use endora_kernel::RepositoryError;
use endora_persistence::Db;
use rusqlite::params;
use serde_json::{Value, json};

/// An hour, in milliseconds.
pub const HOUR_MS: i64 = 60 * 60 * 1000;
/// The most restarts the whole team may make in an hour.
pub const TEAM_RESTARTS_AN_HOUR: usize = 3;
/// The most of a container's log a turn is given, after scrubbing.
pub const MOST_OF_A_LOG: usize = 16 * 1024;
/// The most lines of a container's log a turn is given: the newest.
pub const LOG_LINES: usize = 100;

/// What kind of ability a contract is (ADR 0106).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Looks, and changes nothing: free.
    Read,
    /// A safe fix that runs on its own, within limits, and lands on the owner's desk.
    AutomaticAct,
}

impl Kind {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::AutomaticAct => "automatic_act",
        }
    }
}

/// One contract, as a turn is offered it.
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    /// Its name, which is the tool's.
    pub name: &'static str,
    /// What the model is told it does, and its limits.
    pub description: &'static str,
    /// What kind it is.
    pub kind: Kind,
    /// The data tier of what it gives back (ADR 0105): `internal` or `confidential`.
    pub tier: &'static str,
    /// Its arguments, as a JSON schema.
    pub input: Value,
}

impl Contract {
    /// Whether a turn of a member in `role` may be given what this returns: a Confidential
    /// contract (a log) by the bridge's rule, to a turn on the house's model or to the lead's on
    /// a cloud model, as the owner decided (ADR 0109).
    #[must_use]
    pub fn may_be_seen_by(&self, role: &str, on_a_cloud_model: bool) -> bool {
        let tier = if self.tier == "confidential" {
            endora_kernel::Tier::Confidential
        } else {
            endora_kernel::Tier::Internal
        };
        crate::bridge::may_see(tier, self.name, role, on_a_cloud_model)
    }

    /// Whether a turn of a member in `role` is offered this contract: what it may see, and an
    /// act only for a member who works on the house. The CEO reads what the lead reads and
    /// directs; it takes no step on the house itself (ADR 0111, 0120). The wing's git contracts (ADR 0115) go to
    /// a lead or a developer, whatever its brain: the fork is public.
    #[must_use]
    pub fn offered_to(&self, role: &str, on_a_cloud_model: bool) -> bool {
        if self.name == REPO_CHECKOUT || self.name == REPO_SUBMIT {
            return matches!(role, "lead" | "developer");
        }
        if self.kind == Kind::AutomaticAct && role == "ceo" {
            return false;
        }
        self.may_be_seen_by(role, on_a_cloud_model)
    }

    /// As an MCP tool: name, description and input schema.
    #[must_use]
    pub fn as_tool(&self) -> Value {
        json!({ "name": self.name, "description": self.description, "inputSchema": self.input })
    }
}

/// The contract that reads the home lab's state.
pub const HOMELAB_STATUS: &str = "homelab_status";
/// The contract that reads a container's log.
pub const CONTAINER_LOGS: &str = "container_logs";
/// The contract that restarts a container.
pub const RESTART_CONTAINER: &str = "restart_container";
/// The contract that reads how the home lab is built: who manages each container and what
/// depends on what (ADR 0112).
pub const HOMELAB_MAP: &str = "homelab_map";
/// The contract that reads the lab's ansible-nas docs (ADR 0112).
pub const ANSIBLE_NAS_DOC: &str = "ansible_nas_doc";
/// The contract that hands a sandbox a bundle of the fork to work on (ADR 0115).
pub const REPO_CHECKOUT: &str = "repo_checkout";
/// The contract that takes a sandbox's branch as a bundle (ADR 0115).
pub const REPO_SUBMIT: &str = "repo_submit";

/// Every contract Endora offers the team, in the order a turn is offered them.
#[must_use]
pub fn all() -> Vec<Contract> {
    let container = json!({ "type": "string", "description": "The container's name, as homelab_status lists it, e.g. sonarr." });
    vec![
        Contract {
            name: HOMELAB_STATUS,
            description: "Read the home lab's state: each service (docker, sonarr, radarr) and its items. For each container its name, state, status text and health. Marked stale when Endora has not seen it for a minute. Free.",
            kind: Kind::Read,
            tier: "internal",
            input: json!({ "type": "object", "properties": {} }),
        },
        Contract {
            name: CONTAINER_LOGS,
            description: "Read the newest 100 lines of one container's log, at most 16 KB, with secrets replaced by [redacted]. Not Endora's own containers. Free.",
            kind: Kind::Read,
            tier: "confidential",
            input: json!({ "type": "object", "properties": { "container": container }, "required": ["container"] }),
        },
        Contract {
            name: RESTART_CONTAINER,
            description: "Restart one container, and read back its state. A safe fix: it runs at once, and the owner is told, with your reason. At most one restart of a container an hour, and three in all an hour across the team. Never Endora's own containers, and never one a restart will not fix: already restarting, out of memory, or crash-looping (ask the owner instead). Say why in a sentence.",
            kind: Kind::AutomaticAct,
            tier: "internal",
            input: json!({ "type": "object", "properties": {
                "container": container,
                "why": { "type": "string", "description": "Why it needs a restart, in a sentence the owner reads." },
            }, "required": ["container", "why"] }),
        },
        Contract {
            name: HOMELAB_MAP,
            description: "Read how the home lab is built, from Docker: for each container its image, state, restart policy, who manages it (an ansible-nas role, Endora, or unknown), what it depends on and what depends on it, e.g. \"gluetun → deluge, nzbget, prowlarr (run in its network)\": those stop when gluetun restarts and come back on their own. Names and wiring only. Read it before treating a stop as a fault. Free.",
            kind: Kind::Read,
            tier: "internal",
            input: json!({ "type": "object", "properties": {} }),
        },
        Contract {
            name: ANSIBLE_NAS_DOC,
            description: "Read the lab's ansible-nas docs, which say how each app is set up: a file (roles/<name>/README.md, docs/…/*.md or website/docs/…/*.md, e.g. website/docs/applications/download-tools/gluetun.md), or a folder to list (roles lists every role; docs, website/docs or a folder in them). Nothing else of ansible-nas can be read. Free.",
            kind: Kind::Read,
            tier: "internal",
            input: json!({ "type": "object", "properties": {
                "path": { "type": "string", "description": "A doc's path, or a folder to list, e.g. roles or roles/deluge/README.md." },
            } }),
        },
        Contract {
            name: REPO_CHECKOUT,
            description: "Check out the lab's ansible-nas fork into your sandbox at /work/repo: its default branch, or one of the team's hq/ branches. A branch your team submitted comes back whole before the owner has pushed it, on top of its base, to go on from or to review: your own work's (its task, the tasks under and above it) or one you submitted yourself. Git is set up there (commits are authored Endora HQ with the trailer added for you). Free; the sandbox stays offline.",
            kind: Kind::Read,
            tier: "internal",
            input: json!({ "type": "object", "properties": {
                "branch": { "type": "string", "description": "The fork's default branch unless you name a team branch, hq/task-N-slug (a submitted one of your work)." },
                "why": { "type": "string", "description": "Why, in a few words." },
            }, "required": ["why"] }),
        },
        Contract {
            name: REPO_SUBMIT,
            description: "Send your branch (hq/task-N-slug, N being your task's number, committed in /work/repo) to Endora, which keeps it, writes its diff as a document on your task and runs the lab's checks (ansible syntax, ansible-lint, yamllint) on the dev node, judged against the fork's develop (it passes when no check is worse than on develop); the result comes back as a receipt. Refused unless it is named for your task, sits on the fork's default branch, changes at most 200 files and 1 MB, and every commit is HQ's. Nothing is pushed: the owner pushes an accepted branch.",
            kind: Kind::AutomaticAct,
            tier: "internal",
            input: json!({ "type": "object", "properties": {
                "branch": { "type": "string", "description": "Your branch, hq/task-N-slug, N being your task's number." },
                "why": { "type": "string", "description": "What the branch does, in a sentence the tester reads." },
            }, "required": ["branch", "why"] }),
        },
    ]
}

/// The compose services that are Endora's own, which the team may never touch.
const ENDORAS_SERVICES: [&str; 9] = [
    "node",
    "hq",
    "runner",
    "worker",
    "docker-observer",
    "sound",
    "senders",
    "spotify",
    "runtime",
];

/// Whether a container is Endora's own (or on the path the owner reaches it by), which no
/// contract touches: a name starting `endora`, the words the console protects, or one of
/// Endora's compose services, as its name or its compose label (`service`, empty when
/// unknown). A worker's container is matched by its image through the console's protection.
#[must_use]
pub fn endoras_own(name: &str, service: &str) -> bool {
    let name = name.trim_start_matches('/').to_ascii_lowercase();
    let service = service.to_ascii_lowercase();
    if name.starts_with("endora") || service.starts_with("endora") {
        return true;
    }
    if ["cloudflared", "portainer", "docker-observer"]
        .iter()
        .any(|word| name.contains(word))
    {
        return true;
    }
    ENDORAS_SERVICES.iter().any(|own| {
        service == *own
            || name == *own
            || name.starts_with(&format!("{own}-"))
            || name.starts_with(&format!("{own}_"))
    })
}

/// Whether the team may restart a container now, given when it and every container were
/// restarted by the team before: once an hour each, and [`TEAM_RESTARTS_AN_HOUR`] an hour in
/// all.
///
/// # Errors
/// Why not, as the model reads it.
pub fn may_restart(this_one: &[i64], all: &[i64], now: i64) -> Result<(), String> {
    let within_the_hour = |at: &&i64| now - **at < HOUR_MS;
    if this_one.iter().any(|at| now - *at < HOUR_MS) {
        return Err(
            "it was restarted less than an hour ago: one restart a container an hour. If it is still unwell, tell the owner"
                .to_owned(),
        );
    }
    if all.iter().filter(within_the_hour).count() >= TEAM_RESTARTS_AN_HOUR {
        return Err(format!(
            "the team has made its {TEAM_RESTARTS_AN_HOUR} restarts this hour. Wait, or tell the owner"
        ));
    }
    Ok(())
}

/// What the team is told when a restart would not help.
pub const RESTART_WONT_FIX: &str = "restarting will not fix this; put it on the owner's desk with ask_owner (e.g. raise its memory)";

/// What the team is told when the container is not running: a restart would start it, and a
/// stopped container may be stopped on purpose (an old one kept for its settings, whose start
/// would take a port another needs).
pub const RESTART_STOPPED: &str = "it is stopped, perhaps on purpose: restart only fixes a running container; ask the owner with ask_owner if it should run";

/// Whether a restart would start a container that is not running, from what Docker's
/// inspection says of it. Only a `running` container is restarted: anything else, a state
/// that cannot be read included, is the owner's to start.
#[must_use]
pub fn restart_would_start(inspected: &Value) -> bool {
    inspected["state"] != "running"
}

/// How many restarts by Docker, with the last start within the hour, make a crash loop.
pub const CRASH_LOOP_RESTARTS: u64 = 3;

/// Whether a restart would not fix a container, from what Docker's inspection says of it (the
/// console's `team-inspect`: `state`, `restart_count`, `oom_killed`, `started_at`): it is
/// already restarting, it ran out of memory, or it is crash-looping (Docker has restarted it
/// [`CRASH_LOOP_RESTARTS`] times or more and it last started within the hour). A start time
/// that cannot be read counts as recent: in doubt, the owner decides.
#[must_use]
pub fn restart_would_not_fix(inspected: &Value, now_ms: i64) -> bool {
    if inspected["state"] == "restarting" || inspected["oom_killed"] == true {
        return true;
    }
    if inspected["restart_count"].as_u64().unwrap_or(0) < CRASH_LOOP_RESTARTS {
        return false;
    }
    let started = inspected["started_at"].as_str().and_then(millis_of);
    started.is_none_or(|at| now_ms - at < HOUR_MS)
}

/// Milliseconds since the epoch of a UTC time as Docker writes it
/// (`2026-10-17T10:00:00.123456789Z`); the fraction is ignored.
fn millis_of(at: &str) -> Option<i64> {
    let (date, time) = at.strip_suffix('Z')?.split_once('T')?;
    let mut date = date.split('-').map(str::parse::<i64>);
    let (y, m, d) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let mut time = time.split('.').next()?.split(':').map(str::parse::<i64>);
    let (h, min, sec) = (time.next()?.ok()?, time.next()?.ok()?, time.next()?.ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + min) * 60_000 + sec * 1000)
}

/// The newest [`LOG_LINES`] lines of a log, held to [`MOST_OF_A_LOG`] bytes from the newest
/// end on a line's edge, so what is kept is whole lines.
#[must_use]
pub fn newest_of(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let newest = &lines[lines.len().saturating_sub(LOG_LINES)..];
    let mut kept: Vec<&str> = Vec::new();
    let mut size = 0;
    for line in newest.iter().rev() {
        if size + line.len() + 1 > MOST_OF_A_LOG {
            break;
        }
        size += line.len() + 1;
        kept.push(line);
    }
    kept.reverse();
    kept.join("\n")
}

/// The team's restarts, kept in the node's database so that restarting the node does not
/// give the team its limits back.
pub struct RestartLedger {
    db: Db,
}

impl RestartLedger {
    /// The ledger, its table made if it is not there.
    ///
    /// # Errors
    /// [`RepositoryError`] if the table cannot be made.
    pub fn new(db: Db) -> Result<Self, RepositoryError> {
        db.lock()?
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS team_restarts (container TEXT NOT NULL, at INTEGER NOT NULL);",
            )
            .map_err(failed)?;
        Ok(Self { db })
    }

    /// Takes one restart of `container` at `now`, if [`may_restart`] allows it, in one step:
    /// two turns asking at once cannot both take the last one. A restart is taken before it
    /// is tried, so one that fails still counts.
    ///
    /// # Errors
    /// `Ok(Err(why))` when the limits refuse it; [`RepositoryError`] when the ledger fails.
    pub fn take(&self, container: &str, now: i64) -> Result<Result<(), String>, RepositoryError> {
        let mut conn = self.db.lock()?;
        let tx = conn.transaction().map_err(failed)?;
        let since: Vec<(String, i64)> = {
            let mut stmt = tx
                .prepare("SELECT container, at FROM team_restarts WHERE at > ?1")
                .map_err(failed)?;
            stmt.query_map([now - HOUR_MS], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(failed)?
                .collect::<Result<_, _>>()
                .map_err(failed)?
        };
        let this_one: Vec<i64> = since
            .iter()
            .filter(|(c, _)| c == container)
            .map(|(_, at)| *at)
            .collect();
        let all: Vec<i64> = since.iter().map(|(_, at)| *at).collect();
        if let Err(why) = may_restart(&this_one, &all, now) {
            return Ok(Err(why));
        }
        tx.execute(
            "INSERT INTO team_restarts (container, at) VALUES (?1, ?2)",
            params![container, now],
        )
        .map_err(failed)?;
        tx.execute(
            "DELETE FROM team_restarts WHERE at < ?1",
            [now - 24 * HOUR_MS],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(Ok(()))
    }
}

fn failed(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Backend(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: i64 = 1_792_166_400_000;

    #[test]
    fn seven_contracts_each_a_tool_with_a_schema() {
        let tools: Vec<Value> = all().iter().map(Contract::as_tool).collect();
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(
            names,
            [
                HOMELAB_STATUS,
                CONTAINER_LOGS,
                RESTART_CONTAINER,
                HOMELAB_MAP,
                ANSIBLE_NAS_DOC,
                REPO_CHECKOUT,
                REPO_SUBMIT
            ]
        );
        for tool in &tools {
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
            assert!(!tool["description"].as_str().unwrap().is_empty());
        }
        let kinds: Vec<Kind> = all().iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Read,
                Kind::Read,
                Kind::AutomaticAct,
                Kind::Read,
                Kind::Read,
                Kind::Read,
                Kind::AutomaticAct
            ]
        );
        assert_eq!(all()[1].tier, "confidential", "logs are");
    }

    #[test]
    fn the_wings_git_goes_to_a_lead_or_a_developer_on_any_brain_and_nobody_else() {
        let all = all();
        for contract in &all[5..] {
            assert_eq!(contract.tier, "internal", "the fork is public");
            for role in ["lead", "developer"] {
                for cloud in [false, true] {
                    assert!(
                        contract.offered_to(role, cloud),
                        "{} to a {role}",
                        contract.name
                    );
                }
            }
            for role in ["ceo", "reviewer", "researcher", ""] {
                assert!(
                    !contract.offered_to(role, false),
                    "{} to a {role}",
                    contract.name
                );
            }
        }
    }

    #[test]
    fn every_role_on_any_brain_is_offered_the_map_and_the_docs() {
        let all = all();
        for contract in &all[3..5] {
            assert_eq!(contract.tier, "internal", "names and wiring only");
            for role in ["ceo", "lead", "developer", "reviewer", "researcher"] {
                for cloud in [false, true] {
                    assert!(
                        contract.offered_to(role, cloud),
                        "{} to a {role}",
                        contract.name
                    );
                }
            }
        }
    }

    #[test]
    fn a_log_goes_to_a_cloud_turn_only_when_it_is_the_leads() {
        let all = all();
        let (status, logs, restart) = (&all[0], &all[1], &all[2]);
        for role in ["lead", "developer", "reviewer", "researcher"] {
            assert!(logs.may_be_seen_by(role, false), "{role} at home");
            assert!(status.may_be_seen_by(role, true) && restart.may_be_seen_by(role, true));
        }
        assert!(logs.may_be_seen_by("lead", true));
        for role in ["developer", "reviewer", "researcher"] {
            assert!(!logs.may_be_seen_by(role, true), "a cloud {role}");
        }
        // The CEO reads the home lab as the lead does, its state and its logs, and never acts
        // on it (ADR 0120, the owner's word of 2026-10-18).
        assert!(status.offered_to("ceo", true) && logs.offered_to("ceo", true));
        assert!(!restart.offered_to("ceo", true) && !restart.offered_to("ceo", false));
        assert!(restart.offered_to("lead", true));
    }

    #[test]
    fn endoras_own_containers_are_never_the_teams() {
        for (name, service) in [
            ("endora-node", ""),
            ("endora-hq", "hq"),
            ("endora-runner", ""),
            ("Endora-Sound", ""),
            ("endora-senders", ""),
            ("endora-spotify", "spotify"),
            ("/endora-node", ""),
            ("node", ""),
            ("hq-1", ""),
            ("project_runner_1", "runner"),
            ("quirky_hopper", "worker"),
            ("docker-observer", ""),
            ("portainer", ""),
            ("cloudflared-tunnel", ""),
        ] {
            assert!(endoras_own(name, service), "{name} / {service}");
        }
        for name in ["sonarr", "radarr", "jellyfin", "homenode", "soundcloud-dl"] {
            assert!(!endoras_own(name, ""), "{name}");
        }
    }

    #[test]
    fn once_an_hour_a_container_and_three_an_hour_in_all() {
        assert_eq!(may_restart(&[], &[], NOON), Ok(()));
        assert!(may_restart(&[NOON - HOUR_MS + 1], &[NOON - HOUR_MS + 1], NOON).is_err());
        assert_eq!(
            may_restart(&[NOON - HOUR_MS], &[NOON - HOUR_MS], NOON),
            Ok(()),
            "an hour on, it may again"
        );
        let three = [NOON - 10, NOON - 20, NOON - 30];
        let why = may_restart(&[], &three, NOON).unwrap_err();
        assert!(why.contains('3'), "{why}");
        assert_eq!(may_restart(&[], &three[..2], NOON), Ok(()));
    }

    #[test]
    fn the_ledger_holds_the_limits_and_survives_being_opened_again() {
        let db = Db::open_in_memory().unwrap();
        let ledger = RestartLedger::new(db.clone()).unwrap();
        assert_eq!(ledger.take("sonarr", NOON).unwrap(), Ok(()));
        assert!(
            ledger.take("sonarr", NOON + 60_000).unwrap().is_err(),
            "once an hour"
        );
        assert_eq!(ledger.take("radarr", NOON + 1).unwrap(), Ok(()));
        // A node restarted: the same database, a new ledger, the same limits.
        let again = RestartLedger::new(db).unwrap();
        assert_eq!(again.take("jellyfin", NOON + 2).unwrap(), Ok(()));
        assert!(
            again.take("bazarr", NOON + 3).unwrap().is_err(),
            "three an hour"
        );
        assert_eq!(again.take("bazarr", NOON + HOUR_MS + 1).unwrap(), Ok(()));
    }

    #[test]
    fn a_restart_will_not_fix_a_container_already_restarting_out_of_memory_or_crash_looping() {
        // NOON is 2026-10-16 16:00 UTC.
        assert_eq!(millis_of("2026-10-16T16:00:00.5Z"), Some(NOON));
        assert_eq!(millis_of("2026-10-16T15:00:00Z"), Some(NOON - HOUR_MS));
        assert_eq!(millis_of("2024-02-29T00:00:00Z"), Some(1_709_164_800_000));
        let seen = |state: &str, count: u64, oom: bool, started: &str| json!({ "state": state, "restart_count": count, "oom_killed": oom, "started_at": started });
        let recent = "2026-10-16T15:50:00.000000000Z";
        let long_ago = "2026-10-15T09:00:00Z";
        assert!(restart_would_not_fix(
            &seen("restarting", 0, false, long_ago),
            NOON
        ));
        assert!(restart_would_not_fix(
            &seen("exited", 0, true, long_ago),
            NOON
        ));
        assert!(restart_would_not_fix(
            &seen("running", 14, false, recent),
            NOON
        ));
        assert!(restart_would_not_fix(
            &seen("running", 3, false, "garbled"),
            NOON
        ));
        assert!(
            !restart_would_not_fix(&seen("running", 14, false, long_ago), NOON),
            "it settled"
        );
        assert!(!restart_would_not_fix(
            &seen("running", 2, false, recent),
            NOON
        ));
        assert!(!restart_would_not_fix(
            &seen("exited", 0, false, long_ago),
            NOON
        ));
    }

    #[test]
    fn a_restart_never_starts_a_container_that_is_not_running() {
        let state = |s: &str| json!({ "state": s, "restart_count": 0, "oom_killed": false });
        assert!(!restart_would_start(&state("running")));
        for stopped in ["exited", "created", "paused", "dead", ""] {
            assert!(restart_would_start(&state(stopped)), "{stopped}");
        }
        assert!(restart_would_start(&json!({})), "unread is not running");
    }

    #[test]
    fn the_newest_lines_are_kept_whole_and_within_the_size() {
        let log: String = (1..=150).map(|i| format!("line {i}\n")).collect();
        let kept = newest_of(&log);
        assert!(kept.starts_with("line 51\n"), "{kept}");
        assert!(kept.ends_with("line 150"));
        let long: String = (1..=100)
            .map(|i| format!("{i:04} {}\n", "x".repeat(400)))
            .collect();
        let kept = newest_of(&long);
        assert!(kept.len() <= MOST_OF_A_LOG);
        assert!(kept.ends_with(&"x".repeat(400)));
        assert!(kept.lines().all(|l| l.len() == 405), "whole lines only");
    }
}
