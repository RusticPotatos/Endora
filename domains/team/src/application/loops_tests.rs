//! One team of loops (ADR 0142): members are ranked models, loops hand out work, and an item
//! climbs the ranks when its checks fail. Over HQ's real SQLite store and the simulated runner.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::desk_ports::DeskStore;
use crate::application::schedule_ports::ScheduleStore;

use crate::application::run_ports::RunStore;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, TeamStore};
use crate::domain::money::DOLLAR;
use crate::domain::{Brain, GoalId, MemberId, Priority, Role, TaskId, TaskStatus, WakeReason};
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
    hq.wake(bo, Some(asked), WakeReason::Mentioned).unwrap();
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
        Brain::DeepSeek,
        Brain::Mac,
        Brain::Local,
    ]
    .into_iter()
    .map(Brain::rank)
    .collect();
    assert_eq!(ranks, [1, 2, 3, 4, 5, 6]);
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let rae = hq.hire("Rae", Role::Developer, 10, 2, 0).unwrap();
    hq.set_model(rae, "mac").unwrap();
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["members"][0]["rank"], 5, "{}", snap["members"][0]);
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

/// A loop works its plan from the top, and gives it to the member at its starting rank: Bo,
/// on the house's model, unless the loop says otherwise (ADR 0142). Set to one item at a time,
/// it works as it did before items ran side by side.
///
/// Given a loop of one item at a time with two items on its plan, the second more urgent,
/// when HQ ticks,
/// then Bo is given the urgent one and woken, and the other waits until it is done.
#[test]
fn a_loop_gives_its_top_item_to_the_member_at_its_starting_rank_one_at_a_time() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.set_loop_items(f.loop_, 1).unwrap();
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
    assert!(hq.set_loop_rank(f.loop_, 0).is_err() && hq.set_loop_rank(f.loop_, 7).is_err());
}

/// A loop has up to two items in hand at once unless the owner says otherwise (the baseline
/// experiment of 28 Oct: eight tasks in one loop ran one after another while members sat
/// idle). The second goes to a different member, the nearest free one above the first.
///
/// Given a loop with three items on its plan,
/// when HQ ticks,
/// then the top two start together, Bo's and Rae's, both woken, and the third waits until one
/// of them is done.
#[test]
fn two_items_start_together_on_two_members_and_a_third_waits() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let third = item(&hq, &f, "Tidy the README", Priority::Low);
    let second = item(&hq, &f, "Pin the images", Priority::Normal);
    let first = item(&hq, &f, "Fix the lint", Priority::High);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, first), Some(f.bo), "the top item, to Bo");
    assert_eq!(
        assignee(&hq, second),
        Some(f.rae),
        "the next, beside it, to Rae"
    );
    assert_eq!(assignee(&hq, third), None, "two at once, no more");
    assert!(woken(&hq, f.bo, first, WakeReason::Assigned));
    assert!(woken(&hq, f.rae, second, WakeReason::Assigned));
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, third), None, "still two at once");
    hq.store()
        .set_task_status(first, crate::domain::TaskStatus::Done)
        .unwrap();
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, third), Some(f.bo), "Bo is free again");
}

/// The second item never stacks on the member who holds the first: with nobody else free at
/// or near the starting rank, it waits.
#[test]
fn with_no_other_member_free_the_second_item_waits() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.set_loop_rank(f.loop_, 1).unwrap();
    let first = item(&hq, &f, "Find what stops gluetun", Priority::High);
    let second = item(&hq, &f, "Pin the images", Priority::Normal);
    hq.tick().unwrap();
    assert!(assignee(&hq, first).is_some(), "Mara takes the top item");
    assert_eq!(assignee(&hq, second), None, "nobody above Mara: it waits");
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, second), None);
}

