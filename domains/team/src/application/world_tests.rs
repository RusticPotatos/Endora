//! The team's world (ADR 0122): the house log and the lessons moved in once and nothing
//! deleted; stages and trust worked out at read time; canon the owner's alone, with a visible
//! record; stories with a handle; each turn's situation, its budget and what it left out named;
//! and a handover note whenever work is handed on.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Called, Engine, Settings};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use super::world_ports::WorldStore;
use crate::domain::situations::Situation;
use crate::domain::world::{Scope, Stage};
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
    tester: MemberId,
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
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let tester = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    Team {
        ceo,
        lead,
        dev,
        tester,
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

fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn end(hq: &Hq<'_>, clock: &Hands, context: &RunContext) {
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
    clock.0.fetch_add(3 * 60_000, Ordering::SeqCst);
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

fn handover() -> Value {
    json!({ "done": "The proxy config is read.", "not_done": "The restart.",
            "next": "Restart it after 22:00.", "watch_out": "The VPN drops for a minute." })
}

#[test]
fn the_house_log_and_the_lessons_move_in_once_and_nothing_old_is_deleted() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    assert!(hq.seed_house_log().unwrap());
    let log = format!(
        "{}\n- 16 Oct: The NAS has 16 GB.\n- 17 Oct 2026: Sonarr may have 2 GB.",
        hq.house_log().unwrap()
    );
    hq.set_house_log(&log).unwrap();
    let lesson = "- Closed too soon: the alarm came back two hours later. (Dee, 11 Oct)";
    hq.set_goal_note(t.goal, &format!("Owntone is being retired.\n{lesson}"))
        .unwrap();
    let note = hq.owner_note(t.dev, "Never restart at night.").unwrap();

    assert!(hq.migrate_world().unwrap(), "moved now");
    let timeline = hq.store().timeline("house").unwrap();
    let lines: Vec<(i64, &str, &str)> = timeline
        .iter()
        .map(|l| (l.at, l.by.as_str(), l.line.as_str()))
        .collect();
    assert_eq!(
        lines,
        vec![
            (OCT_18 - 2 * DAY, "the house log", "The NAS has 16 GB."),
            (OCT_18 - DAY, "the house log", "Sonarr may have 2 GB."),
        ],
        "every dated line at its date; how the lab is built is the lab page now"
    );
    let stories = hq.store().stories().unwrap();
    assert_eq!(stories.len(), 1);
    let story = &stories[0];
    assert_eq!(story.handle, "closed-too-soon");
    assert!(story.to_retell);
    assert_eq!(story.stage, Stage::Observation);
    assert_eq!(story.scope, Scope::Area(t.lead), "Dee's lead's area");
    assert_eq!(story.author, "Dee");

    // Nothing deleted: the house log, the goal's notes and the notebook are as they were.
    assert_eq!(hq.house_log().unwrap(), log);
    assert_eq!(
        hq.goal_note(t.goal).unwrap(),
        format!("Owntone is being retired.\n{lesson}")
    );
    let notes = hq.notebook(t.dev).unwrap();
    assert_eq!(
        notes.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![note],
        "the notebook stays where it is"
    );

    // A second start changes nothing.
    assert!(!hq.migrate_world().unwrap());
    assert_eq!(hq.store().timeline("house").unwrap().len(), 2);
    assert_eq!(hq.store().stories().unwrap().len(), 1);
    // The house log is read-only now; note_house adds to the timeline.
    assert!(hq.set_house_log("anything").is_err());

    // A briefing reads the lesson as a story, not twice.
    let task = task_for(&hq, t.goal, t.lead, "Look over the lab");
    let ada = turn(&hq, t.lead);
    assert!(
        ada.told
            .contains("Notes from the owner for this goal: Owntone is being retired.\n"),
        "{}",
        ada.told
    );
    assert!(!ada.told.contains("(Dee, 11 Oct)"), "{}", ada.told);
    assert!(
        ada.told
            .contains("- Story closed-too-soon: Closed too soon: the alarm came back two hours later. (to retell)"),
        "{}",
        ada.told
    );
    assert!(
        ada.told
            .contains("Lessons of your area to retell as stories"),
        "the lead is asked to retell: {}",
        ada.told
    );
    // The CEO, who keeps the house page, is asked to write its sections from the timeline.
    task_for(&hq, t.goal, t.ceo, "Review the house");
    let mara = turn(&hq, t.ceo);
    assert!(
        mara.told.contains(
            "No sections yet: write what is true of the house now, from its timeline, with \
             write_world"
        ),
        "{}",
        mara.told
    );
    assert!(
        mara.told.contains("the house log: Sonarr may have 2 GB."),
        "{}",
        mara.told
    );
    let _ = task;
}

#[test]
fn a_move_that_dated_the_house_log_a_year_out_is_repaired_once_and_nothing_is_lost() {
    use crate::domain::world_migration::{Plan, TimelineLine};
    // The move ran at 04:00 on 18 Oct 2026, and the live data came out as it did then.
    let early = OCT_18 - 8 * 3_600_000;
    let clock = Hands(AtomicI64::new(early));
    let hq = engine(&clock);
    assert_eq!(
        hq.repair_world_dates().unwrap(),
        0,
        "nothing to mend before the move"
    );
    hq.set_house_log("- 18 Oct: 18 Oct: #668 accepted.\n- 17 Oct: 2025-10-18: Taken.\n- 16 Oct 2025: A year ago.")
        .unwrap();
    let row = |at, line: &str| TimelineLine {
        page: "house".to_owned(),
        at,
        by: "the house log".to_owned(),
        line: line.to_owned(),
    };
    let wrong = Plan {
        timeline: vec![
            row(OCT_18 - 365 * DAY, "18 Oct: #668 accepted."),
            row(OCT_18 - DAY, "2025-10-18: Taken."),
            row(OCT_18 - 2 * DAY - 365 * DAY, "A year ago."),
        ],
        lessons: Vec::new(),
    };
    assert!(hq.store().migrate_world(&wrong, early).unwrap());

    assert_eq!(hq.repair_world_dates().unwrap(), 2, "two rows mended");
    let lines: Vec<(i64, String)> = hq
        .store()
        .timeline("house")
        .unwrap()
        .into_iter()
        .map(|l| (l.at, l.line))
        .collect();
    assert_eq!(
        lines,
        vec![
            (OCT_18 - 2 * DAY - 365 * DAY, "A year ago.".to_owned()),
            (OCT_18 - DAY, "Taken.".to_owned()),
            (OCT_18, "#668 accepted.".to_owned()),
        ],
        "the year of a yearless date; the writer's own year, and a date written twice"
    );
    assert_eq!(hq.repair_world_dates().unwrap(), 0, "once");
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
fn every_briefing_says_what_day_it_is_and_a_year_out_on_todays_date_is_corrected() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    for (member, who) in [
        (t.ceo, "Mara"),
        (t.lead, "Ada"),
        (t.dev, "Linus"),
        (t.tester, "Dee"),
    ] {
        task_for(&hq, t.goal, member, "Look");
        let told = turn(&hq, member).told;
        assert!(
            told.contains(&format!("You are {who}, the team's"))
                && told.contains(" Today is Sun 18 Oct 2026.\n"),
            "{told}"
        );
    }
    // The CEO writes a house note, a section, and a note, each starting with last year's date.
    task_for(&hq, t.goal, t.ceo, "Review");
    let mara = turn(&hq, t.ceo);
    let fresh = reading(&hq, &mara);
    use_tool(
        &hq,
        &mara,
        "note_house",
        &json!({ "entry": "2025-10-18: The NAS is fine." }),
    )
    .unwrap();
    use_tool(
        &hq,
        &mara,
        "write_world",
        &json!({ "heading": "Backups", "headline": "18 Oct 2025: Backups run at 03:00.",
                 "body": "2025-10-17: nightly", "stage": "insight", "warrant": "read",
                 "evidence": [fresh.clone()] }),
    )
    .unwrap();
    use_tool(
        &hq,
        &mara,
        "note",
        &json!({ "text": "Oct 18, 2025 the NAS is fine", "evidence": [fresh] }),
    )
    .unwrap();
    let section = &hq.store().sections().unwrap()[0];
    assert_eq!(section.headline, "18 Oct 2026: Backups run at 03:00.");
    assert_eq!(section.body, "2026-10-17: nightly");
    assert_eq!(
        hq.notebook(t.ceo).unwrap()[0].text,
        "Oct 18, 2026 the NAS is fine"
    );
    let corrected = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.data["what"] == "year corrected")
        .count();
    assert_eq!(corrected, 4, "each correction is recorded");
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
    task_for(&hq, t.goal, t.lead, "Look over the lab");
    let ada = turn(&hq, t.lead);
    assert!(
        ada.told
            .contains("- The NAS has 32 GB. [insight] (world:house#nas)"),
        "{}",
        ada.told
    );
    task_for(&hq, t.goal, t.dev, "Fix it");
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

#[test]
fn only_the_owner_makes_something_canon_and_a_demotion_is_said_with_its_reason() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    task_for(&hq, t.goal, t.ceo, "Review the house");
    let mara = turn(&hq, t.ceo);
    let fresh = reading(&hq, &mara);
    let write = |stage: &str, cited: &[String]| {
        use_tool(
            &hq,
            &mara,
            "write_world",
            &json!({ "heading": "Backups", "headline": "Backups run at 03:00.",
                     "stage": stage, "warrant": "the lab says so", "evidence": cited }),
        )
    };
    assert!(
        write("canon", &[fresh.clone()])
            .unwrap_err()
            .contains("only the owner")
    );
    assert!(
        write("insight", &["task 1".to_owned()])
            .unwrap_err()
            .contains("two different turns")
    );
    assert_eq!(
        write("insight", std::slice::from_ref(&fresh)).unwrap(),
        "Written: world:house#backups. Only the owner makes it canon."
    );
    end(&hq, &clock, &mara);

    // The lead may not write the house page, and may not make it canon; it proposes.
    task_for(&hq, t.goal, t.lead, "Look at the backups");
    let ada = turn(&hq, t.lead);
    let refused = use_tool(
        &hq,
        &ada,
        "write_world",
        &json!({ "heading": "Backups", "headline": "x", "stage": "observation",
                 "evidence": ["task 1"] }),
    )
    .unwrap_err();
    assert!(
        refused.contains("there is no tool") || refused.contains("Mara"),
        "{refused}"
    );
    use_tool(
        &hq,
        &ada,
        "propose_canon",
        &json!({ "address": "world:house#backups", "warrant": "seen every night this week" }),
    )
    .unwrap();
    let desk = hq.store().desk().unwrap();
    let item = desk.iter().find(|d| d.kind == "canon").unwrap();
    assert!(
        item.text.contains("Ada proposes making this canon"),
        "{}",
        item.text
    );
    assert!(
        item.text.contains("seen every night this week"),
        "{}",
        item.text
    );
    assert!(
        item.text.contains("homelab_status"),
        "its evidence: {}",
        item.text
    );
    assert_eq!(
        hq.store().sections().unwrap()[0].stage,
        Stage::Insight,
        "proposing changes nothing"
    );

    hq.decide(item.id, "approve").unwrap();
    let canon = hq.store().sections().unwrap().remove(0);
    assert_eq!(canon.stage, Stage::Canon);
    assert_eq!(canon.warrant, "the lab says so", "it keeps its warrant");
    assert_eq!(
        canon.evidence[0].said,
        "homelab_status: backups: last ran 03:00"
    );
    let timeline = hq.store().timeline("house").unwrap();
    assert!(
        timeline
            .last()
            .unwrap()
            .line
            .contains("Made canon by the owner on 18 Oct: on Ada's proposal"),
        "{timeline:?}"
    );
    // Canon is not the steward's to rewrite or retire.
    end(&hq, &clock, &ada);
    task_for(&hq, t.goal, t.ceo, "Again");
    let mara = turn(&hq, t.ceo);
    let fresh = reading(&hq, &mara);
    assert!(
        use_tool(
            &hq,
            &mara,
            "write_world",
            &json!({ "heading": "Backups", "headline": "Backups run at 04:00.",
                     "stage": "insight", "warrant": "w", "evidence": [fresh] }),
        )
        .unwrap_err()
        .contains("only the owner")
    );
    assert!(
        use_tool(
            &hq,
            &mara,
            "retire_world",
            &json!({ "address": "world:house#backups", "why": "moved" }),
        )
        .is_err()
    );

    // The owner demotes it, with a reason: the feed and the page say so.
    assert!(
        hq.demote("world:house#backups", Stage::Insight, " ")
            .is_err()
    );
    hq.demote(
        "world:house#backups",
        Stage::Insight,
        "the schedule moved to 04:00",
    )
    .unwrap();
    let section = hq.store().sections().unwrap().remove(0);
    assert_eq!(section.stage, Stage::Insight);
    assert!(
        section.why.contains("the schedule moved to 04:00"),
        "{}",
        section.why
    );
    let last = hq.store().timeline("house").unwrap().pop().unwrap();
    assert!(
        last.line.contains(
            "Demoted from canon to insight by the owner on 18 Oct: the schedule moved to 04:00"
        ),
        "{}",
        last.line
    );
    let demoted = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "world.changed")
        .unwrap();
    assert_eq!(demoted.data["what"], "demoted");
    assert_eq!(demoted.data["why"], "the schedule moved to 04:00");
    assert!(
        hq.demote("world:house#backups", Stage::Observation, "again")
            .is_err(),
        "not canon"
    );
}

