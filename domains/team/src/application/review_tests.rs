//! The reviewer before the owner's desk: a member's question about the home lab is a draft the
//! reviewer checks first, sent back or passed on; and whatever reaches the desk lists the
//! names the home lab does not have.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::json;

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, DeskItem, NewRoutine, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::routines::HOUR_MS;
use crate::domain::{MemberId, Priority, Role, TaskId, TaskStatus, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

struct Team {
    dev: MemberId,
    reviewer: Option<MemberId>,
    task: TaskId,
}

/// A lead, a developer and (when `with_reviewer`) a reviewer; a goal, with the home lab's
/// routine when `lab`; and a task of the developer's under it, whose turn has started.
fn team(clock: &Hands, with_reviewer: bool, lab: bool) -> (Hq<'_>, Team, RunContext) {
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    );
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let reviewer = with_reviewer.then(|| hq.hire("Grace", Role::Reviewer, 20, 2, 1).unwrap());
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    if lab {
        hq.add_routine(
            Author::Owner,
            NewRoutine {
                title: "Check the home lab".to_owned(),
                description: "Look at the home lab with homelab_status.".to_owned(),
                goal,
                project: None,
                assignee: lead,
                every_ms: 6 * HOUR_MS,
                on_lab_trouble: true,
            },
        )
        .unwrap();
    }
    let task = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: "Look into the media apps".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let context = hq
        .runner()
        .started()
        .into_iter()
        .find(|c| c.member == dev)
        .expect("the developer's turn");
    (
        hq,
        Team {
            dev,
            reviewer,
            task,
        },
        context,
    )
}

/// Calls a tool as the turn `context` is.
fn use_tool(
    hq: &Hq<'_>,
    context: &RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

fn questions(hq: &Hq<'_>) -> Vec<DeskItem> {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "question")
        .collect()
}

fn last_wake(hq: &Hq<'_>) -> (MemberId, Option<TaskId>, WakeReason) {
    let w = hq.store().wakes().unwrap().into_iter().next_back().unwrap();
    (w.member, w.task, w.reason)
}

/// The developer's draft has gone to the reviewer; its turn ends and the reviewer's starts.
fn reviewers_turn(hq: &Hq<'_>, t: &Team, dev_turn: &RunContext) -> RunContext {
    hq.runner().finish(dev_turn.run, true);
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| Some(c.member) == t.reviewer)
        .expect("the reviewer's turn")
}

#[test]
fn a_home_lab_question_is_a_draft_for_the_reviewer_not_a_card_on_the_desk() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, true);
    let said = use_tool(
        &hq,
        &context,
        "ask_owner",
        &json!({ "text": "Crappy_Cray failed a ping test. Shall I restart it?" }),
    )
    .unwrap();
    assert!(said.contains("with the reviewer"), "{said}");
    assert!(
        questions(&hq).is_empty(),
        "nothing reached the owner's desk"
    );
    let task = hq.store().task(t.task).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.owner, Some(t.dev), "the draft's task is its asker's");
    let draft = hq.store().thread(t.task).unwrap().pop().unwrap();
    assert_eq!(
        (draft.author.as_str(), draft.text.as_str()),
        (
            "Linus",
            "Draft for the owner: Crappy_Cray failed a ping test. Shall I restart it?"
        )
    );
    assert_eq!(
        last_wake(&hq),
        (
            t.reviewer.unwrap(),
            Some(t.task),
            WakeReason::ReviewRequested
        )
    );
    assert!(
        hq.turn_of(&context.token).unwrap().is_none(),
        "asking ended the developer's turn"
    );

    let review = reviewers_turn(&hq, &t, &context);
    assert!(
        review.told.contains(
            "Linus wrote this draft for the owner: Crappy_Cray failed a ping test. Shall I \
             restart it?"
        ),
        "{}",
        review.told
    );
    assert!(review.told.contains(
        "Check every claim in the draft against your tools and the receipts it cites (below, as \
         the node recorded them) (homelab_status, container_logs, the queues). If everything \
         holds, call ask_owner with the draft, corrected if needed."
    ));
    // Live, a reviewer forwarded a draft that only asked the owner for what the queues could
    // tell: such a draft goes back, never on.
    assert!(
        review
            .told
            .contains("Send back, never forward, a draft that asks the owner for something"),
        "{}",
        review.told
    );
    assert!(
        review
            .told
            .contains("Send back any problem that no tool shows causing harm."),
        "{}",
        review.told
    );
    assert!(review.told.contains("review"), "{}", review.told);
}

