//! The bridge (ADR 0108): Endora's own capabilities, and the tools of the tool servers it has
//! installed, offered to HQ's team as tools, under the policy Endora already has. A worker
//! proposes a call; this decides it, in a fixed order, from what Endora knows of the tool and
//! never from what a tool says of itself:
//!
//! 1. **Never offered**: a tool the owner switched off; a Restricted one (where they are,
//!    and the like); Endora's own administration (a connection's settings and their test);
//!    anything about the team, its money, or the owner's chat, brain, memory or mail; and a
//!    switch of the owner's own devices (an act of the tool server paired with Home
//!    Assistant), which the team's remit never was.
//! 2. **The data tier**: a Confidential tool's result is given only to a turn on a local
//!    model, but for the few the owner allowed a lead thinking with Claude ([`may_see`]).
//! 3. **The member's role**: reads for every role, acts only for the roles that act
//!    ([`PROFILES`]).
//! 4. **The owner's stance**: an act that is on runs at once, within [`ACTS_AN_HOUR`] a tool
//!    an hour across the team, and is told to the owner; an act that asks waits on the owner's
//!    desk until they approve exactly what it would run. A turn that has read a stranger's
//!    words (a third-party result) asks before any act it gives arguments to.
//! 5. **Approvals** run exactly what was stored, only while the stance is not off, and expire
//!    unrun after [`APPROVALS_LAST_MS`].
//!
//! The node serves the tools on its team listener and keeps what is here in its own database.

use endora_kernel::{RepositoryError, Reversibility, Tier};
use endora_persistence::Db;
use rusqlite::{OptionalExtension as _, params};
use serde_json::{Value, json};

use crate::application::Stance;

/// An hour, in milliseconds.
pub const HOUR_MS: i64 = 60 * 60 * 1000;
/// The most times the team may run one act on its own in an hour, across the team.
pub const ACTS_AN_HOUR: usize = 3;
/// How long an approval may wait for the owner before it goes unrun: a day.
pub const APPROVALS_LAST_MS: i64 = 24 * HOUR_MS;

/// Whether a bridged tool only looks or changes something, by Endora's own knowledge of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Looks, and changes nothing.
    Read,
    /// Changes something.
    Act,
}

impl Kind {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Act => "act",
        }
    }

    /// The kind of a built-in skill, from the band it declares: only an observation reads.
    #[must_use]
    pub fn of_skill(band: Reversibility) -> Self {
        if band == Reversibility::Observe {
            return Self::Read;
        }
        Self::Act
    }

    /// The kind of a tool server's tool. A server's own description of a tool is not
    /// evidence (ADR 0054): only the tool the owner named as the server's state reader reads.
    /// Every other tool, whatever it calls itself, is an act.
    #[must_use]
    pub const fn of_server_tool(the_owners_reader: bool) -> Self {
        if the_owners_reader {
            return Self::Read;
        }
        Self::Act
    }
}

/// What each role may do with a bridged tool. A small table that a later owner setting could
/// replace: the lead and developers read and act; the CEO (ADR 0111), reviewers and
/// researchers only read. A role not listed may do nothing.
pub const PROFILES: [(&str, &[Kind]); 5] = [
    ("ceo", &[Kind::Read]),
    ("lead", &[Kind::Read, Kind::Act]),
    ("developer", &[Kind::Read, Kind::Act]),
    ("reviewer", &[Kind::Read]),
    ("researcher", &[Kind::Read]),
];

/// Whether a member in `role` may use a tool of `kind`.
#[must_use]
pub fn may(role: &str, kind: Kind) -> bool {
    PROFILES
        .iter()
        .any(|(r, kinds)| *r == role && kinds.contains(&kind))
}

/// The built-in skills that are never the team's, whatever their stance: the owner's mail,
/// what Endora did and said in the owner's chat, the brain's pages, the brief (calendar,
/// mail's senders, the brain), the image the owner sent, and where they are.
pub const NEVER_BRIDGED: [&str; 6] = [
    "mail",
    "own_activity",
    "read_page",
    "brief",
    "image_review",
    "location",
];

