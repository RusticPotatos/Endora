//! The engineering wing and the git flow (ADR 0115): a submitted branch recorded with its
//! document and check, the check's result as a receipt that wakes or sends back, the owner's
//! push offered only once the work is done and run only on their approval, hiring by the owner
//! and by a lead's request within the wing's seats, the wing seeded once, and documents read.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings, Submission};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::wing::{MOST_DEVELOPERS, WING_GOAL, WING_PROJECT};
use crate::domain::{
    Brain, Criterion, GoalId, MemberId, Priority, Refusal, Role, TaskId, TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const OCT_17: i64 = 1_792_166_400_000;

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(6),
        clock,
        Settings::default(),
    )
}

/// The team as the owner has it: the CEO, Ada the lead, Dee the tester, and the wing seeded.
struct Team {
    ceo: MemberId,
    ada: MemberId,
    dee: MemberId,
    kit: MemberId,
    rae: MemberId,
    goal: GoalId,
    lab: GoalId,
    plan: TaskId,
}

fn team(hq: &Hq<'_>) -> Team {
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    let lab = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let seeded = hq.seed_wing().unwrap().expect("the wing is seeded");
    Team {
        ceo,
        ada,
        dee,
        kit: seeded.lead,
        rae: seeded.developers[0],
        goal: seeded.goal,
        lab,
        plan: seeded.task,
    }
}

/// Kit hands Rae a branch's worth of work under the plan, with criteria.
fn raes_task(hq: &Hq<'_>, t: &Team) -> TaskId {
    hq.add_task_with(
        Author::Member(t.kit),
        NewTask {
            goal: t.goal,
            project: None,
            parent: Some(t.plan),
            title: "Tidy the deluge role".to_owned(),
            description: String::new(),
            assignee: Some(t.rae),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
        &[Criterion::new("The branch passes its checks", false)],
    )
    .unwrap()
}

/// The newest turn of `member`'s, after a tick or two to start it.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn")
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

fn submission(can_push: bool) -> Submission {
    Submission {
        branch: "hq/task-3-tidy-the-deluge-role".to_owned(),
        head: "a1b2c3d4e5f60718293a4b5c".to_owned(),
        base: "0123456789abcdef01234567".to_owned(),
        files: 2,
        plus: 10,
        minus: 3,
        stat: " roles/deluge/tasks/main.yml | 9 ++---\n 2 files changed".to_owned(),
        diff: "diff --git a/roles/deluge/tasks/main.yml\n-old\n+new\n".to_owned(),
        can_push,
    }
}

fn thread(hq: &Hq<'_>, task: TaskId) -> Vec<String> {
    hq.store()
        .thread(task)
        .unwrap()
        .into_iter()
        .map(|p| format!("{}: {}", p.author, p.text))
        .collect()
}

fn wakes_after(hq: &Hq<'_>, n: usize) -> Vec<(MemberId, WakeReason)> {
    hq.store()
        .wakes()
        .unwrap()
        .into_iter()
        .skip(n)
        .map(|w| (w.member, w.reason))
        .collect()
}

/// Rae submits her branch in her turn: the node's record of it, and what the check hands out.
fn submitted(
    hq: &Hq<'_>,
    t: &Team,
    can_push: bool,
) -> (TaskId, RunContext, super::engine::Recorded) {
    let task = raes_task(hq, t);
    let rae = turn(hq, t.rae);
    use_tool(hq, &rae, "claim", &json!({ "task": task.0 })).unwrap();
    let recorded = hq.record_branch(&rae.token, &submission(can_push)).unwrap();
    (task, rae, recorded)
}

#[test]
fn a_submitted_branch_is_recorded_with_its_document_and_a_check_is_queued_once() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    assert!(
        hq.record_branch("forged", &submission(true)).is_err(),
        "only a running turn's token"
    );
    let (task, rae, recorded) = submitted(&hq, &t, true);
    let branch = hq.store().branches().unwrap().pop().unwrap();
    assert_eq!(
        (
            branch.id,
            branch.task,
            branch.member,
            branch.status.as_str()
        ),
        (recorded.branch, task, t.rae, "submitted")
    );
    let document = hq.store().document(recorded.document).unwrap().unwrap();
    assert_eq!(
        document.title,
        "branch hq/task-3-tidy-the-deluge-role (2 files, +10 −3)"
    );
    assert!(document.text.contains("+new"), "the diff is the document");
    let last = thread(&hq, task).pop().unwrap();
    assert!(
        last.starts_with("endora: Submitted branch hq/task-3-tidy-the-deluge-role (2 files, +10 −3) at a1b2c3d4e5f6: document d")
            && last.contains(&format!("Check c{} is queued", recorded.check)),
        "{last}"
    );
    // The runner is handed the check once, with the job's own token; the token names it.
    let handed = hq.next_check().unwrap().expect("a check waits");
    assert_eq!(
        (handed.id, handed.status.as_str()),
        (recorded.check, "running")
    );
    assert_eq!(handed.token.len(), 64);
    assert!(hq.next_check().unwrap().is_none(), "handed out once");
    let (check, named) = hq
        .check_by_token(&handed.token)
        .unwrap()
        .expect("the job's token");
    assert_eq!(
        (check.id, named.name.as_str()),
        (recorded.check, "hq/task-3-tidy-the-deluge-role")
    );
    assert!(hq.check_by_token("not-a-token").unwrap().is_none());
    assert!(
        hq.check_by_token(&rae.token).unwrap().is_none(),
        "a turn's token opens no check"
    );
    // A branch not under hq/ is never recorded.
    let mut odd = submission(true);
    odd.branch = "main".to_owned();
    assert!(hq.record_branch(&rae.token, &odd).is_err());
    // The briefing of the next turn names the document and the branch.
    hq.runner().finish(rae.run, true);
    hq.tick().unwrap();
    hq.wake(t.rae, Some(task), WakeReason::OwnerReplied)
        .unwrap();
    let again = turn(&hq, t.rae);
    let told = use_tool(&hq, &again, "my_turn", &json!({})).unwrap();
    assert!(
        told.contains(&format!("d{} branch hq/task-3", recorded.document)),
        "{told}"
    );
    assert!(
        told.contains("Branch hq/task-3-tidy-the-deluge-role at a1b2c3d4e5f6 is submitted."),
        "{told}"
    );
    let read = use_tool(
        &hq,
        &again,
        "read_document",
        &json!({ "document": recorded.document }),
    )
    .unwrap();
    assert!(
        read.contains("(part 1 of 1)") && read.contains("+new"),
        "{read}"
    );
    assert!(use_tool(&hq, &again, "read_document", &json!({ "document": 999 })).is_err());
}

