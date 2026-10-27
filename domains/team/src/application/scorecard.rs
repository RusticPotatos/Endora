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

pub mod outcomes;

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
    /// The `ms` before `now`, `now` itself included: the day and the week the guards look over
    /// (ADR 0124), ending at the moment HQ looks.
    #[must_use]
    pub const fn rolling(now: Millis, ms: Millis) -> Self {
        Self {
            from: now - ms + 1,
            until: now + 1,
        }
    }

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
    /// The events kept of the [`KINDS_READ`], oldest first.
    pub events: &'a [Event],
    /// When the kept feed begins, its oldest event of any kind: pruning drops the oldest.
    pub kept_from: Option<Millis>,
}

/// The kinds of event the scorecard and its outcomes read. Only these are handed to them:
/// ticks and the rest are most of the feed, and parsing them was most of the card's cost.
/// A measure that reads another kind adds it here.
pub const KINDS_READ: [&str; 7] = [
    "task.changed",
    "routine.fired",
    "routine.posted",
    "desk.added",
    "branch.pushed",
    "task.sent_back",
    "record.asked",
];

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
    /// How many tasks assigned to the member reached done in the window: with
    /// [`Self::done_spent`], the cost of a finished task by member, which the Claude Code pilot
    /// is measured by (ADR 0117).
    pub done: usize,
    /// Everything ever spent on turns on those tasks, by anyone.
    pub done_spent: Micros,
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
        since: record.kept_from.filter(|at| *at > window.from),
        turns: with_done(turns(record.runs, window), record, window),
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
                    done: 0,
                    done_spent: 0,
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

/// Each member's turns with the tasks assigned to it that reached done in the window, and
/// what their whole lives cost.
fn with_done(mut turns: Turns, record: &Record<'_>, window: &Window) -> Turns {
    let done: HashSet<TaskId> = record
        .events
        .iter()
        .filter(|e| window.holds(e.at) && status_of(e) == Some(TaskStatus::Done))
        .filter_map(task_of)
        .collect();
    for mine in &mut turns.members {
        let theirs: Vec<TaskId> = record
            .tasks
            .iter()
            .filter(|t| t.assignee == Some(mine.member) && done.contains(&t.id))
            .map(|t| t.id)
            .collect();
        mine.done = theirs.len();
        mine.done_spent = theirs.iter().map(|t| spent_on(record.runs, *t)).sum();
    }
    turns
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
    let waiting: HashSet<i64> = record
        .desk
        .iter()
        .filter(|d| d.status == "waiting")
        .map(|d| d.id.0)
        .collect();
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
        desk.waiting += usize::from(item.is_some_and(|i| waiting.contains(&i)));
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

// ---- each team's measures (ADR 0120) ------------------------------------------------------

/// An outage this long or longer, from an alarm to its recovery (or its task closed), is
/// unplanned downtime worth counting.
pub const OUTAGE_MS: Millis = 10 * 60 * 1000;

/// How close two alarms about the same containers are a repeat: a day.
pub const REPEAT_MS: Millis = DAY_MS;

/// One lead's area as the record says it: the lead, whoever reports to it, the projects it
/// leads, and which measures apply (the home lab's, when its projects take the lab's alarms;
/// the wing's, when it works through branches). Never told by a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    /// The lead.
    pub lead: MemberId,
    /// The lead and its members.
    pub members: Vec<MemberId>,
    /// The tasks in the projects it leads.
    pub tasks: HashSet<TaskId>,
    /// Whether the home lab's measures apply.
    pub lab: bool,
    /// Whether the wing's measures apply.
    pub branches: bool,
}

/// What the measures read beyond the [`Record`]: the branches, the threads of the tasks that
/// have one, the testers' names, the home lab's containers when the node said them, and now.
#[derive(Debug, Clone, Copy)]
pub struct AreaRecord<'a> {
    /// Every event kept, oldest first.
    pub events: &'a [Event],
    /// Every task.
    pub tasks: &'a [Task],
    /// Every desk item.
    pub desk: &'a [DeskItem],
    /// Every branch submitted.
    pub branches: &'a [crate::domain::Branch],
    /// The threads of the branches' tasks: each post's author and text, oldest first.
    pub threads: &'a HashMap<TaskId, Vec<(String, String)>>,
    /// The testers' names.
    pub testers: &'a [String],
    /// The home lab's containers, if the node said them.
    pub lab: Option<&'a [String]>,
    /// Now.
    pub now: Millis,
}