#[test]
fn the_reviewer_sends_a_draft_back_and_its_member_is_woken() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, true);
    use_tool(&hq, &context, "claim", &json!({ "task": t.task.0 })).unwrap();
    use_tool(
        &hq,
        &context,
        "ask_owner",
        &json!({ "text": "Restart `Crappy_Cray`? It failed a ping test." }),
    )
    .unwrap();
    let review = reviewers_turn(&hq, &t, &context);
    let said = use_tool(
        &hq,
        &review,
        "review",
        &json!({ "task": t.task.0, "verdict": "send_back",
                 "notes": "the container does not exist" }),
    )
    .unwrap();
    assert!(said.contains("sent back"), "{said}");
    let task = hq.store().task(t.task).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Doing);
    assert_eq!(task.owner, Some(t.dev), "still the developer's");
    assert_eq!(
        hq.store().thread(t.task).unwrap().pop().unwrap().text,
        "Sent back: the container does not exist"
    );
    assert_eq!(last_wake(&hq), (t.dev, Some(t.task), WakeReason::SentBack));
    assert!(questions(&hq).is_empty());
}

#[test]
fn the_reviewers_own_question_reaches_the_desk_with_what_the_lab_does_not_have() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, true);
    hq.set_lab_names(vec!["sonarr".to_owned(), "radarr".to_owned()]);
    use_tool(
        &hq,
        &context,
        "ask_owner",
        &json!({ "text": "Restart `sonarr` and `Crappy_Cray`?" }),
    )
    .unwrap();
    let review = reviewers_turn(&hq, &t, &context);
    let said = use_tool(
        &hq,
        &review,
        "ask_owner",
        &json!({ "text": "Restart `sonarr` and `Crappy_Cray`?" }),
    )
    .unwrap();
    assert!(said.contains("owner's desk"), "{said}");
    let asked = questions(&hq);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].text,
        "Grace asks: Restart `sonarr` and `Crappy_Cray`?\nNot found in Endora's home lab: \
         Crappy_Cray",
        "flagged on the card, never removed"
    );
    assert_eq!(asked[0].member, t.reviewer);
    // The owner's answer reaches the reviewer who asked: its verdict comes after.
    hq.runner().finish(review.run, true);
    hq.tick().unwrap();
    hq.answer(asked[0].id, "sonarr only.").unwrap();
    assert_eq!(
        last_wake(&hq),
        (
            t.reviewer.unwrap(),
            Some(t.task),
            WakeReason::ProposalDecided
        )
    );
}

#[test]
fn with_no_reviewer_a_question_goes_straight_to_the_desk_still_checked() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, false, true);
    hq.set_lab_names(vec!["sonarr".to_owned()]);
    let said = use_tool(
        &hq,
        &context,
        "ask_owner",
        &json!({ "text": "Crappy_Cray failed a ping test. Restart it?" }),
    )
    .unwrap();
    assert!(said.contains("owner's desk"), "{said}");
    let asked = questions(&hq);
    assert_eq!(
        asked[0].text,
        "Linus asks: Crappy_Cray failed a ping test. Restart it?\nNot found in Endora's home \
         lab: Crappy_Cray"
    );
    assert_ne!(
        hq.store().task(t.task).unwrap().unwrap().status,
        TaskStatus::InReview
    );
}

