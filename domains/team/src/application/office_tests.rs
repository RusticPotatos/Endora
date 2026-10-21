//! The open office (ADR 0119): the office as data seeded once, proposals the team decides
//! together by votes or by a quiet six hours, the owner's undo, reset and veto, and downtime:
//! a turn with no task for a member with nothing to do, on the NAS's own model, free (ADR 0131).

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::domain::office::{self, Kind};
use crate::domain::{
    GoalId, MemberId, Priority, Receipt, Refused, Role, RunStatus, WakeReason, money,
};
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
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
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

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, who) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &who, tool, args)
}

/// A turn of `member`'s about no task, started by a mention, ready for tool calls. The tester's
/// work is the Mac's (ADR 0131), so for the tester the node says it is free; the others read on
/// the NAS, and leave the Mac to her.
fn mention_turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    let tester = hq
        .store()
        .members()
        .unwrap()
        .iter()
        .any(|m| m.id == member && m.role == Role::Reviewer);
    if tester {
        hq.tell_mac(true, "ready");
    }
    hq.wake(member, None, WakeReason::Mentioned).unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn")
}

/// Ends every turn the runner has running, as a worker finishing.
fn end_all(hq: &Hq<'_>) {
    for context in hq.runner().started() {
        hq.runner().finish(context.run, true);
    }
    hq.tick().unwrap();
}

fn propose(hq: &Hq<'_>, context: &RunContext, changes: Value, why: &str) -> Result<String, String> {
    use_tool(
        hq,
        context,
        "office",
        &json!({ "action": "propose", "changes": changes, "why": why }),
    )
}

fn react(hq: &Hq<'_>, context: &RunContext, proposal: i64, vote: &str) -> Result<String, String> {
    use_tool(
        hq,
        context,
        "office",
        &json!({ "action": "react", "proposal": proposal, "vote": vote, "why": "fine" }),
    )
}

fn items(hq: &Hq<'_>) -> Vec<office::Item> {
    hq.store().office_items().unwrap()
}

fn events(hq: &Hq<'_>, kind: &str) -> Vec<Value> {
    hq.store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.data)
        .collect()
}

fn fish_tank() -> Value {
    json!([{ "op": "place", "kind": "fish_tank", "x": 10, "y": 3 }])
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

#[test]
fn the_snapshot_carries_the_items_the_catalog_and_the_open_proposals() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "the kitchen needs <b>life</b>").unwrap();
    let shown = hq.snapshot().unwrap()["office"].clone();
    assert_eq!(
        shown["items"].as_array().unwrap().len(),
        office::default_layout().len()
    );
    let first = &shown["items"][0];
    assert_eq!(first["kind"], "plant");
    assert_eq!(
        (first["w"].clone(), first["h"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(shown["catalog"].as_array().unwrap().len(), 16);
    let fish = shown["catalog"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "fish_tank")
        .unwrap();
    assert_eq!(fish["most"], 2);
    let open = &shown["proposals"][0];
    assert_eq!(open["by"], "Bo");
    assert_eq!(open["why"], "the kitchen needs <b>life</b>");
    assert_eq!(open["changes"][0], "add a fish tank in the commons");
    assert_eq!(open["votes"][0]["who"], "Bo");
    assert_eq!(open["votes"][0]["vote"], "yes");
    assert_eq!(shown["can_undo"], false);
}

// ---- deciding together ---------------------------------------------------------------------

#[test]
fn a_proposal_is_said_in_the_feed_and_is_neither_a_task_nor_work_on_the_board() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    let said = propose(
        &hq,
        &bo,
        json!([
            { "op": "move", "item": 5, "x": 19, "y": 18 },
            { "op": "place", "kind": "fish_tank", "x": 10, "y": 3 },
        ]),
        "the kitchen needs life",
    )
    .unwrap();
    assert!(said.contains("Proposal 1"), "{said}");
    let feed = events(&hq, "office.proposed");
    assert_eq!(feed.len(), 1);
    assert_eq!(
        feed[0]["text"],
        "Bo proposes: move the fern to the lounge; add a fish tank in the commons \u{2014} 'the kitchen needs life'"
    );
    // Nothing moved yet: the proposer's yes alone does not carry it.
    assert_eq!(items(&hq).len(), office::default_layout().len());
    // Not a task, not a desk item, not on the scorecard or the board.
    assert!(hq.store().tasks().unwrap().is_empty());
    assert!(hq.store().desk().unwrap().is_empty());
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["tasks"].as_array().unwrap().len(), 0);
    assert_eq!(snap["scorecard"]["today"]["turns"]["total"], 0);
}

