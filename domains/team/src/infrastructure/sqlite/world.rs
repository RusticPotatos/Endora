//! The team's world in HQ's store (ADR 0122). Its tables are made only if missing, so a live
//! store gains them empty and no row of any other table changes. Revisions, timelines and
//! handover notes are append-only: a trigger refuses to change or remove one.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, backend, corrupt};
use crate::application::ports::Stored;
use crate::application::world_ports::WorldStore;
use crate::domain::situations::{Briefed, Situation};
use crate::domain::world::{Handover, HandoverNote, Scope, Section, Snapshot, Stage, Story};
use crate::domain::world_migration::{Fix, Plan, Row, TimelineLine};
use crate::domain::{MemberId, Millis, Ref, RunId, TaskId};

/// Set, in the same transaction as the move itself, once the old record is in the world.
pub(super) const MIGRATED: &str = "world_migrated";

/// Set, in the same transaction as the mending, once the move's wrong dates are mended.
pub(super) const REPAIRED: &str = "world_dates_repaired";

const LATEST_RECIPIENT_HANDOVER: &str =
    "SELECT id,task,by_member,from_member,to_member,run,done,not_done,next,watch_out,evidence,at_ms
    FROM handovers WHERE task=?1 AND to_member=?2 ORDER BY id DESC LIMIT 1";
const LATEST_TASK_HANDOVER: &str =
    "SELECT id,task,by_member,from_member,to_member,run,done,not_done,next,watch_out,evidence,at_ms
    FROM handovers WHERE task=?1 ORDER BY id DESC LIMIT 1";

