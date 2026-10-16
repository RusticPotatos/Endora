//! Understanding infrastructure — SQLite-backed belief, preference, outcome and
//! intention repositories.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{BeliefId, IntentionId, OutcomeId, PreferenceId, Timestamp};
use endora_persistence::{Db, backend, corrupt, id_text, parse_id};
use rusqlite::{Connection, params};

mod brain_folder;
pub use brain_folder::BrainFolder;

use crate::application::{
    IntentionRepository, OutcomeRepository, PreferenceRepository, ResearchRepository,
};
use crate::domain::{
    Intention, IntentionOrigin, IntentionState, Outcome, Preference, PreferenceKind, Reaction,
    ResearchAssignment, ResearchReport, ResearchStatus,
};

/// Creates the understanding tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS intentions (
                id                 TEXT PRIMARY KEY,
                statement          TEXT NOT NULL,
                motivating_belief  TEXT,
                origin             TEXT NOT NULL DEFAULT 'belief',
                request_message_id TEXT,
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
            ) STRICT;",
        )
        .map_err(backend)?;
    migrate_intentions_0077(db)?;
    db.lock()?
        .execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_intention_origin
            ON intentions(origin) WHERE state = 'active';
         CREATE TABLE IF NOT EXISTS intention_research (
            intention_id TEXT PRIMARY KEY, assignment TEXT NOT NULL,
            request_key TEXT NOT NULL UNIQUE, source_urls_json TEXT NOT NULL,
            search_capability_id TEXT, status TEXT NOT NULL, phase TEXT NOT NULL,
            reason_code TEXT NOT NULL, reason TEXT NOT NULL, checkpoint TEXT NOT NULL,
            report_markdown TEXT, report_partial INTEGER NOT NULL DEFAULT 0,
            report_created_ms INTEGER, model_calls_used INTEGER NOT NULL,
            read_attempts_used INTEGER NOT NULL, created_ms INTEGER NOT NULL,
            updated_ms INTEGER NOT NULL, expires_ms INTEGER NOT NULL,
            FOREIGN KEY (intention_id) REFERENCES intentions(id) ON DELETE CASCADE
         ) STRICT;
         CREATE INDEX IF NOT EXISTS idx_intention_research_status
            ON intention_research(status);",
        )
        .map_err(backend)?;
    Ok(())
}

fn migrate_intentions_0077(db: &Db) -> Result<(), RepositoryError> {
    let mut conn = db.lock()?;
    let has_origin = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(intentions)")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(backend)?
            .iter()
            .any(|name| name == "origin")
    };
    if has_origin {
        return Ok(());
    }
    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM intentions WHERE state = 'active'",
            [],
            |r| r.get(0),
        )
        .map_err(backend)?;
    if active > 1 {
        return Err(RepositoryError::Corrupt(
            "more than one active legacy intention; resolve it before upgrading".to_owned(),
        ));
    }
    let tx = conn.transaction().map_err(backend)?;
    tx.execute_batch(
        "ALTER TABLE intentions RENAME TO intentions_before_0077;
         CREATE TABLE intentions (
            id TEXT PRIMARY KEY, statement TEXT NOT NULL, motivating_belief TEXT,
            origin TEXT NOT NULL, request_message_id TEXT, note TEXT NOT NULL,
            state TEXT NOT NULL, created_ms INTEGER NOT NULL,
            last_progressed_ms INTEGER NOT NULL, steps_taken INTEGER NOT NULL,
            CHECK ((origin = 'belief' AND motivating_belief IS NOT NULL AND request_message_id IS NULL)
                OR (origin = 'user_request' AND motivating_belief IS NULL AND request_message_id IS NOT NULL))
         ) STRICT;
         INSERT INTO intentions
            (id, statement, motivating_belief, origin, request_message_id, note, state,
             created_ms, last_progressed_ms, steps_taken)
         SELECT id, statement, motivating_belief, 'belief', NULL, note, state,
                created_ms, last_progressed_ms, steps_taken
         FROM intentions_before_0077;
         DROP TABLE intentions_before_0077;
         CREATE UNIQUE INDEX idx_one_active_intention_origin
            ON intentions(origin) WHERE state = 'active';",
    ).map_err(backend)?;
    tx.commit().map_err(backend)
}

/// SQLite-backed store for beliefs, preferences and outcomes over the shared connection.
pub struct UnderstandingStore {
    db: Db,
}

