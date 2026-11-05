//! The scorecard over made-up rows: each number from the record alone, nothing guessed.

use serde_json::{Value, json};

use super::ports::{DeskItem, Run};
use super::scorecard::{Record, Window, by_model_shown, score};
use crate::domain::money::{DAY_MS, day_of};
use crate::domain::runs;
use crate::domain::{
    Brain, DeskId, Event, GoalId, Label, MemberId, Priority, RunId, RunStatus, Task, TaskId,
    TaskStatus, WakeId,
};

/// 2026-10-17 12:00 UTC.
pub(super) const NOON: i64 = 1_792_238_400_000;
pub(super) const MINUTE: i64 = 60_000;

fn today() -> Window {
    Window::day(day_of(NOON, 0), 0)
}

fn week() -> Window {
    Window::days_ending(day_of(NOON, 0), 7, 0)
}

pub(super) fn run(
    id: i64,
    member: i64,
    task: Option<i64>,
    ended_at: Option<i64>,
    label: Option<Label>,
) -> Run {
    Run {
        id: RunId(id),
        member: MemberId(member),
        task: task.map(TaskId),
        wake: WakeId(id),
        status: if ended_at.is_some() {
            RunStatus::Succeeded
        } else {
            RunStatus::Running
        },
        attempts: 1,
        queued_at: ended_at.unwrap_or(NOON) - 2 * MINUTE,
        started_at: Some(ended_at.unwrap_or(NOON) - MINUTE),
        retry_at: None,
        ended_at,
        reserved: 0,
        spent: 0,
        told: String::new(),
        end_reason: String::new(),
        moved_forward: false,
        posted: false,
        blocked: false,
        label,
        input_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 0,
        asked_by_owner: false,
        brain: None,
        interrupted: 0,
    }
}

pub(super) fn spending(mut run: Run, spent: i64) -> Run {
    run.spent = spent;
    run
}

fn task(id: i64, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id),
        goal: GoalId(1),
        project: None,
        parent: None,
        title: format!("task {id}"),
        description: String::new(),
        owner: None,
        assignee: None,
        status,
        priority: Priority::Normal,
        depth: 0,
        waits_on: Vec::new(),
        nudged: false,
        criteria: Vec::new(),
    }
}

pub(super) fn event(id: i64, at: i64, kind: &str, data: Value) -> Event {
    Event {
        id,
        at,
        kind: kind.to_owned(),
        data,
    }
}

fn changed(id: i64, at: i64, task: i64, status: TaskStatus) -> Event {
    event(
        id,
        at,
        "task.changed",
        json!({ "task": task, "status": status.name() }),
    )
}

fn desk_item(id: i64, kind: &str, status: &str) -> DeskItem {
    DeskItem {
        id: DeskId(id),
        kind: kind.to_owned(),
        task: None,
        goal: None,
        project: None,
        member: None,
        text: String::new(),
        status: status.to_owned(),
        at: NOON,
    }
}

fn record<'a>(
    runs: &'a [Run],
    tasks: &'a [Task],
    desk: &'a [DeskItem],
    events: &'a [Event],
) -> Record<'a> {
    Record {
        runs,
        tasks,
        desk,
        events,
        kept_from: events.first().map(|e| e.at),
    }
}

#[test]
fn the_windows_are_the_owners_days() {
    let day = day_of(NOON, -300);
    let today = Window::day(day, -300);
    assert_eq!(today.until - today.from, DAY_MS);
    assert!(today.from <= NOON && NOON < today.until);
    let week = Window::days_ending(day, 7, -300);
    assert_eq!(week.until, today.until);
    assert_eq!(week.from, today.from - 6 * DAY_MS);
}

