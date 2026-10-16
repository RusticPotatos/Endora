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
    Intention, IntentionId, IntentionOrigin, IntentionState, MessageId, MessageRole, Outcome,
    OutcomeId, Preference, PreferenceId, PreferenceKind, Reaction, ResearchAssignment, Timestamp,
};
use endora_application::{
    ExecutionLeaseRepository, MemorySnapshot, MemoryStore, RepositoryError, ResearchDelivery,
    ResearchRepository,
};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS terminal_operations (
    id TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    result TEXT NOT NULL,
    created_ms INTEGER NOT NULL
) STRICT;
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
    motivating_belief  TEXT,
    origin             TEXT NOT NULL DEFAULT 'belief',
    request_message_id TEXT,
    note               TEXT NOT NULL,
    state              TEXT NOT NULL,
    created_ms         INTEGER NOT NULL,
    last_progressed_ms INTEGER NOT NULL,
    steps_taken        INTEGER NOT NULL,
    CHECK ((origin = 'belief' AND motivating_belief IS NOT NULL AND request_message_id IS NULL)
        OR (origin = 'user_request' AND motivating_belief IS NULL AND request_message_id IS NOT NULL))
) STRICT;
-- One durable lease table is shared by every resumable worker. The execution
-- key is namespaced by its owner (`research:`), so domains retain their own
-- checkpoints while worker ownership follows one contract.
CREATE TABLE IF NOT EXISTS execution_leases (
    execution_key TEXT PRIMARY KEY,
    attempts INTEGER NOT NULL DEFAULT 0,
    lease_until_ms INTEGER NOT NULL DEFAULT 0
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
    confirm           INTEGER NOT NULL DEFAULT 0,
    stance            TEXT NOT NULL DEFAULT ''
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
    synth_repeat  REAL,
    preferred_url   TEXT NOT NULL DEFAULT '',
    preferred_model TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE TABLE IF NOT EXISTS model_usage (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    at_ms      INTEGER NOT NULL,
    role       TEXT NOT NULL,
    model      TEXT NOT NULL,
    prompt     INTEGER NOT NULL,
    completion INTEGER NOT NULL,
    cached     INTEGER NOT NULL
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
-- standing_trouble (ADR 0056, devices that stopped answering) is no longer created or used
-- (2026-10-16). A database made before keeps the table, untouched.
-- The person's own capabilities (ADR 0071) — five fields, validated at authoring
-- time, never code.
CREATE TABLE IF NOT EXISTS recipes (
    id          TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    inputs      TEXT NOT NULL,
    get_url     TEXT NOT NULL,
    say         TEXT NOT NULL
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
    last_failure_ms INTEGER NOT NULL DEFAULT 0,
    -- A replacement authenticator secret, waiting for a code that proves it was really
    -- scanned. The live one keeps working until then, so a half-finished swap cannot lock
    -- somebody out of their own house.
    pending_totp    TEXT NOT NULL DEFAULT ''
) STRICT;
-- Tokens handed out by a successful sign-in. Separate from the bootstrap token on purpose:
-- that one is the recovery path and should not be what a password buys, and these can be
-- thrown away without changing it.
CREATE TABLE IF NOT EXISTS node_sessions (
    token     TEXT PRIMARY KEY,
    issued_ms INTEGER NOT NULL,
    -- 'full' for the console, 'checks' for the smoke suite's read-only credential.
    scope     TEXT NOT NULL DEFAULT 'full'
) STRICT;
-- Questions the person answered by asking not to be asked again. Answering is the
-- dismissal (ADR 0052), and somebody who does not know a real name still needs an answer.
CREATE TABLE IF NOT EXISTS repairs_left_alone (
    server TEXT NOT NULL,
    thing  TEXT NOT NULL,
    PRIMARY KEY (server, thing)
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
    /// Atomically records an explicit research commission and its user message.
    #[expect(
        clippy::too_many_arguments,
        reason = "the transaction receives the complete immutable commission"
    )]
    pub fn commission_research(
        &self,
        intention_id: IntentionId,
        message_id: MessageId,
        request_key: &str,
        assignment: &str,
        source_urls_json: &str,
        search_capability_id: Option<&str>,
        at: Timestamp,
    ) -> Result<ResearchAssignment, RepositoryError> {
        if assignment.trim().is_empty() || assignment.chars().count() > 4_000 {
            return Err(RepositoryError::Corrupt(
                "research assignment must be 1–4000 characters".to_owned(),
            ));
        }
        if request_key.trim().is_empty() || request_key.chars().count() > 200 {
            return Err(RepositoryError::Corrupt(
                "invalid research request key".to_owned(),
            ));
        }
        let work = ResearchAssignment::new(
            intention_id,
            assignment,
            request_key,
            source_urls_json,
            search_capability_id.map(str::to_owned),
            at,
        )
        .map_err(corrupt)?;
        let mut conn = self.lock()?;
        let existing: Option<(String, String, String, Option<String>)> = conn
            .query_row(
                "SELECT intention_id, assignment, source_urls_json, search_capability_id
             FROM intention_research WHERE request_key=?1",
                params![request_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((id, prior_assignment, prior_urls, prior_search)) = existing {
            if prior_assignment != assignment.trim()
                || prior_urls != source_urls_json
                || prior_search.as_deref() != search_capability_id
            {
                return Err(RepositoryError::Backend(
                    "research request key was reused with different input".to_owned(),
                ));
            }
            drop(conn);
            let existing_id = IntentionId::new(parse_id(&id)?);
            return endora_understanding::UnderstandingStore::new(self.db())
                .research(existing_id)?
                .ok_or_else(|| {
                    RepositoryError::Corrupt("research request lost its work row".to_owned())
                });
        }
        let tx = conn.transaction().map_err(backend)?;
        let active: i64 = tx.query_row(
            "SELECT COUNT(*) FROM intention_research WHERE status IN ('queued','running','retry_wait')",
            [], |r| r.get(0),
        ).map_err(backend)?;
        if active > 0 {
            return Err(RepositoryError::Backend(
                "another research assignment is active".to_owned(),
            ));
        }
        tx.execute(
            "INSERT INTO messages (id, role, body, at_ms) VALUES (?1,'user',?2,?3)",
            params![
                message_id.value().to_string(),
                assignment.trim(),
                at.unix_millis()
            ],
        )
        .map_err(backend)?;
        tx.execute(
            "INSERT INTO intentions (id,statement,motivating_belief,origin,request_message_id,note,state,created_ms,last_progressed_ms,steps_taken)
             VALUES (?1,?2,NULL,'user_request',?3,'','active',?4,?4,0)",
            params![intention_id.value().to_string(), assignment.trim(), message_id.value().to_string(), at.unix_millis()],
        ).map_err(backend)?;
        tx.execute(
            "INSERT INTO intention_research
             (intention_id,assignment,request_key,source_urls_json,search_capability_id,status,phase,
              reason_code,reason,checkpoint,model_calls_used,read_attempts_used,created_ms,updated_ms,expires_ms)
             VALUES (?1,?2,?3,?4,?5,'queued','gathering','','',?6,0,0,?7,?7,?8)",
            params![intention_id.value().to_string(), assignment.trim(), request_key,
                source_urls_json, search_capability_id, work.checkpoint(), at.unix_millis(),
                work.expires_at().unix_millis()],
        ).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(work)
    }

    /// Atomically cancels commissioned research and abandons its parent intention.
    pub fn cancel_research(
        &self,
        intention_id: IntentionId,
        at: Timestamp,
    ) -> Result<ResearchAssignment, RepositoryError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM intention_research WHERE intention_id=?1",
                params![intention_id.value().to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(status) = status else {
            return Err(RepositoryError::Backend("research not found".to_owned()));
        };
        if status != "cancelled"
            && matches!(
                status.as_str(),
                "delivered" | "blocked" | "budget_exhausted" | "failed"
            )
        {
            return Err(RepositoryError::Backend(
                "research is already terminal".to_owned(),
            ));
        }
        if status != "cancelled" {
            tx.execute(
                "UPDATE intention_research SET status='cancelled', reason_code='cancelled',
                 reason='You cancelled this research assignment.', updated_ms=?2
                 WHERE intention_id=?1",
                params![intention_id.value().to_string(), at.unix_millis()],
            )
            .map_err(backend)?;
            tx.execute(
                "UPDATE intentions SET state='abandoned', last_progressed_ms=?2 WHERE id=?1",
                params![intention_id.value().to_string(), at.unix_millis()],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        drop(conn);
        endora_understanding::UnderstandingStore::new(self.db())
            .research(intention_id)?
            .ok_or_else(|| RepositoryError::Corrupt("cancelled research disappeared".to_owned()))
    }

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

    /// The replacement authenticator secret waiting to be confirmed, if any.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn pending_authenticator(&self) -> Result<String, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT pending_totp FROM node_auth WHERE id = 0")
            .map_err(backend)?;
        let mut rows = stmt.query([]).map_err(backend)?;
        let Some(row) = rows.next().map_err(backend)? else {
            return Ok(String::new());
        };
        row.get(0).map_err(backend)
    }

    /// Puts a replacement authenticator secret aside, or clears it with an empty string.
    ///
    /// **Never touches the live secret.** That is the whole point: until a code proves the new
    /// one was actually scanned, the old phone keeps working.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_pending_authenticator(&self, secret: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT INTO node_auth (id, token, pending_totp) VALUES (0, '', ?1) \
                 ON CONFLICT(id) DO UPDATE SET pending_totp = ?1",
                params![secret],
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
        scope: &str,
        now_ms: i64,
        keep_for_ms: i64,
    ) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO node_sessions (token, issued_ms, scope) VALUES (?1, ?2, ?3)",
            params![token, now_ms, scope],
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
    pub fn sessions(
        &self,
        now_ms: i64,
        keep_for_ms: i64,
    ) -> Result<Vec<(String, String)>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT token, scope FROM node_sessions WHERE issued_ms >= ?1")
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![now_ms - keep_for_ms], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
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
            drop_the_clean_slate(&conn)?;
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
            renamed_capability(&conn, "local_events", "city_meetings")?;
            // Eight permission axes fold into one stance (ADR 0062). The fold errs toward
            // `ask`, the recoverable direction: a tool that asks when it could act is a tap;
            // one that acts when it should ask is the reason the axes existed.
            ensure_column(
                &conn,
                "capability_config",
                "stance",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            conn.execute_batch(
                "UPDATE capability_config SET stance = CASE \
                     WHEN stance != '' THEN stance \
                     WHEN enabled = 0 THEN 'off' \
                     WHEN confirm = 1 THEN 'ask' \
                     WHEN open_irreversible = 1 THEN 'ask' \
                     ELSE '' END;",
            )
            .map_err(backend)?;
            ensure_column(
                &conn,
                "deep_model",
                "escalate",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            // The preferred base rung (a bigger model on a sometimes-away machine). Added
            // here, not only in the capabilities test migration: without it every database
            // older than the feature failed to read its model config at all.
            ensure_column(
                &conn,
                "butler_model_config",
                "preferred_url",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(
                &conn,
                "butler_model_config",
                "preferred_model",
                "TEXT NOT NULL DEFAULT ''",
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
            // Sessions shipped without one; existing rows are the console's, which is full.
            ensure_column(
                &conn,
                "node_sessions",
                "scope",
                "TEXT NOT NULL DEFAULT 'full'",
            )?;
            ensure_column(
                &conn,
                "node_auth",
                "pending_totp",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(
                &conn,
                "execution_leases",
                "owner_token",
                "TEXT NOT NULL DEFAULT ''",
            )?;
        }
        endora_understanding::migrate(&db)?;
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

impl endora_application::UsageLog for SqliteStore {
    fn record(&self, role: &str, model: &str, usage: endora_application::ModelUsage) {
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        // Best effort: bookkeeping must never fail the turn it describes.
        if let Ok(conn) = self.lock() {
            let _ = conn.execute(
                "INSERT INTO model_usage (at_ms, role, model, prompt, completion, cached)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    at_ms,
                    role,
                    model,
                    i64::try_from(usage.prompt_tokens).unwrap_or(i64::MAX),
                    i64::try_from(usage.completion_tokens).unwrap_or(i64::MAX),
                    i64::try_from(usage.cached_tokens).unwrap_or(i64::MAX),
                ],
            );
        }
    }
}

impl SqliteStore {
    /// What `role`'s model calls have used since `since_ms`.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database cannot be read.
    pub fn model_usage_totals(
        &self,
        role: &str,
        since_ms: i64,
    ) -> Result<endora_application::UsageTotals, RepositoryError> {
        let conn = self.lock()?;
        let (calls, prompt, completion, cached): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(prompt),0), COALESCE(SUM(completion),0),
                        COALESCE(SUM(cached),0)
                 FROM model_usage WHERE role=?1 AND at_ms>=?2",
                params![role, since_ms],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(backend)?;
        let n = |v: i64| u64::try_from(v).unwrap_or(0);
        Ok(endora_application::UsageTotals {
            calls: n(calls),
            prompt_tokens: n(prompt),
            completion_tokens: n(completion),
            cached_tokens: n(cached),
        })
    }
}

impl ExecutionLeaseRepository for SqliteStore {
    fn reserve_execution_attempt(
        &self,
        execution_key: &str,
        at: Timestamp,
        max_attempts: u32,
    ) -> Result<bool, RepositoryError> {
        let key = execution_key.trim();
        if key.is_empty() || key.chars().count() > 240 || max_attempts == 0 {
            return Err(RepositoryError::Corrupt(
                "execution lease requires a key and positive attempt budget".to_owned(),
            ));
        }
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR IGNORE INTO execution_leases (execution_key) VALUES (?1)",
            params![key],
        )
        .map_err(backend)?;
        let changed = conn
            .execute(
                "UPDATE execution_leases
                 SET attempts = attempts + 1, lease_until_ms=?2 + ?3, owner_token=''
                 WHERE execution_key=?1 AND attempts < ?4 AND lease_until_ms <= ?2",
                params![
                    key,
                    at.unix_millis(),
                    endora_application::EXECUTION_LEASE_MS,
                    i64::from(max_attempts)
                ],
            )
            .map_err(backend)?;
        Ok(changed == 1)
    }

    fn release_execution_attempt(
        &self,
        execution_key: &str,
        lease_until_ms: i64,
    ) -> Result<(), RepositoryError> {
        let key = execution_key.trim();
        if key.is_empty() || key.chars().count() > 240 || lease_until_ms <= 0 {
            return Err(RepositoryError::Corrupt(
                "execution lease release requires a key and lease token".to_owned(),
            ));
        }
        self.lock()?
            .execute(
                "UPDATE execution_leases SET lease_until_ms=0
                 WHERE execution_key=?1 AND lease_until_ms=?2 AND owner_token=''",
                params![key, lease_until_ms],
            )
            .map_err(backend)?;
        Ok(())
    }
}

// The eight direction repositories are implemented by the direction context's
// DirectionStore over the shared Db now (ADR 0050); the all_*/parse_* helpers
// below remain for MemoryStore's export.

impl MemoryStore for SqliteStore {
    fn export(&self) -> Result<MemorySnapshot, RepositoryError> {
        let research = endora_understanding::UnderstandingStore::new(self.db()).list_research()?;
        let conn = self.lock()?;
        Ok(MemorySnapshot {
            audit: all_audit(&conn)?,
            messages: all_messages(&conn)?,
            preferences: all_preferences(&conn)?,
            beliefs: all_beliefs(&conn)?,
            outcomes: all_outcomes(&conn)?,
            intentions: all_intentions(&conn)?,
            research,
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
            "beliefs",
            "outcomes",
            "target_aliases",
            "intention_research",
            "execution_leases",
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

impl ResearchDelivery for SqliteStore {
    fn publish_research(
        &self,
        work: &ResearchAssignment,
        notice: &ChatMessage,
        audit: &[AuditRecord],
    ) -> Result<bool, RepositoryError> {
        let Some(report) = work.report() else {
            return Err(RepositoryError::Corrupt(
                "cannot publish research without a report".to_owned(),
            ));
        };
        if work.status() != endora_application::ResearchStatus::Delivered {
            return Err(RepositoryError::Corrupt(
                "cannot publish non-delivered research".to_owned(),
            ));
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        let changed = tx.execute(
            "UPDATE intention_research SET status='delivered', phase=?2, reason_code='', reason='',
             checkpoint=?3, report_markdown=?4, report_partial=?5, report_created_ms=?6,
             model_calls_used=?7, read_attempts_used=?8, updated_ms=?9
             WHERE intention_id=?1 AND status NOT IN
                ('delivered','blocked','budget_exhausted','cancelled','failed')",
            params![
                work.intention_id().value().to_string(), work.phase(), work.checkpoint(),
                report.markdown(), report.partial(), report.created_at().unix_millis(),
                work.model_calls_used(), work.read_attempts_used(), work.updated_at().unix_millis(),
            ],
        ).map_err(backend)?;
        if changed == 0 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE intentions SET state='done', last_progressed_ms=?2 WHERE id=?1",
            params![
                work.intention_id().value().to_string(),
                work.updated_at().unix_millis()
            ],
        )
        .map_err(backend)?;
        tx.execute(
            "INSERT INTO messages (id,role,body,at_ms) VALUES (?1,?2,?3,?4)",
            params![
                notice.id().value().to_string(),
                notice.role().name(),
                notice.text(),
                notice.at().unix_millis()
            ],
        )
        .map_err(backend)?;
        for record in audit {
            tx.execute(
                "INSERT INTO audit_log (id,at_ms,summary) VALUES (?1,?2,?3)",
                params![
                    record.id().value().to_string(),
                    record.at().unix_millis(),
                    record.summary()
                ],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(true)
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
            "SELECT id, statement, motivating_belief, origin, request_message_id, note, state,
             created_ms, last_progressed_ms, steps_taken
             FROM intentions ORDER BY last_progressed_ms DESC, rowid DESC",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, u32>(9)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, statement, belief, origin, request, note, state, created_ms, progressed_ms, steps) =
            row.map_err(backend)?;
        let origin = match (origin.as_str(), belief, request) {
            ("belief", Some(id), None) => IntentionOrigin::Belief(BeliefId::new(parse_id(&id)?)),
            ("user_request", None, Some(id)) => {
                IntentionOrigin::UserRequest(MessageId::new(parse_id(&id)?))
            }
            _ => {
                return Err(RepositoryError::Corrupt(
                    "invalid intention provenance".to_owned(),
                ));
            }
        };
        out.push(Intention::from_parts(
            IntentionId::new(parse_id(&id)?),
            statement,
            origin,
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
/// Carries a renamed capability's stored config and settings across to its new id.
///
/// A skill called `local_events` answers with civic agendas — council, committee, planning,
/// zoning — and nothing else. Its description said exactly that; its id did not, and on a
/// small model the id is the headline. Asked what was on at a stadium it reached for this
/// one, twice, confidently, and reported the council calendar. The tool was working
/// perfectly and the name was a promise the description had to walk back.
///
/// Renaming without this would silently drop whatever the person had configured and leave
/// the skill asking to be set up again — settings loss wearing a fresh install's clothes,
/// which is the worst shape a migration can take because nothing looks broken.
///
/// `UPDATE OR IGNORE` then `DELETE`: if a row already exists under the new id, that is the
/// person's newer choice and it wins, and the stale one goes rather than lingering.
fn renamed_capability(
    conn: &rusqlite::Connection,
    from: &str,
    to: &str,
) -> Result<(), RepositoryError> {
    for (table, column) in [
        ("capability_config", "id"),
        ("capability_settings", "capability_id"),
    ] {
        conn.execute(
            &format!("UPDATE OR IGNORE {table} SET {column} = ?1 WHERE {column} = ?2"),
            rusqlite::params![to, from],
        )
        .map_err(backend)?;
        conn.execute(
            &format!("DELETE FROM {table} WHERE {column} = ?1"),
            rusqlite::params![from],
        )
        .map_err(backend)?;
    }
    Ok(())
}

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

/// Drops what the clean slate removed (owner, 2026-10-16): the check-in, the nightly loop
/// and model-tune schedules, notions, standing trouble, the failed asks kept for the nightly loop to
/// replay (specimens), and durable goals with their leases. For the same
/// reason as [`drop_goal_tracker`]: a table nothing reads is data nothing can export or
/// delete (constitution §6). A no-op on a fresh database.
fn drop_the_clean_slate(conn: &Connection) -> Result<(), RepositoryError> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS checkin;
         DROP TABLE IF EXISTS night_loop_schedule;
         DROP TABLE IF EXISTS model_tune_schedule;
         DROP TABLE IF EXISTS notion_citations;
         DROP TABLE IF EXISTS notions;
         DROP TABLE IF EXISTS standing_trouble;
         DROP TABLE IF EXISTS specimens;
         DROP INDEX IF EXISTS idx_execution_goals_active;
         DROP TABLE IF EXISTS execution_goals;",
    )
    .map_err(backend)?;
    // Leases are shared with research; only the goals' own go.
    let leases_exist: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='execution_leases')",
            [],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if leases_exist {
        conn.execute(
            "DELETE FROM execution_leases WHERE execution_key LIKE 'goal:%'",
            [],
        )
        .map_err(backend)?;
    }
    Ok(())
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

    /// A rename must not look like a fresh install.
    ///
    /// The value here is one somebody typed once and would have to find again: drop it and
    /// the skill politely asks to be set up, which reads as working software and is not.
    #[test]
    fn renaming_a_capability_carries_what_the_person_configured() {
        use endora_application::{CapabilityConfigRepository, CapabilitySettingsRepository};
        let store = store();
        let cfg = cfg_store(&store);

        (&cfg as &dyn CapabilitySettingsRepository)
            .set_setting("old_name", "client", "somewhere")
            .unwrap();
        (&cfg as &dyn CapabilityConfigRepository)
            .set_stance("old_name", endora_capabilities::Stance::Off)
            .unwrap();

        super::renamed_capability(&store.db().lock().unwrap(), "old_name", "new_name").unwrap();

        let settings = (&cfg as &dyn CapabilitySettingsRepository)
            .all_settings()
            .unwrap();
        assert_eq!(
            settings,
            vec![(
                "new_name".to_owned(),
                "client".to_owned(),
                "somewhere".to_owned()
            )],
            "the configured value did not travel with the rename"
        );
        // The stance travelled too, and the old id is gone rather than lingering as a
        // second, stale row.
        let config = (&cfg as &dyn CapabilityConfigRepository).stances().unwrap();
        assert_eq!(
            config,
            vec![("new_name".to_owned(), endora_capabilities::Stance::Off)]
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
            preferred_url: "http://192.168.1.10:11434/v1".to_owned(),
            preferred_model: "qwen2.5:14b".to_owned(),
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
    fn a_stance_round_trips_and_an_unrecognised_word_is_dropped() {
        use endora_application::CapabilityConfigRepository;
        use endora_capabilities::Stance;
        let store = store();
        let cfg = cfg_store(&store);
        let repo: &dyn CapabilityConfigRepository = &cfg;

        assert!(repo.stances().unwrap().is_empty());
        repo.set_stance("weather", Stance::Off).unwrap();
        repo.set_stance("flights", Stance::Ask).unwrap();
        repo.set_stance("weather", Stance::Auto).unwrap(); // latest per id wins
        let mut got = repo.stances().unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("flights".to_owned(), Stance::Ask),
                ("weather".to_owned(), Stance::Auto)
            ]
        );

        // Clearing returns a tool to its band's default.
        repo.clear_stance("flights").unwrap();
        assert_eq!(
            repo.stances().unwrap(),
            vec![("weather".to_owned(), Stance::Auto)]
        );

        // A word nobody recognises is dropped rather than guessed at: a permission that
        // deserialises leniently is a permission that widens by typo.
        store
            .db()
            .lock()
            .unwrap()
            .execute(
                "UPDATE capability_config SET stance = 'yolo' WHERE id = 'weather'",
                [],
            )
            .unwrap();
        assert!(repo.stances().unwrap().is_empty());
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
        ConfigStore, ConfigWrite, ConfigWriteLog, Recipe, RecipeRepository, TargetAlias,
        TargetAliasRepository, WriteKind,
    };

    /// Builds the stores exactly as `main.rs` does: one production [`SqliteStore`], with the
    /// context stores layered over **its** `Db` handle. No test migration anywhere.
    fn as_production_does() -> ConfigStore {
        let store = SqliteStore::open_in_memory().expect("open the production store");
        ConfigStore::new(store.db())
    }

    #[test]
    fn a_database_from_before_the_preferred_endpoint_reads_its_model_config() {
        // Live: GET /v1/model-config (and the AI screen) answered "internal error" on every
        // node older than the preferred endpoint, because its two columns were added only in
        // `endora_capabilities::migrate`, which production never runs.
        use endora_application::ButlerModelConfigRepository;
        let dir =
            std::env::temp_dir().join(format!("endora-old-model-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE butler_model_config (
                    id INTEGER PRIMARY KEY CHECK (id = 0), base_url TEXT NOT NULL,
                    api_key TEXT NOT NULL, mixture INTEGER NOT NULL, single_model TEXT NOT NULL,
                    single_temp REAL, single_top_p REAL, single_top_k INTEGER, single_repeat REAL,
                    router_model TEXT NOT NULL, router_temp REAL, router_top_p REAL,
                    router_top_k INTEGER, router_repeat REAL, synth_model TEXT NOT NULL,
                    synth_temp REAL, synth_top_p REAL, synth_top_k INTEGER, synth_repeat REAL
                ) STRICT;
                INSERT INTO butler_model_config VALUES
                    (0, 'http://localhost:11434/v1', '', 0, 'qwen2.5:7b',
                     NULL, NULL, NULL, NULL, '', NULL, NULL, NULL, NULL, '', NULL, NULL, NULL, NULL);",
            )
            .unwrap();
        }
        let store = SqliteStore::open(path.to_str().unwrap()).expect("an older database opens");
        let cfg = ConfigStore::new(store.db());
        let read = ButlerModelConfigRepository::get(&cfg)
            .expect("its model config reads")
            .expect("and is there");
        assert_eq!(read.single.model, "qwen2.5:7b");
        assert!(read.preferred_url.is_empty());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_column_the_test_migration_adds_production_adds_too() {
        // `endora_capabilities::migrate` runs in tests and nowhere else. A column added there
        // alone exists in every test database and is missing from every real one older than
        // it, which is how the model config broke. Each `ALTER TABLE … ADD COLUMN` there must
        // have its `ensure_column` here.
        let test_only = include_str!("../../../domains/capabilities/src/store.rs");
        let production = include_str!("sqlite.rs")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let mut checked = 0;
        for after in test_only.split("ALTER TABLE ").skip(1) {
            let words: Vec<&str> = after.split_whitespace().take(4).collect();
            let [table, "ADD", "COLUMN", column] = words.as_slice() else {
                continue;
            };
            let added = format!("ensure_column( &conn, \"{table}\", \"{column}\",");
            let added_short = format!("ensure_column(&conn, \"{table}\", \"{column}\",");
            assert!(
                production.contains(&added) || production.contains(&added_short),
                "{table}.{column} is added only by the test migration"
            );
            checked += 1;
        }
        assert!(
            checked >= 9,
            "the scan found the test migration's columns ({checked})"
        );
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

        // A third time would be its own kind of lesson: the recipes table (ADR
        // 0071) went into `endora_capabilities::migrate` alone on the first pass
        // and was invisible here — every unit test green, `/v1/recipes` a 500 on
        // the very first real request, caught by this exact test before it ever
        // reached a person's house.
        let recipe = Recipe::new(
            "air_quality",
            "Today's air quality.",
            vec![],
            "https://x.test/",
            "s",
        )
        .unwrap();
        RecipeRepository::save_recipe(&config, &recipe).expect("recipes");
        assert_eq!(
            RecipeRepository::recipes(&config).expect("read back").len(),
            1
        );
        RecipeRepository::delete_recipe(&config, "air_quality").expect("delete");
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
                key: "house::person.john".to_owned(),
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
                key: "house::person.john".to_owned(),
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
    fn commissioned_research_survives_and_a_late_result_cannot_undo_cancellation() {
        use endora_application::{
            IntentionId, MemoryStore, MessageId, ResearchRepository, ResearchStatus, Timestamp,
        };

        let store = SqliteStore::open_in_memory().expect("open the production store");
        let mut running = store
            .commission_research(
                IntentionId::new(10),
                MessageId::new(11),
                "request-1",
                "Compare backup approaches",
                r#"["https://example.com"]"#,
                None,
                Timestamp::from_unix_millis(1_000),
            )
            .expect("commission atomically");
        assert!(running.reserve_model_call(Timestamp::from_unix_millis(1_001)));
        let understanding = endora_understanding::UnderstandingStore::new(store.db());
        understanding.save_research(&running).unwrap();

        let mut cancelled = understanding
            .research(IntentionId::new(10))
            .unwrap()
            .unwrap();
        cancelled.cancel(Timestamp::from_unix_millis(1_002));
        understanding.save_research(&cancelled).unwrap();

        running.fail(
            "late",
            "late worker result",
            Timestamp::from_unix_millis(1_003),
        );
        understanding.save_research(&running).unwrap();
        assert_eq!(
            understanding
                .research(IntentionId::new(10))
                .unwrap()
                .unwrap()
                .status(),
            ResearchStatus::Cancelled,
        );
        assert_eq!(store.export().unwrap().research.len(), 1);
        store.purge().unwrap();
        assert!(store.export().unwrap().research.is_empty());
    }

    #[test]
    fn publishing_research_commits_report_intention_and_notice_once() {
        use endora_application::{
            ChatMessage, IntentionRepository, MemoryStore, MessageId, MessageRole,
            ResearchDelivery, ResearchReport, ResearchRepository, Timestamp,
        };

        let store = SqliteStore::open_in_memory().unwrap();
        let mut work = store
            .commission_research(
                endora_application::IntentionId::new(30),
                MessageId::new(31),
                "request-publish",
                "Research backups",
                r#"["https://example.com"]"#,
                None,
                Timestamp::from_unix_millis(1_000),
            )
            .unwrap();
        work.deliver(
            ResearchReport::new(
                work.intention_id(),
                "# Report\n\n## Sources\n\n- [S1](https://example.com)",
                false,
                Timestamp::from_unix_millis(2_000),
            )
            .unwrap(),
            Timestamp::from_unix_millis(2_000),
        );
        let notice = ChatMessage::new(
            MessageId::new(32),
            MessageRole::Butler,
            "Your research report is ready.",
            Timestamp::from_unix_millis(2_000),
        )
        .unwrap();

        assert!(store.publish_research(&work, &notice, &[]).unwrap());
        assert!(!store.publish_research(&work, &notice, &[]).unwrap());
        let understanding = endora_understanding::UnderstandingStore::new(store.db());
        assert!(
            !understanding
                .get(work.intention_id())
                .unwrap()
                .unwrap()
                .is_active()
        );
        assert_eq!(
            store.export().unwrap().messages.len(),
            2,
            "commission plus one notice"
        );
        assert_eq!(
            understanding
                .research(work.intention_id())
                .unwrap()
                .unwrap()
                .status(),
            endora_application::ResearchStatus::Delivered
        );
    }

    #[test]
    fn a_published_report_survives_reopening_the_production_database() {
        use endora_application::{
            ChatMessage, MemoryStore, MessageId, MessageRole, ResearchDelivery, ResearchReport,
            Timestamp,
        };
        let path = std::env::temp_dir().join(format!(
            "endora-research-restart-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        {
            let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
            let mut work = store
                .commission_research(
                    endora_application::IntentionId::new(40),
                    MessageId::new(41),
                    "restart-key",
                    "Research backups",
                    r#"["https://example.com"]"#,
                    None,
                    Timestamp::from_unix_millis(1_000),
                )
                .unwrap();
            work.deliver(
                ResearchReport::new(
                    work.intention_id(),
                    "# Report\n\nSaved.\n\n## Sources\n\n- [S1](https://example.com)",
                    false,
                    Timestamp::from_unix_millis(2_000),
                )
                .unwrap(),
                Timestamp::from_unix_millis(2_000),
            );
            let notice = ChatMessage::new(
                MessageId::new(42),
                MessageRole::Butler,
                "Your research report is ready.",
                Timestamp::from_unix_millis(2_000),
            )
            .unwrap();
            assert!(store.publish_research(&work, &notice, &[]).unwrap());
        }
        let reopened = SqliteStore::open(path.to_str().unwrap()).unwrap();
        let snapshot = reopened.export().unwrap();
        assert_eq!(snapshot.research.len(), 1);
        assert!(snapshot.research[0].report().is_some());
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn research_workers_share_reclaimable_execution_leases() {
        use endora_application::{ExecutionLeaseRepository, Timestamp};

        let store = SqliteStore::open_in_memory().unwrap();
        let first = Timestamp::from_unix_millis(1_000);
        assert!(
            store
                .reserve_execution_attempt("research:request-1", first, 16)
                .unwrap()
        );
        assert!(
            !store
                .reserve_execution_attempt(
                    "research:request-1",
                    Timestamp::from_unix_millis(2_000),
                    16
                )
                .unwrap()
        );

        // A dead worker's lease is reclaimable after the fixed 30-second window.
        let reclaimed_at = Timestamp::from_unix_millis(31_001);
        assert!(
            store
                .reserve_execution_attempt("research:request-1", reclaimed_at, 16)
                .unwrap()
        );

        // A late worker cannot release the newer worker's lease token.
        store
            .release_execution_attempt("research:request-1", first.unix_millis() + 30_000)
            .unwrap();
        assert!(
            !store
                .reserve_execution_attempt(
                    "research:request-1",
                    Timestamp::from_unix_millis(31_002),
                    16
                )
                .unwrap()
        );
        store
            .release_execution_attempt(
                "research:request-1",
                reclaimed_at.unix_millis() + endora_application::EXECUTION_LEASE_MS,
            )
            .unwrap();
        assert!(
            store
                .reserve_execution_attempt(
                    "research:request-2",
                    Timestamp::from_unix_millis(2_000),
                    8
                )
                .unwrap()
        );
    }

    #[test]
    fn an_existing_database_sheds_what_the_clean_slate_removed() {
        use endora_application::{ExecutionLeaseRepository, Timestamp};

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE checkin (id INTEGER PRIMARY KEY);
             CREATE TABLE night_loop_schedule (id INTEGER PRIMARY KEY);
             CREATE TABLE model_tune_schedule (id INTEGER PRIMARY KEY);
             CREATE TABLE notions (id INTEGER PRIMARY KEY);
             CREATE TABLE notion_citations (notion INTEGER);
             CREATE TABLE standing_trouble (entity TEXT PRIMARY KEY);
             CREATE TABLE specimens (id TEXT PRIMARY KEY, asked TEXT);
             CREATE TABLE execution_goals (request_key TEXT PRIMARY KEY, objective TEXT);
             INSERT INTO execution_goals VALUES ('ask-1', 'find John Doe a plumber');
             CREATE TABLE execution_leases (
                 execution_key TEXT PRIMARY KEY,
                 attempts INTEGER NOT NULL DEFAULT 0,
                 lease_until_ms INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO execution_leases (execution_key, attempts) VALUES ('goal:ask-1', 1);
             INSERT INTO execution_leases (execution_key, attempts) VALUES ('research:r-1', 1);",
        )
        .unwrap();
        let store =
            SqliteStore::from_db(endora_persistence::Db::from_connection(conn).unwrap()).unwrap();
        let conn = store.lock().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for gone in [
            "checkin",
            "night_loop_schedule",
            "model_tune_schedule",
            "notions",
            "notion_citations",
            "standing_trouble",
            "specimens",
            "execution_goals",
        ] {
            assert!(!tables.iter().any(|t| t == gone), "{gone} is still there");
        }
        let leases: Vec<String> = conn
            .prepare("SELECT execution_key FROM execution_leases")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(leases, ["research:r-1"], "research keeps its lease");
        drop(conn);
        // And research still reserves through the table that stayed.
        assert!(
            store
                .reserve_execution_attempt("research:r-2", Timestamp::from_unix_millis(1_000), 4)
                .unwrap()
        );
    }
}