impl UnderstandingStore {
    /// Builds the store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl IntentionRepository for UnderstandingStore {
    fn save(&self, intention: &Intention) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT INTO intentions \
                 (id, statement, motivating_belief, origin, request_message_id, note, state,
                  created_ms, last_progressed_ms, steps_taken)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    note=excluded.note, state=excluded.state,
                    last_progressed_ms=excluded.last_progressed_ms,
                    steps_taken=excluded.steps_taken",
                params![
                    id_text(intention.id().value()),
                    intention.statement(),
                    intention.motivating_belief().map(|id| id_text(id.value())),
                    intention.origin().name(),
                    match intention.origin() {
                        IntentionOrigin::UserRequest(id) => Some(id_text(id.value())),
                        _ => None,
                    },
                    intention.note(),
                    intention.state().name(),
                    intention.created_at().unix_millis(),
                    intention.last_progressed_at().unix_millis(),
                    intention.steps_taken(),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: IntentionId) -> Result<Option<Intention>, RepositoryError> {
        let conn = self.db.lock()?;
        Ok(all_intentions(&conn)?.into_iter().find(|i| i.id() == id))
    }

    fn active(&self) -> Result<Option<Intention>, RepositoryError> {
        let conn = self.db.lock()?;
        // At most one is active (ADR 0052); if a bug ever produced two, the most
        // recently moved wins, since `all_intentions` is ordered by that.
        Ok(all_intentions(&conn)?
            .into_iter()
            .find(Intention::is_active))
    }

    fn list(&self) -> Result<Vec<Intention>, RepositoryError> {
        let conn = self.db.lock()?;
        all_intentions(&conn)
    }

    fn forget_its_own(&self) -> Result<usize, RepositoryError> {
        self.db
            .lock()?
            .execute("DELETE FROM intentions WHERE origin = 'belief'", [])
            .map_err(backend)
    }
}

fn all_intentions(conn: &Connection) -> Result<Vec<Intention>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, statement, motivating_belief, origin, request_message_id, note, state, created_ms,
             last_progressed_ms, steps_taken
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
                IntentionOrigin::UserRequest(endora_kernel::ids::MessageId::new(parse_id(&id)?))
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

impl ResearchRepository for UnderstandingStore {
    fn save_research(&self, work: &ResearchAssignment) -> Result<(), RepositoryError> {
        if work.status() == ResearchStatus::Delivered {
            return Err(RepositoryError::Corrupt(
                "delivered research must use the atomic publisher".to_owned(),
            ));
        }
        let report = work.report();
        self.db.lock()?.execute(
            "UPDATE intention_research SET status=?2, phase=?3, reason_code=?4, reason=?5,
             checkpoint=?6, report_markdown=?7, report_partial=?8, report_created_ms=?9,
             model_calls_used=?10, read_attempts_used=?11, updated_ms=?12
             WHERE intention_id=?1 AND status NOT IN ('delivered','blocked','budget_exhausted','cancelled','failed')",
            params![id_text(work.intention_id().value()), work.status().name(), work.phase(),
                work.reason_code(), work.reason(), work.checkpoint(), report.map(ResearchReport::markdown),
                report.is_some_and(ResearchReport::partial), report.map(|r| r.created_at().unix_millis()),
                work.model_calls_used(), work.read_attempts_used(), work.updated_at().unix_millis()],
        ).map_err(backend)?;
        Ok(())
    }

    fn research(&self, id: IntentionId) -> Result<Option<ResearchAssignment>, RepositoryError> {
        let conn = self.db.lock()?;
        Ok(
            research_where(&conn, "WHERE intention_id=?1", &[&id_text(id.value())])?
                .into_iter()
                .next(),
        )
    }

    fn active_research(&self) -> Result<Option<ResearchAssignment>, RepositoryError> {
        let conn = self.db.lock()?;
        Ok(research_where(
            &conn,
            "WHERE status NOT IN ('delivered','blocked','budget_exhausted','cancelled','failed')",
            &[],
        )?
        .into_iter()
        .next())
    }

    fn list_research(&self) -> Result<Vec<ResearchAssignment>, RepositoryError> {
        let conn = self.db.lock()?;
        research_where(&conn, "", &[])
    }
}

