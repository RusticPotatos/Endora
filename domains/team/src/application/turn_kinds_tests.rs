//! Turns with kinds (ADR 0131), end to end over HQ's real SQLite store and the simulated
//! runner: where a turn runs is decided from why it was asked for, a turn that cannot run where
//! it is allowed waits, and nothing a wake asked for is thrown away. The owner's words are in
//! `features/the-two-brains.feature`; these are the edges.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, EngineError, Settings};
use super::ports::{Clock, NewTask, Run, TeamStore};
use crate::domain::money::{DOLLAR, MOST_A_TURN};
use crate::domain::{
    Brain, GoalId, MemberId, Priority, Receipt, Refusal, Role, RunStatus, TaskId, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
impl Hands {
    fn forward(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000; // 2026-10-16 12:00 UTC
const DAY: i64 = 24 * 60 * 60 * 1000;

fn hands() -> Hands {
    Hands(AtomicI64::new(NOON))
}

/// An HQ whose turns have kinds, with a pool of `per_day` dollars a day.
fn hq(clock: &Hands, per_day: i64) -> Hq<'_> {
    let mut hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            pool_per_month: 30 * per_day * DOLLAR,
            ..Settings::default()
        },
    );
    hq.set_turn_kinds(true);
    hq
}

struct Team {
    lead: MemberId,
    dev: MemberId,
    reviewer: MemberId,
    researcher: MemberId,
    goal: GoalId,
}

/// A lead, a developer, a reviewer and a researcher, each hired on the house's model (so it
/// works with it) and given a Claude to judge with, and a goal.
fn team(hq: &Hq<'_>) -> Team {
    let hired = |name: &str, role: Role, claude: &str| {
        let id = hq.hire(name, role, role.share(), 2, 0).unwrap();
        hq.set_judgment_brain(id, claude).unwrap();
        id
    };
    Team {
        lead: hired("Ada", Role::Lead, "claude-sonnet-5-5"),
        dev: hired("Linus", Role::Developer, "claude-sonnet-5-5"),
        reviewer: hired("Grace", Role::Reviewer, "claude-sonnet-5-5"),
        researcher: hired("Sam", Role::Researcher, "claude-haiku-5-5"),
        goal: hq
            .set_goal("Keep the lab healthy", "you asked", Author::Owner)
            .unwrap(),
    }
}

