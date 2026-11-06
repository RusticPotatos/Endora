//! One team of loops (ADR 0142): members are ranked models, loops hand out work, and an item
//! climbs the ranks when its checks fail. Over HQ's real SQLite store and the simulated runner.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::desk_ports::DeskStore;
use crate::application::schedule_ports::ScheduleStore;
use crate::application::wing_ports::WingStore;

use crate::application::receipt_ports::ReceiptStore;
use crate::application::run_ports::RunStore;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, TeamStore};
use crate::domain::money::DOLLAR;
use crate::domain::{Brain, GoalId, MemberId, Priority, TaskId, TaskStatus, WakeReason};
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
    let bo = hq.hire("Bo", 10, 2, 0).unwrap();
    let kit = hq.hire("Kit", 10, 2, 0).unwrap();
    assert_eq!(hq.set_model(bo, "local").unwrap().brain, Brain::Local);
    assert_eq!(hq.set_model(kit, "sonnet").unwrap().brain, Brain::Sonnet);
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
    let rae = hq.hire("Rae", 10, 2, 0).unwrap();
    hq.set_brain(rae, "mac").unwrap();
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["members"][0]["rank"], 5, "{}", snap["members"][0]);
}

/// Opus is no longer the CEO's alone: a title is a name only (ADR 0142).
#[test]
fn any_member_may_be_any_model() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let mara = hq.hire("Mara", 10, 2, 0).unwrap();
    assert_eq!(hq.set_model(mara, "opus").unwrap().brain, Brain::Opus);
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
    let first = hq.hire("", 10, 2, 0).unwrap();
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
    let seat = |name: &str, model: &str| {
        let id = hq.hire(name, 10, 2, 0).unwrap();
        hq.set_brain(id, model).unwrap();
        id
    };
    let mara = seat("Mara", "opus");
    seat("Kit", "sonnet");
    let ada = seat("Ada", "haiku");
    let rae = seat("Rae", "mac");
    let bo = seat("Bo", "local");
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

/// The owner names the member who works a loop alone, and may give the loop back to the team
/// (ADR 0142, amended 2026-11-01); a loop that does not exist is refused.
///
/// Given a loop worked alone by Ada, when HQ ticks, then its item is Ada's though the loop
/// starts at Bo's rank; set back to the team, the loop names nobody.
#[test]
fn the_owner_sets_a_loop_to_be_worked_alone_and_back() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    assert!(
        hq.set_loop_solo(crate::domain::ProjectId(999), Some(f.ada))
            .is_err(),
        "no such loop"
    );
    hq.set_loop_solo(f.loop_, Some(f.ada)).unwrap();
    assert_eq!(hq.loop_solo(f.loop_).unwrap(), Some(f.ada));
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let tidy = item(&hq, &f, "Tidy the README", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, fix), Some(f.ada), "to Ada alone");
    assert_eq!(assignee(&hq, tidy), None, "one at a time");
    hq.set_loop_solo(f.loop_, None).unwrap();
    assert_eq!(hq.loop_solo(f.loop_).unwrap(), None);
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
    for c in hq.runner().started() {
        hq.runner().finish(c.run, true);
    }
    // Rae is woken on it, and then the Mac goes away: the turn is queued, waiting for it.
    hq.wake(f.rae, Some(held), WakeReason::OwnerReplied)
        .unwrap();
    hq.tell_mac(false, "busy");
    hq.tick().unwrap();
    assert_eq!(
        assignee(&hq, held),
        Some(f.ada),
        "the Mac is busy: one rank up"
    );
    // Rae's queued turn on it ends with the hand-up, so it never starts later on an item that
    // is now Ada's (review of 30 Oct).
    let rae_waiting = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .filter(|r| r.member == f.rae && r.task == Some(held))
        .filter(|r| {
            matches!(
                r.status,
                crate::domain::RunStatus::Queued | crate::domain::RunStatus::RetryWaiting
            )
        })
        .count();
    assert_eq!(rae_waiting, 0, "Rae's queued turns on it ended");
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
            .any(|t| t["name"] == "review" && t["_meta"][super::protocol::UP_FRONT] == true),
        "the reviewer one rank up is offered the review tool, up front"
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

