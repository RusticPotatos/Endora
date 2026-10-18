//! Searching HQ's record (ADR 0129). Each kind of record is asked for the passages that hold
//! any of the question's words, newest first, within the days a search looks back: posts (a turn's
//! last words among them), handover notes, world sections and timelines, stories, notebooks, and
//! the conversations of turns, which are searched through the full-text index where the SQLite
//! has one (the bundled one does) and by a bounded scan where it does not. The choosing, the
//! ranking and the tier rule are the domain's; this only finds.

use endora_kernel::tier::Tier;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use super::{SqliteTeamStore, backend};
use crate::application::ports::Stored;
use crate::domain::history::{tier_from_name, tier_of};
use crate::domain::record::{PER_SOURCE, Passage, Query, Source, fts_query};
use crate::domain::{RunId, TaskId};

/// `column LIKE ?` for any of the words, as one condition; the words are letters and digits only.
fn like_any(column: &str, terms: &[String], args: &mut Vec<Value>) -> String {
    let each: Vec<String> = terms
        .iter()
        .map(|t| {
            args.push(Value::Text(format!("%{t}%")));
            format!("{column} LIKE ?")
        })
        .collect();
    format!("({})", each.join(" OR "))
}

/// A turn read Confidential data: the node marked one of its receipts so, for good.
fn read_secrets(run: &str) -> String {
    format!("EXISTS (SELECT 1 FROM call_receipts c WHERE c.run = {run} AND c.confidential = 1)")
}

/// What a question about one task, or one project's tasks, adds to a search of `task` column.
fn about(column: &str, q: &Query, args: &mut Vec<Value>) -> String {
    let mut sql = String::new();
    if let Some(task) = q.task {
        args.push(Value::Integer(task.0));
        sql.push_str(&format!(" AND {column} = ?"));
    }
    if let Some(project) = q.project {
        args.push(Value::Integer(project));
        sql.push_str(&format!(
            " AND {column} IN (SELECT id FROM tasks WHERE project = ?)"
        ));
    }
    sql
}

fn run_of(conn: &Connection, sql: &str, args: &[Value]) -> Stored<Vec<Vec<Value>>> {
    let mut stmt = conn.prepare(sql).map_err(backend)?;
    let width = stmt.column_count();
    let rows = stmt
        .query_map(params_from_iter(args.iter()), |r| {
            (0..width).map(|i| r.get::<_, Value>(i)).collect()
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<Vec<Value>>>>()
        .map_err(backend)?;
    Ok(rows)
}

fn text(v: &Value) -> String {
    match v {
        Value::Text(s) => s.clone(),
        Value::Integer(n) => n.to_string(),
        _ => String::new(),
    }
}

fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Integer(n) => Some(*n),
        _ => None,
    }
}

fn tier(v: &Value) -> Tier {
    tier_of(int(v) == Some(1))
}

impl SqliteTeamStore {
    /// The passages of every kind of record that hold any of the query's words, newest first
    /// from each, [`PER_SOURCE`] at most from each.
    pub(super) fn search(&self, q: &Query) -> Stored<Vec<Passage>> {
        if q.terms.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let mut found = Vec::new();
        found.extend(posts(&conn, q)?);
        found.extend(handovers(&conn, q)?);
        found.extend(notes(&conn, q)?);
        found.extend(conversations(&conn, q)?);
        // What is not about a task or a project, or one member's: the pages and the stories.
        if q.task.is_none() && q.project.is_none() {
            found.extend(sections(&conn, q)?);
            found.extend(timelines(&conn, q)?);
            found.extend(stories(&conn, q)?);
        }
        Ok(found)
    }
}

fn limit(args: &mut Vec<Value>) {
    args.push(Value::Integer(i64::try_from(PER_SOURCE).unwrap_or(60)));
}

fn posts(conn: &Connection, q: &Query) -> Stored<Vec<Passage>> {
    let mut args = vec![Value::Integer(q.since)];
    let mut sql = format!(
        "SELECT p.id, p.task, p.author, p.text, p.at_ms,
            EXISTS (SELECT 1 FROM runs r JOIN members m ON m.id = r.member
                    JOIN call_receipts c ON c.run = r.id
                    WHERE m.name = p.author AND r.task = p.task AND c.confidential = 1
                      AND r.started_at <= p.at_ms AND p.at_ms <= COALESCE(r.ended_at, p.at_ms))
         FROM posts p WHERE p.at_ms >= ? AND {}",
        like_any("p.text", &q.terms, &mut args)
    );
    if let Some(member) = &q.member {
        args.push(Value::Text(member.clone()));
        sql.push_str(" AND p.author = ?");
    }
    sql.push_str(&about("p.task", q, &mut args));
    sql.push_str(" ORDER BY p.at_ms DESC LIMIT ?");
    limit(&mut args);
    Ok(run_of(conn, &sql, &args)?
        .iter()
        .map(|r| Passage {
            source: Source::Post,
            locator: format!("post {}", text(&r[0])),
            by: text(&r[2]),
            at: int(&r[4]).unwrap_or(0),
            task: int(&r[1]).map(TaskId),
            run: None,
            tier: tier(&r[5]),
            indexed: false,
            text: text(&r[3]),
        })
        .collect())
}

