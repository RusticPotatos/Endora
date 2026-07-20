//! SQLite-backed implementations of the application repository ports.
//!
//! A single [`SqliteStore`] owns one connection behind a mutex and implements
//! the repository traits. Identifiers are `u128`, which SQLite integers cannot
//! hold, so they are stored as their decimal `TEXT` form and parsed back on
//! read. The store is synchronous; when the async node uses it, calls run on a
//! blocking thread (see `docs/adr/0007-async-web-stack.md`).

use std::sync::Mutex;

use serde_json::{Value as JsonValue, json};

use endora_application::{
    ActivityEvent, AssumptionRepository, AuditLog, AutonomyEnvelope, AutonomyEnvelopeRepository,
    BeliefRepository, ButlerProposal, CapabilityConfigRepository, ChatRepository,
    CheckinRepository, CheckinSchedule, DirectionRepository, EventLog, ExperimentRepository,
    MemorySnapshot, MemoryStore, ObservationRepository, PreferenceRepository,
    ProcessChangeRepository, ReflectionRepository, RepositoryError, Snooze, SnoozeRepository,
    Suggestion, SuggestionRepository, SuggestionStatus, TargetRepository, ValueRepository,
};
use endora_domain::{
    ApprovalState, Assumption, AssumptionId, AuditId, AuditRecord, Belief, BeliefId, BeliefKind,
    BeliefStatus, ChatMessage, Confidence, Direction, DirectionId, Experiment, ExperimentId,
    ExperimentStatus, LifecycleStatus, MessageId, MessageRole, Observation, ObservationId,
    Preference, PreferenceId, PreferenceKind, ProcessChangeId, ProposedProcessChange, Reflection,
    ReflectionId, SuggestionId, Target, TargetId, Timestamp, Value, ValueId,
};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS \"values\" (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS directions (
    id       TEXT PRIMARY KEY,
    title    TEXT NOT NULL,
    status   TEXT NOT NULL DEFAULT 'active',
    value_id TEXT REFERENCES \"values\"(id)
) STRICT;

CREATE TABLE IF NOT EXISTS targets (
    id           TEXT PRIMARY KEY,
    direction_id TEXT NOT NULL REFERENCES directions(id),
    statement    TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'active'
) STRICT;

CREATE INDEX IF NOT EXISTS idx_targets_direction ON targets(direction_id);

