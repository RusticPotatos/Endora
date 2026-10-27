//! A drain before a restart (ADR 0134), through the engine and the team protocol. Live, a deploy
//! and the NAS's own security reboot cut turns of ten or fifteen minutes in the middle of a
//! step, and the work was lost with them. Now a restart is announced first: nothing new starts,
//! the turns under way are told to wrap up, and what the restart still cuts is counted and told.

use std::sync::atomic::AtomicI64;

use serde_json::{Value, json};

use super::engine::{Engine, Settings};
use super::guards_tests::{Hands, Hq, NOON, later, team, team_over, turn, use_tool};
use super::ports::{Run, RunContext, TeamStore};
use super::world_ports::WorldStore as _;
use crate::domain::drain::{MAX_AGE_MS, NOTICE};
use crate::domain::{MemberId, RunStatus, TaskId};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

const MINUTE: i64 = 60_000;

fn canonical_team(clock: &Hands) -> (Hq<'_>, MemberId, TaskId) {
    use super::engine::Author;
    use super::ports::NewTask;
    use crate::domain::{Priority, Role};
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    {
        let id = hq.hire("Member 1", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    hq.hire("Member 2", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Member 3", Role::Developer, 40, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Inspect fixture".into(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    (hq, dev, task)
}

#[test]
fn canonical_context_preserves_its_restart_handover() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = canonical_team(&clock);
    let worker = working(&hq, dev, task);
    hq.begin_drain(None).unwrap();
    use_tool(
        &hq,
        &worker,
        "post",
        &json!({
            "task":task.0, "text":"Migration paused safely.", "handover":note()
        }),
    )
    .unwrap();
    later(&clock, 2 * MINUTE);
    hq.end_drain().unwrap();
    hq.tick().unwrap();
    let again = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|turn| turn.member == dev)
        .unwrap();
    assert_eq!(again.run, worker.run);
    assert!(
        again.told.contains("Your own note") && again.told.contains("Rebuild the index"),
        "restart context must survive: {}",
        again.told
    );
}

/// A note for the next turn, in all four parts.
fn note() -> Value {
    json!({
        "done": "The table is migrated.",
        "not_done": "The index is not rebuilt.",
        "next": "Rebuild the index, then check the downloads.",
        "watch_out": "The index rebuild locks the table.",
    })
}

/// Linus's turn on his task, with the task claimed and his talk in its thread.
fn working(hq: &Hq<'_>, dev: MemberId, task: TaskId) -> RunContext {
    let t = turn(hq, dev);
    hq.claim_in(t.run, task).unwrap();
    t
}

fn run_of(hq: &Hq<'_>, id: crate::domain::RunId) -> Run {
    hq.store().run(id).unwrap().unwrap()
}

fn events(hq: &Hq<'_>, kind: &str) -> Vec<Value> {
    hq.events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.data)
        .collect()
}

fn digest(hq: &Hq<'_>) -> Vec<Value> {
    hq.snapshot().unwrap()["digest"].as_array().unwrap().clone()
}

fn desk(hq: &Hq<'_>) -> Vec<Value> {
    hq.snapshot().unwrap()["desk"].as_array().unwrap().clone()
}

/// HQ started again over the team's file: the old one is dropped, nothing of its memory
/// survives, and its runner has forgotten what it was handed.
fn restarted<'a>(hq: Hq<'a>, clock: &'a Hands, file: &str) -> Hq<'a> {
    drop(hq);
    Engine::new(
        SqliteTeamStore::open(file).unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    )
}

/// A team whose store is a file of its own, so a test can start HQ again.
fn in_a_file<'a>(clock: &'a Hands, name: &str) -> (Hq<'a>, MemberId, TaskId, String) {
    let path = std::env::temp_dir().join(format!("endora-drain-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let file = path.to_str().unwrap().to_owned();
    let (hq, dev, task) = team_over(SqliteTeamStore::open(&file).unwrap(), clock, false);
    (hq, dev, task, file)
}

#[test]
fn a_drain_starts_no_new_turn_and_the_queued_work_waits_for_a_restart() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    hq.begin_drain(None).unwrap();
    assert!(hq.draining());
    hq.tick().unwrap();
    assert!(hq.runner().started().is_empty(), "nothing started");
    let waiting: Vec<Run> = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.member == dev && r.task == Some(task))
        .collect();
    assert_eq!(waiting.len(), 1, "the work was not dropped: {waiting:?}");
    assert_eq!(waiting[0].status, RunStatus::Queued);
    assert_eq!(
        waiting[0].end_reason,
        "waiting for a restart: nothing new starts until it has been"
    );

    // It starts once the drain is over, and the words are cleared with it.
    hq.end_drain().unwrap();
    hq.tick().unwrap();
    assert_eq!(hq.runner().started().len(), 1);
    let started = run_of(&hq, waiting[0].id);
    assert_eq!(started.status, RunStatus::Running);
    assert_eq!(started.end_reason, "");
}

