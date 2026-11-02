//! The engineering wing and the git flow (ADR 0115): a submitted branch recorded with its
//! document and check, the check's result as a receipt that wakes or sends back, the owner's
//! push offered only once the work is done and run only on their approval, hiring by the owner
//! and documents read.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings, Submission};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::application::desk_ports::DeskStore;
use crate::application::receipt_ports::ReceiptStore;
use crate::application::wing_ports::WingStore;
use crate::domain::wing::{WING_GOAL, WING_PROJECT};
use crate::domain::{
    Brain, Criterion, GoalId, MemberId, Priority, Refusal, TaskId, TaskStatus, WakeReason,
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

/// The wing as the tests have it: the goal and its project, Kit leading it on Claude Sonnet, Rae
/// and Zed on Claude Haiku, and Kit's first task, the plan.
struct Wing {
    goal: GoalId,
    lead: MemberId,
    developers: Vec<MemberId>,
    task: TaskId,
}

fn wing(hq: &Hq<'_>) -> Wing {
    let lead = hq.hire("Kit", 25, 2, 5).unwrap();
    hq.set_brain(lead, Brain::Sonnet.name()).unwrap();
    let mut developers = Vec::new();
    for (name, look) in [("Rae", 2), ("Zed", 4)] {
        let id = hq.hire(name, 40, 2, look).unwrap();
        hq.set_brain(id, Brain::Haiku.name()).unwrap();
        developers.push(id);
    }
    let goal = hq.set_goal(WING_GOAL, "", Author::Owner).unwrap();
    let project = hq.add_project(goal, WING_PROJECT, "", lead, None).unwrap();
    let task = hq
        .add_task_with(
            Author::Owner,
            NewTask {
                goal,
                project: Some(project),
                parent: None,
                title: "Plan the clean-up".to_owned(),
                description: String::new(),
                assignee: Some(lead),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
            &[Criterion::new("Three branches are proposed", false)],
        )
        .unwrap();
    Wing {
        goal,
        lead,
        developers,
        task,
    }
}

/// The team as the owner has it: the CEO, Ada the lead, Dee the tester, and the wing.
struct Team {
    ceo: MemberId,
    dee: MemberId,
    kit: MemberId,
    rae: MemberId,
    goal: GoalId,
    plan: TaskId,
}

fn team(hq: &Hq<'_>) -> Team {
    let ceo = {
        let id = hq.hire("Mara", 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    hq.hire("Ada", 25, 2, 0).unwrap();
    let dee = hq.hire("Dee", 20, 2, 1).unwrap();
    hq.set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let seeded = wing(hq);
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
        dee,
        kit: seeded.lead,
        rae: seeded.developers[0],
        goal: seeded.goal,
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
        Author::Owner,
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
        changed: Vec::new(),
        texts: Vec::new(),
        workflows: None,
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
            Author::Owner,
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
            Author::Owner,
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
    // Work with no branch has nothing to check.
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
        Author::Owner,
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

    // The owner's word on the push: the cards are back (ADR 0115, amended 2026-10-31).
    hq.set_delivery(Some(true), None).unwrap();
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
    hq.set_delivery(Some(true), None).unwrap();
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
fn the_owner_hires_any_title_on_any_model_endora_has() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let sam = hq.hire_by_owner("Sam", "claude-haiku-5-5").unwrap();
    let members = hq.store().members().unwrap();
    let found = members.iter().find(|m| m.id == sam).unwrap();
    assert_eq!((found.brain, found.share_percent), (Brain::Haiku, 40));
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
    // A title is a name only (ADR 0142): a second CEO, or a researcher on Sonnet, is a member.
    hq.hire_by_owner("Second", "claude-opus-5-5").unwrap();
    hq.hire_by_owner("Pat", "claude-sonnet-5-5").unwrap();
    assert!(matches!(
        hq.hire_by_owner("Quinn", "gpt-5"),
        Err(super::EngineError::Refused(Refusal::NotABrain { .. }))
    ));
    assert_eq!(
        hq.hire_by_owner("sam", "local"),
        Err(super::EngineError::Refused(Refusal::NotAName)),
        "names stay unique"
    );
    let _ = t.ceo;
}

// --- Accepted work reaches the fork without a card (ADR 0115, amended 2026-10-31) ------------

/// Without a deploy key, an accepted branch is listed with why it is not pushed, never put on
/// the desk and never asked of the node.
#[test]
fn without_a_deploy_key_accepted_work_is_listed_not_pushed() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, _, _) = submitted(&hq, &t, false);
    let handed = hq.next_check().unwrap().expect("a check waits");
    hq.check_done(handed.id, &handed.token, true, "syntax ok", "")
        .unwrap();
    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    hq.offer_push(task).unwrap();
    assert!(hq.store().desk().unwrap().iter().all(|d| d.kind != "push"));
    assert!(hq.deliveries().unwrap().push.is_empty());
    let listed = hq.snapshot().unwrap()["accepted"].clone();
    assert_eq!(listed[0]["status"], "accepted", "{listed}");
    assert!(
        listed[0]["why"]
            .as_str()
            .unwrap()
            .contains("pushing is not set up"),
        "{listed}"
    );
}

/// A branch whose check has not passed is not pushed by rule, whatever review said.
#[test]
fn a_branch_whose_check_did_not_pass_is_not_pushed() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, _, _) = submitted(&hq, &t, true);
    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    hq.offer_push(task).unwrap();
    assert!(hq.deliveries().unwrap().push.is_empty());
    let listed = hq.snapshot().unwrap()["accepted"].clone();
    assert_eq!(listed[0]["why"], "no check of it passed", "{listed}");
}