fn handovers(conn: &Connection, q: &Query) -> Stored<Vec<Passage>> {
    let body = "(h.done || ' ' || h.not_done || ' ' || h.next || ' ' || h.watch_out)";
    let mut args = vec![Value::Integer(q.since)];
    let mut sql = format!(
        "SELECT h.id, h.task, mb.name, {body}, h.at_ms, h.run, {}
         FROM handovers h JOIN members mb ON mb.id = h.by_member
         WHERE h.at_ms >= ? AND {}",
        read_secrets("h.run"),
        like_any(body, &q.terms, &mut args)
    );
    if let Some(member) = &q.member {
        args.push(Value::Text(member.clone()));
        args.push(Value::Text(member.clone()));
        sql.push_str(
            " AND (mb.name = ? OR h.from_member IN (SELECT id FROM members WHERE name = ?))",
        );
    }
    sql.push_str(&about("h.task", q, &mut args));
    sql.push_str(" ORDER BY h.at_ms DESC LIMIT ?");
    limit(&mut args);
    Ok(run_of(conn, &sql, &args)?
        .iter()
        .map(|r| Passage {
            source: Source::Handover,
            locator: format!("handover {}", text(&r[0])),
            by: text(&r[2]),
            at: int(&r[4]).unwrap_or(0),
            task: int(&r[1]).map(TaskId),
            run: int(&r[5]).map(RunId),
            tier: tier(&r[6]),
            indexed: false,
            text: text(&r[3]),
        })
        .collect())
}

fn notes(conn: &Connection, q: &Query) -> Stored<Vec<Passage>> {
    let mut args = vec![Value::Integer(q.since)];
    let task = "(SELECT r.task FROM runs r WHERE r.id = n.written_in_run)";
    let mut sql = format!(
        "SELECT n.id, {task}, m.name, n.text, n.written_at, n.written_in_run, {}, n.from_owner
         FROM notes n JOIN members m ON m.id = n.member
         WHERE n.written_at >= ? AND {}",
        read_secrets("n.written_in_run"),
        like_any("n.text", &q.terms, &mut args)
    );
    if let Some(member) = &q.member {
        args.push(Value::Text(member.clone()));
        sql.push_str(" AND m.name = ?");
    }
    sql.push_str(&about(task, q, &mut args));
    sql.push_str(" ORDER BY n.written_at DESC LIMIT ?");
    limit(&mut args);
    Ok(run_of(conn, &sql, &args)?
        .iter()
        .map(|r| Passage {
            source: Source::Note,
            locator: format!("n{} in the notebook of {}", text(&r[0]), text(&r[2])),
            by: if int(&r[7]) == Some(1) {
                "the owner".to_owned()
            } else {
                text(&r[2])
            },
            at: int(&r[4]).unwrap_or(0),
            task: int(&r[1]).map(TaskId),
            run: int(&r[5]).map(RunId),
            tier: tier(&r[6]),
            indexed: false,
            text: text(&r[3]),
        })
        .collect())
}

fn sections(conn: &Connection, q: &Query) -> Stored<Vec<Passage>> {
    let body = "(s.heading || ' ' || s.headline || ' ' || s.body)";
    let mut args = vec![Value::Integer(q.since)];
    let mut sql = format!(
        "SELECT s.page, s.anchor, s.written_by, {body}, s.updated_at, s.written_in_run, {}
         FROM world_sections s WHERE s.updated_at >= ? AND {}",
        read_secrets("s.written_in_run"),
        like_any(body, &q.terms, &mut args)
    );
    if let Some(member) = &q.member {
        args.push(Value::Text(member.clone()));
        sql.push_str(" AND s.written_by = ?");
    }
    sql.push_str(" ORDER BY s.updated_at DESC LIMIT ?");
    limit(&mut args);
    Ok(run_of(conn, &sql, &args)?
        .iter()
        .map(|r| Passage {
            source: Source::Section,
            locator: format!("world:{}#{}", text(&r[0]), text(&r[1])),
            by: text(&r[2]),
            at: int(&r[4]).unwrap_or(0),
            task: None,
            run: int(&r[5]).map(RunId),
            tier: tier(&r[6]),
            indexed: false,
            text: text(&r[3]),
        })
        .collect())
}