/// A paused member, or the Mac's while the Mac is away, is not given the second item: the
/// nearest free member above takes it.
#[test]
fn a_paused_or_away_member_is_not_given_the_second_item() {
    for away in [false, true] {
        let clock = Hands(AtomicI64::new(NOON));
        let hq = hq(&clock);
        let f = five(&hq);
        if away {
            hq.tell_mac(false, "busy");
        } else {
            hq.pause(f.rae, true).unwrap();
        }
        let first = item(&hq, &f, "Fix the lint", Priority::High);
        let second = item(&hq, &f, "Pin the images", Priority::Normal);
        hq.tick().unwrap();
        assert_eq!(assignee(&hq, first), Some(f.bo));
        assert_eq!(
            assignee(&hq, second),
            Some(f.ada),
            "Rae passed over (away: {away})"
        );
    }
}

/// The owner sets how many items a loop has in hand at once, from 1 to 4, kept like its
/// starting rank; two until the owner says.
#[test]
fn the_owner_sets_a_loop_s_items_at_once_from_one_to_four() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    assert_eq!(
        hq.loop_items(f.loop_).unwrap(),
        2,
        "two until the owner says"
    );
    assert!(hq.set_loop_items(f.loop_, 0).is_err());
    assert!(hq.set_loop_items(f.loop_, 5).is_err());
    assert!(
        hq.set_loop_items(crate::domain::ProjectId(9_999), 2)
            .is_err(),
        "no such loop"
    );
    hq.set_loop_items(f.loop_, 3).unwrap();
    assert_eq!(hq.loop_items(f.loop_).unwrap(), 3, "kept");
    assert_eq!(
        hq.store()
            .setting(&format!("loop_items:{}", f.loop_.0))
            .unwrap()
            .as_deref(),
        Some("3"),
        "in the store, so it survives a restart"
    );
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

/// A member paused while it holds an item lets it go one rank up, so the item does not wait
/// for it (owner, 30 Oct: the Mac is the owner's laptop, needed for the evening).
///
/// Given an item Bo holds,
/// when the owner pauses Bo,
/// then once Bo's running turn on it ends, the item is Rae's, one rank up, and Rae is woken.
#[test]
fn a_paused_member_lets_its_items_go_one_rank_up() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let held = item(&hq, &f, "Fix the lint", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, held), Some(f.bo));
    hq.pause(f.bo, true).unwrap();
    // While Bo's turn on it runs, it stays; once that turn ends, it goes up.
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, held), Some(f.bo), "not mid-turn");
    for c in hq.runner().started() {
        hq.runner().finish(c.run, true);
    }
    hq.tick().unwrap();
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, held), Some(f.rae));
    assert!(woken(&hq, f.rae, held, WakeReason::Assigned));
    let now = hq.store().task(held).unwrap().unwrap();
    assert_eq!(now.owner, None, "not left claimed by the paused member");
}

/// The owner's laptop is the Mac: when the node says it is not free (busy, in use, on battery,
/// or not heard from), the Mac's member is passed over and its items go one rank up, so nothing
/// waits on the laptop (owner, 30 Oct); when it is free again it takes items as before. Before
/// the node has said anything (HQ just started), nothing moves.
#[test]
fn while_the_mac_is_away_its_member_is_passed_over_and_its_items_go_up() {
    let hq_of = hq;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq_of(&clock);
    let f = five(&hq);
    hq.set_loop_rank(f.loop_, Brain::Mac.rank()).unwrap();
    let held = item(&hq, &f, "Fix the lint", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, held), Some(f.rae), "nothing said yet: Rae's");
    hq.tell_mac(false, "busy");
    for c in hq.runner().started() {
        hq.runner().finish(c.run, true);
    }
    hq.tick().unwrap();
    assert_eq!(
        assignee(&hq, held),
        Some(f.ada),
        "the Mac is busy: one rank up"
    );
    // A fresh loop item while the Mac is away starts above it.
    let clock2 = Hands(AtomicI64::new(NOON));
    let hq2 = hq_of(&clock2);
    let f2 = five(&hq2);
    hq2.set_loop_rank(f2.loop_, Brain::Mac.rank()).unwrap();
    hq2.tell_mac(false, "on battery");
    let fresh = item(&hq2, &f2, "Fix the lint", Priority::Normal);
    hq2.tick().unwrap();
    assert_eq!(
        assignee(&hq2, fresh),
        Some(f2.ada),
        "started above the away Mac"
    );
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