#[test]
fn turns_are_counted_with_what_they_showed_and_the_unlabelled_said_so() {
    let runs = [
        spending(
            run(1, 2, Some(7), Some(NOON), Some(Label::Advanced)),
            100_000,
        ),
        spending(
            run(2, 2, Some(7), Some(NOON + MINUTE), Some(Label::TalkOnly)),
            50_000,
        ),
        // From before labels (ADR 0114): it cannot be told, and is not guessed.
        spending(run(3, 1, Some(7), Some(NOON + 2 * MINUTE), None), 20_000),
        // Yesterday's, empty: the week's, not today's.
        spending(
            run(4, 1, None, Some(NOON - DAY_MS), Some(Label::Empty)),
            10_000,
        ),
        // Still running: not a turn that ended.
        run(5, 2, Some(7), None, None),
    ];
    let r = record(&runs, &[], &[], &[]);

    let today = score(&r, &today()).turns;
    assert_eq!((today.total, today.nothing, today.unlabelled), (3, 1, 1));
    assert_eq!(today.members.len(), 2, "two members took turns");
    assert_eq!(today.members[0].member, MemberId(1), "in order of id");
    assert_eq!(
        (
            today.members[0].turns,
            today.members[0].unlabelled,
            today.members[0].spent
        ),
        (1, 1, 20_000)
    );
    assert_eq!(
        (
            today.members[1].turns,
            today.members[1].nothing,
            today.members[1].spent
        ),
        (2, 1, 150_000)
    );

    let week = score(&r, &week()).turns;
    assert_eq!((week.total, week.nothing, week.unlabelled), (4, 2, 1));
}

#[test]
fn a_send_back_is_work_that_left_review_for_doing() {
    let events = [
        changed(1, NOON, 1, TaskStatus::InReview),
        changed(2, NOON + MINUTE, 1, TaskStatus::Doing),
        changed(3, NOON + 2 * MINUTE, 2, TaskStatus::InReview),
        changed(4, NOON + 3 * MINUTE, 2, TaskStatus::Done),
        // Claimed from todo: not a send-back.
        changed(5, NOON + 4 * MINUTE, 3, TaskStatus::Doing),
        // Submitted yesterday, sent back today: today's send-back, yesterday's submission.
        changed(6, NOON - DAY_MS, 4, TaskStatus::InReview),
        changed(7, NOON + 5 * MINUTE, 4, TaskStatus::Doing),
    ];
    let r = record(&[], &[], &[], &events);
    let card = score(&r, &today());
    assert_eq!((card.review.submissions, card.review.sent_back), (2, 2));
    assert_eq!(card.json()["review"]["sent_back_share"], json!(100));
    let week = score(&r, &week()).review;
    assert_eq!((week.submissions, week.sent_back), (3, 2));
}

#[test]
fn a_finished_task_costs_its_whole_life_and_a_dropped_one_is_waste() {
    let runs = [
        spending(
            run(1, 2, Some(1), Some(NOON - DAY_MS), Some(Label::Checked)),
            100_000,
        ),
        spending(
            run(2, 2, Some(1), Some(NOON), Some(Label::Advanced)),
            50_000,
        ),
        spending(
            run(3, 2, Some(2), Some(NOON), Some(Label::TalkOnly)),
            30_000,
        ),
        spending(run(4, 2, Some(3), Some(NOON), Some(Label::Advanced)), 999),
    ];
    let events = [
        changed(1, NOON, 1, TaskStatus::Done),
        // Said twice: one task.
        changed(2, NOON + MINUTE, 1, TaskStatus::Done),
        changed(3, NOON + 2 * MINUTE, 2, TaskStatus::Dropped),
        // Still open: not finished.
        changed(4, NOON + 3 * MINUTE, 3, TaskStatus::Doing),
    ];
    let r = record(&runs, &[], &[], &events);
    let finished = score(&r, &today()).finished;
    assert_eq!((finished.done, finished.done_spent), (1, 150_000));
    assert_eq!(finished.per_done(), Some(150_000));
    assert_eq!((finished.dropped, finished.dropped_spent), (1, 30_000));
    let empty = score(&record(&[], &[], &[], &[]), &today()).finished;
    assert_eq!(empty.per_done(), None, "no finished task, no cost per one");
}

