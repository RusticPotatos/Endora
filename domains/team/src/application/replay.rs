//! Replay: one turn read back as everything that happened in it, in order, for the owner's
//! screen. Pure: it reads the store's rows it is handed and writes nothing.
//!
//! What belongs to a turn is taken from the record by its run id wherever the record keeps
//! one: the wake that started it and the wakes folded into it, its call receipts, the evidence
//! it cited and its ledger rows. Events name their turn too (`"run"` in what they carry) from
//! the moment [`TAGGED_FROM`] records; a post is the one its turn's `thread.posted` event
//! names. A turn from before then is matched by time: the events about its member, or about its
//! task, between when it was queued and when it ended, and its member's posts in its task's
//! thread then. Each moment matched that way says so (`by_time`), as does the turn.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::ports::{Cited, LedgerRow, Run, ThreadPost, Wake};
use crate::domain::{CallReceipt, Event, Label, Member, Micros, Millis, Ref, TaskId, WakeReason};

/// The setting that keeps when the engine began naming each event's turn: events from before
/// it never name one, so a turn queued before it is matched by time.
pub const TAGGED_FROM: &str = "events_name_their_turn_from";

/// The events a turn's other rows already show, or that are noise beside them: a receipt's
/// event (the receipt is shown), a post's (the post is), and each report of spend (the money is
/// summed).
const SHOWN_OTHERWISE: [&str; 3] = ["receipt.recorded", "thread.posted", "budget.changed"];

/// The events a turn from before [`TAGGED_FROM`] may have caused, matched by time.
const MATCHED_BY_TIME: [&str; 11] = [
    "task.claimed",
    "task.released",
    "task.changed",
    "task.created",
    "desk.added",
    "checkback.set",
    "lesson.noted",
    "gate.result",
    "branch.submitted",
    "project.changed",
    "house.noted",
];

/// What HQ kept that a turn's replay is read from: any rows, its own among them.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    /// The team.
    pub members: &'a [Member],
    /// The wakes.
    pub wakes: &'a [Wake],
    /// Call receipts.
    pub receipts: &'a [CallReceipt],
    /// Posts, with what they cited, in the threads the turn may have written in.
    pub posts: &'a [ThreadPost],
    /// Evidence cited.
    pub cited: &'a [Cited],
    /// The event feed, as far back as HQ keeps it.
    pub events: &'a [Event],
    /// Ledger rows.
    pub ledger: &'a [LedgerRow],
    /// When events began naming their turn, if they have.
    pub tagged_from: Option<Millis>,
    /// Now: the end of a turn still running.
    pub now: Millis,
}

/// What a turn held, used, was charged and had released, from its ledger rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Money {
    /// Reserved before it started, across attempts.
    pub reserved: Micros,
    /// Use it reported while it ran.
    pub used: Micros,
    /// Charged when it ended.
    pub charged: Micros,
    /// Released when it ended.
    pub released: Micros,
}

/// One thing that happened in a turn.
#[derive(Debug, Clone, PartialEq)]
pub enum What {
    /// The wake that started it.
    Woken(WakeReason),
    /// A wake folded into it while it ran.
    Joined(WakeReason),
    /// What it was told at the start of its latest attempt.
    Briefing(String),
    /// A call it made of Endora's tools.
    Call(CallReceipt),
    /// A post in a thread: its own, or Endora's about what it did.
    Posted(ThreadPost),
    /// Evidence it cited for a criterion.
    Cited(Cited),
    /// An event of the feed: a start or an end, a claim, a status change, a desk item, a wake
    /// it raised…
    Event(Event),
    /// How it ended, from its row, when the feed no longer has its end.
    Ended,
}

/// One moment of a turn.
#[derive(Debug, Clone, PartialEq)]
pub struct Moment {
    /// When.
    pub at: Millis,
    /// What.
    pub what: What,
    /// Whether it was matched to the turn by its member, its task and the time, not by its run.
    pub by_time: bool,
}

/// A turn, read back.
#[derive(Debug, Clone, PartialEq)]
pub struct RunReplay {
    /// The turn's row.
    pub run: Run,
    /// Its member, if still on the team.
    pub member: Option<Member>,
    /// Why it was woken.
    pub reason: Option<WakeReason>,
    /// Its money, from the ledger.
    pub money: Money,
    /// What happened, oldest first.
    pub moments: Vec<Moment>,
    /// Whether any moment was matched by time: a turn from before events named their turn.
    pub by_time: bool,
}