#[test]
fn a_check_that_passes_is_a_receipt_that_wakes_the_submitter_and_the_tester() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, recorded) = submitted(&hq, &t, true);
    let handed = hq.next_check().unwrap().unwrap();
    assert!(
        hq.check_done(handed.id, "wrong-token", true, "ok", "")
            .is_err(),
        "the job's token, or nothing"
    );
    let before = hq.store().wakes().unwrap().len();
    hq.check_done(
        handed.id,
        &handed.token,
        true,
        "syntax ok, ansible-lint 0 findings, yamllint ok",
        "ansible-playbook --syntax-check nas.yml ... playbook: nas.yml",
    )
    .unwrap();
    let receipt = hq.store().call_receipts_of(rae.run).unwrap().pop().unwrap();
    assert_eq!(
        (receipt.tool.as_str(), receipt.ok, receipt.act, receipt.task),
        ("check", true, false, Some(task))
    );
    assert!(
        receipt
            .args
            .starts_with(&format!("c{} hq/task-3", recorded.check)),
        "{}",
        receipt.args
    );
    let last = thread(&hq, task).pop().unwrap();
    assert!(
        last.contains("syntax ok, ansible-lint 0 findings, yamllint ok")
            && last.contains(&format!("receipt r{}", receipt.id)),
        "{last}"
    );
    let woken = wakes_after(&hq, before);
    assert!(woken.contains(&(t.rae, WakeReason::CheckDone)), "{woken:?}");
    assert!(woken.contains(&(t.dee, WakeReason::CheckDone)), "{woken:?}");
    assert_eq!(
        hq.check_done(handed.id, &handed.token, true, "again", ""),
        Err(super::EngineError::Refused(Refusal::Finished)),
        "reported once"
    );
    // The check's report is a document the tester reads.
    let documents = hq.store().documents_of(task).unwrap();
    assert_eq!(documents.len(), 2);
    assert!(
        documents[1]
            .title
            .starts_with(&format!("check c{}: syntax ok", recorded.check))
    );
}

