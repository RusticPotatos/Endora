//! Replay over made-up rows: a turn read back in order from what the record keeps, its own
//! rows by run id, an older turn's by its member, task and time (and saying so); then the
//! engine naming each event's turn, and a task's turns read back oldest first.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::ports::{Cited, Clock, LedgerRow, NewTask, Post, Run, TeamStore, ThreadPost, Wake};
use super::replay::{Moment, Record, What, replay, replay_all};
use crate::application::event_ports::EventStore;
use crate::domain::{
    Brain, CallReceipt, Event, Label, Member, MemberId, Priority, Receipt, Ref, Role, RunId,
    RunStatus, TaskId, TaskStatus, WakeId, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

/// 2026-10-17 12:00 UTC.
const NOON: i64 = 1_792_238_400_000;
const SECOND: i64 = 1_000;

fn kit() -> Member {
    Member {
        id: MemberId(1),
        name: "Kit".to_owned(),
        role: Role::Developer,
        share_percent: 40,
        slots: 2,
        look: 3,
        paused: false,
        brain: Brain::Haiku,
        work_brain: Brain::Mac,
        reports_to: None,
        furloughed_at: None,
        recalled_at: None,
    }
}

/// Run 5: Kit's turn on task 7, woken by wake 3, queued at noon and ended a minute later.
fn turn() -> Run {
    Run {
        id: RunId(5),
        member: MemberId(1),
        task: Some(TaskId(7)),
        wake: WakeId(3),
        status: RunStatus::Succeeded,
        attempts: 1,
        queued_at: NOON,
        started_at: Some(NOON + SECOND),
        retry_at: None,
        ended_at: Some(NOON + 60 * SECOND),
        reserved: 400_000,
        spent: 120_000,
        told: "You are Kit. <b>Look</b> at sonarr.".to_owned(),
        end_reason: "finished".to_owned(),
        moved_forward: true,
        posted: true,
        blocked: false,
        label: Some(Label::Advanced),
        input_tokens: 1_200,
        cache_read_tokens: 9_000,
        cache_write_tokens: 0,
        output_tokens: 300,
        asked_by_owner: false,
        brain: Some(Brain::Haiku),
        interrupted: 0,
    }
}

fn wake(id: i64, reason: WakeReason, receipt: Receipt, at: i64) -> Wake {
    Wake {
        id: WakeId(id),
        member: MemberId(1),
        task: Some(TaskId(7)),
        reason,
        receipt,
        run: Some(RunId(5)),
        at,
        by_owner: false,
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

fn post(task: i64, author: &str, text: &str, at: i64, evidence: Vec<Ref>) -> ThreadPost {
    ThreadPost {
        task: TaskId(task),
        post: Post {
            author: author.to_owned(),
            text: text.to_owned(),
            at,
            unsupported: evidence.is_empty(),
        },
        evidence,
    }
}

fn receipt(id: i64, run: i64, at: i64) -> CallReceipt {
    CallReceipt {
        id,
        run: RunId(run),
        member: MemberId(1),
        task: Some(TaskId(7)),
        tool: "container_logs".to_owned(),
        args: "container: sonarr".to_owned(),
        excerpt: "GET /api 500 <script>".to_owned(),
        ok: true,
        act: false,
        confidential: true,
        at,
        sandbox: false,
    }
}

fn row(kind: &str, amount: i64, at: i64) -> LedgerRow {
    LedgerRow {
        run: RunId(5),
        kind: kind.to_owned(),
        amount,
        at,
    }
}

/// What each moment is, in order, as the screen names it.
fn kinds(moments: &[Moment]) -> Vec<String> {
    moments
        .iter()
        .map(|m| match &m.what {
            What::Event(e) => e.kind.clone(),
            What::Woken(_) => "woken".to_owned(),
            What::Joined(_) => "joined".to_owned(),
            What::Briefing(_) => "briefing".to_owned(),
            What::Call(_) => "call".to_owned(),
            What::Posted(_) => "post".to_owned(),
            What::Cited(_) => "cited".to_owned(),
            What::Ended => "ended".to_owned(),
        })
        .collect()
}

struct Rows {
    members: Vec<Member>,
    wakes: Vec<Wake>,
    receipts: Vec<CallReceipt>,
    posts: Vec<ThreadPost>,
    cited: Vec<Cited>,
    events: Vec<Event>,
    ledger: Vec<LedgerRow>,
}

impl Rows {
    fn record(&self, tagged_from: Option<i64>) -> Record<'_> {
        Record {
            members: &self.members,
            wakes: &self.wakes,
            receipts: &self.receipts,
            posts: &self.posts,
            cited: &self.cited,
            events: &self.events,
            ledger: &self.ledger,
            tagged_from,
            now: NOON + 3_600 * SECOND,
        }
    }
}

/// A turn from since events name their turn: its own rows, and others' about the same task.
fn tagged() -> Rows {
    let at = |s: i64| NOON + s * SECOND;
    Rows {
        members: vec![kit()],
        wakes: vec![
            wake(3, WakeReason::Assigned, Receipt::Queued, at(0) - 5 * SECOND),
            wake(4, WakeReason::OwnerReplied, Receipt::Folded, at(30)),
        ],
        receipts: vec![receipt(12, 5, at(10)), receipt(13, 6, at(11))],
        posts: vec![
            post(7, "owner", "Is sonarr up?", at(2), vec![]),
            post(
                7,
                "Kit",
                "sonarr: 500s since 9.",
                at(20),
                vec![Ref::Receipt(12)],
            ),
            post(7, "Kit", "From another turn.", at(25), vec![]),
        ],
        cited: vec![Cited {
            task: TaskId(7),
            run: RunId(5),
            member: MemberId(1),
            stage: "submitted".to_owned(),
            criterion: 1,
            refs: vec![Ref::Receipt(12)],
            at: at(40),
        }],
        events: vec![
            event(
                100,
                at(1),
                "run.started",
                json!({ "run": 5, "member": 1, "task": 7 }),
            ),
            event(
                101,
                at(5),
                "task.claimed",
                json!({ "task": 7, "member": 1, "run": 5 }),
            ),
            event(
                102,
                at(10),
                "receipt.recorded",
                json!({ "receipt": 12, "run": 5 }),
            ),
            event(
                103,
                at(20),
                "thread.posted",
                json!({ "task": 7, "author": "Kit", "run": 5 }),
            ),
            event(
                104,
                at(21),
                "budget.changed",
                json!({ "run": 5, "spent": 120_000 }),
            ),
            event(
                105,
                at(25),
                "thread.posted",
                json!({ "task": 7, "author": "Kit", "run": 6 }),
            ),
            event(
                106,
                at(41),
                "task.changed",
                json!({ "task": 7, "status": "in_review", "run": 5 }),
            ),
            event(
                107,
                at(42),
                "desk.added",
                json!({ "item": 2, "task": 7, "run": 5 }),
            ),
            event(
                108,
                at(43),
                "task.changed",
                json!({ "task": 7, "status": "doing" }),
            ),
            event(
                109,
                at(60),
                "run.ended",
                json!({ "run": 5, "result": "succeeded" }),
            ),
        ],
        ledger: vec![
            row("reserve", 400_000, at(1)),
            row("use", 120_000, at(21)),
            row("charge", 120_000, at(60)),
            row("release", 400_000, at(60)),
        ],
    }
}

#[test]
fn a_turn_reads_back_in_order_from_its_own_rows() {
    let rows = tagged();
    let r = replay(&rows.record(Some(NOON - 3_600 * SECOND)), &turn());
    assert_eq!(
        kinds(&r.moments),
        [
            "woken",
            "run.started",
            "briefing",
            "task.claimed",
            "call",
            "post",
            "joined",
            "cited",
            "task.changed",
            "desk.added",
            "run.ended"
        ],
        "another turn's receipt, post and events are not its; the owner's post is not; a \
         status change naming no turn is not, in a turn since events name theirs"
    );
    assert!(r.moments.iter().all(|m| !m.by_time) && !r.by_time);
    assert_eq!(r.reason, Some(WakeReason::Assigned));
    assert_eq!(
        (
            r.money.reserved,
            r.money.used,
            r.money.charged,
            r.money.released
        ),
        (400_000, 120_000, 120_000, 400_000)
    );
    let What::Posted(p) = &r.moments[5].what else {
        panic!("{:?}", r.moments[5]);
    };
    assert_eq!(
        (p.post.text.as_str(), p.evidence.as_slice()),
        ("sonarr: 500s since 9.", [Ref::Receipt(12)].as_slice())
    );
}

#[test]
fn the_screen_is_given_everything_as_written_confidential_excerpts_too() {
    let rows = tagged();
    let shown = replay(&rows.record(Some(NOON - 3_600 * SECOND)), &turn()).shown();
    assert_eq!(shown["member"]["name"], "Kit");
    assert_eq!(shown["member"]["role"], "developer");
    assert_eq!(shown["member"]["brain"], "claude-haiku-5-5");
    assert_eq!(shown["reason"], "assigned");
    assert_eq!(shown["label"], "advanced");
    assert_eq!(shown["money"]["spent"], 120_000);
    assert_eq!(shown["tokens"]["cache_read"], 9_000);
    let moments = shown["moments"].as_array().unwrap();
    let call = moments.iter().find(|m| m["kind"] == "call").unwrap();
    assert_eq!(call["receipt"], "r12");
    assert_eq!(
        call["excerpt"], "GET /api 500 <script>",
        "the owner's screen escapes it"
    );
    assert_eq!(call["confidential"], true);
    let told = moments.iter().find(|m| m["kind"] == "briefing").unwrap();
    assert_eq!(told["text"], "You are Kit. <b>Look</b> at sonarr.");
    let post = moments.iter().find(|m| m["kind"] == "post").unwrap();
    assert_eq!(post["evidence"], json!(["r12"]));
    assert_eq!(post["unsupported"], false);
}

#[test]
fn an_older_turn_is_matched_by_its_member_task_and_time_and_says_so() {
    let at = |s: i64| NOON + s * SECOND;
    let rows = Rows {
        members: vec![kit()],
        wakes: vec![wake(3, WakeReason::Assigned, Receipt::Queued, at(0))],
        receipts: vec![receipt(12, 5, at(10))],
        posts: vec![
            post(7, "Kit", "Looking.", at(15), vec![]),
            post(7, "owner", "Thanks.", at(16), vec![]),
            post(8, "Kit", "Another thread.", at(17), vec![]),
            post(7, "Kit", "Later, another turn.", at(90), vec![]),
        ],
        cited: Vec::new(),
        events: vec![
            event(
                100,
                at(1),
                "run.started",
                json!({ "run": 5, "member": 1, "task": 7 }),
            ),
            event(
                101,
                at(5),
                "task.claimed",
                json!({ "task": 7, "member": 1 }),
            ),
            event(
                102,
                at(6),
                "task.claimed",
                json!({ "task": 9, "member": 2 }),
            ),
            event(
                103,
                at(30),
                "task.changed",
                json!({ "task": 7, "status": "in_review" }),
            ),
            event(104, at(31), "desk.added", json!({ "item": 4, "task": 8 })),
            event(
                105,
                at(32),
                "house.noted",
                json!({ "by": "Kit", "entry": "- a note" }),
            ),
            event(106, at(33), "tick", json!({ "started": 0 })),
            event(
                107,
                at(90),
                "task.changed",
                json!({ "task": 7, "status": "done" }),
            ),
        ],
        ledger: Vec::new(),
    };
    for tagged_from in [None, Some(at(61))] {
        let r = replay(&rows.record(tagged_from), &turn());
        assert_eq!(
            kinds(&r.moments),
            [
                "woken",
                "run.started",
                "briefing",
                "task.claimed",
                "call",
                "post",
                "task.changed",
                "house.noted",
                "ended"
            ],
            "{tagged_from:?}: another member's claim, another task's desk item, the tick and \
             what came after it are not its; its end is from its row, the feed no longer has it"
        );
        assert!(r.by_time);
        let by_time: Vec<bool> = r.moments.iter().map(|m| m.by_time).collect();
        assert_eq!(
            by_time,
            [false, false, false, true, false, true, true, true, false],
            "the run's own rows are exact, the rest say they were matched by time"
        );
    }
}

#[test]
fn a_turn_retried_reads_back_both_attempts_and_ends_once() {
    let at = |s: i64| NOON + s * SECOND;
    let mut run = turn();
    run.attempts = 2;
    run.status = RunStatus::Failed;
    run.label = None;
    let rows = Rows {
        members: vec![kit()],
        wakes: vec![wake(3, WakeReason::Assigned, Receipt::Queued, at(0))],
        receipts: Vec::new(),
        posts: Vec::new(),
        cited: Vec::new(),
        events: vec![
            event(100, at(1), "run.started", json!({ "run": 5 })),
            event(
                101,
                at(9),
                "run.ended",
                json!({ "run": 5, "result": "retry_waiting" }),
            ),
            event(102, at(20), "run.started", json!({ "run": 5 })),
            event(
                103,
                at(60),
                "run.ended",
                json!({ "run": 5, "result": "failed" }),
            ),
        ],
        ledger: Vec::new(),
    };
    let r = replay(&rows.record(Some(NOON - SECOND)), &run);
    assert_eq!(
        kinds(&r.moments),
        [
            "woken",
            "run.started",
            "briefing",
            "run.ended",
            "run.started",
            "run.ended"
        ]
    );
}

#[test]
fn a_tasks_turns_read_back_oldest_first() {
    let rows = tagged();
    let mut later = turn();
    later.id = RunId(9);
    later.queued_at = NOON + 120 * SECOND;
    let mut earlier = turn();
    earlier.id = RunId(2);
    earlier.queued_at = NOON - 120 * SECOND;
    let all = replay_all(
        &rows.record(None),
        &[later.clone(), turn(), earlier.clone()],
    );
    let ids: Vec<RunId> = all.iter().map(|r| r.run.id).collect();
    assert_eq!(ids, [RunId(2), RunId(5), RunId(9)]);
}

// ---- through the engine ---------------------------------------------------------------------

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

fn office(clock: &Hands) -> (Hq<'_>, TaskId) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "a test", Author::Owner)
        .unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Look at the music stack".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    (hq, task)
}

#[test]
fn every_event_a_turns_call_emits_names_the_turn_and_nothing_else_does() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, task) = office(&clock);
    assert_eq!(
        hq.store().setting(super::replay::TAGGED_FROM).unwrap(),
        None
    );
    hq.tick().unwrap();
    let turn = hq.runner().started()[0].clone();
    clock.0.fetch_add(SECOND, Ordering::SeqCst);
    let before = hq.store().last_event().unwrap();
    hq.claim_in(turn.run, task).unwrap();
    hq.post_in(turn.run, task, "Having a look.").unwrap();
    hq.set_status_in(turn.run, task, TaskStatus::Blocked, "waiting on the NAS")
        .unwrap();
    let from_the_turn = hq.store().events_after(before).unwrap();
    assert!(from_the_turn.len() >= 4, "{from_the_turn:?}");
    for e in &from_the_turn {
        assert_eq!(e.data["run"], turn.run.0, "{e:?}");
    }
    assert_eq!(
        hq.store().setting(super::replay::TAGGED_FROM).unwrap(),
        Some((NOON + SECOND).to_string())
    );
    // The owner's reply right after, outside any turn, names none.
    let before = hq.store().last_event().unwrap();
    hq.post(Author::Owner, task, "Thanks.").unwrap();
    for e in hq.store().events_after(before).unwrap() {
        assert!(
            e.data.get("run").is_none() || e.kind.starts_with("run."),
            "{e:?}"
        );
    }
}