/// The member one rank up passes work as it sends it back: with the review tool, on a reading
/// of its own. Live, the tool asked for a tester's role, which no member has in the loops, so a
/// reviewer could only send work back.
///
/// Given Bo's item in review, its gate passed,
/// when Rae (one rank up) approves it on a reading of her own,
/// then the item is done.
#[test]
fn the_member_one_rank_up_passes_work_in_review() {
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
    hq.record_gate(fix, true, "").unwrap();
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, f.rae);
    let mine = hq
        .record_receipt(
            &rae.token,
            &super::engine::Called {
                tool: "homelab_status".to_owned(),
                args: String::new(),
                excerpt: "lint: clean".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    use_tool(
        &hq,
        &rae,
        "review",
        &serde_json::json!({ "task": fix.0, "verdict": "approve", "notes": "The lint is clean.",
                             "evidence": [format!("r{mine}")] }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(fix).unwrap().unwrap().status,
        TaskStatus::Done
    );
}

/// Work that changed nothing has no branch, so there is nothing for the gate to run on: its
/// pass closes it. Live, #855 found roles/ already clean; Rae submitted it with no branch, Ada
/// passed it, and it waited in review for a gate that could never run.
///
/// Given Bo's item in review with no branch and no gate,
/// when Rae (one rank up) approves it on a reading of her own,
/// then the item is done.
#[test]
fn work_with_no_branch_needs_no_gate_to_pass() {
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
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "Already clean.",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, f.rae);
    let mine = hq
        .record_receipt(
            &rae.token,
            &super::engine::Called {
                tool: "homelab_status".to_owned(),
                args: String::new(),
                excerpt: "lint: clean".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    use_tool(
        &hq,
        &rae,
        "review",
        &serde_json::json!({ "task": fix.0, "verdict": "approve", "notes": "Nothing to change.",
                             "evidence": [format!("r{mine}")] }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(fix).unwrap().unwrap().status,
        TaskStatus::Done
    );
}

/// In the loops, turns that show nothing are one of the item's failures, not a model tier up
/// for the same member or a hand-up to whom it reports (ADR 0142): three failures climb a rank.
///
/// Given Bo's item,
/// when Bo takes three turns on it that show nothing,
/// then the item has one failure, it is still Bo's, and nothing went a tier up.
#[test]
fn turns_that_show_nothing_are_one_of_the_items_failures() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
    for _ in 0..3 {
        clock.0.fetch_add(10 * 60_000, Ordering::SeqCst);
        hq.wake(f.bo, Some(fix), WakeReason::OwnerReplied).unwrap();
        let bo = turn_of(&hq, f.bo);
        use_tool(
            &hq,
            &bo,
            "post",
            &serde_json::json!({ "task": fix.0, "text": "Looking." }),
        )
        .unwrap();
        hq.runner().finish(bo.run, true);
        hq.tick().unwrap();
    }
    assert_eq!(hq.loop_failures(fix).unwrap(), 1);
    let now = hq.store().task(fix).unwrap().unwrap();
    assert_eq!(now.owner.or(now.assignee), Some(f.bo));
}

/// A turn the system ended (refused by the relay, timed out, failed) says nothing of the
/// member's work, so it is not one of the item's failures. Baseline 28 Oct: 31 of Mara's
/// refused turns were labelled empty and counted, so escalation climbed for the relay's sake.
///
/// Given Bo's item,
/// when two of Bo's turns on it fail before showing anything, then one of its own shows nothing,
/// then the item has no failure and is still Bo's.
#[test]
fn a_turn_the_system_ended_is_not_the_items_failure() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
    for _ in 0..2 {
        clock.0.fetch_add(10 * 60_000, Ordering::SeqCst);
        hq.wake(f.bo, Some(fix), WakeReason::OwnerReplied).unwrap();
        // Every try of the turn fails, until it has no retries left.
        for _ in 0..6 {
            hq.tick().unwrap();
            for c in hq.runner().started() {
                hq.runner().finish(c.run, c.member != f.bo);
            }
            clock.0.fetch_add(5 * 60_000, Ordering::SeqCst);
        }
        hq.tick().unwrap();
    }
    let failed = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.member == f.bo && r.status == crate::domain::RunStatus::Failed)
        .count();
    assert!(failed >= 2, "{failed} turns failed for good");
    // Then one turn of Bo's own that shows nothing: it is the first, not the third.
    clock.0.fetch_add(10 * 60_000, Ordering::SeqCst);
    hq.wake(f.bo, Some(fix), WakeReason::OwnerReplied).unwrap();
    let bo = turn_of(&hq, f.bo);
    use_tool(
        &hq,
        &bo,
        "post",
        &serde_json::json!({ "task": fix.0, "text": "Looking." }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
    assert_eq!(hq.loop_failures(fix).unwrap(), 0);
    let now = hq.store().task(fix).unwrap().unwrap();
    assert_eq!(now.owner.or(now.assignee), Some(f.bo));
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
    // Baseline 28 Oct: the item stayed doing with Mara, so its loop handed out nothing else and
    // the same notice came back every half hour. It is blocked and let go instead, once.
    let found = hq.store().tasks().unwrap();
    let found = found.iter().find(|t| t.id == hard).unwrap();
    assert_eq!(found.status, TaskStatus::Blocked);
    assert_eq!(found.assignee, None);
    let next = item(&hq, &f, "Pin the homepage image", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, next), Some(f.bo), "the loop moves on");
    for _ in 0..6 {
        hq.loop_failed(hard).unwrap();
    }
    let notices = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.task == Some(hard))
        .count();
    assert_eq!(notices, 1, "told once");
}

/// Today's team of eight, with roles, routines and work, as production has it.
fn the_old_team(hq: &Hq<'_>) -> (GoalId, TaskId) {
    let g = goal(hq);
    let hired = |name: &str, role: Role| hq.hire(name, role, 10, 2, 0).unwrap();
    let mara = hired("Mara", Role::Ceo);
    let kit = hired("Kit", Role::Lead);
    let ada = hired("Ada", Role::Lead);
    hired("Rae", Role::Developer);
    hired("Bo", Role::Developer);
    let cy = hired("Cy", Role::Researcher);
    hired("Dee", Role::Reviewer);
    hired("Zed", Role::Developer);
    let routine = |title: &str, to: MemberId| {
        hq.add_routine(
            Author::Owner,
            super::ports::NewRoutine {
                title: title.to_owned(),
                description: String::new(),
                goal: g,
                project: None,
                assignee: to,
                every_ms: 6 * 60 * 60 * 1000,
                on_lab_trouble: title == "Check the home lab",
            },
        )
        .unwrap()
    };
    routine("Check the home lab", ada);
    routine("Daily review", mara);
    routine("State of my area", kit);
    let cys = task(hq, g, "Cy's research");
    hq.store().set_assignee(cys, Some(cy)).unwrap();
    (g, cys)
}

/// The cut-over (ADR 0142): five members become the ranked models, with no roles or reporting
/// lines; Cy, Dee and Zed retire, their open work back on the plan; only the lab's health check
/// runs on, as Bo's. Running it again changes nothing.
#[test]
fn the_team_becomes_one_team_of_five_ranked_models() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let (_, cys) = the_old_team(&hq);
    hq.become_one_team().unwrap();
    let members = hq.store().members().unwrap();
    let by = |name: &str| members.iter().find(|m| m.name == name).unwrap().clone();
    for (name, brain) in [
        ("Mara", Brain::Opus),
        ("Kit", Brain::Sonnet),
        ("Ada", Brain::Haiku),
        ("Rae", Brain::Mac),
        ("Bo", Brain::Local),
    ] {
        let m = by(name);
        assert_eq!((m.brain, m.work_brain), (brain, brain), "{name}");
        assert_eq!(m.role, Role::Developer, "{name}: no titles in the code");
        assert_eq!(m.reports_to, None, "{name}: no reporting lines");
        assert!(m.at_work(), "{name}");
    }
    for name in ["Cy", "Dee", "Zed"] {
        assert!(by(name).furloughed(), "{name} retires");
    }
    let held = hq.store().task(cys).unwrap().unwrap();
    assert_eq!(
        (held.assignee, held.owner),
        (None, None),
        "Cy's work is back on the plan"
    );
    let routines = hq.store().routines().unwrap();
    let on: Vec<_> = routines.iter().filter(|r| r.enabled).collect();
    assert_eq!(on.len(), 1, "{routines:?}");
    assert_eq!(
        (on[0].title.as_str(), on[0].assignee),
        ("Check the home lab", by("Bo").id)
    );
    hq.become_one_team().unwrap();
    assert_eq!(
        hq.store().members().unwrap().len(),
        8,
        "nothing new the second time"
    );
}