/// The owner, 31 Oct (D7): reviews are capped at Haiku. Work done below Haiku's rank is never
/// reviewed above it, and only the reviewer HQ chose may pass it.
///
/// Given Ada on DeepSeek, so nobody on the team thinks with Haiku, and her item in review,
/// when HQ asks for its review,
/// then Rae (the Mac, below her) is woken to it, not Kit (Sonnet), and Kit may not review it.
#[test]
fn work_below_haiku_is_never_reviewed_above_haiku() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.set_model(f.ada, "deepseek").unwrap();
    hq.set_loop_rank(f.loop_, 4).unwrap();
    let kit = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "Kit")
        .unwrap()
        .id;
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let ada = turn_of(&hq, f.ada);
    use_tool(&hq, &ada, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    use_tool(
        &hq,
        &ada,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "The lint passes.",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    hq.runner().finish(ada.run, true);
    assert!(woken(&hq, f.rae, fix, WakeReason::ReviewRequested));
    assert!(!woken(&hq, kit, fix, WakeReason::ReviewRequested));
    let task = hq.store().task(fix).unwrap().unwrap();
    assert_eq!(
        hq.loop_reviewer_of(&task).unwrap().map(|m| m.id),
        Some(f.rae)
    );
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
    assert_eq!(now.holder(), Some(f.bo));
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
    assert_eq!(now.holder(), Some(f.bo));
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
    let hired = |name: &str| hq.hire(name, 10, 2, 0).unwrap();
    let mara = hired("Mara");
    let kit = hired("Kit");
    let ada = hired("Ada");
    hired("Rae");
    hired("Bo");
    let cy = hired("Cy");
    hired("Dee");
    hired("Zed");
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
        assert_eq!(m.brain, brain, "{name}");
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
    // The item is the member's: its briefing is read for it, so no protocol moves (D30).
    for gone in ["my_turn", "claim"] {
        assert!(!offered.iter().any(|n| n == gone), "{gone} in {offered:?}");
    }
    for kept in [
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
    // On an item, the map's lines that name what it names (ADR 0142, amended 2026-10-30).
    item(&hq, &f, "Turn the backup timer off", Priority::Normal);
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
        let bo = hq.hire("Bo", 10, 2, 0).unwrap();
        hq.set_brain(bo, "local").unwrap();
        bo
    };
    let again = SqliteTeamStore::open(&path).unwrap();
    let found = again
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == bo)
        .unwrap();
    assert_eq!(found.brain, Brain::Local);
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

// ---- the handover (ADR 0142, amended 2026-10-30) ---------------------------------------------

/// The handover in a briefing: from its first line to its end, alone.
fn handover_in(told: &str) -> &str {
    let start = told
        .find(crate::domain::handover::START)
        .unwrap_or_else(|| panic!("a handover in the briefing: {told}"));
    let rest = &told[start..];
    let end = rest
        .find(crate::domain::handover::END)
        .expect("the handover's end")
        + crate::domain::handover::END.len();
    &rest[..end]
}

/// A reading a turn took, as the node records it.
fn read(hq: &Hq<'_>, turn: &super::ports::RunContext, tool: &str, args: &str, got: &str) -> i64 {
    hq.record_receipt(
        &turn.token,
        &super::engine::Called {
            tool: tool.to_owned(),
            args: args.to_owned(),
            excerpt: got.to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
}

/// One item at a time, an earlier item done with its own readings and words, then a hard item
/// with a criterion, on which Bo takes a reading and says what he tried; then `more` long
/// readings and posts.
fn bo_tried(hq: &Hq<'_>, f: &Five, more: usize) -> (TaskId, i64) {
    hq.set_loop_items(f.loop_, 1).unwrap();
    let other = item(hq, f, "Pin the homepage image", Priority::Normal);
    let bo = turn_of(hq, f.bo);
    read(hq, &bo, "homepage_probe", "homepage", "OTHER-ITEM-READING");
    use_tool(
        hq,
        &bo,
        "post",
        &serde_json::json!({ "task": other.0, "text": "OTHER-ITEM-WORDS pinned it." }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.store().set_task_status(other, TaskStatus::Done).unwrap();
    let hard = item(hq, f, "Find what stops gluetun", Priority::Normal);
    hq.set_criteria(
        hard,
        &[crate::domain::evidence::Criterion::new(
            "gluetun is running and healthy",
            false,
        )],
    )
    .unwrap();
    let bo = turn_of(hq, f.bo);
    use_tool(hq, &bo, "claim", &serde_json::json!({ "task": hard.0 })).unwrap();
    let logs = read(
        hq,
        &bo,
        "container_logs",
        "gluetun",
        "AUTH_FAILED from the provider",
    );
    use_tool(
        hq,
        &bo,
        "post",
        &serde_json::json!({ "task": hard.0, "text": "The VPN credentials look stale." }),
    )
    .unwrap();
    for i in 0..more {
        read(
            hq,
            &bo,
            "container_logs",
            &format!("app{i}"),
            &"x".repeat(300),
        );
        use_tool(
            hq,
            &bo,
            "post",
            &serde_json::json!({ "task": hard.0, "text": format!("try {i} {}", "y".repeat(500)) }),
        )
        .unwrap();
    }
    hq.runner().finish(bo.run, true);
    (hard, logs)
}

/// Rung 5 of closing the gap: fewer handoffs. Baseline 28 Oct: an item that climbed was
/// started over at every rank, the same container logs read again each time.
///
/// Given an item Bo took a reading on and said what he tried,
/// when it fails three times at Bo's rank and climbs to Rae,
/// then Rae's briefing carries a handover: the item's criterion and what the record shows of
/// it, Bo's reading to cite, Bo's last post; it is posted on the thread as Endora's; and
/// nothing of the loop's other item is in it.
#[test]
fn an_item_that_climbs_carries_a_handover_to_the_next_rank() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let (hard, logs) = bo_tried(&hq, &f, 0);
    for _ in 0..3 {
        hq.loop_failed(hard).unwrap();
    }
    assert_eq!(assignee(&hq, hard), Some(f.rae));
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, f.rae);
    let note = handover_in(&rae.told);
    assert!(note.contains(&format!("#{} to Rae", hard.0)), "{note}");
    assert!(note.contains("failed 3 times at Bo's rank"), "{note}");
    assert!(
        note.contains("1. gluetun is running and healthy [nothing cited yet]"),
        "{note}"
    );
    assert!(
        note.contains(&format!(
            "r{logs} container_logs gluetun: AUTH_FAILED from the provider"
        )),
        "{note}"
    );
    assert!(
        note.contains("Bo: The VPN credentials look stale."),
        "{note}"
    );
    assert!(
        !note.contains("OTHER-ITEM"),
        "nothing of another item: {note}"
    );
    assert!(
        note.chars().count() <= crate::domain::handover::chars_for(Brain::Mac),
        "{note}"
    );
    let thread = hq.store().thread(hard).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.author == "endora" && p.text.starts_with("Handover of #")),
        "the owner sees what was handed over: {thread:?}"
    );
    assert_eq!(
        rae.told.matches(crate::domain::handover::START).count(),
        1,
        "told once, not again among the thread's news: {}",
        rae.told
    );
    let (run, _) = hq.turn_of(&rae.token).unwrap().unwrap();
    assert!(
        crate::application::history_ports::HistoryStore::run_read_confidential(hq.store(), run.id)
            .unwrap(),
        "members' words are of unknown origin"
    );
}

/// The handover is held to the receiving model's size: the house's models get less.
///
/// Given an item Bo took many long readings on, with many long posts,
/// when it climbs to Rae, on the Mac,
/// then the handover in Rae's briefing is within the Mac's size and still says the criterion.
#[test]
fn a_handover_stays_within_a_small_model_s_size() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let (hard, _) = bo_tried(&hq, &f, 30);
    for _ in 0..3 {
        hq.loop_failed(hard).unwrap();
    }
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, f.rae);
    let note = handover_in(&rae.told);
    assert!(
        note.chars().count() <= crate::domain::handover::chars_for(Brain::Mac),
        "{} chars: {note}",
        note.chars().count()
    );
    assert!(note.contains("1. gluetun is running and healthy"), "{note}");
    assert!(note.contains("app29"), "the newest readings first: {note}");
}

/// A send-back hands the item back to its worker with the reviewer's reasons.
///
/// Given Bo's item in review, with a reading of his,
/// when Rae (one rank up) sends it back,
/// then Bo's next briefing carries a handover with Rae's reasons and his own reading to cite.
#[test]
fn a_send_back_s_reasons_reach_the_worker_in_a_handover() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    let lint = read(&hq, &bo, "run_lint", "roles/gluetun", "3 warnings");
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "The lint passes.",
                             "evidence": [format!("r{lint}")] }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, f.rae);
    use_tool(
        &hq,
        &rae,
        "review",
        &serde_json::json!({ "task": fix.0, "verdict": "send_back",
                             "notes": "The lint still fails on roles/gluetun line 12." }),
    )
    .unwrap();
    hq.runner().finish(rae.run, true);
    let bo = turn_of(&hq, f.bo);
    let note = handover_in(&bo.told);
    assert!(note.contains(&format!("#{} to Bo", fix.0)), "{note}");
    assert!(note.contains("sent back by Rae"), "{note}");
    assert!(note.contains("Sent back because:"), "{note}");
    assert!(
        note.contains("Rae: The lint still fails on roles/gluetun line 12."),
        "{note}"
    );
    assert!(note.contains(&format!("r{lint} run_lint")), "{note}");
    assert!(
        note.contains(&format!("- Fix the lint [cited r{lint}, then sent back]")),
        "{note}"
    );
    assert!(
        note.chars().count() <= crate::domain::handover::chars_for(Brain::Local),
        "{note}"
    );
}