/// The built-in skills that reach any address with arguments the caller chooses. Offered to
/// the team they would be a way out of its fence: what a turn has read (the home lab's state,
/// a scrubbed log) could leave in a URL. The team asks the owner instead.
pub const OUT_OF_THE_FENCE: [&str; 2] = ["web_fetch", "web_search"];

/// Words that keep a tool server's tool off the bridge whatever its stance: a tool about the
/// owner's mail, chat, brain or memory, about Endora itself or the team, or about money. A
/// tool server can be named anything, so this fails safe: a tool kept off by mistake is
/// asked for in a thread, never quietly given.
const NEVER_WORDS: [&str; 15] = [
    "mail", "inbox", "imap", "smtp", "chat", "brain", "memory", "endora", "team", "bank", "money",
    "pay", "wallet", "finance", "budget",
];

/// What the bridge knows of one capability or tool server's tool.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Its id: `sonarr_queue`, or `server.tool`.
    pub id: String,
    /// What it does, as Endora describes it.
    pub description: String,
    /// Its arguments' schema, if it takes any.
    pub input_schema: Option<Value>,
    /// Whether it reads or acts, by Endora's knowledge.
    pub kind: Kind,
    /// The owner's stance, or its default.
    pub stance: Stance,
    /// Whether it is a tool server's tool.
    pub from_a_server: bool,
    /// Whether it is set up and can run.
    pub ready: bool,
    /// Whether it only holds a connection's settings (Endora's administration).
    pub administration: bool,
    /// Whether what it returns is a stranger's words (ADR 0064).
    pub third_party: bool,
    /// Whether it switches the owner's own devices ([`switches_the_owners_devices`]).
    pub owners_devices: bool,
}

/// Whether a tool server's tool `id` (`server.tool`) of `kind` switches the owner's own
/// devices: an act of the server Endora pairs with Home Assistant, `paired` by the owner's
/// setting (see [`crate::paired_server`]). Live, on 2026-10-17, the developer asked to run
/// `HassTurnOff` on a container's name "to restart" it: the team works on the home lab's
/// containers, and turning the owner's lights, switches and appliances on or off was never
/// part of that. Only the tool the owner named the server's reader is a read (ADR 0054), and
/// it stays under the tier's rules; every other tool of that server is kept from the team.
#[must_use]
pub fn switches_the_owners_devices(id: &str, kind: Kind, paired: &str) -> bool {
    kind == Kind::Act
        && id
            .split_once('.')
            .is_some_and(|(server, _)| server == paired)
}

/// One tool the team is offered.
#[derive(Debug, Clone, PartialEq)]
pub struct Bridged {
    /// The tool's name: its id, with anything a model's tool name may not hold made `_`.
    pub name: String,
    /// The capability or tool it runs.
    pub id: String,
    /// What the model is told.
    pub description: String,
    /// Its arguments' schema, with `why` added for an act.
    pub input: Value,
    /// Whether it reads or acts.
    pub kind: Kind,
    /// The owner's stance on it.
    pub stance: Stance,
    /// The tier of what it returns.
    pub tier: Tier,
    /// Whether what it returns is a stranger's words.
    pub third_party: bool,
    /// Whether it takes any arguments of its own.
    pub takes_arguments: bool,
}

impl Bridged {
    /// As an MCP tool: name, description, input schema, and its kind.
    #[must_use]
    pub fn as_tool(&self) -> Value {
        json!({ "name": self.name, "description": self.description,
                "inputSchema": self.input, "kind": self.kind.name() })
    }
}

