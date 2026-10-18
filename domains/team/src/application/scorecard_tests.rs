//! The scorecard over made-up rows: each number from the record alone, nothing guessed.

use serde_json::{Value, json};

use super::ports::{DeskItem, Run};
use super::scorecard::{Record, Window, score};
use crate::domain::money::{DAY_MS, day_of};
use crate::domain::{
    DeskId, Event, GoalId, Label, MemberId, Priority, RunId, RunStatus, Task, TaskId, TaskStatus,
    WakeId,
};

/// 2026-10-17 12:00 UTC.
const NOON: i64 = 1_792_238_400_000;
const MINUTE: i64 = 60_000;

fn today() -> Window {
    Window::day(day_of(NOON, 0), 0)
}

fn week() -> Window {
    Window::days_ending(day_of(NOON, 0), 7, 0)
}

fn run(
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
    }
}

fn spending(mut run: Run, spent: i64) -> Run {
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

fn event(id: i64, at: i64, kind: &str, data: Value) -> Event {
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
