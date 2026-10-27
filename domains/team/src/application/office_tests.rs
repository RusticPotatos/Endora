//! The open office (ADR 0119): the office as data seeded once, proposals the team decides
//! together by votes or by a quiet six hours, the owner's undo, reset and veto, and downtime:
//! a turn with no task for a member with nothing to do, on the NAS's own model, free (ADR 0131).

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use crate::domain::office;
use crate::domain::{GoalId, MemberId, Priority, Receipt, Refused, Role, WakeReason, money};
use crate::infrastructure::simulated::Script;
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
const HOUR: i64 = 60 * 60 * 1000;
const DAY: i64 = 24 * HOUR;

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            downtime: true,
            ..Settings::default()
        },
    )
}

/// The team: the CEO, a lead, a developer, a researcher and the tester, all on the house's
/// model but the CEO, with nothing to do.
struct Team {
    ceo: MemberId,
    ada: MemberId,
    bo: MemberId,
    cy: MemberId,
    dee: MemberId,
}

fn team(hq: &Hq<'_>) -> Team {
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let bo = hq.hire("Bo", Role::Developer, 40, 2, 4).unwrap();
    let cy = hq.hire("Cy", Role::Researcher, 20, 1, 1).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 1, 2).unwrap();
    Team {
        ceo,
        ada,
        bo,
        cy,
        dee,
    }
}

fn run_of(hq: &Hq<'_>, context: &RunContext) -> crate::application::ports::Run {
    hq.store().run(context.run).unwrap().unwrap()
}

/// Ends every turn the runner has running, as a worker finishing.
fn end_all(hq: &Hq<'_>) {
    for context in hq.runner().started() {
        hq.runner().finish(context.run, true);
    }
    hq.tick().unwrap();
}

fn items(hq: &Hq<'_>) -> Vec<office::Item> {
    hq.store().office_items().unwrap()
}

// ---- the office as data ------------------------------------------------------------------

#[test]
fn the_office_is_seeded_once_from_the_default_layout() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    assert!(items(&hq).is_empty(), "nothing before HQ seeds it");
    assert!(hq.seed_office().unwrap());
    let seeded = items(&hq);
    assert_eq!(seeded.len(), office::default_layout().len());
    assert!(seeded.iter().all(|i| i.placed_by == "endora"));
    // Again: nothing is added, and what the team removed does not come back.
    assert!(!hq.seed_office().unwrap());
    hq.store().remove_office_item(seeded[0].id).unwrap();
    assert!(!hq.seed_office().unwrap());
    assert_eq!(items(&hq).len(), seeded.len() - 1);
}

// ---- deciding together ---------------------------------------------------------------------

// ---- the owner is the landlord ---------------------------------------------------------------

// ---- downtime --------------------------------------------------------------------------------

fn downtime_turns(hq: &Hq<'_>) -> Vec<RunContext> {
    let wakes = hq.store().wakes().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .filter(|c| {
            let run = run_of(hq, c);
            wakes
                .iter()
                .any(|w| w.id == run.wake && w.reason == WakeReason::Downtime)
        })
        .collect()
}

#[test]
fn downtime_is_a_wake_that_never_reaches_the_ceo() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    assert_eq!(WakeReason::Downtime.name(), "downtime");
    assert_eq!(
        WakeReason::from_name("downtime"),
        Some(WakeReason::Downtime)
    );
    assert_eq!(
        hq.wake(t.ceo, None, WakeReason::Downtime).unwrap(),
        Receipt::Refused(Refused::NotForTheCeo)
    );
}

#[test]
fn a_member_with_nothing_to_do_gets_a_free_downtime_turn_once_a_day_one_at_a_time() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.tick().unwrap();
    let first = downtime_turns(&hq);
    assert_eq!(first.len(), 1, "one downtime turn at a time");
    let run = run_of(&hq, &first[0]);
    assert!(run.task.is_none(), "a downtime turn is about no task");
    assert_eq!(run.reserved, 0, "the house's model costs nothing");
    assert_ne!(first[0].member, t.ceo);
    hq.tick().unwrap();
    assert_eq!(downtime_turns(&hq).len(), 1, "still the one is running");
    // It ends: the next member has its turn, and so on until everyone has had theirs today.
    end_all(&hq);
    assert_eq!(downtime_turns(&hq).len(), 2);
    end_all(&hq);
    end_all(&hq);
    end_all(&hq);
    end_all(&hq);
    let members: std::collections::HashSet<_> =
        downtime_turns(&hq).iter().map(|c| c.member).collect();
    assert_eq!(members, [t.ada, t.bo, t.cy, t.dee].into_iter().collect());
    assert_eq!(downtime_turns(&hq).len(), 4, "once a member a day");
    // The next day they may have another.
    clock.0.store(START + DAY, Ordering::SeqCst);
    end_all(&hq);
    assert!(downtime_turns(&hq).len() > 4);
}

