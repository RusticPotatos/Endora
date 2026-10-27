//! Caps on a runaway (ADR 0133), through the engine: a member's day of turns on the house's own
//! models, and the tripwire that stops the team itself. Live, the scripted demo ran 1,539 turns in
//! 3 h 40 min, the free models have no budget to stop a runaway, and only the owner could pull the
//! kill switch.

use std::sync::atomic::AtomicI64;

use crate::application::run_ports::RunStore;

use serde_json::Value;

use super::engine::{Engine, Settings};
use super::guards_tests::{Hands, Hq, NOON, later, one_turn, team};
use super::ports::{Run, TeamStore};
use crate::domain::guards::{Stopped, THRESHOLDS};
use crate::domain::turns;
use crate::domain::{Brain, MemberId, Receipt, Refused, Role, RunStatus, TaskId, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;

/// The turns of the team that wait for the day to turn.
fn capped(hq: &Hq<'_>) -> Vec<Run> {
    hq.store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.status == RunStatus::Queued && turns::is_waiting_for_cap(&r.end_reason))
        .collect()
}

/// How many turns `member` has started.
fn started_by(hq: &Hq<'_>, member: MemberId) -> usize {
    hq.store()
        .runs()
        .unwrap()
        .iter()
        .filter(|r| r.member == member && r.started_at.is_some())
        .count()
}

/// The turns waiting on the owner's desk as the tripwire's.
fn tripwire_cards(hq: &Hq<'_>) -> Vec<Value> {
    hq.snapshot().unwrap()["desk"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["kind"] == "tripwire")
        .cloned()
        .collect()
}

/// Wakes `member` on `task` and lets its turn come and go at once, as a crash loop would: a turn
/// started, and ended before it did anything.
fn quick_turn(hq: &Hq<'_>, member: MemberId, task: TaskId) {
    hq.wake(member, Some(task), WakeReason::Mentioned).unwrap();
    hq.tick().unwrap();
    for run in hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.status == RunStatus::Running)
    {
        hq.runner().finish(run.id, true);
    }
    hq.tick().unwrap();
}

/// Turns come and go every five seconds, alternating between `members`, until the team is
/// stopped or `most` have been tried. Returns how many were tried.
fn runaway(hq: &Hq<'_>, clock: &Hands, members: &[MemberId], task: TaskId, most: usize) -> usize {
    for tried in 0..most {
        if hq.store().stopped().unwrap() {
            return tried;
        }
        quick_turn(hq, members[tried % members.len()], task);
        later(clock, 5_000);
    }
    most
}

#[test]
fn the_sixty_first_turn_of_a_day_on_the_house_waits_for_tomorrow_and_starts_when_the_day_turns() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, linus, task) = team(&clock, false);
    // Sixty turns, two minutes apart: five in ten minutes, nowhere near a runaway, and each on
    // the house's own models, which cost nothing and so are bounded by nothing else.
    // Turns that act: one that shows nothing would be taken a model tier up (ADR 0139).
    for _ in 0..60 {
        one_turn(&hq, linus, task, true);
        later(&clock, 2 * MINUTE);
    }
    assert_eq!(started_by(&hq, linus), 60);
    assert!(
        capped(&hq).is_empty(),
        "the sixtieth is a turn like the rest"
    );
    assert!(!hq.store().stopped().unwrap(), "and not a runaway");

    // The sixty-first is not lost: it is queued, and waits, and says so.
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Queued
    );
    hq.tick().unwrap();
    let waiting = capped(&hq);
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(
        waiting[0].end_reason,
        "waiting for tomorrow: 60 turns a day is the most a member takes on the house's own models"
    );
    assert_eq!(started_by(&hq, linus), 60, "it did not start");

    // It is on the screen where waits are: the top bar, the card of what waits, the roster line.
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["pool"]["capped"], 1);
    assert_eq!(snap["scorecard"]["outcomes"]["waiting"]["cap"], 1);
    assert!(
        snap["runs"].as_array().unwrap().iter().any(|r| {
            r["member"] == linus.0
                && r["status"] == "queued"
                && r["why"]
                    .as_str()
                    .is_some_and(|w| w.starts_with("waiting for tomorrow: "))
        }),
        "{}",
        snap["runs"]
    );
    // A wake that waits is not a stall: the work is held, and nothing climbs for it.
    assert_eq!(
        hq.diagnosis(task).unwrap().map(|d| d.stopped),
        Some(Stopped::Held)
    );
    // Asked again, it is the same one waiting turn.
    hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap();
    hq.tick().unwrap();
    assert_eq!(capped(&hq).len(), 1);

    // The day turns: it starts, and the count begins again.
    later(&clock, 12 * HOUR);
    hq.tick().unwrap();
    assert!(capped(&hq).is_empty());
    assert_eq!(started_by(&hq, linus), 61);
    assert_eq!(hq.snapshot().unwrap()["pool"]["capped"], 0);
}