CREATE TABLE IF NOT EXISTS assumptions (
    id        TEXT PRIMARY KEY,
    target_id TEXT NOT NULL REFERENCES targets(id),
    statement TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_assumptions_target ON assumptions(target_id);

CREATE TABLE IF NOT EXISTS experiments (
    id            TEXT PRIMARY KEY,
    assumption_id TEXT NOT NULL REFERENCES assumptions(id),
    hypothesis    TEXT NOT NULL,
    status        TEXT NOT NULL,
    review_by_ms  INTEGER
) STRICT;

CREATE INDEX IF NOT EXISTS idx_experiments_assumption ON experiments(assumption_id);

CREATE TABLE IF NOT EXISTS observations (
    id            TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiments(id),
    note          TEXT NOT NULL,
    at_ms         INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_observations_experiment ON observations(experiment_id);

CREATE TABLE IF NOT EXISTS reflections (
    id      TEXT PRIMARY KEY,
    target_id TEXT NOT NULL REFERENCES targets(id),
    summary TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_reflections_target ON reflections(target_id);

-- A reflection's evidence, ordered, one row per cited observation.
CREATE TABLE IF NOT EXISTS reflection_evidence (
    reflection_id  TEXT NOT NULL REFERENCES reflections(id),
    ordinal        INTEGER NOT NULL,
    observation_id TEXT NOT NULL REFERENCES observations(id),
    PRIMARY KEY (reflection_id, ordinal)
) STRICT;

CREATE TABLE IF NOT EXISTS process_changes (
    id            TEXT PRIMARY KEY,
    reflection_id TEXT NOT NULL REFERENCES reflections(id),
    description   TEXT NOT NULL,
    approval      TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_process_changes_reflection
    ON process_changes(reflection_id);

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

CREATE INDEX IF NOT EXISTS idx_messages_at ON messages(at_ms);

CREATE TABLE IF NOT EXISTS attention_snoozes (
    kind     TEXT NOT NULL,
    subject  TEXT NOT NULL,
    count    INTEGER NOT NULL,
    until_ms INTEGER NOT NULL,
    PRIMARY KEY (kind, subject)
) STRICT;

CREATE TABLE IF NOT EXISTS preferences (
    id    TEXT PRIMARY KEY,
    body  TEXT NOT NULL,
    kind  TEXT NOT NULL,
    at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS suggestions (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    payload      TEXT NOT NULL,
    status       TEXT NOT NULL,
    from_message TEXT,
    created_ms   INTEGER NOT NULL,
    decided_ms   INTEGER
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
CREATE TABLE IF NOT EXISTS capability_config (
    id      TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS autonomy_envelope (
    id               INTEGER PRIMARY KEY CHECK (id = 0),
    auto_external    INTEGER NOT NULL,
    auto_consequential INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS events (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    at_ms   INTEGER NOT NULL,
    summary TEXT NOT NULL
) STRICT;
";

/// A SQLite-backed store implementing the persistence ports.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Opens (creating if needed) a store at `path` and applies the schema.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be opened or migrated.
    pub fn open(path: &str) -> Result<Self, RepositoryError> {
        let conn = Connection::open(path).map_err(backend)?;
        Self::from_connection(conn)
    }

    /// Opens a private in-memory store, mainly for tests.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the database cannot be created or migrated.
    pub fn open_in_memory() -> Result<Self, RepositoryError> {
        let conn = Connection::open_in_memory().map_err(backend)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, RepositoryError> {
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(backend)?;
        // Rename a pre-rename `goals`/`goal_id` schema to `targets`/`target_id`
        // *before* creating any tables, so the schema below does not create an
        // empty `targets` alongside the old data. (Goal was renamed to Target.)
        migrate_goals_to_targets(&conn)?;
        conn.execute_batch(SCHEMA).map_err(backend)?;
        // Migrations for databases created by an earlier schema version. The
        // review index is created only once the column is guaranteed present,
        // so opening a pre-review database does not fail on the missing column.
        ensure_column(&conn, "experiments", "review_by_ms", "INTEGER")?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_experiments_review ON experiments(review_by_ms);",
        )
        .map_err(backend)?;
        // Lifecycle status on North Stars and Targets; existing rows default to
        // 'active'.
        ensure_column(
            &conn,
            "directions",
            "status",
            "TEXT NOT NULL DEFAULT 'active'",
        )?;
        ensure_column(&conn, "targets", "status", "TEXT NOT NULL DEFAULT 'active'")?;
        // The Value link on North Stars (nullable; existing rows read back unfiled).
        ensure_column(
            &conn,
            "directions",
            "value_id",
            "TEXT REFERENCES \"values\"(id)",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, RepositoryError> {
        self.conn
            .lock()
            .map_err(|_| RepositoryError::Backend("connection lock poisoned".to_owned()))
    }
}

impl DirectionRepository for SqliteStore {
    fn save(&self, direction: &Direction) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO directions (id, title, status, value_id) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(direction.id().value()),
                direction.title(),
                direction.status().name(),
                direction.value().map(|v| id_text(v.value()))
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String, Option<String>)> = conn
            .query_row(
                "SELECT title, status, value_id FROM directions WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((title, status, value_id)) = row else {
            return Ok(None);
        };
        let direction = Direction::from_parts(
            id,
            &title,
            parse_lifecycle(&status)?,
            parse_opt_value(value_id)?,
        )
        .map_err(corrupt)?;
        Ok(Some(direction))
    }

    fn list_all(&self) -> Result<Vec<Direction>, RepositoryError> {
        let conn = self.lock()?;
        all_directions(&conn)
    }

    fn delete(&self, id: DirectionId) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM directions WHERE id = ?1",
            params![id_text(id.value())],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl ValueRepository for SqliteStore {
    fn save(&self, value: &Value) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO \"values\" (id, name) VALUES (?1, ?2)",
            params![id_text(value.id().value()), value.name()],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: ValueId) -> Result<Option<Value>, RepositoryError> {
        let conn = self.lock()?;
        let name: Option<String> = conn
            .query_row(
                "SELECT name FROM \"values\" WHERE id = ?1",
                params![id_text(id.value())],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(name) = name else {
            return Ok(None);
        };
        Ok(Some(Value::new(id, &name).map_err(corrupt)?))
    }

    fn list_all(&self) -> Result<Vec<Value>, RepositoryError> {
        let conn = self.lock()?;
        all_values(&conn)
    }

    fn delete(&self, id: ValueId) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM \"values\" WHERE id = ?1",
            params![id_text(id.value())],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl TargetRepository for SqliteStore {
    fn save(&self, target: &Target) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO targets (id, direction_id, statement, status) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(target.id().value()),
                id_text(target.direction().value()),
                target.statement(),
                target.status().name()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: TargetId) -> Result<Option<Target>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT direction_id, statement, status FROM targets WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((direction_id, statement, status)) = row else {
            return Ok(None);
        };
        let direction = DirectionId::new(parse_id(&direction_id)?);
        let target = Target::from_parts(id, direction, &statement, parse_lifecycle(&status)?)
            .map_err(corrupt)?;
        Ok(Some(target))
    }

    fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Target>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, statement, status FROM targets WHERE direction_id = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(direction.value())], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;

        let mut targets = Vec::new();
        for row in rows {
            let (id_text, statement, status) = row.map_err(backend)?;
            let target = Target::from_parts(
                TargetId::new(parse_id(&id_text)?),
                direction,
                &statement,
                parse_lifecycle(&status)?,
            )
            .map_err(corrupt)?;
            targets.push(target);
        }
        Ok(targets)
    }

    fn delete(&self, id: TargetId) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM targets WHERE id = ?1",
            params![id_text(id.value())],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl AssumptionRepository for SqliteStore {
    fn save(&self, assumption: &Assumption) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO assumptions (id, target_id, statement) VALUES (?1, ?2, ?3)",
            params![
                id_text(assumption.id().value()),
                id_text(assumption.target().value()),
                assumption.statement()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: AssumptionId) -> Result<Option<Assumption>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT target_id, statement FROM assumptions WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((target_id, statement)) = row else {
            return Ok(None);
        };
        let target = TargetId::new(parse_id(&target_id)?);
        let assumption = Assumption::new(id, target, &statement).map_err(corrupt)?;
        Ok(Some(assumption))
    }

    fn list_for_target(&self, target: TargetId) -> Result<Vec<Assumption>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, statement FROM assumptions WHERE target_id = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(target.value())], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?;

        let mut assumptions = Vec::new();
        for row in rows {
            let (id, statement) = row.map_err(backend)?;
            let assumption = Assumption::new(AssumptionId::new(parse_id(&id)?), target, &statement)
                .map_err(corrupt)?;
            assumptions.push(assumption);
        }
        Ok(assumptions)
    }
}

impl ExperimentRepository for SqliteStore {
    fn save(&self, experiment: &Experiment) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO experiments (id, assumption_id, hypothesis, status, review_by_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id_text(experiment.id().value()),
                id_text(experiment.assumption().value()),
                experiment.hypothesis(),
                experiment.status().name(),
                experiment.review_by().map(|t| t.unix_millis())
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: ExperimentId) -> Result<Option<Experiment>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String, String, Option<i64>)> = conn
            .query_row(
                "SELECT assumption_id, hypothesis, status, review_by_ms FROM experiments WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((assumption_id, hypothesis, status, review_by_ms)) = row else {
            return Ok(None);
        };
        let assumption = AssumptionId::new(parse_id(&assumption_id)?);
        Ok(Some(build_experiment(
            id,
            assumption,
            &hypothesis,
            &status,
            review_by_ms,
        )?))
    }

    fn list_for_assumption(
        &self,
        assumption: AssumptionId,
    ) -> Result<Vec<Experiment>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, hypothesis, status, review_by_ms FROM experiments \
                 WHERE assumption_id = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(assumption.value())], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(backend)?;

        let mut experiments = Vec::new();
        for row in rows {
            let (id, hypothesis, status, review_by_ms) = row.map_err(backend)?;
            experiments.push(build_experiment(
                ExperimentId::new(parse_id(&id)?),
                assumption,
                &hypothesis,
                &status,
                review_by_ms,
            )?);
        }
        Ok(experiments)
    }

    fn list_due_reviews(&self, now: Timestamp) -> Result<Vec<Experiment>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, assumption_id, hypothesis, status, review_by_ms FROM experiments \
                 WHERE review_by_ms IS NOT NULL AND review_by_ms <= ?1 AND status != 'concluded' \
                 ORDER BY review_by_ms, id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![now.unix_millis()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })
            .map_err(backend)?;

        let mut experiments = Vec::new();
        for row in rows {
            let (id, assumption_id, hypothesis, status, review_by_ms) = row.map_err(backend)?;
            experiments.push(build_experiment(
                ExperimentId::new(parse_id(&id)?),
                AssumptionId::new(parse_id(&assumption_id)?),
                &hypothesis,
                &status,
                review_by_ms,
            )?);
        }
        Ok(experiments)
    }
}