#[test]
fn a_question_off_the_home_lab_naming_no_container_goes_to_the_desk() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, false);
    use_tool(
        &hq,
        &context,
        "ask_owner",
        &json!({ "text": "Steel or titanium first?" }),
    )
    .unwrap();
    assert_eq!(questions(&hq).len(), 1);
    assert_eq!(questions(&hq)[0].member, Some(t.dev));

    // Naming a container the lab has sends it to the reviewer, routine or not.
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, false);
    hq.set_lab_names(vec!["sonarr".to_owned()]);
    use_tool(
        &hq,
        &context,
        "ask_owner",
        &json!({ "text": "sonarr keeps restarting. Raise its memory?" }),
    )
    .unwrap();
    assert!(questions(&hq).is_empty());
    assert_eq!(
        last_wake(&hq),
        (
            t.reviewer.unwrap(),
            Some(t.task),
            WakeReason::ReviewRequested
        )
    );
}

/// The developer claims its task and moves it to review.
fn to_review(hq: &Hq<'_>, t: &Team, context: &RunContext) {
    use_tool(hq, context, "claim", &json!({ "task": t.task.0 })).unwrap();
    use_tool(
        hq,
        context,
        "post",
        &json!({ "task": t.task.0, "text": "sonarr's queue is clear now." }),
    )
    .unwrap();
    use_tool(
        hq,
        context,
        "set_status",
        &json!({ "task": t.task.0, "status": "in_review", "reason": "ready for a look",
                 "evidence": [format!("task {}", t.task.0)] }),
    )
    .unwrap();
}

/// A reading the node recorded for a turn: its receipt's id.
fn reading(hq: &Hq<'_>, context: &RunContext, tool: &str) -> i64 {
    hq.record_receipt(
        &context.token,
        &super::engine::Called {
            tool: tool.to_owned(),
            args: String::new(),
            excerpt: "sonarr running, healthy".to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
}

/// The lead's turn, once woken.
fn leads_turn(hq: &Hq<'_>, lead: MemberId, after: &RunContext) -> RunContext {
    hq.runner().finish(after.run, true);
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == lead)
        .expect("the lead's turn")
}

#[test]
fn work_moved_to_review_wakes_the_reviewer_and_its_owner_waits() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, true);
    to_review(&hq, &t, &context);
    assert_eq!(
        last_wake(&hq),
        (
            t.reviewer.unwrap(),
            Some(t.task),
            WakeReason::ReviewRequested
        ),
        "live, a task sat in review with nobody woken to look"
    );
    let review = reviewers_turn(&hq, &t, &context);
    assert!(
        review.told.contains("Linus asks for your review."),
        "{}",
        review.told
    );
    assert!(review.told.contains(
        "You are the tester: re-check it yourself. Take your own readings (homelab_status, \
         container_logs, the queues)"
    ));
    assert!(review.told.contains("You are Grace, the team's tester."));
    assert!(
        review.told.contains(&format!(
            "It was submitted on this evidence:\nThe work:\n  - task {}",
            t.task.0
        )),
        "{}",
        review.told
    );
    assert!(
        review
            .told
            .contains("Send back any problem that no tool shows causing harm."),
        "{}",
        review.told
    );
    // Its owner takes no turn on it meanwhile, except for the owner's own word.
    let receipt = hq
        .wake(t.dev, Some(t.task), WakeReason::AssignmentRecovery)
        .unwrap();
    assert_eq!(
        receipt,
        crate::domain::Receipt::Refused(crate::domain::Refused::InReview)
    );
    let receipt = hq.wake(t.dev, Some(t.task), WakeReason::Mentioned).unwrap();
    assert_eq!(
        receipt,
        crate::domain::Receipt::Refused(crate::domain::Refused::InReview)
    );
    hq.post(Author::Owner, t.task, "Thanks, looks right.")
        .unwrap();
    assert_eq!(
        last_wake(&hq),
        (t.dev, Some(t.task), WakeReason::OwnerReplied)
    );
    assert!(!matches!(
        hq.store().wakes().unwrap().last().unwrap().receipt,
        crate::domain::Receipt::Refused(_)
    ));
}

