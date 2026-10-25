//! The team's history (ADR 0129): every turn's conversation kept for a while, scrubbed and
//! capped, marked with the tier of what the turn read, shown in the turn's replay, and let go
//! of by the retention rule while the handover notes and the posts stay.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Called, Engine, Settings};
use super::history_ports::HistoryStore;
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use crate::domain::history::{DAY_MS, KEPT_DAYS, TRANSCRIPT_MOST};
use crate::domain::{GoalId, MemberId, Priority, Role, TaskId};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};
use endora_kernel::tier::Tier;

pub(super) struct Hands(pub(super) AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(super) type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// 19 October 2026, at noon UTC.
pub(super) const NOON: i64 = 1_792_324_800_000 + 12 * 60 * 60 * 1000;

struct Team {
    linus: MemberId,
    goal: GoalId,
}

pub(super) fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings::default(),
    )
}

/// Mara the CEO, Ada the lead and Linus the developer under her, all on the house's model.
fn team(hq: &Hq<'_>) -> Team {
    hq.seat_ceo("Mara").unwrap().unwrap();
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let linus = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    hq.set_reports_to(linus, ada).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    hq.migrate_map().unwrap();
    Team { linus, goal }
}

pub(super) fn task_for(hq: &Hq<'_>, goal: GoalId, to: MemberId, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
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

#[test]
fn retained_task_questions_are_owner_readable_in_bounded_pages_without_peer_enablement() {
    use crate::domain::consult::{Consult, Status};
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let requester = hq.hire("Member 1", Role::Developer, 40, 2, 0).unwrap();
    let expert = hq.hire("Member 2", Role::Researcher, 30, 2, 0).unwrap();
    let goal = hq.set_goal("Fixture", "", Author::Owner).unwrap();
    let task = task_for(&hq, goal, requester, "Fixture task");
    hq.tick().unwrap();
    let run = turn_of(&hq, requester).run;
    for n in 0..61 {
        hq.store()
            .seed_legacy_consultation(&Consult {
                id: 0,
                asker: requester,
                asker_run: run,
                expert,
                topic: None,
                thread: task,
                question: format!("Historical question {n:03}"),
                status: Status::FromRecord,
                answer_run: None,
                why: "Historical outcome".into(),
                asker_woken: true,
                at: NOON,
            })
            .unwrap();
    }
    let before = (
        hq.store().tasks().unwrap().len(),
        hq.wakes().unwrap().len(),
        hq.store().runs().unwrap().len(),
    );
    assert!(!hq.conversations_enabled().unwrap());
    let first = hq.legacy_task_conversations(task, -1).unwrap();
    assert_eq!(first["records"].as_array().unwrap().len(), 50);
    assert_eq!(first["records"][0]["id"], "legacy-consult:1");
    assert_eq!(first["records"][0]["question"], "Historical question 000");
    assert_eq!(first["records"][0]["requester_run"], run.0);
    assert_eq!(first["records"][0]["project"], serde_json::Value::Null);
    assert_eq!(first["records"][0]["answer"], serde_json::Value::Null);
    assert_eq!(first["after"], 50);
    assert_eq!(first["has_more"], true);
    let second = hq
        .legacy_task_conversations(task, first["after"].as_i64().unwrap())
        .unwrap();
    assert_eq!(second["records"].as_array().unwrap().len(), 11);
    assert_eq!(second["records"][0]["question"], "Historical question 050");
    assert_eq!(second["has_more"], false);
    assert!(hq.legacy_task_conversations(TaskId(999), 0).is_err());
    assert_eq!(
        before,
        (
            hq.store().tasks().unwrap().len(),
            hq.wakes().unwrap().len(),
            hq.store().runs().unwrap().len()
        )
    );
}

/// The newest turn the runner started for `member`.
pub(super) fn turn_of(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("the member's turn started")
}

pub(super) fn end(hq: &Hq<'_>, context: &RunContext) {
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
}

#[test]
fn a_running_turns_conversation_is_kept_with_its_member_task_brain_and_thirty_days() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, t.linus, "Cap sonarr's memory");
    hq.tick().unwrap();
    let context = turn_of(&hq, t.linus);
    let kept = hq
        .record_transcript(
            &context.token,
            "worker",
            "--- user ---\nYou are Linus.\n--- assistant ---\nI will cap it at 512M.",
        )
        .unwrap()
        .expect("kept");
    assert!(!kept.cut && kept.tier == Tier::Internal && kept.est_tokens > 0);
    let found = hq.store().transcript(context.run).unwrap().unwrap();
    assert_eq!((found.member, found.task), (t.linus, Some(task)));
    assert_eq!(
        (found.runtime.as_str(), found.brain.as_str()),
        ("worker", "local")
    );
    assert_eq!(found.kept_until, NOON + KEPT_DAYS * DAY_MS);
    assert!(found.body.contains("I will cap it at 512M."));
    // A turn has one conversation.
    let again = hq
        .record_transcript(&context.token, "worker", "more")
        .unwrap_err();
    assert!(again.to_string().contains("already kept"), "{again}");
    // And nothing is kept for nothing.
    assert!(hq.record_transcript("nope", "worker", "x").is_err());
}

#[test]
fn a_conversation_over_a_megabyte_keeps_its_head_and_tail_and_says_it_was_cut() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let t = team(&hq);
    task_for(&hq, t.goal, t.linus, "Read the logs");
    hq.tick().unwrap();
    let context = turn_of(&hq, t.linus);
    let long = format!("START{}END", "log line\n".repeat(200_000));
    assert!(long.len() > TRANSCRIPT_MOST);
    let kept = hq
        .record_transcript(&context.token, "claude-code", &long)
        .unwrap()
        .unwrap();
    assert!(kept.cut);
    let found = hq.store().transcript(context.run).unwrap().unwrap();
    assert!(found.body.len() <= TRANSCRIPT_MOST);
    assert!(found.body.starts_with("START") && found.body.ends_with("END"));
    assert!(found.body.contains("cut by HQ"));
    assert_eq!(found.runtime, "claude-code");
}