#[test]
fn a_proposal_carries_on_a_second_yes_and_the_office_changes() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let before = items(&hq).len();
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "the kitchen needs life").unwrap();
    let cy = mention_turn(&hq, t.cy);
    let said = react(&hq, &cy, 1, "yes").unwrap();
    assert!(said.contains("carried"), "{said}");
    let now = items(&hq);
    assert_eq!(now.len(), before + 1);
    let tank = now.iter().find(|i| i.kind == Kind::FishTank).unwrap();
    assert_eq!((tank.x, tank.y, tank.placed_by.as_str()), (10, 3, "Bo"));
    let changed = events(&hq, "office.changed");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["by"], "Bo");
    assert!(
        changed[0]["text"]
            .as_str()
            .unwrap()
            .contains("add a fish tank in the commons"),
        "{changed:?}"
    );
    // It was Cy's turn that carried it: its Replay holds the event.
    assert_eq!(changed[0]["run"], cy.run.0);
    assert_eq!(
        hq.store().office_proposals().unwrap()[0].status,
        office::Status::Applied
    );
}

#[test]
fn a_majority_of_no_drops_a_proposal() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "").unwrap();
    let cy = mention_turn(&hq, t.cy);
    // One no against the proposer's yes is a tie: still open.
    react(&hq, &cy, 1, "no").unwrap();
    assert_eq!(
        hq.store().office_proposals().unwrap()[0].status,
        office::Status::Open
    );
    let dee = mention_turn(&hq, t.dee);
    let said = react(&hq, &dee, 1, "no").unwrap();
    assert!(said.contains("dropped"), "{said}");
    assert!(items(&hq).iter().all(|i| i.kind != Kind::FishTank));
    assert_eq!(events(&hq, "office.dropped").len(), 1);
}

#[test]
fn a_proposal_nobody_objects_to_carries_after_six_quiet_hours() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "").unwrap();
    clock.0.store(START + 6 * HOUR - 1, Ordering::SeqCst);
    hq.tick().unwrap();
    assert!(items(&hq).iter().all(|i| i.kind != Kind::FishTank));
    clock.0.store(START + 6 * HOUR, Ordering::SeqCst);
    hq.tick().unwrap();
    assert!(items(&hq).iter().any(|i| i.kind == Kind::FishTank));
    let changed = events(&hq, "office.changed");
    assert_eq!(changed[0]["via"], "quiet");
}

#[test]
fn a_contested_proposal_that_never_carries_is_dropped_after_two_days() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "").unwrap();
    let cy = mention_turn(&hq, t.cy);
    react(&hq, &cy, 1, "no").unwrap();
    clock.0.store(START + 47 * HOUR, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(
        hq.store().office_proposals().unwrap()[0].status,
        office::Status::Open
    );
    clock.0.store(START + 48 * HOUR, Ordering::SeqCst);
    hq.tick().unwrap();
    let kept = &hq.store().office_proposals().unwrap()[0];
    assert_eq!(kept.status, office::Status::Dropped);
    assert!(kept.outcome.contains("two days"), "{}", kept.outcome);
    assert!(items(&hq).iter().all(|i| i.kind != Kind::FishTank));
}