#[test]
fn the_tester_passes_on_her_own_readings_and_then_the_lead_accepts_it_as_done() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, true);
    to_review(&hq, &t, &context);
    let review = reviewers_turn(&hq, &t, &context);
    let pass = |evidence: serde_json::Value| {
        use_tool(
            &hq,
            &review,
            "review",
            &json!({ "task": t.task.0, "verdict": "approve", "notes": "The queue is clear.",
                     "evidence": evidence }),
        )
    };
    // Re-checked herself, or not passed: another member's word is no reading of hers.
    assert_eq!(
        pass(json!([format!("task {}", t.task.0)])).unwrap_err(),
        crate::domain::Refusal::NoOwnReading.to_string()
    );
    let mine = reading(&hq, &review, "homelab_status");
    let said = pass(json!([[format!("r{mine}")]])).unwrap();
    assert!(said.contains("approved"), "{said}");
    let task = hq.store().task(t.task).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::InReview, "the lead accepts it");
    let ada = hq.store().members().unwrap()[0].id;
    assert_eq!(
        last_wake(&hq),
        (ada, Some(t.task), WakeReason::AcceptanceAsked)
    );
    assert_eq!(
        hq.store().thread(t.task).unwrap().pop().unwrap().text,
        format!("Tested and passed: The queue is clear.\nEvidence: r{mine} (homelab_status)")
    );

    let lead = leads_turn(&hq, ada, &review);
    assert!(
        lead.told.contains(&format!(
            "The tester passed it on this evidence:\nThe work:\n  - r{mine} homelab_status (ok): \
             sonarr running, healthy"
        )),
        "{}",
        lead.told
    );
    let accept = |evidence: serde_json::Value| {
        use_tool(
            &hq,
            &lead,
            "accept",
            &json!({ "task": t.task.0, "verdict": "accept", "notes": "Holds.",
                     "evidence": evidence }),
        )
    };
    assert!(accept(json!([])).unwrap_err().contains("needs evidence"));
    let said = accept(json!([format!("r{mine}")])).unwrap();
    assert!(said.contains("done"), "{said}");
    assert_eq!(
        hq.store().task(t.task).unwrap().unwrap().status,
        TaskStatus::Done
    );
}

#[test]
fn the_lead_accepts_only_once_the_tester_has_passed_it_and_never_its_own() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, true);
    to_review(&hq, &t, &context);
    let ada = hq.store().members().unwrap()[0].id;
    // Ada takes a turn of her own on the routine's work and tries to accept Linus's early.
    let review = reviewers_turn(&hq, &t, &context);
    hq.runner().finish(review.run, true);
    hq.tick().unwrap();
    hq.wake(ada, Some(t.task), WakeReason::Mentioned).unwrap();
    hq.tick().unwrap();
    let lead = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == ada)
        .expect("the lead's turn");
    let early = use_tool(
        &hq,
        &lead,
        "accept",
        &json!({ "task": t.task.0, "verdict": "accept", "notes": "Fine.",
                 "evidence": [format!("task {}", t.task.0)] }),
    )
    .unwrap_err();
    assert_eq!(early, crate::domain::Refusal::NotTested.to_string());
    // Her send-back needs no pass: it is a verdict, and counts a round.
    use_tool(
        &hq,
        &lead,
        "accept",
        &json!({ "task": t.task.0, "verdict": "send_back", "notes": "Say which queue." }),
    )
    .unwrap();
    assert_eq!(
        hq.store().task(t.task).unwrap().unwrap().status,
        TaskStatus::Doing
    );
    assert_eq!(last_wake(&hq), (t.dev, Some(t.task), WakeReason::SentBack));
}