/// A paused member's item goes one rank up with a handover too.
#[test]
fn a_paused_member_s_item_goes_up_with_a_handover() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let (hard, logs) = bo_tried(&hq, &f, 0);
    hq.pause(f.bo, true).unwrap();
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, hard), Some(f.rae));
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, f.rae);
    let note = handover_in(&rae.told);
    assert!(note.contains("Bo is paused"), "{note}");
    assert!(note.contains(&format!("r{logs} container_logs")), "{note}");
}

// ---- a turn out of room continues fresh (ADR 0142, amended 2026-10-30) ----------------------

/// One turn of Bo's on `hard` that takes the same reading of gluetun's log, then ends as the
/// worker ends a turn whose prompt reached its budget: its last word, marked as continuing.
/// Returns the reading's receipt.
fn bo_out_of_room(hq: &Hq<'_>, f: &Five, hard: TaskId) -> i64 {
    let bo = turn_of(hq, f.bo);
    use_tool(hq, &bo, "claim", &serde_json::json!({ "task": hard.0 })).unwrap();
    let logs = read(
        hq,
        &bo,
        "container_logs",
        "gluetun",
        "AUTH_FAILED from the provider",
    );
    // The relay metered what his calls read: near the house's budget of about 60,000 tokens.
    hq.note_tokens(bo.run, 50_000, 0, 0, 500).unwrap();
    use_tool(
        hq,
        &bo,
        "post",
        &serde_json::json!({ "task": hard.0, "last_word": true, "continues": true,
            "text": "The VPN credentials look stale.\n\nThe turn's prompt reached its budget: \
                     it ends here and goes on in a fresh turn." }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
    logs
}

/// An item of one at a time with a criterion, given to Bo.
fn hard_item(hq: &Hq<'_>, f: &Five) -> TaskId {
    hq.set_loop_items(f.loop_, 1).unwrap();
    let hard = item(hq, f, "Find what stops gluetun", Priority::Normal);
    hq.set_criteria(
        hard,
        &[crate::domain::evidence::Criterion::new(
            "gluetun is running and healthy",
            false,
        )],
    )
    .unwrap();
    hard
}

/// Ralph's rule, every pass fits one fresh context window: a turn whose prompt reached its
/// budget ends cleanly, and the same member goes on in a fresh turn that starts from HQ's
/// handover instead of the whole conversation. It is not one of the item's failures.
///
/// Given Bo's item, on which he took a reading,
/// when his turn ends at its prompt's budget,
/// then he is woken to continue it, it is still his with no failure, and his next briefing
/// carries a handover to him with his reading to cite and what he said.
#[test]
fn a_turn_out_of_room_continues_with_the_same_member_from_a_handover() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    let logs = bo_out_of_room(&hq, &f, hard);
    assert!(woken(&hq, f.bo, hard, WakeReason::Continued));
    assert_eq!(hq.loop_failures(hard).unwrap(), 0);
    assert_eq!(assignee(&hq, hard), Some(f.bo));
    let bo = turn_of(&hq, f.bo);
    let note = handover_in(&bo.told);
    assert!(note.contains(&format!("#{} to Bo", hard.0)), "{note}");
    assert!(note.contains("out of room, continuation 1 of 3"), "{note}");
    assert!(
        note.contains(&format!(
            "r{logs} container_logs gluetun: AUTH_FAILED from the provider"
        )),
        "{note}"
    );
    assert!(
        note.contains("Bo: The VPN credentials look stale."),
        "{note}"
    );
    assert!(
        note.chars().count() <= crate::domain::handover::chars_for(Brain::Local),
        "{note}"
    );
    let thread = hq.store().thread(hard).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.author == "endora" && p.text.starts_with("Handover of #")),
        "the owner sees what was handed over: {thread:?}"
    );
}

/// An item cannot continue for ever: past three continuations with one member, running out
/// of room again is one of its failures, as a turn that never finished was before. Turns out
/// of room are never counted as showing nothing, even when each takes the same reading.
///
/// Given Bo's item,
/// when four of his turns on it in a row end at their prompt's budget, each reading the same
/// log,
/// then the first three continue with no failure, and the fourth is one failure, and he is
/// woken to go on from a fresh handover.
#[test]
fn past_three_continuations_running_out_of_room_is_one_failure() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    for n in 1..=3 {
        bo_out_of_room(&hq, &f, hard);
        assert_eq!(hq.loop_failures(hard).unwrap(), 0, "continuation {n}");
        let thread = hq.store().thread(hard).unwrap();
        assert!(
            thread
                .last()
                .is_some_and(|p| p.text.contains(&format!("continuation {n} of 3"))),
            "{thread:?}"
        );
    }
    bo_out_of_room(&hq, &f, hard);
    assert_eq!(hq.loop_failures(hard).unwrap(), 1);
    assert_eq!(assignee(&hq, hard), Some(f.bo));
    let thread = hq.store().thread(hard).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.author == "endora" && p.text.contains("ran out of room 4 times")),
        "{thread:?}"
    );
    let bo = turn_of(&hq, f.bo);
    assert!(handover_in(&bo.told).contains("out of room"), "{}", bo.told);
}

// ---- a long item makes progress across turns (ADR 0142, amended 2026-10-31, D12) -----------

/// The work in progress a turn's worker keeps as the turn runs out of room, at `head`.
fn kept_at(head: &str) -> super::engine::Submission {
    super::engine::Submission {
        branch: "hq/task-1-gluetun".to_owned(),
        head: head.to_owned(),
        base: "ee878f7619e9".to_owned(),
        files: 1,
        plus: 3,
        minus: 0,
        stat: " roles/gluetun/defaults/main.yml | 3 +++".to_owned(),
        diff: "+gluetun_dns: 192.168.1.10".to_owned(),
        can_push: false,
        changed: vec!["roles/gluetun/defaults/main.yml".to_owned()],
        texts: Vec::new(),
        workflows: None,
    }
}

/// One turn of Bo's on `hard` that keeps its work at `head` (when given) and ends at its
/// prompt's budget.
fn bo_keeps_and_runs_out(hq: &Hq<'_>, f: &Five, hard: TaskId, head: Option<&str>) {
    let bo = turn_of(hq, f.bo);
    use_tool(hq, &bo, "claim", &serde_json::json!({ "task": hard.0 })).unwrap();
    if let Some(head) = head {
        hq.keep_work_in_progress(&bo.token, &kept_at(head)).unwrap();
    }
    hq.note_tokens(bo.run, 50_000, 0, 0, 500).unwrap();
    use_tool(
        hq,
        &bo,
        "post",
        &serde_json::json!({ "task": hard.0, "last_word": true, "continues": true,
            "text": "The turn's prompt reached its budget: it ends here." }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
}

/// D12: work in progress is kept as a branch HQ records but neither checks nor offers, and the
/// next turn is told to check it out, and may.
#[test]
fn work_kept_in_progress_is_handed_to_the_next_turn_unchecked() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    bo_keeps_and_runs_out(&hq, &f, hard, Some("3c1cc497e55d0000"));
    assert!(
        hq.store().checks().unwrap().is_empty(),
        "nothing is checked"
    );
    assert!(hq.store().documents_of(hard).unwrap().is_empty());
    let kept = hq.store().branches().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].status, crate::domain::wing::IN_PROGRESS);
    let bo = turn_of(&hq, f.bo);
    let note = handover_in(&bo.told);
    assert!(
        note.contains("Its work so far is kept on hq/task-1-gluetun at 3c1cc497e55d"),
        "{note}"
    );
    assert!(
        note.contains("repo_checkout with branch hq/task-1-gluetun"),
        "{note}"
    );
    let handed = hq.checkout_branch(&bo.token, "hq/task-1-gluetun").unwrap();
    assert_eq!(handed.head, "3c1cc497e55d0000");
}