#[test]
fn a_tasks_turns_are_read_back_through_the_engine_each_with_its_own() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, task) = office(&clock);
    hq.tick().unwrap();
    let first = hq.runner().started()[0].clone();
    clock.0.fetch_add(SECOND, Ordering::SeqCst);
    hq.claim_in(first.run, task).unwrap();
    hq.post_in(first.run, task, "First look: <img src=x onerror=alert(1)>")
        .unwrap();
    clock.0.fetch_add(SECOND, Ordering::SeqCst);
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();
    // The owner replies; the member's next turn answers.
    clock.0.fetch_add(60 * SECOND, Ordering::SeqCst);
    hq.post(Author::Owner, task, "And now?").unwrap();
    hq.tick().unwrap();
    let second = hq
        .runner()
        .started()
        .into_iter()
        .find(|t| t.run != first.run)
        .unwrap();
    clock.0.fetch_add(SECOND, Ordering::SeqCst);
    hq.post_in(second.run, task, "Second look.").unwrap();
    clock.0.fetch_add(SECOND, Ordering::SeqCst);
    hq.runner().finish(second.run, true);
    hq.tick().unwrap();

    let all = hq.replay_task(task).unwrap();
    let ids: Vec<RunId> = all.iter().map(|r| r.run.id).collect();
    assert_eq!(ids, [first.run, second.run]);
    let texts = |r: &super::replay::RunReplay| -> Vec<String> {
        r.moments
            .iter()
            .filter_map(|m| match &m.what {
                What::Posted(p) => Some(p.post.text.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(texts(&all[0]), ["First look: <img src=x onerror=alert(1)>"]);
    assert_eq!(
        texts(&all[1]),
        ["Second look."],
        "the owner's reply is nobody's turn"
    );
    assert!(
        all.iter().all(|r| r.reason.is_some()),
        "each says why it was woken"
    );
    // The first turn was queued before any event named its turn, so what names none is matched
    // by time; every turn queued since takes only its own, by run id.
    assert!(!all[1].by_time, "{:?}", all[1].moments);
    for r in &all {
        assert!(kinds(&r.moments).contains(&"run.ended".to_owned()));
    }
    assert!(kinds(&all[0].moments).contains(&"task.claimed".to_owned()));
    assert_eq!(hq.replay_run(second.run).unwrap(), all[1]);
    assert!(hq.replay_run(RunId(999)).is_err());
    assert!(hq.replay_task(TaskId(999)).is_err());
}
