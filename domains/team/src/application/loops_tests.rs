//! One team of loops (ADR 0142): members are ranked models, loops hand out work, and an item
//! climbs the ranks when its checks fail. Over HQ's real SQLite store and the simulated runner.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, TeamStore};
use crate::domain::money::DOLLAR;
use crate::domain::{Brain, GoalId, MemberId, Priority, Role, TaskId, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000;

fn hq(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            pool_per_month: 30 * DOLLAR,
            ..Settings::default()
        },
    )
}

fn goal(hq: &Hq<'_>) -> GoalId {
    hq.set_goal("Keep the lab healthy", "you asked", Author::Owner)
        .unwrap()
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

fn brain_of_latest(hq: &Hq<'_>, member: MemberId) -> Option<Brain> {
    hq.store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.member == member)
        .next_back()
        .and_then(|r| r.brain)
}

/// The owner, 2026-10-27: the members are the models. A member's every turn runs on its own
/// model, whatever woke it: work does not go down to the Mac, and judgment does not go up to
/// Claude.
///
/// Given Bo on the house's model and Kit on Sonnet,
/// when Bo is asked a question (judgment) and Kit is given work,
/// then Bo's turn runs on the house's model and Kit's on Sonnet, paid from the pool.
#[test]
fn a_member_is_its_model_whatever_woke_it() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let g = goal(&hq);
    let bo = hq.hire("Bo", Role::Developer, 10, 2, 0).unwrap();
    let kit = hq.hire("Kit", Role::Developer, 10, 2, 0).unwrap();
    assert_eq!(hq.set_model(bo, "local").unwrap(), Brain::Local);
    assert_eq!(hq.set_model(kit, "sonnet").unwrap(), Brain::Sonnet);
    let asked = task(&hq, g, "A question for Bo");
    let given = task(&hq, g, "Work for Kit");
    hq.wake(bo, Some(asked), WakeReason::QuestionAsked).unwrap();
    hq.wake(kit, Some(given), WakeReason::Assigned).unwrap();
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    assert_eq!(brain_of_latest(&hq, bo), Some(Brain::Local));
    assert_eq!(brain_of_latest(&hq, kit), Some(Brain::Sonnet));
    let kits = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .rfind(|r| r.member == kit)
        .unwrap();
    assert!(kits.reserved > 0, "a Claude member's turn is paid for");
}

/// The members are ranked by their model (ADR 0142): Opus first, the house's model last. The
/// snapshot says each member's rank.
#[test]
fn members_are_ranked_by_their_model() {
    let ranks: Vec<u8> = [
        Brain::Opus,
        Brain::Sonnet,
        Brain::Haiku,
        Brain::Mac,
        Brain::Local,
    ]
    .into_iter()
    .map(Brain::rank)
    .collect();
    assert_eq!(ranks, [1, 2, 3, 4, 5]);
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let rae = hq.hire("Rae", Role::Developer, 10, 2, 0).unwrap();
    hq.set_model(rae, "mac").unwrap();
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["members"][0]["rank"], 4, "{}", snap["members"][0]);
}

/// Opus is no longer the CEO's alone: a title is a name only (ADR 0142).
#[test]
fn any_member_may_be_any_model() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let mara = hq.hire("Mara", Role::Developer, 10, 2, 0).unwrap();
    assert_eq!(hq.set_model(mara, "opus").unwrap(), Brain::Opus);
    assert!(hq.set_model(mara, "gpt").is_err(), "not a model Endora has");
}

/// The owner: the eight names are fixed, a generator names any member past eight, and every
/// name is one member's only.
#[test]
fn names_are_the_eight_then_generated_and_never_shared() {
    use crate::domain::names::next_free;
    assert_eq!(next_free(&[]), "Mara");
    assert_eq!(
        next_free(&["Mara", "kit"]),
        "Ada",
        "taken whatever the case"
    );
    let eight = ["Mara", "Kit", "Ada", "Rae", "Bo", "Cy", "Dee", "Zed"];
    let ninth = next_free(&eight);
    assert!(!eight.contains(&ninth.as_str()) && !ninth.is_empty());
    let mut taken: Vec<String> = eight.iter().map(|s| (*s).to_owned()).collect();
    for _ in 0..200 {
        let next = next_free(&taken.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(
            !taken.iter().any(|t| t.eq_ignore_ascii_case(&next)),
            "{next} is taken"
        );
        taken.push(next);
    }
}

/// A member hired with no name gets the next free one.
#[test]
fn a_member_hired_without_a_name_is_given_one() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let first = hq.hire("", Role::Developer, 10, 2, 0).unwrap();
    let names: Vec<String> = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .filter(|m| m.id == first)
        .map(|m| m.name)
        .collect();
    assert_eq!(names, ["Mara"]);
}
