//! Understanding infrastructure — SQLite-backed belief, preference, outcome and
//! intention repositories.

use endora_kernel::RepositoryError;
use endora_kernel::ids::{BeliefId, IntentionId, NotionId, OutcomeId, PreferenceId, Timestamp};
use endora_persistence::{Db, backend, corrupt, id_text, parse_id};
use rusqlite::{Connection, params};

use crate::application::{
    BeliefRepository, IntentionRepository, NotionRepository, OutcomeRepository,
    PreferenceRepository,
};
use crate::domain::{
    Belief, BeliefKind, BeliefStatus, Citation, Confidence, Intention, IntentionState, Notion,
    NotionStatus, Outcome, Preference, PreferenceKind, Reaction, Source,
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
            CREATE TABLE IF NOT EXISTS notions (
                id                TEXT PRIMARY KEY,
                statement         TEXT NOT NULL,
                settles_when      TEXT NOT NULL,
                created_ms        INTEGER NOT NULL,
                last_supported_ms INTEGER NOT NULL,
                status            TEXT NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS notion_citations (
                notion_id TEXT NOT NULL,
                source    TEXT NOT NULL,
                reference TEXT NOT NULL,
                PRIMARY KEY (notion_id, source, reference)
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

impl NotionRepository for UnderstandingStore {
    fn save(&self, notion: &Notion) -> Result<(), RepositoryError> {
        let mut conn = self.db.lock()?;
        // The row and its evidence are one fact, so they move together. Without the
        // transaction a failure between the two statements leaves a notion whose citations
        // were just deleted — which reads as "nothing supports this" and gets it killed by
        // the very rule that is supposed to protect the person from unfounded speculation.
        let tx = conn.transaction().map_err(backend)?;
        let id = id_text(notion.id().value());
        tx.execute(
            "INSERT OR REPLACE INTO notions \
             (id, statement, settles_when, created_ms, last_supported_ms, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                notion.statement(),
                notion.settles_when(),
                notion.created_at().unix_millis(),
                notion.last_supported_at().unix_millis(),
                notion.status().name(),
            ],
        )
        .map_err(backend)?;
        tx.execute(
            "DELETE FROM notion_citations WHERE notion_id = ?1",
            params![id],
        )
        .map_err(backend)?;
        for citation in notion.citations() {
            tx.execute(
                "INSERT OR IGNORE INTO notion_citations (notion_id, source, reference) \
                 VALUES (?1, ?2, ?3)",
                params![id, citation.source().name(), citation.reference()],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(())
    }

    fn get(&self, id: NotionId) -> Result<Option<Notion>, RepositoryError> {
        let conn = self.db.lock()?;
        Ok(all_notions(&conn)?.into_iter().find(|n| n.id() == id))
    }

    fn open(&self) -> Result<Vec<Notion>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut open: Vec<Notion> = all_notions(&conn)?
            .into_iter()
            .filter(|n| n.status() == NotionStatus::Open)
            .collect();
        // Best-supported first, so the caller reading only the head of this list is reading
        // the notions with the most behind them.
        open.sort_by_key(|n| std::cmp::Reverse(n.standing()));
        Ok(open)
    }

    fn list(&self) -> Result<Vec<Notion>, RepositoryError> {
        let conn = self.db.lock()?;
        all_notions(&conn)
    }
}

impl IntentionRepository for UnderstandingStore {
    fn save(&self, intention: &Intention) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO intentions \
                 (id, statement, motivating_belief, note, state, created_ms, \
                  last_progressed_ms, steps_taken) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id_text(intention.id().value()),
                    intention.statement(),
                    id_text(intention.motivating_belief().value()),
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
}

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

/// Every notion with its evidence, most recently supported first.
///
/// A citation whose stored `source` is not one this build understands is **dropped**, and a
/// notion left with none is dropped with it. That is the cautious direction and it matches
/// what the domain refuses to construct: a citation nobody knows how to check is exactly the
/// thing that must never come back as evidence, and reading one is no safer than writing one.
fn all_notions(conn: &Connection) -> Result<Vec<Notion>, RepositoryError> {
    let mut stmt = conn
        .prepare(
            "SELECT n.id, n.statement, n.settles_when, n.created_ms, n.last_supported_ms, \
                    n.status, c.source, c.reference \
             FROM notions n LEFT JOIN notion_citations c ON c.notion_id = n.id \
             ORDER BY n.last_supported_ms DESC, n.rowid DESC, c.source, c.reference",
        )
        .map_err(backend)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(backend)?;

    // The join gives one row per citation, so the notion's own fields repeat. Grouping by id
    // keeps the query to one round trip while still assembling whole notions.
    let mut order: Vec<String> = Vec::new();
    let mut gathered: std::collections::HashMap<String, (Notion, Vec<Citation>)> =
        std::collections::HashMap::new();
    for row in rows {
        let (id, statement, settles, created_ms, supported_ms, status, source, reference) =
            row.map_err(backend)?;
        if !gathered.contains_key(&id) {
            order.push(id.clone());
            gathered.insert(
                id.clone(),
                (
                    Notion::from_parts(
                        NotionId::new(parse_id(&id)?),
                        statement,
                        Vec::new(),
                        settles,
                        Timestamp::from_unix_millis(created_ms),
                        Timestamp::from_unix_millis(supported_ms),
                        NotionStatus::from_name(&status),
                    ),
                    Vec::new(),
                ),
            );
        }
        let (Some(source), Some(reference)) = (source, reference) else {
            continue; // a notion with no citations at all
        };
        let Ok(source) = Source::from_name(&source) else {
            continue; // a kind of evidence this build cannot check
        };
        let Ok(citation) = Citation::new(source, &reference) else {
            continue; // an unnamed record cannot be checked
        };
        if let Some((_, citations)) = gathered.get_mut(&id) {
            citations.push(citation);
        }
    }

    let mut out = Vec::new();
    for id in order {
        let Some((notion, mut citations)) = gathered.remove(&id) else {
            continue;
        };
        // The domain keeps citations sorted by its own ordering; SQL sorted them as text. A
        // notion that came back in a different order than it went in would not compare equal
        // to itself, which is a trap for every later caller rather than a cosmetic difference.
        citations.sort();
        if citations.is_empty() {
            continue; // no evidence survived; it is not a notion (ADR 0057)
        }
        out.push(Notion::from_parts(
            notion.id(),
            notion.statement().to_owned(),
            citations,
            notion.settles_when().to_owned(),
            notion.created_at(),
            notion.last_supported_at(),
            notion.status(),
        ));
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
    use crate::application::{
        BeliefRepository, IntentionRepository, NotionRepository, OutcomeRepository,
        PreferenceRepository,
    };
    use crate::domain::{
        Citation, Intention, Notion, NotionStatus, Outcome, Preference, PreferenceKind, Reaction,
        Source,
    };
    use endora_kernel::ids::NotionId;
    use endora_kernel::ids::{BeliefId, IntentionId, OutcomeId, PreferenceId, Timestamp};
    use endora_persistence::Db;
    use endora_persistence::id_text;
    use rusqlite::params;

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
        assert_eq!(stored.motivating_belief(), BeliefId::new(7));
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

    /// An open notion last supported at a chosen moment — the shape one has after sitting in
    /// the database for a while, which `new` alone cannot produce.
    fn a_notion(id: u128, references: &[(Source, &str)], supported_ms: i64) -> Notion {
        let citations = references
            .iter()
            .map(|(source, reference)| Citation::new(*source, reference).unwrap())
            .collect();
        let formed = Notion::new(
            NotionId::new(id),
            "the Monday gym block gets cancelled",
            citations,
            "whether next Monday's block survives",
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        Notion::from_parts(
            formed.id(),
            formed.statement().to_owned(),
            formed.citations().to_vec(),
            formed.settles_when().to_owned(),
            formed.created_at(),
            Timestamp::from_unix_millis(supported_ms),
            NotionStatus::Open,
        )
    }

    #[test]
    fn a_notion_round_trips_with_every_piece_of_its_evidence() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);

        let notion = a_notion(
            1,
            &[
                (Source::Outcome, "outcome-7"),
                (Source::Message, "msg-2"),
                (Source::Reading, "calendar.rustic"),
            ],
            500,
        );
        NotionRepository::save(&store, &notion).unwrap();

        let stored = NotionRepository::get(&store, NotionId::new(1))
            .unwrap()
            .expect("saved");
        assert_eq!(stored, notion);
        assert_eq!(stored.support_count(), 3);
    }

    #[test]
    fn saving_again_does_not_multiply_the_evidence() {
        // The write replaces a notion in place — supporting one and saving it must leave three
        // pieces of evidence, not six. Storing evidence twice would mature a notion on a
        // single record, which is the failure the count exists to prevent.
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);

        let mut notion = a_notion(1, &[(Source::Outcome, "outcome-7")], 100);
        NotionRepository::save(&store, &notion).unwrap();
        notion.support(
            Citation::new(Source::Message, "msg-2").unwrap(),
            Timestamp::from_unix_millis(200),
        );
        NotionRepository::save(&store, &notion).unwrap();
        NotionRepository::save(&store, &notion).unwrap();

        let stored = NotionRepository::get(&store, NotionId::new(1))
            .unwrap()
            .expect("saved");
        assert_eq!(stored.support_count(), 2);
        assert_eq!(stored.last_supported_at(), Timestamp::from_unix_millis(200));
    }

    #[test]
    fn only_open_notions_are_still_thought_about() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);

        NotionRepository::save(&store, &a_notion(1, &[(Source::Outcome, "a")], 100)).unwrap();
        let mut gone = a_notion(2, &[(Source::Outcome, "b")], 200);
        gone.die();
        NotionRepository::save(&store, &gone).unwrap();

        let open = NotionRepository::open(&store).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id(), NotionId::new(1));
        assert_eq!(
            NotionRepository::list(&store).unwrap().len(),
            2,
            "what it dropped stays visible"
        );
    }

    #[test]
    fn the_open_list_leads_with_the_best_supported() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db);

        NotionRepository::save(&store, &a_notion(1, &[(Source::Outcome, "a")], 900)).unwrap();
        NotionRepository::save(
            &store,
            &a_notion(2, &[(Source::Outcome, "b"), (Source::Message, "c")], 100),
        )
        .unwrap();

        let open = NotionRepository::open(&store).unwrap();
        assert_eq!(
            open[0].id(),
            NotionId::new(2),
            "two pieces of old evidence beat one recent one"
        );
    }

    #[test]
    fn a_notion_whose_evidence_cannot_be_checked_does_not_come_back() {
        // A row written by a later build, read by this one. Dropping it is the same caution
        // the domain applies at construction: evidence nobody knows how to verify must never
        // count, and a notion left with none is not a notion at all (ADR 0057).
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = UnderstandingStore::new(db.clone());

        NotionRepository::save(&store, &a_notion(1, &[(Source::Outcome, "a")], 100)).unwrap();
        db.lock()
            .unwrap()
            .execute(
                "UPDATE notion_citations SET source = 'vibes' WHERE notion_id = ?1",
                params![id_text(1)],
            )
            .unwrap();

        assert!(NotionRepository::list(&store).unwrap().is_empty());
    }
}