/// A task done with nothing checked (a loop worked alone, no branch) is counted done and, apart,
/// as not verified, so the card never folds it into verified outcomes; a done said before the
/// mark existed, or verified, is not (ADR 0124, amended 2026-11-03).
#[test]
fn a_task_completed_with_nothing_checked_is_counted_apart_as_not_verified() {
    let unverified = event(
        1,
        NOON,
        "task.changed",
        json!({ "task": 1, "status": "done", "unverified": true }),
    );
    let events = [
        unverified,
        changed(2, NOON, 2, TaskStatus::Done),
        changed(3, NOON, 3, TaskStatus::Done),
    ];
    let finished = score(&record(&[], &[], &[], &events), &today()).finished;
    assert_eq!((finished.done, finished.unverified), (3, 1));
    let shown = score(&record(&[], &[], &[], &events), &today()).json();
    assert_eq!(shown["finished"]["unverified"], json!(1));
}

#[test]
fn each_members_finished_tasks_and_their_cost_compare_two_developers() {
    // The Claude Code pilot (ADR 0117): Rae (2) and Zed (3), each with a task finished today.
    let runs = [
        spending(
            run(1, 2, Some(1), Some(NOON), Some(Label::Advanced)),
            120_000,
        ),
        spending(
            run(2, 2, Some(1), Some(NOON), Some(Label::Advanced)),
            30_000,
        ),
        spending(
            run(3, 3, Some(2), Some(NOON), Some(Label::Advanced)),
            40_000,
        ),
        spending(run(4, 3, Some(3), Some(NOON), Some(Label::TalkOnly)), 5_000),
    ];
    let mut tasks = [
        task(1, TaskStatus::Done),
        task(2, TaskStatus::Done),
        task(3, TaskStatus::Doing),
    ];
    tasks[0].assignee = Some(MemberId(2));
    tasks[1].assignee = Some(MemberId(3));
    tasks[2].assignee = Some(MemberId(3));
    let events = [
        changed(1, NOON, 1, TaskStatus::Done),
        changed(2, NOON, 2, TaskStatus::Done),
    ];
    let card = score(&record(&runs, &tasks, &[], &events), &today());
    let of = |m: i64| {
        let mine = card
            .turns
            .members
            .iter()
            .find(|t| t.member == MemberId(m))
            .unwrap();
        (mine.done, mine.done_spent)
    };
    assert_eq!(of(2), (1, 150_000));
    assert_eq!(of(3), (1, 40_000), "an open task is not a finished one");
    let shown = card.json();
    let rae = &shown["turns"]["members"][0];
    assert_eq!(
        (rae["done"].clone(), rae["per_done"].clone()),
        (json!(1), json!(150_000))
    );
    let none = score(&record(&runs, &[], &[], &[]), &today()).json();
    assert_eq!(none["turns"]["members"][0]["per_done"], Value::Null);
}

#[test]
fn alarms_count_the_tasks_they_made_and_how_long_each_took_to_close() {
    let fired = |id, at, task| {
        event(
            id,
            at,
            "routine.fired",
            json!({ "routine": 1, "task": task, "firing": "alarm", "why": "deluge exited" }),
        )
    };
    let events = [
        fired(1, NOON, 5),
        changed(2, NOON + 10 * MINUTE, 5, TaskStatus::Done),
        fired(3, NOON + MINUTE, 6),
        changed(4, NOON + 31 * MINUTE, 6, TaskStatus::Dropped),
        fired(5, NOON + 2 * MINUTE, 7),
        // Passed into an open task: an alarm, no new task.
        event(
            6,
            NOON + 3 * MINUTE,
            "routine.posted",
            json!({ "routine": 1, "task": 5, "said": "x" }),
        ),
        // A recovery is news, not an alarm.
        event(
            7,
            NOON + 4 * MINUTE,
            "routine.posted",
            json!({ "routine": 1, "task": 5, "said": "y", "recovered": true }),
        ),
        // A routine falling due is not an alarm.
        event(
            8,
            NOON + 5 * MINUTE,
            "routine.fired",
            json!({ "routine": 1, "task": 8, "firing": "due", "why": "" }),
        ),
    ];
    let tasks = [
        task(5, TaskStatus::Done),
        task(6, TaskStatus::Dropped),
        task(7, TaskStatus::Doing),
    ];
    let r = record(&[], &tasks, &[], &events);
    let alarms = score(&r, &today()).alarms;
    assert_eq!((alarms.count, alarms.tasks, alarms.open), (4, 3, 1));
    assert_eq!(
        alarms.closed_median_ms,
        Some(20 * MINUTE),
        "the middle of 10 and 30 minutes"
    );

    // A task the record no longer says closed, that is finished all the same, is not open.
    let only_fired = [fired(1, NOON, 5)];
    let r = record(&[], &tasks, &[], &only_fired);
    let alarms = score(&r, &today()).alarms;
    assert_eq!((alarms.open, alarms.closed_median_ms), (0, None));
}