/// Rows are filtered in SQL: the worker asks for the active assignment every few
/// seconds, and each row carries a full report body.
fn research_where(
    conn: &Connection,
    filter: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<ResearchAssignment>, RepositoryError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT intention_id, assignment, request_key, source_urls_json, search_capability_id,
         status, phase, reason_code, reason, checkpoint, report_markdown, report_partial,
         report_created_ms, model_calls_used, read_attempts_used, created_ms, updated_ms, expires_ms
         FROM intention_research {filter} ORDER BY updated_ms DESC, rowid DESC"
        ))
        .map_err(backend)?;
    let rows = stmt
        .query_map(args, |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, Option<String>>(10)?,
                r.get::<_, bool>(11)?,
                r.get::<_, Option<i64>>(12)?,
                r.get::<_, u32>(13)?,
                r.get::<_, u32>(14)?,
                r.get::<_, i64>(15)?,
                r.get::<_, i64>(16)?,
                r.get::<_, i64>(17)?,
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (
            id,
            assignment,
            key,
            urls,
            search,
            status,
            phase,
            code,
            reason,
            checkpoint,
            markdown,
            partial,
            report_ms,
            model_calls,
            reads,
            created,
            updated,
            expires,
        ) = row.map_err(backend)?;
        let iid = IntentionId::new(parse_id(&id)?);
        let report = match (markdown, report_ms) {
            (Some(body), Some(at)) => Some(
                ResearchReport::new(iid, &body, partial, Timestamp::from_unix_millis(at))
                    .map_err(corrupt)?,
            ),
            (None, None) => None,
            _ => {
                return Err(RepositoryError::Corrupt(
                    "incomplete research report".to_owned(),
                ));
            }
        };
        out.push(ResearchAssignment::from_parts(
            iid,
            assignment,
            key,
            urls,
            search,
            ResearchStatus::from_name(&status),
            phase,
            code,
            reason,
            checkpoint,
            report,
            model_calls,
            reads,
            Timestamp::from_unix_millis(created),
            Timestamp::from_unix_millis(updated),
            Timestamp::from_unix_millis(expires),
        ));
    }
    Ok(out)
}

impl OutcomeRepository for UnderstandingStore {
    fn save(&self, outcome: &Outcome) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO outcomes \
                 (id, capability, input, claim, observation, at_ms, motivating_belief, \
                  reaction, changed) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id_text(outcome.id().value()),
                    outcome.capability(),
                    outcome.input(),
                    outcome.claim(),
                    outcome.observation(),
                    outcome.at().unix_millis(),
                    outcome.motivating_belief().map(|b| id_text(b.value())),
                    outcome.reaction().map(Reaction::name),
                    outcome.changed().map(i64::from),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: OutcomeId) -> Result<Option<Outcome>, RepositoryError> {
        let conn = self.db.lock()?;
        Ok(all_outcomes(&conn)?.into_iter().find(|o| o.id() == id))
    }

    fn list(&self) -> Result<Vec<Outcome>, RepositoryError> {
        let conn = self.db.lock()?;
        all_outcomes(&conn)
    }
}

fn all_outcomes(conn: &Connection) -> Result<Vec<Outcome>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, capability, input, claim, observation, at_ms, motivating_belief, \
             reaction, changed \
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
        // A reaction we cannot parse is corrupt, but an *absent* one is the normal case
        // (ADR 0053 — the person is never asked), so only a present-and-unknown value is
        // an error.
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

/// Moves what the person told Endora from the old `preferences` table into memory's
/// "What you told me", oldest first, then drops the table (ADR 0101): the database holds
/// settings, the brain holds what Endora knows about them. Returns how many moved; a
/// database without the table moves nothing. If the brain cannot take them the table
/// stays, and the next start tries again; a line already there is not added twice.
///
/// # Errors
/// [`RepositoryError`] if the table cannot be read or dropped, or the brain written.
pub fn move_told_into_the_brain(db: &Db, brain: &BrainFolder) -> Result<usize, RepositoryError> {
    let told = {
        let conn = db.lock()?;
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='preferences')",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if !exists {
            return Ok(0);
        }
        all_preferences(&conn)?
    };
    for preference in &told {
        PreferenceRepository::save(brain, preference)?;
    }
    db.lock()?
        .execute("DROP TABLE preferences", [])
        .map_err(backend)?;
    Ok(told.len())
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

#[cfg(test)]
mod tests {
    use super::{BrainFolder, UnderstandingStore, migrate, move_told_into_the_brain};
    use crate::application::{IntentionRepository, OutcomeRepository, PreferenceRepository};
    use crate::domain::{Intention, Outcome, Reaction};
    use endora_kernel::ids::{BeliefId, IntentionId, OutcomeId, Timestamp};
    use endora_persistence::Db;