impl ObservationRepository for SqliteStore {
    fn save(&self, observation: &Observation) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO observations (id, experiment_id, note, at_ms) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(observation.id().value()),
                id_text(observation.experiment().value()),
                observation.note(),
                observation.recorded_at().unix_millis()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn list_for_experiment(
        &self,
        experiment: ExperimentId,
    ) -> Result<Vec<Observation>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, note, at_ms FROM observations \
                 WHERE experiment_id = ?1 ORDER BY at_ms, id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(experiment.value())], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(backend)?;

        let mut observations = Vec::new();
        for row in rows {
            let (id, note, at_ms) = row.map_err(backend)?;
            let observation = Observation::record(
                ObservationId::new(parse_id(&id)?),
                experiment,
                &note,
                Timestamp::from_unix_millis(at_ms),
            )
            .map_err(corrupt)?;
            observations.push(observation);
        }
        Ok(observations)
    }

    fn recent(&self, limit: usize) -> Result<Vec<Observation>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, experiment_id, note, at_ms FROM observations \
                 ORDER BY at_ms DESC, id DESC LIMIT ?1",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(backend)?;

        let mut observations = Vec::new();
        for row in rows {
            let (id, experiment_id, note, at_ms) = row.map_err(backend)?;
            observations.push(
                Observation::record(
                    ObservationId::new(parse_id(&id)?),
                    ExperimentId::new(parse_id(&experiment_id)?),
                    &note,
                    Timestamp::from_unix_millis(at_ms),
                )
                .map_err(corrupt)?,
            );
        }
        Ok(observations)
    }
}

impl ReflectionRepository for SqliteStore {
    fn save(&self, reflection: &Reflection) -> Result<(), RepositoryError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        let rid = id_text(reflection.id().value());
        tx.execute(
            "INSERT OR REPLACE INTO reflections (id, target_id, summary) VALUES (?1, ?2, ?3)",
            params![
                rid,
                id_text(reflection.target().value()),
                reflection.summary()
            ],
        )
        .map_err(backend)?;
        tx.execute(
            "DELETE FROM reflection_evidence WHERE reflection_id = ?1",
            params![rid],
        )
        .map_err(backend)?;
        for (ordinal, observation) in reflection.evidence().iter().enumerate() {
            tx.execute(
                "INSERT INTO reflection_evidence (reflection_id, ordinal, observation_id) \
                 VALUES (?1, ?2, ?3)",
                params![
                    rid,
                    i64::try_from(ordinal).unwrap_or(i64::MAX),
                    id_text(observation.value())
                ],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: ReflectionId) -> Result<Option<Reflection>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT target_id, summary FROM reflections WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((target_id, summary)) = row else {
            return Ok(None);
        };
        let evidence = evidence_for(&conn, id)?;
        let target = TargetId::new(parse_id(&target_id)?);
        let reflection = Reflection::new(id, target, &summary, evidence).map_err(corrupt)?;
        Ok(Some(reflection))
    }

    fn list_for_target(&self, target: TargetId) -> Result<Vec<Reflection>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, summary FROM reflections WHERE target_id = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows: Vec<(String, String)> = stmt
            .query_map(params![id_text(target.value())], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?;
        drop(stmt);

        let mut reflections = Vec::new();
        for (id, summary) in rows {
            let reflection_id = ReflectionId::new(parse_id(&id)?);
            let evidence = evidence_for(&conn, reflection_id)?;
            let reflection =
                Reflection::new(reflection_id, target, &summary, evidence).map_err(corrupt)?;
            reflections.push(reflection);
        }
        Ok(reflections)
    }
}

impl ProcessChangeRepository for SqliteStore {
    fn save(&self, change: &ProposedProcessChange) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO process_changes (id, reflection_id, description, approval) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(change.id().value()),
                id_text(change.reflection().value()),
                change.description(),
                change.approval().name()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: ProcessChangeId) -> Result<Option<ProposedProcessChange>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT reflection_id, description, approval FROM process_changes WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((reflection_id, description, approval)) = row else {
            return Ok(None);
        };
        let reflection = ReflectionId::new(parse_id(&reflection_id)?);
        let change = ProposedProcessChange::from_parts(
            id,
            reflection,
            &description,
            parse_approval(&approval)?,
        )
        .map_err(corrupt)?;
        Ok(Some(change))
    }

    fn list_for_reflection(
        &self,
        reflection: ReflectionId,
    ) -> Result<Vec<ProposedProcessChange>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, description, approval FROM process_changes \
                 WHERE reflection_id = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(reflection.value())], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;

        let mut changes = Vec::new();
        for row in rows {
            let (id, description, approval) = row.map_err(backend)?;
            let change = ProposedProcessChange::from_parts(
                ProcessChangeId::new(parse_id(&id)?),
                reflection,
                &description,
                parse_approval(&approval)?,
            )
            .map_err(corrupt)?;
            changes.push(change);
        }
        Ok(changes)
    }
}

impl MemoryStore for SqliteStore {
    fn export(&self) -> Result<MemorySnapshot, RepositoryError> {
        let conn = self.lock()?;
        Ok(MemorySnapshot {
            values: all_values(&conn)?,
            directions: all_directions(&conn)?,
            targets: all_targets(&conn)?,
            assumptions: all_assumptions(&conn)?,
            experiments: all_experiments(&conn)?,
            observations: all_observations(&conn)?,
            reflections: all_reflections(&conn)?,
            process_changes: all_process_changes(&conn)?,
            audit: all_audit(&conn)?,
            messages: all_messages(&conn)?,
            preferences: all_preferences(&conn)?,
            suggestions: all_suggestions(&conn, None)?,
            beliefs: all_beliefs(&conn)?,
        })
    }

    fn purge(&self) -> Result<(), RepositoryError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        // Delete children before parents so foreign keys stay satisfied.
        for table in [
            "reflection_evidence",
            "process_changes",
            "reflections",
            "observations",
            "experiments",
            "assumptions",
            "targets",
            "directions",
            "\"values\"",
            "audit_log",
            "messages",
            "attention_snoozes",
            "preferences",
            "suggestions",
            "checkin",
            "beliefs",
            "events",
            "capability_config",
            "autonomy_envelope",
        ] {
            tx.execute(&format!("DELETE FROM {table}"), [])
                .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }
}

