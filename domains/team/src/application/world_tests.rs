//! The team's world (ADR 0122): the house log and the lessons moved in once and nothing
//! deleted; stages and trust worked out at read time; canon the owner's alone, with a visible
//! record; stories with a handle; each turn's situation, its budget and what it left out named;
//! and a handover note whenever work is handed on.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Called, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext};
use super::protocol::call;
use super::world_ports::WorldStore;
use crate::domain::world::Stage;
use crate::domain::{GoalId, MemberId, Priority, Role, TaskId};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// 18 October 2026, at noon UTC.
const OCT_18: i64 = 1_792_324_800_000;
const DAY: i64 = 24 * 60 * 60 * 1000;

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
}

/// Mara the CEO, Ada the lead on Sonnet, Linus a developer on the house's model, and Dee the
/// tester, who reports to Ada.
fn team(hq: &Hq<'_>) -> Team {
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
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

/// The newest turn of `member`'s, once the tick has started what is queued. The tester's work
/// is the Mac's (ADR 0131), so the node says it is free.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

/// A reading the node witnessed in the turn: its receipt.
fn reading(hq: &Hq<'_>, context: &RunContext) -> String {
    let id = hq
        .record_receipt(
            &context.token,
            &Called {
                tool: "homelab_status".to_owned(),
                args: String::new(),
                excerpt: "backups: last ran 03:00".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    format!("r{id}")
}

#[test]
fn the_export_reads_newest_first_and_each_page_once_with_the_screens_role_names() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    team(&hq);
    for (at, line) in [
        (OCT_18 - 3 * DAY, "An older line."),
        (OCT_18 - DAY, "A newer line."),
    ] {
        hq.store()
            .add_timeline(
                &crate::domain::world_migration::TimelineLine {
                    page: "house".to_owned(),
                    at,
                    by: "Mara".to_owned(),
                    line: line.to_owned(),
                },
                None,
            )
            .unwrap();
    }
    let md = hq.world_markdown().unwrap();
    let newer = md.find("A newer line.").expect("the newer line");
    let older = md.find("An older line.").expect("the older line");
    assert!(newer < older, "newest first\n{md}");
    assert!(
        md.contains("newest first"),
        "the heading line says so\n{md}"
    );
    let lab = md
        .lines()
        .position(|l| l == "## How the lab is built (world:lab)")
        .unwrap_or_else(|| panic!("a clean heading\n{md}"));
    let lines: Vec<&str> = md.lines().collect();
    assert!(lines[lab + 1].is_empty(), "{md}");
    let first = lines[lab + 2];
    assert!(first.starts_with("- "), "{md}");
    assert_eq!(md.matches(first).count(), 1, "the lab's lines once\n{md}");
    assert!(!md.contains("(world:lab):"), "{md}");
    assert!(md.contains("- Mara (CEO, "), "{md}");
    assert!(md.contains("- Ada (Lead, "), "{md}");
    assert!(md.contains("- Linus (Developer, "), "{md}");
    assert!(md.contains("- Dee (Tester, "), "{md}");
}

#[test]
fn the_owner_writes_a_section_and_a_timeline_entry_that_the_team_reads_and_members_cannot_replace()
{
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let address = hq
        .write_as_owner(
            "house",
            "nas",
            "The NAS",
            "The NAS has 16 GB.\nSwap fills at night.",
        )
        .unwrap();
    assert_eq!(address, "world:house#nas");
    let section = hq.store().sections().unwrap().remove(0);
    assert_eq!(
        (
            section.stage,
            section.written_by.as_str(),
            section.headline.as_str(),
            section.body.as_str()
        ),
        (
            Stage::Insight,
            "the owner",
            "The NAS has 16 GB.",
            "Swap fills at night."
        )
    );
    // Replacing it keeps what it said.
    hq.write_as_owner("house", "nas", "The NAS", "The NAS has 32 GB.")
        .unwrap();
    let section = hq.store().sections().unwrap().remove(0);
    assert_eq!(
        (section.revision, section.headline.as_str()),
        (2, "The NAS has 32 GB.")
    );
    assert_eq!(
        hq.store().section_revisions(section.id).unwrap(),
        vec![
            (1, "The NAS has 16 GB.".to_owned()),
            (2, "The NAS has 32 GB.".to_owned())
        ]
    );
    // A page nobody writes by hand, and a section with nothing in it, are refused.
    assert!(hq.write_as_owner("lab", "x", "X", "y").is_err());
    assert!(hq.write_as_owner("house", "x", "X", "  ").is_err());
    assert!(hq.write_as_owner("nowhere", "x", "X", "y").is_err());
    // The timeline entry: a date at its start is HQ's, a year out is mended and recorded.
    hq.add_to_timeline_as_owner("house", None, "2025-10-18: The owner moved the router.")
        .unwrap();
    let line = hq.store().timeline("house").unwrap().pop().unwrap();
    assert_eq!(
        (line.by.as_str(), line.at, line.line.as_str()),
        ("the owner", OCT_18, "The owner moved the router.")
    );
    assert!(hq.add_to_timeline_as_owner("lab", None, "x").is_err());

    // It is in the export, and in every member's briefing, first.
    assert!(hq.world_markdown().unwrap().contains("The NAS has 32 GB."));
    // On an item, the sections that name what it names (ADR 0142, amended 2026-10-30).
    task_for(&hq, t.goal, t.lead, "Look over the NAS");
    let ada = turn(&hq, t.lead);
    assert!(
        ada.told
            .contains("- The NAS has 32 GB. [insight] (world:house#nas)"),
        "{}",
        ada.told
    );
    task_for(&hq, t.goal, t.dev, "Fix the NAS's swap");
    let linus = turn(&hq, t.dev);
    assert!(linus.told.contains("(world:house#nas)"), "{}", linus.told);
    // A member's own section on the same key is refused: it is the owner's.
    task_for(&hq, t.goal, t.ceo, "Review");
    let mara = turn(&hq, t.ceo);
    let fresh = reading(&hq, &mara);
    let refused = use_tool(
        &hq,
        &mara,
        "write_world",
        &json!({ "heading": "The NAS", "anchor": "nas", "headline": "Overwritten.", "stage": "insight",
                 "warrant": "read", "evidence": [fresh] }),
    );
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(
        hq.store().sections().unwrap()[0].headline,
        "The NAS has 32 GB."
    );
}
