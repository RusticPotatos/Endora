//! The CEO and the chain of command (ADR 0111): a member's question goes up the chain, never
//! to the board's desk; whoever is asked answers in the thread, which wakes the asker; only the
//! CEO's decision, checked by the reviewer, reaches the board; the CEO wakes only for that
//! and its own work; and the pool is shared among the paid alone.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use serde_json::json;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::money::{CEO_MOST_A_TURN, MOST_A_TURN};
use crate::domain::{GoalId, MemberId, Priority, Refusal, Role, TaskId, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// 17 October 2026, at noon UTC.
const OCT_17: i64 = 1_792_238_400_000;

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// A public record, a plan and the team's standards, as HQ embeds them.
const LIBRARY: &[(&str, &str)] = &[
    (
        "docs/adr/0111-the-ceo-and-the-chain-of-command.md",
        "# 0111 — The CEO",
    ),
    ("docs/plans/endora-hq.md", "# Endora HQ: the design"),
    ("docs/standards.md", "# The team's standards"),
];

struct Team {
    ceo: MemberId,
    lead: MemberId,
    dev: MemberId,
    goal: GoalId,
}

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    )
    .with_docs(LIBRARY)
}

/// The CEO (Mara, seated as HQ seats it, on Opus), a lead on Sonnet, a developer as hired (on
/// Haiku), and a reviewer when `with_reviewer`; a goal.
fn team(hq: &Hq<'_>, with_reviewer: bool) -> Team {
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    if with_reviewer {
        hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap();
    }
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    Team {
        ceo,
        lead,
        dev,
        goal,
    }
}

fn task_for(hq: &Hq<'_>, goal: GoalId, to: MemberId, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: None,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

/// The newest turn of `member`'s, once the tick has started what is queued.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(
    hq: &Hq<'_>,
    context: &RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

#[test]
fn endora_seeds_the_house_log_with_how_the_lab_is_built_once_and_only_when_it_is_empty() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    assert!(hq.seed_house_log().unwrap(), "an empty log is seeded");
    let log = hq.house_log().unwrap();
    assert!(
        log.starts_with("How this lab is built:\n- The home lab is managed by ansible-nas"),
        "{log}"
    );
    assert!(
        log.contains("homelab_map") && log.contains("behind gluetun"),
        "{log}"
    );
    assert_eq!(log.lines().count(), 4, "a title and a line a sentence");
    let noted = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "house.noted")
        .unwrap();
    assert_eq!(noted.data["by"], "Endora", "said in the feed as Endora's");
    assert_eq!(noted.data["entry"], "How this lab is built");
    // Once: the owner clears it, and it stays clear.
    hq.set_house_log("").unwrap();
    assert!(!hq.seed_house_log().unwrap());
    assert_eq!(hq.house_log().unwrap(), "");
    // A log with text in it is never overwritten.
    let other = engine(&clock);
    other.set_house_log("- 17 Oct: The NAS has 16 GB.").unwrap();
    assert!(!other.seed_house_log().unwrap());
    assert_eq!(other.house_log().unwrap(), "- 17 Oct: The NAS has 16 GB.");
    other.set_house_log("").unwrap();
    assert!(
        !other.seed_house_log().unwrap(),
        "seeded or not, only ever the first time"
    );
}

#[test]
fn every_member_reads_endoras_public_library_and_nothing_else() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let task = task_for(&hq, t.goal, t.lead, "Plan the week");
    let lead = turn(&hq, t.lead);
    assert!(
        lead.told
            .contains("The team's standards: read_doc docs/standards.md"),
        "{}",
        lead.told
    );
    let standards = use_tool(
        &hq,
        &lead,
        "read_doc",
        &json!({ "path": "docs/standards.md" }),
    )
    .unwrap();
    assert!(
        standards.ends_with("# The team's standards"),
        "the lead reads the standards: {standards}"
    );
    let ceo = lead;
    let read = use_tool(
        &hq,
        &ceo,
        "read_doc",
        &json!({ "path": "docs/plans/endora-hq.md" }),
    )
    .unwrap();
    assert!(
        read.starts_with("docs/plans/endora-hq.md, part 1 of 1."),
        "{read}"
    );
    assert!(read.ends_with("# Endora HQ: the design"));
    let listed = use_tool(&hq, &ceo, "read_doc", &json!({ "path": "docs/adr" })).unwrap();
    assert_eq!(
        listed,
        "In docs/adr:\ndocs/adr/0111-the-ceo-and-the-chain-of-command.md"
    );
    for path in [
        "docs/adr/../../local.mk",
        "../local.mk",
        "docs/homelab.md",
        "/etc/hosts",
    ] {
        assert_eq!(
            use_tool(&hq, &ceo, "read_doc", &json!({ "path": path })).unwrap_err(),
            Refusal::NotADoc.to_string(),
            "{path}"
        );
    }
    assert!(
        use_tool(
            &hq,
            &ceo,
            "read_doc",
            &json!({ "path": "docs/adr/9999-none.md" })
        )
        .unwrap_err()
        .contains("no such document")
    );
    let _ = task;
}

#[test]
fn the_pool_is_split_by_what_each_member_judges_with_and_work_is_never_turned_away_for_money() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let snap = hq.snapshot().unwrap();
    let per_day: Vec<i64> = snap["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["per_day"].as_i64().unwrap())
        .collect();
    assert_eq!(
        per_day,
        [1_683_848, 601_374, 48_109],
        "35 on Opus to 25 on Sonnet to 40 on Haiku, at their prices, of $2.33"
    );

    // The developer's work is done on the house's model: it reserves nothing, and is never
    // out of budget, whatever the pool holds (ADR 0131).
    let a = task_for(&hq, t.goal, t.dev, "One");
    let dev = turn(&hq, t.dev);
    assert_eq!(hq.store().run(dev.run).unwrap().unwrap().reserved, 0);
    let b = task_for(&hq, t.goal, t.dev, "Two");
    assert_eq!(hq.store().task(b).unwrap().unwrap().assignee, Some(t.dev));
    let _ = a;

    // The CEO's turn may spend a dollar; the lead's decision forty cents.
    let task = task_for(&hq, t.goal, t.lead, "Plan");
    hq.wake(t.lead, Some(task), WakeReason::Decide).unwrap();
    let lead = turn(&hq, t.lead);
    assert_eq!(
        hq.store().run(lead.run).unwrap().unwrap().reserved,
        MOST_A_TURN
    );
    use_tool(
        &hq,
        &lead,
        "ask_owner",
        &json!({ "text": "Which first?", "evidence": [format!("task {}", lead.task.unwrap().0)] }),
    )
    .unwrap();
    hq.wake(t.ceo, Some(task), WakeReason::Decide).unwrap();
    let ceo = turn(&hq, t.ceo);
    assert_eq!(
        hq.store().run(ceo.run).unwrap().unwrap().reserved,
        CEO_MOST_A_TURN
    );
    assert!(
        hq.report_usage(ceo.run, 900_000).unwrap(),
        "inside a dollar"
    );
    assert!(!hq.report_usage(ceo.run, 200_000).unwrap(), "past it");
    let _ = task;
}