#[test]
fn a_branch_that_fails_its_checks_is_sent_back() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    use_tool(
        &hq,
        &rae,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done", "evidence": [[format!("task {}", task.0)]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::InReview
    );
    let handed = hq.next_check().unwrap().unwrap();
    let before = hq.store().wakes().unwrap().len();
    hq.check_done(
        handed.id,
        &handed.token,
        false,
        "syntax ok, ansible-lint 2 findings, yamllint failed",
        "yaml: trailing spaces",
    )
    .unwrap();
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Doing,
        "sent back by the check, not by a person"
    );
    let posts = thread(&hq, task);
    assert!(
        posts
            .iter()
            .any(|p| p.starts_with("endora: Sent back: check c") && p.contains("yamllint failed")),
        "{posts:?}"
    );
    let woken = wakes_after(&hq, before);
    assert!(woken.contains(&(t.rae, WakeReason::SentBack)), "{woken:?}");
    assert!(
        !woken.iter().any(|(m, _)| *m == t.dee),
        "the tester is not asked to read what failed"
    );
    assert!(
        hq.store().desk().unwrap().iter().all(|d| d.kind != "push"),
        "nothing is offered"
    );
}

#[test]
fn a_lost_check_is_given_up_after_half_an_hour() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (_, _, recorded) = submitted(&hq, &t, true);
    let handed = hq.next_check().unwrap().unwrap();
    clock.0.fetch_add(
        crate::domain::wing::CHECK_LOST_AFTER_MS + 1,
        Ordering::SeqCst,
    );
    assert!(hq.next_check().unwrap().is_none());
    let check = hq
        .store()
        .checks()
        .unwrap()
        .into_iter()
        .find(|c| c.id == recorded.check)
        .unwrap();
    assert_eq!((check.status.as_str(), check.ok), ("done", Some(false)));
    assert!(
        check.summary.contains("never reported"),
        "{}",
        check.summary
    );
    assert!(
        hq.check_done(handed.id, &handed.token, true, "late", "")
            .is_err()
    );
}

