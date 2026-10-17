//! The CEO and the chain of command (ADR 0111): a member's question goes up the chain, never
//! to the board's desk; whoever is asked answers in the thread, which wakes the asker; only the
//! CEO's decision, checked by the reviewer, reaches the board; the CEO wakes only for that
//! and its own work; and the pool is shared among the paid alone.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Engine, EngineError, Settings};
use super::ports::{Clock, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::domain::money::{CEO_MOST_A_TURN, MOST_A_TURN};
use crate::domain::routines::HOUR_MS;
use crate::domain::{
    GoalId, MemberId, Priority, Receipt, Refusal, Refused, Role, TaskId, TaskStatus, WakeReason,
};
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

/// A public record and a plan, as HQ embeds them.
const LIBRARY: &[(&str, &str)] = &[
    (
        "docs/adr/0111-the-ceo-and-the-chain-of-command.md",
        "# 0111 — The CEO",
    ),
    ("docs/plans/endora-hq.md", "# Endora HQ: the design"),
];

struct Team {
    ceo: MemberId,
    lead: MemberId,
    dev: MemberId,
    reviewer: Option<MemberId>,
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

/// The CEO (Mara, seated as HQ seats it, on Opus), a lead on Sonnet, a developer on the
/// house's model, and a reviewer when `with_reviewer`; a goal.
fn team(hq: &Hq<'_>, with_reviewer: bool) -> Team {
    let ceo = hq.seat_ceo("Mara").unwrap().expect("seated");
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.set_brain(lead, "claude-sonnet-5-5").unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let reviewer = with_reviewer.then(|| hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap());
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    Team {
        ceo,
        lead,
        dev,
        reviewer,
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

fn last_wake(hq: &Hq<'_>) -> (MemberId, Option<TaskId>, WakeReason, Receipt) {
    let w = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    (w.member, w.task, w.reason, w.receipt)
}

fn thread(hq: &Hq<'_>, task: TaskId) -> Vec<String> {
    hq.store()
        .thread(task)
        .unwrap()
        .into_iter()
        .map(|p| format!("{}: {}", p.author, p.text))
        .collect()
}

fn desk_kinds(hq: &Hq<'_>) -> Vec<String> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .map(|d| d.kind)
        .collect()
}

#[test]
fn a_question_goes_up_the_chain_to_a_decision_on_the_boards_desk_and_back_down() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, true);
    let reviewer = t.reviewer.unwrap();
    let task = task_for(&hq, t.goal, t.dev, "Sonarr keeps running out of memory");

    // The developer asks: it goes to the lead, in the thread, and its turn ends.
    let dev = turn(&hq, t.dev);
    let said = use_tool(
        &hq,
        &dev,
        "ask_owner",
        &json!({ "text": "May I raise sonarr's memory limit to 2 GB?" }),
    )
    .unwrap();
    assert!(said.contains("It is with Ada"), "{said}");
    assert!(
        thread(&hq, task).contains(
            &"Linus: Question for Ada: May I raise sonarr's memory limit to 2 GB?".to_owned()
        ),
        "{:?}",
        thread(&hq, task)
    );
    assert!(
        hq.turn_of(&dev.token).unwrap().is_none(),
        "asking ends the turn"
    );
    assert_eq!(
        last_wake(&hq),
        (
            t.lead,
            Some(task),
            WakeReason::QuestionAsked,
            Receipt::Queued
        )
    );
    assert!(desk_kinds(&hq).is_empty(), "nothing reached the board");

    // The lead reads it, and asks the CEO in turn.
    let lead = turn(&hq, t.lead);
    assert!(
        lead.told
            .contains("Linus asks you: May I raise sonarr's memory limit to 2 GB?"),
        "{}",
        lead.told
    );
    assert!(
        lead.told
            .contains("Your questions (ask_owner) go to Mara, the CEO")
    );
    use_tool(
        &hq,
        &lead,
        "ask_owner",
        &json!({ "text": "Linus wants sonarr at 2 GB; the NAS has 16 GB. Spend it?",
                 "evidence": [format!("task {}", lead.task.unwrap().0)] }),
    )
    .unwrap();
    assert_eq!(
        last_wake(&hq),
        (
            t.ceo,
            Some(task),
            WakeReason::QuestionAsked,
            Receipt::Queued
        )
    );

    // The CEO decides it is the board's: a decision, checked by the reviewer first.
    let ceo = turn(&hq, t.ceo);
    assert!(ceo.told.contains("Ada asks you:"), "{}", ceo.told);
    let said = use_tool(
        &hq,
        &ceo,
        "ask_owner",
        &json!({ "decision": "Give sonarr 2 GB of the NAS's 16?",
                 "recommendation": "Yes: it is the one container short of memory.",
                 "why": "Linus found it killed for memory three times today.",
                 "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    assert!(said.contains("with the reviewer"), "{said}");
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::InReview
    );
    assert!(
        desk_kinds(&hq).is_empty(),
        "not yet: the reviewer checks it first"
    );
    assert_eq!(
        last_wake(&hq),
        (
            reviewer,
            Some(task),
            WakeReason::ReviewRequested,
            Receipt::Queued
        )
    );

    // The reviewer checks it and passes it on: on the board's desk, as the CEO's.
    let check = turn(&hq, reviewer);
    assert!(
        check
            .told
            .contains("Mara, the CEO, wrote this decision for the board"),
        "{}",
        check.told
    );
    let draft = "Decision: Give sonarr 2 GB of the NAS's 16?\nRecommendation: Yes: it is the \
                 one container short of memory.\nWhy: Its log shows it killed for memory three \
                 times.";
    // She passes it on only on evidence of her own (ADR 0114).
    assert!(
        use_tool(&hq, &check, "ask_owner", &json!({ "text": draft }))
            .unwrap_err()
            .contains("needs evidence")
    );
    use_tool(
        &hq,
        &check,
        "ask_owner",
        &json!({ "text": draft, "evidence": [format!("task {}", task.0)] }),
    )
    .unwrap();
    let desk = hq.store().desk().unwrap();
    assert_eq!(desk.len(), 1, "{desk:?}");
    assert_eq!(desk[0].kind, "decision");
    assert_eq!(
        desk[0].member,
        Some(t.ceo),
        "the CEO's, for the board's word to wake"
    );
    assert!(
        desk[0].text.ends_with("Checked by Grace, the reviewer."),
        "{}",
        desk[0].text
    );
    let snap = hq.snapshot().unwrap();
    assert_eq!(
        snap["desk"][0]["decision"],
        json!({ "decision": "Give sonarr 2 GB of the NAS's 16?",
                "recommendation": "Yes: it is the one container short of memory.",
                "why": "Its log shows it killed for memory three times." })
    );

    // The board approves, with a word: in the thread, and the CEO woken to act on it.
    hq.decide_saying(desk[0].id, "approve", "Only until the upgrade.")
        .unwrap();
    assert!(
        thread(&hq, task).contains(
            &"owner: The board approves: Give sonarr 2 GB of the NAS's 16?\nOnly until the upgrade."
                .to_owned()
        ),
        "{:?}",
        thread(&hq, task)
    );
    assert_eq!(
        last_wake(&hq),
        (
            t.ceo,
            Some(task),
            WakeReason::ProposalDecided,
            Receipt::Queued
        )
    );

    // The CEO answers the lead in the thread; the lead answers the developer.
    let ceo = turn(&hq, t.ceo);
    use_tool(
        &hq,
        &ceo,
        "post",
        &json!({ "task": task.0, "text": "Approved: 2 GB for sonarr until the upgrade." }),
    )
    .unwrap();
    let (to, on, why, _) = last_wake(&hq);
    assert_eq!(
        (to, on, why),
        (t.lead, Some(task), WakeReason::QuestionAnswered),
        "the lead, who asked"
    );
    hq.runner().finish(ceo.run, true);
    let lead = turn(&hq, t.lead);
    use_tool(
        &hq,
        &lead,
        "post",
        &json!({ "task": task.0, "text": "Yes, 2 GB: the board agreed." }),
    )
    .unwrap();
    let (to, on, why, _) = last_wake(&hq);
    assert_eq!(
        (to, on, why),
        (t.dev, Some(task), WakeReason::QuestionAnswered)
    );
}

#[test]
fn without_a_ceo_the_team_asks_the_owner_as_before_and_without_a_lead_it_asks_the_ceo() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq.set_goal("Ship the tracker", "", Author::Owner).unwrap();
    let task = task_for(&hq, goal, dev, "Steel or titanium?");
    let asked = turn(&hq, dev);
    use_tool(
        &hq,
        &asked,
        "ask_owner",
        &json!({ "text": "Steel or titanium?" }),
    )
    .unwrap();
    assert_eq!(desk_kinds(&hq), ["question"], "no CEO: the owner's desk");
    assert!(!asked.told.contains("Your questions (ask_owner) go to"));

    // A CEO, and the lead paused: the developer's question goes to the CEO.
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    hq.pause(lead, true).unwrap();
    let other = task_for(&hq, goal, dev, "Which strap?");
    let asked = turn(&hq, dev);
    assert!(asked.told.contains("go to Mara, the CEO"), "{}", asked.told);
    use_tool(
        &hq,
        &asked,
        "ask_owner",
        &json!({ "text": "Leather or rubber?" }),
    )
    .unwrap();
    assert_eq!(
        last_wake(&hq),
        (ceo, Some(other), WakeReason::QuestionAsked, Receipt::Queued)
    );
    assert_eq!(desk_kinds(&hq), ["question"], "nothing more on the desk");
    let _ = task;
}

#[test]
fn the_ceo_wakes_for_escalations_and_its_own_work_never_for_being_named() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let task = task_for(&hq, t.goal, t.lead, "Plan the week");
    let lead = turn(&hq, t.lead);
    use_tool(
        &hq,
        &lead,
        "post",
        &json!({ "task": task.0, "text": "@Mara have a look at this" }),
    )
    .unwrap();
    assert_eq!(
        last_wake(&hq),
        (
            t.ceo,
            Some(task),
            WakeReason::Mentioned,
            Receipt::Refused(Refused::NotForTheCeo)
        )
    );
    assert_eq!(
        hq.wake(t.ceo, Some(task), WakeReason::Alarm).unwrap(),
        Receipt::Refused(Refused::NotForTheCeo)
    );
    // Nobody hands the CEO work from below.
    let refused = hq
        .add_task(
            Author::Member(t.lead),
            NewTask {
                goal: t.goal,
                project: None,
                parent: Some(task),
                title: "Decide for me".to_owned(),
                description: String::new(),
                assignee: Some(t.ceo),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap_err();
    assert_eq!(refused, EngineError::Refused(Refusal::NotBelow));
}

#[test]
fn with_no_reviewer_the_ceos_decision_goes_straight_to_the_board_and_is_never_bare_text() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let routine = hq
        .add_routine(
            Author::Endora,
            NewRoutine {
                title: "Daily review".to_owned(),
                description: "Review the house.".to_owned(),
                goal: t.goal,
                project: None,
                assignee: t.ceo,
                every_ms: 24 * HOUR_MS,
                on_lab_trouble: false,
            },
        )
        .unwrap();
    hq.fire_routine(routine).unwrap();
    let ceo = turn(&hq, t.ceo);
    let bare = use_tool(&hq, &ceo, "ask_owner", &json!({ "text": "What now?" })).unwrap_err();
    assert!(bare.contains("`decision`"), "{bare}");
    assert_eq!(
        hq.ask_owner(ceo.run, "What now?").unwrap_err(),
        EngineError::Refused(Refusal::NotADecision)
    );
    // Nothing reaches the owner's desk without evidence from HQ's record (ADR 0114).
    let ask = |evidence: serde_json::Value| {
        use_tool(
            &hq,
            &ceo,
            "ask_owner",
            &json!({ "decision": "Hire a second developer?", "recommendation": "Not yet.",
                     "why": "The queue is short.", "evidence": evidence }),
        )
    };
    let unfounded = ask(json!([])).unwrap_err();
    assert!(unfounded.contains("needs evidence"), "{unfounded}");
    assert_eq!(
        ask(json!(["task 999"])).unwrap_err(),
        "task 999 is not in HQ's record"
    );
    assert!(hq.store().desk().unwrap().is_empty());
    let own = ceo.task.unwrap().0;
    let said = ask(json!([format!("#{own}")])).unwrap();
    assert!(said.contains("board's desk"), "{said}");
    let desk = hq.store().desk().unwrap();
    assert_eq!(
        (desk[0].kind.as_str(), desk[0].text.as_str()),
        (
            "decision",
            format!(
                "Decision: Hire a second developer?\nRecommendation: Not yet.\nWhy: The queue is \
                 short.\nEvidence: task {own}"
            )
            .as_str()
        )
    );
    // Declined: the thread says so and the CEO is woken; only the decision's words take.
    assert!(hq.decide_saying(desk[0].id, "merged", "").is_err());
    hq.decide_saying(desk[0].id, "decline", "").unwrap();
    assert_eq!(last_wake(&hq).2, WakeReason::ProposalDecided);
    assert!(
        thread(&hq, desk[0].task.unwrap())
            .last()
            .unwrap()
            .ends_with("The board declines: Hire a second developer?")
    );
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
fn the_house_log_is_the_ceos_to_keep_and_starts_the_ceos_and_the_leads_turns() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let task = task_for(&hq, t.goal, t.lead, "Plan the week");
    let lead = turn(&hq, t.lead);
    let refused = use_tool(&hq, &lead, "note_house", &json!({ "entry": "x" })).unwrap_err();
    assert_eq!(refused, Refusal::OnlyTheCeo.to_string());
    assert!(
        !tools(Role::Lead)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "note_house" || t["name"] == "read_doc"),
        "the lead is offered neither"
    );
    use_tool(
        &hq,
        &lead,
        "ask_owner",
        &json!({ "text": "What matters this week?", "evidence": [format!("task {}", lead.task.unwrap().0)] }),
    )
    .unwrap();
    let ceo = turn(&hq, t.ceo);
    assert!(
        ceo.told.contains("The house log, yours to keep"),
        "{}",
        ceo.told
    );
    let said = use_tool(
        &hq,
        &ceo,
        "note_house",
        &json!({ "entry": "The NAS has 16 GB; sonarr needs 2." }),
    )
    .unwrap();
    assert!(
        said.ends_with("- 17 Oct: The NAS has 16 GB; sonarr needs 2."),
        "{said}"
    );
    assert_eq!(
        hq.house_log().unwrap(),
        "- 17 Oct: The NAS has 16 GB; sonarr needs 2."
    );
    let long = "x".repeat(501);
    assert!(use_tool(&hq, &ceo, "note_house", &json!({ "entry": long })).is_err());
    use_tool(
        &hq,
        &ceo,
        "rewrite_house",
        &json!({ "log": "x".repeat(5_990) }),
    )
    .unwrap();
    let full = use_tool(&hq, &ceo, "note_house", &json!({ "entry": "one more" })).unwrap_err();
    assert!(full.contains("rewrite_house"), "{full}");
    // The owner edits it on the screen; the lead's next turn starts with it, read-only.
    hq.set_house_log("- 17 Oct: Sonarr may have 2 GB.").unwrap();
    assert_eq!(
        hq.snapshot().unwrap()["house_log"],
        "- 17 Oct: Sonarr may have 2 GB."
    );
    assert!(hq.set_house_log(&"x".repeat(6_001)).is_err());
    use_tool(
        &hq,
        &ceo,
        "post",
        &json!({ "task": task.0, "text": "Sonarr first." }),
    )
    .unwrap();
    hq.runner().finish(ceo.run, true);
    let lead = turn(&hq, t.lead);
    assert!(
        lead.told.contains(
            "The house log, kept by the CEO (read it; it is not yours to change):\n- 17 Oct: \
             Sonarr may have 2 GB."
        ),
        "{}",
        lead.told
    );
    let kinds: Vec<String> = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(kinds.contains(&"house.noted".to_owned()));
}

#[test]
fn the_ceo_reads_endoras_public_records_and_nothing_else() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let task = task_for(&hq, t.goal, t.lead, "Plan the week");
    let lead = turn(&hq, t.lead);
    assert_eq!(
        use_tool(
            &hq,
            &lead,
            "read_doc",
            &json!({ "path": "docs/plans/endora-hq.md" })
        )
        .unwrap_err(),
        Refusal::OnlyTheCeo.to_string()
    );
    use_tool(
        &hq,
        &lead,
        "ask_owner",
        &json!({ "text": "Which plan?", "evidence": [format!("task {}", lead.task.unwrap().0)] }),
    )
    .unwrap();
    let ceo = turn(&hq, t.ceo);
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
fn the_ceo_is_seated_once_with_opus_and_the_owner_may_rename_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let mara = hq.seat_ceo("Mara").unwrap().unwrap();
    let found = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == mara)
        .unwrap();
    assert_eq!(
        (found.role, found.brain.name(), found.share_percent),
        (Role::Ceo, "claude-opus-5-5", 35)
    );
    let hired = hq
        .store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "member.hired")
        .unwrap();
    assert_eq!(hired.data["name"], "Mara");
    assert_eq!(hq.seat_ceo("Mara").unwrap(), None, "once");
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    assert_eq!(hq.rename(mara, "  Jane   Doe ").unwrap(), "Jane Doe");
    assert_eq!(
        hq.rename(mara, "ada").unwrap_err(),
        EngineError::Refused(Refusal::NotAName)
    );
    assert!(hq.rename(mara, " ").is_err());
    assert!(hq.rename(mara, &"x".repeat(41)).is_err());
    assert_eq!(hq.seat_ceo("Mara").unwrap(), None, "renamed, still seated");
    // Opus is the CEO's alone.
    assert!(hq.set_brain(mara, "claude-sonnet-5-5").is_ok());
    let lead = hq.store().members().unwrap()[1].id;
    assert!(hq.set_brain(lead, "claude-opus-5-5").is_err());
}

