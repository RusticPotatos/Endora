//! The team's map in HQ's store (ADR 0126). Its tables are made only if missing, so a live store
//! gains them empty and no row of any other table changes.

use rusqlite::params;

use super::{SqliteTeamStore, backend, corrupt};
use crate::application::map_ports::MapStore;
use crate::application::ports::Stored;
use crate::domain::map::{PageProposal, ProposalStatus};
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
";

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
}