/// The trigger that keeps a timeline row as written; the mending lifts it and puts it back
/// within its one transaction.
const TIMELINE_KEPT: &str = "CREATE TRIGGER IF NOT EXISTS world_timeline_is_kept_update
    BEFORE UPDATE ON world_timeline BEGIN SELECT RAISE(ABORT, 'a timeline is append-only'); END;";

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS world_sections (
    id INTEGER PRIMARY KEY, page TEXT NOT NULL, anchor TEXT NOT NULL, heading TEXT NOT NULL,
    headline TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', stage TEXT NOT NULL,
    warrant TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '[]', verified_at INTEGER,
    verified_by TEXT NOT NULL DEFAULT '', superseded_by TEXT, why TEXT NOT NULL DEFAULT '',
    written_by TEXT NOT NULL, written_in_run INTEGER REFERENCES runs(id),
    revision INTEGER NOT NULL, updated_at INTEGER NOT NULL, last_used_at INTEGER NOT NULL,
    UNIQUE (page, anchor)
) STRICT;
CREATE TABLE IF NOT EXISTS world_revisions (
    id INTEGER PRIMARY KEY, section INTEGER NOT NULL REFERENCES world_sections(id),
    revision INTEGER NOT NULL, heading TEXT NOT NULL, headline TEXT NOT NULL, body TEXT NOT NULL,
    stage TEXT NOT NULL, warrant TEXT NOT NULL, evidence TEXT NOT NULL, by TEXT NOT NULL,
    run INTEGER REFERENCES runs(id), at_ms INTEGER NOT NULL, why TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE TRIGGER IF NOT EXISTS world_revisions_are_kept_update
    BEFORE UPDATE ON world_revisions BEGIN SELECT RAISE(ABORT, 'a revision is kept as written'); END;
CREATE TRIGGER IF NOT EXISTS world_revisions_are_kept_delete
    BEFORE DELETE ON world_revisions BEGIN SELECT RAISE(ABORT, 'a revision is kept as written'); END;
CREATE TABLE IF NOT EXISTS world_timeline (
    id INTEGER PRIMARY KEY, page TEXT NOT NULL, at_ms INTEGER NOT NULL, by TEXT NOT NULL,
    run INTEGER REFERENCES runs(id), line TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS world_timeline_by_page ON world_timeline(page, at_ms);
CREATE TABLE IF NOT EXISTS world_timeline_repairs (
    id INTEGER PRIMARY KEY, row INTEGER NOT NULL, old_at_ms INTEGER NOT NULL,
    old_line TEXT NOT NULL, repaired_at INTEGER NOT NULL
) STRICT;
CREATE TRIGGER IF NOT EXISTS world_timeline_is_kept_update
    BEFORE UPDATE ON world_timeline BEGIN SELECT RAISE(ABORT, 'a timeline is append-only'); END;
CREATE TRIGGER IF NOT EXISTS world_timeline_is_kept_delete
    BEFORE DELETE ON world_timeline BEGIN SELECT RAISE(ABORT, 'a timeline is append-only'); END;
CREATE TABLE IF NOT EXISTS stories (
    id INTEGER PRIMARY KEY, scope TEXT NOT NULL, handle TEXT NOT NULL UNIQUE,
    moment TEXT NOT NULL DEFAULT '', reflex TEXT NOT NULL DEFAULT '', instead TEXT NOT NULL,
    because TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '[]', stage TEXT NOT NULL,
    warrant TEXT NOT NULL DEFAULT '', taught TEXT NOT NULL DEFAULT '', author TEXT NOT NULL,
    written_in_run INTEGER REFERENCES runs(id), written_at INTEGER NOT NULL,
    to_retell INTEGER NOT NULL DEFAULT 0, migrated_from TEXT UNIQUE, verified_at INTEGER,
    last_used_at INTEGER NOT NULL, why TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE TABLE IF NOT EXISTS handovers (
    id INTEGER PRIMARY KEY, task INTEGER NOT NULL REFERENCES tasks(id),
    by_member INTEGER NOT NULL REFERENCES members(id), from_member INTEGER REFERENCES members(id),
    to_member INTEGER REFERENCES members(id), run INTEGER REFERENCES runs(id),
    done TEXT NOT NULL, not_done TEXT NOT NULL, next TEXT NOT NULL, watch_out TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '', at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS handovers_by_task ON handovers(task);
CREATE INDEX IF NOT EXISTS handovers_for_member ON handovers(task,to_member,id);
CREATE TRIGGER IF NOT EXISTS handovers_are_kept_update
    BEFORE UPDATE ON handovers BEGIN SELECT RAISE(ABORT, 'a handover note is kept as written'); END;
CREATE TRIGGER IF NOT EXISTS handovers_are_kept_delete
    BEFORE DELETE ON handovers BEGIN SELECT RAISE(ABORT, 'a handover note is kept as written'); END;
CREATE TABLE IF NOT EXISTS run_briefings (
    run INTEGER PRIMARY KEY REFERENCES runs(id), situation TEXT NOT NULL,
    briefed TEXT NOT NULL, dropped TEXT NOT NULL, tokens INTEGER NOT NULL,
    budget INTEGER NOT NULL, over_budget INTEGER NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
";

const SECTION_SELECT: &str = "SELECT id, page, anchor, heading, headline, body, stage, warrant, \
    evidence, verified_at, verified_by, superseded_by, why, written_by, written_in_run, revision, \
    updated_at, last_used_at FROM world_sections";

fn section_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Section, String)> {
    let stage: String = r.get(6)?;
    Ok((
        Section {
            id: r.get(0)?,
            page: r.get(1)?,
            anchor: r.get(2)?,
            heading: r.get(3)?,
            headline: r.get(4)?,
            body: r.get(5)?,
            stage: Stage::Observation,
            warrant: r.get(7)?,
            evidence: Snapshot::read(&r.get::<_, String>(8)?),
            verified_at: r.get(9)?,
            verified_by: r.get(10)?,
            superseded_by: r.get(11)?,
            why: r.get(12)?,
            written_by: r.get(13)?,
            written_in_run: r.get::<_, Option<i64>>(14)?.map(RunId),
            revision: r.get(15)?,
            updated_at: r.get(16)?,
            last_used_at: r.get(17)?,
        },
        stage,
    ))
}

const STORY_SELECT: &str = "SELECT id, scope, handle, moment, reflex, instead, because, evidence, \
    stage, warrant, taught, author, written_in_run, written_at, to_retell, migrated_from, \
    verified_at, last_used_at, why FROM stories";

fn story_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Story, String, String)> {
    let scope: String = r.get(1)?;
    let stage: String = r.get(8)?;
    Ok((
        Story {
            id: r.get(0)?,
            scope: Scope::House,
            handle: r.get(2)?,
            moment: r.get(3)?,
            reflex: r.get(4)?,
            instead: r.get(5)?,
            because: r.get(6)?,
            evidence: Snapshot::read(&r.get::<_, String>(7)?),
            stage: Stage::Observation,
            warrant: r.get(9)?,
            taught: Situation::read_list(&r.get::<_, String>(10)?),
            author: r.get(11)?,
            written_in_run: r.get::<_, Option<i64>>(12)?.map(RunId),
            written_at: r.get(13)?,
            to_retell: r.get::<_, i64>(14)? != 0,
            migrated_from: r.get(15)?,
            verified_at: r.get(16)?,
            last_used_at: r.get(17)?,
            why: r.get(18)?,
        },
        scope,
        stage,
    ))
}

fn handover(r: &rusqlite::Row<'_>) -> rusqlite::Result<Handover> {
    Ok(Handover {
        id: r.get(0)?,
        task: TaskId(r.get(1)?),
        by: MemberId(r.get(2)?),
        from: r.get::<_, Option<i64>>(3)?.map(MemberId),
        to: r.get::<_, Option<i64>>(4)?.map(MemberId),
        run: r.get::<_, Option<i64>>(5)?.map(RunId),
        note: HandoverNote {
            done: r.get(6)?,
            not_done: r.get(7)?,
            next: r.get(8)?,
            watch_out: r.get(9)?,
        },
        evidence: Ref::read(&r.get::<_, String>(10)?),
        at: r.get(11)?,
    })
}

impl WorldStore for SqliteTeamStore {
    fn sections(&self) -> Stored<Vec<Section>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{SECTION_SELECT} ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], section_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(mut s, stage)| {
                s.stage = Stage::from_name(&stage).ok_or_else(|| corrupt("a section's stage"))?;
                Ok(s)
            })
            .collect()
    }

    fn save_section(&self, s: &Section, why: &str) -> Stored<i64> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let evidence = Snapshot::keep(&s.evidence);
        let run = s.written_in_run.map(|r| r.0);
        let id = if s.id == 0 {
            tx.execute(
                "INSERT INTO world_sections (page, anchor, heading, headline, body, stage, warrant,
                    evidence, verified_at, verified_by, superseded_by, why, written_by,
                    written_in_run, revision, updated_at, last_used_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    s.page,
                    s.anchor,
                    s.heading,
                    s.headline,
                    s.body,
                    s.stage.name(),
                    s.warrant,
                    evidence,
                    s.verified_at,
                    s.verified_by,
                    s.superseded_by,
                    s.why,
                    s.written_by,
                    run,
                    s.revision,
                    s.updated_at,
                    s.last_used_at
                ],
            )
            .map_err(backend)?;
            tx.last_insert_rowid()
        } else {
            tx.execute(
                "UPDATE world_sections SET heading = ?2, headline = ?3, body = ?4, stage = ?5,
                    warrant = ?6, evidence = ?7, verified_at = ?8, verified_by = ?9,
                    superseded_by = ?10, why = ?11, written_by = ?12, written_in_run = ?13,
                    revision = ?14, updated_at = ?15, last_used_at = ?16
                 WHERE id = ?1",
                params![
                    s.id,
                    s.heading,
                    s.headline,
                    s.body,
                    s.stage.name(),
                    s.warrant,
                    evidence,
                    s.verified_at,
                    s.verified_by,
                    s.superseded_by,
                    s.why,
                    s.written_by,
                    run,
                    s.revision,
                    s.updated_at,
                    s.last_used_at
                ],
            )
            .map_err(backend)?;
            s.id
        };
        tx.execute(
            "INSERT INTO world_revisions (section, revision, heading, headline, body, stage, warrant,
                evidence, by, run, at_ms, why)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                s.revision,
                s.heading,
                s.headline,
                s.body,
                s.stage.name(),
                s.warrant,
                evidence,
                s.written_by,
                run,
                s.updated_at,
                why
            ],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(id)
    }

    fn touch_section(&self, id: i64, at: Millis) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE world_sections SET last_used_at = ?2 WHERE id = ?1",
                params![id, at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn section_revisions(&self, section: i64) -> Stored<Vec<(i64, String)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT revision, headline FROM world_revisions WHERE section = ?1
                 ORDER BY revision, id",
            )
            .map_err(backend)?;
        stmt.query_map([section], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn timeline(&self, page: &str) -> Stored<Vec<TimelineLine>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT page, at_ms, by, line FROM world_timeline WHERE page = ?1 ORDER BY at_ms, id",
            )
            .map_err(backend)?;
        stmt.query_map([page], |r| {
            Ok(TimelineLine {
                page: r.get(0)?,
                at: r.get(1)?,
                by: r.get(2)?,
                line: r.get(3)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn timeline_with_runs(&self, page: &str) -> Stored<Vec<(TimelineLine, Option<RunId>)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT page, at_ms, by, line, run FROM world_timeline WHERE page = ?1
                 ORDER BY at_ms, id",
            )
            .map_err(backend)?;
        stmt.query_map([page], |r| {
            Ok((
                TimelineLine {
                    page: r.get(0)?,
                    at: r.get(1)?,
                    by: r.get(2)?,
                    line: r.get(3)?,
                },
                r.get::<_, Option<i64>>(4)?.map(RunId),
            ))
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn add_timeline(&self, line: &TimelineLine, run: Option<RunId>) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO world_timeline (page, at_ms, by, run, line) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![line.page, line.at, line.by, run.map(|r| r.0), line.line],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn stories(&self) -> Stored<Vec<Story>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("{STORY_SELECT} ORDER BY id"))
            .map_err(backend)?;
        let rows = stmt
            .query_map([], story_row)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)?;
        rows.into_iter()
            .map(|(mut s, scope, stage)| {
                s.scope = Scope::from_name(&scope).ok_or_else(|| corrupt("a story's scope"))?;
                s.stage = Stage::from_name(&stage).ok_or_else(|| corrupt("a story's stage"))?;
                Ok(s)
            })
            .collect()
    }

    fn save_story(&self, s: &Story) -> Stored<i64> {
        let conn = self.conn()?;
        let values = params![
            s.id,
            s.scope.name(),
            s.handle,
            s.moment,
            s.reflex,
            s.instead,
            s.because,
            Snapshot::keep(&s.evidence),
            s.stage.name(),
            s.warrant,
            Situation::keep_list(&s.taught),
            s.author,
            s.written_in_run.map(|r| r.0),
            s.written_at,
            s.to_retell,
            s.migrated_from,
            s.verified_at,
            s.last_used_at,
            s.why
        ];
        if s.id != 0 {
            conn.execute(
                "UPDATE stories SET scope = ?2, handle = ?3, moment = ?4, reflex = ?5,
                    instead = ?6, because = ?7, evidence = ?8, stage = ?9, warrant = ?10,
                    taught = ?11, author = ?12, written_in_run = ?13, written_at = ?14,
                    to_retell = ?15, migrated_from = ?16, verified_at = ?17, last_used_at = ?18,
                    why = ?19
                 WHERE id = ?1",
                values,
            )
            .map_err(backend)?;
            return Ok(s.id);
        }
        conn.execute(
            "INSERT INTO stories (scope, handle, moment, reflex, instead, because, evidence, stage,
                warrant, taught, author, written_in_run, written_at, to_retell, migrated_from,
                verified_at, last_used_at, why)
             SELECT ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                ?19 WHERE ?1 = 0",
            values,
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn touch_story(&self, id: i64, at: Millis) -> Stored<()> {
        self.conn()?
            .execute(
                "UPDATE stories SET last_used_at = ?2 WHERE id = ?1",
                params![id, at],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn add_handover(&self, h: &Handover) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO handovers (task, by_member, from_member, to_member, run, done, not_done,
                next, watch_out, evidence, at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                h.task.0,
                h.by.0,
                h.from.map(|m| m.0),
                h.to.map(|m| m.0),
                h.run.map(|r| r.0),
                h.note.done,
                h.note.not_done,
                h.note.next,
                h.note.watch_out,
                Ref::kept(&h.evidence),
                h.at
            ],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn handovers(&self, task: TaskId) -> Stored<Vec<Handover>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task, by_member, from_member, to_member, run, done, not_done, next,
                    watch_out, evidence, at_ms FROM handovers WHERE task = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        stmt.query_map([task.0], handover)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn latest_handover(&self, task: TaskId, member: MemberId) -> Stored<Option<Handover>> {
        let conn = self.conn()?;
        let addressed = conn
            .query_row(
                LATEST_RECIPIENT_HANDOVER,
                params![task.0, member.0],
                handover,
            )
            .optional()
            .map_err(backend)?;
        if addressed.is_some() {
            return Ok(addressed);
        }
        conn.query_row(LATEST_TASK_HANDOVER, [task.0], handover)
            .optional()
            .map_err(backend)
    }

    fn save_briefed(&self, b: &Briefed) -> Stored<()> {
        let list = |all: &[String]| serde_json::json!(all).to_string();
        self.conn()?
            .execute(
                "INSERT INTO run_briefings (run, situation, briefed, dropped, tokens, budget,
                    over_budget, at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(run) DO UPDATE SET situation = excluded.situation,
                    briefed = excluded.briefed, dropped = excluded.dropped,
                    tokens = excluded.tokens, budget = excluded.budget,
                    over_budget = excluded.over_budget, at_ms = excluded.at_ms",
                params![
                    b.run.0,
                    b.situation.name(),
                    list(&b.briefed),
                    list(&b.dropped),
                    i64::try_from(b.tokens).unwrap_or(i64::MAX),
                    i64::try_from(b.budget).unwrap_or(i64::MAX),
                    b.over_budget,
                    b.at
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn briefed(&self, run: RunId) -> Stored<Option<Briefed>> {
        let found = self
            .conn()?
            .query_row(
                "SELECT situation, briefed, dropped, tokens, budget, over_budget, at_ms
                 FROM run_briefings WHERE run = ?1",
                [run.0],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)? != 0,
                        r.get::<_, i64>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        let Some((situation, briefed, dropped, tokens, budget, over_budget, at)) = found else {
            return Ok(None);
        };
        let list = |kept: &str| serde_json::from_str::<Vec<String>>(kept).unwrap_or_default();
        Ok(Some(Briefed {
            run,
            situation: Situation::from_name(&situation)
                .ok_or_else(|| corrupt("a briefing's situation"))?,
            briefed: list(&briefed),
            dropped: list(&dropped),
            tokens: usize::try_from(tokens).unwrap_or(0),
            budget: usize::try_from(budget).unwrap_or(0),
            over_budget,
            at,
        }))
    }

    fn timeline_rows_by(&self, page: &str, by: &str) -> Stored<Vec<Row>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, page, at_ms, by, line FROM world_timeline
                 WHERE page = ?1 AND by = ?2 ORDER BY at_ms, id",
            )
            .map_err(backend)?;
        stmt.query_map(params![page, by], |r| {
            Ok((
                r.get(0)?,
                TimelineLine {
                    page: r.get(1)?,
                    at: r.get(2)?,
                    by: r.get(3)?,
                    line: r.get(4)?,
                },
            ))
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn repair_timeline(&self, fixes: &[Fix], at: Millis) -> Stored<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let done: bool = tx
            .query_row(
                "SELECT COUNT(*) > 0 FROM settings WHERE key = ?1",
                [REPAIRED],
                |r| r.get(0),
            )
            .map_err(backend)?;
        if done {
            return Ok(false);
        }
        if !fixes.is_empty() {
            tx.execute_batch("DROP TRIGGER IF EXISTS world_timeline_is_kept_update;")
                .map_err(backend)?;
        }
        for fix in fixes {
            tx.execute(
                "INSERT INTO world_timeline_repairs (row, old_at_ms, old_line, repaired_at)
                 SELECT id, at_ms, line, ?2 FROM world_timeline WHERE id = ?1",
                params![fix.id, at],
            )
            .map_err(backend)?;
            tx.execute(
                "UPDATE world_timeline SET at_ms = ?2, line = ?3 WHERE id = ?1",
                params![fix.id, fix.at, fix.line],
            )
            .map_err(backend)?;
        }
        if !fixes.is_empty() {
            tx.execute_batch(TIMELINE_KEPT).map_err(backend)?;
        }
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            params![REPAIRED, at.to_string()],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(true)
    }

    fn migrate_world(&self, plan: &Plan, at: Millis) -> Stored<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let done: bool = tx
            .query_row(
                "SELECT COUNT(*) > 0 FROM settings WHERE key = ?1",
                [MIGRATED],
                |r| r.get(0),
            )
            .map_err(backend)?;
        if done {
            return Ok(false);
        }
        for line in &plan.timeline {
            tx.execute(
                "INSERT INTO world_timeline (page, at_ms, by, line) VALUES (?1, ?2, ?3, ?4)",
                params![line.page, line.at, line.by, line.line],
            )
            .map_err(backend)?;
        }
        for lesson in &plan.lessons {
            tx.execute(
                "INSERT INTO stories (scope, handle, instead, stage, taught, author, written_at,
                    to_retell, migrated_from, last_used_at)
                 VALUES (?1, ?2, ?3, ?4, '', ?5, ?6, 1, ?7, ?6)
                 ON CONFLICT DO NOTHING",
                params![
                    lesson.scope.name(),
                    lesson.handle,
                    lesson.instead,
                    Stage::Observation.name(),
                    lesson.author,
                    lesson.at,
                    lesson.line
                ],
            )
            .map_err(backend)?;
        }
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            params![MIGRATED, at.to_string()],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::application::ports::TeamStore;
    use crate::domain::world_migration::Lesson;

    /// A store as it is live before the world: every table HQ had, and data in the old places.
    fn live_store() -> SqliteTeamStore {
        let conn =
            SqliteTeamStore::before_the_world(Connection::open_in_memory().unwrap()).unwrap();
        conn.execute_batch(
            "INSERT INTO members (id, name, role, share_percent, slots) VALUES
                (1, 'Ada', 'lead', 25, 2), (2, 'Dee', 'reviewer', 20, 2);
             INSERT INTO goals (id, title, reason, status) VALUES
                (1, 'Keep the home lab healthy', '', 'active');
             INSERT INTO goal_notes (goal, note) VALUES
                (1, '- Closed too soon: the alarm came back. (Dee, 11 Oct)');
             INSERT INTO settings (key, value) VALUES ('house_log', '- 17 Oct: The NAS has 16 GB.');
             INSERT INTO notes (member, text, evidence, written_at, last_used_at) VALUES
                (1, 'deluge needs the VPN', 'task 1', 1, 1);",
        )
        .unwrap();
        SqliteTeamStore::over(conn).unwrap()
    }

    fn the_plan() -> Plan {
        Plan {
            timeline: vec![TimelineLine {
                page: "house".to_owned(),
                at: 5,
                by: "the house log".to_owned(),
                line: "The NAS has 16 GB.".to_owned(),
            }],
            lessons: vec![Lesson {
                scope: Scope::Area(MemberId(1)),
                handle: "closed-too-soon".to_owned(),
                instead: "Closed too soon: the alarm came back.".to_owned(),
                author: "Dee".to_owned(),
                at: 4,
                line: "- Closed too soon: the alarm came back. (Dee, 11 Oct)".to_owned(),
            }],
        }
    }

    #[test]
    fn a_live_store_gains_the_worlds_tables_empty_and_keeps_everything_else() {
        let store = live_store();
        assert!(store.sections().unwrap().is_empty());
        assert!(store.stories().unwrap().is_empty());
        assert!(store.timeline("house").unwrap().is_empty());
        assert_eq!(store.members().unwrap().len(), 2);
        assert_eq!(
            store.setting("house_log").unwrap().as_deref(),
            Some("- 17 Oct: The NAS has 16 GB.")
        );
        assert_eq!(store.notes(MemberId(1)).unwrap().len(), 1);
        assert!(
            store
                .goal_note(crate::domain::GoalId(1))
                .unwrap()
                .contains("Closed too soon")
        );
    }

    #[test]
    fn the_move_is_applied_once_and_deletes_nothing() {
        let store = live_store();
        assert!(store.migrate_world(&the_plan(), 9).unwrap());
        assert!(
            !store.migrate_world(&the_plan(), 10).unwrap(),
            "a second start changes nothing"
        );
        assert_eq!(store.timeline("house").unwrap().len(), 1);
        let stories = store.stories().unwrap();
        assert_eq!(stories.len(), 1);
        assert!(stories[0].to_retell);
        assert_eq!(stories[0].stage, Stage::Observation);
        assert_eq!(stories[0].scope, Scope::Area(MemberId(1)));
        assert_eq!(stories[0].last_used_at, 4);
        // What it came from is all still there.
        assert_eq!(
            store.setting("house_log").unwrap().as_deref(),
            Some("- 17 Oct: The NAS has 16 GB.")
        );
        assert!(
            store
                .goal_note(crate::domain::GoalId(1))
                .unwrap()
                .contains("Closed too soon")
        );
        assert_eq!(store.notes(MemberId(1)).unwrap().len(), 1);
        assert_eq!(store.setting(MIGRATED).unwrap().as_deref(), Some("9"));
    }

    #[test]
    fn mended_rows_keep_what_they_said_and_the_timeline_stays_append_only() {
        let store = live_store();
        assert!(store.migrate_world(&the_plan(), 9).unwrap());
        let rows = store.timeline_rows_by("house", "the house log").unwrap();
        assert_eq!(rows.len(), 1);
        let fix = Fix {
            id: rows[0].0,
            at: 77,
            line: "Mended.".to_owned(),
        };
        assert!(store.repair_timeline(&[fix.clone()], 100).unwrap());
        assert!(!store.repair_timeline(&[fix], 101).unwrap(), "once");
        let now = store.timeline("house").unwrap();
        assert_eq!((now[0].at, now[0].line.as_str()), (77, "Mended."));
        let kept: (i64, String) = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT old_at_ms, old_line FROM world_timeline_repairs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kept, (rows[0].1.at, "The NAS has 16 GB.".to_owned()));
        assert!(
            store
                .conn()
                .unwrap()
                .execute("UPDATE world_timeline SET line = ''", [])
                .is_err(),
            "append-only again"
        );
    }

    #[test]
    fn revisions_timelines_and_handover_notes_are_kept_as_written() {
        let store = live_store();
        let mut section = Section {
            id: 0,
            page: "house".to_owned(),
            anchor: "backups".to_owned(),
            heading: "Backups".to_owned(),
            headline: "Backups run at 03:00.".to_owned(),
            body: String::new(),
            stage: Stage::Observation,
            warrant: String::new(),
            evidence: vec![Snapshot {
                cited: Ref::Task(TaskId(1)),
                said: "task 1".to_owned(),
            }],
            verified_at: Some(3),
            verified_by: "Mara".to_owned(),
            superseded_by: None,
            why: String::new(),
            written_by: "Mara".to_owned(),
            written_in_run: None,
            revision: 1,
            updated_at: 3,
            last_used_at: 3,
        };
        section.id = store.save_section(&section, "written").unwrap();
        section.stage = Stage::Canon;
        section.revision = 2;
        store
            .save_section(&section, "made canon by the owner")
            .unwrap();
        let read = store.sections().unwrap();
        assert_eq!(read, vec![section.clone()]);
        // A second section at the same anchor of the same page is refused.
        let mut twin = section.clone();
        twin.id = 0;
        assert!(store.save_section(&twin, "again").is_err());
        let conn = store.conn().unwrap();
        let revisions: i64 = conn
            .query_row("SELECT COUNT(*) FROM world_revisions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(revisions, 2);
        assert!(
            conn.execute("UPDATE world_revisions SET why = 'x'", [])
                .is_err()
        );
        assert!(conn.execute("DELETE FROM world_revisions", []).is_err());
    }

    #[test]
    fn latest_handover_prefers_the_recipient_and_bounds_retained_history_reads() {
        use rusqlite::StatementStatus;
        let store = live_store();
        let conn = store.conn().unwrap();
        conn.execute_batch(
            "INSERT INTO tasks(id,goal,title,description,status,priority,depth)
            VALUES(1,1,'Fixture','','doing','normal',0);
            INSERT INTO handovers(id,task,by_member,to_member,done,not_done,next,watch_out,at_ms)
            VALUES(1,1,1,2,'Original','Pending','Check','Care',0);",
        )
        .unwrap();
        let mut samples = Vec::new();
        for size in [1_000, 100_000] {
            let previous = if size == 1_000 { 1 } else { 1_001 };
            conn.execute("WITH RECURSIVE n(id) AS (VALUES(?1+1) UNION ALL SELECT id+1 FROM n WHERE id<?2+1)
                INSERT INTO handovers(id,task,by_member,to_member,done,not_done,next,watch_out,at_ms)
                SELECT id,1,1,1,'Fixture','Pending','Check','Care',0 FROM n", params![previous,size]).unwrap();
            let mut sample = Vec::new();
            for (sql, parameters) in [
                (LATEST_RECIPIENT_HANDOVER, vec![1, 2]),
                (LATEST_RECIPIENT_HANDOVER, vec![1, 3]),
                (LATEST_TASK_HANDOVER, vec![1]),
            ] {
                let mut query = conn.prepare(sql).unwrap();
                let _ = query
                    .query_row(rusqlite::params_from_iter(parameters), handover)
                    .optional()
                    .unwrap();
                sample.push(query.get_status(StatementStatus::VmStep));
            }
            samples.push(sample);
        }
        assert_eq!(samples[0], samples[1]);
        assert!(samples[1].iter().all(|steps| *steps < 100));
        drop(conn);
        assert_eq!(
            store
                .latest_handover(TaskId(1), MemberId(2))
                .unwrap()
                .unwrap()
                .id,
            1
        );
        assert_eq!(
            store
                .latest_handover(TaskId(1), MemberId(3))
                .unwrap()
                .unwrap()
                .id,
            100_001
        );
        assert!(
            store
                .latest_handover(TaskId(2), MemberId(2))
                .unwrap()
                .is_none()
        );
        let conn = store.conn().unwrap();
        conn.execute_batch("DROP INDEX handovers_for_member;")
            .unwrap();
        let mut query = conn.prepare(LATEST_RECIPIENT_HANDOVER).unwrap();
        assert!(
            query
                .query_row([1, 3], handover)
                .optional()
                .unwrap()
                .is_none()
        );
        assert!(
            query.get_status(StatementStatus::VmStep) > 100_000,
            "negative control must scan the task's retained notes"
        );
    }

    #[test]
    fn a_handover_and_a_briefing_are_read_back_as_kept() {
        let store = live_store();
        store
            .conn()
            .unwrap()
            .execute_batch(
                "INSERT INTO tasks (id, goal, title, description, status, priority, depth)
                 VALUES (7, 1, 'Fix the proxy', '', 'todo', 'normal', 0);",
            )
            .unwrap();
        let handover = Handover {
            id: 0,
            task: TaskId(7),
            by: MemberId(1),
            from: Some(MemberId(2)),
            to: Some(MemberId(1)),
            run: None,
            note: HandoverNote::new("a", "b", "c", "d").unwrap(),
            evidence: vec![Ref::Task(TaskId(7))],
            at: 8,
        };
        let id = store.add_handover(&handover).unwrap();
        assert_eq!(
            store.handovers(TaskId(7)).unwrap(),
            vec![Handover { id, ..handover }]
        );
        assert!(
            store
                .conn()
                .unwrap()
                .execute("DELETE FROM handovers", [])
                .is_err()
        );
        store
            .conn()
            .unwrap()
            .execute_batch(
                "INSERT INTO wakes (id, member, task, reason, receipt, at_ms)
                    VALUES (1, 1, 7, 'assigned', 'started', 1);
                 INSERT INTO runs (id, member, task, wake, status, attempts, queued_at)
                    VALUES (3, 1, 7, 1, 'running', 1, 1);",
            )
            .unwrap();
        let mut briefed = Briefed {
            run: RunId(3),
            situation: Situation::Handover,
            briefed: vec!["handover".to_owned()],
            dropped: vec!["story:closed-too-soon".to_owned()],
            tokens: 900,
            budget: 3_000,
            over_budget: false,
            at: 9,
        };
        assert_eq!(store.briefed(RunId(3)).unwrap(), None);
        store.save_briefed(&briefed).unwrap();
        briefed.situation = Situation::Continuing;
        store.save_briefed(&briefed).unwrap();
        assert_eq!(
            store.briefed(RunId(3)).unwrap(),
            Some(briefed),
            "the latest attempt's"
        );
        store
            .add_timeline(
                &TimelineLine {
                    page: "house".to_owned(),
                    at: 1,
                    by: "Mara".to_owned(),
                    line: "x".to_owned(),
                },
                None,
            )
            .unwrap();
        assert!(
            store
                .conn()
                .unwrap()
                .execute("UPDATE world_timeline SET line = ''", [])
                .is_err()
        );
    }
}
