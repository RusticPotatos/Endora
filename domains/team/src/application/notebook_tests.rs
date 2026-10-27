//! Per-member notebooks (ADR 0118): a note rests on evidence from HQ's record, a member writes
//! and forgets only its own, the notebook is in the briefing newest first and capped, a tidy is
//! asked for after a week, a note nobody used for a month is stale, and a furloughed member
//! keeps its notes and is told to read them first when it comes back.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::{GoalId, MemberId, Priority, Ref, Role, TaskId, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// 16 Oct 2026, 16:00 UTC.
const START: i64 = 1_792_166_400_000;
const DAY: i64 = 24 * 60 * 60 * 1000;

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings::default(),
    )
}

struct Team {
    ceo: MemberId,
    kit: MemberId,
    rae: MemberId,
    zed: MemberId,
    goal: GoalId,
}

fn team(hq: &Hq<'_>) -> Team {
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    hq.set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let seeded = hq.seed_wing().unwrap().expect("the wing is seeded");
    // Kit's first turn, on the plan, is over: the Mac takes one turn at a time (ADR 0131), and
    // these tests are about the notebooks of the others.
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
    }
    hq.tick().unwrap();
    Team {
        ceo,
        kit: seeded.lead,
        rae: seeded.developers[0],
        zed: seeded.developers[1],
        goal: seeded.goal,
    }
}

/// An open task handed to `to`, which wakes it.
fn task_for(hq: &Hq<'_>, t: &Team, to: MemberId, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: None,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

/// The newest turn of `member`'s, after a tick or two to start it. The wing's members build
/// with the Mac (ADR 0131), so the node says it is free.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn")
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, who) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &who, tool, args)
}

/// Rae's turn on a fresh task, with `token`s to call tools by.
fn raes_turn(hq: &Hq<'_>, t: &Team, title: &str) -> (RunContext, TaskId) {
    // Every earlier turn is over, hers and the others' (the Mac takes one turn at a time, ADR
    // 0131, and two slots would not hold hers), so this one starts at once.
    for earlier in hq.runner().started() {
        hq.runner().finish(earlier.run, true);
    }
    hq.tick().unwrap();
    // ...and the open tasks put away, hers and Kit's plan, so nothing is left to recover when
    // the clock jumps a week (a recovery turn would hold the Mac).
    for old in hq.store().tasks().unwrap() {
        if !old.status.finished() {
            hq.drop_task(old.id).unwrap();
        }
    }
    let task = task_for(hq, t, t.rae, title);
    (turn(hq, t.rae), task)
}

/// A note of Rae's, straight into the store (as an earlier turn would have left it).
fn kept_note(hq: &Hq<'_>, t: &Team, text: &str, at: i64) -> i64 {
    hq.store()
        .add_note(t.rae, text, &[Ref::Task(TaskId(1))], false, None, at)
        .unwrap()
}

#[test]
fn the_ceos_briefing_carries_only_the_count_of_its_notes_and_the_tool() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    for n in 1..=5 {
        hq.store()
            .add_note(
                t.ceo,
                &format!("House note {n}."),
                &[Ref::Task(TaskId(1))],
                false,
                None,
                START,
            )
            .unwrap();
    }
    hq.wake(t.ceo, None, WakeReason::QuestionAsked).unwrap();
    let ceo = turn(&hq, t.ceo);
    assert!(
        ceo.told
            .contains("Your notes (5): read_notebook shows them."),
        "{}",
        ceo.told
    );
    assert!(!ceo.told.contains("House note 1."), "{}", ceo.told);
}