/// The home lab's measures for an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LabMeasures {
    /// Alarms about its work.
    pub alarms: usize,
    /// Alarms whose recovery (or task closed) came [`OUTAGE_MS`] or more after, or not yet.
    pub outages: usize,
    /// The median time from an alarm to its task closed.
    pub closed_median_ms: Option<Millis>,
    /// Alarms about containers an alarm named within [`REPEAT_MS`] before.
    pub repeats: usize,
}

/// The wing's measures for an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BranchMeasures {
    /// Branches its members submitted.
    pub submitted: usize,
    /// The median time from a branch submitted to the owner approving its push (the merge
    /// itself is on GitHub, out of HQ's sight).
    pub to_push_median_ms: Option<Millis>,
    /// Branch tasks the tester gave a verdict on.
    pub tested: usize,
    /// Of those, the ones whose first verdict was a pass.
    pub passed_first: usize,
}

/// What an alarm event says and is about, if it is one: (its task, its words).
fn alarm_of(event: &Event) -> Option<(Option<TaskId>, &str)> {
    if event.kind == "routine.fired" && event.data["firing"] == "alarm" {
        return Some((task_of(event), event.data["why"].as_str().unwrap_or("")));
    }
    if event.kind == "routine.posted" && event.data["recovered"] != true {
        return Some((task_of(event), event.data["said"].as_str().unwrap_or("")));
    }
    None
}

/// The containers an alarm names, or its words when the lab's names are not known.
fn named_in(said: &str, lab: Option<&[String]>) -> Vec<String> {
    match lab {
        Some(lab) => crate::domain::review::about(said, "", lab),
        None => vec![said.trim().to_ascii_lowercase()],
    }
}

/// The home lab's measures of an area, in a window.
#[must_use]
pub fn lab_measures(area: &Area, record: &AreaRecord<'_>, window: &Window) -> LabMeasures {
    let mut measures = LabMeasures::default();
    let alarms: Vec<(&Event, TaskId, &str)> = record
        .events
        .iter()
        .filter_map(|e| {
            let (task, said) = alarm_of(e)?;
            let task = task.filter(|t| area.tasks.contains(t))?;
            Some((e, task, said))
        })
        .collect();
    let mut closed = Vec::new();
    for (i, (event, task, said)) in alarms.iter().enumerate() {
        if !window.holds(event.at) {
            continue;
        }
        measures.alarms += 1;
        let ended = record
            .events
            .iter()
            .filter(|e| e.at >= event.at && task_of(e) == Some(*task) && e.id > event.id)
            .find(|e| {
                (e.kind == "routine.posted" && e.data["recovered"] == true)
                    || matches!(status_of(e), Some(s) if s.finished())
            })
            .map(|e| e.at);
        let lasted = ended.unwrap_or(record.now.min(window.until)) - event.at;
        measures.outages += usize::from(lasted >= OUTAGE_MS);
        if event.kind == "routine.fired" {
            if let Some(at) = closed_after(record.events, *task, event.at) {
                closed.push(at - event.at);
            }
        }
        let names = named_in(said, record.lab);
        let again = alarms[..i].iter().any(|(before, _, was)| {
            event.at - before.at <= REPEAT_MS
                && named_in(was, record.lab).iter().any(|n| names.contains(n))
        });
        measures.repeats += usize::from(again);
    }
    measures.closed_median_ms = median(closed);
    measures
}

