//! SQLite-backed implementations of the application repository ports.
//!
//! A single [`SqliteStore`] owns one connection behind a mutex and implements
//! the repository traits. Identifiers are `u128`, which SQLite integers cannot
//! hold, so they are stored as their decimal `TEXT` form and parsed back on
//! read. The store is synchronous; when the async node uses it, calls run on a
//! blocking thread (see `docs/adr/0007-async-web-stack.md`).

use std::sync::Mutex;

use endora_application::{
    AssumptionRepository, AuditLog, DirectionRepository, ExperimentRepository, GoalRepository,
    MemorySnapshot, MemoryStore, ObservationRepository, ProcessChangeRepository,
    ReflectionRepository, RepositoryError,
};
use endora_domain::{
    ApprovalState, Assumption, AssumptionId, AuditId, AuditRecord, Direction, DirectionId,
    Experiment, ExperimentId, ExperimentStatus, Goal, GoalId, Observation, ObservationId,
    ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId, Timestamp,
};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS directions (
    id    TEXT PRIMARY KEY,
    title TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS goals (
    id           TEXT PRIMARY KEY,
    direction_id TEXT NOT NULL REFERENCES directions(id),
    statement    TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_goals_direction ON goals(direction_id);

CREATE TABLE IF NOT EXISTS assumptions (
    id        TEXT PRIMARY KEY,
    goal_id   TEXT NOT NULL REFERENCES goals(id),
    statement TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_assumptions_goal ON assumptions(goal_id);

CREATE TABLE IF NOT EXISTS experiments (
    id            TEXT PRIMARY KEY,
    assumption_id TEXT NOT NULL REFERENCES assumptions(id),
    hypothesis    TEXT NOT NULL,
    status        TEXT NOT NULL
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
    goal_id TEXT NOT NULL REFERENCES goals(id),
    summary TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_reflections_goal ON reflections(goal_id);

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
        conn.execute_batch(SCHEMA).map_err(backend)?;
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
            "INSERT OR REPLACE INTO directions (id, title) VALUES (?1, ?2)",
            params![id_text(direction.id().value()), direction.title()],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: DirectionId) -> Result<Option<Direction>, RepositoryError> {
        let conn = self.lock()?;
        let title: Option<String> = conn
            .query_row(
                "SELECT title FROM directions WHERE id = ?1",
                params![id_text(id.value())],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(title) = title else {
            return Ok(None);
        };
        let direction = Direction::new(id, &title).map_err(corrupt)?;
        Ok(Some(direction))
    }

    fn list_all(&self) -> Result<Vec<Direction>, RepositoryError> {
        let conn = self.lock()?;
        all_directions(&conn)
    }
}

impl GoalRepository for SqliteStore {
    fn save(&self, goal: &Goal) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO goals (id, direction_id, statement) VALUES (?1, ?2, ?3)",
            params![
                id_text(goal.id().value()),
                id_text(goal.direction().value()),
                goal.statement()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: GoalId) -> Result<Option<Goal>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT direction_id, statement FROM goals WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((direction_id, statement)) = row else {
            return Ok(None);
        };
        let direction = DirectionId::new(parse_id(&direction_id)?);
        let goal = Goal::new(id, direction, &statement).map_err(corrupt)?;
        Ok(Some(goal))
    }

    fn list_for_direction(&self, direction: DirectionId) -> Result<Vec<Goal>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, statement FROM goals WHERE direction_id = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(direction.value())], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?;

        let mut goals = Vec::new();
        for row in rows {
            let (id_text, statement) = row.map_err(backend)?;
            let goal = Goal::new(GoalId::new(parse_id(&id_text)?), direction, &statement)
                .map_err(corrupt)?;
            goals.push(goal);
        }
        Ok(goals)
    }
}