/// D12: an item in review keeps no work in progress, since a new head would void its review.
#[test]
fn work_in_review_keeps_no_work_in_progress() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": hard.0 })).unwrap();
    hq.store()
        .set_task_status(hard, TaskStatus::InReview)
        .unwrap();
    let refused = hq.keep_work_in_progress(&bo.token, &kept_at("3c1cc497e55d0000"));
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("in review")),
        "{refused:?}"
    );
    assert!(hq.store().branches().unwrap().is_empty());
}

/// D12: a turn out of room that kept new work was making progress, so it starts the count
/// again; one that kept the same head kept nothing new.
#[test]
fn only_new_work_kept_starts_the_count_again() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    for n in 0..6 {
        bo_keeps_and_runs_out(&hq, &f, hard, Some(&format!("{:040x}", n + 1)));
        assert_eq!(hq.loop_failures(hard).unwrap(), 0, "turn {n}");
    }
    // The same head again, four times: nothing new, and the fourth is a failure.
    for _ in 0..4 {
        bo_keeps_and_runs_out(&hq, &f, hard, Some(&format!("{:040x}", 6)));
    }
    assert_eq!(hq.loop_failures(hard).unwrap(), 1);
    assert_eq!(assignee(&hq, hard), Some(f.bo));
}

/// D12: an item goes on with one member at most twelve times in all, new work kept or not;
/// the next is one of its failures.
#[test]
fn twelve_continuations_in_all_then_a_failure() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    for n in 0..12 {
        bo_keeps_and_runs_out(&hq, &f, hard, Some(&format!("{:040x}", n + 1)));
        assert_eq!(hq.loop_failures(hard).unwrap(), 0, "turn {n}");
    }
    bo_keeps_and_runs_out(&hq, &f, hard, Some(&format!("{:040x}", 13)));
    assert_eq!(hq.loop_failures(hard).unwrap(), 1);
    let thread = hq.store().thread(hard).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.text.contains("ran out of room 12 continuations in all")),
        "{thread:?}"
    );
}

/// D12: `my_turn` shows the newest handover once (in the briefing) and leaves out the older
/// ones and the last words before it, saying so; the thread keeps them all.
#[test]
fn my_turn_shows_the_newest_handover_once() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    for _ in 0..5 {
        bo_keeps_and_runs_out(&hq, &f, hard, None);
    }
    let bo = turn_of(&hq, f.bo);
    let read = use_tool(&hq, &bo, "my_turn", &serde_json::json!({})).unwrap();
    assert_eq!(
        read.matches(crate::domain::handover::START).count(),
        1,
        "{read}"
    );
    assert!(read.contains("older handovers"), "{read}");
    let thread = hq.store().thread(hard).unwrap();
    assert_eq!(
        thread
            .iter()
            .filter(|p| p.text.starts_with(crate::domain::handover::START))
            .count(),
        5,
        "the record keeps every handover"
    );
}

/// Only the worker's own last word says a turn continues: a post of the model's that says so
/// is a post, and the turn ends as any other does.
#[test]
fn only_the_workers_last_word_continues_a_turn() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": hard.0 })).unwrap();
    use_tool(
        &hq,
        &bo,
        "post",
        &serde_json::json!({ "task": hard.0, "continues": true, "text": "Going on later." }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
    assert!(!woken(&hq, f.bo, hard, WakeReason::Continued));
}

/// One turn of a member's on `task` that the relay metered at `tokens` prompt tokens, ending
/// with a last word marked as continuing, as the worker's is.
fn says_out_of_room(hq: &Hq<'_>, member: MemberId, task: TaskId, tokens: i64) -> String {
    let turn = turn_of(hq, member);
    use_tool(hq, &turn, "claim", &serde_json::json!({ "task": task.0 })).unwrap();
    hq.note_tokens(turn.run, tokens, 0, 0, 100).unwrap();
    let said = use_tool(
        hq,
        &turn,
        "post",
        &serde_json::json!({ "task": task.0, "last_word": true, "continues": true,
            "text": "The turn's prompt reached its budget." }),
    )
    .unwrap();
    hq.runner().finish(turn.run, true);
    hq.tick().unwrap();
    said
}

/// HQ decides whether a turn ran out of room, not the post that says so (models propose,
/// policy authorizes): a turn the relay saw read far less than its brain's budget is not
/// continued, whatever its last word is marked.
///
/// Given Bo's item, on the house's model (about 60,000 tokens a turn),
/// when a turn of his that read 5,000 prompt tokens says it ran out of room,
/// then its last word is posted, but he is not woken to continue it.
#[test]
fn a_turn_that_read_far_less_than_its_budget_does_not_continue() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    let said = says_out_of_room(&hq, f.bo, hard, 5_000);
    assert!(said.contains("not continued"), "{said}");
    assert!(!woken(&hq, f.bo, hard, WakeReason::Continued));
    let thread = hq.store().thread(hard).unwrap();
    assert!(
        thread
            .iter()
            .any(|p| p.text.contains("The turn's prompt reached its budget.")),
        "{thread:?}"
    );
}

/// A Claude Code turn has no prompt budget of the worker's, and has the turn's token in its
/// environment: its post marked as continuing is never taken as one, however much it read.
///
/// Given Bo on Haiku, his turns run by Claude Code, with the loop's item,
/// when a turn of his that read 200,000 prompt tokens says it ran out of room,
/// then he is not woken to continue it.
#[test]
fn a_claude_code_turn_never_continues() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    // Claude Code runs only Claude (ADR 0117, amended 2026-10-31): Bo takes Haiku's rank, and
    // Ada, who had it, works on the Mac.
    hq.set_model(f.ada, "mac").unwrap();
    hq.set_model(f.bo, "haiku").unwrap();
    hq.set_runtime(f.bo, "claude-code").unwrap();
    hq.set_loop_rank(f.loop_, Brain::Haiku.rank()).unwrap();
    let hard = hard_item(&hq, &f);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, hard), Some(f.bo));
    let said = says_out_of_room(&hq, f.bo, hard, 200_000);
    assert!(said.contains("not continued"), "{said}");
    assert!(!woken(&hq, f.bo, hard, WakeReason::Continued));
}

