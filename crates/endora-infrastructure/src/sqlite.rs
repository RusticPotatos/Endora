//! SQLite-backed implementations of the application repository ports.
//!
//! A single [`SqliteStore`] owns one connection behind a mutex and implements
//! the repository traits. Identifiers are `u128`, which SQLite integers cannot
//! hold, so they are stored as their decimal `TEXT` form and parsed back on
//! read. The store is synchronous; when the async node uses it, calls run on a
//! blocking thread (see `docs/adr/0007-async-web-stack.md`).

use std::path::Path;
use std::sync::Arc;

use endora_persistence::Db;
use endora_persistence::secrets::{KeyFile, KeySource, SecretColumn};
use sha2::{Digest as _, Sha256};

use endora_application::{ActivityEvent, AuditId, AuditRecord, Timestamp};
use endora_application::{MemorySnapshot, MemoryStore, RepositoryError};
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


-- The running conversation summary (ADR 0053 context compaction): a single row.
-- Persisted so a restart doesn't re-summarise the whole backlog (which, on a slow
-- local model, degraded the first turns after every deploy) and the butler keeps
-- the day's thread across restarts. `covered` = how many of the oldest messages the
-- summary already folds in.


-- One durable lease table is shared by every resumable worker. The execution
-- key is namespaced by its owner (`research:`), so domains retain their own
-- checkpoints while worker ownership follows one contract.
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
CREATE TABLE IF NOT EXISTS model_providers (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    name    TEXT NOT NULL,
    url     TEXT NOT NULL,
    api_key TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS model_entries (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    job      TEXT NOT NULL,
    provider INTEGER NOT NULL,
    model    TEXT NOT NULL,
    enabled  INTEGER NOT NULL DEFAULT 1,
    position INTEGER NOT NULL,
    sampling TEXT NOT NULL DEFAULT '{}',
    thinks   INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS model_lists_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
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
    -- Never the token itself: `sha256:<hex>` of it (ADR 0125). A copy of this table lets
    -- nobody in.
    token     TEXT PRIMARY KEY,
    issued_ms INTEGER NOT NULL,
    -- 'full' for the console, 'checks' for the smoke suite's read-only credential.
    scope     TEXT NOT NULL DEFAULT 'full',
    -- When it was last let in, for the console's idle limit.
    last_seen_ms INTEGER NOT NULL DEFAULT 0
) STRICT;
-- Questions the person answered by asking not to be asked again. Answering is the
-- dismissal (ADR 0052), and somebody who does not know a real name still needs an answer.
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
CREATE TABLE IF NOT EXISTS bench_spend (
    day    INTEGER PRIMARY KEY,
    micros INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS provider_spend (
    provider TEXT PRIMARY KEY,
    micros   INTEGER NOT NULL
) STRICT;
";

/// Every column in the node's database that holds a credential, sealed at rest (ADR 0125).
///
/// The migration seals what an older node left in plaintext, and a rekey re-seals all of
/// it, by walking this list — so a credential column added later must be added here too.
/// The stores that read and write these columns seal and unseal them; a value that reaches
/// one of them unsealed is refused.
///
/// `capability_settings.value` is sealed whole rather than by key: the Home Assistant
/// token, the *arr and dashboard API keys, passwords, and Spotify's tokens all live there
/// beside harmless settings, and a guess at which is which would be the one place a new
/// credential slipped through.
pub const SECRET_COLUMNS: &[SecretColumn] = &[
    SecretColumn {
        table: "model_providers",
        column: "api_key",
    },
    SecretColumn {
        table: "mcp_servers",
        column: "env",
    },
    SecretColumn {
        table: "mcp_servers",
        column: "auth",
    },
    SecretColumn {
        table: "capability_settings",
        column: "value",
    },
    SecretColumn {
        table: "node_auth",
        column: "token",
    },
    SecretColumn {
        table: "node_auth",
        column: "totp_secret",
    },
    SecretColumn {
        table: "node_auth",
        column: "pending_totp",
    },
];

/// What a session token is stored as: `sha256:<hex>`. The tokens are 256 random bits, so a
/// plain hash is enough — there is nothing to guess that a slow hash would slow down.
fn session_digest(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{SESSION_HASHED}{hex}")
}

const SESSION_HASHED: &str = "sha256:";

/// How often a session's last use is written down: once a minute is plenty for a seven-day
/// limit, and a write on every request is not.
const TOUCH_EVERY_MS: i64 = 60_000;

/// Compares two strings without stopping at the first difference.
fn same_text(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |differences, (x, y)| differences | (x ^ y))
            == 0
}

/// Replaces every session token stored raw with its digest, and gives each a last use of
/// `now_ms` so nobody is signed out by the upgrade itself. Guarded by the prefix, so a
/// second run changes nothing. Returns how many were hashed.
fn hash_stored_sessions(conn: &mut Connection, now_ms: i64) -> Result<usize, RepositoryError> {
    let tx = conn.transaction().map_err(backend)?;
    let raw: Vec<(i64, String)> = {
        let mut stmt = tx
            .prepare(&format!(
                "SELECT rowid, token FROM node_sessions WHERE substr(token, 1, {}) != '{SESSION_HASHED}'",
                SESSION_HASHED.len()
            ))
            .map_err(backend)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?
    };
    for (rowid, token) in &raw {
        tx.execute(
            "UPDATE node_sessions SET token = ?1 WHERE rowid = ?2",
            params![session_digest(token), rowid],
        )
        .map_err(backend)?;
    }
    tx.execute(
        "UPDATE node_sessions SET last_seen_ms = ?1 WHERE last_seen_ms = 0",
        params![now_ms],
    )
    .map_err(backend)?;
    tx.commit().map_err(backend)?;
    if !raw.is_empty() {
        conn.execute_batch("VACUUM").map_err(backend)?;
    }
    Ok(raw.len())
}

/// The way out when the secrets key is lost for good (ADR 0125): empties every sealed
/// credential and the password, drops the data keys, and so lets the node start with a new
/// key. Sign-in is then set up again from the bootstrap token the log prints, and every
/// provider key, token and password is entered again. Needs no key.
///
/// # Errors
/// [`RepositoryError`] if the database fails; nothing changes then.
pub fn forget_sealed_secrets(db: &Db) -> Result<usize, RepositoryError> {
    let emptied = db.forget_sealed(SECRET_COLUMNS)?;
    db.lock()?
        .execute("UPDATE node_auth SET password_hash = ''", [])
        .map_err(backend)?;
    Ok(emptied)
}

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
        let token = self.db.unseal("node_auth.token", &token)?;
        Ok((!token.is_empty()).then_some(token))
    }

    /// Stores the token this node will accept from then on, sealed: unlike a session it has
    /// to be read back (the log prints it until sign-in is set up, and the console reveals
    /// it), so it cannot be a hash.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_node_token(&self, token: &str) -> Result<(), RepositoryError> {
        let sealed = self.db.seal(token)?;
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO node_auth (id, token) VALUES (0, ?1)",
                params![sealed],
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
        let hash: String = row.get(0).map_err(backend)?;
        let secret: String = row.get(1).map_err(backend)?;
        Ok((hash, self.db.unseal("node_auth.totp_secret", &secret)?))
    }

    /// Stores the password hash and authenticator secret, the secret sealed. The hash stays
    /// as it is: Argon2id is already what a stolen copy should face.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_sign_in_setup(&self, hash: &str, secret: &str) -> Result<(), RepositoryError> {
        let secret = self.db.seal(secret)?;
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
        let pending: String = row.get(0).map_err(backend)?;
        self.db.unseal("node_auth.pending_totp", &pending)
    }

    /// Puts a replacement authenticator secret aside, or clears it with an empty string.
    ///
    /// **Never touches the live secret.** That is the whole point: until a code proves the new
    /// one was actually scanned, the old phone keeps working.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_pending_authenticator(&self, secret: &str) -> Result<(), RepositoryError> {
        let secret = self.db.seal(secret)?;
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
    /// Only the token's digest is kept (ADR 0125): the token goes back to whoever signed in
    /// and nowhere else, so a copy of the database or a backup lets nobody in.
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
            "INSERT OR REPLACE INTO node_sessions (token, issued_ms, scope, last_seen_ms) \
             VALUES (?1, ?2, ?3, ?2)",
            params![session_digest(token), now_ms, scope],
        )
        .map_err(backend)?;
        conn.execute(
            "DELETE FROM node_sessions WHERE issued_ms < ?1",
            params![now_ms - keep_for_ms],
        )
        .map_err(backend)?;
        Ok(())
    }

    /// The scope of the session `token` opens, or `None` if it opens none.
    ///
    /// Looked up by digest, so the token itself is never stored or compared in the
    /// database. A session lasts `keep_for_ms` from being issued; a **console** session
    /// (`full`) also ends after `idle_ms` unused. The checking and dashboard credentials do
    /// not idle out: they live in a file or on a pocket device, are used rarely by design,
    /// and expiring them while unused would only break `make live-smoke` and the dashboard.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn session_scope(
        &self,
        token: &str,
        now_ms: i64,
        keep_for_ms: i64,
        idle_ms: i64,
    ) -> Result<Option<String>, RepositoryError> {
        if token.is_empty() {
            return Ok(None);
        }
        let digest = session_digest(token);
        let conn = self.db.lock()?;
        let found: Option<(String, String, i64, i64)> = conn
            .query_row(
                "SELECT token, scope, issued_ms, last_seen_ms FROM node_sessions WHERE token = ?1",
                params![digest],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((stored, scope, issued_ms, last_seen_ms)) = found else {
            return Ok(None);
        };
        if !same_text(&stored, &digest) || issued_ms < now_ms - keep_for_ms {
            return Ok(None);
        }
        if scope == "full" && last_seen_ms < now_ms - idle_ms {
            return Ok(None);
        }
        if now_ms - last_seen_ms >= TOUCH_EVERY_MS {
            conn.execute(
                "UPDATE node_sessions SET last_seen_ms = ?1 WHERE token = ?2",
                params![now_ms, digest],
            )
            .map_err(backend)?;
        }
        Ok(Some(scope))
    }

    /// Opens (creating if needed) a store at `path`, with `keys` as its key source, and
    /// applies the schema.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be opened, its data keys do not
    /// open with `keys`, or it cannot be migrated.
    pub fn open(path: &str, keys: Arc<dyn KeySource>) -> Result<Self, RepositoryError> {
        let db = Db::open(path)?;
        db.attach_key_source(keys)?;
        Self::from_db(db)
    }

    /// Opens the node's store the way the node does (ADR 0125): the database at `path`, its
    /// secrets sealed under the key in `key_file`.
    ///
    /// The key file is made (owner-only) on the first start, and **only** then: a database
    /// that already holds data keys with no key file beside it is an error, because a new
    /// key would open none of them.
    ///
    /// # Errors
    /// [`RepositoryError`] if the key file is missing over sealed secrets, is not a key, or
    /// does not open the data keys; or if the database cannot be opened or migrated.
    pub fn open_sealed(path: &str, key_file: &Path) -> Result<Self, RepositoryError> {
        let db = Db::open(path)?;
        let keys = KeyFile::open(key_file, !db.has_data_keys()?)?;
        db.attach_key_source(Arc::new(keys))?;
        Self::from_db(db)
    }

    /// Rotates the data key: every sealed credential is sealed again under a new one, and
    /// the old one is forgotten. Returns the new key's id (an id, not a secret).
    ///
    /// # Errors
    /// [`RepositoryError`] if a value does not open or the database fails; nothing changes
    /// then.
    pub fn rekey_secrets(&self) -> Result<String, RepositoryError> {
        self.db.rekey(SECRET_COLUMNS)
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
        Self::with_schema(db, false)
    }

    fn with_schema(db: Db, waiting: bool) -> Result<Self, RepositoryError> {
        {
            let mut conn = db.lock()?;
            // Drop the goal-tracker schema before creating anything, so an existing
            // database sheds it rather than carrying dead tables forever.
            drop_goal_tracker(&conn)?;
            drop_the_chat(&conn)?;
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
            // Whether a model-list entry thinks before answering (ADR 0105). An entry made
            // before reads back as not thinking, the quick default for a local model.
            ensure_column(
                &conn,
                "model_entries",
                "thinks",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
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
            // Sessions are kept as digests (ADR 0125); one stored raw by an older node is
            // hashed in place, and given a last use of now so the upgrade signs nobody out.
            ensure_column(
                &conn,
                "node_sessions",
                "last_seen_ms",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            let hashed = hash_stored_sessions(&mut conn, now_ms())?;
            if hashed > 0 {
                println!("secrets: {hashed} session token(s) now kept only as a digest");
            }
        }
        // Credentials an older node stored in plaintext are sealed now, once (ADR 0125). A
        // store with no key source is refused here rather than left holding them in the clear.
        // A store waiting for the vault (ADR 0127) has no keys to seal with yet: that is done
        // when the keys are attached ([`attach_keys`](Self::attach_keys)).
        if db.has_key_source() || !waiting {
            let sealed = db.seal_plaintext(SECRET_COLUMNS)?;
            if sealed > 0 {
                println!("secrets: sealed {sealed} stored credential(s) at rest");
            }
        }
        Ok(Self { db })
    }

    /// Opens the store over `db` whose data keys a **sealed vault** wraps (ADR 0127): the
    /// schema is applied and everything that needs no secret works; everything that does
    /// fails saying the vault is locked, until [`attach_keys`](Self::attach_keys).
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the schema cannot be applied.
    pub fn open_waiting(db: Db) -> Result<Self, RepositoryError> {
        db.wait_for_the_vault();
        Self::with_schema(db, true)
    }

    /// Opens the data keys of a store that was waiting for the vault, with `keys`, and seals
    /// anything an older node left in plaintext.
    ///
    /// # Errors
    /// [`RepositoryError`] if the keys do not open or the database fails.
    pub fn attach_keys(&self, keys: Arc<dyn KeySource>) -> Result<(), RepositoryError> {
        self.db.attach_key_source(keys)?;
        let sealed = self.db.seal_plaintext(SECRET_COLUMNS)?;
        if sealed > 0 {
            println!("secrets: sealed {sealed} stored credential(s) at rest");
        }
        Ok(())
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
    /// What the bench credential (ADR 0123) has spent on `day` (days since the epoch, UTC), in
    /// micros: kept apart from HQ's pool, so its daily cap holds across a restart.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database cannot be read.
    pub fn bench_spent(&self, day: i64) -> Result<i64, RepositoryError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT micros FROM bench_spend WHERE day=?1",
            params![day],
            |r| r.get(0),
        )
        .optional()
        .map(Option::unwrap_or_default)
        .map_err(backend)
    }

    /// Adds `micros` to what the bench has spent on `day`.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database cannot be written.
    pub fn add_bench_spent(&self, day: i64, micros: i64) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO bench_spend (day, micros) VALUES (?1, ?2)
             ON CONFLICT(day) DO UPDATE SET micros = micros + excluded.micros",
            params![day, micros],
        )
        .map(|_| ())
        .map_err(backend)
    }

    /// What Endora has paid `provider` (`anthropic`, `deepseek`) in all, in micros: what a
    /// balance the owner read at one moment has lost since.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database cannot be read.
    pub fn provider_spent(&self, provider: &str) -> Result<i64, RepositoryError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT micros FROM provider_spend WHERE provider=?1",
            params![provider],
            |r| r.get(0),
        )
        .optional()
        .map(Option::unwrap_or_default)
        .map_err(backend)
    }

    /// Adds `micros` to what Endora has paid `provider`, at once, so two calls charged together
    /// both count.
    ///
    /// # Errors
    /// [`RepositoryError`] if the database cannot be written.
    pub fn add_provider_spent(&self, provider: &str, micros: i64) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO provider_spend (provider, micros) VALUES (?1, ?2)
             ON CONFLICT(provider) DO UPDATE SET micros = micros + excluded.micros",
            params![provider, micros],
        )
        .map(|_| ())
        .map_err(backend)
    }

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