/// The wing's measures of an area, in a window.
#[must_use]
pub fn branch_measures(area: &Area, record: &AreaRecord<'_>, window: &Window) -> BranchMeasures {
    let mut measures = BranchMeasures::default();
    let ours: Vec<&crate::domain::Branch> = record
        .branches
        .iter()
        .filter(|b| area.members.contains(&b.member) && window.holds(b.at))
        .collect();
    let mut waited = Vec::new();
    for branch in &ours {
        measures.submitted += 1;
        let items: Vec<i64> = record
            .desk
            .iter()
            .filter(|d| d.kind == "push" && d.task == Some(branch.task) && d.at >= branch.at)
            .map(|d| d.id.0)
            .collect();
        let approved = record.events.iter().find(|e| {
            e.kind == "desk.decided"
                && e.data["decision"] == "approve"
                && e.data["item"].as_i64().is_some_and(|i| items.contains(&i))
        });
        if let Some(approved) = approved {
            waited.push(approved.at - branch.at);
        }
    }
    measures.to_push_median_ms = median(waited);
    let tasks: HashSet<TaskId> = ours.iter().map(|b| b.task).collect();
    for task in tasks {
        let Some(thread) = record.threads.get(&task) else {
            continue;
        };
        let first = thread.iter().find(|(author, text)| {
            record.testers.contains(author)
                && (text.starts_with("Tested and passed") || text.starts_with("Sent back: "))
        });
        let Some((_, verdict)) = first else {
            continue;
        };
        measures.tested += 1;
        measures.passed_first += usize::from(verdict.starts_with("Tested and passed"));
    }
    measures
}

impl LabMeasures {
    /// For the screen.
    #[must_use]
    pub fn json(&self) -> Value {
        json!({ "alarms": self.alarms, "outages": self.outages,
                "closed_median_ms": self.closed_median_ms, "repeats": self.repeats })
    }

    /// As the CEO and the lead read them.
    #[must_use]
    pub fn said(&self) -> String {
        format!(
            "{} alarms; unplanned outages over 10 min: {}; median alarm to closed: {}; repeat \
             alarms within 24 h: {}",
            self.alarms,
            self.outages,
            self.closed_median_ms.map_or("none closed".to_owned(), span),
            self.repeats
        )
    }
}

impl BranchMeasures {
    /// For the screen.
    #[must_use]
    pub fn json(&self) -> Value {
        json!({ "submitted": self.submitted, "to_push_median_ms": self.to_push_median_ms,
                "tested": self.tested, "passed_first": self.passed_first })
    }

    /// As the CEO and the lead read them.
    #[must_use]
    pub fn said(&self) -> String {
        format!(
            "{} branches submitted; median branch submitted to push approved: {}; branches \
             passing the tester first time: {} of {}",
            self.submitted,
            self.to_push_median_ms
                .map_or("none approved".to_owned(), span),
            self.passed_first,
            self.tested
        )
    }
}