impl AuditLog for SqliteStore {
    fn append(&self, record: &AuditRecord) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO audit_log (id, at_ms, summary) VALUES (?1, ?2, ?3)",
            params![
                id_text(record.id().value()),
                record.at().unix_millis(),
                record.summary()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn recent(&self, limit: usize) -> Result<Vec<AuditRecord>, RepositoryError> {
        let conn = self.lock()?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = conn
            .prepare(
                "SELECT id, at_ms, summary FROM audit_log ORDER BY at_ms DESC, id DESC LIMIT ?1",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;

        let mut records = Vec::new();
        for row in rows {
            let (id, at_ms, summary) = row.map_err(backend)?;
            let record = AuditRecord::new(
                AuditId::new(parse_id(&id)?),
                Timestamp::from_unix_millis(at_ms),
                &summary,
            )
            .map_err(corrupt)?;
            records.push(record);
        }
        Ok(records)
    }
}

impl SnoozeRepository for SqliteStore {
    fn get(&self, kind: &str, subject: &str) -> Result<Option<Snooze>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(i64, i64)> = conn
            .query_row(
                "SELECT count, until_ms FROM attention_snoozes WHERE kind = ?1 AND subject = ?2",
                params![kind, subject],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        Ok(row.map(|(count, until)| Snooze {
            count: u32::try_from(count).unwrap_or(0),
            until: Timestamp::from_unix_millis(until),
        }))
    }

    fn set(&self, kind: &str, subject: &str, snooze: Snooze) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO attention_snoozes (kind, subject, count, until_ms) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                kind,
                subject,
                i64::from(snooze.count),
                snooze.until.unix_millis()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl ChatRepository for SqliteStore {
    fn append(&self, message: &ChatMessage) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO messages (id, role, body, at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(message.id().value()),
                message.role().name(),
                message.text(),
                message.at().unix_millis()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<ChatMessage>, RepositoryError> {
        let conn = self.lock()?;
        all_messages(&conn)
    }
}

impl PreferenceRepository for SqliteStore {
    fn save(&self, preference: &Preference) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO preferences (id, body, kind, at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(preference.id().value()),
                preference.text(),
                preference.kind().name(),
                preference.at().unix_millis()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn list_all(&self) -> Result<Vec<Preference>, RepositoryError> {
        let conn = self.lock()?;
        all_preferences(&conn)
    }

    fn delete(&self, id: PreferenceId) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM preferences WHERE id = ?1",
            params![id_text(id.value())],
        )
        .map_err(backend)?;
        Ok(())
    }
}

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

impl SuggestionRepository for SqliteStore {
    fn save(&self, suggestion: &Suggestion) -> Result<(), RepositoryError> {
        let (kind, payload) = proposal_to_row(&suggestion.proposal);
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO suggestions \
             (id, kind, payload, status, from_message, created_ms, decided_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id_text(suggestion.id.value()),
                kind,
                payload,
                suggestion.status.name(),
                suggestion.from_message.map(|m| id_text(m.value())),
                suggestion.created_at.unix_millis(),
                suggestion.decided_at.map(Timestamp::unix_millis),
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: SuggestionId) -> Result<Option<Suggestion>, RepositoryError> {
        let conn = self.lock()?;
        let mut found = all_suggestions(&conn, None)?;
        Ok(found.drain(..).find(|s| s.id == id))
    }

    fn list(&self, status: Option<SuggestionStatus>) -> Result<Vec<Suggestion>, RepositoryError> {
        let conn = self.lock()?;
        all_suggestions(&conn, status)
    }
}

impl CheckinRepository for SqliteStore {
    fn get(&self) -> Result<Option<CheckinSchedule>, RepositoryError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT enabled, interval_ms, next_at_ms FROM checkin WHERE id = 0",
            [],
            |r| {
                Ok(CheckinSchedule {
                    enabled: r.get::<_, i64>(0)? != 0,
                    interval_ms: r.get::<_, i64>(1)?,
                    next_at: Timestamp::from_unix_millis(r.get::<_, i64>(2)?),
                })
            },
        )
        .optional()
        .map_err(backend)
    }

    fn set(&self, schedule: &CheckinSchedule) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO checkin (id, enabled, interval_ms, next_at_ms) \
             VALUES (0, ?1, ?2, ?3)",
            params![
                i64::from(schedule.enabled),
                schedule.interval_ms,
                schedule.next_at.unix_millis()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl EventLog for SqliteStore {
    fn record(&self, at: Timestamp, summary: &str) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO events (at_ms, summary) VALUES (?1, ?2)",
            params![at.unix_millis(), summary],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn recent(&self, limit: usize) -> Result<Vec<ActivityEvent>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT at_ms, summary FROM events ORDER BY id DESC LIMIT ?1")
            .map_err(backend)?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok(ActivityEvent {
                    at: Timestamp::from_unix_millis(r.get::<_, i64>(0)?),
                    summary: r.get::<_, String>(1)?,
                })
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }
}

impl AutonomyEnvelopeRepository for SqliteStore {
    fn get(&self) -> Result<AutonomyEnvelope, RepositoryError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT auto_external, auto_consequential FROM autonomy_envelope WHERE id = 0",
            [],
            |r| {
                Ok(AutonomyEnvelope {
                    auto_external: r.get::<_, i64>(0)? != 0,
                    auto_consequential: r.get::<_, i64>(1)? != 0,
                })
            },
        )
        .optional()
        .map_err(backend)
        .map(Option::unwrap_or_default)
    }

    fn set(&self, envelope: &AutonomyEnvelope) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO autonomy_envelope (id, auto_external, auto_consequential) \
             VALUES (0, ?1, ?2)",
            params![
                i64::from(envelope.auto_external),
                i64::from(envelope.auto_consequential)
            ],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl CapabilityConfigRepository for SqliteStore {
    fn enabled_overrides(&self) -> Result<Vec<(String, bool)>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, enabled FROM capability_config")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
            })
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO capability_config (id, enabled) VALUES (?1, ?2)",
            params![id, i64::from(enabled)],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl BeliefRepository for SqliteStore {
    fn save(&self, belief: &Belief) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO beliefs \
             (id, statement, kind, confidence, evidence, created_ms, last_affirmed_ms, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id_text(belief.id().value()),
                belief.statement(),
                belief.kind().name(),
                belief.confidence().name(),
                belief.evidence(),
                belief.created_at().unix_millis(),
                belief.last_affirmed_at().unix_millis(),
                belief.status().name(),
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: BeliefId) -> Result<Option<Belief>, RepositoryError> {
        let conn = self.lock()?;
        Ok(all_beliefs(&conn)?.into_iter().find(|b| b.id() == id))
    }

    fn list(&self) -> Result<Vec<Belief>, RepositoryError> {
        let conn = self.lock()?;
        all_beliefs(&conn)
    }
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

/// Serializes a proposal to its stored `(kind, payload-json)` form.
fn proposal_to_row(p: &ButlerProposal) -> (&'static str, String) {
    match p {
        ButlerProposal::CreateValue { name } => {
            ("create_value", json!({ "name": name }).to_string())
        }
        ButlerProposal::CreateNorthStar { title } => {
            ("create_north_star", json!({ "title": title }).to_string())
        }
        ButlerProposal::CreateTarget {
            direction_ref,
            statement,
        } => (
            "create_target",
            json!({ "direction_ref": direction_ref, "statement": statement }).to_string(),
        ),
        ButlerProposal::RememberPreference { text, kind } => (
            "remember_preference",
            json!({ "text": text, "preference_kind": kind.name() }).to_string(),
        ),
    }
}

/// Reconstructs a proposal from its stored `(kind, payload-json)` form.
fn row_to_proposal(kind: &str, payload: &str) -> Result<ButlerProposal, RepositoryError> {
    let v: JsonValue = serde_json::from_str(payload)
        .map_err(|e| RepositoryError::Corrupt(format!("bad suggestion payload: {e}")))?;
    let s = |k: &str| {
        v.get(k)
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match kind {
        "create_value" => Ok(ButlerProposal::CreateValue { name: s("name") }),
        "create_north_star" => Ok(ButlerProposal::CreateNorthStar { title: s("title") }),
        "create_target" => Ok(ButlerProposal::CreateTarget {
            direction_ref: s("direction_ref"),
            statement: s("statement"),
        }),
        "remember_preference" => Ok(ButlerProposal::RememberPreference {
            text: s("text"),
            kind: PreferenceKind::from_name(&s("preference_kind")).unwrap_or(PreferenceKind::Taste),
        }),
        other => Err(RepositoryError::Corrupt(format!(
            "unknown suggestion kind {other:?}"
        ))),
    }
}

fn all_suggestions(
    conn: &Connection,
    status: Option<SuggestionStatus>,
) -> Result<Vec<Suggestion>, RepositoryError> {
    // Newest first; rowid breaks same-millisecond ties.
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, payload, status, from_message, created_ms, decided_ms \
             FROM suggestions ORDER BY created_ms DESC, rowid DESC",
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
                r.get::<_, Option<i64>>(6)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, kind, payload, status_s, from_message, created_ms, decided_ms) =
            row.map_err(backend)?;
        let st = SuggestionStatus::from_name(&status_s).ok_or_else(|| {
            RepositoryError::Corrupt(format!("unknown suggestion status {status_s:?}"))
        })?;
        if let Some(want) = status {
            if st != want {
                continue;
            }
        }
        let from_message = match from_message {
            Some(m) => Some(MessageId::new(parse_id(&m)?)),
            None => None,
        };
        out.push(Suggestion {
            id: SuggestionId::new(parse_id(&id)?),
            proposal: row_to_proposal(&kind, &payload)?,
            status: st,
            from_message,
            created_at: Timestamp::from_unix_millis(created_ms),
            decided_at: decided_ms.map(Timestamp::from_unix_millis),
        });
    }
    Ok(out)
}

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

/// Renders a `u128` identifier as the decimal `TEXT` used for storage.
fn id_text(value: u128) -> String {
    value.to_string()
}

/// Parses a stored identifier back into a `u128`, or reports corruption.
fn parse_id(text: &str) -> Result<u128, RepositoryError> {
    text.parse::<u128>()
        .map_err(|e| RepositoryError::Corrupt(format!("invalid stored id {text:?}: {e}")))
}

fn all_directions(conn: &Connection) -> Result<Vec<Direction>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, title, status, value_id FROM directions ORDER BY id")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, status, value_id) = row.map_err(backend)?;
        out.push(
            Direction::from_parts(
                DirectionId::new(parse_id(&id)?),
                &title,
                parse_lifecycle(&status)?,
                parse_opt_value(value_id)?,
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

fn all_values(conn: &Connection) -> Result<Vec<Value>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, name FROM \"values\" ORDER BY id")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name) = row.map_err(backend)?;
        out.push(Value::new(ValueId::new(parse_id(&id)?), &name).map_err(corrupt)?);
    }
    Ok(out)
}