#[test]
fn each_member_votes_once_and_the_proposers_yes_is_already_counted() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "").unwrap();
    let again = react(&hq, &bo, 1, "yes").unwrap_err();
    assert!(again.contains("already voted"), "{again}");
    let cy = mention_turn(&hq, t.cy);
    react(&hq, &cy, 1, "no").unwrap();
    let twice = react(&hq, &cy, 1, "yes").unwrap_err();
    assert!(twice.contains("already voted"), "{twice}");
    let proposal = &hq.store().office_proposals().unwrap()[0];
    assert_eq!((proposal.yes(), proposal.no()), (1, 1));
    assert!(
        react(&hq, &cy, 9, "yes")
            .unwrap_err()
            .contains("no proposal 9")
    );
    assert!(
        react(&hq, &cy, 1, "maybe")
            .unwrap_err()
            .contains("yes or no")
    );
}

#[test]
fn a_change_the_rules_refuse_is_refused_when_proposed() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    for (change, said) in [
        (
            json!({ "op": "place", "kind": "plant", "x": 30, "y": 11 }),
            "door",
        ),
        (
            json!({ "op": "place", "kind": "plant", "x": 3, "y": 9 }),
            "desks",
        ),
        (
            json!({ "op": "place", "kind": "plant", "x": 12, "y": 7 }),
            "lane",
        ),
        (
            json!({ "op": "place", "kind": "plant", "x": 40, "y": 3 }),
            "off the floor",
        ),
        (
            json!({ "op": "place", "kind": "trampoline", "x": 12, "y": 3 }),
            "trampoline",
        ),
        (
            json!({ "op": "move", "item": 999, "x": 12, "y": 3 }),
            "no item 999",
        ),
        (json!({ "op": "place", "kind": "plant" }), "x"),
    ] {
        let refused = propose(&hq, &bo, json!([change]), "why not").unwrap_err();
        assert!(refused.contains(said), "{said}: {refused}");
    }
    let six = json!([
        { "op": "remove", "item": 1 }, { "op": "remove", "item": 2 }, { "op": "remove", "item": 3 },
        { "op": "remove", "item": 4 }, { "op": "remove", "item": 5 }, { "op": "remove", "item": 6 },
    ]);
    assert!(
        propose(&hq, &bo, six, "")
            .unwrap_err()
            .contains("at most 5")
    );
    assert!(
        propose(&hq, &bo, json!([]), "")
            .unwrap_err()
            .contains("changes something")
    );
    assert!(hq.store().office_proposals().unwrap().is_empty());
}

#[test]
fn a_proposal_is_checked_again_when_it_carries() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let spot = |kind: &str| json!([{ "op": "place", "kind": kind, "x": 9, "y": 4 }]);
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, spot("fern"), "").unwrap();
    let cy = mention_turn(&hq, t.cy);
    propose(&hq, &cy, spot("lamp"), "").unwrap();
    // The lamp carries first and takes the tile.
    let dee = mention_turn(&hq, t.dee);
    react(&hq, &dee, 2, "yes").unwrap();
    assert!(
        items(&hq)
            .iter()
            .any(|i| i.kind == Kind::Lamp && (i.x, i.y) == (9, 4))
    );
    // The fern's second yes comes too late: it no longer fits, and is dropped, not made.
    let said = react(&hq, &dee, 1, "yes").unwrap();
    assert!(
        said.contains("dropped") && said.contains("already there"),
        "{said}"
    );
    let fern = &hq.store().office_proposals().unwrap()[0];
    assert_eq!(fern.status, office::Status::Dropped);
    assert!(fern.outcome.contains("no longer fits"), "{}", fern.outcome);
    assert_eq!(
        items(&hq).iter().filter(|i| (i.x, i.y) == (9, 4)).count(),
        1
    );
}

#[test]
fn every_role_is_offered_the_office_tool() {
    for role in [
        Role::Ceo,
        Role::Lead,
        Role::Developer,
        Role::Researcher,
        Role::Reviewer,
    ] {
        let all = tools(role);
        let tool = all
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "office")
            .unwrap_or_else(|| panic!("{role:?} lacks the office tool"));
        assert!(tool["inputSchema"]["properties"].get("member").is_none());
        assert!(tool["description"].as_str().unwrap().contains("propose"));
    }
}