#[test]
fn the_wings_branch_reaches_the_owner_only_on_approval() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    let handed = hq.next_check().unwrap().unwrap();
    hq.check_done(
        handed.id,
        &handed.token,
        true,
        "syntax ok, ansible-lint ok, yamllint ok",
        "",
    )
    .unwrap();
    assert!(hq.push_asked(crate::domain::DeskId(999)).unwrap().is_none());
    assert!(
        hq.store().desk().unwrap().iter().all(|d| d.kind != "push"),
        "nothing is offered before the work is done"
    );
    // Rae submits, Dee passes it, Kit accepts: done, and only then the desk.
    let receipt = hq.store().call_receipts_of(rae.run).unwrap().pop().unwrap();
    use_tool(
        &hq,
        &rae,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done", "evidence": [[format!("r{}", receipt.id)]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    hq.runner().finish(rae.run, true);
    let dee = turn(&hq, t.dee);
    let own = hq
        .record_receipt(
            &dee.token,
            &super::engine::Called {
                tool: "read_document".to_owned(),
                args: String::new(),
                excerpt: "read".to_owned(),
                ok: true,
                act: false,
                confidential: false,
            },
        )
        .unwrap();
    use_tool(
        &hq,
        &dee,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "read the diff and the check", "evidence": [[format!("r{}", receipt.id), format!("r{own}")]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    hq.runner().finish(dee.run, true);
    let kit = turn(&hq, t.kit);
    use_tool(
        &hq,
        &kit,
        "accept",
        &json!({ "task": task.0, "verdict": "accept", "notes": "good", "evidence": [[format!("r{}", receipt.id)]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    let items: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "push")
        .collect();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert!(
        item.text.starts_with(
            "Push hq/task-3-tidy-the-deluge-role to your fork (2 files, checks passed)."
        ),
        "{}",
        item.text
    );
    assert_eq!(item.task, Some(task));
    assert_eq!(hq.store().branches().unwrap()[0].status, "offered");
    // Declined: it stays on the node, and Rae hears.
    hq.decide(item.id, "decline").unwrap();
    assert_eq!(hq.store().branches().unwrap()[0].status, "declined");
    assert!(
        thread(&hq, task)
            .pop()
            .unwrap()
            .contains("declined pushing")
    );
    assert!(hq.push_asked(item.id).unwrap().is_none());
    // Approved (a second submission, the same way): the node is asked, and tells what came of it.
    let (task2, rae2, _) = submitted(&hq, &t, true);
    hq.store()
        .set_task_status(task2, TaskStatus::InReview)
        .unwrap();
    let current = hq.store().task(task2).unwrap().unwrap();
    hq.store().set_task_status(task2, TaskStatus::Done).unwrap();
    hq.offer_push(current.id).unwrap();
    let item = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "push" && d.status == "waiting")
        .next_back()
        .unwrap();
    hq.decide(item.id, "approve").unwrap();
    let branch = hq
        .push_asked(item.id)
        .unwrap()
        .expect("the node is asked to push");
    assert_eq!((branch.task, branch.status.as_str()), (task2, "approved"));
    let before = hq.store().wakes().unwrap().len();
    hq.push_outcome(
        item.id,
        true,
        "Pushed hq/task-3-tidy-the-deluge-role (a1b2c3d4e5f6) to the fork: merge it on GitHub.",
    )
    .unwrap();
    assert_eq!(
        hq.store().branches().unwrap().pop().unwrap().status,
        "pushed"
    );
    let receipt = hq
        .store()
        .call_receipts_of(rae2.run)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        (receipt.tool.as_str(), receipt.act, receipt.ok),
        ("push", true, true)
    );
    assert!(
        thread(&hq, task2)
            .pop()
            .unwrap()
            .contains("Pushed hq/task-3")
    );
    assert!(wakes_after(&hq, before).contains(&(t.rae, WakeReason::ProposalDecided)));
    assert_eq!(
        hq.push_outcome(item.id, true, "again"),
        Err(super::EngineError::Refused(Refusal::Finished)),
        "told once"
    );
    assert_eq!(
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .find(|d| d.id == item.id)
            .unwrap()
            .status,
        "pushed"
    );
}

#[test]
fn without_a_deploy_key_the_desk_offers_the_bundle_instead() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, _, _) = submitted(&hq, &t, false);
    let current = hq.store().task(task).unwrap().unwrap();
    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    hq.offer_push(current.id).unwrap();
    let item = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.kind == "push")
        .unwrap();
    assert!(
        item.text
            .contains("pushing is not set up on the node (ENDORA_GIT_PUSH_KEY_FILE"),
        "{}",
        item.text
    );
    assert!(
        item.text.contains(&format!(
            "/v1/hq/bundles/{}/task-3-tidy-the-deluge-role",
            task.0
        )),
        "{}",
        item.text
    );
    assert!(item.text.contains("checks not finished"), "{}", item.text);
    assert!(
        hq.decide(item.id, "approve").is_err(),
        "nothing to approve without a key"
    );
    hq.decide(item.id, "seen").unwrap();
    assert!(hq.push_asked(item.id).unwrap().is_none());
}

#[test]
fn the_owner_hires_and_a_second_ceo_is_refused() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let sam = hq
        .hire_by_owner("Sam", Role::Developer, "claude-haiku-5-5", Some(t.kit))
        .unwrap();
    let members = hq.store().members().unwrap();
    let found = members.iter().find(|m| m.id == sam).unwrap();
    assert_eq!(
        (
            found.role,
            found.brain,
            found.reports_to,
            found.share_percent
        ),
        (Role::Developer, Brain::Haiku, Some(t.kit), 40)
    );
    let hired = hq
        .events_after(0)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "member.hired")
        .unwrap();
    assert_eq!(
        (hired.data["name"].as_str(), hired.data["by"].as_str()),
        (Some("Sam"), Some("owner"))
    );
    // By default a developer reports to the earliest lead; a researcher may not think with Sonnet.
    let lee = hq
        .hire_by_owner("Lee", Role::Researcher, "local", None)
        .unwrap();
    assert_eq!(members_of(&hq, lee).reports_to, Some(t.ada));
    assert_eq!(
        hq.hire_by_owner("Second", Role::Ceo, "claude-opus-5-5", None),
        Err(super::EngineError::Refused(Refusal::OnlyOneCeo))
    );
    assert!(matches!(
        hq.hire_by_owner("Pat", Role::Researcher, "claude-sonnet-5-5", None),
        Err(super::EngineError::Refused(Refusal::NotABrain { .. }))
    ));
    assert_eq!(
        hq.hire_by_owner("sam", Role::Developer, "local", None),
        Err(super::EngineError::Refused(Refusal::NotAName)),
        "names stay unique"
    );
    assert!(
        hq.hire_by_owner("Nobody", Role::Developer, "local", Some(MemberId(999)))
            .is_err()
    );
    let _ = t.ceo;
}