#[test]
fn the_desk_is_split_by_kind_with_what_still_waits() {
    let added = |id, at, item, kind| {
        event(
            id,
            at,
            "desk.added",
            json!({ "item": item, "kind": kind, "task": 7 }),
        )
    };
    let events = [
        added(1, NOON, 1, "approval"),
        added(2, NOON, 2, "question"),
        added(3, NOON, 3, "stuck"),
        added(4, NOON, 4, "notice"),
        added(5, NOON, 5, "goal"),
        added(6, NOON, 6, "rounds"),
        added(7, NOON - DAY_MS, 7, "approval"),
    ];
    let desk = [
        desk_item(1, "approval", "waiting"),
        desk_item(2, "question", "seen"),
        desk_item(3, "stuck", "waiting"),
        desk_item(4, "notice", "seen"),
        desk_item(5, "goal", "approve"),
        desk_item(6, "rounds", "waiting"),
        desk_item(7, "approval", "waiting"),
    ];
    let r = record(&[], &[], &desk, &events);
    let today = score(&r, &today()).desk;
    assert_eq!(
        (
            today.approvals,
            today.questions,
            today.stuck,
            today.notices,
            today.other
        ),
        (1, 1, 1, 1, 2)
    );
    assert_eq!(today.waiting, 3);
    assert_eq!(score(&r, &week()).desk.approvals, 2);
}

#[test]
fn the_cache_share_is_of_the_paid_turns_input() {
    let mut cloud = spending(run(1, 2, Some(7), Some(NOON), Some(Label::Checked)), 40_000);
    cloud.input_tokens = 1_000;
    cloud.cache_read_tokens = 3_000;
    let mut local = run(2, 1, Some(7), Some(NOON), Some(Label::Checked));
    local.input_tokens = 50_000;
    let runs = [cloud, local];
    let cache = score(&record(&runs, &[], &[], &[]), &today()).cache;
    assert_eq!((cache.input, cache.read), (4_000, 3_000));
    assert_eq!(cache.share(), Some(75));
    let none = score(&record(&[], &[], &[], &[]), &today()).cache;
    assert_eq!(none.share(), None, "nothing sent, no share");
}

#[test]
fn a_record_shorter_than_the_window_says_where_it_starts() {
    let late = [changed(1, NOON, 1, TaskStatus::Doing)];
    let card = score(&record(&[], &[], &[], &late), &week());
    assert_eq!(card.since, Some(NOON), "the week began before the feed did");
    let card = score(&record(&[], &[], &[], &late), &today());
    assert_eq!(card.since, Some(NOON));
    let early = [changed(1, NOON - 10 * DAY_MS, 1, TaskStatus::Doing)];
    assert_eq!(score(&record(&[], &[], &[], &early), &week()).since, None);
    assert_eq!(score(&record(&[], &[], &[], &[]), &week()).since, None);
    // The feed starts where it is kept from, though the kinds the card never reads (ticks,
    // the guards' findings) are not handed to it.
    let mut kept = record(&[], &[], &[], &late);
    kept.kept_from = Some(NOON - 10 * DAY_MS);
    assert_eq!(score(&kept, &week()).since, None);
}

