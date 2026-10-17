//! How the team is doing: the numbers that say whether its work is good, computed from HQ's own
//! record and nothing else (no model's word), for the screen's "How the team is doing" card.
//! Pure: it reads the store's rows it is handed and writes nothing. Two windows are scored,
//! today and the last seven days, by the owner's day boundaries, as the rest of the screen's
//! money is.
//!
//! What cannot be known is not guessed: a turn from before labels (ADR 0114) is *unlabelled*,
//! not productive; a window that starts before the oldest event HQ still keeps says so
//! (`since`), because the node prunes the feed and anything older than what it kept cannot be
//! counted.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::ports::{DeskItem, Run};
use crate::domain::money::{self, DAY_MS, Micros};
use crate::domain::{Event, MemberId, Millis, Task, TaskId, TaskStatus};

/// A span of time, `from` inclusive to `until` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Its first moment.
    pub from: Millis,
    /// The first moment after it.
    pub until: Millis,
}

impl Window {
    /// One day, as the owner's clock counts them ([`money::day_of`]).
    #[must_use]
    pub const fn day(day: i64, offset_minutes: i64) -> Self {
        let from = money::day_start(day, offset_minutes);
        Self {
            from,
            until: from + DAY_MS,
        }
    }

    /// The `days` days ending with `day`, that day included.
    #[must_use]
    pub const fn days_ending(day: i64, days: i64, offset_minutes: i64) -> Self {
        let last = Self::day(day, offset_minutes);
        Self {
            from: last.from - (days - 1) * DAY_MS,
            until: last.until,
        }
    }

    const fn holds(&self, at: Millis) -> bool {
        self.from <= at && at < self.until
    }
}

/// HQ's record, as the store keeps it: every run, task, desk item and event.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    /// Every turn.
    pub runs: &'a [Run],
    /// Every task.
    pub tasks: &'a [Task],
    /// Every desk item, waiting or decided.
    pub desk: &'a [DeskItem],
    /// Every event kept, oldest first.
    pub events: &'a [Event],
}

/// One member's turns in a window, and what they cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberTurns {
    /// Whose.
    pub member: MemberId,
    /// How many ended in the window.
    pub turns: usize,
    /// How many of them showed nothing: talk only, or empty.
    pub nothing: usize,
    /// How many are from before labels, so cannot be told.
    pub unlabelled: usize,
    /// What they spent, in micros.
    pub spent: Micros,
}

/// The turns that ended in a window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Turns {
    /// How many.
    pub total: usize,
    /// How many showed nothing (talk only, or empty): the main waste number.
    pub nothing: usize,
    /// How many have no label (turns from before ADR 0114): counted, never guessed.
    pub unlabelled: usize,
    /// Each member's, in order of its id, members with none left out.
    pub members: Vec<MemberTurns>,
}

/// Submissions and what came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Review {
    /// Work submitted for review (moved to `in_review`).
    pub submissions: usize,
    /// Work sent back to its member from review, by the tester, a lead or the owner.
    pub sent_back: usize,
}

/// Tasks that finished, and what their whole lives cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Finished {
    /// Tasks that reached done.
    pub done: usize,
    /// Everything ever spent on turns on those tasks.
    pub done_spent: Micros,
    /// Tasks that were dropped.
    pub dropped: usize,
    /// Everything ever spent on turns on those: waste.
    pub dropped_spent: Micros,
}

impl Finished {
    /// The cost of one finished task: what the done ones cost, over their count.
    #[must_use]
    pub fn per_done(&self) -> Option<Micros> {
        if self.done == 0 {
            return None;
        }
        Some(self.done_spent / self.done as Micros)
    }
}

/// The home lab's alarms, and the work they made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Alarms {
    /// How many alarms reached the team: fired into a new task, or posted into an open one.
    pub count: usize,
    /// How many tasks they made.
    pub tasks: usize,
    /// The median time from an alarm to its task closed (done or dropped), for those closed.
    pub closed_median_ms: Option<Millis>,
    /// How many of their tasks are still open.
    pub open: usize,
}

/// What reached the owner's desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Desk {
    /// Actions a member asked to run.
    pub approvals: usize,
    /// Questions a member asked.
    pub questions: usize,
    /// Work that got stuck.
    pub stuck: usize,
    /// Notices: things done the owner is only told of.
    pub notices: usize,
    /// Every other kind: goals, the CEO's decisions, caps, rounds, things to read.
    pub other: usize,
    /// How many of all of them still wait.
    pub waiting: usize,
}

/// The prompt cache's share of the input the team's paid turns sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cache {
    /// All input tokens: fresh, read from the cache, written to it.
    pub input: i64,
    /// The input tokens read from the cache.
    pub read: i64,
}