fn members_of(hq: &Hq<'_>, id: MemberId) -> crate::domain::Member {
    hq.store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

#[test]
fn the_wing_may_not_grow_past_its_seats() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let kit = turn(&hq, t.kit);
    let ask = |args: Value| use_tool(&hq, &kit, "request_hire", &args);
    // Evidence, a role below, a brain the role may use, and the wing's brains.
    let refused = ask(
        json!({ "role": "developer", "brain": "claude-haiku-5-5", "why": "two branches wait" }),
    )
    .unwrap_err();
    assert!(refused.contains("evidence"), "{refused}");
    let refused = ask(json!({ "role": "developer", "brain": "claude-haiku-5-5", "why": "x", "evidence": ["task 999"] })).unwrap_err();
    assert!(refused.contains("not in HQ's record"), "{refused}");
    let refused = ask(json!({ "role": "developer", "brain": "claude-sonnet-5-5", "why": "x", "evidence": [format!("task {}", t.plan.0)] })).unwrap_err();
    assert!(
        refused.contains("Claude Haiku or the house's model"),
        "{refused}"
    );
    let refused = ask(json!({ "role": "researcher", "brain": "claude-haiku-5-5", "why": "x", "evidence": [format!("task {}", t.plan.0)] })).unwrap_err();
    assert!(refused.contains("no seat for a researcher"), "{refused}");
    assert!(
        ask(json!({ "role": "ceo", "brain": "local", "why": "x", "evidence": ["task 1"] }))
            .is_err()
    );
    // Within the seats: the request waits on the desk, one at a time.
    let said = ask(json!({ "name": "Sam", "role": "developer", "brain": "claude-haiku-5-5", "why": "two branches wait with nobody free", "evidence": [format!("task {}", t.plan.0)] })).unwrap();
    assert!(said.contains("owner's desk"), "{said}");
    let item = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.kind == "hire")
        .unwrap();
    assert!(
        item.text
            .starts_with("Hire Sam as developer on claude-haiku-5-5, reporting to member"),
        "{}",
        item.text
    );
    assert!(
        item.text.contains("Kit asks: two branches wait"),
        "{}",
        item.text
    );
    let refused = ask(json!({ "role": "developer", "brain": "local", "why": "x", "evidence": [format!("task {}", t.plan.0)] })).unwrap_err();
    assert!(refused.contains("still waiting"), "{refused}");
    // Approved: Sam joins, reporting to Kit, and Kit hears in the thread.
    hq.decide(item.id, "approve").unwrap();
    let sam = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "Sam")
        .expect("hired");
    assert_eq!(
        (sam.role, sam.brain, sam.reports_to),
        (Role::Developer, Brain::Haiku, Some(t.kit))
    );
    assert!(
        thread(&hq, t.plan)
            .pop()
            .unwrap()
            .contains("approved the hire: Sam")
    );
    assert_eq!(hq.wing_members().unwrap().len(), 4, "Kit, Rae, Zed, Sam");
    // Up to five developers; the sixth is refused with the count.
    for name in ["Lee", "Max"] {
        hq.hire_by_owner(name, Role::Developer, "local", Some(t.kit))
            .unwrap();
    }
    assert_eq!(hq.wing_members().unwrap().len(), 6);
    let refused = ask(json!({ "role": "developer", "brain": "local", "why": "x", "evidence": [format!("task {}", t.plan.0)] })).unwrap_err();
    assert!(
        refused.contains(&format!(
            "{MOST_DEVELOPERS} of its {MOST_DEVELOPERS} developers"
        )),
        "{refused}"
    );
    let refused = ask(json!({ "role": "lead", "brain": "claude-sonnet-5-5", "why": "x", "evidence": [format!("task {}", t.plan.0)] })).unwrap_err();
    assert!(
        refused.contains("below your own"),
        "a lead asks for a role below: {refused}"
    );
    // The owner's own hiring is not limited; the screen says the agreed seats.
    hq.hire_by_owner("Ash", Role::Developer, "claude-haiku-5-5", Some(t.kit))
        .unwrap();
    let snap = hq.snapshot().unwrap();
    assert_eq!(snap["wing"]["developers"], 6);
    assert_eq!(snap["wing"]["most_developers"], MOST_DEVELOPERS);
    assert_eq!(snap["wing"]["leads"], json!([t.kit.0]));
    // Outside the wing, Ada may ask without the wing's count; only a lead asks at all.
    hq.runner().finish(kit.run, true);
    hq.tick().unwrap();
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.lab,
            project: None,
            parent: None,
            title: "Look over the lab".to_owned(),
            description: String::new(),
            assignee: Some(t.ada),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    let ada = turn(&hq, t.ada);
    let said = use_tool(&hq, &ada, "request_hire", &json!({ "role": "developer", "brain": "claude-sonnet-5-5", "why": "the lab needs a second pair of hands", "evidence": [format!("task {}", t.plan.0)] })).unwrap();
    assert!(said.contains("owner's desk"), "{said}");
    hq.wake(t.rae, Some(t.plan), WakeReason::Mentioned).unwrap();
    let rae = turn(&hq, t.rae);
    let refused = use_tool(
        &hq,
        &rae,
        "request_hire",
        &json!({ "role": "developer", "brain": "local", "why": "x", "evidence": ["task 1"] }),
    )
    .unwrap_err();
    assert!(refused.contains("only a lead"), "{refused}");
    let _ = t.dee;
}