#[test]
fn the_pool_is_split_between_the_paid_and_a_local_member_is_never_turned_away_for_money() {
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
        [1_361_110, 972_222, 0],
        "35 to 25 of $2.33; the local free"
    );

    // The developer, on the house's model, reserves nothing, and is never out of budget.
    let a = task_for(&hq, t.goal, t.dev, "One");
    let dev = turn(&hq, t.dev);
    assert_eq!(hq.store().run(dev.run).unwrap().unwrap().reserved, 0);
    let b = task_for(&hq, t.goal, t.dev, "Two");
    assert_eq!(hq.store().task(b).unwrap().unwrap().assignee, Some(t.dev));
    let _ = a;

    // The CEO's turn may spend a dollar; the lead's forty cents.
    let task = task_for(&hq, t.goal, t.lead, "Plan");
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

#[test]
fn the_ceos_briefing_holds_the_house_the_team_the_work_the_trouble_and_the_board() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    hq.set_goal_note(
        t.goal,
        "- An RSS sync that grabs nothing is normal. (Ada, 16 Oct)",
    )
    .unwrap();
    hq.add_routine(
        Author::Owner,
        NewRoutine {
            title: "Check the home lab".to_owned(),
            description: String::new(),
            goal: t.goal,
            project: None,
            assignee: t.lead,
            every_ms: 6 * HOUR_MS,
            on_lab_trouble: true,
        },
    )
    .unwrap();
    hq.alarm("sonarr became unhealthy at 11:58").unwrap();
    hq.notice(None, "Ada restarted sonarr: it was unhealthy.")
        .unwrap();
    let lab_task = hq.store().tasks().unwrap()[0].id;
    let lead = turn(&hq, t.lead);
    use_tool(
        &hq,
        &lead,
        "ask_owner",
        &json!({ "text": "Sonarr again: what now?", "evidence": [format!("task {}", lead.task.unwrap().0)] }),
    )
    .unwrap();
    let ceo = turn(&hq, t.ceo);
    for said in [
        "You are Mara, the team's CEO.",
        "The house log, yours to keep",
        "(empty)",
        "Your team: Ada (lead, Claude Sonnet), Linus (developer, Local).",
        "- #1 Keep the home lab healthy (active)",
        "  - An RSS sync that grabs nothing is normal. (Ada, 16 Oct)",
        &format!("- #{} Check the home lab (todo, Ada)", lab_task.0),
        "Home-lab trouble in the last 7 days (Endora's alarms):",
        "- 17 Oct: Endora raised an alarm: sonarr became unhealthy at 11:58.",
        "- 17 Oct, notice: Ada restarted sonarr: it was unhealthy. (waiting for the board)",
        "money beyond the team's pool",
        "hiring or letting go a member",
        "Ada asks you: Sonarr again: what now?",
        "read_doc",
    ] {
        assert!(ceo.told.contains(said), "{said}\n---\n{}", ceo.told);
    }
    assert!(!ceo.told.contains("run_gate"), "{}", ceo.told);
}