/// Bo's turns, until his item's newest has failed for good.
fn fail_to_start(hq: &Hq<'_>, clock: &Hands, member: MemberId, task: TaskId) {
    for _ in 0..10 {
        hq.tick().unwrap();
        let newest = hq
            .store()
            .runs()
            .unwrap()
            .into_iter()
            .filter(|r| r.member == member && r.task == Some(task))
            .max_by_key(|r| r.id)
            .unwrap();
        if newest.status == crate::domain::RunStatus::Failed {
            return;
        }
        clock.0.fetch_add(61_000, Ordering::SeqCst);
    }
    panic!("the turn did not fail for good");
}

fn wakes_on(hq: &Hq<'_>, member: MemberId, task: TaskId) -> Vec<super::ports::Wake> {
    hq.wakes()
        .unwrap()
        .into_iter()
        .filter(|w| w.member == member && w.task == Some(task))
        .collect()
}

/// D2 of round 2 (31 Oct): Rae's turns failed because the runner could not start her runtime,
/// and her items sat with nobody woken until the owner wrote on each thread. A turn that never
/// started did nothing, well or badly: it is no failure of the item and shows nothing, and the
/// item is woken again for the same member after a wait (ADR 0142, amended 2026-10-31).
///
/// Given Bo's item, and a runner that cannot start Bo's runtime,
/// when every try of Bo's turn on it fails to start,
/// then the item has no failure and is still Bo's, the turn says it never started and has no
/// label, nobody is woken for five minutes, and then Bo is woken on it again, as he was.
#[test]
fn a_turn_that_never_started_is_no_failure_and_its_item_is_woken_again_later() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.runner()
        .script(f.bo, crate::infrastructure::Script::NeverStarts);
    let pin = item(&hq, &f, "Pin the homepage image", Priority::Normal);
    fail_to_start(&hq, &clock, f.bo, pin);
    assert_eq!(hq.loop_failures(pin).unwrap(), 0);
    assert_eq!(assignee(&hq, pin), Some(f.bo));
    let failed = hq
        .store()
        .runs()
        .unwrap()
        .into_iter()
        .rfind(|r| r.task == Some(pin))
        .unwrap();
    assert!(
        crate::domain::runs::is_never_started(&failed.end_reason),
        "{}",
        failed.end_reason
    );
    assert_eq!(failed.label, None, "a turn that never ran shows nothing");
    assert_eq!(wakes_on(&hq, f.bo, pin).len(), 1);
    clock.0.fetch_add(4 * 60_000, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(wakes_on(&hq, f.bo, pin).len(), 1, "not before its wait");
    clock.0.fetch_add(2 * 60_000, Ordering::SeqCst);
    hq.tick().unwrap();
    let wakes = wakes_on(&hq, f.bo, pin);
    assert_eq!(wakes.len(), 2, "{wakes:?}");
    assert_eq!(wakes[1].reason, WakeReason::Assigned, "woken as it was");
}

/// D3 of round 2 (31 Oct): item 938, blocked and judged, woke Kit about every eleven minutes. A
/// blocked item in the loops is not stranded work: it is neither nudged nor failed, and only
/// the owner's word wakes it (ADR 0142, amended 2026-10-31).
///
/// Given Bo's item, which he blocks,
/// when three hours pass,
/// then Bo was woken on it only the once, it has no failure and is still his; a check-back is
/// refused, and the owner's reply wakes him.
#[test]
fn a_blocked_item_in_the_loops_wakes_nobody_but_for_the_owner() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let timer = item(&hq, &f, "Judge the gluetun timer", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": timer.0 })).unwrap();
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": timer.0, "status": "blocked",
                             "reason": "the logs do not reach back to the window" }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    for _ in 0..36 {
        clock.0.fetch_add(5 * 60_000, Ordering::SeqCst);
        hq.tick().unwrap();
        for c in hq.runner().started() {
            hq.runner().finish(c.run, true);
        }
    }
    let woken: Vec<_> = wakes_on(&hq, f.bo, timer)
        .into_iter()
        .filter(|w| !matches!(w.receipt, crate::domain::Receipt::Refused(_)))
        .collect();
    assert_eq!(woken.len(), 1, "{woken:?}");
    assert_eq!(hq.loop_failures(timer).unwrap(), 0);
    let now = hq.store().task(timer).unwrap().unwrap();
    assert_eq!(now.status, TaskStatus::Blocked);
    assert_eq!(now.owner, Some(f.bo));
    assert_eq!(
        hq.wake(f.bo, Some(timer), WakeReason::CheckBack).unwrap(),
        crate::domain::Receipt::Refused(crate::domain::Refused::Blocked)
    );
    hq.post(Author::Owner, timer, "Keep the timer.").unwrap();
    assert_eq!(
        hq.wakes().unwrap().last().unwrap().reason,
        WakeReason::OwnerReplied
    );
    assert!(!matches!(
        hq.wakes().unwrap().last().unwrap().receipt,
        crate::domain::Receipt::Refused(_)
    ));
}

/// A member's block puts one question on the owner's desk, since only the owner now moves the
/// item; the owner seeing it with no words wakes nobody, and answering it in words wakes the
/// member who holds it.
#[test]
fn a_block_asks_the_owner_once_and_only_words_wake_its_holder() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let timer = item(&hq, &f, "Judge the gluetun timer", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": timer.0 })).unwrap();
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": timer.0, "status": "blocked", "reason": "no logs" }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tick().unwrap();
    let cards: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.task == Some(timer) && d.status == "waiting")
        .collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert_eq!(cards[0].kind, "question");
    assert!(cards[0].text.contains("no logs"), "{}", cards[0].text);
    let before = wakes_on(&hq, f.bo, timer).len();
    hq.decide(cards[0].id, "seen").unwrap();
    assert_eq!(
        wakes_on(&hq, f.bo, timer).len(),
        before,
        "seen, with no words"
    );
}

/// D10 of round 2: a blocked item asked the owner again and again. An item has one question
/// waiting on the owner's desk at a time: another member's question on it is refused with the
/// reason, and nothing new reaches the desk (ADR 0142, amended 2026-10-31).
#[test]
fn a_second_question_on_an_item_is_refused_while_the_first_waits() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let mem = item(&hq, &f, "Raise sonarr's memory", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(
        &hq,
        &bo,
        "ask_owner",
        &serde_json::json!({ "text": "May sonarr have 2 GB?", "evidence": [format!("task {}", mem.0)] }),
    )
    .unwrap();
    hq.pause(f.bo, true).unwrap();
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, mem), Some(f.rae));
    let rae = turn_of(&hq, f.rae);
    let said = use_tool(
        &hq,
        &rae,
        "ask_owner",
        &serde_json::json!({ "text": "May sonarr have 3 GB?", "evidence": [format!("task {}", mem.0)] }),
    )
    .unwrap_err();
    assert!(said.contains("already waits on the owner's desk"), "{said}");
    let cards = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.task == Some(mem) && d.status == "waiting")
        .count();
    assert_eq!(cards, 1);
}