#[test]
fn a_running_turns_answers_carry_the_notice_while_a_drain_lasts_and_only_then() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    let t = working(&hq, dev, task);
    let before = use_tool(&hq, &t, "my_turn", &json!({})).unwrap();
    assert!(!before.contains(NOTICE), "no drain, no notice");

    hq.begin_drain(None).unwrap();
    let during = use_tool(&hq, &t, "my_turn", &json!({})).unwrap();
    assert!(during.ends_with(&format!(
        "\n\n{NOTICE} {}",
        crate::domain::drain::HOW_TO_LEAVE_IT
    )));
    assert!(
        during.starts_with(&before),
        "what it was told is all still there"
    );
    // A refusal carries it too: every protocol answer a turn gets does.
    let refused = use_tool(&hq, &t, "claim", &json!({ "task": 9_999 })).unwrap_err();
    assert!(refused.contains(NOTICE), "{refused}");

    hq.end_drain().unwrap();
    let after = use_tool(&hq, &t, "my_turn", &json!({})).unwrap();
    assert!(!after.contains(NOTICE), "the notice goes with the drain");
}

#[test]
fn a_member_wraps_up_with_a_handover_and_keeps_its_task() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    let t = working(&hq, dev, task);
    let held_before = hq.store().task(task).unwrap().unwrap();
    hq.begin_drain(None).unwrap();

    let said = use_tool(
        &hq,
        &t,
        "post",
        &json!({ "task": task.0, "text": "Halfway through the migration.", "handover": note() }),
    )
    .unwrap();
    assert!(said.contains("stop now"), "{said}");

    // The turn is over, and the deploy need not wait for it.
    let ended = run_of(&hq, t.run);
    assert_eq!(ended.status, RunStatus::RetryWaiting, "{ended:?}");
    assert_eq!(hq.drain_shown().unwrap()["running"], 0);
    // The task is still its member's: not handed on, not blocked, not stuck.
    let held = hq.store().task(task).unwrap().unwrap();
    assert_eq!(
        (held.owner, held.assignee),
        (held_before.owner, held_before.assignee)
    );
    assert_eq!(held.status, held_before.status);
    // What it said is in the thread, and its note is kept, for itself.
    let thread = hq.store().thread(task).unwrap();
    assert!(thread.iter().any(|p| p.text.contains("Halfway through")));
    let notes = hq.store().handovers(task).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        (notes[0].by, notes[0].from, notes[0].to),
        (dev, Some(dev), Some(dev))
    );
    assert_eq!(notes[0].run, Some(t.run));

    // After the restart it goes on, as the same member, with the note first in its briefing,
    // and the wrap-up costs it nothing.
    later(&clock, 2 * MINUTE);
    hq.end_drain().unwrap();
    hq.tick().unwrap();
    let again = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == dev)
        .unwrap();
    assert_eq!(again.run, t.run, "the same turn is tried again");
    assert!(again.told.contains("Your own note"), "{}", again.told);
    assert!(again.told.contains("Rebuild the index"), "{}", again.told);
    let retried = run_of(&hq, t.run);
    assert_eq!(
        (retried.attempts, retried.interrupted),
        (1, 0),
        "{retried:?}"
    );
}