#[test]
fn the_json_carries_the_shares_the_screen_shows() {
    let runs = [
        run(1, 2, Some(7), Some(NOON), Some(Label::TalkOnly)),
        run(2, 2, Some(7), Some(NOON), Some(Label::Advanced)),
        run(3, 2, Some(7), Some(NOON), None),
    ];
    let shown = score(&record(&runs, &[], &[], &[]), &today()).json();
    assert_eq!(shown["turns"]["total"], json!(3));
    assert_eq!(shown["turns"]["nothing_share"], json!(33));
    assert_eq!(shown["turns"]["unlabelled"], json!(1));
    assert_eq!(shown["turns"]["members"][0]["member"], json!(2));
    assert_eq!(shown["review"]["sent_back_share"], Value::Null);
    assert_eq!(shown["finished"]["per_done"], Value::Null);
    assert_eq!(shown["cache"]["share"], Value::Null);
    assert_eq!(shown["since"], Value::Null);
}

// ---- What each model cost (the owner, 2026-10-31) -------------------------------------------

fn on(mut run: Run, brain: Brain) -> Run {
    run.brain = Some(brain);
    run
}

#[test]
fn by_model_counts_the_turns_that_ran_on_each_brain_this_week_and_the_week_before() {
    let day = day_of(NOON, 0);
    let last_week = NOON - 7 * DAY_MS;
    let mut never = on(
        spending(run(5, 2, Some(7), Some(NOON), None), 0),
        Brain::Sonnet,
    );
    never.status = RunStatus::Failed;
    never.end_reason = format!("{}: the runtime could not start", runs::NEVER_STARTED);
    // Turned away before it started (the guards, the kill switch): it ran nothing.
    let mut refused = on(run(6, 2, Some(7), Some(NOON), None), Brain::Opus);
    refused.started_at = None;
    refused.status = RunStatus::Stopped;
    let runs = [
        on(
            spending(run(1, 2, Some(7), Some(NOON), None), 400_000),
            Brain::Sonnet,
        ),
        on(
            spending(run(2, 3, Some(7), Some(NOON), None), 10_000),
            Brain::Haiku,
        ),
        on(run(3, 4, Some(7), Some(NOON), None), Brain::Local),
        // Still running: not yet a turn that ran.
        on(run(4, 4, Some(7), None, None), Brain::Local),
        never,
        refused,
        // Before brains were recorded on a turn: which it ran on cannot be told.
        spending(run(7, 2, Some(7), Some(NOON), None), 90_000),
        on(
            spending(run(8, 3, Some(7), Some(last_week), None), 30_000),
            Brain::Haiku,
        ),
        on(run(9, 4, Some(7), Some(last_week), None), Brain::Mac),
        // Older than both weeks.
        on(
            spending(run(10, 3, Some(7), Some(NOON - 15 * DAY_MS), None), 5_000),
            Brain::Haiku,
        ),
    ];
    let shown = by_model_shown(&runs, day, 0);
    let names: Vec<&str> = shown["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["brain"].as_str().expect("a brain"))
        .collect();
    assert_eq!(
        names,
        ["claude-sonnet-5-5", "claude-haiku-5-5", "local", "mac"],
        "{shown}"
    );
    assert_eq!(
        shown["rows"][0],
        json!({ "brain": "claude-sonnet-5-5", "trend": "up",
                "now": { "turns": 1, "spent": 400_000, "per_turn": 400_000, "share": 98 },
                "before": { "turns": 0, "spent": 0, "per_turn": null, "share": 0 } })
    );
    assert_eq!(
        shown["rows"][1],
        json!({ "brain": "claude-haiku-5-5", "trend": "down",
                "now": { "turns": 1, "spent": 10_000, "per_turn": 10_000, "share": 2 },
                "before": { "turns": 1, "spent": 30_000, "per_turn": 30_000, "share": 100 } })
    );
    assert_eq!(shown["rows"][3]["now"]["per_turn"], Value::Null);
    assert_eq!(shown["rows"][3]["before"]["turns"], json!(1));
    let week = Window::days_ending(day, 7, 0);
    assert_eq!(
        shown["now"],
        json!({ "from": week.from, "until": week.until, "turns": 3, "spent": 410_000,
                "per_turn": 136_666 })
    );
    assert_eq!(shown["before"]["from"], json!(week.from - 7 * DAY_MS));
    assert_eq!(shown["before"]["until"], json!(week.from));
    assert_eq!(shown["before"]["turns"], json!(2));
}
