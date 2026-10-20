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
    // Kit's first turn, on the plan, is over: the Mac takes one turn at a time (ADR 0131), and
    // these tests are about the branches of the others.
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
    }
    hq.tick().unwrap();
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

/// Every turn that has started is over, and what its end woke has started and is over too. The
/// Mac takes one turn at a time (ADR 0131), so a test about the next branch begins clear.
fn all_turns_over(hq: &Hq<'_>) {
    for _ in 0..3 {
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
        hq.tell_mac(true, "ready");
        hq.tick().unwrap();
    }
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
    }
}

/// As [`all_turns_over`], for a test that has woken several members: the Mac takes one turn at
/// a time, so each round ends what started and starts the next, until none waits.
fn quiet(hq: &Hq<'_>) {
    for _ in 0..12 {
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
        hq.tell_mac(true, "ready");
        hq.tick().unwrap();
    }
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
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

/// The newest turn of `member`'s, after a tick or two to start it. The wing's members build
/// with the Mac (ADR 0131), so the node says it is free.
fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn")
}

/// A new turn of `member`'s on `task`: woken (a mention), then started. Kit's first turn, on
/// the plan, was over before a test began (see `team`), so a test that wants a turn it can
/// still use tools in wakes him again.
fn woken_turn(hq: &Hq<'_>, member: MemberId, task: TaskId) -> RunContext {
    hq.wake(member, Some(task), WakeReason::Mentioned).unwrap();
    turn(hq, member)
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

/// The tester (or anyone) checks a kept branch out, as the node does for the turn (ADR 0115,
/// amended): HQ's rule says whether the branch is the turn's to have, and the node writes the
/// receipt of the checkout, which names the branch and its head. Returns the receipt, the
/// turn's own reading of the branch. Panics when HQ refuses.
fn checks_out(hq: &Hq<'_>, context: &RunContext, name: &str) -> i64 {
    let kept = hq
        .checkout_branch(&context.token, name)
        .unwrap_or_else(|e| panic!("{e}"));
    hq.record_receipt(
        &context.token,
        &super::engine::Called {
            tool: "repo_checkout".to_owned(),
            args: format!("branch: {name}"),
            excerpt: format!(
                "checked out the team's kept branch {name} at {} (task #{}'s, on top of develop)",
                &kept.head[..12],
                kept.task.0
            ),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
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

/// Live (2026-10-18, tasks 670 and 674): Rae and Kit asked the node for the branch it was
/// keeping and were refused, because the node could not tell whose it was, and recreated the
/// work. HQ's record can: a kept branch is handed to the work it is for (its task, the tasks
/// under it, the tasks above it) and to the member who submitted it, at the head HQ recorded;
/// to no one else, whatever the name says.
#[test]
fn a_kept_branch_may_be_checked_out_by_its_own_work_and_its_own_member_and_no_one_else() {
    const NAME: &str = "hq/task-3-tidy-the-deluge-role";
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    // The member who submitted it, in the turn that did: the head and base HQ recorded.
    let kept = hq.checkout_branch(&rae.token, NAME).unwrap();
    assert_eq!(
        (kept.head.as_str(), kept.base.as_str(), kept.task),
        ("a1b2c3d4e5f60718293a4b5c", "0123456789abcdef01234567", task)
    );
    // Only a running turn's token asks, and only for a branch HQ recorded.
    assert!(hq.checkout_branch("forged", NAME).is_err());
    assert_eq!(
        hq.checkout_branch(&rae.token, "hq/task-9-none"),
        Err(super::EngineError::Unknown("branch of that name")),
        "the team never submitted it"
    );
    // Others, each in a turn of their own on the task they are woken on.
    let zed = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "Zed")
        .unwrap()
        .id;
    let sibling = |title: &str, to: MemberId| {
        hq.add_task_with(
            Author::Member(t.kit),
            NewTask {
                goal: t.goal,
                project: None,
                parent: Some(t.plan),
                title: title.to_owned(),
                description: String::new(),
                assignee: Some(to),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
            &[Criterion::new("Done", false)],
        )
        .unwrap()
    };
    let under = hq
        .add_task_with(
            Author::Member(t.kit),
            NewTask {
                goal: t.goal,
                project: None,
                parent: Some(task),
                title: "Check the role's defaults".to_owned(),
                description: String::new(),
                assignee: Some(zed),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
            &[Criterion::new("Checked", false)],
        )
        .unwrap();
    let elsewhere = sibling("Sort the cables", zed);
    let raes_other = sibling("Write the role's readme", t.rae);
    let ask = |member: MemberId, on: TaskId| {
        all_turns_over(&hq);
        let context = woken_turn(&hq, member, on);
        hq.checkout_branch(&context.token, NAME)
    };
    // The tester, on the task: the same work.
    assert_eq!(ask(t.dee, task).map(|k| k.task), Ok(task));
    // The lead, on the task above it: the branch is for work under hers.
    assert_eq!(ask(t.kit, t.plan).map(|k| k.task), Ok(task));
    // Zed, on a task under it: the branch is for the work above his.
    assert_eq!(ask(zed, under).map(|k| k.task), Ok(task));
    // Rae, on another task: the branch is hers, whatever task she is on.
    assert_eq!(ask(t.rae, raes_other).map(|k| k.task), Ok(task));
    // Zed, on a task of another piece of work: refused, and told whose it is.
    let refused = ask(zed, elsewhere).unwrap_err();
    assert_eq!(
        refused,
        super::EngineError::Refused(Refusal::NotYourBranch {
            branch: NAME.to_owned(),
            task
        })
    );
    let said = refused.to_string();
    assert!(
        said.contains(NAME) && said.contains(&format!("task #{}'s", task.0)),
        "{said}"
    );
}

// Of the eight times a passing check woke the tester (18-19 Oct), the work was never in
// review: five of those turns asked the lead what to do, two did nothing, one checked. Review
// wakes the tester when the work is submitted; a pass wakes her only for work already in
// review, which she may have been waiting on, and only the tester review wakes.
#[test]
fn a_check_that_passes_on_work_in_review_wakes_its_tester_and_no_other() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let other = hq.hire("Ivy", Role::Reviewer, 20, 2, 1).unwrap();
    let (task, _rae, _) = submitted(&hq, &t, true);
    hq.store()
        .set_task_status(task, TaskStatus::InReview)
        .unwrap();
    let handed = hq.next_check().unwrap().unwrap();
    let before = hq.store().wakes().unwrap().len();
    hq.check_done(handed.id, &handed.token, true, "syntax ok", "")
        .unwrap();
    let woken = wakes_after(&hq, before);
    assert!(woken.contains(&(t.dee, WakeReason::CheckDone)), "{woken:?}");
    assert!(
        !woken.iter().any(|(m, _)| *m == other),
        "one tester, as review wakes: {woken:?}"
    );
}

#[test]
fn a_check_that_passes_is_a_receipt_that_wakes_the_submitter() {
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
    assert!(
        !woken.iter().any(|(m, _)| *m == t.dee),
        "the work is not in review yet: nothing for the tester: {woken:?}"
    );
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

/// Live (tasks 674 and 679): a check was queued only by submitting a branch from a sandbox that
/// held it, which a later turn never did. A check that failed for the box's own reasons (tools
/// missing, a collection absent) cost a recreated branch each time: task 674 existed only to
/// check the same commit again, and 679's probe branch (no file in it) only to get a check run.
/// A lead or a developer on the work now asks for the kept branch to be checked again.
/// Task 687: the Ops lead built and submitted a branch herself; she could neither accept her own
/// work nor hand it on, and the CEO was asked twice. Changing the lab is the wing's (ADR 0120):
/// outside it a submission and a recheck are refused, naming the wing's lead and the way to ask
/// for the change, and nothing is kept. The wing's own developer still submits.
#[test]
fn branch_work_outside_the_wing_is_refused_and_names_the_wings_lead() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let task = hq
        .add_task_with(
            Author::Owner,
            NewTask {
                goal: t.lab,
                project: None,
                parent: None,
                title: "Add the drift check".to_owned(),
                description: String::new(),
                assignee: Some(t.ada),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
            &[Criterion::new("The check is no worse than develop", false)],
        )
        .unwrap();
    let ada = turn(&hq, t.ada);
    let refused = hq
        .record_branch(
            &ada.token,
            &Submission {
                branch: format!("hq/task-{}-add-the-drift-check", task.0),
                ..submission(true)
            },
        )
        .unwrap_err();
    assert_eq!(
        refused,
        super::EngineError::Refused(Refusal::BranchesAreTheWings {
            lead: Some("Kit".to_owned())
        })
    );
    let said = refused.to_string();
    assert!(
        said.contains("engineering wing")
            && said.contains("Kit")
            && said.contains("propose_project"),
        "{said}"
    );
    assert!(hq.store().branches().unwrap().is_empty(), "nothing is kept");
    assert!(hq.next_check().unwrap().is_none(), "nothing is checked");
    let again = use_tool(
        &hq,
        &ada,
        "recheck",
        &json!({ "task": task.0, "why": "the check image has its tools now" }),
    )
    .unwrap_err();
    assert!(again.contains("engineering wing"), "{again}");
    all_turns_over(&hq);
    // The wing's developer, on the wing's work, is not touched by it.
    let (_, _, recorded) = submitted(&hq, &t, true);
    assert!(recorded.check > 0);
}

#[test]
fn a_kept_branch_is_checked_again_without_a_new_submission() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    // The first check failed for the box's own reasons.
    let first = hq.next_check().unwrap().unwrap();
    hq.check_done(
        first.id,
        &first.token,
        false,
        "syntax could not run (develop could not be checked: syntax exited 127 (not found) on develop)",
        "",
    )
    .unwrap();
    assert_eq!(hq.store().gate(task).unwrap(), Some(false));
    let branches = hq.store().branches().unwrap().len();
    // Rae, in her turn, asks for it again: no sandbox, no new submission.
    let said = use_tool(
        &hq,
        &rae,
        "recheck",
        &json!({ "task": task.0, "why": "the check image has its tools now" }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(said.contains("queued"), "{said}");
    let checks = hq.store().checks().unwrap();
    assert_eq!(checks.len(), 2);
    let again = checks.iter().find(|c| c.id != first.id).unwrap().clone();
    assert_eq!(
        (again.branch, again.status.as_str(), again.run),
        (first.branch, "waiting", rae.run)
    );
    assert_eq!(
        hq.store().branches().unwrap().len(),
        branches,
        "no new branch row: the kept one is checked again"
    );
    let last = thread(&hq, task).pop().unwrap();
    assert!(
        last.contains(&format!("Check c{} is queued again", again.id))
            && last.contains("hq/task-3-tidy-the-deluge-role")
            && last.contains("the check image has its tools now"),
        "{last}"
    );
    // One at a time: refused while the branch's last check waits, and while it runs.
    let ask = |context: &RunContext| {
        use_tool(
            &hq,
            context,
            "recheck",
            &json!({ "task": task.0, "why": "again" }),
        )
    };
    let waiting = ask(&rae).unwrap_err();
    assert!(
        waiting.contains(&format!("c{}", again.id)) && waiting.contains("waiting or running"),
        "{waiting}"
    );
    let handed = hq.next_check().unwrap().expect("the runner is handed it");
    assert_eq!(handed.id, again.id);
    assert!(ask(&rae).is_err(), "and not while it runs");
    // Its result is the task's gate, as the first's was, and a receipt of Rae's turn.
    let before = hq.store().wakes().unwrap().len();
    hq.check_done(
        handed.id,
        &handed.token,
        true,
        "syntax ok, ansible-lint 25 findings (develop 25), yamllint ok: no worse than develop",
        "",
    )
    .unwrap();
    assert_eq!(hq.store().gate(task).unwrap(), Some(true));
    let receipt = hq.store().call_receipts_of(rae.run).unwrap().pop().unwrap();
    assert_eq!((receipt.tool.as_str(), receipt.ok), ("check", true));
    assert!(
        receipt
            .args
            .starts_with(&format!("c{} hq/task-3", again.id)),
        "{}",
        receipt.args
    );
    assert!(
        !wakes_after(&hq, before).iter().any(|(m, _)| *m == t.dee),
        "not in review: the tester is woken when it is"
    );
    // The branch is not offered anew, and a checked-again branch is still one branch.
    assert_eq!(hq.store().branches().unwrap().len(), branches);
    // A lead asks too, for work under hers, and is told the result with the branch's member.
    quiet(&hq);
    let kit = woken_turn(&hq, t.kit, t.plan);
    use_tool(
        &hq,
        &kit,
        "recheck",
        &json!({ "task": task.0, "why": "to see it pass again" }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let third = hq.next_check().unwrap().expect("a third check waits");
    let before = hq.store().wakes().unwrap().len();
    hq.check_done(
        third.id,
        &third.token,
        true,
        "syntax ok, ansible-lint ok, yamllint ok",
        "",
    )
    .unwrap();
    let woken = wakes_after(&hq, before);
    assert!(woken.contains(&(t.kit, WakeReason::CheckDone)), "{woken:?}");
    assert!(woken.contains(&(t.rae, WakeReason::CheckDone)), "{woken:?}");
    // The tester judges and does not run the lab's checks; work with no branch has nothing to check.
    quiet(&hq);
    let dee = woken_turn(&hq, t.dee, task);
    let refused = ask(&dee).unwrap_err();
    assert!(refused.contains("lead or developer"), "{refused}");
    quiet(&hq);
    let other = raes_other_task(&hq, &t);
    let rae_again = woken_turn(&hq, t.rae, other);
    let nothing = use_tool(
        &hq,
        &rae_again,
        "recheck",
        &json!({ "task": other.0, "why": "x" }),
    )
    .unwrap_err();
    assert!(nothing.contains("submitted branch"), "{nothing}");
}

/// Another piece of Rae's work, with nothing submitted on it.
fn raes_other_task(hq: &Hq<'_>, t: &Team) -> TaskId {
    hq.add_task_with(
        Author::Member(t.kit),
        NewTask {
            goal: t.goal,
            project: None,
            parent: Some(t.plan),
            title: "Write the role's readme".to_owned(),
            description: String::new(),
            assignee: Some(t.rae),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
        &[Criterion::new("The readme is written", false)],
    )
    .unwrap()
}

/// The push the owner is offered is of the branch's newest head, and the tester and the lead
/// judged one head. A head submitted after the pass (a probe, a second try) used to change what
/// the card offered, with the gate left as the first head's check set it.
///
/// Given a branch whose check passed, that the tester passed, and the lead is to accept,
/// when a new head is submitted,
/// then the task is back in doing with no pass standing and its gate waiting for the new head's
/// own check, the lead has nothing to accept, and no push is offered until it is reviewed again.
#[test]
fn a_new_head_after_the_pass_needs_a_new_review() {
    const NAME: &str = "hq/task-3-tidy-the-deluge-role";
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    // The first head: its check passes, Rae submits the task, Dee passes it on her checkout.
    let c1 = hq.next_check().unwrap().unwrap();
    hq.check_done(
        c1.id,
        &c1.token,
        true,
        "syntax ok, ansible-lint ok, yamllint ok",
        "",
    )
    .unwrap();
    let check1 = hq.store().call_receipts_of(rae.run).unwrap().pop().unwrap();
    use_tool(
        &hq,
        &rae,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done",
                 "evidence": [[format!("r{}", check1.id)]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    hq.runner().finish(rae.run, true);
    let dee = turn(&hq, t.dee);
    let own = checks_out(&hq, &dee, NAME);
    use_tool(
        &hq,
        &dee,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "it holds",
                 "evidence": [[format!("r{}", check1.id), format!("r{own}")]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        wakes_after(&hq, 0).contains(&(t.kit, WakeReason::AcceptanceAsked)),
        "the lead is asked to accept the first head"
    );
    hq.runner().finish(dee.run, true);
    // The lead's turn on it comes and ends without a decision (turns on one task take their
    // turn one at a time). Then the owner speaks to Rae, who submits a new head in her turn.
    let kit1 = turn(&hq, t.kit);
    hq.runner().finish(kit1.run, true);
    hq.tick().unwrap();
    hq.post(Author::Owner, task, "One more page, please.")
        .unwrap();
    let rae2 = turn(&hq, t.rae);
    let mut second = submission(true);
    second.head = "0f1e2d3c4b5a69788796a5b4".to_owned();
    hq.record_branch(&rae2.token, &second).unwrap();
    // (The node's receipt of that submission, as it writes one: a turn that submitted a branch
    // is not a turn that showed nothing.)
    hq.record_receipt(
        &rae2.token,
        &super::engine::Called {
            tool: "repo_submit".to_owned(),
            args: format!("branch: {NAME}"),
            excerpt: format!(
                "submitted branch {NAME} at 0f1e2d3c4b5a for task {}",
                task.0
            ),
            ok: true,
            act: true,
            confidential: false,
        },
    )
    .unwrap();
    // The review is void: back in doing, the gate waits for the new head's own check.
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Doing,
        "a new head needs a new review"
    );
    assert_eq!(
        hq.store().gate(task).unwrap(),
        Some(false),
        "the first head's check is not the new head's"
    );
    let said = thread(&hq, task);
    assert!(
        said.iter().any(|p| p.starts_with("endora: Sent back: ")
            && p.contains("submitted again at 0f1e2d3c4b5a")
            && p.contains("new head needs a new review")),
        "{said:?}"
    );
    // Rae's turn ends; HQ finds the task idle in doing and wakes her once more, which runs its
    // course at a turn's own pace, half an hour on. Then the lead, asked again, has nothing to
    // accept, and nothing is offered to the owner.
    hq.runner().finish(rae2.run, true);
    hq.tick().unwrap();
    clock.0.fetch_add(30 * 60 * 1000, Ordering::SeqCst);
    quiet(&hq);
    hq.wake(t.kit, Some(task), WakeReason::AcceptanceAsked)
        .unwrap();
    let kit = turn(&hq, t.kit);
    let refused = use_tool(
        &hq,
        &kit,
        "accept",
        &json!({ "task": task.0, "verdict": "accept", "notes": "good",
                 "evidence": [[format!("r{}", check1.id)]] }),
    )
    .unwrap_err();
    assert!(refused.contains("does not go from doing"), "{refused}");
    assert!(hq.store().desk().unwrap().iter().all(|d| d.kind != "push"));
    // The second round: the new head's check, the task submitted again, a new pass, the lead.
    hq.runner().finish(kit.run, true);
    hq.tick().unwrap();
    let c2 = hq.next_check().unwrap().unwrap();
    hq.check_done(
        c2.id,
        &c2.token,
        true,
        "syntax ok, ansible-lint ok, yamllint ok",
        "",
    )
    .unwrap();
    assert_eq!(hq.store().gate(task).unwrap(), Some(true));
    let check2 = hq
        .store()
        .call_receipts_of(rae2.run)
        .unwrap()
        .pop()
        .unwrap();
    let rae3 = turn(&hq, t.rae);
    use_tool(
        &hq,
        &rae3,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done again",
                 "evidence": [[format!("r{}", check2.id)]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    hq.runner().finish(rae3.run, true);
    hq.tick().unwrap();
    let dee2 = turn(&hq, t.dee);
    let own2 = checks_out(&hq, &dee2, NAME);
    use_tool(
        &hq,
        &dee2,
        "review",
        &json!({ "task": task.0, "verdict": "approve", "notes": "the new head holds",
                 "evidence": [[format!("r{}", check2.id), format!("r{own2}")]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    hq.runner().finish(dee2.run, true);
    let kit2 = turn(&hq, t.kit);
    use_tool(
        &hq,
        &kit2,
        "accept",
        &json!({ "task": task.0, "verdict": "accept", "notes": "good",
                 "evidence": [[format!("r{}", check2.id)]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Done
    );
    // The card offers the head that was reviewed this time.
    let pushes: Vec<_> = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == "push")
        .collect();
    assert_eq!(pushes.len(), 1);
    assert!(
        pushes[0].text.contains("0f1e2d3c4b5a"),
        "the second head: {}",
        pushes[0].text
    );
}

/// The gate is the task's and every check overwrote it: a check of an older head that reported
/// late set the gate for a newer one. It is the newest head's.
#[test]
fn a_late_check_of_an_older_head_leaves_the_gate_to_the_newest() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    let mut second = submission(true);
    second.head = "0f1e2d3c4b5a69788796a5b4".to_owned();
    hq.record_branch(&rae.token, &second).unwrap();
    let (one, two) = (
        hq.next_check().unwrap().unwrap(),
        hq.next_check().unwrap().unwrap(),
    );
    // The newer head's check fails; the older head's passes, late.
    hq.check_done(two.id, &two.token, false, "yamllint failed", "")
        .unwrap();
    let before = hq.store().wakes().unwrap().len();
    hq.check_done(
        one.id,
        &one.token,
        true,
        "syntax ok, ansible-lint ok, yamllint ok",
        "",
    )
    .unwrap();
    assert_eq!(
        hq.store().gate(task).unwrap(),
        Some(false),
        "the older head's pass is not the newest head's"
    );
    let last = thread(&hq, task).pop().unwrap();
    assert!(
        last.contains("older head") && last.contains("syntax ok, ansible-lint ok"),
        "it is said, with its result: {last}"
    );
    assert!(
        wakes_after(&hq, before).is_empty(),
        "and nobody is woken to read a head that has been replaced"
    );
    // The other way: the newest passes, an older one fails late, and the gate stays passed.
    let mut third = submission(true);
    third.head = "aa11bb22cc33dd44ee55ff66".to_owned();
    hq.record_branch(&rae.token, &third).unwrap();
    let mut fourth = submission(true);
    fourth.head = "bb22cc33dd44ee55ff66aa11".to_owned();
    hq.record_branch(&rae.token, &fourth).unwrap();
    let (old, new) = (
        hq.next_check().unwrap().unwrap(),
        hq.next_check().unwrap().unwrap(),
    );
    assert!(old.id < new.id);
    hq.check_done(
        new.id,
        &new.token,
        true,
        "syntax ok, ansible-lint ok, yamllint ok",
        "",
    )
    .unwrap();
    hq.check_done(old.id, &old.token, false, "yamllint failed", "")
        .unwrap();
    assert_eq!(hq.store().gate(task).unwrap(), Some(true));
}

/// Live (tasks 671, 672, 679 and 670): the CEO put a draft for the owner on a wing task, which
/// moved the work task itself to review, and the tester's `approve` of "the review" then went
/// through the draft's path: with the gate false it was refused for the gate, and with the
/// gate true the task closed as done and the push was offered, with no evidence judged and no
/// lead's acceptance. A draft is the owner's to read: the tester forwards it, or sends it back.
#[test]
fn a_testers_approve_on_a_draft_is_refused_and_no_push_is_offered() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    // The branch's check is green: the gate has passed.
    let handed = hq.next_check().unwrap().unwrap();
    hq.check_done(
        handed.id,
        &handed.token,
        true,
        "syntax ok, ansible-lint 25 findings (develop 25), yamllint ok: no worse than develop",
        "",
    )
    .unwrap();
    assert_eq!(hq.store().gate(task).unwrap(), Some(true));
    hq.runner().finish(rae.run, true);
    let dee = woken_turn(&hq, t.dee, task);
    // Mara, the CEO, has put a decision for the board on the task: what her `ask_owner` leaves
    // is a draft in the task's thread and the work task itself in review (the reviewer is
    // asked to check it before it reaches the board).
    hq.store()
        .set_task_status(task, TaskStatus::InReview)
        .unwrap();
    hq.store()
        .post(
            task,
            "Mara",
            &format!(
                "{}Decision: should the wing take on a second lead?\nRecommendation: not yet.\nWhy: one branch waits.",
                crate::domain::review::DRAFT
            ),
            clock.0.load(Ordering::SeqCst),
        )
        .unwrap();
    let approve = || {
        use_tool(
            &hq,
            &dee,
            "review",
            &json!({ "task": task.0, "verdict": "approve", "notes": "it holds",
                     "evidence": [[format!("task {}", task.0)]] }),
        )
    };
    let refused = approve().unwrap_err();
    assert!(
        refused.contains("draft for the owner") && refused.contains("ask_owner"),
        "{refused}"
    );
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::InReview,
        "the task stays in review: the draft closed nothing"
    );
    assert!(
        hq.store().desk().unwrap().iter().all(|d| d.kind != "push"),
        "and no push is offered"
    );
    assert_eq!(hq.store().branches().unwrap()[0].status, "submitted");
    // Forwarding is the way to the board, and sending it back stays what it was.
    use_tool(
        &hq,
        &dee,
        "review",
        &json!({ "task": task.0, "verdict": "send_back", "notes": "the question is not the board's" }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::Doing
    );
}

/// Live (task 670): the tester had no reading of a branch she could take, so her pass was
/// refused for want of one of her own, in three turns. Checking the kept branch out is one.
#[test]
fn a_testers_pass_needs_her_own_reading_and_her_checkout_of_the_branch_is_one() {
    const NAME: &str = "hq/task-3-tidy-the-deluge-role";
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, rae, _) = submitted(&hq, &t, true);
    let handed = hq.next_check().unwrap().unwrap();
    hq.check_done(
        handed.id,
        &handed.token,
        true,
        "syntax ok, ansible-lint 25 findings (develop 25), yamllint ok: no worse than develop",
        "",
    )
    .unwrap();
    let check = hq.store().call_receipts_of(rae.run).unwrap().pop().unwrap();
    assert_eq!(check.tool, "check");
    use_tool(
        &hq,
        &rae,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "done",
                 "evidence": [[format!("r{}", check.id)]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    hq.runner().finish(rae.run, true);
    let dee = turn(&hq, t.dee);
    let pass = |evidence: Vec<String>| {
        use_tool(
            &hq,
            &dee,
            "review",
            &json!({ "task": task.0, "verdict": "approve", "notes": "the page is what it says",
                     "evidence": [evidence] }),
        )
    };
    // The check's receipt is the submitter's turn's, not hers: no reading of her own.
    let refused = pass(vec![format!("r{}", check.id)]).unwrap_err();
    assert!(refused.contains("her own readings"), "{refused}");
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::InReview,
        "still in review, with no pass recorded"
    );
    // She checks the kept branch out; that checkout's receipt is hers, in this turn.
    let own = checks_out(&hq, &dee, NAME);
    let before = hq.store().wakes().unwrap().len();
    pass(vec![format!("r{}", check.id), format!("r{own}")]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        hq.store().task(task).unwrap().unwrap().status,
        TaskStatus::InReview,
        "her pass is not the end: the lead accepts"
    );
    assert!(
        wakes_after(&hq, before).contains(&(t.kit, WakeReason::AcceptanceAsked)),
        "{:?}",
        wakes_after(&hq, before)
    );
    // A branch HQ never recorded is not there to be read, so no reading of it can be cited.
    assert_eq!(
        hq.checkout_branch(&dee.token, "hq/task-9-none"),
        Err(super::EngineError::Unknown("branch of that name"))
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
    // Her own reading is the branch itself: she checks it out, as the node lets her.
    let own = checks_out(&hq, &dee, "hq/task-3-tidy-the-deluge-role");
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
    // Kit's turns (the acceptance, then the plan whose branches are all done) are over, so the
    // Mac, which takes one turn at a time, is free for the next branch.
    all_turns_over(&hq);
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

/// A change the owner approved and the node pushed is a landed change (ADR 0132), and the screen's
/// outcomes say so; the branch submitted and the check passed before it are counted too, and an
/// approval the node has not yet pushed is not a change that landed.
#[test]
fn what_the_node_pushed_on_the_owners_word_is_a_landed_change_in_the_outcomes() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let outcomes = |hq: &Hq<'_>| hq.snapshot().unwrap()["scorecard"]["outcomes"].clone();
    let nothing = outcomes(&hq);
    assert_eq!(nothing["week"]["landed"], 0);
    assert_eq!(
        nothing["week"]["per_landed"],
        Value::Null,
        "nothing landed, so no cost to give"
    );
    let (task, _rae, _) = submitted(&hq, &t, true);
    let handed = hq.next_check().unwrap().expect("a check waits");
    hq.check_done(handed.id, &handed.token, true, "syntax ok", "")
        .unwrap();
    let before = outcomes(&hq);
    assert_eq!(before["week"]["submitted"], 1);
    assert_eq!(before["week"]["checks_passed"], 1);
    assert_eq!(before["week"]["landed"], 0);

    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    hq.offer_push(task).unwrap();
    let item = hq
        .store()
        .desk()
        .unwrap()
        .into_iter()
        .find(|d| d.kind == "push")
        .unwrap();
    hq.decide(item.id, "approve").unwrap();
    hq.push_asked(item.id).unwrap().expect("the node is asked");
    assert_eq!(
        outcomes(&hq)["week"]["landed"],
        0,
        "approved, and not yet pushed"
    );
    hq.push_outcome(item.id, true, "Pushed hq/task-3 to the fork.")
        .unwrap();
    let after = outcomes(&hq);
    assert_eq!(after["day"]["landed"], 1);
    assert_eq!(after["week"]["landed"], 1);
    assert_eq!(
        after["week"]["per_landed"], 0,
        "the house's own models cost nothing: one landed for nothing"
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
    let kit = woken_turn(&hq, t.kit, t.plan);
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
    all_turns_over(&hq);
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
    // Ada's turn is over, and the Mac, which takes one turn at a time, is free for Rae's.
    all_turns_over(&hq);
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

#[test]
fn the_wing_is_told_the_forks_agents_md_is_its_rules_and_its_lead_that_the_draft_comes_first() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let kit = turn(&hq, t.kit);
    for said in [
        "The team's standards: read_doc docs/standards.md",
        "The fork's own AGENTS.md, when it exists, is the repo's rules; propose one if missing.",
        "docs/drafts/ansible-nas-AGENTS.md",
        "the first branch to propose",
    ] {
        assert!(kit.told.contains(said), "{said}\n---\n{}", kit.told);
    }
    raes_task(&hq, &t);
    let rae = turn(&hq, t.rae);
    assert!(
        rae.told
            .contains("The fork's own AGENTS.md, when it exists, is the repo's rules"),
        "{}",
        rae.told
    );
    assert!(
        !rae.told.contains("the first branch to propose"),
        "{}",
        rae.told
    );
    hq.add_task_with(
        Author::Owner,
        NewTask {
            goal: t.lab,
            project: None,
            parent: None,
            title: "Check the home lab".to_owned(),
            description: String::new(),
            assignee: Some(t.ada),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
        &[Criterion::new("The lab's state is in the thread", false)],
    )
    .unwrap();
    let ada = turn(&hq, t.ada);
    assert!(
        ada.told
            .contains("The team's standards: read_doc docs/standards.md"),
        "{}",
        ada.told
    );
    assert!(
        !ada.told.contains("AGENTS.md"),
        "not the wing's: {}",
        ada.told
    );
}

// ---- the check after a deploy (ADR 0115, amended) -------------------------------------------

/// The house's routine that takes the home lab's alarms, as the owner has it: in "Home lab
/// upkeep", which Ada leads (ADR 0107, 0113, 0120).
fn lab_routine(hq: &Hq<'_>, t: &Team) -> (crate::domain::RoutineId, crate::domain::ProjectId) {
    let upkeep = hq
        .add_project(t.lab, super::engine::LAB_UPKEEP, "", t.ada, None)
        .unwrap();
    let routine = hq
        .add_routine(
            Author::Owner,
            super::ports::NewRoutine {
                title: "Check the home lab".to_owned(),
                description: String::new(),
                goal: t.lab,
                project: Some(upkeep),
                assignee: t.ada,
                every_ms: 7 * 24 * 3_600_000,
                on_lab_trouble: true,
            },
        )
        .unwrap();
    (routine, upkeep)
}

/// The newest item of `kind` waiting on the owner's desk.
fn waiting(hq: &Hq<'_>, kind: &str) -> super::ports::DeskItem {
    hq.store()
        .desk()
        .unwrap()
        .into_iter()
        .filter(|d| d.kind == kind && d.status == "waiting")
        .next_back()
        .unwrap_or_else(|| panic!("no {kind} waits on the desk"))
}

/// A wing branch, accepted and pushed on the owner's word: returns its task and the card that
/// asks the owner to say when it is deployed.
fn pushed(hq: &Hq<'_>, t: &Team) -> (TaskId, super::ports::DeskItem) {
    let (task, _, _) = submitted(hq, t, true);
    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    hq.offer_push(task).unwrap();
    let push = waiting(hq, "push");
    hq.decide(push.id, "approve").unwrap();
    hq.push_asked(push.id).unwrap().expect("the node is asked");
    hq.push_outcome(
        push.id,
        true,
        "Pushed hq/task-3-tidy-the-deluge-role (a1b2c3d4e5f6) to the fork: merge it on GitHub.",
    )
    .unwrap();
    all_turns_over(hq);
    (task, waiting(hq, "deploy"))
}

/// The deploy check's task, the only one titled for the branch.
fn deploy_check(hq: &Hq<'_>) -> crate::domain::Task {
    let checks: Vec<_> = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .filter(|k| k.title.starts_with("Check the home lab after deploying "))
        .collect();
    assert_eq!(checks.len(), 1, "one check: {checks:?}");
    checks[0].clone()
}

/// The member's started turn on `task`, the task claimed as the worker claims it before the
/// model's first round.
fn turn_on(hq: &Hq<'_>, member: MemberId, task: TaskId) -> RunContext {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    hq.tick().unwrap();
    let context = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member && c.task == Some(task))
        .expect("a turn on the check");
    use_tool(hq, &context, "claim", &json!({ "task": task.0 })).unwrap_or_else(|e| panic!("{e}"));
    context
}

/// A reading the node took for the turn, as its receipt: `homelab_status`, read-only.
fn reads_the_lab(hq: &Hq<'_>, context: &RunContext) -> i64 {
    hq.record_receipt(
        &context.token,
        &super::engine::Called {
            tool: "homelab_status".to_owned(),
            args: String::new(),
            excerpt: "deluge running (healthy), sonarr running (healthy)".to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
}

/// The turn ends, as the runner reports it, and HQ sees it on its next tick.
fn ends(hq: &Hq<'_>, context: &RunContext) {
    hq.runner().finish(context.run, true);
    hq.tick().unwrap();
}

/// Behaviour 1: The owner's Deployed starts the check through the routine that takes the lab's alarms:
/// its goal, its project and the member it hands work to, with one live criterion. The
/// routine's own task is left to its alarms.
#[test]
fn the_deployed_button_hands_the_lab_routines_member_a_live_check() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (routine, upkeep) = lab_routine(&hq, &t);
    let (task, card) = pushed(&hq, &t);
    assert_eq!(card.task, Some(task));
    assert!(
        card.text.contains("hq/task-3-tidy-the-deluge-role") && card.text.contains("Deployed"),
        "{}",
        card.text
    );
    let before = hq.store().wakes().unwrap().len();
    let last_task = |hq: &Hq<'_>| {
        hq.store()
            .routines()
            .unwrap()
            .into_iter()
            .find(|r| r.id == routine)
            .unwrap()
            .last_task
    };
    let routines_own = last_task(&hq);
    hq.decide(card.id, "deployed").unwrap();
    let check = deploy_check(&hq);
    assert_eq!(
        check.title,
        "Check the home lab after deploying hq/task-3-tidy-the-deluge-role"
    );
    assert_eq!(
        (check.goal, check.project, check.assignee),
        (t.lab, Some(upkeep), Some(t.ada))
    );
    assert_eq!(check.criteria.len(), 1);
    assert!(check.criteria[0].live, "{:?}", check.criteria);
    assert!(
        check.description.contains("The branch passes its checks"),
        "the change's own criteria are quoted: {}",
        check.description
    );
    assert_eq!(
        hq.store().branches().unwrap().pop().unwrap().status,
        "deployed"
    );
    assert!(wakes_after(&hq, before).contains(&(t.ada, WakeReason::Assigned)));
    assert_eq!(
        last_task(&hq),
        routines_own,
        "the alarms keep their own task"
    );
    assert!(
        hq.events_after(0)
            .unwrap()
            .iter()
            .any(|e| e.kind == crate::domain::EventKind::BranchDeployed.name()),
        "the feed says it"
    );
}

/// Behaviour 2: A second click, or a click after Won't deploy, starts nothing more.
#[test]
fn a_second_click_starts_no_second_check() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    lab_routine(&hq, &t);
    let (_, card) = pushed(&hq, &t);
    hq.decide(card.id, "deployed").unwrap();
    assert!(hq.decide(card.id, "deployed").is_err());
    assert!(hq.decide(card.id, "not_deploying").is_err());
    deploy_check(&hq);
}

/// Behaviour 2b: Won't deploy: the branch says so and no check is made.
#[test]
fn wont_deploy_makes_no_check() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    lab_routine(&hq, &t);
    let (_, card) = pushed(&hq, &t);
    hq.decide(card.id, "not_deploying").unwrap();
    assert!(hq.decide(card.id, "deployed").is_err());
    assert_eq!(
        hq.store().branches().unwrap().pop().unwrap().status,
        "not_deployed"
    );
    assert!(
        hq.store()
            .tasks()
            .unwrap()
            .iter()
            .all(|k| !k.title.starts_with("Check the home lab after deploying "))
    );
}

/// Behaviour 3 and 4: The check's turn is offered HQ's four tools to say what it found, and nothing that
/// makes work or reaches further; the node is told to give it the lab's reads alone, and the
/// runner to start it without a shell. A call outside them is refused by HQ, whatever the
/// model was told.
#[test]
fn the_checks_turn_has_the_lab_reads_and_nothing_more() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    lab_routine(&hq, &t);
    let (_, card) = pushed(&hq, &t);
    hq.decide(card.id, "deployed").unwrap();
    let check = deploy_check(&hq);
    let ada = turn_on(&hq, t.ada, check.id);
    let (run, member) = hq.turn_of(&ada.token).unwrap().unwrap();
    let mut offered: Vec<String> = super::protocol::tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect();
    offered.sort();
    assert_eq!(offered, ["claim", "my_turn", "post", "set_status"]);
    for (tool, args) in [
        ("create_task", json!({ "title": "x", "assignee": "Dee" })),
        ("hand_to", json!({ "task": check.id.0, "to": "Dee" })),
        ("ask_owner", json!({ "text": "x", "evidence": [] })),
        ("read_document", json!({ "document": 1 })),
        ("chess", json!({ "action": "board" })),
        ("recheck", json!({ "task": check.id.0, "why": "x" })),
    ] {
        let said = use_tool(&hq, &ada, tool, &args).unwrap_err();
        assert!(said.contains("post-deploy check"), "{tool}: {said}");
    }
    let said = use_tool(
        &hq,
        &ada,
        "set_status",
        &json!({ "task": check.id.0, "status": "in_review", "reason": "x", "evidence": [] }),
    )
    .unwrap_err();
    assert!(said.contains("post-deploy check"), "{said}");
    assert_eq!(
        hq.lab_tools_of(ada.run).unwrap(),
        Some(
            ["homelab_status", "container_logs", "nas_status"]
                .map(str::to_owned)
                .to_vec()
        )
    );
    assert!(!hq.may_use_a_shell(ada.run).unwrap());
    // Any other turn is as it was: no list, and a shell where the runner gives one.
    let rae = woken_turn(&hq, t.rae, raes_task(&hq, &t));
    assert_eq!(hq.lab_tools_of(rae.run).unwrap(), None);
    assert!(hq.may_use_a_shell(rae.run).unwrap());
}

/// Behaviour 5: A fresh reading, cited: the check closes in its one turn and the change reads healthy,
/// in its own thread and on the desk; nothing wakes the check again.
#[test]
fn a_check_with_a_fresh_reading_closes_healthy_in_one_turn() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    lab_routine(&hq, &t);
    let (task, card) = pushed(&hq, &t);
    hq.decide(card.id, "deployed").unwrap();
    let check = deploy_check(&hq);
    let ada = turn_on(&hq, t.ada, check.id);
    let reading = reads_the_lab(&hq, &ada);
    use_tool(
        &hq,
        &ada,
        "set_status",
        &json!({ "task": check.id.0, "status": "done", "reason": "deluge and sonarr healthy",
                 "evidence": [[format!("r{reading}")]] }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    ends(&hq, &ada);
    assert_eq!(
        hq.store().task(check.id).unwrap().unwrap().status,
        TaskStatus::Done
    );
    let said = thread(&hq, task).pop().unwrap();
    assert!(
        said.contains("Post-deploy infrastructure check: Healthy.")
            && said.contains(&format!("r{reading}"))
            && said.contains("not that every part of the change works"),
        "{said}"
    );
    assert!(
        hq.store()
            .desk()
            .unwrap()
            .iter()
            .any(|d| d.kind == "notice" && d.text.contains(": Healthy")),
        "the digest says it"
    );
    let runs = hq.store().runs().unwrap().len();
    hq.wake(t.ada, Some(check.id), WakeReason::Mentioned)
        .unwrap();
    quiet(&hq);
    assert!(
        hq.store()
            .runs()
            .unwrap()
            .iter()
            .skip(runs)
            .all(|r| r.task != Some(check.id)),
        "one turn"
    );
}

/// Behaviour 5 and 6: A turn that ends without a verdict is inconclusive: the check is closed, the change
/// is not called healthy, and the owner is told.
#[test]
fn a_turn_without_a_verdict_is_inconclusive_and_never_healthy() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    lab_routine(&hq, &t);
    let (task, card) = pushed(&hq, &t);
    hq.decide(card.id, "deployed").unwrap();
    let check = deploy_check(&hq);
    let ada = turn_on(&hq, t.ada, check.id);
    // Done without a reading of her own this turn is refused by the evidence rule.
    assert!(
        use_tool(
            &hq,
            &ada,
            "set_status",
            &json!({ "task": check.id.0, "status": "done", "reason": "looks fine",
                     "evidence": [[format!("task {}", task.0)]] }),
        )
        .is_err()
    );
    ends(&hq, &ada);
    assert_eq!(
        hq.store().task(check.id).unwrap().unwrap().status,
        TaskStatus::Dropped
    );
    let said = thread(&hq, task).pop().unwrap();
    assert!(
        said.contains("Post-deploy infrastructure check: Inconclusive."),
        "{said}"
    );
    assert!(!said.contains("healthy"), "{said}");
    let item = waiting(&hq, "deploy_check");
    assert!(
        item.text.contains(": Inconclusive") && !item.text.to_lowercase().contains("healthy"),
        "{}",
        item.text
    );
    assert!(crate::domain::desk::interrupts("deploy_check"));
}

/// Behaviour 6: The member finds a problem and says so: the check is closed as a problem, in her words,
/// and the owner is told; never healthy.
#[test]
fn a_problem_found_is_reported_as_a_problem() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    lab_routine(&hq, &t);
    let (task, card) = pushed(&hq, &t);
    hq.decide(card.id, "deployed").unwrap();
    let check = deploy_check(&hq);
    let ada = turn_on(&hq, t.ada, check.id);
    reads_the_lab(&hq, &ada);
    use_tool(
        &hq,
        &ada,
        "set_status",
        &json!({ "task": check.id.0, "status": "blocked",
                 "reason": "deluge restarts every minute since the deploy" }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    ends(&hq, &ada);
    assert_eq!(
        hq.store().task(check.id).unwrap().unwrap().status,
        TaskStatus::Dropped
    );
    let said = thread(&hq, task).pop().unwrap();
    assert!(
        said.contains("Post-deploy infrastructure check: Unhealthy.")
            && said.contains("deluge restarts every minute"),
        "{said}"
    );
    let item = waiting(&hq, "deploy_check");
    assert!(
        item.text.contains(": Unhealthy") && !item.text.contains(": Healthy"),
        "{}",
        item.text
    );
}

/// Behaviour 7: The lab's alarms still reach the routine's own task, not the check.
#[test]
fn an_alarm_during_a_check_goes_to_the_routines_own_task() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    lab_routine(&hq, &t);
    let (_, card) = pushed(&hq, &t);
    hq.decide(card.id, "deployed").unwrap();
    let check = deploy_check(&hq);
    let fired = hq.alarm("sonarr became unhealthy at 11:58").unwrap();
    let (_, fired) = fired.expect("the routine takes the alarm");
    let alarm_task = match fired {
        super::Fired::Created(task) | super::Fired::Posted(task) => task,
        super::Fired::Skipped(why) => panic!("{why}"),
    };
    assert_ne!(alarm_task, check.id);
}
