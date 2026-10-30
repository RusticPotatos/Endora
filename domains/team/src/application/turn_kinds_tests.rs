//! Turns with kinds (ADR 0131), end to end over HQ's real SQLite store and the simulated
//! runner: where a turn runs is decided from why it was asked for, a turn that cannot run where
//! it is allowed waits, and nothing a wake asked for is thrown away. The owner's words are in
//! `features/the-two-brains.feature`; these are the edges.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use super::engine::{Author, Engine, EngineError, Fired, STRANDED_AFTER_MS, Settings};
use super::ports::{Clock, NewRoutine, NewTask, Run, TeamStore};
use crate::application::desk_ports::DeskStore;
use crate::domain::guards::Stopped;
use crate::domain::money::{DOLLAR, MOST_A_TURN};
use crate::domain::{
    Brain, GoalId, MemberId, Priority, Receipt, Refusal, Role, RunStatus, TaskId, TaskStatus,
    WakeReason,
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
    dev: MemberId,
    reviewer: MemberId,
    researcher: MemberId,
    goal: GoalId,
}

/// A lead, a developer, a reviewer and a researcher, hired as they are, and a goal: each judges
/// with Haiku and works with the Mac, the defaults.
fn team(hq: &Hq<'_>) -> Team {
    let hired = |name: &str, role: Role| hq.hire(name, role, role.share(), 2, 0).unwrap();
    Team {
        lead: hired("Ada", Role::Lead),
        dev: hired("Linus", Role::Developer),
        reviewer: hired("Grace", Role::Reviewer),
        researcher: hired("Sam", Role::Researcher),
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

fn member(hq: &Hq<'_>, id: MemberId) -> crate::domain::Member {
    hq.store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

/// `member` decides `task` over and over, each turn spending all it was allowed, until a
/// decision waits for budget instead of starting. Returns the turn that waits.
fn spend_the_day(hq: &Hq<'_>, member: MemberId, task: TaskId) -> Run {
    loop {
        hq.wake(member, Some(task), WakeReason::Decide).unwrap();
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
fn a_new_member_judges_with_haiku_and_works_with_the_mac_until_the_owner_says_otherwise() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    let ada = member(&hq, t.lead);
    assert_eq!(
        (ada.judgment_brain(), ada.work_brain),
        (Brain::Haiku, Brain::Mac)
    );
    // The CEO, who may not think with Haiku, judges with Opus as it is seated.
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    assert_eq!(member(&hq, ceo).judgment_brain(), Brain::Opus);
    // The owner chooses each from its own list, and each keeps the other as it was.
    assert_eq!(
        hq.set_judgment_brain(t.lead, "claude-sonnet-5-5").unwrap(),
        Brain::Sonnet
    );
    assert_eq!(hq.set_work_brain(t.lead, "local").unwrap(), Brain::Local);
    let ada = member(&hq, t.lead);
    assert_eq!(
        (ada.judgment_brain(), ada.work_brain),
        (Brain::Sonnet, Brain::Local)
    );
    // The old way of naming one brain puts it where it belongs.
    assert_eq!(hq.set_brain(t.lead, "mac").unwrap(), Brain::Mac);
    assert_eq!(
        hq.set_brain(t.lead, "claude-haiku-5-5").unwrap(),
        Brain::Haiku
    );
    let ada = member(&hq, t.lead);
    assert_eq!(
        (ada.judgment_brain(), ada.work_brain),
        (Brain::Haiku, Brain::Mac)
    );
    // Every member has a Claude to judge with, so there is no taking it away.
    for refused in ["none", "local", "mac", "gpt-5", ""] {
        assert!(
            matches!(
                hq.set_judgment_brain(t.lead, refused),
                Err(EngineError::Refused(Refusal::NotABrain { .. }))
            ),
            "{refused:?}"
        );
    }
    for refused in ["claude-sonnet-5-5", "none", "gpt-5", ""] {
        assert!(
            matches!(
                hq.set_work_brain(t.lead, refused),
                Err(EngineError::Refused(Refusal::NotABrain { .. }))
            ),
            "{refused:?}"
        );
    }
    // Sonnet is the CEO's cheapest, and the researcher's only Haiku.
    assert!(hq.set_judgment_brain(ceo, "claude-haiku-5-5").is_err());
    assert!(
        hq.set_judgment_brain(t.researcher, "claude-sonnet-5-5")
            .is_err()
    );
}

#[test]
fn an_owner_hire_names_either_brain_or_both_and_the_other_has_its_default() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let both = |id| {
        let m = member(&hq, id);
        (m.judgment_brain(), m.work_brain)
    };
    // On the house's model, as owners have always hired: it works there and judges with Haiku.
    let on_the_nas = hq
        .hire_by_owner("Jane Doe", Role::Developer, "local", None)
        .unwrap();
    assert_eq!(both(on_the_nas), (Brain::Haiku, Brain::Local));
    // On Claude: it judges there and works with the Mac.
    let on_sonnet = hq
        .hire_by_owner("John Doe", Role::Lead, "claude-sonnet-5-5", None)
        .unwrap();
    assert_eq!(both(on_sonnet), (Brain::Sonnet, Brain::Mac));
    // Each named apart.
    let apart = hq
        .hire_by_owner_with(
            "Jane Roe",
            Role::Researcher,
            None,
            Some("local"),
            Some("claude-haiku-5-5"),
            None,
        )
        .unwrap();
    assert_eq!(both(apart), (Brain::Haiku, Brain::Local));
    // A brain in the wrong slot is refused, and nobody is hired for it.
    let before = hq.store().members().unwrap().len();
    for (work, judgment) in [
        (Some("claude-haiku-5-5"), None),
        (None, Some("local")),
        (None, Some("gpt-5")),
    ] {
        assert!(matches!(
            hq.hire_by_owner_with("John Roe", Role::Developer, None, work, judgment, None),
            Err(EngineError::Refused(Refusal::NotABrain { .. }))
        ));
    }
    assert_eq!(hq.store().members().unwrap().len(), before);
}

#[test]
fn the_snapshot_lists_both_brains_and_what_each_may_be() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    hq.set_judgment_brain(t.lead, "claude-sonnet-5-5").unwrap();
    let shown = hq.snapshot().unwrap();
    let find = |id: MemberId| {
        shown["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == id.0)
            .unwrap()
            .clone()
    };
    let ada = find(t.lead);
    assert_eq!(ada["work_brain"], "mac");
    assert_eq!(ada["judgment_brain"], "claude-sonnet-5-5");
    assert_eq!(
        ada["brain"], "claude-sonnet-5-5",
        "the old field is the one it judges with"
    );
    let names = |list: &serde_json::Value| -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|b| b["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&ada["work_brains"]), ["mac", "local"]);
    assert_eq!(
        names(&ada["judgment_brains"]),
        [
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "claude-haiku-5-5",
            "deepseek-flash"
        ],
        "Opus too, since one team (ADR 0142)"
    );
    let sam = find(t.researcher);
    assert_eq!(
        names(&sam["judgment_brains"]),
        ["claude-haiku-5-5", "deepseek-flash"],
        "a researcher judges with Haiku or DeepSeek"
    );
    assert_eq!(sam["judgment_brain"], "claude-haiku-5-5");
}

#[test]
fn a_decision_joins_a_waiting_work_turn_and_makes_it_a_decision_on_claude() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    hq.set_judgment_brain(t.lead, "claude-sonnet-5-5").unwrap();
    let work = task(&hq, t.goal, "Cap sonarr's memory");
    // A work turn that needs the Mac, and the Mac is away: it waits.
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
fn a_consult_and_the_offices_downtime_are_free_and_stay_off_the_mac() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    hq.tell_mac(true, "ready");
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
    assert_eq!(run.reserved, 0, "and nobody pays for it");
}

#[test]
fn a_member_out_of_its_share_has_its_decision_kept_and_the_owners_allowance_releases_it() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    let work = task(&hq, t.goal, "Add a list");
    // Linus spends his share of $1.00, and as much again, and then his next decision is not
    // thrown away: it waits, and the owner is asked once.
    let waiting = spend_the_day(&hq, t.dev, work);
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
    let waiting = spend_the_day(&hq, t.lead, work);
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
    assert_eq!(run.brain, Some(Brain::Haiku));
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
    assert_eq!(latest(&hq, t.reviewer).brain, Some(Brain::Haiku));
    // Reading the check's result is code work: the Mac is its floor. This tester's own work
    // brain is the house's model, but the floor is a minimum, not her ceiling: the reading runs
    // on the Mac, never on the model that cannot do it, and never waits forever on her setting.
    let other = task(&hq, t.goal, "Tidy the radarr role");
    hq.wake(t.reviewer, Some(other), WakeReason::CheckDone)
        .unwrap();
    hq.tick().unwrap();
    let reading = runs_of(&hq, t.reviewer).pop().unwrap();
    assert_ne!(reading.brain, Some(Brain::Local), "{reading:?}");
    assert!(
        reading.brain == Some(Brain::Mac)
            || (reading.status == RunStatus::Queued
                && reading.end_reason.starts_with("waiting for the Mac: ")
                && !reading.end_reason.contains("work brain")),
        "{reading:?}"
    );
}

#[test]
fn an_alarm_out_of_budget_runs_at_once_on_a_house_model_and_the_owner_is_told_no_decision_is() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    let spent_on = task(&hq, t.goal, "Earlier decisions");
    let decision = spend_the_day(&hq, t.lead, spent_on);
    assert!(decision.end_reason.starts_with("waiting for budget"));
    // An alarm about another task does not wait: the Mac is away, so the NAS's model takes it.
    let disk = task(&hq, t.goal, "Disk almost full");
    assert_eq!(
        hq.wake(t.lead, Some(disk), WakeReason::Alarm).unwrap(),
        Receipt::Queued
    );
    hq.tick().unwrap();
    let alarm = latest(&hq, t.lead);
    assert_eq!(alarm.task, Some(disk));
    assert_eq!(alarm.status, RunStatus::Running);
    assert_eq!(alarm.brain, Some(Brain::Local));
    assert_eq!(alarm.reserved, 0, "no money was left, and none was asked");
    // The owner is told, once, on the desk.
    let notices = |hq: &Hq<'_>| -> Vec<String> {
        hq.store()
            .desk()
            .unwrap()
            .into_iter()
            .filter(|d| d.kind == "notice" && d.task == Some(disk))
            .map(|d| d.text)
            .collect()
    };
    let said = notices(&hq);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("Ada's alarm turn"), "{}", said[0]);
    assert!(said[0].contains("not on Claude"), "{}", said[0]);
    assert!(
        said[0].contains("the house's own model on the NAS"),
        "{}",
        said[0]
    );
    hq.tick().unwrap();
    assert_eq!(notices(&hq).len(), 1, "told once for a turn");
    // The decision that was waiting is still waiting: nothing but an alarm falls back.
    let still = hq.store().run(decision.id).unwrap().unwrap();
    assert_eq!(still.status, RunStatus::Queued);
    assert!(still.end_reason.starts_with("waiting for budget"));
    // With the Mac free, the next alarm goes there.
    hq.runner().finish(alarm.id, true);
    hq.tick().unwrap();
    hq.tell_mac(true, "ready");
    let fire = task(&hq, t.goal, "Container down");
    hq.wake(t.lead, Some(fire), WakeReason::Alarm).unwrap();
    hq.tick().unwrap();
    assert_eq!(latest(&hq, t.lead).brain, Some(Brain::Mac));
    assert!(
        notices(&hq).len() == 1,
        "that notice is about another task: {:?}",
        notices(&hq)
    );
    // An alarm that joins a decision waiting for budget does not let the decision run on the
    // house: the turn waits as a decision does.
    assert_eq!(
        hq.wake(t.lead, Some(spent_on), WakeReason::Alarm).unwrap(),
        Receipt::Folded
    );
    hq.tick().unwrap();
    let joined = hq.store().run(decision.id).unwrap().unwrap();
    assert_eq!(joined.status, RunStatus::Queued);
    assert!(joined.end_reason.starts_with("waiting for budget"));
}