#[test]
fn trust_is_read_from_dated_evidence_and_a_fresh_reading_restores_it() {
    let clock = Hands(AtomicI64::new(OCT_18 - 31 * DAY));
    let hq = engine(&clock);
    let t = team(&hq);
    task_for(&hq, t.goal, t.lead, "Look at the alarms");
    let ada = turn(&hq, t.lead);
    let fresh = reading(&hq, &ada);
    use_tool(
        &hq,
        &ada,
        "tell_story",
        &json!({ "handle": "closed-too-soon", "moment": "An alarm's task looks fixed after a restart.",
                 "reflex": "Close it at once.", "instead": "Wait an hour with no new alarm.",
                 "because": "It came back two hours later.", "evidence": [fresh],
                 "stage": "insight", "warrant": "read on the lab" }),
    )
    .unwrap();
    end(&hq, &clock, &ada);
    let shown = |hq: &Hq<'_>| hq.world_shown().unwrap()["stories"][0]["said"].clone();
    assert_eq!(shown(&hq), "insight");
    // A month and a day later, though nobody edited it.
    clock.0.store(OCT_18, Ordering::SeqCst);
    assert_eq!(shown(&hq), "unconfirmed since 17 Sep");
    task_for(&hq, t.goal, t.lead, "Look again");
    let ada = turn(&hq, t.lead);
    assert!(
        ada.told
            .contains("Because: It came back two hours later. [unconfirmed since 17 Sep]\n"),
        "it counts as an observation, and says so: {}",
        ada.told
    );
    // Only its steward confirms it, on a fresh witnessed reading.
    let fresh = reading(&hq, &ada);
    use_tool(
        &hq,
        &ada,
        "verify_world",
        &json!({ "address": "story:closed-too-soon", "evidence": [fresh] }),
    )
    .unwrap();
    assert_eq!(shown(&hq), "insight");
}