fn task(hq: &Hq<'_>, goal: GoalId, title: &str) -> TaskId {
    hq.add_task(
        Author::Endora,
        NewTask {
            goal,
            project: None,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn runs_of(hq: &Hq<'_>, member: MemberId) -> Vec<Run> {
    hq.store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.member == member)
        .collect()
}

fn latest(hq: &Hq<'_>, member: MemberId) -> Run {
    runs_of(hq, member).pop().expect("the member has a turn")
}

#[test]
fn a_new_member_keeps_the_brain_it_had_as_its_work_brain_and_a_claude_is_its_judgment_brain() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    let members = hq.store().members().unwrap();
    let ada = members.iter().find(|m| m.id == t.lead).unwrap();
    assert_eq!(ada.judgment_brain(), Some(Brain::Sonnet));
    assert_eq!(
        ada.work_brain,
        Brain::Mac,
        "a member given a Claude and never told where to work, works with the Mac"
    );
    // Taking the Claude away leaves it thinking with its work brain, and judging with no one.
    assert_eq!(hq.set_judgment_brain(t.lead, "none").unwrap(), None);
    let ada = hq.store().members().unwrap().remove(0);
    assert_eq!((ada.brain, ada.judgment_brain()), (Brain::Mac, None));
    // The owner chooses each from its own list, and the work brain it chose is kept when a
    // Claude comes back.
    assert_eq!(hq.set_work_brain(t.lead, "local").unwrap(), Brain::Local);
    hq.set_judgment_brain(t.lead, "claude-haiku-5-5").unwrap();
    let ada = hq.store().members().unwrap().remove(0);
    assert_eq!(
        (ada.work_brain, ada.judgment_brain()),
        (Brain::Local, Some(Brain::Haiku))
    );
    assert_eq!(hq.set_work_brain(t.lead, "mac").unwrap(), Brain::Mac);
    for refused in ["claude-sonnet-5-5", "gpt-5", ""] {
        assert!(matches!(
            hq.set_work_brain(t.lead, refused),
            Err(EngineError::Refused(Refusal::NotABrain { .. }))
        ));
    }
    for refused in ["claude-opus-5-5", "local", "mac", "gpt-5"] {
        assert!(
            matches!(
                hq.set_judgment_brain(t.lead, refused),
                Err(EngineError::Refused(Refusal::NotABrain { .. }))
            ),
            "{refused}"
        );
    }
}

#[test]
fn the_snapshot_lists_both_brains_and_what_each_may_be() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    hq.set_work_brain(t.lead, "mac").unwrap();
    let shown = hq.snapshot().unwrap();
    let ada = shown["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == t.lead.0)
        .unwrap();
    assert_eq!(ada["work_brain"], "mac");
    assert_eq!(ada["judgment_brain"], "claude-sonnet-5-5");
    assert_eq!(
        ada["brain"], "claude-sonnet-5-5",
        "the old field still reads"
    );
    let work: Vec<&str> = ada["work_brains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(work, ["mac", "local"]);
    let judges: Vec<&str> = ada["judgment_brains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        judges,
        ["claude-sonnet-5-5", "claude-haiku-5-5"],
        "never Opus"
    );
    let sam = shown["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == t.researcher.0)
        .unwrap();
    assert_eq!(
        sam["judgment_brains"].as_array().unwrap().len(),
        1,
        "a researcher judges with Haiku only"
    );
}

#[test]
fn a_decision_joins_a_waiting_work_turn_and_makes_it_a_decision_on_claude() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    let work = task(&hq, t.goal, "Cap sonarr's memory");
    // A work turn that needs the Mac, and the Mac is away: it waits.
    hq.set_work_brain(t.lead, "mac").unwrap();
    hq.tell_mac(false, "owner active");
    hq.wake(t.lead, Some(work), WakeReason::CheckDone).unwrap();
    hq.tick().unwrap();
    let waiting = latest(&hq, t.lead);
    assert_eq!(waiting.status, RunStatus::Queued);
    assert!(waiting.end_reason.starts_with("waiting for the Mac"));
    // The lead is asked to decide the same task: one turn, not two, and now a decision.
    assert_eq!(
        hq.wake(t.lead, Some(work), WakeReason::Decide).unwrap(),
        Receipt::Folded
    );
    hq.tick().unwrap();
    assert_eq!(runs_of(&hq, t.lead).len(), 1);
    let run = latest(&hq, t.lead);
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(
        run.brain,
        Some(Brain::Sonnet),
        "decided by Claude though the Mac is away and the work wanted it"
    );
}

#[test]
fn the_mac_takes_one_turn_at_a_time_and_reading_work_goes_to_the_house_model_meanwhile() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    hq.set_work_brain(t.lead, "mac").unwrap();
    hq.set_work_brain(t.reviewer, "mac").unwrap();
    hq.set_work_brain(t.researcher, "mac").unwrap();
    hq.tell_mac(true, "ready");
    let one = task(&hq, t.goal, "Mend the proxy");
    let two = task(&hq, t.goal, "Mend the dns");
    let three = task(&hq, t.goal, "Read the disks");
    // The lead mends what a reviewer sent back: code work, so the Mac.
    hq.wake(t.lead, Some(one), WakeReason::SentBack).unwrap();
    hq.tick().unwrap();
    assert_eq!(latest(&hq, t.lead).brain, Some(Brain::Mac));
    // The reviewer's turn of the same need waits: the Mac takes one at a time.
    hq.wake(t.reviewer, Some(two), WakeReason::SentBack)
        .unwrap();
    hq.tick().unwrap();
    let waiting = latest(&hq, t.reviewer);
    assert_eq!(waiting.status, RunStatus::Queued);
    assert_eq!(
        waiting.end_reason,
        "waiting for the Mac: another turn is using the Mac"
    );
    // The researcher reads the lab: it is allowed on the house's model, and goes there.
    hq.wake(t.researcher, Some(three), WakeReason::Assigned)
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(latest(&hq, t.researcher).brain, Some(Brain::Local));
    // When the Mac's turn ends, the waiting one has it.
    let first = latest(&hq, t.lead);
    hq.runner().finish(first.id, true);
    hq.tick().unwrap();
    assert_eq!(latest(&hq, t.reviewer).brain, Some(Brain::Mac));
}

#[test]
fn the_offices_downtime_and_a_consult_stay_off_the_mac() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    hq.set_work_brain(t.researcher, "mac").unwrap();
    hq.tell_mac(true, "ready");
    // Downtime for a member on Claude is within the owner's allowance, and none is given.
    hq.set_judgment_brain(t.researcher, "none").unwrap();
    assert_eq!(
        hq.wake(t.researcher, None, WakeReason::Downtime).unwrap(),
        Receipt::Queued
    );
    hq.tick().unwrap();
    let run = latest(&hq, t.researcher);
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(
        run.brain,
        Some(Brain::Local),
        "not quality work: not the Mac"
    );
}