/// A member in the loops has the work's tools and none of the company's.
#[test]
fn a_member_in_the_loops_is_offered_the_work_s_tools_alone() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    let (run, member) = hq.turn_of(&bo.token).unwrap().unwrap();
    let offered: Vec<String> = super::protocol::tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect();
    for gone in [
        "consult",
        "escalate",
        "huddle_view",
        "note",
        "propose_world",
        "office",
        "request_assistance",
        "decide_stuck",
        "hand_to",
    ] {
        assert!(!offered.iter().any(|n| n == gone), "{gone} in {offered:?}");
    }
    for kept in [
        "my_turn",
        "claim",
        "post",
        "set_status",
        "create_task",
        "ask_owner",
        "read_world",
        "ask_record",
    ] {
        assert!(
            offered.iter().any(|n| n == kept),
            "{kept} missing from {offered:?}"
        );
    }
    let _ = fix;
}

/// A member in the loops asks the owner directly, on the desk: there is no chain to go up.
#[test]
fn a_member_in_the_loops_asks_the_owner_on_the_desk() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let fix = item(&hq, &f, "Raise sonarr's memory", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(
        &hq,
        &bo,
        "ask_owner",
        &serde_json::json!({ "text": "May sonarr have 2 GB of the NAS's 16?",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    let desk = hq.store().desk().unwrap();
    assert!(
        desk.iter()
            .any(|d| d.kind == "question" && d.task == Some(fix)),
        "{desk:?}"
    );
}

/// A loop item that sits with nothing moving it is nudged once; each later stall is one of its
/// failures, and there is no ladder and no desk.
#[test]
fn a_stalled_loop_item_counts_failures_instead_of_climbing_the_ladder() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    hq.runner().finish(bo.run, true);
    for _ in 0..6 {
        clock
            .0
            .fetch_add(super::engine::STRANDED_AFTER_MS + 60_000, Ordering::SeqCst);
        hq.tick().unwrap();
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
    }
    assert!(hq.loop_failures(fix).unwrap() > 0 || assignee(&hq, fix) != Some(f.bo));
    assert!(
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .all(|d| d.task != Some(fix)),
        "no desk card for a stall"
    );
}

/// The owner's desk in the loops holds only what is the owner's (ADR 0142): a notice is told
/// in the feed and kept, but never waits on the desk.
#[test]
fn in_the_loops_a_notice_never_waits_on_the_desk() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let _f = five(&hq);
    let id = hq.notice(None, "A restart cut 1 turn.").unwrap();
    let found = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap();
    assert_ne!(found.status, "waiting", "{found:?}");
}

/// Outside the loops a notice waits, as before.
#[test]
fn outside_the_loops_a_notice_waits() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    hq.hire("Ada", Role::Lead, 10, 2, 0).unwrap();
    let id = hq.notice(None, "A restart cut 1 turn.").unwrap();
    let found = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap();
    assert_eq!(found.status, "waiting");
}