#[test]
fn looking_at_the_office_lists_the_items_and_the_open_proposals() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "life").unwrap();
    let seen = use_tool(&hq, &bo, "office", &json!({ "action": "look" })).unwrap();
    assert!(seen.contains("fern #5"), "{seen}");
    assert!(seen.contains("Proposal 1"), "{seen}");
    assert!(
        use_tool(&hq, &bo, "office", &json!({ "action": "dance" }))
            .unwrap_err()
            .contains("propose, react or look")
    );
}

// ---- the owner is the landlord ---------------------------------------------------------------

#[test]
fn the_owner_undoes_the_last_change_resets_the_office_and_vetoes_a_proposal() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    let seeded = items(&hq);
    assert_eq!(hq.undo_office().unwrap(), None, "nothing to undo yet");
    // A change carries; the owner undoes it.
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "").unwrap();
    let cy = mention_turn(&hq, t.cy);
    react(&hq, &cy, 1, "yes").unwrap();
    assert_eq!(items(&hq).len(), seeded.len() + 1);
    assert!(
        hq.snapshot().unwrap()["office"]["can_undo"]
            .as_bool()
            .unwrap()
    );
    let undone = hq.undo_office().unwrap().expect("something to undo");
    assert!(undone.contains("fish tank"), "{undone}");
    assert_eq!(items(&hq).len(), seeded.len());
    assert!(items(&hq).iter().all(|i| i.kind != Kind::FishTank));
    assert_eq!(
        events(&hq, "office.changed").last().unwrap()["via"],
        "owner_undo"
    );
    // Reset puts back the seeded layout, whatever the team did; and can itself be undone.
    let dee = mention_turn(&hq, t.dee);
    propose(
        &hq,
        &dee,
        json!([{ "op": "remove", "item": seeded[0].id }, { "op": "remove", "item": seeded[1].id }]),
        "clear space",
    )
    .unwrap();
    react(&hq, &cy, 2, "yes").unwrap();
    assert_eq!(items(&hq).len(), seeded.len() - 2);
    hq.reset_office().unwrap();
    assert_eq!(items(&hq).len(), office::default_layout().len());
    assert!(items(&hq).iter().all(|i| i.placed_by == "endora"));
    hq.undo_office().unwrap().expect("a reset can be undone");
    assert_eq!(items(&hq).len(), seeded.len() - 2);
    // A veto closes an open proposal at once, and only an open one.
    let ada = mention_turn(&hq, t.ada);
    propose(&hq, &ada, fish_tank(), "").unwrap();
    let open = hq
        .store()
        .office_proposals()
        .unwrap()
        .into_iter()
        .find(|p| p.status == office::Status::Open)
        .unwrap();
    hq.veto_office(open.id).unwrap();
    let vetoed = hq
        .store()
        .office_proposals()
        .unwrap()
        .into_iter()
        .find(|p| p.id == open.id)
        .unwrap();
    assert_eq!(vetoed.status, office::Status::Vetoed);
    assert!(
        react(&hq, &cy, open.id, "yes")
            .unwrap_err()
            .contains("closed")
    );
    assert!(hq.veto_office(open.id).is_err());
    assert!(hq.veto_office(99).is_err());
    assert_eq!(events(&hq, "office.vetoed").len(), 1);
}

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
    hq.furlough(t.dee).unwrap();
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
fn a_downtime_briefing_says_what_the_office_has_and_what_may_be_done_and_makes_no_tasks() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    for other in [t.ada, t.cy, t.dee] {
        hq.pause(other, true).unwrap();
    }
    let bo = mention_turn(&hq, t.bo);
    propose(&hq, &bo, fish_tank(), "the kitchen needs life").unwrap();
    // Bo's turn ends, and only Cy is free for the downtime turn that follows.
    hq.runner().finish(bo.run, true);
    hq.pause(t.bo, true).unwrap();
    hq.pause(t.cy, false).unwrap();
    hq.tick().unwrap();
    let turns = downtime_turns(&hq);
    assert_eq!(turns.len(), 1);
    let told = &turns[0].told;
    for said in [
        "downtime",
        "no task",
        "Proposal 1",
        "add a fish tank in the commons",
        "the kitchen needs life",
        "plant",
        "fish_tank (at most 2)",
        "at most 40",
        "react",
        "propose",
        "note",
    ] {
        assert!(told.contains(said), "{said}\n---\n{told}");
    }
    // It carries no quiz, so it is not told of the tool that answers one (ADR 0131, amended).
    assert!(!told.contains("answer_quiz"), "{told}");
    assert!(told.len() < 3_500, "keep it short: {}", told.len());
    // No work is made in a downtime turn.
    let cy = &turns[0];
    for (tool, args) in [
        (
            "create_task",
            json!({ "title": "x", "description": "y", "assignee": "Ada" }),
        ),
        ("hand_to", json!({ "task": 1, "to": "Ada", "why": "z" })),
        ("propose_project", json!({ "title": "p", "reason": "r" })),
    ] {
        let refused = use_tool(&hq, cy, tool, &args).unwrap_err();
        assert!(refused.contains("downtime"), "{tool}: {refused}");
    }
    assert!(hq.store().tasks().unwrap().is_empty());
    // It may react and propose.
    propose(
        &hq,
        cy,
        json!([{ "op": "place", "kind": "fern", "x": 12, "y": 6 }]),
        "green",
    )
    .unwrap();
    let marks = run_of(&hq, cy);
    assert_eq!(marks.status, RunStatus::Running);
}