#[test]
fn a_note_unused_for_thirty_days_is_stale_until_it_is_cited() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let old = kept_note(&hq, &t, "The proxy restarts clean.", START);
    kept_note(&hq, &t, "The router logs to the NAS.", START + 29 * DAY);
    clock.0.store(START + 31 * DAY, Ordering::SeqCst);
    let (rae, task) = raes_turn(&hq, &t, "Look at the proxy");
    assert!(
        rae.told.contains("n1: The proxy restarts clean. (stale)"),
        "{}",
        rae.told
    );
    assert!(
        !rae.told.contains("The router logs to the NAS. (stale)"),
        "{}",
        rae.told
    );
    // Citing the note's number in a post uses it; so does citing what it rests on.
    use_tool(&hq, &rae, "post", &json!({ "task": task.0, "text": format!("n{old} still holds: the proxy came back."), "evidence": [format!("task {task}")] })).unwrap();
    let kept = hq.store().notes(t.rae).unwrap();
    assert_eq!(kept[0].last_used_at, START + 31 * DAY);
    let (later, _) = raes_turn(&hq, &t, "Look at the DNS");
    assert!(!later.told.contains("(stale)"), "{}", later.told);
    // Evidence alone also counts: the second note rests on task 1.
    clock.0.store(START + 70 * DAY, Ordering::SeqCst);
    let (rae3, task3) = raes_turn(&hq, &t, "Look at the modem");
    assert!(rae3.told.contains("(stale)"), "{}", rae3.told);
    use_tool(&hq, &rae3, "post", &json!({ "task": task3.0, "text": "Checked the first task again.", "evidence": ["task 1"] })).unwrap();
    assert!(
        hq.store()
            .notes(t.rae)
            .unwrap()
            .iter()
            .all(|n| n.last_used_at == START + 70 * DAY)
    );
}

#[test]
fn a_furloughed_member_keeps_its_notes_and_is_told_to_read_them_first_on_its_return() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    kept_note(&hq, &t, "The proxy restarts clean.", START);
    hq.furlough(t.rae).unwrap();
    assert_eq!(
        hq.store().notes(t.rae).unwrap().len(),
        1,
        "the notebook stays"
    );
    clock.0.store(START + 2 * DAY, Ordering::SeqCst);
    hq.recall(t.rae).unwrap();
    assert_eq!(hq.store().notes(t.rae).unwrap().len(), 1);
    let (back, _) = raes_turn(&hq, &t, "Look at the proxy");
    assert!(
        back.told
            .contains("You were furloughed from 16 Oct to 18 Oct; read your notes first."),
        "{}",
        back.told
    );
    assert!(
        back.told.contains("n1: The proxy restarts clean."),
        "{}",
        back.told
    );
    let (next, _) = raes_turn(&hq, &t, "Look at the router");
    assert!(!next.told.contains("You were furloughed"), "{}", next.told);
}

#[test]
fn the_owner_writes_edits_and_deletes_notes_with_no_evidence_and_they_say_whose_they_are() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let mine = hq
        .owner_note(t.rae, "Never restart the proxy at night.")
        .unwrap();
    assert!(hq.owner_note(t.rae, "  ").is_err());
    assert!(hq.owner_note(t.rae, &"x".repeat(301)).is_err());
    let theirs = kept_note(&hq, &t, "The router logs to the NAS.", START);
    hq.owner_edit_note(t.rae, theirs, "The router logs to the NAS, hourly.")
        .unwrap();
    let all = hq.notebook(t.rae).unwrap();
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .any(|n| n.id == mine && n.from_owner && n.evidence.is_empty())
    );
    assert!(
        all.iter()
            .any(|n| n.id == theirs && n.text.ends_with("hourly."))
    );
    let (rae, _) = raes_turn(&hq, &t, "Look at the proxy");
    assert!(
        rae.told.contains(&format!(
            "n{mine}: Never restart the proxy at night. (from the owner)"
        )),
        "{}",
        rae.told
    );
    // Another member's note is not reachable through someone else's notebook.
    assert!(hq.owner_delete_note(t.zed, theirs).is_err());
    hq.owner_delete_note(t.rae, mine).unwrap();
    assert_eq!(hq.notebook(t.rae).unwrap().len(), 1);
    let kinds: Vec<String> = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(kinds.iter().filter(|k| *k == "note.written").count() >= 2);
    assert!(kinds.contains(&"note.forgotten".to_owned()));
    let _ = (t.kit, t.ceo);
}