impl AssumptionRepository for SqliteStore {
    fn save(&self, assumption: &Assumption) -> Result<(), RepositoryError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO assumptions (id, goal_id, statement) VALUES (?1, ?2, ?3)",
            params![
                id_text(assumption.id().value()),
                id_text(assumption.goal().value()),
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
                "SELECT goal_id, statement FROM assumptions WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((goal_id, statement)) = row else {
            return Ok(None);
        };
        let goal = GoalId::new(parse_id(&goal_id)?);
        let assumption = Assumption::new(id, goal, &statement).map_err(corrupt)?;
        Ok(Some(assumption))
    }

    fn list_for_goal(&self, goal: GoalId) -> Result<Vec<Assumption>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, statement FROM assumptions WHERE goal_id = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(goal.value())], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(backend)?;

        let mut assumptions = Vec::new();
        for row in rows {
            let (id, statement) = row.map_err(backend)?;
            let assumption = Assumption::new(AssumptionId::new(parse_id(&id)?), goal, &statement)
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
            "INSERT OR REPLACE INTO experiments (id, assumption_id, hypothesis, status) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id_text(experiment.id().value()),
                id_text(experiment.assumption().value()),
                experiment.hypothesis(),
                experiment.status().name()
            ],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: ExperimentId) -> Result<Option<Experiment>, RepositoryError> {
        let conn = self.lock()?;
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT assumption_id, hypothesis, status FROM experiments WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((assumption_id, hypothesis, status)) = row else {
            return Ok(None);
        };
        let assumption = AssumptionId::new(parse_id(&assumption_id)?);
        let experiment =
            Experiment::from_parts(id, assumption, &hypothesis, parse_status(&status)?)
                .map_err(corrupt)?;
        Ok(Some(experiment))
    }

    fn list_for_assumption(
        &self,
        assumption: AssumptionId,
    ) -> Result<Vec<Experiment>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, hypothesis, status FROM experiments \
                 WHERE assumption_id = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![id_text(assumption.value())], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;

        let mut experiments = Vec::new();
        for row in rows {
            let (id, hypothesis, status) = row.map_err(backend)?;
            let experiment = Experiment::from_parts(
                ExperimentId::new(parse_id(&id)?),
                assumption,
                &hypothesis,
                parse_status(&status)?,
            )
            .map_err(corrupt)?;
            experiments.push(experiment);
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
}

impl ReflectionRepository for SqliteStore {
    fn save(&self, reflection: &Reflection) -> Result<(), RepositoryError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(backend)?;
        let rid = id_text(reflection.id().value());
        tx.execute(
            "INSERT OR REPLACE INTO reflections (id, goal_id, summary) VALUES (?1, ?2, ?3)",
            params![
                rid,
                id_text(reflection.goal().value()),
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
                "SELECT goal_id, summary FROM reflections WHERE id = ?1",
                params![id_text(id.value())],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((goal_id, summary)) = row else {
            return Ok(None);
        };
        let evidence = evidence_for(&conn, id)?;
        let goal = GoalId::new(parse_id(&goal_id)?);
        let reflection = Reflection::new(id, goal, &summary, evidence).map_err(corrupt)?;
        Ok(Some(reflection))
    }

    fn list_for_goal(&self, goal: GoalId) -> Result<Vec<Reflection>, RepositoryError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, summary FROM reflections WHERE goal_id = ?1 ORDER BY id")
            .map_err(backend)?;
        let rows: Vec<(String, String)> = stmt
            .query_map(params![id_text(goal.value())], |row| {
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
                Reflection::new(reflection_id, goal, &summary, evidence).map_err(corrupt)?;
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
            directions: all_directions(&conn)?,
            goals: all_goals(&conn)?,
            assumptions: all_assumptions(&conn)?,
            experiments: all_experiments(&conn)?,
            observations: all_observations(&conn)?,
            reflections: all_reflections(&conn)?,
            process_changes: all_process_changes(&conn)?,
            audit: all_audit(&conn)?,
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
            "goals",
            "directions",
            "audit_log",
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
        .prepare("SELECT id, title FROM directions ORDER BY id")
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title) = row.map_err(backend)?;
        out.push(Direction::new(DirectionId::new(parse_id(&id)?), &title).map_err(corrupt)?);
    }
    Ok(out)
}

fn all_goals(conn: &Connection) -> Result<Vec<Goal>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, direction_id, statement FROM goals ORDER BY id")
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
        let (id, direction, statement) = row.map_err(backend)?;
        let goal = Goal::new(
            GoalId::new(parse_id(&id)?),
            DirectionId::new(parse_id(&direction)?),
            &statement,
        )
        .map_err(corrupt)?;
        out.push(goal);
    }
    Ok(out)
}

