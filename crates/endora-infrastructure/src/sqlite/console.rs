//! What the Console contract needs from the node's store (ADR 0145): when a session ends, ending
//! sessions on request, and the owner's recorded answers to desk items.
//!
//! A session is still looked up only by its digest (ADR 0125); nothing here stores or compares
//! a token itself.

use rusqlite::{OptionalExtension, params};

use super::{SqliteStore, backend, same_text, session_digest};
use endora_application::RepositoryError;

/// What the store knows of one live session: enough to say when it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSeen {
    /// `full`, `checks` or `dashboard`.
    pub scope: String,
    /// When it was issued.
    pub issued_ms: i64,
    /// When it was last let in.
    pub last_seen_ms: i64,
    /// A short reference to it, from its digest: safe to show and to record, opens nothing.
    pub reference: String,
}

/// The owner's answer to one desk item, and how far it has got.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecisionAnswer {
    /// The desk item's id.
    pub desk: i64,
    /// The item's kind, as HQ named it when the answer was given.
    pub kind: String,
    /// The choice.
    pub choice: String,
    /// The owner's words, if any.
    pub words: String,
    /// Who answered: the node's one account.
    pub person: String,
    /// Which session answered ([`SessionSeen::reference`]), or `bootstrap`.
    pub session: String,
    /// Which client it came through.
    pub channel: String,
    /// The client's key for the press.
    pub request: String,
    /// When the node reserved the item for it.
    pub requested_ms: i64,
    /// When HQ recorded it.
    pub accepted_ms: Option<i64>,
    /// When what it does ran, whether that succeeded, and what came of it.
    pub executed: Option<(i64, bool, String)>,
    /// When it was read back, whether that matched, and how it was checked.
    pub verified: Option<(i64, bool, String)>,
}

/// A session's reference: `s-` and the first eight hex digits of its digest.
fn reference_of(digest: &str) -> String {
    let hex = digest.rsplit(':').next().unwrap_or_default();
    format!("s-{}", &hex[..hex.len().min(8)])
}

