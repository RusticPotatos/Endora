//! Direction infrastructure — SQLite adapters for the aims + learning loop.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{
    AssumptionId, DirectionId, ExperimentId, ObservationId, ProcessChangeId, ReflectionId,
    TargetId, Timestamp, ValueId,
};
use endora_persistence::{Db, backend, corrupt, id_text, parse_id};
use rusqlite::{Connection, OptionalExtension, params};

use crate::application::{
    AssumptionRepository, DirectionRepository, ExperimentRepository, ObservationRepository,
    ProcessChangeRepository, ReflectionRepository, TargetRepository, ValueRepository,
};
use crate::domain::{
    ApprovalState, Assumption, Direction, Experiment, ExperimentStatus, LifecycleStatus,
    Observation, ProposedProcessChange, Reflection, Target, Value,
};

/// Creates the direction tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS \"values\" (
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
            CREATE INDEX IF NOT EXISTS idx_experiments_review ON experiments(review_by_ms);
            CREATE TABLE IF NOT EXISTS observations (
                id            TEXT PRIMARY KEY,
                experiment_id TEXT NOT NULL REFERENCES experiments(id),
                note          TEXT NOT NULL,
                at_ms         INTEGER NOT NULL
            ) STRICT;
            CREATE INDEX IF NOT EXISTS idx_observations_experiment ON observations(experiment_id);
            CREATE TABLE IF NOT EXISTS reflections (
                id        TEXT PRIMARY KEY,
                target_id TEXT NOT NULL REFERENCES targets(id),
                summary   TEXT NOT NULL
            ) STRICT;
            CREATE INDEX IF NOT EXISTS idx_reflections_target ON reflections(target_id);
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
                ON process_changes(reflection_id);",
        )
        .map_err(backend)?;
    Ok(())
}

/// SQLite-backed store for the eight direction repositories over the shared
/// connection handle.
pub struct DirectionStore {
    db: Db,
}

impl DirectionStore {
    /// Builds the store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl DirectionRepository for DirectionStore {
    fn save(&self, direction: &Direction) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
        all_directions(&conn)
    }

    fn delete(&self, id: DirectionId) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "DELETE FROM directions WHERE id = ?1",
            params![id_text(id.value())],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl ValueRepository for DirectionStore {
    fn save(&self, value: &Value) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO \"values\" (id, name) VALUES (?1, ?2)",
            params![id_text(value.id().value()), value.name()],
        )
        .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: ValueId) -> Result<Option<Value>, RepositoryError> {
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
        all_values(&conn)
    }

    fn delete(&self, id: ValueId) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "DELETE FROM \"values\" WHERE id = ?1",
            params![id_text(id.value())],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl TargetRepository for DirectionStore {
    fn save(&self, target: &Target) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
        conn.execute(
            "DELETE FROM targets WHERE id = ?1",
            params![id_text(id.value())],
        )
        .map_err(backend)?;
        Ok(())
    }
}

impl AssumptionRepository for DirectionStore {
    fn save(&self, assumption: &Assumption) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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

impl ExperimentRepository for DirectionStore {
    fn save(&self, experiment: &Experiment) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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

impl ObservationRepository for DirectionStore {
    fn save(&self, observation: &Observation) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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

impl ReflectionRepository for DirectionStore {
    fn save(&self, reflection: &Reflection) -> Result<(), RepositoryError> {
        let mut conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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

impl ProcessChangeRepository for DirectionStore {
    fn save(&self, change: &ProposedProcessChange) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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
        let conn = self.db.lock()?;
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

fn parse_lifecycle(text: &str) -> Result<LifecycleStatus, RepositoryError> {
    LifecycleStatus::from_name(text)
        .ok_or_else(|| RepositoryError::Corrupt(format!("unknown lifecycle status {text:?}")))
}

fn parse_status(text: &str) -> Result<ExperimentStatus, RepositoryError> {
    ExperimentStatus::from_name(text)
        .ok_or_else(|| RepositoryError::Corrupt(format!("unknown experiment status {text:?}")))
}

fn parse_approval(text: &str) -> Result<ApprovalState, RepositoryError> {
    ApprovalState::from_name(text)
        .ok_or_else(|| RepositoryError::Corrupt(format!("unknown approval state {text:?}")))
}

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