/// The order of moments at the same millisecond: woken, started, told, the rest, ended.
fn rank(what: &What) -> u8 {
    match what {
        What::Woken(_) => 0,
        What::Event(e) if e.kind == "run.started" => 1,
        What::Briefing(_) => 2,
        What::Event(e) if e.kind == "run.ended" => 8,
        What::Ended => 9,
        _ => 5,
    }
}

fn number(data: &Value, key: &str) -> Option<i64> {
    data.get(key).and_then(Value::as_i64)
}

/// Whether an event from before [`TAGGED_FROM`] reads as this turn's: of a kind a turn causes,
/// in its time, and about its member (by id, or by name for the house log), or, naming no
/// member, about its task.
fn matched_by_time(e: &Event, run: &Run, name: Option<&str>, until: Millis) -> bool {
    if !MATCHED_BY_TIME.contains(&e.kind.as_str()) || e.at < run.queued_at || e.at > until {
        return false;
    }
    if let Some(member) = number(&e.data, "member") {
        return member == run.member.0;
    }
    if let Some(by) = e.data.get("by").and_then(Value::as_str) {
        return name == Some(by);
    }
    match (number(&e.data, "task"), run.task) {
        (Some(task), Some(own)) => task == own.0,
        _ => false,
    }
}

/// A turn read back from the record.
#[must_use]
pub fn replay(record: &Record<'_>, run: &Run) -> RunReplay {
    let member = record.members.iter().find(|m| m.id == run.member).cloned();
    let name = member.as_ref().map(|m| m.name.as_str());
    let until = run.ended_at.unwrap_or(record.now);
    let tagged = record.tagged_from.is_some_and(|from| run.queued_at >= from);
    let mut moments = Vec::new();
    let mut add =
        |at: Millis, what: What, by_time: bool| moments.push(Moment { at, what, by_time });

    let wake = record.wakes.iter().find(|w| w.id == run.wake);
    if let Some(w) = wake {
        add(w.at, What::Woken(w.reason), false);
    }
    for w in record
        .wakes
        .iter()
        .filter(|w| w.run == Some(run.id) && w.id != run.wake)
    {
        add(w.at, What::Joined(w.reason), false);
    }
    if !run.told.is_empty() {
        add(
            run.started_at.unwrap_or(run.queued_at),
            What::Briefing(run.told.clone()),
            false,
        );
    }
    for r in record.receipts.iter().filter(|r| r.run == run.id) {
        add(r.at, What::Call(r.clone()), false);
    }
    for c in record.cited.iter().filter(|c| c.run == run.id) {
        add(c.at, What::Cited(c.clone()), false);
    }

    // The events: its own by run id; an older turn's also by time.
    let mut ended = false;
    let mut posted: Vec<&Event> = Vec::new();
    for e in record.events {
        let own = match number(&e.data, "run") {
            Some(id) => id == run.id.0,
            None => !tagged && matched_by_time(e, run, name, until),
        };
        if !own {
            continue;
        }
        if e.kind == "thread.posted" {
            posted.push(e);
        }
        if SHOWN_OTHERWISE.contains(&e.kind.as_str()) {
            continue;
        }
        ended |= run.ended_at.is_some()
            && e.kind == "run.ended"
            && e.data.get("result").and_then(Value::as_str) == Some(run.status.name());
        add(
            e.at,
            What::Event(e.clone()),
            number(&e.data, "run").is_none(),
        );
    }

    // The posts: each the one its event names (the latest by its author in that thread, at or
    // before the event); an older turn's, its member's in its task's thread in its time.
    let mut used: HashSet<usize> = HashSet::new();
    for e in posted {
        let task = number(&e.data, "task");
        let author = e.data.get("author").and_then(Value::as_str);
        let found = record
            .posts
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                !used.contains(i)
                    && Some(p.task.0) == task
                    && Some(p.post.author.as_str()) == author
                    && p.post.at >= run.queued_at
                    && p.post.at <= e.at
            })
            .max_by_key(|(_, p)| p.post.at);
        if let Some((i, p)) = found {
            used.insert(i);
            add(
                p.post.at,
                What::Posted(p.clone()),
                number(&e.data, "run").is_none(),
            );
        }
    }
    if !tagged {
        for (i, p) in record.posts.iter().enumerate() {
            let theirs = Some(p.post.author.as_str()) == name && Some(p.task) == run.task;
            if used.contains(&i) || !theirs || p.post.at < run.queued_at || p.post.at > until {
                continue;
            }
            add(p.post.at, What::Posted(p.clone()), true);
        }
    }
    if let (Some(at), false) = (run.ended_at, ended) {
        add(at, What::Ended, false);
    }
    moments.sort_by_key(|m| (m.at, rank(&m.what)));

    let mut money = Money::default();
    for row in record.ledger.iter().filter(|l| l.run == run.id) {
        match row.kind.as_str() {
            "reserve" => money.reserved += row.amount,
            "use" => money.used += row.amount,
            "charge" => money.charged += row.amount,
            "release" => money.released += row.amount,
            _ => {}
        }
    }
    let by_time = moments.iter().any(|m| m.by_time);
    RunReplay {
        run: run.clone(),
        member,
        reason: wake.map(|w| w.reason),
        money,
        moments,
        by_time,
    }
}