#[test]
fn a_member_out_of_its_share_has_its_decision_kept_and_the_owners_allowance_releases_it() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    let work = task(&hq, t.goal, "Add a list");
    // Linus spends his share of $1.00, and as much again, and then his next decision is not
    // thrown away: it waits, and the owner is asked once.
    let waiting = loop {
        hq.wake(t.dev, Some(work), WakeReason::Decide).unwrap();
        hq.tick().unwrap();
        let run = latest(&hq, t.dev);
        if run.status == RunStatus::Queued {
            break run;
        }
        assert!(runs_of(&hq, t.dev).len() < 10, "he never ran out");
        hq.report_usage(run.id, run.reserved).unwrap();
        hq.runner().finish(run.id, true);
        hq.tick().unwrap();
    };
    assert!(
        hq.store()
            .committed(None, crate::domain::money::day_of(NOON, 0))
            .unwrap()
            < DOLLAR,
        "the pool still has money: it was his share that ran out"
    );
    assert!(waiting.end_reason.starts_with("waiting for budget"));
    let asks: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "out_for_day" && d.status == "waiting")
        .collect();
    assert_eq!(asks.len(), 1);
    hq.tick().unwrap();
    assert_eq!(
        hq.store()
            .desk()
            .unwrap()
            .into_iter()
            .filter(|d| d.kind == "out_for_day")
            .count(),
        1,
        "asked once a day, however long it waits"
    );
    // The allowance releases it, and it spends no more than the allowance.
    hq.decide_saying(asks[0].id, "approve", "0.05").unwrap();
    hq.tick().unwrap();
    let released = latest(&hq, t.dev);
    assert_eq!(released.id, waiting.id, "the same turn, kept");
    assert_eq!(released.status, RunStatus::Running);
    assert_eq!(released.reserved, 50_000);
    assert_eq!(released.end_reason, "", "it no longer says it waits");
}

#[test]
fn a_waiting_decision_is_released_when_the_pools_day_turns() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    // Only Ada is asked to judge, and she is out of the pool's day.
    let work = task(&hq, t.goal, "Cap sonarr's memory");
    let waiting = loop {
        hq.wake(t.lead, Some(work), WakeReason::Decide).unwrap();
        hq.tick().unwrap();
        let run = latest(&hq, t.lead);
        if run.status == RunStatus::Queued {
            break run;
        }
        assert!(runs_of(&hq, t.lead).len() < 10, "she never ran out");
        hq.report_usage(run.id, run.reserved).unwrap();
        hq.runner().finish(run.id, true);
        hq.tick().unwrap();
    };
    assert!(
        waiting.end_reason.starts_with("waiting for budget"),
        "{waiting:?}"
    );
    assert_eq!(hq.snapshot().unwrap()["pool"]["waiting"], 1);
    clock.forward(DAY);
    hq.tick().unwrap();
    let released = latest(&hq, t.lead);
    assert_eq!(
        (released.id, released.status),
        (waiting.id, RunStatus::Running)
    );
    assert_eq!(hq.snapshot().unwrap()["pool"]["waiting"], 0);
}