/// Live, small models given only a list of coordinates and three things to do aimed plant
/// after plant at the rug's own tiles, slid one tile off when refused, and copied each other
/// round it. So the briefing says what is the member's own, how much there is of each kind,
/// how big the bigger things are, and that leaving the office alone is a good turn.
#[test]
fn a_downtime_briefing_says_what_is_yours_what_is_plentiful_how_big_things_are_and_that_nothing_will_do()
 {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.seed_office().unwrap();
    hq.store()
        .add_office_item(Kind::Plant, 10, 4, "Bo", START)
        .unwrap();
    let bos = items(&hq)
        .into_iter()
        .find(|i| i.placed_by == "Bo")
        .unwrap();
    let rug = items(&hq)
        .into_iter()
        .find(|i| i.kind == Kind::Rug)
        .unwrap();
    let plants = items(&hq).iter().filter(|i| i.kind == Kind::Plant).count();
    for other in [t.ada, t.cy, t.dee] {
        hq.pause(other, true).unwrap();
    }
    hq.tick().unwrap();
    let turns = downtime_turns(&hq);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].member, t.bo);
    let told = &turns[0].told;
    for said in [
        format!("You have placed: plant #{} (the commons, 10,4)", bos.id),
        format!("rug #{} (the commons, 11,4, 2 by 2)", rug.id),
        format!("plant ({plants})"),
        "None yet: fish tank".to_owned(),
        "your notes may say what you like".to_owned(),
        "Leaving the office as it is is a good turn too".to_owned(),
    ] {
        assert!(told.contains(&said), "{said}\n---\n{told}");
    }
    // A thing one tile square is not given a size: the list stays short.
    assert!(!told.contains("1 by 1"), "{told}");
    assert!(told.len() < 3_500, "keep it short: {}", told.len());
    // Cy has put nothing down, and is told so.
    end_all(&hq);
    hq.pause(t.bo, true).unwrap();
    hq.pause(t.cy, false).unwrap();
    hq.tick().unwrap();
    let cy = downtime_turns(&hq)
        .into_iter()
        .find(|c| c.member == t.cy)
        .expect("Cy's downtime turn");
    assert!(
        cy.told.contains("You have placed nothing yet"),
        "{}",
        cy.told
    );
}

#[test]
fn downtime_does_not_count_as_a_turn_on_the_scorecard_nor_stop_the_idle_look() {
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
    // A week of downtime turns and nothing else: the developer is still offered for furlough.
    let offers: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "furlough")
        .filter_map(|d| d.member)
        .collect();
    assert!(offers.contains(&t.bo), "{offers:?}");
}