#[test]
fn a_handover_without_a_drain_is_refused_and_the_turn_goes_on() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    let t = working(&hq, dev, task);
    let refused = use_tool(
        &hq,
        &t,
        "post",
        &json!({ "task": task.0, "text": "Done for now.", "handover": note() }),
    )
    .unwrap_err();
    assert!(refused.contains("no restart is coming"), "{refused}");
    assert_eq!(run_of(&hq, t.run).status, RunStatus::Running);
    assert!(hq.store().handovers(task).unwrap().is_empty());
}

#[test]
fn a_handover_that_leaves_a_part_out_is_refused_and_the_turn_goes_on() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    let t = working(&hq, dev, task);
    hq.begin_drain(None).unwrap();
    let refused = use_tool(
        &hq,
        &t,
        "post",
        &json!({ "task": task.0, "text": "Stopping.",
                 "handover": { "done": "Some", "not_done": "", "next": "More", "watch_out": "Care" } }),
    )
    .unwrap_err();
    assert!(refused.contains("handover"), "{refused}");
    assert_eq!(run_of(&hq, t.run).status, RunStatus::Running);
    assert!(hq.store().handovers(task).unwrap().is_empty());
}

#[test]
fn a_drain_that_began_is_kept_and_a_second_asking_changes_nothing() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    working(&hq, dev, task);
    let first = hq.begin_drain(Some(120)).unwrap();
    assert_eq!(first["draining"], true);
    assert_eq!(
        (first["since"].as_i64(), first["cap_seconds"].as_i64()),
        (Some(NOON), Some(120))
    );
    assert_eq!(first["running"], 1);

    later(&clock, MINUTE);
    let second = hq.begin_drain(Some(10)).unwrap();
    assert_eq!(second["since"], first["since"], "it began once");
    assert_eq!(second["cap_seconds"], 120);
    assert_eq!(events(&hq, "drain.began").len(), 1);
    assert_eq!(hq.drain_shown().unwrap()["draining"], true);
}

#[test]
fn resuming_lifts_a_drain_once_and_says_what_it_came_to() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task) = team(&clock, false);
    let t = working(&hq, dev, task);
    hq.begin_drain(None).unwrap();
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();

    let lifted = hq.end_drain().unwrap();
    assert_eq!(lifted["draining"], false);
    assert!(!hq.draining());
    let ended = events(&hq, "drain.ended");
    assert_eq!(ended.len(), 1);
    assert_eq!(
        (ended[0]["by"].as_str(), ended[0]["held"].as_i64()),
        (Some("resume"), Some(1))
    );
    assert_eq!(
        (ended[0]["clean"].as_i64(), ended[0]["cut"].as_i64()),
        (Some(1), Some(0))
    );
    // The last one is told with the status, for a deploy to read.
    assert_eq!(hq.drain_shown().unwrap()["last"]["clean"], 1);

    // Resuming with no drain does nothing at all.
    assert_eq!(hq.end_drain().unwrap()["draining"], false);
    assert_eq!(events(&hq, "drain.ended").len(), 1);
}

#[test]
fn a_drain_nobody_ended_lifts_itself_after_thirty_minutes_and_the_work_goes_on() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _dev, _task) = team(&clock, false);
    hq.begin_drain(None).unwrap();
    later(&clock, MAX_AGE_MS - 1);
    hq.tick().unwrap();
    assert!(hq.draining(), "not yet");
    assert!(hq.runner().started().is_empty());

    later(&clock, 1);
    hq.tick().unwrap();
    assert!(!hq.draining(), "forgotten drains do not stall the team");
    assert_eq!(hq.runner().started().len(), 1, "and what waited starts");
    let ended = events(&hq, "drain.ended");
    assert_eq!(ended[0]["by"], "max age");
}

