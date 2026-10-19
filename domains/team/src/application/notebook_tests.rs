//! Per-member notebooks (ADR 0118): a note rests on evidence from HQ's record, a member writes
//! and forgets only its own, the notebook is in the briefing newest first and capped, a tidy is
//! asked for after a week, a note nobody used for a month is stale, and a furloughed member
//! keeps its notes and is told to read them first when it comes back.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::{call, tools};
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
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
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
fn a_note_needs_evidence_that_is_in_hqs_record() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let (rae, task) = raes_turn(&hq, &t, "Look at the proxy");
    let note = |args: Value| use_tool(&hq, &rae, "note", &args);
    let refused = note(json!({ "text": "The proxy restarts clean." })).unwrap_err();
    assert!(refused.contains("evidence"), "{refused}");
    let refused = note(json!({ "text": "The proxy restarts clean.", "evidence": [] })).unwrap_err();
    assert!(refused.contains("evidence"), "{refused}");
    let refused =
        note(json!({ "text": "The proxy restarts clean.", "evidence": ["task 999"] })).unwrap_err();
    assert!(
        refused.contains("task 999") && refused.contains("not in HQ's record"),
        "{refused}"
    );
    let refused =
        note(json!({ "text": "x".repeat(301), "evidence": [format!("task {task}")] })).unwrap_err();
    assert!(refused.contains("300"), "{refused}");
    let refused = note(json!({ "text": "  ", "evidence": [format!("task {task}")] })).unwrap_err();
    assert!(refused.contains("text"), "{refused}");
    let said =
        note(json!({ "text": "The proxy restarts clean.", "evidence": [format!("task {task}")] }))
            .unwrap();
    assert!(said.contains("n1"), "{said}");
    let kept = hq.store().notes(t.rae).unwrap();
    assert_eq!(kept.len(), 1);
    let kept = &kept[0];
    assert_eq!(kept.text, "The proxy restarts clean.");
    assert_eq!(kept.evidence, [Ref::Task(task)]);
    assert!(!kept.from_owner);
    assert!(kept.written_in_run.is_some());
    assert_eq!((kept.written_at, kept.last_used_at), (START, START));
}

#[test]
fn a_member_writes_and_forgets_only_its_own_notes() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let first = kept_note(&hq, &t, "The proxy restarts clean.", START);
    let (zed, _) = {
        let task = task_for(&hq, &t, t.zed, "Look at the router");
        (turn(&hq, t.zed), task)
    };
    // The tools name no member: a note goes in the notebook of the turn's own member.
    for role in [
        Role::Ceo,
        Role::Lead,
        Role::Developer,
        Role::Researcher,
        Role::Reviewer,
    ] {
        let all = tools(role);
        for name in ["note", "forget", "read_notebook", "notebook_checked"] {
            let tool = all
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["name"] == name)
                .unwrap_or_else(|| panic!("{role:?} lacks {name}"));
            assert!(
                tool["inputSchema"]["properties"].get("member").is_none(),
                "{name} names a member"
            );
        }
    }
    let refused = use_tool(
        &hq,
        &zed,
        "forget",
        &json!({ "note": first, "reason": "wrong" }),
    )
    .unwrap_err();
    assert!(refused.contains("not in your notebook"), "{refused}");
    assert_eq!(hq.store().notes(t.rae).unwrap().len(), 1);
    let (rae, _) = raes_turn(&hq, &t, "Look at the proxy");
    let refused = use_tool(&hq, &rae, "forget", &json!({ "note": first })).unwrap_err();
    assert!(refused.contains("why"), "{refused}");
    let said = use_tool(
        &hq,
        &rae,
        "forget",
        &json!({ "note": format!("n{first}"), "reason": "it was a one-off" }),
    )
    .unwrap();
    assert!(said.contains("forgotten"), "{said}");
    assert!(hq.store().notes(t.rae).unwrap().is_empty());
    let events = hq.store().events_after(0).unwrap();
    let gone = events
        .iter()
        .find(|e| e.kind == "note.forgotten")
        .expect("said in the feed");
    assert_eq!(gone.data["reason"], "it was a one-off");
    assert_eq!(gone.data["text"], "The proxy restarts clean.");
}