#[test]
fn a_story_is_told_in_its_tellers_scope_and_cited_by_its_handle() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, t.tester, "Test the fix");
    let dee = turn(&hq, t.tester);
    let tell = |handle: &str, stage: &str| {
        use_tool(
            &hq,
            &dee,
            "tell_story",
            &json!({ "handle": handle, "moment": "m", "reflex": "r", "instead": "i",
                     "because": "b", "evidence": [format!("task {}", task.0)],
                     "stage": stage, "warrant": "w" }),
        )
    };
    assert!(
        tell("one", "observation")
            .unwrap_err()
            .contains("not a handle")
    );
    assert!(
        tell("passed-on-faith", "insight")
            .unwrap_err()
            .contains("Ada"),
        "an area's insight is its lead's"
    );
    tell("passed-on-faith", "observation").unwrap();
    assert!(
        tell("passed-on-faith", "observation")
            .unwrap_err()
            .contains("already")
    );
    let story = hq.store().stories().unwrap().remove(0);
    assert_eq!(story.scope, Scope::Area(t.lead), "the tester's lead's area");
    assert!(
        use_tool(
            &hq,
            &dee,
            "tell_story",
            &json!({ "handle": "no-parts-here", "instead": "i", "evidence": ["task 1"] }),
        )
        .unwrap_err()
        .contains("moment")
    );
    // Cited by its handle in a post, it is used.
    clock.0.fetch_add(DAY, Ordering::SeqCst);
    use_tool(
        &hq,
        &dee,
        "post",
        &json!({ "task": task.0, "text": "Sent back: passed-on-faith." }),
    )
    .unwrap();
    let used = hq.store().stories().unwrap().remove(0);
    assert_eq!(used.last_used_at, OCT_18 + DAY);
    // Its read says what it is.
    let read = use_tool(
        &hq,
        &dee,
        "read_world",
        &json!({ "address": "story:passed-on-faith" }),
    )
    .unwrap();
    assert!(
        read.contains("Story passed-on-faith (Ada's area, observation)"),
        "{read}"
    );
}