/// Endora's record of what it did (ADR 0136): the audit trail and the event log. What the
/// owner exports, and what deleting forgets.
impl MemoryStore for SqliteStore {
    fn export(&self) -> Result<MemorySnapshot, RepositoryError> {
        let conn = self.lock()?;
        Ok(MemorySnapshot {
            audit: all_audit(&conn)?,
            events: all_events(&conn)?,
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
        // Nor are the settings the team runs on (ADR 0136): the skills' stances and settings,
        // which the bridge offers the team's turns, and the model providers and lists, which
        // hold the relay's key and house model. They are set up, not learned about the person.
        for table in ["audit_log", "events"] {
            tx.execute(&format!("DELETE FROM {table}"), [])
                .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }
}

/// Every event, newest first.
fn all_events(conn: &Connection) -> Result<Vec<ActivityEvent>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT at_ms, summary FROM events ORDER BY at_ms DESC, id DESC")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ActivityEvent {
                at: Timestamp::from_unix_millis(r.get::<_, i64>(0)?),
                summary: r.get::<_, String>(1)?,
            })
        })
        .map_err(backend)?;
    rows.collect::<Result<_, _>>().map_err(backend)
}

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
/// and model-tune schedules, notions, standing trouble, the failed asks kept for the
/// nightly loop to replay (specimens), and durable goals with their leases; and beliefs,
/// which the brain's pages replaced (ADR 0101). For the same reason as
/// [`drop_goal_tracker`]: a table nothing reads is data nothing can export or delete
/// (constitution §6). A no-op on a fresh database.
/// Endora is HQ (ADR 0136): what only the chat and the butler's own loops kept goes, once the
/// owner has taken a copy. A table nothing reads is data nothing can export or delete (ADR
/// 0100). The Deep list's entries go too: nothing asks that list any more.
fn drop_the_chat(conn: &Connection) -> Result<(), RepositoryError> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_messages_at;
         DROP INDEX IF EXISTS idx_transitions_at;
         DROP TABLE IF EXISTS message_actions;
         DROP TABLE IF EXISTS messages;
         DROP TABLE IF EXISTS conversation_summary;
         DROP TABLE IF EXISTS intention_research;
         DROP TABLE IF EXISTS execution_leases;
         DROP TABLE IF EXISTS intentions;
         DROP TABLE IF EXISTS outcomes;
         DROP TABLE IF EXISTS autonomy_envelope;
         DROP TABLE IF EXISTS brief_schedule;
         DROP TABLE IF EXISTS deep_model;
         DROP TABLE IF EXISTS butler_model_config;
         DROP TABLE IF EXISTS watched_things;
         DROP TABLE IF EXISTS transitions;
         DROP TABLE IF EXISTS recipes;
         DROP TABLE IF EXISTS target_aliases;
         DROP TABLE IF EXISTS repairs_left_alone;
         DROP TABLE IF EXISTS agenda_items;
         DROP TABLE IF EXISTS agenda_restarts;
         DROP TABLE IF EXISTS agenda_rules;
         DROP TABLE IF EXISTS agenda_responses;
         DROP TABLE IF EXISTS agenda_pushes;
         DROP TABLE IF EXISTS agenda_checked;
         DROP TABLE IF EXISTS mail_messages;
         DROP TABLE IF EXISTS mail_meta;",
    )
    .map_err(backend)?;
    let entries_exist: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_entries')",
            [],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if entries_exist {
        conn.execute("DELETE FROM model_entries WHERE job = 'deep'", [])
            .map_err(backend)?;
    }
    Ok(())
}