#[test]
fn the_ceo_answers_a_question_at_low_effort_its_review_at_medium_and_the_lead_at_medium() {
    use crate::domain::Effort;
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let task = task_for(&hq, t.goal, t.lead, "Plan the week");
    let lead = turn(&hq, t.lead);
    assert_eq!(lead.effort, Some(Effort::Medium), "the lead, on Sonnet");
    use_tool(
        &hq,
        &lead,
        "ask_owner",
        &json!({ "text": "Which first?", "evidence": [format!("task {}", lead.task.unwrap().0)] }),
    )
    .unwrap();
    let asked = turn(&hq, t.ceo);
    assert_eq!(asked.task, Some(task));
    assert_eq!(
        asked.effort,
        Some(Effort::Low),
        "answering a question is ordinary"
    );

    let dev_task = task_for(&hq, t.goal, t.dev, "Fix the feed");
    let dev = turn(&hq, t.dev);
    assert_eq!(dev.task, Some(dev_task));
    assert_eq!(dev.effort, None, "the house's model is told no effort");

    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    let routine = hq
        .add_routine(
            Author::Endora,
            NewRoutine {
                title: "Daily review".to_owned(),
                description: "Review the house.".to_owned(),
                goal: t.goal,
                project: None,
                assignee: t.ceo,
                every_ms: 24 * HOUR_MS,
                on_lab_trouble: false,
            },
        )
        .unwrap();
    hq.fire_routine(routine).unwrap();
    let review = turn(&hq, t.ceo);
    assert_eq!(review.effort, Some(Effort::Medium), "its daily review");
    // A proposal waiting for its decision: high (the owner's decision of 2026-10-17).
    let mine = task_for(&hq, t.goal, t.lead, "Look over the lab");
    let lead = turn(&hq, t.lead);
    use_tool(
        &hq,
        &lead,
        "propose_project",
        &json!({ "title": "Media stack", "reason": "it keeps failing", "tasks": [mine.0] }),
    )
    .unwrap();
    hq.runner().finish(review.run, true);
    hq.tick().unwrap();
    task_for(&hq, t.goal, t.ceo, "Weigh it");
    let weighs = turn(&hq, t.ceo);
    assert_eq!(weighs.effort, Some(Effort::High), "a project to decide");
}