#[test]
fn one_members_day_is_its_own_and_judgment_is_not_a_turn_on_the_house() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, linus, task) = team(&clock, false);
    let dee = hq.hire("Dee", Role::Developer, 40, 2, 4).unwrap();
    // Turns that act: one that shows nothing would be taken a model tier up (ADR 0139).
    for _ in 0..60 {
        one_turn(&hq, linus, task, true);
        later(&clock, 2 * MINUTE);
    }
    // Dee has taken none: her turn starts as ever.
    one_turn(&hq, dee, task, false);
    assert_eq!(started_by(&hq, dee), 1);
    // New work handed to Linus waits with the rest of his work.
    let goal = hq.store().task(task).unwrap().unwrap().goal;
    let second = super::guards_tests::loose_task(&hq, goal, "Look at the NAS fans", linus, None);
    hq.tick().unwrap();
    let waiting = capped(&hq);
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].task, Some(second));
    // A decision asked of him is judgment, on his Claude, and is not held by the day's count: the
    // wake joins his waiting turn and makes it one of judgment.
    hq.wake(linus, Some(second), WakeReason::Decide).unwrap();
    hq.tick().unwrap();
    assert!(capped(&hq).is_empty(), "{:?}", capped(&hq));
    let run = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .rfind(|r| r.member == linus && r.task == Some(second))
        .unwrap();
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.brain, Some(Brain::Haiku));
}

#[test]
fn forty_turns_in_ten_minutes_stop_the_team_and_the_forty_first_never_starts() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, linus, task) = team(&clock, true);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.role == Role::Lead)
        .unwrap()
        .id;
    let tried = runaway(&hq, &clock, &[ada, linus], task, 100);
    assert!(tried < 100, "it stopped itself");

    // The team is stopped, by the wire, and says so.
    assert!(hq.store().stopped().unwrap());
    let started = hq
        .store()
        .runs()
        .unwrap()
        .iter()
        .filter(|r| r.started_at.is_some())
        .count();
    assert_eq!(started, 40, "the forty-first never started");
    let feed = hq.events_after(0).unwrap();
    let stopped: Vec<_> = feed.iter().filter(|e| e.kind == "stopped").collect();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].data["by"], "tripwire");
    assert_eq!(stopped[0].data["why"], "stopped by the runaway tripwire");
    assert_eq!(stopped[0].data["turns"], 40);
    let counted: i64 = stopped[0].data["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["turns"].as_i64().unwrap())
        .sum();
    assert_eq!(counted, 40, "{}", stopped[0].data);

    // The owner is told once, on the desk, where they are interrupted, with who started what.
    let snap = hq.snapshot().unwrap();
    assert_eq!(
        (snap["stopped"].clone(), snap["stopped_by"].clone()),
        (true.into(), "tripwire".into())
    );
    let cards = tripwire_cards(&hq);
    assert_eq!(cards.len(), 1, "{cards:?}");
    let text = cards[0]["text"].as_str().unwrap();
    assert!(
        text.starts_with(
            "Stopped by the runaway tripwire: the team started 40 turns in 10 minutes"
        ),
        "{text}"
    );
    for member in [ada, linus] {
        let name = hq
            .store()
            .members()
            .unwrap()
            .into_iter()
            .find(|m| m.id == member)
            .unwrap()
            .name;
        assert!(text.contains(&format!("{name} ")), "{name}: {text}");
    }
    // Nothing starts, and every wake is turned away, until it is resumed.
    assert_eq!(
        hq.wake(linus, Some(task), WakeReason::Mentioned).unwrap(),
        Receipt::Refused(Refused::KillSwitch)
    );
    hq.tick().unwrap();
    assert_eq!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .filter(|r| r.started_at.is_some())
            .count(),
        40
    );
}