#[test]
fn a_member_with_open_work_a_pause_or_a_furlough_gets_no_downtime() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    let goal: GoalId = hq
        .set_goal("Keep the lab healthy", "", Author::Owner)
        .unwrap();
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: None,
            parent: None,
            title: "Look at the proxy".to_owned(),
            description: String::new(),
            assignee: Some(t.bo),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    hq.pause(t.cy, true).unwrap();
    hq.store().furlough_member(t.dee, START).unwrap();
    hq.runner().script(t.bo, Script::Succeeds);
    hq.runner().script(t.ada, Script::Succeeds);
    for day in 0..3 {
        clock.0.store(START + day * DAY, Ordering::SeqCst);
        for _ in 0..4 {
            end_all(&hq);
        }
    }
    let who: std::collections::HashSet<_> = downtime_turns(&hq).iter().map(|c| c.member).collect();
    assert_eq!(who, [t.ada].into_iter().collect(), "only Ada is free");
}

#[test]
fn never_two_downtime_turns_back_to_back_across_midnight() {
    let clock = Hands(AtomicI64::new(START + 7 * HOUR + 50 * 60 * 1000));
    let hq = engine(&clock);
    let t = team(&hq);
    // Only Bo is free: the others are paused.
    for other in [t.ada, t.cy, t.dee] {
        hq.pause(other, true).unwrap();
    }
    hq.tick().unwrap();
    assert_eq!(downtime_turns(&hq).len(), 1);
    end_all(&hq);
    // Twenty minutes on is a new owner's day, but it is the turn right after.
    clock
        .0
        .store(START + 8 * HOUR + 10 * 60 * 1000, Ordering::SeqCst);
    end_all(&hq);
    assert_eq!(downtime_turns(&hq).len(), 1, "not twice in a row");
    clock.0.store(
        START + 8 * HOUR + 10 * 60 * 1000 + 13 * HOUR,
        Ordering::SeqCst,
    );
    end_all(&hq);
    assert_eq!(downtime_turns(&hq).len(), 2);
}

#[test]
fn downtime_is_work_on_the_nas_for_nothing_whatever_the_member_judges_with() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    // Bo and Cy judge with Haiku and work with the Mac; the Mac is free; Ada and Dee are away.
    // The office is not quality work, so it never uses the Mac (ADR 0131), and it is not paid
    // for, so there is no allowance to ask the owner for (it was ADR 0119's, when Claude did
    // it).
    hq.set_brain(t.bo, "claude-haiku-5-5").unwrap();
    hq.set_brain(t.bo, "mac").unwrap();
    hq.set_brain(t.cy, "claude-haiku-5-5").unwrap();
    hq.tell_mac(true, "ready");
    for away in [t.ada, t.dee] {
        hq.pause(away, true).unwrap();
    }
    end_all(&hq);
    let turns = downtime_turns(&hq);
    assert_eq!(turns.len(), 1, "one at a time, once a day each");
    let run = run_of(&hq, &turns[0]);
    assert_eq!(run.reserved, 0, "it costs nothing");
    assert_eq!(run.brain, Some(crate::domain::Brain::Local));
    assert!(!run.asked_by_owner, "nobody's money, nobody's share");
    let day = money::day_of(START, 0);
    assert_eq!(hq.store().committed(None, day).unwrap(), 0);
    assert!(
        hq.snapshot().unwrap()["office"]["downtime_paid_cents_per_day"].is_null(),
        "there is no allowance to show"
    );
}

#[test]
fn downtime_does_not_count_as_a_turn_on_the_scorecard() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    for member in [t.ada, t.bo, t.cy, t.dee] {
        hq.runner().script(member, Script::Succeeds);
    }
    for day in 0..9 {
        clock.0.store(START + day * DAY, Ordering::SeqCst);
        for _ in 0..5 {
            end_all(&hq);
        }
    }
    assert!(!downtime_turns(&hq).is_empty());
    let snap = hq.snapshot().unwrap();
    assert_eq!(
        snap["scorecard"]["week"]["turns"]["total"], 0,
        "downtime is not work"
    );
}