fn drop_the_clean_slate(conn: &Connection) -> Result<(), RepositoryError> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS checkin;
         DROP TABLE IF EXISTS night_loop_schedule;
         DROP TABLE IF EXISTS model_tune_schedule;
         DROP TABLE IF EXISTS notion_citations;
         DROP TABLE IF EXISTS notions;
         DROP TABLE IF EXISTS standing_trouble;
         DROP TABLE IF EXISTS specimens;
         DROP TABLE IF EXISTS beliefs;
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
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
    fn the_benchs_spending_is_kept_by_day_and_adds_up() {
        let store = store();
        assert_eq!(store.bench_spent(20_000).unwrap(), 0, "nothing spent yet");
        store.add_bench_spent(20_000, 1_500).unwrap();
        store.add_bench_spent(20_000, 2_500).unwrap();
        store.add_bench_spent(20_001, 7).unwrap();
        assert_eq!(store.bench_spent(20_000).unwrap(), 4_000);
        assert_eq!(
            store.bench_spent(20_001).unwrap(),
            7,
            "a new day starts again"
        );
        assert_eq!(store.bench_spent(19_999).unwrap(), 0);
    }

    #[test]
    fn what_each_provider_was_paid_adds_up_for_its_balance() {
        let store = store();
        assert_eq!(
            store.provider_spent("anthropic").unwrap(),
            0,
            "nothing paid yet"
        );
        store.add_provider_spent("anthropic", 1_500).unwrap();
        store.add_provider_spent("anthropic", 2_500).unwrap();
        store.add_provider_spent("deepseek", 7).unwrap();
        assert_eq!(store.provider_spent("anthropic").unwrap(), 4_000);
        assert_eq!(store.provider_spent("deepseek").unwrap(), 7);
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

    /// Deleting what Endora knows about the person is not unplugging the team (ADR 0136). The
    /// team's relay takes its key from the providers and its house model from the Everyday
    /// list, and the bridge offers the house's tools with each skill's stance and settings;
    /// purging those stopped every team turn, which "every message, belief and record" never
    /// said it would.
    #[test]
    fn purge_keeps_the_settings_the_team_runs_on() {
        use endora_application::MemoryStore;
        use endora_application::{CapabilityConfigRepository, CapabilitySettingsRepository};
        use endora_capabilities::model_lists::{Job, ModelListRepository};

        let store = store();
        let cfg = cfg_store(&store);
        let lists: &dyn ModelListRepository = &cfg;
        let provider = lists
            .add_provider("house", "http://192.168.1.10:11434/v1", "")
            .unwrap();
        lists
            .add_entry(Job::Everyday, provider, "qwen2.5:7b")
            .unwrap();
        (&cfg as &dyn CapabilitySettingsRepository)
            .set_setting("home_assistant", "token", "example-token")
            .unwrap();
        (&cfg as &dyn CapabilityConfigRepository)
            .set_stance("home_assistant", endora_capabilities::Stance::Ask)
            .unwrap();

        (&store as &dyn MemoryStore).purge().unwrap();

        assert_eq!(lists.providers().unwrap().len(), 1, "the providers went");
        assert_eq!(
            lists.entries(Job::Everyday).unwrap().len(),
            1,
            "the Everyday list went"
        );
        assert_eq!(
            (&cfg as &dyn CapabilitySettingsRepository)
                .all_settings()
                .unwrap()
                .len(),
            1,
            "the skills' settings went"
        );
        assert_eq!(
            (&cfg as &dyn CapabilityConfigRepository).stances().unwrap(),
            vec![(
                "home_assistant".to_owned(),
                endora_capabilities::Stance::Ask
            )],
            "the skills' stances went"
        );
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
}

#[cfg(test)]
mod secrets_at_rest;

#[cfg(test)]
mod the_schema_production_actually_runs {
    use super::SqliteStore;
    use endora_capabilities::{ConfigStore, ConfigWrite, ConfigWriteLog, WriteKind};

    /// Builds the stores exactly as `main.rs` does: one production [`SqliteStore`], with the
    /// context stores layered over **its** `Db` handle. No test migration anywhere.
    fn as_production_does() -> ConfigStore {
        let store = SqliteStore::open_in_memory().expect("open the production store");
        ConfigStore::new(store.db())
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
            // A table production drops (ADR 0136) needs no column added.
            if THE_CHATS.contains(table) {
                continue;
            }
            let added = format!("ensure_column( &conn, \"{table}\", \"{column}\",");
            let added_short = format!("ensure_column(&conn, \"{table}\", \"{column}\",");
            assert!(
                production.contains(&added) || production.contains(&added_short),
                "{table}.{column} is added only by the test migration"
            );
            checked += 1;
        }
        assert!(
            checked >= 5,
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
    }

    /// The tables only the chat and the butler's own loops used (ADR 0136).
    const THE_CHATS: [&str; 24] = [
        "messages",
        "message_actions",
        "conversation_summary",
        "intentions",
        "intention_research",
        "execution_leases",
        "outcomes",
        "autonomy_envelope",
        "brief_schedule",
        "deep_model",
        "butler_model_config",
        "watched_things",
        "transitions",
        "recipes",
        "target_aliases",
        "repairs_left_alone",
        "agenda_items",
        "agenda_restarts",
        "agenda_rules",
        "agenda_responses",
        "agenda_pushes",
        "agenda_checked",
        "mail_messages",
        "mail_meta",
    ];

    fn tables(store: &SqliteStore) -> Vec<String> {
        store
            .lock()
            .unwrap()
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// Endora is HQ (ADR 0136): an existing database drops what only the chat used when the
    /// node starts, keeps what the team and the house run on, and loses the Deep list's
    /// entries, which nothing asks any more.
    #[test]
    fn an_existing_database_sheds_what_the_chat_used() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for table in THE_CHATS {
            conn.execute_batch(&format!(
                "CREATE TABLE {table} (id TEXT); INSERT INTO {table} VALUES ('John Doe said hello');"
            ))
            .unwrap();
        }
        let store =
            SqliteStore::from_db(endora_persistence::Db::from_connection(conn).unwrap()).unwrap();
        let left = tables(&store);
        for gone in THE_CHATS {
            assert!(!left.iter().any(|t| t == gone), "{gone} is still there");
        }
        for kept in [
            "audit_log",
            "events",
            "capability_config",
            "capability_settings",
            "model_providers",
            "model_entries",
            "model_usage",
            "mcp_servers",
            "config_writes",
            "node_auth",
            "node_sessions",
        ] {
            assert!(left.iter().any(|t| t == kept), "{kept} went too");
        }
    }

    #[test]
    fn a_fresh_database_never_makes_what_the_chat_used() {
        let left = tables(&SqliteStore::open_in_memory().unwrap());
        for gone in THE_CHATS {
            assert!(!left.iter().any(|t| t == gone), "{gone} is made afresh");
        }
    }

    #[test]
    fn the_deep_lists_entries_go_and_everydays_stay() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE model_providers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL, api_key TEXT NOT NULL DEFAULT '');
             CREATE TABLE model_entries (id INTEGER PRIMARY KEY AUTOINCREMENT, job TEXT NOT NULL, provider INTEGER NOT NULL, model TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, position INTEGER NOT NULL, sampling TEXT NOT NULL DEFAULT '{}', thinks INTEGER NOT NULL DEFAULT 0);
             INSERT INTO model_providers (id, name, url) VALUES (1, 'house', 'http://192.168.1.10:11434/v1');
             INSERT INTO model_entries (job, position, provider, model) VALUES ('everyday', 0, 1, 'qwen2.5:7b');
             INSERT INTO model_entries (job, position, provider, model) VALUES ('deep', 0, 1, 'big-model');",
        )
        .unwrap();
        let store =
            SqliteStore::from_db(endora_persistence::Db::from_connection(conn).unwrap()).unwrap();
        let jobs: Vec<String> = store
            .lock()
            .unwrap()
            .prepare("SELECT job FROM model_entries")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(jobs, ["everyday"]);
    }

    #[test]
    fn an_existing_database_sheds_what_the_clean_slate_removed() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE checkin (id INTEGER PRIMARY KEY);
             CREATE TABLE night_loop_schedule (id INTEGER PRIMARY KEY);
             CREATE TABLE model_tune_schedule (id INTEGER PRIMARY KEY);
             CREATE TABLE notions (id INTEGER PRIMARY KEY);
             CREATE TABLE notion_citations (notion INTEGER);
             CREATE TABLE standing_trouble (entity TEXT PRIMARY KEY);
             CREATE TABLE specimens (id TEXT PRIMARY KEY, asked TEXT);
             CREATE TABLE beliefs (id TEXT PRIMARY KEY, statement TEXT);
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
            "beliefs",
            "execution_goals",
            // Research went with the chat (ADR 0136), and its leases with it.
            "execution_leases",
        ] {
            assert!(!tables.iter().any(|t| t == gone), "{gone} is still there");
        }
    }
}