fn timelines(conn: &Connection, q: &Query) -> Stored<Vec<Passage>> {
    let mut args = vec![Value::Integer(q.since)];
    let mut sql = format!(
        "SELECT t.id, t.page, t.by, t.line, t.at_ms, t.run, {}
         FROM world_timeline t WHERE t.at_ms >= ? AND {}",
        read_secrets("t.run"),
        like_any("t.line", &q.terms, &mut args)
    );
    if let Some(member) = &q.member {
        args.push(Value::Text(member.clone()));
        sql.push_str(" AND t.by = ?");
    }
    sql.push_str(" ORDER BY t.at_ms DESC LIMIT ?");
    limit(&mut args);
    Ok(run_of(conn, &sql, &args)?
        .iter()
        .map(|r| Passage {
            source: Source::Timeline,
            locator: format!("world:{} timeline line {}", text(&r[1]), text(&r[0])),
            by: text(&r[2]),
            at: int(&r[4]).unwrap_or(0),
            task: None,
            run: int(&r[5]).map(RunId),
            tier: tier(&r[6]),
            indexed: false,
            text: text(&r[3]),
        })
        .collect())
}

fn stories(conn: &Connection, q: &Query) -> Stored<Vec<Passage>> {
    let body =
        "(s.handle || ' ' || s.moment || ' ' || s.reflex || ' ' || s.instead || ' ' || s.because)";
    let mut args = vec![Value::Integer(q.since)];
    let mut sql = format!(
        "SELECT s.handle, s.author, {body}, s.written_at, s.written_in_run, {}
         FROM stories s WHERE s.written_at >= ? AND {}",
        read_secrets("s.written_in_run"),
        like_any(body, &q.terms, &mut args)
    );
    if let Some(member) = &q.member {
        args.push(Value::Text(member.clone()));
        sql.push_str(" AND s.author = ?");
    }
    sql.push_str(" ORDER BY s.written_at DESC LIMIT ?");
    limit(&mut args);
    Ok(run_of(conn, &sql, &args)?
        .iter()
        .map(|r| Passage {
            source: Source::Story,
            locator: format!("story:{}", text(&r[0])),
            by: text(&r[1]),
            at: int(&r[3]).unwrap_or(0),
            task: None,
            run: int(&r[4]).map(RunId),
            tier: tier(&r[5]),
            indexed: false,
            text: text(&r[2]),
        })
        .collect())
}

/// Whether the full-text index over the conversations exists in this store.
fn has_index(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name = 'transcripts_fts'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .is_ok_and(|n| n == 1)
}

fn conversations(conn: &Connection, q: &Query) -> Stored<Vec<Passage>> {
    let mut args: Vec<Value> = Vec::new();
    let mut sql;
    if has_index(conn) {
        args.push(Value::Text(fts_query(&q.terms)));
        args.push(Value::Integer(q.since));
        sql = "SELECT t.run, t.task, mb.name,
                      snippet(transcripts_fts, 0, '', '', ' … ', 40), t.at_ms, t.tier
               FROM transcripts_fts JOIN transcripts t ON t.run = transcripts_fts.rowid
                    JOIN members mb ON mb.id = t.member
               WHERE transcripts_fts MATCH ? AND t.at_ms >= ?"
            .to_owned();
    } else {
        // No index: a bounded scan, shown around the first word.
        args.push(Value::Text(q.terms[0].clone()));
        args.push(Value::Integer(q.since));
        sql = format!(
            "SELECT t.run, t.task, mb.name,
                    substr(t.body, max(1, instr(lower(t.body), ?) - 150), 400), t.at_ms, t.tier
             FROM transcripts t JOIN members mb ON mb.id = t.member
             WHERE t.at_ms >= ? AND {}",
            like_any("t.body", &q.terms, &mut args)
        );
    }
    if let Some(member) = &q.member {
        args.push(Value::Text(member.clone()));
        sql.push_str(" AND mb.name = ?");
    }
    sql.push_str(&about("t.task", q, &mut args));
    sql.push_str(" ORDER BY t.at_ms DESC LIMIT ?");
    limit(&mut args);
    Ok(run_of(conn, &sql, &args)?
        .iter()
        .map(|r| Passage {
            source: Source::Conversation,
            locator: format!("turn R{}", text(&r[0])),
            by: text(&r[2]),
            at: int(&r[4]).unwrap_or(0),
            task: int(&r[1]).map(TaskId),
            run: int(&r[0]).map(RunId),
            tier: tier_from_name(&text(&r[5])),
            indexed: true,
            text: text(&r[3]),
        })
        .collect())
}