/// Parses an optional stored value id (the North Star's `value_id` column).
fn parse_opt_value(raw: Option<String>) -> Result<Option<ValueId>, RepositoryError> {
    raw.map(|s| parse_id(&s).map(ValueId::new)).transpose()
}

fn all_targets(conn: &Connection) -> Result<Vec<Target>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, direction_id, statement, status FROM targets ORDER BY id")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, direction, statement, status) = row.map_err(backend)?;
        let target = Target::from_parts(
            TargetId::new(parse_id(&id)?),
            DirectionId::new(parse_id(&direction)?),
            &statement,
            parse_lifecycle(&status)?,
        )
        .map_err(corrupt)?;
        out.push(target);
    }
    Ok(out)
}

fn all_assumptions(conn: &Connection) -> Result<Vec<Assumption>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, target_id, statement FROM assumptions ORDER BY id")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, target, statement) = row.map_err(backend)?;
        out.push(
            Assumption::new(
                AssumptionId::new(parse_id(&id)?),
                TargetId::new(parse_id(&target)?),
                &statement,
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

fn all_experiments(conn: &Connection) -> Result<Vec<Experiment>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, assumption_id, hypothesis, status, review_by_ms \
             FROM experiments ORDER BY id",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, assumption, hypothesis, status, review_by_ms) = row.map_err(backend)?;
        out.push(build_experiment(
            ExperimentId::new(parse_id(&id)?),
            AssumptionId::new(parse_id(&assumption)?),
            &hypothesis,
            &status,
            review_by_ms,
        )?);
    }
    Ok(out)
}

/// Reconstitutes an [`Experiment`] from its persisted columns, mapping the
/// stored millisecond timestamp back into a domain [`Timestamp`].
fn build_experiment(
    id: ExperimentId,
    assumption: AssumptionId,
    hypothesis: &str,
    status: &str,
    review_by_ms: Option<i64>,
) -> Result<Experiment, RepositoryError> {
    Experiment::from_parts(
        id,
        assumption,
        hypothesis,
        parse_status(status)?,
        review_by_ms.map(Timestamp::from_unix_millis),
    )
    .map_err(corrupt)
}