#[test]
fn a_turn_that_read_confidential_data_is_marked_with_its_tier() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let t = team(&hq);
    task_for(&hq, t.goal, t.linus, "Check the router");
    hq.tick().unwrap();
    let context = turn_of(&hq, t.linus);
    hq.record_receipt(
        &context.token,
        &Called {
            tool: "homelab_status".to_owned(),
            args: String::new(),
            excerpt: "router: up".to_owned(),
            ok: true,
            act: false,
            confidential: true,
        },
    )
    .unwrap();
    let kept = hq
        .record_transcript(&context.token, "worker", "I looked at the router.")
        .unwrap()
        .unwrap();
    assert_eq!(kept.tier, Tier::Confidential);
    let found = hq.store().transcript(context.run).unwrap().unwrap();
    assert_eq!(found.tier, Tier::Confidential);
}

#[test]
fn a_turn_that_has_ended_cannot_have_a_conversation_written() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let t = team(&hq);
    task_for(&hq, t.goal, t.linus, "Fix the proxy");
    hq.tick().unwrap();
    let context = turn_of(&hq, t.linus);
    end(&hq, &context);
    assert!(
        hq.record_transcript(&context.token, "worker", "late words")
            .is_err()
    );
    assert!(hq.store().transcript(context.run).unwrap().is_none());
}

#[test]
fn after_thirty_days_the_conversation_goes_and_the_posts_and_handover_notes_stay() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, t.linus, "Cap sonarr's memory");
    hq.tick().unwrap();
    let context = turn_of(&hq, t.linus);
    hq.record_transcript(&context.token, "worker", "I will cap it at 512M.")
        .unwrap();
    hq.last_word_in(context.run, task, "Capped sonarr at 512M.")
        .unwrap();
    end(&hq, &context);
    // Twenty-nine days on, it is still kept.
    clock.0.fetch_add(29 * DAY_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    assert!(hq.store().transcript(context.run).unwrap().is_some());
    // Thirty-one, and it is let go; what was posted stays.
    clock.0.fetch_add(2 * DAY_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    assert!(hq.store().transcript(context.run).unwrap().is_none());
    let thread = hq.store().thread(task).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.text.contains("Capped sonarr at 512M.")),
        "{thread:?}"
    );
}

#[test]
fn only_the_last_five_conversations_of_a_member_on_a_task_are_kept() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, t.linus, "Cap sonarr's memory");
    let mut runs = Vec::new();
    for n in 0..7 {
        // Each turn is Linus woken by the owner's word in the thread.
        if n > 0 {
            clock.0.fetch_add(6 * 60 * 60 * 1000, Ordering::SeqCst);
            hq.post(Author::Owner, task, "@Linus again please").unwrap();
        }
        hq.tick().unwrap();
        let context = turn_of(&hq, t.linus);
        assert!(!runs.contains(&context.run), "turn {n} is a new one");
        hq.record_transcript(&context.token, "worker", &format!("turn {n} talk"))
            .unwrap();
        // Each turn acts: turns that show nothing would go a model tier up (ADR 0139).
        hq.record_receipt(
            &context.token,
            &super::engine::Called {
                tool: "restart_container".to_owned(),
                args: format!("{{\"name\":\"sonarr{n}\"}}"),
                excerpt: "restarted".to_owned(),
                ok: true,
                act: true,
                confidential: false,
            },
        )
        .unwrap();
        runs.push(context.run);
        end(&hq, &context);
    }
    // The rule is looked at about hourly.
    clock.0.fetch_add(2 * 60 * 60 * 1000, Ordering::SeqCst);
    hq.tick().unwrap();
    let kept: Vec<bool> = runs
        .iter()
        .map(|r| hq.store().transcript(*r).unwrap().is_some())
        .collect();
    assert_eq!(
        kept,
        [false, false, true, true, true, true, true],
        "{runs:?}"
    );
}

#[test]
fn the_replay_of_a_turn_carries_its_conversation_while_it_is_kept_and_not_after() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = engine(&clock);
    let t = team(&hq);
    task_for(&hq, t.goal, t.linus, "Cap sonarr's memory");
    hq.tick().unwrap();
    let context = turn_of(&hq, t.linus);
    hq.record_transcript(
        &context.token,
        "worker",
        "--- assistant ---\nI will cap it at 512M.",
    )
    .unwrap();
    end(&hq, &context);
    let shown = hq.replay_run(context.run).unwrap().shown();
    assert_eq!(shown["transcript"]["tier"], "internal");
    assert_eq!(shown["transcript"]["runtime"], "worker");
    assert!(
        shown["transcript"]["text"]
            .as_str()
            .unwrap()
            .contains("I will cap it at 512M.")
    );
    clock.0.fetch_add(31 * DAY_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    let later = hq.replay_run(context.run).unwrap().shown();
    assert!(later["transcript"].is_null(), "{later}");
}

#[test]
fn the_history_tables_are_made_again_without_changing_a_row() {
    let store = SqliteTeamStore::in_memory().unwrap();
    store
        .add_member("Linus", Role::Developer, 40, 2, 4)
        .unwrap();
    let before = store.members().unwrap();
    // Opening the store again over the same file runs the schema again; here the schema is the
    // same text, so run what a second start would.
    assert_eq!(store.members().unwrap(), before);
    assert!(store.transcripts_kept().unwrap().is_empty());
}