impl SqliteStore {
    /// The live session `token` opens, with when it was issued and last used, or `None`.
    ///
    /// The same limits as [`SqliteStore::session_scope`]; this one only reads.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn session_seen(
        &self,
        token: &str,
        now_ms: i64,
        keep_for_ms: i64,
        idle_ms: i64,
    ) -> Result<Option<SessionSeen>, RepositoryError> {
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
        Ok(Some(SessionSeen {
            scope,
            issued_ms,
            last_seen_ms,
            reference: reference_of(&digest),
        }))
    }

    /// Ends the session `token` opens. True if there was one.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn end_session(&self, token: &str) -> Result<bool, RepositoryError> {
        if token.is_empty() {
            return Ok(false);
        }
        let conn = self.db.lock()?;
        let ended = conn
            .execute(
                "DELETE FROM node_sessions WHERE token = ?1",
                params![session_digest(token)],
            )
            .map_err(backend)?;
        Ok(ended > 0)
    }

    /// Ends every session of `scope` but the one `keep` opens (none kept if `keep` is empty).
    /// How many ended.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn end_sessions_of(&self, scope: &str, keep: &str) -> Result<usize, RepositoryError> {
        let kept = if keep.is_empty() {
            String::new()
        } else {
            session_digest(keep)
        };
        let conn = self.db.lock()?;
        conn.execute(
            "DELETE FROM node_sessions WHERE scope = ?1 AND token != ?2",
            params![scope, kept],
        )
        .map_err(backend)
    }

    /// Reserves desk item `answer.desk` for this answer. False if the item already has one,
    /// which is how a second press or a concurrent retry finds the first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn reserve_decision(&self, answer: &DecisionAnswer) -> Result<bool, RepositoryError> {
        let conn = self.db.lock()?;
        let added = conn
            .execute(
                "INSERT OR IGNORE INTO decision_answers \
                 (desk, kind, choice, words, person, session, channel, request, requested_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    answer.desk,
                    answer.kind,
                    answer.choice,
                    answer.words,
                    answer.person,
                    answer.session,
                    answer.channel,
                    answer.request,
                    answer.requested_ms
                ],
            )
            .map_err(backend)?;
        Ok(added > 0)
    }

    /// Forgets the answer reserved for `desk`, when HQ refused it and nothing was recorded.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn release_decision(&self, desk: i64) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "DELETE FROM decision_answers WHERE desk = ?1 AND accepted_ms IS NULL",
            params![desk],
        )
        .map_err(backend)?;
        Ok(())
    }

    /// Writes down that HQ recorded the answer for `desk`, with its kind; only the first time.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn decision_accepted(
        &self,
        desk: i64,
        kind: &str,
        at_ms: i64,
    ) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "UPDATE decision_answers SET accepted_ms = ?2, \
             kind = CASE WHEN ?3 = '' THEN kind ELSE ?3 END \
             WHERE desk = ?1 AND accepted_ms IS NULL",
            params![desk, at_ms, kind],
        )
        .map_err(backend)?;
        Ok(())
    }

    /// Writes down what came of running the answer for `desk`; only the first time.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn decision_executed(
        &self,
        desk: i64,
        at_ms: i64,
        ok: bool,
        said: &str,
    ) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "UPDATE decision_answers SET executed_ms = ?2, executed_ok = ?3, executed_said = ?4 \
             WHERE desk = ?1 AND executed_ms IS NULL",
            params![desk, at_ms, i64::from(ok), said],
        )
        .map_err(backend)?;
        Ok(())
    }

    /// Writes down the read-back of the answer for `desk`. A later read-back replaces an
    /// earlier one that did not match; one that matched stands.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn decision_verified(
        &self,
        desk: i64,
        at_ms: i64,
        ok: bool,
        how: &str,
    ) -> Result<(), RepositoryError> {
        let conn = self.db.lock()?;
        conn.execute(
            "UPDATE decision_answers SET verified_ms = ?2, verified_ok = ?3, verified_how = ?4 \
             WHERE desk = ?1 AND (verified_ok IS NULL OR verified_ok = 0)",
            params![desk, at_ms, i64::from(ok), how],
        )
        .map_err(backend)?;
        Ok(())
    }

    /// The answer recorded for `desk`, if any.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn decision(&self, desk: i64) -> Result<Option<DecisionAnswer>, RepositoryError> {
        Ok(self
            .decisions_where("WHERE desk = ?1", desk, 1)?
            .into_iter()
            .next())
    }

    /// The latest `most` answers, newest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn latest_decisions(&self, most: usize) -> Result<Vec<DecisionAnswer>, RepositoryError> {
        self.decisions_where("WHERE ?1 = ?1", 0, most)
    }

    fn decisions_where(
        &self,
        filter: &str,
        value: i64,
        most: usize,
    ) -> Result<Vec<DecisionAnswer>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT desk, kind, choice, words, person, session, channel, request, \
                 requested_ms, accepted_ms, executed_ms, executed_ok, executed_said, \
                 verified_ms, verified_ok, verified_how FROM decision_answers {filter} \
                 ORDER BY requested_ms DESC, desk DESC LIMIT ?2"
            ))
            .map_err(backend)?;
        let rows = stmt
            .query_map(
                params![value, i64::try_from(most).unwrap_or(i64::MAX)],
                |r| {
                    let executed_ms: Option<i64> = r.get(10)?;
                    let executed_ok: Option<i64> = r.get(11)?;
                    let verified_ms: Option<i64> = r.get(13)?;
                    let verified_ok: Option<i64> = r.get(14)?;
                    Ok(DecisionAnswer {
                        desk: r.get(0)?,
                        kind: r.get(1)?,
                        choice: r.get(2)?,
                        words: r.get(3)?,
                        person: r.get(4)?,
                        session: r.get(5)?,
                        channel: r.get(6)?,
                        request: r.get(7)?,
                        requested_ms: r.get(8)?,
                        accepted_ms: r.get(9)?,
                        executed: executed_ms
                            .map(|at| (at, executed_ok == Some(1), r.get(12).unwrap_or_default())),
                        verified: verified_ms
                            .map(|at| (at, verified_ok == Some(1), r.get(15).unwrap_or_default())),
                    })
                },
            )
            .map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }
}

#[cfg(test)]
mod tests {
    use super::{DecisionAnswer, SqliteStore};

    const DAY: i64 = 24 * 60 * 60 * 1_000;

