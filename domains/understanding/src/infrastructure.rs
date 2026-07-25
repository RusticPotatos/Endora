//! Understanding infrastructure — SQLite-backed belief, preference and outcome
//! repositories.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{BeliefId, OutcomeId, PreferenceId, Timestamp};
use endora_persistence::{Db, backend, corrupt, id_text, parse_id};
use rusqlite::{Connection, params};

use crate::application::{BeliefRepository, OutcomeRepository, PreferenceRepository};
use crate::domain::{
    Belief, BeliefKind, BeliefStatus, Confidence, Outcome, Preference, PreferenceKind, Reaction,
};

/// Creates the understanding tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS preferences (
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
            CREATE TABLE IF NOT EXISTS outcomes (
                id                TEXT PRIMARY KEY,
                capability        TEXT NOT NULL,
                input             TEXT NOT NULL,
                claim             TEXT NOT NULL,
                observation       TEXT,
                at_ms             INTEGER NOT NULL,
                motivating_belief TEXT,
                reaction          TEXT
            ) STRICT;",
        )
        .map_err(backend)?;
    Ok(())
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

impl PreferenceRepository for UnderstandingStore {
    fn save(&self, preference: &Preference) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
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
        let conn = self.db.lock()?;
        all_preferences(&conn)
    }

    fn delete(&self, id: PreferenceId) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "DELETE FROM preferences WHERE id = ?1",
                params![id_text(id.value())],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl BeliefRepository for UnderstandingStore {
    fn save(&self, belief: &Belief) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
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
        let conn = self.db.lock()?;
        Ok(all_beliefs(&conn)?.into_iter().find(|b| b.id() == id))
    }

    fn list(&self) -> Result<Vec<Belief>, RepositoryError> {
        let conn = self.db.lock()?;
        all_beliefs(&conn)
    }
}

impl OutcomeRepository for UnderstandingStore {
    fn save(&self, outcome: &Outcome) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO outcomes \
                 (id, capability, input, claim, observation, at_ms, motivating_belief, reaction) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id_text(outcome.id().value()),
                    outcome.capability(),
                    outcome.input(),
                    outcome.claim(),
                    outcome.observation(),
                    outcome.at().unix_millis(),
                    outcome.motivating_belief().map(|b| id_text(b.value())),
                    outcome.reaction().map(Reaction::name),
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
            "SELECT id, capability, input, claim, observation, at_ms, motivating_belief, reaction \
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
            ))
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, capability, input, claim, observation, at_ms, belief, reaction) =
            row.map_err(backend)?;
        // A reaction we cannot parse is corrupt, but an *absent* one is the normal case
        // (ADR 0035 — the person is never asked), so only a present-and-unknown value is
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
        ));
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::{UnderstandingStore, migrate};
    use crate::application::{BeliefRepository, OutcomeRepository, PreferenceRepository};
    use crate::domain::{Outcome, Preference, PreferenceKind, Reaction};
    use endora_kernel::ids::{BeliefId, OutcomeId, PreferenceId, Timestamp};
    use endora_persistence::Db;

    #[test]
    fn preferences_round_trip_and_delete() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        let p = Preference::new(
            PreferenceId::new(1),
            "prefers mornings",
            PreferenceKind::Taste,
            Timestamp::from_unix_millis(10),
        )
        .unwrap();
        PreferenceRepository::save(&store, &p).unwrap();
        assert_eq!(store.list_all().unwrap().len(), 1);
        store.delete(PreferenceId::new(1)).unwrap();
        assert!(store.list_all().unwrap().is_empty());
    }

    #[test]
    fn beliefs_list_is_empty_initially() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);
        assert!(BeliefRepository::list(&store).unwrap().is_empty());
    }

    #[test]
    fn an_outcome_round_trips_with_its_claim_and_observation_intact() {
        // The point of the record (ADR 0035): a claim of success and an observation
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
}