#[test]
fn work_that_needs_the_gate_stays_in_review_and_the_lead_is_told_why() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, false);
    to_review(&hq, &t, &context);
    let review = reviewers_turn(&hq, &t, &context);
    let mine = reading(&hq, &review, "homelab_status");
    use_tool(
        &hq,
        &review,
        "review",
        &json!({ "task": t.task.0, "verdict": "approve", "notes": "Looks fine.",
                 "evidence": [format!("r{mine}")] }),
    )
    .unwrap();
    let ada = hq.store().members().unwrap()[0].id;
    let lead = leads_turn(&hq, ada, &review);
    let refused = use_tool(
        &hq,
        &lead,
        "accept",
        &json!({ "task": t.task.0, "verdict": "accept", "notes": "Fine.",
                 "evidence": [format!("r{mine}")] }),
    )
    .unwrap_err();
    assert!(refused.contains("needs the gate"), "{refused}");
    assert_eq!(
        hq.store().task(t.task).unwrap().unwrap().status,
        TaskStatus::InReview
    );
}

#[test]
fn only_the_reviewer_gives_a_verdict_and_never_on_its_own_work() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, true, true);
    use_tool(&hq, &context, "claim", &json!({ "task": t.task.0 })).unwrap();
    let refused = use_tool(
        &hq,
        &context,
        "review",
        &json!({ "task": t.task.0, "verdict": "approve", "notes": "Mine is fine." }),
    )
    .unwrap_err();
    assert!(refused.contains("not yours"), "{refused}");

    // The reviewer's own work in review: nobody else reviews, so it is on the owner's desk,
    // and its own verdict is refused.
    let reviewer = t.reviewer.unwrap();
    let own = hq
        .add_task(
            Author::Owner,
            NewTask {
                goal: hq.store().task(t.task).unwrap().unwrap().goal,
                project: None,
                parent: None,
                title: "Read the logs".to_owned(),
                description: String::new(),
                assignee: Some(reviewer),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    hq.tick().unwrap();
    let hers = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == reviewer)
        .expect("the reviewer's turn on her own task");
    use_tool(&hq, &hers, "claim", &json!({ "task": own.0 })).unwrap();
    use_tool(
        &hq,
        &hers,
        "set_status",
        &json!({ "task": own.0, "status": "in_review", "reason": "done reading",
                 "evidence": [format!("task {}", own.0)] }),
    )
    .unwrap();
    // Nobody else tests it: its lead accepts it alone, and the thread says so.
    let ada = hq.store().members().unwrap()[0].id;
    assert_eq!(
        last_wake(&hq),
        (ada, Some(own), WakeReason::AcceptanceAsked)
    );
    assert!(
        hq.store().thread(own).unwrap().iter().any(|p| p.text
            == "No tester but the one who did this work: Ada, its lead, accepts it alone."),
    );
    let refused = use_tool(
        &hq,
        &hers,
        "review",
        &json!({ "task": own.0, "verdict": "approve", "notes": "Fine." }),
    )
    .unwrap_err();
    assert!(refused.contains("different member"), "{refused}");
}

#[test]
fn with_no_tester_the_lead_accepts_alone_and_with_neither_it_is_on_the_desk_to_read() {
    let clock = Hands(AtomicI64::new(1_792_166_400_000));
    let (hq, t, context) = team(&clock, false, true);
    let ada = hq.store().members().unwrap()[0].id;
    hq.pause(ada, true).unwrap();
    to_review(&hq, &t, &context);
    let item = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.kind == "ready_to_read")
        .expect("on the desk to read");
    assert_eq!(item.task, Some(t.task));
    assert!(
        item.text
            .starts_with("Ready to read: Look into the media apps (nobody on the team reviews)"),
        "{}",
        item.text
    );
    assert!(item.text.contains("- Linus: sonarr's queue is clear now."));
}