/// D10 of round 3: only an answer in words is acted on instead of asked again; a question seen
/// with no words leaves the member free to ask once more (ADR 0142, amended 2026-11-01).
#[test]
fn a_question_seen_with_no_words_does_not_stop_the_next() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let mem = item(&hq, &f, "Raise sonarr's memory", Priority::Normal);
    let ask = |text: &str| {
        let bo = turn_of(&hq, f.bo);
        use_tool(
            &hq,
            &bo,
            "ask_owner",
            &serde_json::json!({ "text": text, "evidence": [format!("task {}", mem.0)] }),
        )
    };
    ask("May sonarr have 2 GB?").unwrap();
    let card = |hq: &Hq<'_>| {
        hq.store()
            .desk()
            .unwrap()
            .into_iter()
            .find(|d| d.task == Some(mem) && d.status == "waiting")
            .map(|d| d.id)
    };
    hq.decide(card(&hq).unwrap(), "seen").unwrap();
    hq.tick().unwrap();
    ask("May sonarr have 2 GB, or is 1 GB enough?").unwrap();
    hq.answer(card(&hq).unwrap(), "  ").unwrap_err();
    hq.answer(card(&hq).unwrap(), "1 GB is enough").unwrap();
    hq.tick().unwrap();
    let said = ask("Should it be 1 GB?").unwrap_err();
    assert!(said.contains("\"1 GB is enough\""), "{said}");
}

/// A failure at the top rank, while a question on the item already waits, is told on that card
/// rather than on a second one.
#[test]
fn a_top_rank_failure_is_folded_into_the_question_already_waiting() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = item(&hq, &f, "Find what stops gluetun", Priority::Normal);
    let bo = turn_of(&hq, f.bo);
    use_tool(
        &hq,
        &bo,
        "ask_owner",
        &serde_json::json!({ "text": "Is the VPN's account paid?", "evidence": [format!("task {}", hard.0)] }),
    )
    .unwrap();
    for _ in 0..15 {
        hq.loop_failed(hard).unwrap();
    }
    let cards: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.task == Some(hard) && d.status == "waiting")
        .collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(cards[0].text.contains("VPN's account"), "{}", cards[0].text);
    assert!(cards[0].text.contains("every rank"), "{}", cards[0].text);
}

/// Review of 31 Oct: with the Mac's member the only one ranked above, a Mac away for a while
/// blocked the item for good, "failed at every rank". It is held where it is instead, neither
/// climbing nor blocked, and goes up once the Mac is back.
///
/// Given Bo's item, with Mara, Kit and Ada paused and the Mac away,
/// when it fails three times at Bo's rank,
/// then it stays Bo's, not blocked, with nothing on the owner's desk, and nobody is woken on it;
/// further failures change nothing; and when the Mac is free again it goes to Rae.
#[test]
fn an_item_waits_for_an_away_mac_above_rather_than_blocking() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let ids: Vec<MemberId> = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .filter(|m| ["Mara", "Kit", "Ada"].contains(&m.name.as_str()))
        .map(|m| m.id)
        .collect();
    for id in ids {
        hq.pause(id, true).unwrap();
    }
    let hard = item(&hq, &f, "Find what stops gluetun", Priority::Normal);
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, hard), Some(f.bo));
    hq.tell_mac(false, "in use");
    let wakes_before = hq.wakes().unwrap().len();
    for _ in 0..6 {
        hq.loop_failed(hard).unwrap();
    }
    let found = hq.store().task(hard).unwrap().unwrap();
    assert_ne!(found.status, TaskStatus::Blocked);
    assert_eq!(found.assignee, Some(f.bo), "held at Bo's rank");
    assert!(
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .all(|d| d.task != Some(hard)),
        "nothing for the owner"
    );
    assert_eq!(
        hq.wakes().unwrap().len(),
        wakes_before,
        "nobody woken to fail it again"
    );
    let thread = hq.store().thread(hard).unwrap();
    let held: Vec<_> = thread
        .iter()
        .filter(|p| p.text.contains("waits for Rae"))
        .collect();
    assert_eq!(held.len(), 1, "said once: {thread:?}");
    // The Mac is free again: the item climbs to Rae.
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    assert_eq!(assignee(&hq, hard), Some(f.rae));
    assert!(woken(&hq, f.rae, hard, WakeReason::Assigned));
    assert_eq!(
        hq.loop_failures(hard).unwrap(),
        0,
        "counted afresh at Rae's rank"
    );
}

/// Review of 31 Oct: a reviewer's turn that ran out of room was never continued, since only
/// the item's holder was, and the review stalled. The reviewer goes on in a fresh review turn,
/// under the same cap; past it, the owner is asked about the item, which is no failure of the
/// worker's.
///
/// Given Bo's item in review, Rae reviewing it one rank up,
/// when four of her review turns in a row end at their prompt's budget,
/// then the first three wake her to review it afresh, and the fourth asks the owner; the item
/// stays in review and Bo's, with no failure.
#[test]
fn a_reviewers_turn_out_of_room_goes_on_in_a_fresh_review() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let hard = hard_item(&hq, &f);
    let bo = turn_of(&hq, f.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": hard.0 })).unwrap();
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": hard.0, "status": "in_review", "reason": "gluetun is up.",
                             "evidence": [format!("task {}", hard.0)] }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    hq.tell_mac(true, "ready");
    let reviews = |hq: &Hq<'_>| {
        hq.wakes()
            .unwrap()
            .iter()
            .filter(|w| {
                w.member == f.rae && w.task == Some(hard) && w.reason == WakeReason::ReviewRequested
            })
            .count()
    };
    assert_eq!(reviews(&hq), 1);
    for n in 1..=4 {
        let rae = turn_of(&hq, f.rae);
        hq.note_tokens(rae.run, 50_000, 0, 0, 500).unwrap();
        let said = use_tool(
            &hq,
            &rae,
            "post",
            &serde_json::json!({ "task": hard.0, "last_word": true, "continues": true,
                "text": "Reading gluetun's log; the turn's prompt reached its budget." }),
        )
        .unwrap();
        assert!(!said.contains("not continued"), "{said}");
        hq.runner().finish(rae.run, true);
        hq.tick().unwrap();
        if n <= 3 {
            assert_eq!(reviews(&hq), 1 + n, "review {n} goes on");
            let thread = hq.store().thread(hard).unwrap();
            assert!(
                thread
                    .iter()
                    .any(|p| p.author == "endora"
                        && p.text.contains(&format!("continuation {n} of 3"))),
                "{thread:?}"
            );
        }
    }
    assert_eq!(reviews(&hq), 4, "not past the cap");
    let found = hq.store().task(hard).unwrap().unwrap();
    assert_eq!(found.status, TaskStatus::InReview);
    assert_eq!(found.holder(), Some(f.bo));
    assert_eq!(hq.loop_failures(hard).unwrap(), 0, "no failure of Bo's");
    let desk = hq.store().desk().unwrap();
    assert!(
        desk.iter().any(|d| d.task == Some(hard)
            && d.status == "waiting"
            && d.text.contains("ran out of room 4 times")),
        "{desk:?}"
    );
}