#[test]
fn an_alarm_with_money_is_judged_on_claude_like_any_decision() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    let disk = task(&hq, t.goal, "Disk almost full");
    hq.wake(t.lead, Some(disk), WakeReason::Alarm).unwrap();
    hq.tick().unwrap();
    let alarm = latest(&hq, t.lead);
    assert_eq!(alarm.brain, Some(Brain::Haiku));
    assert!(alarm.reserved > 0);
    let notices = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "notice")
        .count();
    assert_eq!(notices, 0, "nothing fell back, so nothing is said");
}

#[test]
fn an_alarm_that_makes_the_routines_task_is_an_emergency_like_one_passed_into_an_open_task() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let t = team(&hq);
    hq.add_routine(
        Author::Owner,
        NewRoutine {
            title: "Check the home lab".to_owned(),
            description: String::new(),
            goal: t.goal,
            project: None,
            assignee: t.lead,
            every_ms: 6 * 60 * 60 * 1000,
            on_lab_trouble: true,
        },
    )
    .unwrap();
    let spent_on = task(&hq, t.goal, "Earlier decisions");
    spend_the_day(&hq, t.lead, spent_on);
    // The lab's alarm makes the routine's task and wakes its lead for it, the money gone: the
    // turn does not wait, as the decision still waiting for budget does.
    let (_, fired) = hq
        .alarm("deluge exited at 23:43")
        .unwrap()
        .expect("a routine takes the lab's alarms");
    let Fired::Created(made) = fired else {
        panic!("the alarm made no task: {fired:?}");
    };
    hq.tick().unwrap();
    let alarm = runs_of(&hq, t.lead)
        .into_iter()
        .find(|r| r.task == Some(made))
        .expect("the alarm's turn");
    assert_eq!(alarm.status, RunStatus::Running);
    assert_eq!(alarm.brain, Some(Brain::Local), "the Mac is away");
    assert_eq!(alarm.reserved, 0);
    let told: Vec<String> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "notice" && d.task == Some(made))
        .map(|d| d.text)
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("alarm turn"), "{}", told[0]);
}