/// Why a candidate is not offered, or `None` when it may be.
#[must_use]
pub fn kept_off(c: &Candidate) -> Option<&'static str> {
    let tier = crate::tiers::tier_of(&c.id);
    if c.stance == Stance::Off {
        return Some("switched off");
    }
    if tier == Tier::Restricted {
        return Some("Restricted");
    }
    if c.administration {
        return Some("Endora's administration");
    }
    if NEVER_BRIDGED.contains(&c.id.as_str()) {
        return Some("the owner's, never the team's");
    }
    if OUT_OF_THE_FENCE.contains(&c.id.as_str()) {
        return Some("a way out of the team's fence");
    }
    let lower = c.id.to_ascii_lowercase();
    if c.from_a_server && NEVER_WORDS.iter().any(|w| lower.contains(w)) {
        return Some("the owner's, never the team's");
    }
    if c.owners_devices {
        return Some("the owner's devices, never the team's");
    }
    if !c.ready {
        return Some("not set up");
    }
    None
}

/// The tool a candidate is offered as, or why it is not.
///
/// # Errors
/// Why it is kept off ([`kept_off`]).
pub fn bridge(c: &Candidate) -> Result<Bridged, &'static str> {
    if let Some(why) = kept_off(c) {
        return Err(why);
    }
    let mut input = c
        .input_schema
        .clone()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    if input["properties"].is_null() {
        input["properties"] = json!({});
    }
    let takes_arguments = input["properties"]
        .as_object()
        .is_some_and(|p| !p.is_empty());
    let mut description = c.description.trim().to_owned();
    if c.kind == Kind::Act {
        input["properties"]["why"] = json!({ "type": "string",
            "description": "Why, in a sentence the owner reads." });
        let mut required: Vec<Value> = input["required"].as_array().cloned().unwrap_or_default();
        required.push(json!("why"));
        input["required"] = Value::Array(required);
        description.push_str(if c.stance == Stance::Ask {
            " An action that asks the owner first: calling it puts exactly this on their desk and ends your turn; you are woken when they decide. Say why."
        } else {
            " An action: it runs at once, a few times an hour at most across the team, and the owner is told, with your reason. Say why."
        });
    }
    Ok(Bridged {
        name: tool_name(&c.id),
        id: c.id.clone(),
        description,
        input,
        kind: c.kind,
        stance: c.stance,
        tier: crate::tiers::tier_of(&c.id),
        third_party: c.third_party,
        takes_arguments,
    })
}

/// A tool name a model accepts: letters, digits, `_` and `-`, at most 64. A tool server's
/// `server.tool` becomes `server__tool`.
#[must_use]
pub fn tool_name(id: &str) -> String {
    id.replace('.', "__")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// What a call to a bridged tool comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    /// Run it and give back what it read.
    Read,
    /// Run it now, within the limits, and tell the owner.
    Act,
    /// Put it on the owner's desk for approval, and end the turn.
    AskTheOwner,
}

/// How a call goes, in the policy's order, or why it is refused: the stance (off is never),
/// the role, then for an act the stance and whether the turn has read a stranger's words.
///
/// # Errors
/// Why it is refused, as the model reads it.
pub fn way(tool: &Bridged, role: &str, marked: bool) -> Result<Way, String> {
    if tool.stance == Stance::Off {
        return Err(format!("the owner has switched {} off", tool.name));
    }
    if !may(role, tool.kind) {
        return Err(format!(
            "a {} may only use tools that read; ask the owner, or hand it to the lead",
            if role.is_empty() { "member" } else { role }
        ));
    }
    if tool.kind == Kind::Read {
        return Ok(Way::Read);
    }
    if tool.stance == Stance::Ask || (marked && tool.takes_arguments) {
        return Ok(Way::AskTheOwner);
    }
    Ok(Way::Act)
}

/// The Confidential results a lead's turn on a cloud model may be given, by the owner's
/// decision of 2026-10-17 (ADR 0109): a container's scrubbed log, and the download queues with
/// why each item is stuck. Every other Confidential result stays with turns on the house's
/// own model, and no other role's cloud turn is given any.
pub const CLOUD_LEAD_MAY_SEE: [&str; 3] = ["container_logs", "sonarr_queue", "radarr_queue"];