impl Cache {
    /// The cache's share of the input, in whole percent; `None` with no input to share.
    #[must_use]
    pub fn share(&self) -> Option<u8> {
        percent(self.read as usize, self.input as usize)
    }
}

/// How the team did in one window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scorecard {
    /// The window scored.
    pub from: Millis,
    /// Where it ends.
    pub until: Millis,
    /// The oldest event HQ still keeps, when it is after `from`: the record is shorter than
    /// the window, and the numbers count from here.
    pub since: Option<Millis>,
    /// The turns.
    pub turns: Turns,
    /// Submissions and send-backs.
    pub review: Review,
    /// Tasks finished, and their cost.
    pub finished: Finished,
    /// The alarms.
    pub alarms: Alarms,
    /// The owner's desk.
    pub desk: Desk,
    /// The prompt cache. HQ keeps the tokens and not the brains' prices (the relay's), so
    /// the saving in dollars is not computed here: the share alone is shown.
    pub cache: Cache,
}

/// A whole-percent share, `None` when there is nothing to share.
fn percent(part: usize, whole: usize) -> Option<u8> {
    if whole == 0 {
        return None;
    }
    Some((part * 100 / whole).min(100) as u8)
}

/// Scores one window of the record.
#[must_use]
pub fn score(record: &Record<'_>, window: &Window) -> Scorecard {
    Scorecard {
        from: window.from,
        until: window.until,
        since: record
            .events
            .first()
            .map(|e| e.at)
            .filter(|at| *at > window.from),
        turns: turns(record.runs, window),
        review: review(record.events, window),
        finished: finished(record, window),
        alarms: alarms(record, window),
        desk: desk(record, window),
        cache: cache(record.runs, window),
    }
}

/// A turn that ended in the window: one that started, and ended, in it.
fn ended_in<'a>(runs: &'a [Run], window: &'a Window) -> impl Iterator<Item = &'a Run> {
    runs.iter()
        .filter(move |r| r.started_at.is_some() && r.ended_at.is_some_and(|at| window.holds(at)))
}

fn turns(runs: &[Run], window: &Window) -> Turns {
    let mut turns = Turns::default();
    for run in ended_in(runs, window) {
        let nothing = run.label.is_some_and(|l| !l.productive());
        let unlabelled = run.label.is_none();
        turns.total += 1;
        turns.nothing += usize::from(nothing);
        turns.unlabelled += usize::from(unlabelled);
        let at = match turns.members.iter().position(|m| m.member == run.member) {
            Some(at) => at,
            None => {
                turns.members.push(MemberTurns {
                    member: run.member,
                    turns: 0,
                    nothing: 0,
                    unlabelled: 0,
                    spent: 0,
                });
                turns.members.len() - 1
            }
        };
        let mine = &mut turns.members[at];
        mine.turns += 1;
        mine.nothing += usize::from(nothing);
        mine.unlabelled += usize::from(unlabelled);
        mine.spent += run.spent;
    }
    turns.members.sort_by_key(|m| m.member.0);
    turns
}

/// The task a task event is about.
fn task_of(event: &Event) -> Option<TaskId> {
    event.data["task"].as_i64().map(TaskId)
}

/// A task's status after a `task.changed` event.
fn status_of(event: &Event) -> Option<TaskStatus> {
    if event.kind != "task.changed" {
        return None;
    }
    event.data["status"]
        .as_str()
        .and_then(TaskStatus::from_name)
}

fn review(events: &[Event], window: &Window) -> Review {
    let mut review = Review::default();
    // Where each task was before each change, from the whole record: a send-back is work
    // that left review for `doing`, whoever sent it.
    let mut last: HashMap<TaskId, TaskStatus> = HashMap::new();
    for event in events {
        let (Some(task), Some(status)) = (task_of(event), status_of(event)) else {
            continue;
        };
        let before = last.insert(task, status);
        if !window.holds(event.at) {
            continue;
        }
        if status == TaskStatus::InReview {
            review.submissions += 1;
        }
        if status == TaskStatus::Doing && before == Some(TaskStatus::InReview) {
            review.sent_back += 1;
        }
    }
    review
}

/// Everything ever spent on turns on a task. Its sub-tasks are their own tasks.
fn spent_on(runs: &[Run], task: TaskId) -> Micros {
    runs.iter()
        .filter(|r| r.task == Some(task))
        .map(|r| r.spent)
        .sum()
}

fn finished(record: &Record<'_>, window: &Window) -> Finished {
    let mut done = HashSet::new();
    let mut dropped = HashSet::new();
    for event in record.events.iter().filter(|e| window.holds(e.at)) {
        let Some(task) = task_of(event) else {
            continue;
        };
        match status_of(event) {
            Some(TaskStatus::Done) => {
                done.insert(task);
            }
            Some(TaskStatus::Dropped) => {
                dropped.insert(task);
            }
            _ => {}
        }
    }
    Finished {
        done: done.len(),
        done_spent: done.iter().map(|t| spent_on(record.runs, *t)).sum(),
        dropped: dropped.len(),
        dropped_spent: dropped.iter().map(|t| spent_on(record.runs, *t)).sum(),
    }
}