#[test]
fn work_is_handed_on_only_with_a_handover_note_and_the_next_turn_starts_from_it() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let rae = hq.hire("Rae", Role::Developer, 40, 2, 5).unwrap();
    let task = task_for(&hq, t.goal, t.dev, "Fix the proxy");
    let linus = turn(&hq, t.dev);
    end(&hq, &clock, &linus);
    task_for(&hq, t.goal, t.lead, "Plan");
    let ada = turn(&hq, t.lead);
    let hand = |with: Option<Value>| {
        let mut args = json!({ "task": task.0, "to": "Rae", "why": "Linus is busy" });
        if let Some(note) = with {
            args["handover"] = note;
        }
        use_tool(&hq, &ada, "hand_to", &args)
    };
    assert!(hand(None).unwrap_err().contains("handover note"));
    let mut half = handover();
    half["watch_out"] = json!("");
    assert!(hand(Some(half)).unwrap_err().contains("handover note"));
    hand(Some(handover())).unwrap();
    let kept = hq.store().handovers(task).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!((kept[0].from, kept[0].to), (Some(t.dev), Some(rae)));
    end(&hq, &clock, &ada);

    let rae_turn = turn(&hq, rae);
    assert_eq!(rae_turn.task, Some(task));
    let briefed = hq.briefed(rae_turn.run).unwrap().unwrap();
    assert_eq!(briefed.situation, Situation::Handover);
    let first = rae_turn.told.lines().nth(1).unwrap();
    assert!(
        first.starts_with("Handed over by Ada on #")
            && first.contains("Watch out: The VPN drops for a minute."),
        "the note comes first: {}",
        rae_turn.told
    );
    assert!(briefed.briefed.contains(&"handover".to_owned()));
}