    fn legacy_intentions(db: &Db) {
        db.lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE intentions (
                id TEXT PRIMARY KEY, statement TEXT NOT NULL, motivating_belief TEXT NOT NULL,
                note TEXT NOT NULL, state TEXT NOT NULL, created_ms INTEGER NOT NULL,
                last_progressed_ms INTEGER NOT NULL, steps_taken INTEGER NOT NULL
             ) STRICT;",
            )
            .unwrap();
    }

    #[test]
    fn legacy_intentions_gain_explicit_belief_provenance_without_changing_ids() {
        let db = Db::open_in_memory().unwrap();
        legacy_intentions(&db);
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO intentions VALUES ('9','learn the routine','7','','active',10,10,0)",
                [],
            )
            .unwrap();
        migrate(&db).unwrap();
        let stored = IntentionRepository::get(&UnderstandingStore::new(db), IntentionId::new(9))
            .unwrap()
            .unwrap();
        assert_eq!(stored.motivating_belief(), Some(BeliefId::new(7)));
    }

    #[test]
    fn ambiguous_legacy_active_work_fails_migration_without_rewriting_it() {
        let db = Db::open_in_memory().unwrap();
        legacy_intentions(&db);
        db.lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO intentions VALUES ('1','first','7','','active',10,10,0);
             INSERT INTO intentions VALUES ('2','second','8','','active',11,11,0);",
            )
            .unwrap();
        assert!(migrate(&db).is_err());
        let columns: Vec<String> = {
            let conn = db.lock().unwrap();
            let mut stmt = conn.prepare("PRAGMA table_info(intentions)").unwrap();
            stmt.query_map([], |row| row.get(1))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert!(!columns.iter().any(|name| name == "origin"));
    }

    #[test]
    fn forgetting_its_own_intentions_keeps_the_research_they_asked_for() {
        use endora_kernel::ids::MessageId;
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        let at = Timestamp::from_unix_millis(1);
        let mut own =
            Intention::form(IntentionId::new(1), "run more often", BeliefId::new(9), at).unwrap();
        // Two of the same, as the live list had: one active, one long since let go.
        own.abandon();
        IntentionRepository::save(&store, &own).unwrap();
        IntentionRepository::save(
            &store,
            &Intention::form(IntentionId::new(2), "run more often", BeliefId::new(9), at).unwrap(),
        )
        .unwrap();
        IntentionRepository::save(
            &store,
            &Intention::from_request(
                IntentionId::new(3),
                "compare two laptops",
                MessageId::new(4),
                at,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(store.forget_its_own().unwrap(), 2);
        let left = IntentionRepository::list(&store).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].statement(), "compare two laptops");
    }

    #[test]
    fn what_they_told_it_moves_into_the_brain_once_and_the_table_goes() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        db.lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE preferences (id TEXT PRIMARY KEY, body TEXT NOT NULL,
                     kind TEXT NOT NULL, at_ms INTEGER NOT NULL) STRICT;
                 INSERT INTO preferences VALUES ('2', 'Short answers please', 'taste', 20);
                 INSERT INTO preferences VALUES ('1', 'Based in: New York', 'context', 10);",
            )
            .unwrap();
        let brain = BrainFolder::temporary().unwrap();
        assert_eq!(move_told_into_the_brain(&db, &brain).unwrap(), 2);
        let told: Vec<String> = PreferenceRepository::list_all(&brain)
            .unwrap()
            .iter()
            .map(|p| p.text().to_owned())
            .collect();
        assert_eq!(
            told,
            ["Based in: New York", "Short answers please"],
            "oldest first"
        );
        let gone: bool = db
            .lock()
            .unwrap()
            .query_row(
                "SELECT NOT EXISTS (SELECT 1 FROM sqlite_master WHERE name='preferences')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(gone, "the table is dropped once it has moved");
        assert_eq!(
            move_told_into_the_brain(&db, &brain).unwrap(),
            0,
            "and only once"
        );
    }

    #[test]
    fn an_intention_round_trips_with_its_reason_and_its_progress() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        let mut intention = Intention::form(
            IntentionId::new(1),
            "learn what helps them sleep",
            BeliefId::new(7),
            Timestamp::from_unix_millis(10),
        )
        .unwrap();
        intention.progress(
            "They mentioned the room being too warm.",
            Timestamp::from_unix_millis(20),
        );
        IntentionRepository::save(&store, &intention).unwrap();

        let stored = IntentionRepository::get(&store, IntentionId::new(1))
            .unwrap()
            .expect("saved");
        assert_eq!(stored, intention);
        assert_eq!(stored.motivating_belief(), Some(BeliefId::new(7)));
        assert_eq!(stored.note(), "They mentioned the room being too warm.");
        assert_eq!(stored.steps_taken(), 1);
    }

    #[test]
    fn only_the_active_intention_is_the_current_one() {
        // ADR 0052's cursor-not-queue rule, from the reading side: finished intentions
        // stay visible in `list` but never come back as something Endora is doing.
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);

        let mut done = Intention::form(
            IntentionId::new(1),
            "an earlier thread",
            BeliefId::new(7),
            Timestamp::from_unix_millis(10),
        )
        .unwrap();
        done.complete();
        IntentionRepository::save(&store, &done).unwrap();

        assert!(
            IntentionRepository::active(&store).unwrap().is_none(),
            "a finished intention is not the current one"
        );

        let current = Intention::form(
            IntentionId::new(2),
            "what Endora is on now",
            BeliefId::new(8),
            Timestamp::from_unix_millis(30),
        )
        .unwrap();
        IntentionRepository::save(&store, &current).unwrap();

        assert_eq!(
            IntentionRepository::active(&store)
                .unwrap()
                .expect("one is active")
                .id(),
            IntentionId::new(2)
        );
        assert_eq!(
            IntentionRepository::list(&store).unwrap().len(),
            2,
            "the finished one stays visible"
        );
    }

    #[test]
    fn an_outcome_round_trips_with_its_claim_and_observation_intact() {
        // The point of the record (ADR 0053): a claim of success and an observation
        // that contradicts it must both survive storage, unreconciled.
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        let outcome = Outcome::record(
            OutcomeId::new(1),
            "home.HassTurnOff",
            r#"{"name":"kitchen"}"#,
            "action_done",
            Some("kitchen switch: on"),
            Timestamp::from_unix_millis(10),
            Some(BeliefId::new(7)),
            None,
        )
        .unwrap();
        OutcomeRepository::save(&store, &outcome).unwrap();

        let stored = OutcomeRepository::get(&store, OutcomeId::new(1))
            .unwrap()
            .expect("saved");
        assert_eq!(stored, outcome);
        assert_eq!(stored.claim(), "action_done");
        assert_eq!(stored.observation(), Some("kitchen switch: on"));
        assert_eq!(stored.motivating_belief(), Some(BeliefId::new(7)));
        assert_eq!(stored.reaction(), None);
    }

    #[test]
    fn a_reaction_is_persisted_and_replaces_the_earlier_one() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        let mut outcome = Outcome::record(
            OutcomeId::new(1),
            "weather",
            "{}",
            "done",
            None,
            Timestamp::from_unix_millis(10),
            None,
            None,
        )
        .unwrap();
        OutcomeRepository::save(&store, &outcome).unwrap();

        outcome.react(Reaction::Helped);
        OutcomeRepository::save(&store, &outcome).unwrap();
        let all = OutcomeRepository::list(&store).unwrap();
        assert_eq!(all.len(), 1, "reacting updates rather than duplicating");
        assert_eq!(all[0].reaction(), Some(Reaction::Helped));

        // They may change their mind; the latest word wins.
        outcome.react(Reaction::DidNotHelp);
        OutcomeRepository::save(&store, &outcome).unwrap();
        assert_eq!(
            OutcomeRepository::list(&store).unwrap()[0].reaction(),
            Some(Reaction::DidNotHelp)
        );
    }

    #[test]
    fn outcomes_come_back_most_recent_first() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        for (id, at) in [(1_u128, 10_i64), (2, 30), (3, 20)] {
            let outcome = Outcome::record(
                OutcomeId::new(id),
                "weather",
                "{}",
                "done",
                None,
                Timestamp::from_unix_millis(at),
                None,
                None,
            )
            .unwrap();
            OutcomeRepository::save(&store, &outcome).unwrap();
        }
        let ids: Vec<u128> = OutcomeRepository::list(&store)
            .unwrap()
            .iter()
            .map(|o| o.id().value())
            .collect();
        assert_eq!(ids, vec![2, 3, 1]);
    }

    // --- Notions (ADR 0057) ---
}
