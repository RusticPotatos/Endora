//! Turns that wait (ADR 0131, amended by 0142), end to end over HQ's real SQLite store and the
//! simulated runner: a member is its model, and a turn that cannot run there now waits, held
//! and never stuck, and is said to the owner when it waits long for the Mac.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use super::engine::{Author, Engine, STRANDED_AFTER_MS, Settings};
use super::ports::{Clock, NewTask, Run, TeamStore};
use crate::application::desk_ports::DeskStore;
use crate::domain::guards::Stopped;
use crate::domain::money::DOLLAR;
use crate::domain::{Brain, GoalId, MemberId, Priority, RunStatus, TaskId, TaskStatus, WakeReason};
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

fn hands() -> Hands {
    Hands(AtomicI64::new(NOON))
}

/// An HQ with a pool of `per_day` dollars a day.
fn hq(clock: &Hands, per_day: i64) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            pool_per_month: 30 * per_day * DOLLAR,
            ..Settings::default()
        },
    )
}

struct Team {
    lead: MemberId,
    reviewer: MemberId,
    goal: GoalId,
}

/// A lead on Claude Haiku, a reviewer on the Mac, and a goal.
fn team(hq: &Hq<'_>) -> Team {
    let hired = |name: &str, share: u8, brain: &str| {
        let id = hq.hire(name, share, 2, 0).unwrap();
        hq.set_brain(id, brain).unwrap();
        id
    };
    Team {
        lead: hired("Ada", 25, "claude-haiku-5-5"),
        reviewer: hired("Grace", 20, "mac"),
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

fn status_of(hq: &Hq<'_>, task: TaskId) -> TaskStatus {
    hq.store().task(task).unwrap().unwrap().status
}

/// `member` decides `task` over and over, each turn spending all it was allowed, until a
/// decision waits for budget instead of starting. Returns the turn that waits.
fn spend_the_day(hq: &Hq<'_>, member: MemberId, task: TaskId) -> Run {
    loop {
        hq.wake(member, Some(task), WakeReason::ProposalDecided)
            .unwrap();
        hq.tick().unwrap();
        let run = latest(hq, member);
        if run.status == RunStatus::Queued {
            return run;
        }
        assert!(runs_of(hq, member).len() < 12, "never ran out of money");
        hq.report_usage(run.id, run.reserved).unwrap();
        hq.runner().finish(run.id, true);
        hq.tick().unwrap();
    }
}

#[test]
fn work_whose_turn_waits_for_budget_is_held_never_stuck_and_nothing_climbs() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    // Ada holds the work, and is out of her day's money: her decision about it waits for budget.
    let own = task(&hq, t.goal, "Cap sonarr's memory");
    hq.claim(t.lead, own).unwrap();
    assert_eq!(status_of(&hq, own), TaskStatus::Doing);
    let waiting = spend_the_day(&hq, t.lead, own);
    assert!(
        waiting.end_reason.starts_with("waiting for budget"),
        "{waiting:?}"
    );
    assert_eq!(waiting.task, Some(own));
    // However long it waits, the work is held: not parked, not stuck, nothing nudged, no rung.
    let before = hq.wakes().unwrap().len();
    for _ in 0..40 {
        clock.forward(STRANDED_AFTER_MS / 2);
        hq.tick().unwrap();
        let found = hq.diagnosis(own).unwrap().expect("open work");
        assert_eq!(found.stopped, Stopped::Held, "{}", found.why);
    }
    assert_eq!(status_of(&hq, own), TaskStatus::Doing, "not blocked");
    let after: Vec<_> = hq.wakes().unwrap().into_iter().skip(before).collect();
    assert!(
        after.iter().all(|w| !matches!(
            w.reason,
            WakeReason::Recovery | WakeReason::AssignmentRecovery
        ) || w.task != Some(own)),
        "nothing woke anyone for it: {after:?}"
    );
    let stuck: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "stuck" || d.kind == "rounds")
        .collect();
    assert!(stuck.is_empty(), "{stuck:?}");
}