fn all_observations(conn: &Connection) -> Result<Vec<Observation>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, experiment_id, note, at_ms FROM observations ORDER BY at_ms, id")
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
        let (id, experiment, note, at_ms) = row.map_err(backend)?;
        out.push(
            Observation::record(
                ObservationId::new(parse_id(&id)?),
                ExperimentId::new(parse_id(&experiment)?),
                &note,
                Timestamp::from_unix_millis(at_ms),
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

fn all_reflections(conn: &Connection) -> Result<Vec<Reflection>, RepositoryError> {
    let rows: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, target_id, summary FROM reflections ORDER BY id")
            .map_err(backend)?;
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(backend)?
        .collect::<Result<_, _>>()
        .map_err(backend)?
    };
    let mut out = Vec::new();
    for (id, target, summary) in rows {
        let reflection_id = ReflectionId::new(parse_id(&id)?);
        let evidence = evidence_for(conn, reflection_id)?;
        out.push(
            Reflection::new(
                reflection_id,
                TargetId::new(parse_id(&target)?),
                &summary,
                evidence,
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

fn all_process_changes(conn: &Connection) -> Result<Vec<ProposedProcessChange>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, reflection_id, description, approval FROM process_changes ORDER BY id")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, reflection, description, approval) = row.map_err(backend)?;
        out.push(
            ProposedProcessChange::from_parts(
                ProcessChangeId::new(parse_id(&id)?),
                ReflectionId::new(parse_id(&reflection)?),
                &description,
                parse_approval(&approval)?,
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
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
fn evidence_for(
    conn: &Connection,
    reflection: ReflectionId,
) -> Result<Vec<ObservationId>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT observation_id FROM reflection_evidence \
             WHERE reflection_id = ?1 ORDER BY ordinal",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map(params![id_text(reflection.value())], |row| {
            row.get::<_, String>(0)
        })
        .map_err(backend)?;
    let mut evidence = Vec::new();
    for row in rows {
        evidence.push(ObservationId::new(parse_id(&row.map_err(backend)?)?));
    }
    Ok(evidence)
}

/// Parses a stored experiment status, or reports corruption.
fn parse_status(text: &str) -> Result<ExperimentStatus, RepositoryError> {
    ExperimentStatus::from_name(text)
        .ok_or_else(|| RepositoryError::Corrupt(format!("unknown experiment status {text:?}")))
}

/// Parses a stored lifecycle status, or reports corruption.
fn parse_lifecycle(text: &str) -> Result<LifecycleStatus, RepositoryError> {
    LifecycleStatus::from_name(text)
        .ok_or_else(|| RepositoryError::Corrupt(format!("unknown lifecycle status {text:?}")))
}

/// Parses a stored approval state, or reports corruption.
fn parse_approval(text: &str) -> Result<ApprovalState, RepositoryError> {
    ApprovalState::from_name(text)
        .ok_or_else(|| RepositoryError::Corrupt(format!("unknown approval state {text:?}")))
}

/// Adds `column` to `table` if it is not already present.
///
/// A minimal forward migration for STRICT tables: adding a nullable column to
/// databases created before the column existed. Existing rows read back `NULL`.
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

/// Renames a pre-rename `goals` table and its `goal_id` foreign-key columns to
/// `targets`/`target_id`.
///
/// The domain concept "Goal" was renamed to "Target"; this carries existing
/// databases across without data loss. A no-op on a fresh database (no `goals`
/// table) or one already migrated (a `targets` table exists). Renaming the table
/// updates the foreign keys that reference it; the old-named indexes are dropped
/// so the schema can recreate them under their new names.
fn migrate_goals_to_targets(conn: &Connection) -> Result<(), RepositoryError> {
    if !table_exists(conn, "goals")? || table_exists(conn, "targets")? {
        return Ok(());
    }
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_goals_direction;
         DROP INDEX IF EXISTS idx_assumptions_goal;
         DROP INDEX IF EXISTS idx_reflections_goal;
         ALTER TABLE goals RENAME TO targets;
         ALTER TABLE assumptions RENAME COLUMN goal_id TO target_id;
         ALTER TABLE reflections RENAME COLUMN goal_id TO target_id;",
    )
    .map_err(backend)?;
    Ok(())
}

/// Whether a table named `name` exists in the database.
fn table_exists(conn: &Connection, name: &str) -> Result<bool, RepositoryError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
            |row| row.get(0),
        )
        .map_err(backend)?;
    Ok(count > 0)
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
    use endora_application::{DirectionRepository, TargetRepository};
    use endora_domain::{Direction, DirectionId, Target, TargetId};

    fn store() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    #[test]
    fn direction_round_trips() {
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        let direction = Direction::new(DirectionId::new(1), "Be healthier").unwrap();
        repo.save(&direction).unwrap();
        assert_eq!(repo.get(DirectionId::new(1)).unwrap(), Some(direction));
    }

    #[test]
    fn missing_direction_is_none() {
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        assert_eq!(repo.get(DirectionId::new(99)).unwrap(), None);
    }

    #[test]
    fn save_replaces_existing_direction() {
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        repo.save(&Direction::new(DirectionId::new(1), "First").unwrap())
            .unwrap();
        repo.save(&Direction::new(DirectionId::new(1), "Second").unwrap())
            .unwrap();
        assert_eq!(
            repo.get(DirectionId::new(1)).unwrap().unwrap().title(),
            "Second"
        );
    }

    #[test]
    fn target_round_trips_and_lists_by_direction() {
        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let targets: &dyn TargetRepository = &store;
        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();

        let g1 = Target::new(TargetId::new(10), direction, "Run a 5k").unwrap();
        let g2 = Target::new(TargetId::new(11), direction, "Sleep 8h").unwrap();
        targets.save(&g1).unwrap();
        targets.save(&g2).unwrap();

        assert_eq!(targets.get(TargetId::new(10)).unwrap(), Some(g1.clone()));
        assert_eq!(targets.list_for_direction(direction).unwrap(), vec![g1, g2]);
    }

    #[test]
    fn assumptions_round_trip_and_list_by_target() {
        use endora_application::AssumptionRepository;
        use endora_domain::{Assumption, AssumptionId, TargetId};

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let targets: &dyn TargetRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;

        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let target = TargetId::new(2);
        targets
            .save(&Target::new(target, direction, "Run a 5k").unwrap())
            .unwrap();

        let a = Assumption::new(AssumptionId::new(3), target, "Mornings are freest").unwrap();
        assumptions.save(&a).unwrap();
        assert_eq!(assumptions.list_for_target(target).unwrap(), vec![a]);
        assert_eq!(
            assumptions.list_for_target(TargetId::new(999)).unwrap(),
            vec![]
        );
    }

    #[test]
    fn experiment_status_survives_a_reload() {
        use endora_application::{AssumptionRepository, ExperimentRepository};
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, ExperimentStatus, TargetId,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let targets: &dyn TargetRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;
        let experiments: &dyn ExperimentRepository = &store;

        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let target = TargetId::new(2);
        targets
            .save(&Target::new(target, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        assumptions
            .save(&Assumption::new(assumption, target, "Mornings are freest").unwrap())
            .unwrap();

        // Save a Running experiment; a plain constructor could not rebuild this.
        let mut e = Experiment::propose(ExperimentId::new(4), assumption, "Try mornings").unwrap();
        e.start().unwrap();
        experiments.save(&e).unwrap();

        let loaded = experiments.get(ExperimentId::new(4)).unwrap().unwrap();
        assert_eq!(loaded.status(), ExperimentStatus::Running);
        assert_eq!(
            experiments.list_for_assumption(assumption).unwrap(),
            vec![loaded]
        );
    }

    #[test]
    fn scheduled_review_round_trips_and_lists_when_due() {
        use endora_application::{AssumptionRepository, ExperimentRepository};
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, TargetId, Timestamp,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let targets: &dyn TargetRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;
        let experiments: &dyn ExperimentRepository = &store;

        directions
            .save(&Direction::new(DirectionId::new(1), "Be healthier").unwrap())
            .unwrap();
        targets
            .save(&Target::new(TargetId::new(2), DirectionId::new(1), "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        assumptions
            .save(&Assumption::new(assumption, TargetId::new(2), "Mornings").unwrap())
            .unwrap();

        let mut e = Experiment::propose(ExperimentId::new(4), assumption, "Try mornings").unwrap();
        e.schedule_review(Timestamp::from_unix_millis(1_000));
        experiments.save(&e).unwrap();

        // The due time survives a reload.
        let loaded = experiments.get(ExperimentId::new(4)).unwrap().unwrap();
        assert_eq!(loaded.review_by(), Some(Timestamp::from_unix_millis(1_000)));

        // Not yet due before its time; due once the time arrives.
        assert!(
            experiments
                .list_due_reviews(Timestamp::from_unix_millis(999))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            experiments
                .list_due_reviews(Timestamp::from_unix_millis(1_000))
                .unwrap(),
            vec![loaded]
        );
    }

    #[test]
    fn opening_a_pre_review_database_migrates_the_column() {
        use endora_application::ExperimentRepository;
        use endora_domain::ExperimentId;
        use rusqlite::Connection;

        // Simulate a database created before `review_by_ms` existed: the
        // experiments table has the old four columns and a stored row.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE experiments (
                id            TEXT PRIMARY KEY,
                assumption_id TEXT NOT NULL,
                hypothesis    TEXT NOT NULL,
                status        TEXT NOT NULL
            ) STRICT;
            INSERT INTO experiments (id, assumption_id, hypothesis, status)
            VALUES ('4', '3', 'Try mornings', 'proposed');",
        )
        .unwrap();

        // Opening the store must add the column and read the legacy row back
        // with no review scheduled.
        let store = SqliteStore::from_connection(conn).unwrap();
        let experiments: &dyn ExperimentRepository = &store;
        let loaded = experiments.get(ExperimentId::new(4)).unwrap().unwrap();
        assert_eq!(loaded.review_by(), None);
    }

    #[test]
    fn opening_a_pre_rename_database_migrates_goals_to_targets() {
        use endora_application::{AssumptionRepository, TargetRepository};
        use endora_domain::{AssumptionId, DirectionId, TargetId};
        use rusqlite::Connection;

        // A database created before Goal was renamed to Target: a `goals` table
        // and `goal_id` foreign keys, with a direction, a goal, and an assumption.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE directions (id TEXT PRIMARY KEY, title TEXT NOT NULL) STRICT;
             CREATE TABLE goals (
                id           TEXT PRIMARY KEY,
                direction_id TEXT NOT NULL REFERENCES directions(id),
                statement    TEXT NOT NULL
             ) STRICT;
             CREATE INDEX idx_goals_direction ON goals(direction_id);
             CREATE TABLE assumptions (
                id        TEXT PRIMARY KEY,
                goal_id   TEXT NOT NULL REFERENCES goals(id),
                statement TEXT NOT NULL
             ) STRICT;
             CREATE INDEX idx_assumptions_goal ON assumptions(goal_id);
             CREATE TABLE reflections (
                id      TEXT PRIMARY KEY,
                goal_id TEXT NOT NULL REFERENCES goals(id),
                summary TEXT NOT NULL
             ) STRICT;
             CREATE INDEX idx_reflections_goal ON reflections(goal_id);
             INSERT INTO directions VALUES ('1', 'Get back into running');
             INSERT INTO goals VALUES ('2', '1', 'Run a 5k without stopping');
             INSERT INTO assumptions VALUES ('3', '2', 'Mornings are more sustainable');",
        )
        .unwrap();

        // Opening the store migrates the schema and preserves the data under the
        // new names.
        let store = SqliteStore::from_connection(conn).unwrap();
        let targets: &dyn TargetRepository = &store;
        let loaded = targets.get(TargetId::new(2)).unwrap().unwrap();
        assert_eq!(loaded.statement(), "Run a 5k without stopping");
        assert_eq!(loaded.direction(), DirectionId::new(1));

        // The assumption still links to its target through the renamed column.
        let assumptions: &dyn AssumptionRepository = &store;
        let found = assumptions.list_for_target(TargetId::new(2)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id(), AssumptionId::new(3));
    }

    #[test]
    fn opening_a_pre_lifecycle_database_defaults_status_to_active() {
        use endora_application::TargetRepository;
        use endora_domain::{DirectionId, LifecycleStatus, TargetId};
        use rusqlite::Connection;

        // A database created before the lifecycle `status` column existed.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE directions (id TEXT PRIMARY KEY, title TEXT NOT NULL) STRICT;
             CREATE TABLE targets (
                id           TEXT PRIMARY KEY,
                direction_id TEXT NOT NULL REFERENCES directions(id),
                statement    TEXT NOT NULL
             ) STRICT;
             INSERT INTO directions VALUES ('1', 'Be healthier');
             INSERT INTO targets VALUES ('2', '1', 'Run a 5k');",
        )
        .unwrap();

        // Opening adds the column and reads existing rows back as active.
        let store = SqliteStore::from_connection(conn).unwrap();
        let directions: &dyn DirectionRepository = &store;
        assert_eq!(
            directions
                .get(DirectionId::new(1))
                .unwrap()
                .unwrap()
                .status(),
            LifecycleStatus::Active
        );
        let targets: &dyn TargetRepository = &store;
        assert_eq!(
            targets.get(TargetId::new(2)).unwrap().unwrap().status(),
            LifecycleStatus::Active
        );
    }

    #[test]
    fn observations_round_trip_with_their_timestamp() {
        use endora_application::{
            AssumptionRepository, ExperimentRepository, ObservationRepository,
        };
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, Observation, ObservationId,
            TargetId, Timestamp,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let targets: &dyn TargetRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;
        let experiments: &dyn ExperimentRepository = &store;
        let observations: &dyn ObservationRepository = &store;

        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let target = TargetId::new(2);
        targets
            .save(&Target::new(target, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        assumptions
            .save(&Assumption::new(assumption, target, "Mornings are freest").unwrap())
            .unwrap();
        let experiment = ExperimentId::new(4);
        experiments
            .save(&Experiment::propose(experiment, assumption, "Try mornings").unwrap())
            .unwrap();

        let at = Timestamp::from_unix_millis(1_700_000_000_123);
        let o = Observation::record(ObservationId::new(5), experiment, "felt good", at).unwrap();
        observations.save(&o).unwrap();

        assert_eq!(
            observations.list_for_experiment(experiment).unwrap(),
            vec![o]
        );
    }

    #[test]
    fn reflection_round_trips_with_ordered_evidence() {
        use endora_application::{
            AssumptionRepository, ExperimentRepository, ObservationRepository, ReflectionRepository,
        };
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, Observation, ObservationId,
            Reflection, ReflectionId, TargetId, Timestamp,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let targets: &dyn TargetRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;
        let experiments: &dyn ExperimentRepository = &store;
        let observations: &dyn ObservationRepository = &store;
        let reflections: &dyn ReflectionRepository = &store;

        // Build the chain the evidence FKs require.
        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let target = TargetId::new(2);
        targets
            .save(&Target::new(target, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        assumptions
            .save(&Assumption::new(assumption, target, "Mornings").unwrap())
            .unwrap();
        let experiment = ExperimentId::new(4);
        experiments
            .save(&Experiment::propose(experiment, assumption, "Try mornings").unwrap())
            .unwrap();
        let at = Timestamp::from_unix_millis(1);
        for oid in [10, 11] {
            observations
                .save(
                    &Observation::record(ObservationId::new(oid), experiment, "note", at).unwrap(),
                )
                .unwrap();
        }

        // Evidence order (11 before 10) must survive the round trip.
        let r = Reflection::new(
            ReflectionId::new(5),
            target,
            "mornings worked",
            vec![ObservationId::new(11), ObservationId::new(10)],
        )
        .unwrap();
        reflections.save(&r).unwrap();

        assert_eq!(
            reflections.get(ReflectionId::new(5)).unwrap(),
            Some(r.clone())
        );
        assert_eq!(reflections.list_for_target(target).unwrap(), vec![r]);
    }

    #[test]
    fn process_change_approval_survives_a_reload() {
        use endora_application::{ProcessChangeRepository, ReflectionRepository};
        use endora_domain::{
            ObservationId, ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId,
            TargetId,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let targets: &dyn TargetRepository = &store;
        let reflections: &dyn ReflectionRepository = &store;
        let changes: &dyn ProcessChangeRepository = &store;

        // Reflection with no evidence FKs (evidence table is empty) still needs a
        // real observation? No — Reflection::new requires >=1 evidence id, but the
        // reflection_evidence FK requires the observation to exist. Build a full
        // chain so the evidence id is valid.
        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let target = TargetId::new(2);
        targets
            .save(&Target::new(target, direction, "Run a 5k").unwrap())
            .unwrap();

        use endora_application::{
            AssumptionRepository, ExperimentRepository, ObservationRepository,
        };
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, Observation, Timestamp,
        };
        let assumption = AssumptionId::new(3);
        (&store as &dyn AssumptionRepository)
            .save(&Assumption::new(assumption, target, "Mornings").unwrap())
            .unwrap();
        let experiment = ExperimentId::new(4);
        (&store as &dyn ExperimentRepository)
            .save(&Experiment::propose(experiment, assumption, "Try").unwrap())
            .unwrap();
        let obs = ObservationId::new(5);
        (&store as &dyn ObservationRepository)
            .save(
                &Observation::record(obs, experiment, "n", Timestamp::from_unix_millis(1)).unwrap(),
            )
            .unwrap();

        let reflection = ReflectionId::new(6);
        reflections
            .save(&Reflection::new(reflection, target, "worked", vec![obs]).unwrap())
            .unwrap();

        // Save an approved change; propose() alone could not rebuild this state.
        let mut change =
            ProposedProcessChange::propose(ProcessChangeId::new(7), reflection, "Do mornings")
                .unwrap();
        change.approve().unwrap();
        changes.save(&change).unwrap();

        let loaded = changes.get(ProcessChangeId::new(7)).unwrap().unwrap();
        assert!(loaded.is_approved());
        assert_eq!(
            changes.list_for_reflection(reflection).unwrap(),
            vec![loaded]
        );
    }

    #[test]
    fn export_captures_everything_and_purge_clears_it() {
        use endora_application::{
            AssumptionRepository, AuditLog, ExperimentRepository, MemoryStore,
            ObservationRepository, ProcessChangeRepository, ReflectionRepository,
        };
        use endora_domain::{
            Assumption, AssumptionId, AuditId, AuditRecord, Experiment, ExperimentId, Observation,
            ObservationId, ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId,
            TargetId, Timestamp,
        };

        let store = store();
        // Seed one of every entity.
        let direction = DirectionId::new(1);
        (&store as &dyn DirectionRepository)
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let target = TargetId::new(2);
        (&store as &dyn TargetRepository)
            .save(&Target::new(target, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        (&store as &dyn AssumptionRepository)
            .save(&Assumption::new(assumption, target, "Mornings").unwrap())
            .unwrap();
        let experiment = ExperimentId::new(4);
        (&store as &dyn ExperimentRepository)
            .save(&Experiment::propose(experiment, assumption, "Try").unwrap())
            .unwrap();
        let obs = ObservationId::new(5);
        (&store as &dyn ObservationRepository)
            .save(
                &Observation::record(obs, experiment, "n", Timestamp::from_unix_millis(1)).unwrap(),
            )
            .unwrap();
        let reflection = ReflectionId::new(6);
        (&store as &dyn ReflectionRepository)
            .save(&Reflection::new(reflection, target, "worked", vec![obs]).unwrap())
            .unwrap();
        (&store as &dyn ProcessChangeRepository)
            .save(
                &ProposedProcessChange::propose(ProcessChangeId::new(7), reflection, "Do it")
                    .unwrap(),
            )
            .unwrap();
        (&store as &dyn AuditLog)
            .append(
                &AuditRecord::new(AuditId::new(8), Timestamp::from_unix_millis(9), "noted")
                    .unwrap(),
            )
            .unwrap();

        let snapshot = (&store as &dyn MemoryStore).export().unwrap();
        assert_eq!(snapshot.directions.len(), 1);
        assert_eq!(snapshot.targets.len(), 1);
        assert_eq!(snapshot.assumptions.len(), 1);
        assert_eq!(snapshot.experiments.len(), 1);
        assert_eq!(snapshot.observations.len(), 1);
        assert_eq!(snapshot.reflections.len(), 1);
        assert_eq!(snapshot.reflections[0].evidence(), &[obs]);
        assert_eq!(snapshot.process_changes.len(), 1);
        assert_eq!(snapshot.audit.len(), 1);

        (&store as &dyn MemoryStore).purge().unwrap();
        let empty = (&store as &dyn MemoryStore).export().unwrap();
        assert_eq!(empty, endora_application::MemorySnapshot::default());
    }

    #[test]
    fn audit_records_append_and_read_newest_first() {
        use endora_application::AuditLog;
        use endora_domain::{AuditId, AuditRecord, Timestamp};

        let store = store();
        let log: &dyn AuditLog = &store;
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
    fn events_append_and_read_newest_first() {
        use endora_application::EventLog;
        use endora_domain::Timestamp;
        let store = store();
        let log: &dyn EventLog = &store;
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
    fn autonomy_envelope_defaults_then_round_trips() {
        use endora_application::{AutonomyEnvelope, AutonomyEnvelopeRepository};
        let store = store();
        let repo: &dyn AutonomyEnvelopeRepository = &store;

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
        let repo: &dyn CapabilityConfigRepository = &store;

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
        let store = store();
        let repo: &dyn DirectionRepository = &store;
        let big = u128::MAX;
        repo.save(&Direction::new(DirectionId::new(big), "Edge").unwrap())
            .unwrap();
        assert_eq!(
            repo.get(DirectionId::new(big)).unwrap().unwrap().id(),
            DirectionId::new(big)
        );
    }
}