/// The map (ADR 0142): a member who finds where something lives adds one line; another who adds
/// the same line confirms it; every later briefing shows the most confirmed first; a line nobody
/// confirms for 30 days fades; the owner sees each line and how often it was confirmed.
#[test]
fn the_map_grows_from_what_members_find_and_fades_what_nobody_confirms() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let first = item(&hq, &f, "Find the backup schedule", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    let said = use_tool(
        &hq,
        &bo,
        "add_to_map",
        &serde_json::json!({ "line": "The backup schedule is in roles/backup/defaults/main.yml" }),
    )
    .unwrap();
    assert!(said.contains("map"), "{said}");
    use_tool(
        &hq,
        &bo,
        "add_to_map",
        &serde_json::json!({ "line": "the BACKUP schedule is in roles/backup/defaults/main.yml " }),
    )
    .unwrap();
    let lines = hq.snapshot().unwrap()["map_lines"].clone();
    assert_eq!(lines.as_array().unwrap().len(), 1, "{lines}");
    assert_eq!(lines[0]["uses"], 2, "confirmed: {lines}");
    assert_eq!(lines[0]["by"], "Bo");
    hq.runner().finish(bo.run, true);
    hq.store()
        .set_task_status(first, crate::domain::TaskStatus::Done)
        .unwrap();
    item(&hq, &f, "Turn the timer off", Priority::Normal);
    let next = turn_of(&hq, f.bo);
    assert!(
        next.told.contains("roles/backup/defaults/main.yml"),
        "the next briefing shows the map: {}",
        next.told
    );
    hq.runner().finish(next.run, true);
    clock
        .0
        .fetch_add(31 * 24 * 60 * 60 * 1000, Ordering::SeqCst);
    assert!(
        hq.snapshot().unwrap()["map_lines"]
            .as_array()
            .unwrap()
            .is_empty(),
        "unconfirmed for 30 days, it fades"
    );
}