#[test]
fn the_notebook_is_in_the_briefing_newest_first_capped_and_summed_up() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    for n in 1..=15 {
        kept_note(&hq, &t, &format!("Note number {n}."), START + n);
    }
    let (rae, _) = raes_turn(&hq, &t, "Look at the proxy");
    let told = &rae.told;
    assert!(told.contains("Your notes (15):"), "{told}");
    assert!(
        told.find("n15: Note number 15.").unwrap() < told.find("n14: Note number 14.").unwrap(),
        "{told}"
    );
    assert!(told.contains("n4: Note number 4."), "{told}");
    assert!(!told.contains("n3: Note number 3."), "twelve only: {told}");
    assert!(
        told.contains("and 3 older notes: use read_notebook"),
        "{told}"
    );
    // read_notebook returns all of them, with their evidence.
    let all = use_tool(&hq, &rae, "read_notebook", &json!({})).unwrap();
    assert!(
        all.contains("n1: Note number 1.") && all.contains("n15: Note number 15."),
        "{all}"
    );
    assert!(all.contains("task 1"), "{all}");
    // Long notes run out of room first.
    let hq2_clock = Hands(AtomicI64::new(START));
    let hq2 = engine(&hq2_clock);
    let t2 = team(&hq2);
    for n in 1..=12 {
        kept_note(&hq2, &t2, &format!("{n} {}", "y".repeat(280)), START + n);
    }
    let (rae2, _) = raes_turn(&hq2, &t2, "Look at the proxy");
    let shown = rae2.told.matches("- n").count();
    assert!((1..=4).contains(&shown), "{shown}: {}", rae2.told);
    assert!(
        rae2.told.contains("older notes: use read_notebook"),
        "{}",
        rae2.told
    );
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
fn a_tidy_is_asked_a_week_on_and_forgetting_or_a_check_ends_it() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let first = kept_note(&hq, &t, "The proxy restarts clean.", START);
    kept_note(&hq, &t, "The router logs to the NAS.", START);
    let (early, _) = raes_turn(&hq, &t, "Look at the proxy");
    assert!(!early.told.contains("Tidy your notes"), "{}", early.told);
    clock.0.store(START + 8 * DAY, Ordering::SeqCst);
    let (due, _) = raes_turn(&hq, &t, "Look at the router");
    assert!(
        due.told
            .contains("Tidy your notes: forget any that are wrong or no longer true (cite why)."),
        "{}",
        due.told
    );
    // Asked once a day, not on every turn.
    let (again, _) = raes_turn(&hq, &t, "Look at the DNS");
    assert!(!again.told.contains("Tidy your notes"), "{}", again.told);
    // Forgetting is a tidy: a week of quiet follows.
    clock.0.store(START + 10 * DAY, Ordering::SeqCst);
    use_tool(
        &hq,
        &again,
        "forget",
        &json!({ "note": first, "reason": "wrong" }),
    )
    .unwrap();
    clock.0.store(START + 16 * DAY, Ordering::SeqCst);
    let (soon, _) = raes_turn(&hq, &t, "Look at the printer");
    assert!(!soon.told.contains("Tidy your notes"), "{}", soon.told);
    clock.0.store(START + 18 * DAY, Ordering::SeqCst);
    let (due2, _) = raes_turn(&hq, &t, "Look at the switch");
    assert!(due2.told.contains("Tidy your notes"), "{}", due2.told);
    // Saying nothing needs changing is a tidy too.
    let said = use_tool(&hq, &due2, "notebook_checked", &json!({})).unwrap();
    assert!(said.contains("checked"), "{said}");
    clock.0.store(START + 20 * DAY, Ordering::SeqCst);
    let (after, _) = raes_turn(&hq, &t, "Look at the modem");
    assert!(!after.told.contains("Tidy your notes"), "{}", after.told);
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