/// A task's turns, read back oldest first.
#[must_use]
pub fn replay_all(record: &Record<'_>, runs: &[Run]) -> Vec<RunReplay> {
    let mut runs: Vec<&Run> = runs.iter().collect();
    runs.sort_by_key(|r| (r.queued_at, r.id));
    runs.into_iter().map(|r| replay(record, r)).collect()
}

/// The ids of the tasks a turn may have written in: its own, and every task its events name.
#[must_use]
pub fn tasks_touched(run: &Run, events: &[Event]) -> Vec<TaskId> {
    let mut tasks: Vec<TaskId> = run.task.into_iter().collect();
    for e in events {
        if number(&e.data, "run") != Some(run.id.0) {
            continue;
        }
        if let Some(task) = number(&e.data, "task").map(TaskId) {
            if !tasks.contains(&task) {
                tasks.push(task);
            }
        }
    }
    tasks
}

fn refs(all: &[Ref]) -> Vec<String> {
    all.iter().map(|r| r.said()).collect()
}

impl Moment {
    /// As the owner's screen reads it. Everything a model or a tool wrote is in it as it was
    /// written: the screen shows it as text. A Confidential excerpt is in it too: this is the
    /// owner's own screen (ADR 0114).
    #[must_use]
    pub fn shown(&self, run: &Run) -> Value {
        let mut shown = match &self.what {
            What::Woken(reason) => json!({ "kind": "woken", "reason": reason.name() }),
            What::Joined(reason) => json!({ "kind": "joined", "reason": reason.name() }),
            What::Briefing(text) => json!({ "kind": "briefing", "text": text }),
            What::Call(r) => json!({ "kind": "call", "receipt": r.said(), "tool": r.tool,
                "args": r.args, "excerpt": r.excerpt, "ok": r.ok, "act": r.act,
                "confidential": r.confidential, "task": r.task.map(|t| t.0) }),
            What::Posted(p) => json!({ "kind": "post", "task": p.task.0, "author": p.post.author,
                "text": p.post.text, "evidence": refs(&p.evidence),
                "unsupported": p.post.unsupported }),
            What::Cited(c) => json!({ "kind": "cited", "task": c.task.0, "stage": c.stage,
                "criterion": c.criterion, "refs": refs(&c.refs) }),
            What::Event(e) => json!({ "kind": "event", "event": e.id, "name": e.kind,
                "data": e.data }),
            What::Ended => json!({ "kind": "ended", "status": run.status.name(),
                "reason": run.end_reason, "label": run.label.map(Label::name) }),
        };
        shown["at"] = json!(self.at);
        shown["by_time"] = json!(self.by_time);
        shown
    }
}

impl RunReplay {
    /// As the owner's screen reads it.
    #[must_use]
    pub fn shown(&self) -> Value {
        let r = &self.run;
        json!({
            "run": r.id.0, "task": r.task.map(|t| t.0), "status": r.status.name(),
            "live": r.status.live(), "attempts": r.attempts,
            "queued_at": r.queued_at, "started_at": r.started_at, "ended_at": r.ended_at,
            "reason": self.reason.map(WakeReason::name),
            "end_reason": r.end_reason, "label": r.label.map(Label::name),
            "member": self.member.as_ref().map(|m| json!({ "id": m.id.0, "name": m.name,
                "role": m.role.name(), "look": m.look, "brain": m.brain.name(),
                "brain_label": m.brain.label() })),
            "money": { "reserved": self.money.reserved, "used": self.money.used,
                "charged": self.money.charged, "released": self.money.released,
                "spent": r.spent, "held": r.reserved },
            "tokens": { "input": r.input_tokens, "output": r.output_tokens,
                "cache_read": r.cache_read_tokens, "cache_write": r.cache_write_tokens },
            "by_time": self.by_time,
            "moments": self.moments.iter().map(|m| m.shown(r)).collect::<Vec<_>>(),
        })
    }
}