/// A loop's notes (ADR 0142): a turn on a loop's item is told what the loop did lately, like
/// Ralph's progress file, from the lines its turns ended with.
#[test]
fn a_loop_turn_is_told_what_the_loop_did_lately() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let first = item(&hq, &f, "Fix the lint", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(
        &hq,
        &bo,
        "post",
        &serde_json::json!({ "task": first.0, "text": "Fixed the yamllint line lengths in roles/gluetun." }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.store()
        .set_task_status(first, crate::domain::TaskStatus::Done)
        .unwrap();
    item(&hq, &f, "Fix the next lint", Priority::Normal);
    let next = turn_of(&hq, f.bo);
    assert!(next.told.contains("yamllint line lengths"), "{}", next.told);
}

/// Production, 27 Oct: HQ restarted and a migration of ADR 0131's gave Bo and Rae back a Claude,
/// taking them out of the loops. A member that is one model stays so across a restart.
#[test]
fn a_member_s_model_survives_a_restart() {
    let dir = std::env::temp_dir().join(format!("endora-loops-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("hq.db");
    let _ = std::fs::remove_file(&path);
    let path = path.to_str().unwrap().to_owned();
    let clock = Hands(AtomicI64::new(NOON));
    let bo = {
        let hq = Engine::new(
            SqliteTeamStore::open(&path).unwrap(),
            SimulatedRunner::new(2),
            &clock,
            Settings::default(),
        );
        let bo = hq.hire("Bo", Role::Developer, 10, 2, 0).unwrap();
        hq.set_model(bo, "local").unwrap();
        bo
    };
    let again = SqliteTeamStore::open(&path).unwrap();
    let found = again
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == bo)
        .unwrap();
    assert_eq!(
        (found.brain, found.work_brain),
        (Brain::Local, Brain::Local)
    );
    let _ = std::fs::remove_file(&path);
}

/// A task whose words leave the lab as it is refuses its turns the lab's acts (baseline 28-29
/// Oct: two read-only tasks restarted containers); any other task's turns keep them.
#[test]
fn a_read_only_items_turns_are_refused_the_restart() {
    let hq_of = hq;
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq_of(&clock);
    let f = five(&hq);
    let look = item(
        &hq,
        &f,
        "Is the gluetun restart still needed? Read only.",
        Priority::Normal,
    );
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": look.0 })).unwrap();
    assert_eq!(hq.lab_tools_refused(bo.run).unwrap(), ["restart_container"]);
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
    let other_clock = Hands(AtomicI64::new(NOON));
    let hq2 = hq_of(&other_clock);
    let f2 = five(&hq2);
    let fix = item(
        &hq2,
        &f2,
        "Restart sonarr when it is unhealthy",
        Priority::Normal,
    );
    let bo2 = turn_of(&hq2, f2.bo);
    use_tool(&hq2, &bo2, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    assert!(hq2.lab_tools_refused(bo2.run).unwrap().is_empty());
}