#[test]
fn an_alarm_that_makes_the_routines_task_is_judged_on_claude_when_there_is_money() {
    let clock = hands();
    let hq = hq(&clock, 5);
    let t = team(&hq);
    hq.add_routine(
        Author::Owner,
        NewRoutine {
            title: "Check the home lab".to_owned(),
            description: String::new(),
            goal: t.goal,
            project: None,
            assignee: t.lead,
            every_ms: 6 * 60 * 60 * 1000,
            on_lab_trouble: true,
        },
    )
    .unwrap();
    let (_, fired) = hq.alarm("deluge exited at 23:43").unwrap().unwrap();
    let Fired::Created(made) = fired else {
        panic!("the alarm made no task: {fired:?}");
    };
    hq.tick().unwrap();
    let alarm = runs_of(&hq, t.lead)
        .into_iter()
        .find(|r| r.task == Some(made))
        .expect("the alarm's turn");
    assert_eq!(alarm.brain, Some(Brain::Haiku));
    assert!(alarm.reserved > 0);
}

#[test]
fn work_whose_turn_waits_for_budget_is_held_never_stuck_and_nothing_climbs() {
    let clock = hands();
    let hq = hq(&clock, 1);
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    let t = team(&hq);
    // Ada leads a project, so stuck work in it would climb to her, a huddle and the CEO.
    let project = hq
        .add_project(t.goal, "Keep the disks", "a project", t.lead, None)
        .unwrap();
    let own = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: t.goal,
                project: Some(project),
                parent: None,
                title: "Cap sonarr's memory".to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    // Ada is out of her day's money...
    let other = task(&hq, t.goal, "Earlier decisions");
    spend_the_day(&hq, t.lead, other);
    // ...and the decision about the task she holds waits for budget.
    hq.claim(t.lead, own).unwrap();
    assert_eq!(status_of(&hq, own), TaskStatus::Doing);
    hq.wake(t.lead, Some(own), WakeReason::Decide).unwrap();
    hq.tick().unwrap();
    let waiting = latest(&hq, t.lead);
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
            WakeReason::Recovery | WakeReason::AssignmentRecovery | WakeReason::Decide
        ) || w.task != Some(own)),
        "nothing woke anyone for it: {after:?}"
    );
    assert!(
        after.iter().all(|w| w.member != ceo),
        "nothing climbed to the CEO: {after:?}"
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