/// Whether a turn may be given what the tool `id` of `tier` returns, for a member in `role`:
/// Restricted never (it is not offered at all); Confidential to a turn on a local model, and
/// to a lead's turn on a cloud model only what [`CLOUD_LEAD_MAY_SEE`] names; the rest to all.
/// The CEO on a cloud model is a cloud member like any but the lead: it is given nothing
/// Confidential, and reads its members' findings in their threads (ADR 0111).
#[must_use]
pub fn may_see(tier: Tier, id: &str, role: &str, on_a_cloud_model: bool) -> bool {
    match tier {
        Tier::Restricted => false,
        Tier::Confidential if !on_a_cloud_model => true,
        Tier::Confidential => role == "lead" && CLOUD_LEAD_MAY_SEE.contains(&id),
        _ => true,
    }
}

/// A call's arguments in plain words, one `name: value` a line, in the order given; `why`
/// is left out (it is said apart). Text stays text: the screen escapes it.
#[must_use]
pub fn plain_words(args: &Value) -> Vec<(String, String)> {
    args.as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| k.as_str() != "why")
        .map(|(k, v)| {
            let said = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), said.chars().take(300).collect())
        })
        .collect()
}

/// An approval the node keeps: exactly what will run when the owner says yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// Its id.
    pub id: i64,
    /// The capability or tool it runs.
    pub tool: String,
    /// The tool's name, as the team called it.
    pub name: String,
    /// Its arguments, as JSON text, `why` taken out.
    pub args: String,
    /// The member's reason.
    pub why: String,
    /// The member who asked.
    pub member: String,
    /// The member's role.
    pub role: String,
    /// The turn it was asked in.
    pub run: i64,
    /// The owner's desk item in HQ, once HQ has it.
    pub desk: Option<i64>,
    /// When it was asked.
    pub at: i64,
    /// `waiting`, `approved`, `declined`, `expired` or `refused`.
    pub status: String,
}

/// What the bridge keeps in the node's database: the acts run on their own (for the limits),
/// the approvals, and the turns that have read a stranger's words.
pub struct BridgeStore {
    db: Db,
}

impl BridgeStore {
    /// The store, its tables made if they are not there.
    ///
    /// # Errors
    /// [`RepositoryError`] if the tables cannot be made.
    pub fn new(db: Db) -> Result<Self, RepositoryError> {
        db.lock()?
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS team_acts (tool TEXT NOT NULL, at INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS team_approvals (
                     id INTEGER PRIMARY KEY, tool TEXT NOT NULL, name TEXT NOT NULL,
                     args TEXT NOT NULL, why TEXT NOT NULL, member TEXT NOT NULL,
                     role TEXT NOT NULL, run INTEGER NOT NULL, desk INTEGER,
                     at INTEGER NOT NULL, status TEXT NOT NULL DEFAULT 'waiting');
                 CREATE TABLE IF NOT EXISTS team_marked_runs (run INTEGER PRIMARY KEY);",
            )
            .map_err(failed)?;
        Ok(Self { db })
    }