/// D17 of round 3 (1 Nov): task 947 waited for its review, and each time it sat stranded HQ
/// counted it a failure at its holder's rank and woke the holder, whose wake HQ refused because
/// the work was in review: "Failure 1 of 3 at Mara's rank", "Failure 2 of 3", then it was
/// blocked as failed at every rank. Work in review waits on its reviewer, not its holder: a
/// stall there is no failure, does not climb, and asks the reviewer again.
///
/// Given Bo's item in review, and a reviewer whose turns end with no verdict,
/// when it sits stranded again and again,
/// then it has no failure, is still Bo's and in review, Bo was never woken (and so never
/// refused), and its review was asked for again: of the next reviewer, since a review turn
/// with neither verdict passes its reviewer over (D26).
#[test]
fn work_waiting_for_its_review_is_no_failure_of_its_holder() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.set_loop_rank(f.loop_, 1).unwrap();
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    hq.tick().unwrap();
    let mara = assignee(&hq, fix).unwrap();
    let bo = turn_of(&hq, mara);
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
    let reviewer = hq.wakes().unwrap().last().unwrap().member;
    assert_eq!(reviewer, f.ada, "Haiku reviews");
    for _ in 0..8 {
        hq.tick().unwrap();
        for started in hq.runner().started() {
            // The reviewer's turn reads the item, and ends with no verdict.
            if let Ok(Some(_)) = hq.turn_of(&started.token) {
                let _ = use_tool(&hq, &started, "my_turn", &serde_json::json!({}));
            }
            hq.runner().finish(started.run, true);
        }
        clock
            .0
            .fetch_add(super::engine::STRANDED_AFTER_MS + 60_000, Ordering::SeqCst);
    }
    assert_eq!(hq.loop_failures(fix).unwrap(), 0);
    let now = hq.store().task(fix).unwrap().unwrap();
    assert_eq!(now.status, TaskStatus::InReview);
    assert_eq!(now.holder(), Some(mara));
    let thread = hq.store().thread(fix).unwrap();
    assert!(
        thread.iter().all(|p| !p.text.contains("Failure")),
        "{thread:?}"
    );
    let bo_after: Vec<_> = wakes_on(&hq, mara, fix)
        .into_iter()
        .filter(|w| w.reason != WakeReason::Assigned)
        .collect();
    assert!(bo_after.is_empty(), "the holder is not woken: {bo_after:?}");
    let asked: Vec<_> = hq
        .wakes()
        .unwrap()
        .into_iter()
        .filter(|w| w.task == Some(fix) && w.reason == WakeReason::ReviewRequested)
        .collect();
    assert!(asked.len() >= 2, "the review is asked for again: {asked:?}");
    assert!(
        asked.iter().any(|w| w.member != reviewer),
        "the next reviewer is asked: {asked:?}"
    );
}