/// When a task closed (done or dropped) at or after `from`, if the record says it did.
fn closed_after(events: &[Event], task: TaskId, from: Millis) -> Option<Millis> {
    events
        .iter()
        .filter(|e| e.at >= from && task_of(e) == Some(task))
        .find(|e| matches!(status_of(e), Some(s) if s.finished()))
        .map(|e| e.at)
}

fn median(mut values: Vec<Millis>) -> Option<Millis> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        return Some(values[mid]);
    }
    Some((values[mid - 1] + values[mid]) / 2)
}

fn alarms(record: &Record<'_>, window: &Window) -> Alarms {
    let mut alarms = Alarms::default();
    let mut closed = Vec::new();
    for event in record.events.iter().filter(|e| window.holds(e.at)) {
        // An alarm passed into a routine's open task is an alarm; a recovery is news, not one.
        if event.kind == "routine.posted" && event.data["recovered"] != true {
            alarms.count += 1;
            continue;
        }
        if event.kind != "routine.fired" || event.data["firing"] != "alarm" {
            continue;
        }
        alarms.count += 1;
        let Some(task) = task_of(event) else {
            continue;
        };
        alarms.tasks += 1;
        if let Some(at) = closed_after(record.events, task, event.at) {
            closed.push(at - event.at);
            continue;
        }
        // No closing in the record: open unless the task itself says it finished.
        let finished = record
            .tasks
            .iter()
            .any(|t| t.id == task && t.status.finished());
        alarms.open += usize::from(!finished);
    }
    alarms.closed_median_ms = median(closed);
    alarms
}

fn desk(record: &Record<'_>, window: &Window) -> Desk {
    let mut desk = Desk::default();
    for event in record
        .events
        .iter()
        .filter(|e| e.kind == "desk.added" && window.holds(e.at))
    {
        match event.data["kind"].as_str() {
            Some("approval") => desk.approvals += 1,
            Some("question") => desk.questions += 1,
            Some("stuck") => desk.stuck += 1,
            Some("notice") => desk.notices += 1,
            _ => desk.other += 1,
        }
        let item = event.data["item"].as_i64();
        let waiting = record
            .desk
            .iter()
            .any(|d| Some(d.id.0) == item && d.status == "waiting");
        desk.waiting += usize::from(waiting);
    }
    desk
}

/// The cache's share of the input of the turns the ledger charged: a turn on the house's
/// model costs nothing, so a charged turn is a cloud one.
fn cache(runs: &[Run], window: &Window) -> Cache {
    let mut cache = Cache::default();
    for run in ended_in(runs, window).filter(|r| r.spent > 0) {
        cache.input += run.input_tokens + run.cache_read_tokens + run.cache_write_tokens;
        cache.read += run.cache_read_tokens;
    }
    cache
}

impl Scorecard {
    /// The card's numbers for the screen, which only renders them.
    #[must_use]
    pub fn json(&self) -> Value {
        json!({
            "from": self.from,
            "until": self.until,
            "since": self.since,
            "turns": {
                "total": self.turns.total,
                "nothing": self.turns.nothing,
                "nothing_share": percent(self.turns.nothing, self.turns.total),
                "unlabelled": self.turns.unlabelled,
                "members": self.turns.members.iter().map(|m| json!({
                    "member": m.member.0, "turns": m.turns, "nothing": m.nothing,
                    "unlabelled": m.unlabelled, "spent": m.spent,
                })).collect::<Vec<_>>(),
            },
            "review": {
                "submissions": self.review.submissions,
                "sent_back": self.review.sent_back,
                "sent_back_share": percent(self.review.sent_back, self.review.submissions),
            },
            "finished": {
                "done": self.finished.done,
                "done_spent": self.finished.done_spent,
                "per_done": self.finished.per_done(),
                "dropped": self.finished.dropped,
                "dropped_spent": self.finished.dropped_spent,
            },
            "alarms": {
                "count": self.alarms.count,
                "tasks": self.alarms.tasks,
                "closed_median_ms": self.alarms.closed_median_ms,
                "open": self.alarms.open,
            },
            "desk": {
                "approvals": self.desk.approvals,
                "questions": self.desk.questions,
                "stuck": self.desk.stuck,
                "notices": self.desk.notices,
                "other": self.desk.other,
                "waiting": self.desk.waiting,
            },
            "cache": {
                "input": self.cache.input,
                "read": self.cache.read,
                "share": self.cache.share(),
            },
        })
    }
}