#[test]
fn each_turn_records_its_situation_and_a_recall_starts_with_what_changed() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, t.dev, "Look at the feed");
    let first = turn(&hq, t.dev);
    assert_eq!(
        hq.briefed(first.run).unwrap().unwrap().situation,
        Situation::Fresh
    );
    end(&hq, &clock, &first);
    hq.post(Author::Owner, task, "Anything?").unwrap();
    let again = turn(&hq, t.dev);
    assert_eq!(
        hq.briefed(again.run).unwrap().unwrap().situation,
        Situation::Continuing
    );
    end(&hq, &clock, &again);
    // A day later on the same task: after a gap.
    clock.0.fetch_add(DAY, Ordering::SeqCst);
    hq.post(Author::Owner, task, "And now?").unwrap();
    let later = turn(&hq, t.dev);
    assert_eq!(
        hq.briefed(later.run).unwrap().unwrap().situation,
        Situation::AfterGap
    );
    assert!(
        later.told.contains("What changed since 18 Oct")
            || later.told.contains("changed since 18 Oct"),
        "{}",
        later.told
    );
    end(&hq, &clock, &later);
    hq.drop_task(task).unwrap();

    // Furloughed for a week, and recalled.
    hq.furlough(t.dev).unwrap();
    clock.0.fetch_add(7 * DAY, Ordering::SeqCst);
    task_for(&hq, t.goal, t.ceo, "Weekly");
    let mara = turn(&hq, t.ceo);
    use_tool(
        &hq,
        &mara,
        "note_house",
        &json!({ "entry": "Sonarr moved to the new disk." }),
    )
    .unwrap();
    end(&hq, &clock, &mara);
    hq.recall(t.dev).unwrap();
    task_for(&hq, t.goal, t.dev, "Back to work");
    let back = turn(&hq, t.dev);
    let briefed = hq.briefed(back.run).unwrap().unwrap();
    assert_eq!(briefed.situation, Situation::Recalled);
    let after_name = back.told.lines().nth(1).unwrap();
    assert_eq!(after_name, "What changed since 19 Oct:", "{}", back.told);
    assert!(
        back.told.contains("Mara: Sonarr moved to the new disk."),
        "{}",
        back.told
    );
    assert!(
        back.told
            .contains("You were furloughed from 19 Oct to 26 Oct; read your notes first.")
    );
    assert!(
        briefed.dropped.is_empty(),
        "nothing was left out: {briefed:?}"
    );
    assert!(briefed.tokens <= briefed.budget);
    // The replay shows it.
    let replay = hq.replay_run(back.run).unwrap().shown();
    assert_eq!(replay["briefed"]["situation"], "recalled");
}