#[test]
fn after_its_first_turn_the_ceo_is_told_only_what_changed_and_the_whole_on_its_review() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq, false);
    hq.set_goal_note(
        t.goal,
        "- An RSS sync that grabs nothing is normal. (Ada, 16 Oct)",
    )
    .unwrap();
    // A house with work on: twenty open tasks and a few notices on the board's desk.
    for n in 1..=20 {
        task_for(
            &hq,
            t.goal,
            t.dev,
            &format!("Look into container number {n}"),
        );
    }
    for n in 1..=5 {
        hq.notice(
            None,
            &format!("Ada restarted container number {n}: it was unhealthy."),
        )
        .unwrap();
    }
    let task = task_for(&hq, t.goal, t.lead, "Plan the week");
    let lead = turn(&hq, t.lead);
    use_tool(
        &hq,
        &lead,
        "ask_owner",
        &json!({ "text": "Which first?", "evidence": [format!("task {}", lead.task.unwrap().0)] }),
    )
    .unwrap();

    // Its first turn: everything, as there is no last turn to go from.
    let first = turn(&hq, t.ceo);
    for said in ["Open tasks:", "Its notes and lessons:", "Home-lab trouble"] {
        assert!(first.told.contains(said), "{said}\n---\n{}", first.told);
    }
    use_tool(
        &hq,
        &first,
        "check_back",
        &json!({ "minutes": 30, "why": "see the plan moved" }),
    )
    .unwrap();
    hq.runner().finish(first.run, true);
    hq.tick().unwrap();

    // Woken again with nothing new: the house, the team, the board's rules, and no list.
    clock.0.fetch_add(31 * 60_000, Ordering::SeqCst);
    let quiet = turn(&hq, t.ceo);
    assert_ne!(quiet.run, first.run);
    for said in [
        "The house log, yours to keep",
        "Your team: Ada (lead, Claude Sonnet), Linus (developer, Local).",
        "- #1 Keep the home lab healthy (active)",
        "money beyond the team's pool",
        "Nothing changed since your last turn",
        "You said you would check back: see the plan moved",
    ] {
        assert!(quiet.told.contains(said), "{said}\n---\n{}", quiet.told);
    }
    for gone in [
        "Open tasks:",
        "Its notes and lessons:",
        "Home-lab trouble",
        "The board's desk in the last 7 days",
    ] {
        assert!(!quiet.told.contains(gone), "{gone}\n---\n{}", quiet.told);
    }
    assert!(
        quiet.told.len() * 2 < first.told.len(),
        "under half as long: {} against {}\n{}",
        quiet.told.len(),
        first.told.len(),
        quiet.told
    );
    // The house and the team open every briefing, in one order.
    let house = quiet.told.find("The house log").unwrap();
    let team = quiet.told.find("Your team:").unwrap();
    let news = quiet.told.find("Nothing changed").unwrap();
    assert!(house < team && team < news, "{}", quiet.told);

    // Something moves: it is named, and nothing else is.
    use_tool(
        &hq,
        &quiet,
        "check_back",
        &json!({ "minutes": 30, "why": "look again" }),
    )
    .unwrap();
    hq.runner().finish(quiet.run, true);
    hq.tick().unwrap();
    clock.0.fetch_add(60_000, Ordering::SeqCst);
    let backups = task_for(&hq, t.goal, t.dev, "Check the backups");
    hq.drop_task(backups).unwrap();
    clock.0.fetch_add(30 * 60_000, Ordering::SeqCst);
    let moved = turn(&hq, t.ceo);
    assert_ne!(moved.run, quiet.run);
    for said in [
        "What changed since your last turn:",
        &format!("#{} Check the backups: created, for Linus", backups.0),
        &format!("#{} Check the backups: now dropped", backups.0),
    ] {
        assert!(moved.told.contains(said), "{said}\n---\n{}", moved.told);
    }
    assert!(!moved.told.contains("Open tasks:"), "{}", moved.told);
    hq.runner().finish(moved.run, true);
    hq.tick().unwrap();

    // Its daily review sees the whole again.
    let routine = hq
        .add_routine(
            Author::Endora,
            NewRoutine {
                title: "Daily review".to_owned(),
                description: "Review the house.".to_owned(),
                goal: t.goal,
                project: None,
                assignee: t.ceo,
                every_ms: 24 * HOUR_MS,
                on_lab_trouble: false,
            },
        )
        .unwrap();
    hq.fire_routine(routine).unwrap();
    let review = turn(&hq, t.ceo);
    assert_ne!(review.run, moved.run);
    for said in [
        "Open tasks:",
        &format!("#{} Plan the week", task.0),
        "Its notes and lessons:",
        "Home-lab trouble",
    ] {
        assert!(review.told.contains(said), "{said}\n---\n{}", review.told);
    }
}