#[test]
fn a_restart_after_a_clean_drain_lifts_it_and_tells_the_owner_nothing() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task, file) = in_a_file(&clock, "clean");
    let t = working(&hq, dev, task);
    hq.begin_drain(None).unwrap();
    hq.runner().finish(t.run, true);
    hq.tick().unwrap();
    assert_eq!(hq.drain_shown().unwrap()["running"], 0);

    let hq = restarted(hq, &clock, &file);
    // The drain survived the restart, and holds until HQ's first tick lifts it.
    assert_eq!(hq.drain_shown().unwrap()["draining"], true);
    hq.tick().unwrap();
    assert!(!hq.draining());

    let ended = events(&hq, "drain.ended");
    assert_eq!(ended.len(), 1, "an Activity line");
    assert_eq!(ended[0]["by"], "restart");
    assert_eq!(
        (ended[0]["clean"].as_i64(), ended[0]["cut"].as_i64()),
        (Some(1), Some(0))
    );
    assert!(digest(&hq).is_empty(), "{:?}", digest(&hq));
    assert!(desk(&hq).is_empty());
    let _ = std::fs::remove_file(&file);
}

#[test]
fn a_restart_that_cuts_a_turn_is_one_digest_item_and_the_turn_is_tried_again() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task, file) = in_a_file(&clock, "cut");
    let t = working(&hq, dev, task);
    hq.begin_drain(Some(60)).unwrap();
    // The cap passes with the turn still running, and the deploy restarts HQ under it.
    later(&clock, 61_000);
    let hq = restarted(hq, &clock, &file);
    hq.tick().unwrap();

    let cut = run_of(&hq, t.run);
    assert_eq!(
        cut.status,
        RunStatus::RetryWaiting,
        "the existing retry path: {cut:?}"
    );
    assert_eq!(cut.interrupted, 1);
    let ended = events(&hq, "drain.ended");
    assert_eq!(
        (ended[0]["clean"].as_i64(), ended[0]["cut"].as_i64()),
        (Some(0), Some(1))
    );

    // One item, in the digest, which is not a card that interrupts.
    let items = digest(&hq);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["kind"], "notice");
    let text = items[0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("A restart cut 1 turn: Linus on #"),
        "{text}"
    );
    assert!(text.contains("Clear the stuck downloads"), "{text}");
    assert!(desk(&hq).is_empty(), "nothing waits on the owner");

    // And the work goes on once its minute is up.
    later(&clock, 2 * MINUTE);
    hq.tick().unwrap();
    assert_eq!(run_of(&hq, t.run).status, RunStatus::Running);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn several_turns_cut_are_one_item_naming_each() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, dev, task, file) = in_a_file(&clock, "several");
    let first = working(&hq, dev, task);
    let ada = hq
        .hire("Ada", crate::domain::Role::Developer, 40, 2, 4)
        .unwrap();
    let goal = hq.store().task(task).unwrap().unwrap().goal;
    let other = hq
        .add_task(
            super::engine::Author::Owner,
            super::ports::NewTask {
                goal,
                project: None,
                parent: None,
                title: "Rotate the logs".to_owned(),
                description: String::new(),
                assignee: Some(ada),
                priority: crate::domain::Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let second = working(&hq, ada, other);
    assert_ne!(first.run, second.run);
    hq.begin_drain(None).unwrap();
    let hq = restarted(hq, &clock, &file);
    hq.tick().unwrap();

    let items = digest(&hq);
    assert_eq!(items.len(), 1, "{items:?}");
    let text = items[0]["text"].as_str().unwrap();
    assert!(text.starts_with("A restart cut 2 turns: "), "{text}");
    assert!(text.contains("Linus") && text.contains("Ada"), "{text}");
    let _ = std::fs::remove_file(&file);
}