    /// Takes one run of `tool` at `now`, if the team has made fewer than [`ACTS_AN_HOUR`] of
    /// it in the hour, in one step. Taken before it is tried, so a failed one counts.
    ///
    /// # Errors
    /// `Ok(Err(why))` when the limit refuses it; [`RepositoryError`] when the store fails.
    pub fn take_act(&self, tool: &str, now: i64) -> Result<Result<(), String>, RepositoryError> {
        let mut conn = self.db.lock()?;
        let tx = conn.transaction().map_err(failed)?;
        let made: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM team_acts WHERE tool = ?1 AND at > ?2",
                params![tool, now - HOUR_MS],
                |r| r.get(0),
            )
            .map_err(failed)?;
        if usize::try_from(made).unwrap_or(usize::MAX) >= ACTS_AN_HOUR {
            return Ok(Err(format!(
                "the team has run it {ACTS_AN_HOUR} times this hour. Wait, or ask the owner"
            )));
        }
        tx.execute(
            "INSERT INTO team_acts (tool, at) VALUES (?1, ?2)",
            params![tool, now],
        )
        .map_err(failed)?;
        tx.execute("DELETE FROM team_acts WHERE at < ?1", [now - 24 * HOUR_MS])
            .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(Ok(()))
    }

    /// Marks a turn as having read a stranger's words.
    ///
    /// # Errors
    /// [`RepositoryError`] when the store fails.
    pub fn mark(&self, run: i64) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR IGNORE INTO team_marked_runs (run) VALUES (?1)",
                [run],
            )
            .map_err(failed)?;
        Ok(())
    }

    /// Whether a turn has read a stranger's words.
    ///
    /// # Errors
    /// [`RepositoryError`] when the store fails.
    pub fn marked(&self, run: i64) -> Result<bool, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT 1 FROM team_marked_runs WHERE run = ?1",
                [run],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(failed)
    }

    /// Keeps an approval, waiting, and gives its id.
    ///
    /// # Errors
    /// [`RepositoryError`] when the store fails.
    #[allow(clippy::too_many_arguments)]
    pub fn add_approval(
        &self,
        tool: &str,
        name: &str,
        args: &Value,
        why: &str,
        member: &str,
        role: &str,
        run: i64,
        now: i64,
    ) -> Result<i64, RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "INSERT INTO team_approvals (tool, name, args, why, member, role, run, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![tool, name, args.to_string(), why, member, role, run, now],
        )
        .map_err(failed)?;
        Ok(conn.last_insert_rowid())
    }

    /// Records the desk item HQ made for an approval.
    ///
    /// # Errors
    /// [`RepositoryError`] when the store fails.
    pub fn set_desk(&self, id: i64, desk: i64) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE team_approvals SET desk = ?2 WHERE id = ?1",
                params![id, desk],
            )
            .map_err(failed)?;
        Ok(())
    }

    /// Moves an approval from `from` to `to`, in one step: `false` when it was not in `from`
    /// (someone else moved it first), so it is settled once.
    ///
    /// # Errors
    /// [`RepositoryError`] when the store fails.
    pub fn settle(&self, id: i64, from: &str, to: &str) -> Result<bool, RepositoryError> {
        let changed = self
            .db
            .lock()?
            .execute(
                "UPDATE team_approvals SET status = ?3 WHERE id = ?1 AND status = ?2",
                params![id, from, to],
            )
            .map_err(failed)?;
        Ok(changed == 1)
    }

    /// The approval behind a desk item, if one is: the newest, should HQ have used the number
    /// before.
    ///
    /// # Errors
    /// [`RepositoryError`] when the store fails.
    pub fn by_desk(&self, desk: i64) -> Result<Option<Approval>, RepositoryError> {
        Ok(self
            .approvals("WHERE desk = ?1", desk)?
            .into_iter()
            .next_back())
    }

    /// The approvals waiting for the owner.
    ///
    /// # Errors
    /// [`RepositoryError`] when the store fails.
    pub fn waiting(&self) -> Result<Vec<Approval>, RepositoryError> {
        self.approvals(
            "WHERE status = 'waiting' AND desk IS NOT NULL AND ?1 = ?1",
            0,
        )
    }

    fn approvals(&self, filter: &str, value: i64) -> Result<Vec<Approval>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, tool, name, args, why, member, role, run, desk, at, status
                 FROM team_approvals {filter} ORDER BY id"
            ))
            .map_err(failed)?;
        stmt.query_map([value], |r| {
            Ok(Approval {
                id: r.get(0)?,
                tool: r.get(1)?,
                name: r.get(2)?,
                args: r.get(3)?,
                why: r.get(4)?,
                member: r.get(5)?,
                role: r.get(6)?,
                run: r.get(7)?,
                desk: r.get(8)?,
                at: r.get(9)?,
                status: r.get(10)?,
            })
        })
        .map_err(failed)?
        .collect::<Result<_, _>>()
        .map_err(failed)
    }
}