/// The node's word on each step is taken once; a merge is asked only while merging is on, and
/// a branch merged leaves the list.
#[test]
fn each_step_is_told_once_and_merging_waits_for_its_setting() {
    use super::engine::Delivered;
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let (task, _, _) = submitted(&hq, &t, true);
    let handed = hq.next_check().unwrap().expect("a check waits");
    hq.check_done(handed.id, &handed.token, true, "syntax ok", "")
        .unwrap();
    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    hq.offer_push(task).unwrap();
    let due = hq.deliveries().unwrap();
    assert_eq!(due.push.len(), 1);
    let branch = due.push[0].branch.id;
    let merged = Delivered::Merged {
        ok: true,
        text: "merged".to_owned(),
    };
    assert!(hq.delivered(branch, &merged).is_err(), "not pushed yet");
    let pushed = Delivered::Pushed {
        ok: true,
        text: "pushed".to_owned(),
        needs: Vec::new(),
    };
    hq.delivered(branch, &pushed).unwrap();
    assert!(hq.delivered(branch, &pushed).is_err(), "told once");
    assert!(hq.deliveries().unwrap().merge.is_empty(), "merging is off");
    hq.set_delivery(None, Some(true)).unwrap();
    assert_eq!(hq.deliveries().unwrap().merge.len(), 1);
    hq.delivered(branch, &merged).unwrap();
    assert!(hq.deliveries().unwrap().merge.is_empty());
    assert_eq!(hq.snapshot().unwrap()["accepted"], json!([]));
    let landed = hq.snapshot().unwrap()["scorecard"]["outcomes"]["day"]["landed"].clone();
    assert_eq!(landed, 1, "an automatic push is a landed change");
}

// --- A repository outside the lab (ADR 0115, amended 2026-11-01: round 4's F4) ---------------

fn wing_project(hq: &Hq<'_>) -> crate::domain::ProjectId {
    hq.store()
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.title == WING_PROJECT)
        .expect("the wing's project")
        .id
}

/// The owner names the repository a project's work is done in: a public GitHub address alone,
/// kept in its one plain form, and taken away again with none.
#[test]
fn the_owner_names_an_outside_repository_on_a_project_and_only_a_public_github_one() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    let project = wing_project(&hq);
    for refused in [
        "https://gitlab.com/example/tiny",
        "git@github.com:example/tiny.git",
        // A credential in the address (split so it reads as no mailbox).
        concat!("https://jane:secret", "@", "github.com/example/tiny"),
        "http://github.com/example/tiny",
    ] {
        let why = hq
            .set_project_repo(project, Some(refused))
            .unwrap_err()
            .to_string();
        assert!(why.contains("only a public https://github.com/"), "{why}");
    }
    assert_eq!(hq.project_repo(project).unwrap(), None, "nothing kept");
    assert_eq!(
        hq.set_project_repo(project, Some("https://github.com/example/tiny.git"))
            .unwrap()
            .as_deref(),
        Some("https://github.com/example/tiny")
    );
    // A turn on work under the project (Rae's task sits under the plan, which is in it) is
    // told the repository; the node mirrors and hands out that one.
    let task = raes_task(&hq, &t);
    let rae = turn(&hq, t.rae);
    use_tool(&hq, &rae, "claim", &json!({ "task": task.0 })).unwrap();
    let (run, _) = hq.turn_of(&rae.token).unwrap().expect("a running turn");
    assert_eq!(
        hq.outside_repo_of_run(run.id).unwrap().as_deref(),
        Some("https://github.com/example/tiny")
    );
    hq.set_project_repo(project, None).unwrap();
    assert_eq!(hq.outside_repo_of_run(run.id).unwrap(), None);
}

