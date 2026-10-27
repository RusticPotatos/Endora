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

/// A team of the five, ranked, each one model, and a loop to work in.
struct Five {
    bo: MemberId,
    rae: MemberId,
    ada: MemberId,
    loop_: crate::domain::ProjectId,
    goal: GoalId,
}

fn five(hq: &Hq<'_>) -> Five {
    let g = goal(hq);
    let seat = |name: &str, model: &str, role: Role| {
        let id = hq.hire(name, role, 10, 2, 0).unwrap();
        hq.set_model(id, model).unwrap();
        id
    };
    let mara = seat("Mara", "opus", Role::Ceo);
    seat("Kit", "sonnet", Role::Lead);
    let ada = seat("Ada", "haiku", Role::Lead);
    let rae = seat("Rae", "mac", Role::Developer);
    let bo = seat("Bo", "local", Role::Developer);
    let loop_ = hq
        .add_project(g, "ansible-nas", "the fork's work", mara, None)
        .unwrap();
    Five {
        bo,
        rae,
        ada,
        loop_,
        goal: g,
    }
}

fn item(hq: &Hq<'_>, f: &Five, title: &str, priority: Priority) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: f.goal,
            project: Some(f.loop_),
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: None,
            priority,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn assignee(hq: &Hq<'_>, task: TaskId) -> Option<MemberId> {
    hq.store().task(task).unwrap().unwrap().assignee
}

/// A loop works its plan one item at a time, from the top, and gives it to the member at its
/// starting rank: Bo, on the house's model, unless the loop says otherwise (ADR 0142).
///
/// Given a loop with two items on its plan, the second more urgent,
/// when HQ ticks,
/// then Bo is given the urgent one and woken, and the other waits until it is done.
#[test]
fn a_loop_gives_its_top_item_to_the_member_at_its_starting_rank_one_at_a_time() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let later = item(&hq, &f, "Tidy the README", Priority::Normal);
    let first = item(&hq, &f, "Fix the lint", Priority::High);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, first), Some(f.bo), "the top item, to Bo");
    assert_eq!(assignee(&hq, later), None, "one item at a time");
    assert!(
        hq.wakes()
            .unwrap()
            .iter()
            .any(|w| w.member == f.bo && w.task == Some(first) && w.reason == WakeReason::Assigned),
        "Bo is woken to it"
    );
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, later), None, "still one at a time");
    // Done, the next item goes the same way.
    hq.store()
        .set_task_status(first, crate::domain::TaskStatus::Done)
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, later), Some(f.bo));
}

/// A loop may start higher, for hard work: its items go to the member at that rank.
#[test]
fn a_loop_may_start_at_a_higher_rank() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.set_loop_rank(f.loop_, 3).unwrap();
    let hard = item(&hq, &f, "Find what stops gluetun", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, hard), Some(f.ada), "rank 3 is Ada, on Haiku");
    assert!(hq.set_loop_rank(f.loop_, 0).is_err() && hq.set_loop_rank(f.loop_, 6).is_err());
}

/// With the member at the starting rank paused, the next one up takes the item.
#[test]
fn a_paused_member_s_items_go_one_rank_up() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.pause(f.bo, true).unwrap();
    let next = item(&hq, &f, "Fix the lint", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, next), Some(f.rae));
}

/// Before the members are models, the loops give nothing out: the team works as before.
#[test]
fn without_model_members_no_loop_gives_out_work() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let g = goal(&hq);
    let lead = hq.hire("Ada", Role::Lead, 10, 2, 0).unwrap();
    let p = hq
        .add_project(g, "ansible-nas", "the fork", lead, None)
        .unwrap();
    let t = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: g,
                project: Some(p),
                parent: None,
                title: "Fix the lint".to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, t), None);
}

fn turn_of(hq: &Hq<'_>, member: MemberId) -> super::ports::RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(
    hq: &Hq<'_>,
    context: &super::ports::RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    super::protocol::call(hq, &run, &member, tool, args)
}

fn woken(hq: &Hq<'_>, member: MemberId, task: TaskId, reason: WakeReason) -> bool {
    hq.wakes()
        .unwrap()
        .iter()
        .any(|w| w.member == member && w.task == Some(task) && w.reason == reason)
}

/// The owner, 2026-10-27: no testers; one smarter model checks the work (ADR 0142). Work in
/// review goes to the member one rank up, who is offered the review tool, and a send-back
/// counts as one of the item's failures.
///
/// Given Bo's item, moved to review,
/// when HQ asks for its review,
/// then Rae (one rank up) is woken to it and may review it; Rae's send-back wakes Bo.
#[test]
fn work_in_review_goes_to_the_member_one_rank_up() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "The lint passes.",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    assert!(woken(&hq, f.rae, fix, WakeReason::ReviewRequested));
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, f.rae);
    let (run, member) = hq.turn_of(&rae.token).unwrap().unwrap();
    let offered = super::protocol::tools_in(&hq, &run, &member);
    assert!(
        offered
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "review"),
        "the reviewer one rank up is offered the review tool"
    );
    use_tool(
        &hq,
        &rae,
        "review",
        &serde_json::json!({ "task": fix.0, "verdict": "send_back", "notes": "The lint still fails." }),
    )
    .unwrap();
    assert!(woken(&hq, f.bo, fix, WakeReason::SentBack));
    assert_eq!(hq.loop_failures(fix).unwrap(), 1);
}

/// Three failures move an item up one rank; a failure at the top rank goes to the owner.
///
/// Given Bo's item,
/// when it fails three times, then three more, and so on up the ranks,
/// then it goes to Rae, then Ada, then Kit, then Mara, and Mara's third failure puts it on the
/// owner's desk.
#[test]
fn three_failures_move_an_item_up_a_rank_and_the_top_rank_s_go_to_the_owner() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = item(&hq, &f, "Find what stops gluetun", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, hard), Some(f.bo));
    let names = |id: Option<MemberId>| {
        hq.store()
            .members()
            .unwrap()
            .into_iter()
            .find(|m| Some(m.id) == id)
            .map(|m| m.name)
    };
    for next in ["Rae", "Ada", "Kit", "Mara"] {
        for _ in 0..3 {
            hq.loop_failed(hard).unwrap();
        }
        assert_eq!(names(assignee(&hq, hard)).as_deref(), Some(next));
        assert_eq!(
            hq.loop_failures(hard).unwrap(),
            0,
            "counted afresh at each rank"
        );
    }
    assert!(
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .all(|d| d.task != Some(hard))
    );
    for _ in 0..3 {
        hq.loop_failed(hard).unwrap();
    }
    let desk = hq.store().desk().unwrap();
    assert!(
        desk.iter().any(|d| d.task == Some(hard)
            && d.status == "waiting"
            && d.text.contains("every rank")),
        "{desk:?}"
    );
}