#[test]
fn what_a_work_turn_on_the_house_model_is_told_and_answers_is_the_house_models() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    hq.set_runtime(t.dev, "claude-code").unwrap();
    let work = task(&hq, t.goal, "Look at sonarr");
    // Work first: the house's model, on the worker, though the member is set to Claude Code.
    hq.wake(t.dev, Some(work), WakeReason::Assigned).unwrap();
    hq.tick().unwrap();
    let run = latest(&hq, t.dev);
    assert_eq!(run.brain, Some(Brain::Local));
    let token = hq.runner().started().pop().unwrap().token;
    let (_, as_the_turn) = hq.turn_of(&token).unwrap().unwrap();
    assert_eq!(
        as_the_turn.brain,
        Brain::Local,
        "the node's relay is told so"
    );
    assert!(as_the_turn.brain.house() && !as_the_turn.brain.cloud());
    assert_eq!(
        hq.runtime_of_turn(run.id).unwrap(),
        crate::domain::Runtime::Worker
    );
    assert_eq!(run.reserved, 0, "work costs nothing");
    hq.runner().finish(run.id, true);
    hq.tick().unwrap();
    // A decision next: Claude, so Claude Code may run it.
    hq.wake(t.dev, Some(work), WakeReason::Decide).unwrap();
    hq.tick().unwrap();
    let run = latest(&hq, t.dev);
    assert_eq!(run.brain, Some(Brain::Sonnet));
    assert_eq!(
        hq.runtime_of_turn(run.id).unwrap(),
        crate::domain::Runtime::ClaudeCode
    );
    assert!(run.reserved > 0 && run.reserved <= MOST_A_TURN);
}

#[test]
fn a_tester_reviews_on_claude_and_its_readings_of_a_check_need_the_mac() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    hq.set_work_brain(t.reviewer, "local").unwrap();
    let work = task(&hq, t.goal, "Tidy the deluge role");
    // The verdict is judgment: on Claude, whatever the work brain is.
    hq.wake(t.reviewer, Some(work), WakeReason::ReviewRequested)
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(latest(&hq, t.reviewer).brain, Some(Brain::Sonnet));
    // Reading the check's result is code work, and this tester works on the house's model:
    // it waits, and says so, rather than being given the model that cannot do it.
    let other = task(&hq, t.goal, "Tidy the radarr role");
    hq.wake(t.reviewer, Some(other), WakeReason::CheckDone)
        .unwrap();
    hq.tick().unwrap();
    let waiting = runs_of(&hq, t.reviewer).pop().unwrap();
    assert_eq!(waiting.status, RunStatus::Queued);
    assert!(
        waiting
            .end_reason
            .starts_with("waiting for the Mac: this work needs the Mac")
    );
}

#[test]
fn with_turn_kinds_off_a_wake_out_of_budget_is_still_turned_away_as_before() {
    let clock = hands();
    let mut hq = hq(&clock, 1);
    hq.set_turn_kinds(false);
    let t = team(&hq);
    let work = task(&hq, t.goal, "Add a list");
    // Without kinds every turn runs on the member's one brain, and the budget refuses a wake.
    let mut refused = false;
    for _ in 0..6 {
        let woke = hq.wake(t.dev, Some(work), WakeReason::Mentioned).unwrap();
        if woke != Receipt::Queued {
            refused = true;
            break;
        }
        hq.tick().unwrap();
        let run = latest(&hq, t.dev);
        hq.report_usage(run.id, run.reserved).unwrap();
        hq.runner().finish(run.id, true);
        hq.tick().unwrap();
    }
    assert!(refused, "the old rule still stands without kinds");
}