/// A branch of an outside repository is kept on the node for judging: no check of the lab's is
/// queued (ansible's checks say nothing of it), its item's criteria are its gate, and accepted,
/// it is listed as never pushed: not due to the node, not carded, whatever the deploy key.
#[test]
fn a_branch_of_an_outside_repository_is_kept_for_judging_and_never_delivered() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.set_project_repo(wing_project(&hq), Some("https://github.com/example/tiny"))
        .unwrap();
    let (task, _, recorded) = submitted(&hq, &t, true);
    assert_eq!(recorded.check, 0, "no check is queued");
    assert!(hq.next_check().unwrap().is_none());
    assert_eq!(hq.store().gate(task).unwrap(), Some(true));
    let last = thread(&hq, task).pop().unwrap();
    assert!(
        last.contains("kept on the node for judging") && last.contains("never pushed"),
        "{last}"
    );
    hq.store().set_task_status(task, TaskStatus::Done).unwrap();
    hq.offer_push(task).unwrap();
    assert!(hq.deliveries().unwrap().push.is_empty());
    let listed = hq.snapshot().unwrap()["accepted"].clone();
    assert_eq!(listed[0]["status"], "accepted", "{listed}");
    assert!(
        listed[0]["why"].as_str().unwrap().contains("never pushed"),
        "{listed}"
    );
    // With the cards back on the desk, still no push card for it.
    hq.set_delivery(Some(true), None).unwrap();
    all_turns_over(&hq);
    let (other, _, _) = submitted(&hq, &t, true);
    hq.store().set_task_status(other, TaskStatus::Done).unwrap();
    hq.offer_push(other).unwrap();
    assert!(hq.store().desk().unwrap().iter().all(|d| d.kind != "push"));
    assert!(hq.deliveries().unwrap().push.is_empty());
}

/// #1048 of set 3: Sol's simpleeval fix was submitted and its gate open, and Sol asked for it
/// to be checked again for a receipt to cite. HQ queued the lab's check on the outside branch;
/// the check box fetched its bundle where the lab's branches live, got the node's 404, and the
/// failed check shut the gate and counted as one of the item's failures, twice. No check of the
/// lab's applies to an outside repository, so asking for one is refused, saying how the work is
/// evidenced instead, and nothing is queued or shut.
#[test]
fn a_check_again_of_an_outside_branch_is_refused_and_the_gate_stays_open() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let t = team(&hq);
    hq.set_project_repo(wing_project(&hq), Some("https://github.com/example/tiny"))
        .unwrap();
    let (task, rae, _) = submitted(&hq, &t, true);
    assert_eq!(hq.store().gate(task).unwrap(), Some(true));
    let said = use_tool(
        &hq,
        &rae,
        "recheck",
        &json!({ "task": task.0, "why": "a receipt to cite for in_review" }),
    )
    .unwrap_err();
    assert!(
        said.contains("no check of the lab's applies to https://github.com/example/tiny")
            && said.contains("set_status in_review")
            && said.contains("repo_submit"),
        "{said}"
    );
    assert!(hq.store().checks().unwrap().is_empty(), "nothing is queued");
    assert!(hq.next_check().unwrap().is_none());
    assert_eq!(
        hq.store().gate(task).unwrap(),
        Some(true),
        "the gate stays open"
    );
    // As the refusal says: in review, citing the submission's receipt for each criterion.
    let receipt = hq
        .record_receipt(
            &rae.token,
            &super::engine::Called {
                tool: "repo_submit".to_owned(),
                args: r#"{"branch":"hq/task-3-tidy-the-deluge-role"}"#.to_owned(),
                excerpt: "Submitted hq/task-3-tidy-the-deluge-role".to_owned(),
                ok: true,
                act: true,
                confidential: false,
            },
        )
        .unwrap();
    // Rae's task has one criterion.
    let evidence = json!([[format!("r{receipt}")]]);
    let moved = use_tool(
        &hq,
        &rae,
        "set_status",
        &json!({ "task": task.0, "status": "in_review", "reason": "its tests pass",
                 "evidence": evidence }),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let status = hq.store().task(task).unwrap().unwrap().status;
    assert_eq!(status, TaskStatus::InReview, "{moved}");
}