/// The fork's tools HQ marks up front for one turn, by name.
fn fork_up_front(hq: &Hq<'_>, context: &super::ports::RunContext) -> Vec<String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    super::protocol::node_tools_up_front(hq, &run, &member)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// D27 of round 3 (1 Nov): on the house model, Bo met a fresh, empty sandbox with
/// `repo_checkout` and `repo_submit` held back beside weather and news tools, and took his own
/// `git init` failures for an offline sandbox. A turn on a loop's item marks the fork's tools
/// up front: both for its holder, the checkout alone for its reviewer; none where the loop's
/// own list of tools leaves the fork out.
#[test]
fn a_loop_item_marks_the_forks_tools_up_front_for_its_turn() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.set_loop_rank(f.loop_, 1).unwrap();
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    hq.tick().unwrap();
    let holder = assignee(&hq, fix).expect("the item is handed out");
    let working = turn_of(&hq, holder);
    assert_eq!(
        fork_up_front(&hq, &working),
        ["repo_checkout", "repo_submit"]
    );
    use_tool(
        &hq,
        &working,
        "claim",
        &serde_json::json!({ "task": fix.0 }),
    )
    .unwrap();
    use_tool(
        &hq,
        &working,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "Done.",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    hq.runner().finish(working.run, true);
    let reviewer = hq.wakes().unwrap().last().unwrap().member;
    let review = turn_of(&hq, reviewer);
    assert_eq!(fork_up_front(&hq, &review), ["repo_checkout"]);
    // A loop whose list of tools leaves the fork out marks none.
    hq.set_project_tools(f.loop_, Some(vec!["homelab_status".to_owned()]))
        .unwrap();
    assert!(fork_up_front(&hq, &review).is_empty());
}

/// The tools HQ lists for one turn, by name.
fn listed_for(hq: &Hq<'_>, context: &super::ports::RunContext) -> Vec<String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    super::protocol::tools_in(hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect()
}

/// D30 of round 3 (1 Nov): the house-model replay and round 3's store show turns opening with
/// `my_turn`, which repeats the briefing they already hold, and `claim`. A turn on the item its
/// member holds is offered neither; HQ still answers both, for the worker that reads the
/// briefing and claims before the first round. A reviewer, whose item it is not, keeps them.
#[test]
fn a_turn_on_its_own_item_is_offered_no_protocol_moves() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    hq.set_loop_rank(f.loop_, 1).unwrap();
    let fix = item(&hq, &f, "Fix the lint", Priority::Normal);
    hq.tick().unwrap();
    let holder = assignee(&hq, fix).expect("the item is handed out");
    let working = turn_of(&hq, holder);
    let listed = listed_for(&hq, &working);
    for gone in ["my_turn", "claim"] {
        assert!(!listed.iter().any(|n| n == gone), "{gone} in {listed:?}");
    }
    assert!(listed.iter().any(|n| n == "post"), "{listed:?}");
    let briefing = use_tool(&hq, &working, "my_turn", &serde_json::json!({})).unwrap();
    assert!(briefing.contains("Fix the lint"), "{briefing}");
    use_tool(
        &hq,
        &working,
        "claim",
        &serde_json::json!({ "task": fix.0 }),
    )
    .unwrap();
    use_tool(
        &hq,
        &working,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "Done.",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    hq.runner().finish(working.run, true);
    let reviewer = hq.wakes().unwrap().last().unwrap().member;
    let review = turn_of(&hq, reviewer);
    let listed = listed_for(&hq, &review);
    assert!(listed.iter().any(|n| n == "my_turn"), "{listed:?}");
}

// ---- S1, a turn's own receipts attached to what it says uncited (climb 2, round 36) ----------

/// A receipt of the workbench's shell for `turn`, as the node records it: sandbox-reported.
fn shell_receipt(hq: &Hq<'_>, turn: &super::ports::RunContext, excerpt: &str) -> i64 {
    hq.record_sandbox_receipt(
        &turn.token,
        &super::engine::Called {
            tool: "shell".to_owned(),
            args: "./hostctl check [exit 1]".to_owned(),
            excerpt: excerpt.to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
}

/// The newest post `author` wrote on `task`.
fn newest_post(hq: &Hq<'_>, task: TaskId, author: &str) -> super::ports::Post {
    hq.store()
        .thread(task)
        .unwrap()
        .into_iter()
        .rev()
        .find(|p| p.author == author)
        .expect("a post of theirs")
}

/// RED (S1): a post that cites nothing attaches this run's own receipts for its task, marked as
/// attached by HQ and not cited; another run's receipts are not attached; and receipts the
/// sandbox reported, attached, leave an uncited post unsupported as before.
#[test]
fn s1_an_uncited_post_attaches_this_runs_receipts_and_sandbox_alone_stays_unsupported() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let first = item(&hq, &f, "Look after the host", Priority::Normal);
    let earlier = turn_of(&hq, f.bo);
    let stale = shell_receipt(&hq, &earlier, "FAIL accounts: jdoe LOCKED (earlier)");
    hq.runner().finish(earlier.run, true);
    hq.store().set_task_status(first, TaskStatus::Done).unwrap();
    let second = item(&hq, &f, "Look after the host again", Priority::Normal);
    let now = turn_of(&hq, f.bo);
    let fresh = shell_receipt(&hq, &now, "FAIL accounts: jdoe LOCKED");
    use_tool(
        &hq,
        &now,
        "post",
        &serde_json::json!({ "task": second.0, "text": "The host reads FAIL on accounts." }),
    )
    .unwrap();
    let post = newest_post(&hq, second, "Bo");
    assert!(
        post.text.contains(&format!(
            "Attached by HQ, not cited: r{fresh} (sandbox-reported)"
        )),
        "{}",
        post.text
    );
    assert!(
        !post.text.contains(&format!("r{stale}")),
        "another run's receipt: {}",
        post.text
    );
    assert!(
        post.unsupported,
        "sandbox receipts, attached, do not clear the marker"
    );
}

/// GUARD (S1): a turn with a receipt the node witnessed still clears the marker, as before.
#[test]
fn s1_a_witnessed_receipt_still_supports_an_uncited_post() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let task = item(&hq, &f, "Look after the host", Priority::Normal);
    let now = turn_of(&hq, f.bo);
    hq.record_receipt(
        &now.token,
        &super::engine::Called {
            tool: "homelab_status".to_owned(),
            args: String::new(),
            excerpt: "all up".to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap();
    use_tool(
        &hq,
        &now,
        "post",
        &serde_json::json!({ "task": task.0, "text": "All up." }),
    )
    .unwrap();
    assert!(!newest_post(&hq, task, "Bo").unsupported);
}

/// GUARD (S1): a post that cites its evidence gets no attachment; the citation stands alone.
#[test]
fn s1_a_cited_post_gets_no_attachment() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let task = item(&hq, &f, "Look after the host", Priority::Normal);
    let now = turn_of(&hq, f.bo);
    let id = shell_receipt(&hq, &now, "FAIL accounts: jdoe LOCKED");
    use_tool(
        &hq,
        &now,
        "post",
        &serde_json::json!({ "task": task.0, "text": "Locked.", "evidence": [format!("r{id}")] }),
    )
    .unwrap();
    assert!(!newest_post(&hq, task, "Bo").text.contains("Attached by HQ"));
}

/// The turn the loop has started on `task`, whoever's it is.
fn a_turn_on(hq: &Hq<'_>, task: TaskId) -> super::ports::RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.task == Some(task))
        .expect("a turn on the item")
}

/// S1 (round 36): a question carries no attached receipts, so the same request from two items,
/// each turn with its own ten receipts, stays one card on the owner's desk, as before.
#[test]
fn s1_the_same_question_with_different_receipts_stays_one_card() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let first = item(&hq, &f, "Look after the host", Priority::Normal);
    let mara = hq.store().members().unwrap()[0].id;
    let other = hq
        .add_project(f.goal, "another loop", "other work", mara, None)
        .unwrap();
    let second = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: f.goal,
                project: Some(other),
                parent: None,
                title: "Look after the other host".to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let ask = "Account jdoe is locked after 5 failed logins: unlock it, or leave it locked?";
    for task in [first, second] {
        let turn = a_turn_on(&hq, task);
        for n in 0..10 {
            shell_receipt(&hq, &turn, &format!("reading {n} of #{}", task.0));
        }
        use_tool(&hq, &turn, "ask_owner", &serde_json::json!({ "text": ask })).unwrap();
    }
    let cards: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "question" && d.status == "waiting")
        .collect();
    assert_eq!(cards.len(), 1, "one ask, one card: {cards:#?}");
    assert!(
        !cards[0].text.contains("Attached by HQ"),
        "{}",
        cards[0].text
    );
}

/// S1 (round 36): a long shell command's status survives HQ's own cut of a receipt's arguments
/// (120 characters), because the node writes it first; the stored receipt says it.
#[test]
fn s1_a_long_commands_status_survives_hqs_cut() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    item(&hq, &f, "Look after the host", Priority::Normal);
    let now = turn_of(&hq, f.bo);
    let id = hq
        .record_sandbox_receipt(
            &now.token,
            &super::engine::Called {
                tool: "shell".to_owned(),
                args: format!(
                    "[exit 1] [output cut: 2500 of 4000 bytes] ./hostctl check {}",
                    "--verbose ".repeat(60)
                ),
                excerpt: "FAIL accounts: jdoe LOCKED".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    let stored = hq.store().call_receipt(id).unwrap().expect("the receipt");
    assert!(stored.sandbox);
    assert!(
        stored
            .args
            .starts_with("[exit 1] [output cut: 2500 of 4000 bytes] ./hostctl"),
        "{}",
        stored.args
    );
}

/// S1 (round 36), the disclosed change for Claude Code turns: receipts a turn's sandbox reported
/// (its hook's Bash) leave an uncited post unsupported; a post citing one of them is a citation,
/// judged as before, and not marked.
#[test]
fn s1_a_claude_code_turns_sandbox_receipts_leave_an_uncited_post_unsupported() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let f = five(&hq);
    let task = item(&hq, &f, "Look after the host", Priority::Normal);
    let now = turn_of(&hq, f.bo);
    let bash = hq
        .record_sandbox_receipt(
            &now.token,
            &super::engine::Called {
                tool: "Bash".to_owned(),
                args: "git status".to_owned(),
                excerpt: "clean".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    use_tool(
        &hq,
        &now,
        "post",
        &serde_json::json!({ "task": task.0, "text": "Clean." }),
    )
    .unwrap();
    assert!(
        newest_post(&hq, task, "Bo").unsupported,
        "a sandbox's word alone"
    );
    use_tool(
        &hq,
        &now,
        "post",
        &serde_json::json!({ "task": task.0, "text": "Clean, see the receipt.", "evidence": [format!("r{bash}")] }),
    )
    .unwrap();
    assert!(
        !newest_post(&hq, task, "Bo").unsupported,
        "a citation is judged as before"
    );
}