#[test]
fn the_house_models_briefing_keeps_to_its_budget_and_names_what_it_left_out() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    let task = task_for(&hq, t.goal, t.dev, "Tidy the logs");
    let linus = turn(&hq, t.dev);
    // Forty stories of his own, each a long one.
    for n in 0..40 {
        use_tool(
            &hq,
            &linus,
            "tell_story",
            &json!({ "handle": format!("long-story-{n}"),
                     "moment": "m".repeat(280), "reflex": "r".repeat(280),
                     "instead": "i".repeat(280), "because": "b".repeat(280),
                     "evidence": [format!("task {}", task.0)] }),
        )
        .unwrap();
    }
    end(&hq, &clock, &linus);
    clock.0.fetch_add(8 * DAY, Ordering::SeqCst);
    hq.post(Author::Owner, task, "Back to it").unwrap();
    let next = turn(&hq, t.dev);
    let briefed = hq.briefed(next.run).unwrap().unwrap();
    assert_eq!(briefed.situation, Situation::AfterGap);
    assert_eq!(
        briefed.budget,
        1_800 + 1_200,
        "the house's model, after a gap"
    );
    assert!(briefed.tokens <= briefed.budget, "{briefed:?}");
    assert!(!briefed.dropped.is_empty());
    let last = next.told.lines().last().unwrap();
    assert!(
        last.starts_with(
            "Not included this turn, to keep to your briefing's budget: story:long-story-"
        ) && last.ends_with("Read any of them with read_world (your notes with read_notebook)."),
        "{last}"
    );
    for dropped in &briefed.dropped {
        assert!(last.contains(dropped.as_str()), "{dropped} is named");
    }
    // And what was left out is still read whole.
    let read = use_tool(
        &hq,
        &next,
        "read_world",
        &json!({ "address": briefed.dropped[0] }),
    )
    .unwrap();
    assert!(read.starts_with("Story long-story-"), "{read}");
}

#[test]
fn the_world_exports_as_markdown_with_its_stages() {
    let clock = Hands(AtomicI64::new(OCT_18));
    let hq = engine(&clock);
    let t = team(&hq);
    task_for(&hq, t.goal, t.ceo, "Review");
    let mara = turn(&hq, t.ceo);
    let fresh = reading(&hq, &mara);
    use_tool(
        &hq,
        &mara,
        "write_world",
        &json!({ "heading": "Backups", "headline": "Backups run at 03:00.",
                 "body": "- nightly\n- to the NAS", "stage": "insight", "warrant": "read",
                 "evidence": [fresh] }),
    )
    .unwrap();
    let md = hq.world_markdown().unwrap();
    for said in [
        "# The team's world",
        "## The house (world:house)",
        "### Backups {#backups}",
        "**Backups run at 03:00.**",
        "Stage: insight",
        "- nightly\n- to the NAS",
        "### Timeline",
        "## How the lab is built (world:lab)",
        "## The team (world:team)",
        "- Mara (CEO, Claude Opus",
        "## Stories",
    ] {
        assert!(md.contains(said), "{said}\n---\n{md}");
    }
}