#[test]
fn the_wing_is_seeded_once_and_an_old_team_keeps_what_it_had() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ceo = hq.seat_ceo("Mara").unwrap().unwrap();
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    let seeded = hq.seed_wing().unwrap().expect("seeded");
    let members = hq.store().members().unwrap();
    let kit = members.iter().find(|m| m.name == "Kit").unwrap();
    assert_eq!(
        (kit.role, kit.brain, kit.reports_to),
        (Role::Lead, Brain::Sonnet, Some(ceo))
    );
    for name in ["Rae", "Zed"] {
        let dev = members.iter().find(|m| m.name == name).unwrap();
        assert_eq!(
            (dev.role, dev.brain, dev.reports_to),
            (Role::Developer, Brain::Haiku, Some(kit.id))
        );
        assert!(seeded.developers.contains(&dev.id));
    }
    let goal = hq.store().goal(seeded.goal).unwrap().unwrap();
    assert_eq!(
        (goal.title.as_str(), goal.status),
        (WING_GOAL, crate::domain::GoalStatus::Active)
    );
    let note = hq.store().goal_note(seeded.goal).unwrap();
    assert!(
        note.contains("repo_checkout") && note.contains("never opens a pull request"),
        "{note}"
    );
    let project = hq
        .store()
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.id == seeded.project)
        .unwrap();
    assert_eq!(
        (project.title.as_str(), project.lead, project.goal),
        (WING_PROJECT, kit.id, seeded.goal)
    );
    let task = hq.store().task(seeded.task).unwrap().unwrap();
    assert_eq!(
        (task.assignee, task.project),
        (Some(kit.id), Some(seeded.project))
    );
    assert_eq!(task.criteria.len(), 2);
    assert!(task.criteria[0].text.contains("by receipt"));
    assert_eq!(hq.wing_members().unwrap().len(), 3);
    // Once: nothing is made twice, and a renamed or paused member stays as the owner left it.
    hq.rename(kit.id, "Kai").unwrap();
    assert_eq!(hq.seed_wing().unwrap(), None);
    assert_eq!(hq.store().members().unwrap().len(), 6);
    assert_eq!(hq.store().projects().unwrap().len(), 1);
    assert_eq!(hq.store().tasks().unwrap().len(), 1);
    // A team that already has the goal and a Kit (an older HQ) is completed, not doubled.
    let other = engine(&clock);
    other.seat_ceo("Mara").unwrap();
    let kit = other.hire("Kit", Role::Lead, 25, 2, 5).unwrap();
    let goal = other.set_goal(WING_GOAL, "mine", Author::Owner).unwrap();
    let seeded = other.seed_wing().unwrap().unwrap();
    assert_eq!((seeded.lead, seeded.goal), (kit, goal));
    assert_eq!(other.store().goals().unwrap().len(), 1);
    assert_eq!(
        other.store().members().unwrap().len(),
        4,
        "Mara, Kit, Rae, Zed"
    );
}