#[test]
fn work_whose_turn_waits_for_the_mac_is_held_the_same_way() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    let own = task(&hq, t.goal, "Mend the proxy");
    hq.claim(t.reviewer, own).unwrap();
    hq.tell_mac(false, "owner active");
    hq.wake(t.reviewer, Some(own), WakeReason::SentBack)
        .unwrap();
    hq.tick().unwrap();
    assert!(
        latest(&hq, t.reviewer)
            .end_reason
            .starts_with("waiting for the Mac")
    );
    for _ in 0..30 {
        clock.forward(STRANDED_AFTER_MS / 2);
        hq.tell_mac(false, "owner active");
        hq.tick().unwrap();
        let found = hq.diagnosis(own).unwrap().expect("open work");
        assert_eq!(found.stopped, Stopped::Held, "{}", found.why);
    }
    assert!(
        hq.wakes()
            .unwrap()
            .iter()
            .all(|w| w.reason != WakeReason::Recovery),
        "nobody was nudged"
    );
}

/// The Phase 0 audit's P5, from the live record of 2026-10-19: a tester's turns waited for the
/// Mac for ten hours overnight, and nothing reached the owner. Work that has waited for the Mac
/// past [`crate::domain::mac::LONG_WAIT_MS`] puts one note in the owner's digest, saying who
/// waits, on what, and why; once while the waiting lasts, and again only after it has cleared
/// and begun again. The work still waits for the Mac: it is never moved to another brain.
#[test]
fn work_that_waits_long_for_the_mac_is_said_once_in_the_owners_digest() {
    use crate::application::desk_ports::DeskStore;
    use crate::domain::mac::LONG_WAIT_MS;
    const MINUTE: i64 = 60_000;
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    let notes = |hq: &Hq<'_>| {
        hq.store()
            .desk()
            .unwrap()
            .into_iter()
            .filter(|d| d.kind == "notice" && d.text.contains("waited for the Mac"))
            .collect::<Vec<_>>()
    };
    let wait = |minutes: i64| {
        clock.forward(minutes * MINUTE);
        hq.tell_mac(false, "owner active");
        hq.tick().unwrap();
    };
    let work = task(&hq, t.goal, "Mend the proxy");
    hq.tell_mac(false, "owner active");
    hq.wake(t.reviewer, Some(work), WakeReason::SentBack)
        .unwrap();
    hq.tick().unwrap();
    wait(LONG_WAIT_MS / MINUTE - 30);
    assert!(notes(&hq).is_empty(), "a working day's wait is not news");
    wait(31);
    let said = notes(&hq);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].text.contains("Grace")
            && said[0].text.contains(&format!("#{}", work.0))
            && said[0].text.contains("owner active"),
        "{}",
        said[0].text
    );
    assert!(!crate::domain::desk::interrupts(&said[0].kind));
    let waiting = latest(&hq, t.reviewer);
    assert_eq!(waiting.status, RunStatus::Queued, "the work still waits");
    assert!(crate::domain::mac::is_waiting_words(&waiting.end_reason));
    wait(60);
    assert_eq!(notes(&hq).len(), 1, "once while the waiting lasts");
    // The Mac is back: the turn runs, and the waiting is over.
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    let ran = latest(&hq, t.reviewer);
    assert_eq!(
        (ran.status, ran.brain),
        (RunStatus::Running, Some(Brain::Mac))
    );
    hq.runner().finish(ran.id, true);
    hq.tick().unwrap();
    // Another long wait later is news again.
    let again = task(&hq, t.goal, "Mend the dns");
    hq.tell_mac(false, "on battery");
    hq.wake(t.reviewer, Some(again), WakeReason::SentBack)
        .unwrap();
    hq.tick().unwrap();
    let wait_on_battery = |minutes: i64| {
        clock.forward(minutes * MINUTE);
        hq.tell_mac(false, "on battery");
        hq.tick().unwrap();
    };
    wait_on_battery(LONG_WAIT_MS / MINUTE + 1);
    let said = notes(&hq);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[1].text.contains("on battery"), "{}", said[1].text);
}