/// A span in plain words: `12 min`, `3 h 5 min`, `2 days`.
#[must_use]
pub fn span(ms: Millis) -> String {
    let minutes = ms / 60_000;
    if minutes < 60 {
        return format!("{minutes} min");
    }
    if minutes < 48 * 60 {
        return format!("{} h {} min", minutes / 60, minutes % 60);
    }
    format!("{} days", minutes / (24 * 60))
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
                    "done": m.done, "done_spent": m.done_spent,
                    "per_done": (m.done > 0).then(|| m.done_spent / m.done as Micros),
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

#[cfg(test)]
mod measures_tests {
    use super::*;
    use crate::domain::{Branch, DeskId, RunId};

    const MIN: Millis = 60_000;

    fn event(id: i64, at: Millis, kind: &str, data: Value) -> Event {
        Event {
            id,
            at,
            kind: kind.to_owned(),
            data,
        }
    }

    fn area(tasks: &[i64]) -> Area {
        Area {
            lead: MemberId(2),
            members: vec![MemberId(2), MemberId(4)],
            tasks: tasks.iter().copied().map(TaskId).collect(),
            lab: true,
            branches: true,
        }
    }

    fn window() -> Window {
        Window {
            from: 0,
            until: 10 * DAY_MS,
        }
    }

    /// The night of 17 October as the record kept it: deluge and prowlarr exited, came back,
    /// and exited again two hours later, in two tasks of the upkeep area.
    #[test]
    fn the_labs_measures_count_long_outages_the_time_to_close_and_repeats() {
        let lab = [
            "deluge".to_owned(),
            "prowlarr".to_owned(),
            "nzbget".to_owned(),
        ];
        let events = [
            event(
                1,
                0,
                "routine.fired",
                json!({ "firing": "alarm", "task": 667,
                  "why": "deluge, prowlarr exited" }),
            ),
            event(
                2,
                3 * MIN,
                "routine.posted",
                json!({ "task": 667, "recovered": true,
                  "said": "deluge is back" }),
            ),
            event(
                3,
                3 * MIN,
                "task.changed",
                json!({ "task": 667, "status": "done" }),
            ),
            event(
                4,
                120 * MIN,
                "routine.fired",
                json!({ "firing": "alarm", "task": 668,
                  "why": "deluge, nzbget, prowlarr exited" }),
            ),
            event(
                5,
                150 * MIN,
                "task.changed",
                json!({ "task": 668, "status": "done" }),
            ),
            // Another area's alarm counts nowhere here.
            event(
                6,
                150 * MIN,
                "routine.fired",
                json!({ "firing": "alarm", "task": 900,
                  "why": "deluge exited" }),
            ),
        ];
        let threads = HashMap::new();
        let record = AreaRecord {
            events: &events,
            tasks: &[],
            desk: &[],
            branches: &[],
            threads: &threads,
            testers: &[],
            lab: Some(&lab),
            now: 200 * MIN,
        };
        let m = lab_measures(&area(&[667, 668]), &record, &window());
        assert_eq!(
            m,
            LabMeasures {
                alarms: 2,
                outages: 1,
                closed_median_ms: Some((3 * MIN + 30 * MIN) / 2),
                repeats: 1,
            }
        );
        assert_eq!(
            m.said(),
            "2 alarms; unplanned outages over 10 min: 1; median alarm to closed: 16 min; \
             repeat alarms within 24 h: 1"
        );
    }

    #[test]
    fn the_wings_measures_time_a_branch_to_its_push_and_count_first_time_passes() {
        let branch = |id: i64, task: i64, at: Millis| Branch {
            id,
            task: TaskId(task),
            run: RunId(1),
            member: MemberId(4),
            name: format!("hq/task-{task}"),
            head: String::new(),
            base: String::new(),
            files: 1,
            plus: 1,
            minus: 0,
            can_push: true,
            status: "submitted".to_owned(),
            at,
        };
        let branches = [branch(1, 10, 0), branch(2, 11, 0)];
        let desk = [DeskItem {
            id: DeskId(5),
            kind: "push".to_owned(),
            task: Some(TaskId(10)),
            goal: None,
            project: None,
            member: Some(MemberId(4)),
            text: String::new(),
            status: "approve".to_owned(),
            at: 30 * MIN,
        }];
        let events = [event(
            1,
            90 * MIN,
            "desk.decided",
            json!({ "item": 5, "kind": "push", "decision": "approve" }),
        )];
        let mut threads = HashMap::new();
        threads.insert(
            TaskId(10),
            vec![
                ("Rae".to_owned(), "doing → in_review: ready".to_owned()),
                ("Dee".to_owned(), "Tested and passed: fine".to_owned()),
            ],
        );
        threads.insert(
            TaskId(11),
            vec![
                ("Dee".to_owned(), "Sent back: lint fails".to_owned()),
                ("Dee".to_owned(), "Tested and passed: fixed".to_owned()),
            ],
        );
        let testers = ["Dee".to_owned()];
        let record = AreaRecord {
            events: &events,
            tasks: &[],
            desk: &desk,
            branches: &branches,
            threads: &threads,
            testers: &testers,
            lab: None,
            now: 100 * MIN,
        };
        let m = branch_measures(&area(&[]), &record, &window());
        assert_eq!(
            m,
            BranchMeasures {
                submitted: 2,
                to_push_median_ms: Some(90 * MIN),
                tested: 2,
                passed_first: 1,
            }
        );
        assert!(
            m.said().contains("passing the tester first time: 1 of 2"),
            "{}",
            m.said()
        );
    }
}