    fn answer(desk: i64, choice: &str) -> DecisionAnswer {
        DecisionAnswer {
            desk,
            choice: choice.to_owned(),
            person: "owner".to_owned(),
            session: "s-00000000".to_owned(),
            channel: "console".to_owned(),
            request: "k-1".to_owned(),
            requested_ms: 1_000,
            ..DecisionAnswer::default()
        }
    }

    #[test]
    fn a_session_says_when_it_was_issued_and_a_reference_that_is_not_the_token() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .add_session("session-token-one", "full", 5_000, 30 * DAY)
            .unwrap();
        let seen = store
            .session_seen("session-token-one", 6_000, 30 * DAY, 7 * DAY)
            .unwrap()
            .unwrap();
        assert_eq!((seen.scope.as_str(), seen.issued_ms), ("full", 5_000));
        assert!(seen.reference.starts_with("s-") && seen.reference.len() == 10);
        assert!(!seen.reference.contains("session-token"));
        assert!(
            store
                .session_seen("not-a-session", 6_000, 30 * DAY, 7 * DAY)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_ended_session_opens_nothing() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.add_session("mine", "full", 0, 30 * DAY).unwrap();
        assert!(store.end_session("mine").unwrap());
        assert!(!store.end_session("mine").unwrap(), "already ended");
        assert_eq!(store.session_scope("mine", 1, 30 * DAY, 7 * DAY), Ok(None));
    }

    #[test]
    fn ending_every_console_session_keeps_the_one_asked_and_the_other_scopes() {
        let store = SqliteStore::open_in_memory().unwrap();
        for (token, scope) in [("a", "full"), ("b", "full"), ("c", "checks"), ("d", "full")] {
            store.add_session(token, scope, 0, 30 * DAY).unwrap();
        }
        assert_eq!(store.end_sessions_of("full", "a").unwrap(), 2);
        let open = |t: &str| store.session_scope(t, 1, 30 * DAY, 7 * DAY).unwrap();
        assert_eq!(open("a").as_deref(), Some("full"));
        assert_eq!(open("b"), None);
        assert_eq!(open("c").as_deref(), Some("checks"));
        assert_eq!(store.end_sessions_of("full", "").unwrap(), 1, "none kept");
        assert_eq!(open("a"), None);
    }

    #[test]
    fn an_item_is_reserved_once_and_its_stages_are_written_once() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert!(store.reserve_decision(&answer(7, "approve")).unwrap());
        assert!(
            !store.reserve_decision(&answer(7, "decline")).unwrap(),
            "the second finds the first"
        );
        store.decision_accepted(7, "goal", 2_000).unwrap();
        store.decision_accepted(7, "other", 9_000).unwrap();
        store.decision_executed(7, 2_000, true, "applied").unwrap();
        store.decision_executed(7, 9_000, false, "again").unwrap();
        store.decision_verified(7, 3_000, false, "not yet").unwrap();
        store
            .decision_verified(7, 4_000, true, "read back")
            .unwrap();
        store.decision_verified(7, 9_000, false, "later").unwrap();
        let kept = store.decision(7).unwrap().unwrap();
        assert_eq!(kept.choice, "approve");
        assert_eq!(kept.kind, "goal");
        assert_eq!(kept.accepted_ms, Some(2_000));
        assert_eq!(kept.executed, Some((2_000, true, "applied".to_owned())));
        assert_eq!(kept.verified, Some((4_000, true, "read back".to_owned())));
    }

    #[test]
    fn a_refused_reservation_is_released_but_an_accepted_answer_never_is() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.reserve_decision(&answer(1, "approve")).unwrap();
        store.release_decision(1).unwrap();
        assert_eq!(store.decision(1).unwrap(), None);
        store.reserve_decision(&answer(2, "approve")).unwrap();
        store.decision_accepted(2, "", 5).unwrap();
        store.release_decision(2).unwrap();
        assert!(store.decision(2).unwrap().is_some());
    }

    #[test]
    fn the_latest_answers_come_newest_first() {
        let store = SqliteStore::open_in_memory().unwrap();
        for (desk, at) in [(1, 10), (2, 30), (3, 20)] {
            let mut a = answer(desk, "seen");
            a.requested_ms = at;
            store.reserve_decision(&a).unwrap();
        }
        let desks: Vec<i64> = store
            .latest_decisions(2)
            .unwrap()
            .iter()
            .map(|a| a.desk)
            .collect();
        assert_eq!(desks, [2, 3]);
    }
}