fn failed(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Backend(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, kind: Kind, stance: Stance) -> Candidate {
        Candidate {
            id: id.to_owned(),
            description: "Does a thing.".to_owned(),
            input_schema: None,
            kind,
            stance,
            from_a_server: id.contains('.'),
            ready: true,
            administration: false,
            third_party: false,
            owners_devices: false,
        }
    }

    #[test]
    fn off_restricted_administration_and_the_owners_own_are_never_offered() {
        let kept = |c: &Candidate| kept_off(c);
        assert_eq!(
            kept(&candidate("nas_status", Kind::Read, Stance::Off)),
            Some("switched off")
        );
        assert_eq!(
            kept(&candidate("location", Kind::Read, Stance::Auto)),
            Some("Restricted")
        );
        assert_eq!(
            kept(&candidate(
                "a-skill-added-tomorrow",
                Kind::Read,
                Stance::Auto
            )),
            Some("Restricted"),
            "an unclassified skill fails safe"
        );
        for own in ["mail", "own_activity", "read_page", "brief", "image_review"] {
            assert!(
                kept(&candidate(own, Kind::Read, Stance::Auto)).is_some(),
                "{own}"
            );
        }
        for server in [
            "gmail.search",
            "notes.memory_read",
            "mybank.balance",
            "endora.admin",
        ] {
            assert_eq!(
                kept(&candidate(server, Kind::Read, Stance::Auto)),
                Some("the owner's, never the team's"),
                "{server}"
            );
        }
        // Reaching any address with arguments the member chooses would be a way out of the
        // team's fence: what a turn has read could leave in a URL.
        for out in ["web_fetch", "web_search"] {
            assert_eq!(
                kept(&candidate(out, Kind::Read, Stance::Auto)),
                Some("a way out of the team's fence"),
                "{out}"
            );
        }
        let mut settings_only = candidate("terminal-portainer", Kind::Read, Stance::Auto);
        settings_only.administration = true;
        assert_eq!(kept(&settings_only), Some("Endora's administration"));
        let mut unset = candidate("nas_status", Kind::Read, Stance::Auto);
        unset.ready = false;
        assert_eq!(kept(&unset), Some("not set up"));
        assert_eq!(
            kept(&candidate("nas_status", Kind::Read, Stance::Auto)),
            None
        );
        assert_eq!(
            kept(&candidate("wiki.add_page", Kind::Act, Stance::Ask)),
            None
        );
    }

    #[test]
    fn the_owners_devices_are_never_the_teams_to_switch() {
        // Live, 2026-10-17: the developer asked to run Home Assistant's HassTurnOff on a
        // container's name "to restart" it. Switching the owner's devices was never the team's.
        for tool in [
            "home-assistant.HassTurnOff",
            "home-assistant.HassTurnOn",
            "home-assistant.intent.HassTurnOff",
        ] {
            assert!(
                switches_the_owners_devices(tool, Kind::Act, "home-assistant"),
                "{tool}"
            );
            let mut c = candidate(tool, Kind::Act, Stance::Auto);
            c.owners_devices = true;
            assert_eq!(kept_off(&c), Some("the owner's devices, never the team's"));
            assert!(bridge(&c).is_err());
        }
        // The server is the one the owner paired with Home Assistant, by its name.
        assert!(switches_the_owners_devices(
            "house.HassTurnOff",
            Kind::Act,
            "house"
        ));
        assert!(!switches_the_owners_devices(
            "home-assistant.HassTurnOff",
            Kind::Act,
            "house"
        ));
        assert!(!switches_the_owners_devices(
            "home-assistant-extra.HassTurnOff",
            Kind::Act,
            "home-assistant"
        ));
        // A read, the one the owner named the server's reader, stays under the tier's rules.
        assert!(!switches_the_owners_devices(
            "home-assistant.GetLiveContext",
            Kind::Read,
            "home-assistant"
        ));
        let reader = candidate("home-assistant.GetLiveContext", Kind::Read, Stance::Auto);
        assert_eq!(kept_off(&reader), None);
        assert_eq!(bridge(&reader).unwrap().tier, Tier::Confidential);
        // Built-in skills are no server's tools.
        assert!(!switches_the_owners_devices(
            "home_assistant",
            Kind::Act,
            "home-assistant"
        ));
    }

    #[test]
    fn a_tool_is_named_as_a_model_accepts_and_an_act_must_say_why() {
        let read = bridge(&candidate("nas_status", Kind::Read, Stance::Auto)).unwrap();
        assert_eq!(read.as_tool()["kind"], "read");
        assert_eq!(read.input, json!({ "type": "object", "properties": {} }));
        assert!(!read.takes_arguments);
        let mut act = candidate("wiki.add_page", Kind::Act, Stance::Ask);
        act.input_schema = Some(json!({ "type": "object",
            "properties": { "name": { "type": "string" } }, "required": ["name"] }));
        let act = bridge(&act).unwrap();
        assert_eq!(act.name, "wiki__add_page");
        assert_eq!(act.tier, Tier::Confidential);
        assert_eq!(act.input["required"], json!(["name", "why"]));
        assert!(act.takes_arguments);
        assert!(act.description.contains("asks the owner first"));
        assert_eq!(tool_name("a server.do it!"), "a_server__do_it_");
    }

    #[test]
    fn the_role_then_the_stance_then_a_strangers_words_decide_the_way() {
        let tool = |kind, stance, args: bool| Bridged {
            takes_arguments: args,
            ..bridge(&candidate("sonarr_retry_import", kind, stance)).unwrap()
        };
        let read = tool(Kind::Read, Stance::Auto, false);
        let on = tool(Kind::Act, Stance::Auto, false);
        let on_with_args = tool(Kind::Act, Stance::Auto, true);
        let ask = tool(Kind::Act, Stance::Ask, false);
        for role in ["ceo", "lead", "developer", "reviewer", "researcher"] {
            assert_eq!(way(&read, role, false), Ok(Way::Read), "{role}");
        }
        assert!(way(&on, "ceo", false).is_err(), "the CEO only reads");
        assert_eq!(way(&on, "developer", false), Ok(Way::Act));
        assert_eq!(way(&on, "lead", false), Ok(Way::Act));
        assert!(way(&on, "reviewer", false).is_err());
        assert!(way(&ask, "researcher", false).is_err());
        assert!(
            way(&read, "", false).is_err(),
            "a role not listed does nothing"
        );
        assert_eq!(way(&ask, "developer", false), Ok(Way::AskTheOwner));
        assert_eq!(
            way(&on, "developer", true),
            Ok(Way::Act),
            "given nothing, it is steered by nothing"
        );
        assert_eq!(
            way(&on_with_args, "developer", true),
            Ok(Way::AskTheOwner),
            "after a stranger's words, an act given arguments asks"
        );
        let mut off = on.clone();
        off.stance = Stance::Off;
        assert!(way(&off, "lead", false).unwrap_err().contains("switched"));
    }

    #[test]
    fn a_tool_server_tool_reads_only_when_the_owner_said_so() {
        assert_eq!(Kind::of_server_tool(false), Kind::Act);
        assert_eq!(Kind::of_server_tool(true), Kind::Read);
        assert_eq!(Kind::of_skill(Reversibility::Observe), Kind::Read);
        assert_eq!(Kind::of_skill(Reversibility::Reversible), Kind::Act);
    }

    #[test]
    fn confidential_is_given_to_a_local_turn_and_to_the_cloud_lead_only_as_the_owner_allowed() {
        for role in ["lead", "developer", "reviewer", "researcher"] {
            assert!(may_see(Tier::Confidential, "home_assistant", role, false));
            assert!(may_see(Tier::Confidential, "container_logs", role, false));
            assert!(may_see(Tier::Internal, "homelab_status", role, true));
            assert!(!may_see(Tier::Restricted, "location", role, false));
            assert!(
                !may_see(Tier::Confidential, "home_assistant", role, true),
                "{role}: every other Confidential result stays home"
            );
        }
        // The owner's decision of 2026-10-17 (ADR 0109): the lead's cloud turns may read the
        // scrubbed logs and the download queues, and nothing else Confidential.
        for allowed in CLOUD_LEAD_MAY_SEE {
            assert!(
                may_see(Tier::Confidential, allowed, "lead", true),
                "{allowed}"
            );
            for role in ["ceo", "developer", "reviewer", "researcher", ""] {
                assert!(
                    !may_see(Tier::Confidential, allowed, role, true),
                    "{allowed} to a cloud {role}"
                );
            }
        }
        assert_eq!(
            CLOUD_LEAD_MAY_SEE,
            ["container_logs", "sonarr_queue", "radarr_queue"]
        );
        assert!(
            !may_see(Tier::Restricted, "container_logs", "lead", true),
            "Restricted stays home whatever its name"
        );
    }

    #[test]
    fn arguments_are_said_plainly_without_the_reason() {
        assert_eq!(
            plain_words(&json!({ "item": 102, "why": "stuck", "name": "<b>x</b>" })),
            [
                ("item".to_owned(), "102".to_owned()),
                ("name".to_owned(), "<b>x</b>".to_owned())
            ]
        );
    }

    #[test]
    fn the_store_limits_acts_marks_turns_and_settles_an_approval_once() {
        const NOON: i64 = 1_792_166_400_000;
        let db = Db::open_in_memory().unwrap();
        let store = BridgeStore::new(db.clone()).unwrap();
        for n in 0..3 {
            assert_eq!(
                store.take_act("sonarr_retry_import", NOON + n).unwrap(),
                Ok(())
            );
        }
        assert!(
            store
                .take_act("sonarr_retry_import", NOON + 5)
                .unwrap()
                .is_err()
        );
        assert_eq!(
            store.take_act("radarr_retry_import", NOON + 5).unwrap(),
            Ok(())
        );
        let again = BridgeStore::new(db).unwrap();
        assert!(
            again
                .take_act("sonarr_retry_import", NOON + 6)
                .unwrap()
                .is_err(),
            "kept"
        );
        assert_eq!(
            again
                .take_act("sonarr_retry_import", NOON + HOUR_MS + 1)
                .unwrap(),
            Ok(())
        );

        assert!(!again.marked(7).unwrap());
        again.mark(7).unwrap();
        again.mark(7).unwrap();
        assert!(again.marked(7).unwrap());

        let id = again
            .add_approval(
                "sonarr_remove_and_search",
                "sonarr_remove_and_search",
                &json!({ "item": 102 }),
                "stuck",
                "Linus",
                "developer",
                7,
                NOON,
            )
            .unwrap();
        assert!(again.waiting().unwrap().is_empty(), "not on the desk yet");
        again.set_desk(id, 40).unwrap();
        let kept = again.by_desk(40).unwrap().unwrap();
        assert_eq!(
            (kept.args.as_str(), kept.status.as_str()),
            (r#"{"item":102}"#, "waiting")
        );
        assert_eq!(again.waiting().unwrap().len(), 1);
        assert!(again.settle(id, "waiting", "approved").unwrap());
        assert!(!again.settle(id, "waiting", "approved").unwrap(), "once");
        assert!(again.by_desk(41).unwrap().is_none());
    }
}
