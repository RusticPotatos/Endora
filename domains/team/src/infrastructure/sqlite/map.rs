//! The team's map in HQ's store (ADR 0126). Its tables are made only if missing, so a live store
//! gains them empty and no row of any other table changes.

use rusqlite::params;

use super::{SqliteTeamStore, backend, corrupt};
use crate::application::map_ports::MapStore;
use crate::application::ports::Stored;
use crate::domain::map::{Gap, PageProposal, ProposalStatus};
use crate::domain::quiz::{Quiz, Status as QuizStatus};
use crate::domain::world::Snapshot;
use crate::domain::{MemberId, Millis, RunId};

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS world_pages (
    slug TEXT PRIMARY KEY, created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS world_proposals (
    id INTEGER PRIMARY KEY, page TEXT NOT NULL, anchor TEXT, heading TEXT NOT NULL,
    headline TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '[]',
    by_member INTEGER NOT NULL REFERENCES members(id), run INTEGER REFERENCES runs(id),
    at_ms INTEGER NOT NULL, status TEXT NOT NULL, decided_by TEXT NOT NULL DEFAULT '',
    why TEXT NOT NULL DEFAULT '', decided_at INTEGER, told INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX IF NOT EXISTS world_proposals_by_status ON world_proposals(status);
CREATE TABLE IF NOT EXISTS world_gaps (
    id INTEGER PRIMARY KEY, what TEXT NOT NULL, where_looked TEXT NOT NULL, key TEXT NOT NULL,
    steward INTEGER REFERENCES members(id), by_member INTEGER NOT NULL REFERENCES members(id),
    run INTEGER REFERENCES runs(id), count INTEGER NOT NULL, opened_at INTEGER NOT NULL,
    last_at INTEGER NOT NULL, closed_at INTEGER, closed_by TEXT NOT NULL DEFAULT '',
    how TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE TABLE IF NOT EXISTS world_quizzes (
    id INTEGER PRIMARY KEY, night INTEGER NOT NULL UNIQUE, status TEXT NOT NULL,
    member INTEGER REFERENCES members(id), asked_at INTEGER, answered_at INTEGER,
    questions TEXT NOT NULL DEFAULT '[]', answers TEXT NOT NULL DEFAULT '[]',
    graded TEXT NOT NULL DEFAULT '[]', why TEXT NOT NULL DEFAULT ''
) STRICT;
";

const QUIZ_SELECT: &str = "SELECT id, night, status, member, asked_at, answered_at, questions, \
    answers, graded, why FROM world_quizzes";

fn quiz_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Quiz, String)> {
    Ok((
        Quiz {
            id: r.get(0)?,
            night: r.get(1)?,
            status: QuizStatus::Skipped,
            member: r.get::<_, Option<i64>>(3)?.map(MemberId),
            asked_at: r.get(4)?,
            answered_at: r.get(5)?,
            questions: Quiz::read_questions(&r.get::<_, String>(6)?),
            answers: Quiz::read_answers(&r.get::<_, String>(7)?),
            graded: Quiz::read_graded(&r.get::<_, String>(8)?),
            why: r.get(9)?,
        },
        r.get(2)?,
    ))
}

const GAP_SELECT: &str = "SELECT id, what, where_looked, key, steward, by_member, run, count, \
    opened_at, last_at, closed_at, closed_by, how FROM world_gaps";

fn gap_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Gap> {
    Ok(Gap {
        id: r.get(0)?,
        what: r.get(1)?,
        where_looked: r.get(2)?,
        key: r.get(3)?,
        steward: r.get::<_, Option<i64>>(4)?.map(MemberId),
        by: MemberId(r.get(5)?),
        run: r.get::<_, Option<i64>>(6)?.map(RunId),
        count: r.get(7)?,
        opened_at: r.get(8)?,
        last_at: r.get(9)?,
        closed_at: r.get(10)?,
        closed_by: r.get(11)?,
        how: r.get(12)?,
    })
}

const PROPOSAL_SELECT: &str = "SELECT id, page, anchor, heading, headline, body, evidence, \
    by_member, run, at_ms, status, decided_by, why, decided_at, told FROM world_proposals";

fn proposal_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(PageProposal, String)> {
    Ok((
        PageProposal {
            id: r.get(0)?,
            page: r.get(1)?,
            anchor: r.get(2)?,
            heading: r.get(3)?,
            headline: r.get(4)?,
            body: r.get(5)?,
            evidence: Snapshot::read(&r.get::<_, String>(6)?),
            by: MemberId(r.get(7)?),
            run: r.get::<_, Option<i64>>(8)?.map(RunId),
            at: r.get(9)?,
            status: ProposalStatus::Open,
            decided_by: r.get(11)?,
            why: r.get(12)?,
            decided_at: r.get(13)?,
            told: r.get::<_, i64>(14)? != 0,
        },
        r.get(10)?,
    ))
}

impl MapStore for SqliteTeamStore {
    fn world_pages(&self) -> Stored<Vec<String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT slug FROM world_pages ORDER BY created_at, rowid")
            .map_err(backend)?;
        stmt.query_map([], |r| r.get(0))
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(backend)
    }

    fn add_world_page(&self, slug: &str, at: Millis) -> Stored<bool> {
        let added = self
            .conn()?
            .execute(
                "INSERT OR IGNORE INTO world_pages (slug, created_at) VALUES (?1, ?2)",
                params![slug, at],
            )
            .map_err(backend)?;
        Ok(added == 1)
    }

    fn proposals(&self) -> Stored<Vec<PageProposal>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{PROPOSAL_SELECT} ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], proposal_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(mut p, status)| {
                p.status = ProposalStatus::from_name(&status)
                    .ok_or_else(|| corrupt("a proposal's status"))?;
                Ok(p)
            })
            .collect()
    }

    fn save_proposal(&self, p: &PageProposal) -> Stored<i64> {
        let conn = self.conn()?;
        if p.id != 0 {
            conn.execute(
                "UPDATE world_proposals SET status = ?2, decided_by = ?3, why = ?4,
                    decided_at = ?5, told = ?6 WHERE id = ?1",
                params![
                    p.id,
                    p.status.name(),
                    p.decided_by,
                    p.why,
                    p.decided_at,
                    p.told
                ],
            )
            .map_err(backend)?;
            return Ok(p.id);
        }
        conn.execute(
            "INSERT INTO world_proposals (page, anchor, heading, headline, body, evidence,
                by_member, run, at_ms, status, decided_by, why, decided_at, told)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                p.page,
                p.anchor,
                p.heading,
                p.headline,
                p.body,
                Snapshot::keep(&p.evidence),
                p.by.0,
                p.run.map(|r| r.0),
                p.at,
                p.status.name(),
                p.decided_by,
                p.why,
                p.decided_at,
                p.told
            ],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn gaps(&self) -> Stored<Vec<Gap>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{GAP_SELECT} ORDER BY id"))
            .map_err(backend)?;
        stmt.query_map([], gap_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn save_gap(&self, g: &Gap) -> Stored<i64> {
        let conn = self.conn()?;
        if g.id != 0 {
            conn.execute(
                "UPDATE world_gaps SET count = ?2, last_at = ?3, closed_at = ?4, closed_by = ?5,
                    how = ?6 WHERE id = ?1",
                params![g.id, g.count, g.last_at, g.closed_at, g.closed_by, g.how],
            )
            .map_err(backend)?;
            return Ok(g.id);
        }
        conn.execute(
            "INSERT INTO world_gaps (what, where_looked, key, steward, by_member, run, count,
                opened_at, last_at, closed_at, closed_by, how)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                g.what,
                g.where_looked,
                g.key,
                g.steward.map(|m| m.0),
                g.by.0,
                g.run.map(|r| r.0),
                g.count,
                g.opened_at,
                g.last_at,
                g.closed_at,
                g.closed_by,
                g.how
            ],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn quizzes(&self) -> Stored<Vec<Quiz>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{QUIZ_SELECT} ORDER BY night"))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], quiz_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(mut q, status)| {
                q.status =
                    QuizStatus::from_name(&status).ok_or_else(|| corrupt("a quiz's status"))?;
                Ok(q)
            })
            .collect()
    }

    fn save_quiz(&self, q: &Quiz) -> Stored<i64> {
        let conn = self.conn()?;
        if q.id != 0 {
            conn.execute(
                "UPDATE world_quizzes SET status = ?2, answered_at = ?3, answers = ?4,
                    graded = ?5, why = ?6 WHERE id = ?1",
                params![
                    q.id,
                    q.status.name(),
                    q.answered_at,
                    Quiz::keep_answers(&q.answers),
                    Quiz::keep_graded(&q.graded),
                    q.why
                ],
            )
            .map_err(backend)?;
            return Ok(q.id);
        }
        conn.execute(
            "INSERT INTO world_quizzes (night, status, member, asked_at, answered_at, questions,
                answers, graded, why)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                q.night,
                q.status.name(),
                q.member.map(|m| m.0),
                q.asked_at,
                q.answered_at,
                Quiz::keep_questions(&q.questions),
                Quiz::keep_answers(&q.answers),
                Quiz::keep_graded(&q.graded),
                q.why
            ],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }
}