#[test]
fn the_owner_resumes_a_team_the_wire_stopped_and_what_it_counted_is_not_held_against_it() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, linus, task) = team(&clock, true);
    let ada = hq.store().members().unwrap()[0].id;
    runaway(&hq, &clock, &[ada, linus], task, 100);
    assert!(hq.store().stopped().unwrap());
    let card = tripwire_cards(&hq)[0]["id"].as_i64().unwrap();

    // Resumed straight away, from the desk, it works: the forty turns of a minute ago are the
    // owner's to have looked at, not evidence against what comes next.
    hq.decide(crate::domain::DeskId(card), "resume").unwrap();
    assert!(!hq.store().stopped().unwrap());
    assert!(tripwire_cards(&hq).is_empty(), "the card is answered");
    let settled = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.id.0 == card)
        .unwrap();
    assert_eq!(settled.status, "resumed");
    let before = hq.runner().started().len();
    quick_turn(&hq, linus, task);
    assert_eq!(hq.runner().started().len(), before + 1, "a turn starts");
    assert!(!hq.store().stopped().unwrap());

    // A runaway that goes on after that stops it again, with a card of its own.
    runaway(&hq, &clock, &[ada, linus], task, 100);
    assert!(hq.store().stopped().unwrap());
    assert_eq!(tripwire_cards(&hq).len(), 1);
    // The top bar's Resume does the same, and answers the card.
    hq.resume().unwrap();
    assert!(!hq.store().stopped().unwrap());
    assert!(tripwire_cards(&hq).is_empty());
}

#[test]
fn the_owners_own_kill_switch_is_still_the_owners() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, _linus, _task) = team(&clock, false);
    hq.stop_all("trying it").unwrap();
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["stopped_by"], "owner");
    assert!(tripwire_cards(&hq).is_empty());
    let stopped = hq
        .events_after(0)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "stopped")
        .unwrap();
    assert_eq!(stopped.data["by"], "owner");
    hq.resume().unwrap();
    assert_eq!(hq.snapshot().unwrap()["stopped_by"], Value::Null);
}

/// The scripted demo takes a turn as fast as its tick lets it, and the same team stopped itself:
/// the exemption is what keeps the demo running, not its pace being low (ADR 0133).
#[test]
fn a_scripted_team_is_never_stopped_by_the_wire_and_is_not_held_to_a_day_of_turns() {
    let run = |scripted: bool| {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            SimulatedRunner::new(4),
            &clock,
            Settings {
                scripted,
                ..Settings::default()
            },
        );
        let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
        let linus = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
        let goal = hq
            .set_goal(
                "Keep the home lab healthy",
                "",
                super::engine::Author::Owner,
            )
            .unwrap();
        let task = super::guards_tests::loose_task(&hq, goal, "Plan the backups", ada, None);
        runaway(&hq, &clock, &[ada, linus], task, 150);
        let started = hq
            .store()
            .runs()
            .unwrap()
            .iter()
            .filter(|r| r.started_at.is_some())
            .count();
        (hq.store().stopped().unwrap(), started, capped(&hq).len())
    };
    let (stopped, started, waiting) = run(true);
    assert!(!stopped, "the demo is not stopped");
    assert!(started >= 100, "and it went on: {started}");
    assert_eq!(waiting, 0, "and no member waits for tomorrow");
    let (stopped, started, _) = run(false);
    assert!(stopped, "the same team, not scripted, is stopped");
    assert_eq!(started, THRESHOLDS.runaway_turns);
}

/// A scenario is bounded by its own world (its move limit, its re-wakes), not by its player's
/// day of free turns: live, the model league's black side waited "for tomorrow" mid-game after
/// the night's Holodeck match, and its member could do no real work on the house either
/// (2026-10-26). A scenario's turn starts past the day's cap, and does not count against it.
#[test]
fn a_scenarios_turn_is_bounded_by_its_world_not_by_its_players_day_of_free_turns() {
    let clock = Hands(AtomicI64::new(NOON));
    let (hq, linus, task) = team(&clock, false);
    for _ in 0..60 {
        one_turn(&hq, linus, task, true);
        later(&clock, 2 * MINUTE);
    }
    let goal = hq
        .set_goal("Holodeck", "", super::engine::Author::Owner)
        .unwrap();
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let project = hq.add_project(goal, "Lighthouse", "", lead, None).unwrap();
    hq.set_project_tools(project, Some(vec!["scenario".to_owned()]))
        .unwrap();
    let world = hq.start_scenario(project, linus, 7).unwrap();
    hq.tick().unwrap();
    let played = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .find(|r| r.task == Some(world))
        .expect("a turn on the world");
    assert!(
        played.started_at.is_some(),
        "it starts past the day's cap: {}",
        played.end_reason
    );
    assert!(capped(&hq).is_empty());
}