fn all_assumptions(conn: &Connection) -> Result<Vec<Assumption>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, goal_id, statement FROM assumptions ORDER BY id")
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
        let (id, goal, statement) = row.map_err(backend)?;
        out.push(
            Assumption::new(
                AssumptionId::new(parse_id(&id)?),
                GoalId::new(parse_id(&goal)?),
                &statement,
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
}

fn all_experiments(conn: &Connection) -> Result<Vec<Experiment>, RepositoryError> {
    let mut stmt = conn
        .prepare("SELECT id, assumption_id, hypothesis, status FROM experiments ORDER BY id")
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
        let (id, assumption, hypothesis, status) = row.map_err(backend)?;
        out.push(
            Experiment::from_parts(
                ExperimentId::new(parse_id(&id)?),
                AssumptionId::new(parse_id(&assumption)?),
                &hypothesis,
                parse_status(&status)?,
            )
            .map_err(corrupt)?,
        );
    }
    Ok(out)
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
            .prepare("SELECT id, goal_id, summary FROM reflections ORDER BY id")
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
    for (id, goal, summary) in rows {
        let reflection_id = ReflectionId::new(parse_id(&id)?);
        let evidence = evidence_for(conn, reflection_id)?;
        out.push(
            Reflection::new(
                reflection_id,
                GoalId::new(parse_id(&goal)?),
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

/// Parses a stored approval state, or reports corruption.
fn parse_approval(text: &str) -> Result<ApprovalState, RepositoryError> {
    ApprovalState::from_name(text)
        .ok_or_else(|| RepositoryError::Corrupt(format!("unknown approval state {text:?}")))
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
    use endora_application::{DirectionRepository, GoalRepository};
    use endora_domain::{Direction, DirectionId, Goal, GoalId};

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
    fn goal_round_trips_and_lists_by_direction() {
        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let goals: &dyn GoalRepository = &store;
        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();

        let g1 = Goal::new(GoalId::new(10), direction, "Run a 5k").unwrap();
        let g2 = Goal::new(GoalId::new(11), direction, "Sleep 8h").unwrap();
        goals.save(&g1).unwrap();
        goals.save(&g2).unwrap();

        assert_eq!(goals.get(GoalId::new(10)).unwrap(), Some(g1.clone()));
        assert_eq!(goals.list_for_direction(direction).unwrap(), vec![g1, g2]);
    }

    #[test]
    fn assumptions_round_trip_and_list_by_goal() {
        use endora_application::AssumptionRepository;
        use endora_domain::{Assumption, AssumptionId, GoalId};

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let goals: &dyn GoalRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;

        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let goal = GoalId::new(2);
        goals
            .save(&Goal::new(goal, direction, "Run a 5k").unwrap())
            .unwrap();

        let a = Assumption::new(AssumptionId::new(3), goal, "Mornings are freest").unwrap();
        assumptions.save(&a).unwrap();
        assert_eq!(assumptions.list_for_goal(goal).unwrap(), vec![a]);
        assert_eq!(assumptions.list_for_goal(GoalId::new(999)).unwrap(), vec![]);
    }

    #[test]
    fn experiment_status_survives_a_reload() {
        use endora_application::{AssumptionRepository, ExperimentRepository};
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, ExperimentStatus, GoalId,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let goals: &dyn GoalRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;
        let experiments: &dyn ExperimentRepository = &store;

        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let goal = GoalId::new(2);
        goals
            .save(&Goal::new(goal, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        assumptions
            .save(&Assumption::new(assumption, goal, "Mornings are freest").unwrap())
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
    fn observations_round_trip_with_their_timestamp() {
        use endora_application::{
            AssumptionRepository, ExperimentRepository, ObservationRepository,
        };
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, GoalId, Observation, ObservationId,
            Timestamp,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let goals: &dyn GoalRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;
        let experiments: &dyn ExperimentRepository = &store;
        let observations: &dyn ObservationRepository = &store;

        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let goal = GoalId::new(2);
        goals
            .save(&Goal::new(goal, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        assumptions
            .save(&Assumption::new(assumption, goal, "Mornings are freest").unwrap())
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
            Assumption, AssumptionId, Experiment, ExperimentId, GoalId, Observation, ObservationId,
            Reflection, ReflectionId, Timestamp,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let goals: &dyn GoalRepository = &store;
        let assumptions: &dyn AssumptionRepository = &store;
        let experiments: &dyn ExperimentRepository = &store;
        let observations: &dyn ObservationRepository = &store;
        let reflections: &dyn ReflectionRepository = &store;

        // Build the chain the evidence FKs require.
        let direction = DirectionId::new(1);
        directions
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let goal = GoalId::new(2);
        goals
            .save(&Goal::new(goal, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        assumptions
            .save(&Assumption::new(assumption, goal, "Mornings").unwrap())
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
            goal,
            "mornings worked",
            vec![ObservationId::new(11), ObservationId::new(10)],
        )
        .unwrap();
        reflections.save(&r).unwrap();

        assert_eq!(
            reflections.get(ReflectionId::new(5)).unwrap(),
            Some(r.clone())
        );
        assert_eq!(reflections.list_for_goal(goal).unwrap(), vec![r]);
    }

    #[test]
    fn process_change_approval_survives_a_reload() {
        use endora_application::{ProcessChangeRepository, ReflectionRepository};
        use endora_domain::{
            GoalId, ObservationId, ProcessChangeId, ProposedProcessChange, Reflection, ReflectionId,
        };

        let store = store();
        let directions: &dyn DirectionRepository = &store;
        let goals: &dyn GoalRepository = &store;
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
        let goal = GoalId::new(2);
        goals
            .save(&Goal::new(goal, direction, "Run a 5k").unwrap())
            .unwrap();

        use endora_application::{
            AssumptionRepository, ExperimentRepository, ObservationRepository,
        };
        use endora_domain::{
            Assumption, AssumptionId, Experiment, ExperimentId, Observation, Timestamp,
        };
        let assumption = AssumptionId::new(3);
        (&store as &dyn AssumptionRepository)
            .save(&Assumption::new(assumption, goal, "Mornings").unwrap())
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
            .save(&Reflection::new(reflection, goal, "worked", vec![obs]).unwrap())
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
            Assumption, AssumptionId, AuditId, AuditRecord, Experiment, ExperimentId, GoalId,
            Observation, ObservationId, ProcessChangeId, ProposedProcessChange, Reflection,
            ReflectionId, Timestamp,
        };

        let store = store();
        // Seed one of every entity.
        let direction = DirectionId::new(1);
        (&store as &dyn DirectionRepository)
            .save(&Direction::new(direction, "Be healthier").unwrap())
            .unwrap();
        let goal = GoalId::new(2);
        (&store as &dyn GoalRepository)
            .save(&Goal::new(goal, direction, "Run a 5k").unwrap())
            .unwrap();
        let assumption = AssumptionId::new(3);
        (&store as &dyn AssumptionRepository)
            .save(&Assumption::new(assumption, goal, "Mornings").unwrap())
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
            .save(&Reflection::new(reflection, goal, "worked", vec![obs]).unwrap())
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
        assert_eq!(snapshot.goals.len(), 1);
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
