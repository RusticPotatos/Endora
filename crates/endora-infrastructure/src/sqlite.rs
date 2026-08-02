//! SQLite-backed implementations of the application repository ports.
//!
//! A single [`SqliteStore`] owns one connection behind a mutex and implements
//! the repository traits. Identifiers are `u128`, which SQLite integers cannot
//! hold, so they are stored as their decimal `TEXT` form and parsed back on
//! read. The store is synchronous; when the async node uses it, calls run on a
//! blocking thread (see `docs/adr/0007-async-web-stack.md`).

use endora_persistence::Db;

use endora_application::{
    AuditId, AuditRecord, Belief, BeliefId, BeliefKind, BeliefStatus, ChatMessage, Confidence,
    Intention, IntentionId, IntentionState, MessageId, MessageRole, Outcome, OutcomeId, Preference,
    PreferenceId, PreferenceKind, Reaction, Timestamp,
};
use endora_application::{MemorySnapshot, MemoryStore, RepositoryError};
use rusqlite::{Connection, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS audit_log (
    id      TEXT PRIMARY KEY,
    at_ms   INTEGER NOT NULL,
    summary TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_audit_at ON audit_log(at_ms);

CREATE TABLE IF NOT EXISTS messages (
    id    TEXT PRIMARY KEY,
    role  TEXT NOT NULL,
    body  TEXT NOT NULL,
    at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS message_actions (
    message_id TEXT PRIMARY KEY,
    actions    TEXT NOT NULL
) STRICT;

-- The running conversation summary (ADR 0053 context compaction): a single row.
-- Persisted so a restart doesn't re-summarise the whole backlog (which, on a slow
-- local model, degraded the first turns after every deploy) and the butler keeps
-- the day's thread across restarts. `covered` = how many of the oldest messages the
-- summary already folds in.
CREATE TABLE IF NOT EXISTS conversation_summary (
    id      INTEGER PRIMARY KEY CHECK (id = 0),
    body    TEXT NOT NULL,
    covered INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_messages_at ON messages(at_ms);

CREATE TABLE IF NOT EXISTS preferences (
    id    TEXT PRIMARY KEY,
    body  TEXT NOT NULL,
    kind  TEXT NOT NULL,
    at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS checkin (
    id          INTEGER PRIMARY KEY CHECK (id = 0),
    enabled     INTEGER NOT NULL,
    interval_ms INTEGER NOT NULL,
    next_at_ms  INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS beliefs (
    id               TEXT PRIMARY KEY,
    statement        TEXT NOT NULL,
    kind             TEXT NOT NULL,
    confidence       TEXT NOT NULL,
    evidence         TEXT NOT NULL,
    created_ms       INTEGER NOT NULL,
    last_affirmed_ms INTEGER NOT NULL,
    status           TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS intentions (
    id                 TEXT PRIMARY KEY,
    statement          TEXT NOT NULL,
    motivating_belief  TEXT NOT NULL,
    note               TEXT NOT NULL,
    state              TEXT NOT NULL,
    created_ms         INTEGER NOT NULL,
    last_progressed_ms INTEGER NOT NULL,
    steps_taken        INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS outcomes (
    id                TEXT PRIMARY KEY,
    capability        TEXT NOT NULL,
    input             TEXT NOT NULL,
    claim             TEXT NOT NULL,
    observation       TEXT,
    at_ms             INTEGER NOT NULL,
    motivating_belief TEXT,
    reaction          TEXT,
    changed           INTEGER
) STRICT;
CREATE TABLE IF NOT EXISTS capability_config (
    id                TEXT PRIMARY KEY,
    enabled           INTEGER NOT NULL,
    open_irreversible INTEGER NOT NULL DEFAULT 0,
    confirm           INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS capability_settings (
    capability_id TEXT NOT NULL,
    key           TEXT NOT NULL,
    value         TEXT NOT NULL,
    PRIMARY KEY (capability_id, key)
) STRICT;
CREATE TABLE IF NOT EXISTS autonomy_envelope (
    id               INTEGER PRIMARY KEY CHECK (id = 0),
    auto_external    INTEGER NOT NULL,
    auto_consequential INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS brief_schedule (
    id        INTEGER PRIMARY KEY CHECK (id = 0),
    enabled   INTEGER NOT NULL,
    hour_utc  INTEGER NOT NULL,
    last_ms   INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS night_loop_schedule (
    id        INTEGER PRIMARY KEY CHECK (id = 0),
    enabled   INTEGER NOT NULL,
    hour_utc  INTEGER NOT NULL,
    last_ms   INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS deep_model (
    id       INTEGER PRIMARY KEY CHECK (id = 0),
    url      TEXT NOT NULL,
    model    TEXT NOT NULL,
    api_key  TEXT NOT NULL,
    escalate INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS butler_model_config (
    id            INTEGER PRIMARY KEY CHECK (id = 0),
    base_url      TEXT NOT NULL,
    api_key       TEXT NOT NULL,
    mixture       INTEGER NOT NULL,
    single_model  TEXT NOT NULL,
    single_temp   REAL,
    single_top_p  REAL,
    single_top_k  INTEGER,
    single_repeat REAL,
    router_model  TEXT NOT NULL,
    router_temp   REAL,
    router_top_p  REAL,
    router_top_k  INTEGER,
    router_repeat REAL,
    synth_model   TEXT NOT NULL,
    synth_temp    REAL,
    synth_top_p   REAL,
    synth_top_k   INTEGER,
    synth_repeat  REAL
) STRICT;
CREATE TABLE IF NOT EXISTS model_tune_schedule (
    id       INTEGER PRIMARY KEY CHECK (id = 0),
    enabled  INTEGER NOT NULL,
    hour_utc INTEGER NOT NULL,
    last_ms  INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS events (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    at_ms   INTEGER NOT NULL,
    summary TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS target_aliases (
    server TEXT NOT NULL,
    said   TEXT NOT NULL,
    means  TEXT NOT NULL,
    PRIMARY KEY (server, said)
) STRICT;
CREATE TABLE IF NOT EXISTS config_writes (
    id      TEXT PRIMARY KEY,
    at_ms   INTEGER NOT NULL,
    server  TEXT NOT NULL,
    target  TEXT NOT NULL,
    added   TEXT NOT NULL,
    was     TEXT NOT NULL,
    undone  INTEGER NOT NULL DEFAULT 0,
    kind    TEXT NOT NULL DEFAULT 'name'
) STRICT;
CREATE TABLE IF NOT EXISTS standing_trouble (
    server   TEXT NOT NULL,
    thing    TEXT NOT NULL,
    trouble  TEXT NOT NULL,
    since_ms INTEGER NOT NULL,
    accepted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server, thing, trouble)
) STRICT;
-- The token this node accepts on every /v1 request. One row. Generated on first run and
-- printed to the log, unless a deployment pinned one through ENDORA_TOKEN.
CREATE TABLE IF NOT EXISTS node_auth (
    id    INTEGER PRIMARY KEY CHECK (id = 0),
    token TEXT NOT NULL,
    -- Argon2id, salt and parameters included. Empty until sign-in has been set up.
    password_hash   TEXT NOT NULL DEFAULT '',
    -- The shared secret an authenticator app holds, base32 as the app expects it.
    totp_secret     TEXT NOT NULL DEFAULT '',
    -- Consecutive failed sign-ins, and when the last one was, so guessing gets expensive.
    failures        INTEGER NOT NULL DEFAULT 0,
    last_failure_ms INTEGER NOT NULL DEFAULT 0
) STRICT;
-- Tokens handed out by a successful sign-in. Separate from the bootstrap token on purpose:
-- that one is the recovery path and should not be what a password buys, and these can be
-- thrown away without changing it.
CREATE TABLE IF NOT EXISTS node_sessions (
    token     TEXT PRIMARY KEY,
    issued_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS watched_things (
    key                TEXT PRIMARY KEY,
    settled            TEXT NOT NULL,
    candidate          TEXT NOT NULL,
    candidate_since_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS transitions (
    key      TEXT NOT NULL,
    was      TEXT NOT NULL,
    became   TEXT NOT NULL,
    at_ms    INTEGER NOT NULL,
    PRIMARY KEY (key, at_ms)
) STRICT;
CREATE INDEX IF NOT EXISTS idx_transitions_at ON transitions(at_ms);
CREATE TABLE IF NOT EXISTS notions (
    id                TEXT PRIMARY KEY,
    statement         TEXT NOT NULL,
    settles_when      TEXT NOT NULL,
    created_ms        INTEGER NOT NULL,
    last_supported_ms INTEGER NOT NULL,
    status            TEXT NOT NULL
) STRICT;
-- One row per distinct record behind a notion (ADR 0057). The composite primary key is the
-- schema-level half of the rule that the same record cited twice is one piece of evidence:
-- the domain refuses the duplicate and the table cannot store it either, so no path adds the
-- same evidence twice however a notion comes to be written.
CREATE TABLE IF NOT EXISTS notion_citations (
    notion_id TEXT NOT NULL,
    source    TEXT NOT NULL,
    reference TEXT NOT NULL,
    PRIMARY KEY (notion_id, source, reference)
) STRICT;
CREATE TABLE IF NOT EXISTS mcp_servers (
    name    TEXT PRIMARY KEY,
    kind    TEXT NOT NULL,
    command TEXT NOT NULL DEFAULT '',
    args    TEXT NOT NULL DEFAULT '',
    url     TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    env     TEXT NOT NULL DEFAULT '',
    auth    TEXT NOT NULL DEFAULT '',
    trust_all INTEGER NOT NULL DEFAULT 0,
    reader_tool TEXT NOT NULL DEFAULT ''
) STRICT;
";

/// A SQLite-backed store implementing the persistence ports over the shared
/// [`Db`] handle.
///
/// Repositories not yet moved to their own bounded context still live here
/// during the Responsibility-Oriented reorg (ADR 0050). Because it holds the
/// shared `Db`, any context repository built over the same handle shares this
/// one connection — the composition root uses [`from_db`](Self::from_db) and
/// [`db`](Self::db) to wire them together.
pub struct SqliteStore {
    db: Db,
}

impl SqliteStore {
    /// The token this node accepts, or `None` before one has been made.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails. Deliberately **not** collapsed into `None` —
    /// a caller cannot tell "no token yet" from "the database is unreadable" without it, and
    /// those two need opposite responses.
    pub fn node_token(&self) -> Result<Option<String>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT token FROM node_auth WHERE id = 0")
            .map_err(backend)?;
        let mut rows = stmt.query([]).map_err(backend)?;
        let Some(row) = rows.next().map_err(backend)? else {
            return Ok(None);
        };
        let token: String = row.get(0).map_err(backend)?;
        Ok((!token.is_empty()).then_some(token))
    }

    /// Stores the token this node will accept from then on.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_node_token(&self, token: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO node_auth (id, token) VALUES (0, ?1)",
                params![token],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// How sign-in is set up: the password hash and the authenticator secret.
    ///
    /// Both empty until somebody has enrolled, which is what the console keys "not set up yet"
    /// from.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn sign_in_setup(&self) -> Result<(String, String), RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT password_hash, totp_secret FROM node_auth WHERE id = 0")
            .map_err(backend)?;
        let mut rows = stmt.query([]).map_err(backend)?;
        let Some(row) = rows.next().map_err(backend)? else {
            return Ok((String::new(), String::new()));
        };
        Ok((row.get(0).map_err(backend)?, row.get(1).map_err(backend)?))
    }

    /// Stores the password hash and authenticator secret.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_sign_in_setup(&self, hash: &str, secret: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            // Upsert, not `UPDATE … WHERE id = 0`: that matches nothing before the row exists
            // and reports success anyway, so enrolment appeared to work and stored nothing.
            .execute(
                "INSERT INTO node_auth (id, token, password_hash, totp_secret) \
                 VALUES (0, '', ?1, ?2) \
                 ON CONFLICT(id) DO UPDATE SET password_hash = ?1, totp_secret = ?2",
                params![hash, secret],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Consecutive failed sign-ins, and when the last one was.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn sign_in_failures(&self) -> Result<(u32, i64), RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT failures, last_failure_ms FROM node_auth WHERE id = 0")
            .map_err(backend)?;
        let mut rows = stmt.query([]).map_err(backend)?;
        let Some(row) = rows.next().map_err(backend)? else {
            return Ok((0, 0));
        };
        let failures: i64 = row.get(0).map_err(backend)?;
        Ok((
            u32::try_from(failures).unwrap_or(u32::MAX),
            row.get(1).map_err(backend)?,
        ))
    }

    /// Records how many sign-ins have failed in a row, and when.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_sign_in_failures(&self, failures: u32, at_ms: i64) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            // Upsert for the same reason: a throttle that silently fails to record a failure
            // is not a throttle.
            .execute(
                "INSERT INTO node_auth (id, token, failures, last_failure_ms) \
                 VALUES (0, '', ?1, ?2) \
                 ON CONFLICT(id) DO UPDATE SET failures = ?1, last_failure_ms = ?2",
                params![i64::from(failures), at_ms],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Hands out a session token, and forgets any older than `keep_for_ms`.
    ///
    /// Pruned on issue rather than on a timer: the only moment the set can grow is the only
    /// moment it needs tidying, and nothing accumulates for anybody to clear.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn add_session(
        &self,
        token: &str,
        now_ms: i64,
        keep_for_ms: i64,
    ) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO node_sessions (token, issued_ms) VALUES (?1, ?2)",
            params![token, now_ms],
        )
        .map_err(backend)?;
        conn.execute(
            "DELETE FROM node_sessions WHERE issued_ms < ?1",
            params![now_ms - keep_for_ms],
        )
        .map_err(backend)?;
        Ok(())
    }

    /// Every session token still within `keep_for_ms` of being issued.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn sessions(&self, now_ms: i64, keep_for_ms: i64) -> Result<Vec<String>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT token FROM node_sessions WHERE issued_ms >= ?1")
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![now_ms - keep_for_ms], |r| r.get::<_, String>(0))
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }

    /// Opens (creating if needed) a store at `path` and applies the schema.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be opened or migrated.
    pub fn open(path: &str) -> Result<Self, RepositoryError> {
        Self::from_db(Db::open(path)?)
    }

    /// Opens a private in-memory store, mainly for tests.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be created or migrated.
    pub fn open_in_memory() -> Result<Self, RepositoryError> {
        Self::from_db(Db::open_in_memory()?)
    }

    /// Builds a store over an existing shared [`Db`], applying its schema. The
    /// composition root uses this to share one connection across contexts.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the schema cannot be applied.
    pub fn from_db(db: Db) -> Result<Self, RepositoryError> {
        {
            let conn = db.lock()?;
            // Drop the goal-tracker schema before creating anything, so an existing
            // database sheds it rather than carrying dead tables forever.
            drop_goal_tracker(&conn)?;
            conn.execute_batch(SCHEMA).map_err(backend)?;
            // What sort of change a config write was (ADR 0054). Existing rows are all
            // name changes, which is what the default says — and adding it here as well
            // as in SCHEMA is the point: `CREATE TABLE IF NOT EXISTS` does nothing to a
            // table that already exists, which has now shipped two live bugs.
            ensure_column(
                &conn,
                "config_writes",
                "kind",
                "TEXT NOT NULL DEFAULT 'name'",
            )?;
            // Per-capability irreversible-band opener (ADR 0051); existing rows
            // default to closed (0) — the un-undoable stays blocked until opened.
            ensure_column(
                &conn,
                "capability_config",
                "open_irreversible",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            // Per-skill "ask first" override (on with user input): the skill runs
            // only after the person confirms each use, regardless of its band.
            ensure_column(
                &conn,
                "capability_config",
                "confirm",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            // Per-MCP-server credentials: a child-process environment (stdio) and a
            // bearer token (http). Secrets — stored, never returned by the API.
            // `CREATE TABLE IF NOT EXISTS` never adds a column to a table that already
            // exists, so a new column on an existing install arrives only here.
            ensure_column(
                &conn,
                "deep_model",
                "escalate",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            ensure_column(&conn, "mcp_servers", "env", "TEXT NOT NULL DEFAULT ''")?;
            ensure_column(&conn, "mcp_servers", "auth", "TEXT NOT NULL DEFAULT ''")?;
            // Auto-allow a server's tools on connect (default on). Opened tools stay
            // Block→Confirm, so this never drops the ask-before-each-use safety net.
            ensure_column(
                &conn,
                "mcp_servers",
                "trust_all",
                "INTEGER NOT NULL DEFAULT 1",
            )?;
            // Which tool reads this server's state (ADR 0054). An existing database
            // needs the column added here: `SCHEMA` above is CREATE TABLE IF NOT
            // EXISTS, which is a no-op once the table is there, so a new column on an
            // OLD table only ever arrives through `ensure_column`. Missing it broke
            // every MCP server read on an existing deployment — and because
            // `connect_mcp` swallows the error with `unwrap_or_default`, the visible
            // symptom was every tool silently vanishing rather than an error.
            ensure_column(
                &conn,
                "mcp_servers",
                "reader_tool",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            // Whether an action actually moved the world (ADR 0054). Existing rows read
            // back NULL — nothing to compare — which is the honest default.
            ensure_column(&conn, "outcomes", "changed", "INTEGER")?;
            // Sign-in with a password and an authenticator code. `node_auth` shipped hours
            // earlier holding only a token, and `CREATE TABLE IF NOT EXISTS` does nothing to a
            // table that already exists — the trap this repository has already fallen into
            // twice, both times shipping a green suite and a broken endpoint.
            ensure_column(
                &conn,
                "node_auth",
                "password_hash",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(
                &conn,
                "node_auth",
                "totp_secret",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(&conn, "node_auth", "failures", "INTEGER NOT NULL DEFAULT 0")?;
            ensure_column(
                &conn,
                "node_auth",
                "last_failure_ms",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        Ok(Self { db })
    }

    /// The shared connection handle, for building sibling context repositories
    /// over the same underlying connection.
    #[must_use]
    pub fn db(&self) -> Db {
        self.db.clone()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, RepositoryError> {
        self.db.lock()
    }
}

// The eight direction repositories are implemented by the direction context's
// DirectionStore over the shared Db now (ADR 0050); the all_*/parse_* helpers
// below remain for MemoryStore's export.

impl MemoryStore for SqliteStore {
    fn export(&self) -> Result<MemorySnapshot, RepositoryError> {
        let conn = self.lock()?;
        Ok(MemorySnapshot {
            audit: all_audit(&conn)?,
            messages: all_messages(&conn)?,
            preferences: all_preferences(&conn)?,
            beliefs: all_beliefs(&conn)?,
            outcomes: all_outcomes(&conn)?,
            intentions: all_intentions(&conn)?,
        })
    }

    fn purge(&self) -> Result<(), RepositoryError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        // `config_writes` is deliberately NOT purged (ADR 0054). It is not knowledge about
        // the person; it is a receipt for changes that still exist inside somebody else's
        // service. Deleting the receipt does not undo the change — it only makes the
        // change unrecoverable and invisible, which is strictly worse for the person than
        // keeping it.
        //
        // Delete children before parents so foreign keys stay satisfied.
        for table in [
            "audit_log",
            "messages",
            "message_actions",
            "conversation_summary",
            "preferences",
            "checkin",
            "beliefs",
            "outcomes",
            "target_aliases",
            "intentions",
            "events",
            "capability_config",
            "capability_settings",
            "autonomy_envelope",
            "brief_schedule",
            "deep_model",
        ] {
            tx.execute(&format!("DELETE FROM {table}"), [])
                .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }
}

// AuditLog and EventLog are implemented by the platform context's AuditStore and
// EventStore over the shared Db now (ADR 0050); `all_audit` stays for MemoryStore.

fn all_preferences(conn: &Connection) -> Result<Vec<Preference>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, body, kind, at_ms FROM preferences ORDER BY at_ms, rowid")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, body, kind, at_ms) = row.map_err(backend)?;
        let kind = PreferenceKind::from_name(&kind)
            .ok_or_else(|| RepositoryError::Corrupt(format!("unknown preference kind {kind:?}")))?;
        out.push(
            Preference::new(
                PreferenceId::new(parse_id(&id)?),
                &body,
                kind,
                Timestamp::from_unix_millis(at_ms),
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

fn all_beliefs(conn: &Connection) -> Result<Vec<Belief>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, statement, kind, confidence, evidence, created_ms, last_affirmed_ms, status \
             FROM beliefs ORDER BY last_affirmed_ms DESC, rowid DESC",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, statement, kind, confidence, evidence, created_ms, affirmed_ms, status) =
            row.map_err(backend)?;
        out.push(Belief::from_parts(
            BeliefId::new(parse_id(&id)?),
            statement,
            BeliefKind::from_name(&kind),
            Confidence::from_name(&confidence),
            evidence,
            Timestamp::from_unix_millis(created_ms),
            Timestamp::from_unix_millis(affirmed_ms),
            BeliefStatus::from_name(&status),
        ));
    }
    Ok(out)
}

/// Reads every intention for the memory export (ADR 0052) — what Endora has pursued.
fn all_intentions(conn: &Connection) -> Result<Vec<Intention>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, statement, motivating_belief, note, state, created_ms, \
             last_progressed_ms, steps_taken \
             FROM intentions ORDER BY last_progressed_ms DESC, rowid DESC",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, u32>(7)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, statement, belief, note, state, created_ms, progressed_ms, steps) =
            row.map_err(backend)?;
        out.push(Intention::from_parts(
            IntentionId::new(parse_id(&id)?),
            statement,
            BeliefId::new(parse_id(&belief)?),
            note,
            IntentionState::from_name(&state),
            Timestamp::from_unix_millis(created_ms),
            Timestamp::from_unix_millis(progressed_ms),
            steps,
        ));
    }
    Ok(out)
}

/// Reads every outcome for the memory export (ADR 0053) — what Endora did, what the
/// tool claimed, and what Endora observed afterwards.
fn all_outcomes(conn: &Connection) -> Result<Vec<Outcome>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, capability, input, claim, observation, at_ms, motivating_belief, reaction, \
             changed \
             FROM outcomes ORDER BY at_ms DESC, rowid DESC",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<i64>>(8)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, capability, input, claim, observation, at_ms, belief, reaction, changed) =
            row.map_err(backend)?;
        let reaction = reaction
            .map(|r| {
                Reaction::from_name(&r)
                    .ok_or_else(|| RepositoryError::Corrupt(format!("unknown reaction {r:?}")))
            })
            .transpose()?;
        let motivating_belief = belief
            .map(|b| parse_id(&b).map(BeliefId::new))
            .transpose()?;
        out.push(Outcome::from_parts(
            OutcomeId::new(parse_id(&id)?),
            capability,
            input,
            claim,
            observation,
            Timestamp::from_unix_millis(at_ms),
            motivating_belief,
            reaction,
            changed.map(|c| c != 0),
        ));
    }
    Ok(out)
}

/// Serializes a proposal to its stored `(kind, payload-json)` form.
fn all_messages(conn: &Connection) -> Result<Vec<ChatMessage>, RepositoryError> {
    // Insertion order (rowid) breaks ties so same-millisecond turns keep their
    // real order — ids are random and cannot be a tiebreak.
    let mut stmt = conn
        .prepare("SELECT id, role, body, at_ms FROM messages ORDER BY at_ms, rowid")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, role, body, at_ms) = row.map_err(backend)?;
        let role = MessageRole::from_name(&role)
            .ok_or_else(|| RepositoryError::Corrupt(format!("unknown message role {role:?}")))?;
        out.push(
            ChatMessage::new(
                MessageId::new(parse_id(&id)?),
                role,
                &body,
                Timestamp::from_unix_millis(at_ms),
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

/// Parses a stored identifier back into a `u128`, or reports corruption.
fn parse_id(text: &str) -> Result<u128, RepositoryError> {
    text.parse::<u128>()
        .map_err(|e| RepositoryError::Corrupt(format!("invalid stored id {text:?}: {e}")))
}

fn all_audit(conn: &Connection) -> Result<Vec<AuditRecord>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, at_ms, summary FROM audit_log ORDER BY at_ms DESC, id DESC")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, at_ms, summary) = row.map_err(backend)?;
        out.push(
            AuditRecord::new(
                AuditId::new(parse_id(&id)?),
                Timestamp::from_unix_millis(at_ms),
                &summary,
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

/// Loads a reflection's evidence observation ids, in stored order.
fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> Result<(), RepositoryError> {
    let present: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
            params![table, column],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if present == 0 {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
            [],
        )
        .map_err(backend)?;
    }
    Ok(())
}

/// Drops the goal-tracker schema from an existing database.
///
/// Endora was, for a while, a goal tracker: the person created
/// Direction→Target→Assumption→Experiment→Observation→Reflection and the model
/// summarised afterwards. That inverted the point — it is Endora's job to do the
/// thinking — and the machinery is gone (see the direction reset, ADR 0052, and
/// the ADR that made it final). This sheds the tables rather than leaving an
/// existing database carrying data nothing can read or delete, which would put it
/// outside the memory rights in constitution §6.
///
/// Children are dropped before parents so foreign keys stay satisfied. A no-op on
/// a fresh database.
fn drop_goal_tracker(conn: &Connection) -> Result<(), RepositoryError> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_experiments_review;
         DROP INDEX IF EXISTS idx_goals_direction;
         DROP TABLE IF EXISTS reflection_evidence;
         DROP TABLE IF EXISTS process_changes;
         DROP TABLE IF EXISTS reflections;
         DROP TABLE IF EXISTS observations;
         DROP TABLE IF EXISTS experiments;
         DROP TABLE IF EXISTS assumptions;
         DROP TABLE IF EXISTS targets;
         DROP TABLE IF EXISTS goals;
         DROP TABLE IF EXISTS directions;
         DROP TABLE IF EXISTS \"values\";
         DROP TABLE IF EXISTS suggestions;
         DROP TABLE IF EXISTS attention_snoozes;",
    )
    .map_err(backend)
}

/// Maps any backend error into a [`RepositoryError::Backend`].
fn backend(error: impl core::fmt::Display) -> RepositoryError {
    RepositoryError::Backend(error.to_string())
}

/// Maps a domain reconstruction failure into a [`RepositoryError::Corrupt`].
fn corrupt(error: impl core::fmt::Display) -> RepositoryError {
    RepositoryError::Corrupt(error.to_string())
}

#[cfg(test)]
mod tests {
    // Callers depend on a *port*, not the concrete store: `SqliteStore`
    // implements several repository traits, so we exercise it through the trait
    // (as real use cases do) rather than calling ambiguous methods directly.
    use super::SqliteStore;
    use endora_capabilities::ConfigStore;

    fn store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    /// The direction repositories over the store's shared connection (ADR 0050).
    fn cfg_store(store: &SqliteStore) -> ConfigStore {
        ConfigStore::new(store.db())
    }

    /// The platform audit trail over the store's shared connection (ADR 0050).
    fn audit_store(store: &SqliteStore) -> endora_platform::AuditStore {
        endora_platform::AuditStore::new(store.db())
    }

    /// The platform event log over the store's shared connection (ADR 0050).
    fn event_store(store: &SqliteStore) -> endora_platform::EventStore {
        endora_platform::EventStore::new(store.db())
    }

    #[test]
    fn opening_a_database_created_before_a_column_existed_still_works() {
        // The upgrade path, which nothing covered — and it broke a live deployment.
        // `SCHEMA` is CREATE TABLE IF NOT EXISTS, so on an EXISTING table it does
        // nothing, and a newly added column only ever arrives via `ensure_column`.
        // Tests that build a fresh store never exercise that, because the fresh
        // CREATE TABLE already has every column.
        use endora_capabilities::{McpServer, McpServerRegistry};
        let db = endora_persistence::Db::open_in_memory().unwrap();
        // An `mcp_servers` table as an older Endora left it: no `reader_tool`.
        db.lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE mcp_servers (
                    name    TEXT PRIMARY KEY,
                    kind    TEXT NOT NULL,
                    command TEXT NOT NULL DEFAULT '',
                    args    TEXT NOT NULL DEFAULT '',
                    url     TEXT NOT NULL DEFAULT '',
                    enabled INTEGER NOT NULL DEFAULT 1
                ) STRICT;
                 INSERT INTO mcp_servers (name, kind, command) VALUES ('home', 'stdio', 'x');",
            )
            .unwrap();

        // Opening the store must migrate it rather than leave a table the queries
        // cannot read.
        let store = SqliteStore::from_db(db.clone()).unwrap();
        let config = endora_capabilities::ConfigStore::new(store.db());

        let servers = McpServerRegistry::list(&config).expect("an upgraded database reads back");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "home");
        assert_eq!(
            servers[0].reader_tool, "",
            "nobody has nominated one yet, which is the honest default"
        );

        // And the new column is writable, not just readable.
        let updated = McpServer::stdio("home", "x", Vec::new())
            .unwrap()
            .with_reader("GetLiveContext");
        McpServerRegistry::register(&config, &updated).unwrap();
        assert_eq!(
            McpServerRegistry::list(&config).unwrap()[0].reader_tool,
            "GetLiveContext"
        );
    }

    #[test]
    fn export_captures_everything_and_purge_clears_it() {
        use endora_application::{AuditId, AuditRecord, Timestamp};
        use endora_application::{AuditLog, MemoryStore};

        let store = store();
        (&audit_store(&store) as &dyn AuditLog)
            .append(
                &AuditRecord::new(AuditId::new(8), Timestamp::from_unix_millis(9), "noted")
                    .unwrap(),
            )
            .unwrap();

        let snapshot = (&store as &dyn MemoryStore).export().unwrap();
        assert_eq!(snapshot.audit.len(), 1);

        (&store as &dyn MemoryStore).purge().unwrap();
        let empty = (&store as &dyn MemoryStore).export().unwrap();
        assert_eq!(empty, endora_application::MemorySnapshot::default());
    }

    #[test]
    fn audit_records_append_and_read_newest_first() {
        use endora_application::AuditLog;
        use endora_application::{AuditId, AuditRecord, Timestamp};

        let store = store();

        let aud = audit_store(&store);
        let log: &dyn AuditLog = &aud;
        log.append(
            &AuditRecord::new(AuditId::new(1), Timestamp::from_unix_millis(100), "first").unwrap(),
        )
        .unwrap();
        log.append(
            &AuditRecord::new(AuditId::new(2), Timestamp::from_unix_millis(200), "second").unwrap(),
        )
        .unwrap();

        let recent = log.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].summary(), "second");
        assert_eq!(recent[1].summary(), "first");
        assert_eq!(log.recent(1).unwrap().len(), 1);
    }

    #[test]
    fn capability_settings_round_trip_and_upsert() {
        use endora_application::CapabilitySettingsRepository;
        let store = store();
        let cfg = cfg_store(&store);
        let repo: &dyn CapabilitySettingsRepository = &cfg;
        assert!(repo.all_settings().unwrap().is_empty());
        repo.set_setting("image_review", "model", "moondream")
            .unwrap();
        repo.set_setting("image_review", "model", "llava").unwrap(); // upsert
        let all = repo.all_settings().unwrap();
        assert_eq!(
            all,
            vec![(
                "image_review".to_owned(),
                "model".to_owned(),
                "llava".to_owned()
            )]
        );
    }

    #[test]
    fn events_append_and_read_newest_first() {
        use endora_application::EventLog;
        use endora_application::Timestamp;
        let store = store();
        let evt = event_store(&store);
        let log: &dyn EventLog = &evt;
        log.record(Timestamp::from_unix_millis(100), "Used the weather skill")
            .unwrap();
        log.record(Timestamp::from_unix_millis(200), "Turned news off")
            .unwrap();

        let recent = log.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].summary, "Turned news off");
        assert_eq!(recent[1].summary, "Used the weather skill");
        assert_eq!(log.recent(1).unwrap().len(), 1);
    }

    #[test]
    fn deep_model_config_round_trips() {
        use endora_application::{DeepModel, DeepModelRepository};
        let store = store();
        let cfg = cfg_store(&store);
        let repo: &dyn DeepModelRepository = &cfg;
        assert!(repo.get().unwrap().is_none());
        // Configuring a deep model must NOT enable the automatic fallback: it is reached
        // only when the person presses a button until they say otherwise (ADR 0055).
        let cfg = DeepModel {
            url: "https://api.x.com/v1".to_owned(),
            model: "big-1".to_owned(),
            api_key: "secret".to_owned(),
            escalate: false,
        };
        repo.set(&cfg).unwrap();
        assert_eq!(repo.get().unwrap(), Some(cfg.clone()));
        assert!(
            !repo.get().unwrap().unwrap().escalate,
            "phoning out must never be on by default"
        );

        let opted_in = DeepModel {
            escalate: true,
            ..cfg
        };
        repo.set(&opted_in).unwrap();
        assert_eq!(repo.get().unwrap(), Some(opted_in));
    }

    #[test]
    fn model_tune_schedule_defaults_off_then_round_trips() {
        use endora_application::{ModelTuneSchedule, ModelTuneScheduleRepository};
        let store = store();
        let cfg = cfg_store(&store);
        let repo: &dyn ModelTuneScheduleRepository = &cfg;
        // Unset ⇒ off.
        assert_eq!(repo.get().unwrap(), ModelTuneSchedule::disabled_default());
        let sched = ModelTuneSchedule {
            enabled: true,
            hour_utc: 3,
            last_ms: 123,
        };
        repo.set(&sched).unwrap();
        assert_eq!(repo.get().unwrap(), sched);
        // is_due: on, at hour 3 UTC, and >20h since last run (day 1, hour 3).
        let hour = 3_600_000_i64;
        assert!(sched.is_due((24 + 3) * hour)); // hour 3, ~27h since last ⇒ due
        assert!(!sched.is_due((24 + 4) * hour)); // hour 4 ⇒ wrong hour, not due
    }

    #[test]
    fn butler_model_config_round_trips() {
        use endora_application::{
            ButlerModelConfig, ButlerModelConfigRepository, ModelSlot, Sampling,
        };
        let store = store();
        let cfg_store = cfg_store(&store);
        let repo: &dyn ButlerModelConfigRepository = &cfg_store;
        assert!(repo.get().unwrap().is_none());
        let config = ButlerModelConfig {
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            api_key: "secret".to_owned(),
            mixture: true,
            single: ModelSlot::default(),
            router: ModelSlot {
                model: "anthropic/claude-3.5-haiku".to_owned(),
                sampling: Sampling {
                    temperature: Some(0.1),
                    top_p: None,
                    top_k: Some(20),
                    repeat_penalty: None,
                },
            },
            synth: ModelSlot {
                model: "anthropic/claude-sonnet-5".to_owned(),
                sampling: Sampling {
                    temperature: Some(0.6),
                    ..Sampling::default()
                },
            },
        };
        repo.set(&config).unwrap();
        // Every field, including the nullable sampling knobs, round-trips exactly.
        assert_eq!(repo.get().unwrap(), Some(config));
    }

    #[test]
    fn autonomy_envelope_defaults_then_round_trips() {
        use endora_application::{AutonomyEnvelope, AutonomyEnvelopeRepository};
        let store = store();
        let cfg = cfg_store(&store);
        let repo: &dyn AutonomyEnvelopeRepository = &cfg;

        // Unset ⇒ the default posture (external ok, consequential no).
        assert_eq!(repo.get().unwrap(), AutonomyEnvelope::default());

        let widened = AutonomyEnvelope {
            auto_external: false,
            auto_consequential: true,
        };
        repo.set(&widened).unwrap();
        assert_eq!(repo.get().unwrap(), widened);
    }

    #[test]
    fn capability_enable_overrides_round_trip() {
        use endora_application::CapabilityConfigRepository;
        let store = store();
        let cfg = cfg_store(&store);
        let repo: &dyn CapabilityConfigRepository = &cfg;

        // No overrides to start.
        assert!(repo.enabled_overrides().unwrap().is_empty());

        // Set two, then flip one; the store keeps the latest per id.
        repo.set_enabled("weather", true).unwrap();
        repo.set_enabled("flights", false).unwrap();
        repo.set_enabled("weather", false).unwrap();

        let mut got = repo.enabled_overrides().unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![("flights".to_owned(), false), ("weather".to_owned(), false)]
        );
    }

    #[test]
    fn a_large_u128_id_survives_storage() {
        use endora_application::{
            Belief, BeliefId, BeliefKind, BeliefRepository, Confidence, Timestamp,
        };
        let store = store();
        let understanding = endora_understanding::UnderstandingStore::new(store.db());
        let repo: &dyn BeliefRepository = &understanding;
        let big = u128::MAX;
        repo.save(
            &Belief::new(
                BeliefId::new(big),
                "you like edge cases",
                BeliefKind::Other,
                Confidence::Low,
                "this test",
                Timestamp::from_unix_millis(1),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            repo.get(BeliefId::new(big)).unwrap().unwrap().id(),
            BeliefId::new(big)
        );
    }
}

#[cfg(test)]
mod the_schema_production_actually_runs {
    use super::SqliteStore;
    use endora_capabilities::{
        ConfigStore, ConfigWrite, ConfigWriteLog, StandingTrouble, StandingTroubleRepository,
        TargetAlias, TargetAliasRepository, WriteKind,
    };

    /// Builds the stores exactly as `main.rs` does: one production [`SqliteStore`], with the
    /// context stores layered over **its** `Db` handle. No test migration anywhere.
    fn as_production_does() -> ConfigStore {
        let store = SqliteStore::open_in_memory().expect("open the production store");
        ConfigStore::new(store.db())
    }

    #[test]
    fn every_table_the_api_writes_to_exists_in_the_production_schema() {
        // Twice now, a table has existed only in `endora_capabilities::migrate` — which is
        // called by tests and by nothing else. Both times every unit test passed and the
        // live endpoint answered "internal error", because the tests migrated a schema
        // production never runs.
        //
        // This test refuses that shortcut: it builds the stores the way the composition
        // root does and then actually writes through every repository the HTTP layer
        // exposes. A table missing from the production schema fails here instead of in
        // somebody's house.
        let config = as_production_does();

        let write = ConfigWrite {
            id: 1,
            at_ms: 1_000,
            server: "house".to_owned(),
            target: "light.kitchen".to_owned(),
            added: "kitchen main".to_owned(),
            was: vec!["Kitchen".to_owned()],
            undone: false,
            kind: WriteKind::Name,
        };
        ConfigWriteLog::record(&config, &write).expect("config_writes");
        assert_eq!(
            ConfigWriteLog::writes(&config, 10)
                .expect("read back")
                .len(),
            1
        );
        ConfigWriteLog::mark_undone(&config, 1).expect("undo a write");

        config
            .set_alias(&TargetAlias::new("house", "the lamp", "Living Room Lamp").unwrap())
            .expect("target_aliases");
        assert_eq!(config.aliases().expect("read back").len(), 1);

        config
            .note_trouble(&StandingTrouble {
                server: "house".to_owned(),
                thing: "Living Room Lamp".to_owned(),
                trouble: "unavailable".to_owned(),
                since_ms: 1_000,
                accepted: false,
            })
            .expect("standing_trouble");
        assert_eq!(config.troubles().expect("read back").len(), 1);
        config
            .accept_trouble("house", "Living Room Lamp")
            .expect("accept");
        config
            .clear_trouble("house", "Living Room Lamp")
            .expect("clear");
    }

    #[test]
    fn the_transition_log_works_on_the_schema_production_runs() {
        // Two more tables that exist in `endora_capabilities::migrate` — the shortcut that has
        // now twice shipped a green suite and an "internal error" in the person's house. The
        // watch loop writes to these every two minutes, so a divergence here would be silent
        // until somebody read the log and found it empty (ADR 0058).
        use endora_capabilities::{Transition, TransitionLog, Watched};

        let store = SqliteStore::open_in_memory().expect("open the production store");
        let config = ConfigStore::new(store.db());

        TransitionLog::remember(
            &config,
            &Watched {
                key: "house::person.morgan".to_owned(),
                settled: "home".to_owned(),
                candidate: "home".to_owned(),
                candidate_since_ms: 1_000,
            },
        )
        .expect("watched_things");
        assert_eq!(
            TransitionLog::watching(&config).expect("read back").len(),
            1
        );

        TransitionLog::record(
            &config,
            &Transition {
                key: "house::person.morgan".to_owned(),
                from: "home".to_owned(),
                to: "not_home".to_owned(),
                at_ms: 2_000,
            },
        )
        .expect("transitions");
        assert_eq!(
            TransitionLog::since(&config, 0).expect("read back").len(),
            1
        );

        TransitionLog::forget_before(&config, 10_000).expect("prune");
        assert!(
            TransitionLog::since(&config, 0)
                .expect("read back")
                .is_empty(),
            "the log forgets on its own"
        );
    }

    #[test]
    fn a_notion_can_be_stored_by_the_schema_production_runs() {
        // `endora_understanding::migrate` is the same shortcut the test above was written
        // about: called by that crate's own tests and by nothing else. Notions live in two
        // tables, so this is exactly the shape that has twice shipped a green suite and an
        // "internal error" in the person's house — and the second table, holding the
        // evidence, is the one whose absence would silently turn every notion into an
        // unfounded statement about them (ADR 0057).
        use endora_application::{Citation, Notion, NotionId, NotionRepository, Source, Timestamp};

        let store = SqliteStore::open_in_memory().expect("open the production store");
        let understanding = endora_understanding::UnderstandingStore::new(store.db());

        let notion = Notion::new(
            NotionId::new(1),
            "the Monday gym block gets cancelled",
            vec![
                Citation::new(Source::Outcome, "outcome-7").unwrap(),
                Citation::new(Source::Message, "msg-2").unwrap(),
            ],
            "whether next Monday's block survives",
            Timestamp::from_unix_millis(1_000),
        )
        .unwrap();
        NotionRepository::save(&understanding, &notion).expect("notions");

        let back = NotionRepository::open(&understanding).expect("read back");
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].support_count(), 2, "notion_citations");
    }
}
