//! HQ's mechanical checks when work reaches review (ADR 0142, amended 2026-10-30), over HQ's
//! real SQLite store and the simulated runner: the worker's quotes against their receipts, the
//! branch's files against the forbidden paths and the task's named ones, and the role README
//! against its defaults. All data is fictional.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Called, Engine, Settings, Submission};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use crate::domain::money::DOLLAR;
use crate::domain::{
    Criterion, GoalId, MemberId, Priority, ProjectId, Role, TaskId, TaskStatus, WakeReason,
};
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

struct Team {
    bo: MemberId,
    rae: MemberId,
    loop_: ProjectId,
    goal: GoalId,
}

/// The loops' five, ranked by model: Bo at the bottom, Rae one rank up.
fn team(hq: &Hq<'_>) -> Team {
    let goal = hq
        .set_goal("Keep the fork tidy", "you asked", Author::Owner)
        .unwrap();
    let seat = |name: &str, model: &str, role: Role| {
        let id = hq.hire(name, role, 10, 2, 0).unwrap();
        hq.set_model(id, model).unwrap();
        id
    };
    let mara = seat("Mara", "opus", Role::Ceo);
    seat("Kit", "sonnet", Role::Lead);
    seat("Ada", "haiku", Role::Lead);
    let rae = seat("Rae", "mac", Role::Developer);
    let bo = seat("Bo", "local", Role::Developer);
    let loop_ = hq
        .add_project(goal, "ansible-nas", "the fork's work", mara, None)
        .unwrap();
    Team {
        bo,
        rae,
        loop_,
        goal,
    }
}

fn item(hq: &Hq<'_>, t: &Team, title: &str, description: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: Some(t.loop_),
            parent: None,
            title: title.to_owned(),
            description: description.to_owned(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn turn_of(hq: &Hq<'_>, member: MemberId) -> RunContext {
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
    super::protocol::call(hq, &run, &member, tool, args)
}

fn woken(hq: &Hq<'_>, member: MemberId, task: TaskId, reason: WakeReason) -> bool {
    hq.wakes()
        .unwrap()
        .iter()
        .any(|w| w.member == member && w.task == Some(task) && w.reason == reason)
}

fn receipt(hq: &Hq<'_>, context: &RunContext, tool: &str, excerpt: &str) -> i64 {
    hq.record_receipt(
        &context.token,
        &Called {
            tool: tool.to_owned(),
            args: String::new(),
            excerpt: excerpt.to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
}

fn submit(hq: &Hq<'_>, context: &RunContext, changed: &[&str], texts: &[(&str, &str)]) {
    hq.record_branch(
        &context.token,
        &Submission {
            branch: "hq/task-1-document-the-media-role".to_owned(),
            head: "a1b2c3d4e5f60718293a4b5c".to_owned(),
            base: "0123456789abcdef01234567".to_owned(),
            files: changed.len(),
            plus: 20,
            minus: 0,
            stat: String::new(),
            diff: String::new(),
            can_push: false,
            changed: changed.iter().map(|c| (*c).to_owned()).collect(),
            texts: texts
                .iter()
                .map(|(p, t)| ((*p).to_owned(), (*t).to_owned()))
                .collect(),
        },
    )
    .unwrap();
}

fn thread(hq: &Hq<'_>, task: TaskId) -> Vec<String> {
    hq.store()
        .thread(task)
        .unwrap()
        .into_iter()
        .map(|p| p.text)
        .collect()
}

const DEFAULTS: &str = "---\nmedia_enabled: false\nmedia_port: \"8096\"  # web\n\
                        media_data: \"{{ docker_home }}/media\"\n";
const README: &str = "# Media\n\n| Variable | Default |\n|---|---|\n\
                      | `media_enabled` | `false` |\n| `media_port` | `8080` |\n";

/// The owner, 2026-10-30: what can be judged mechanically, HQ judges itself, and the reviewer
/// is told the results.
///
/// Given Bo's item whose criteria ask for a README listing every default, a log reading, and a
/// branch that changes the role's README and one file outside the paths the task names,
/// when Bo asks for review quoting one line the reading holds and one it does not,
/// then HQ posts its checks on the item (1 of 2 quotes found, the file out of scope, the
/// default not named and the one with the wrong value) and Rae's briefing carries them.
#[test]
fn work_reaching_review_is_checked_mechanically_and_the_reviewer_is_told() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let t = team(&hq);
    let fix = item(
        &hq,
        &t,
        "Document the media role",
        "Write roles/media/README.md from the role's defaults.",
    );
    hq.store()
        .set_criteria(
            fix,
            &[Criterion::new(
                "The README lists every default with its value",
                false,
            )],
        )
        .unwrap();
    let bo = turn_of(&hq, t.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    let logs = receipt(&hq, &bo, "read_logs", "media: listening on 8096, ready");
    submit(
        &hq,
        &bo,
        &["roles/media/README.md", "docs/notes.md"],
        &[
            ("roles/media/README.md", README),
            ("roles/media/defaults/main.yml", DEFAULTS),
        ],
    );
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review",
            "reason": format!("Documented. r{logs} shows `listening on 8096` and `port 9000 open`."),
            "evidence": [format!("r{logs}")] }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    let posted = thread(&hq, fix).join("\n");
    assert!(
        posted.contains(crate::domain::mechanical::HEADER),
        "{posted}"
    );
    assert!(
        posted.contains("1 of 2 quoted lines are in their receipts"),
        "{posted}"
    );
    assert!(
        posted.contains(&format!("r{logs}: `port 9000 open`: NOT in its result")),
        "{posted}"
    );
    assert!(
        posted.contains("Outside the paths the task names (roles/media/README.md): docs/notes.md"),
        "{posted}"
    );
    assert!(
        posted.contains("covers 1 of 3 defaults. Not named: media_data. Named without its value: media_port (default 8096)."),
        "{posted}"
    );
    assert!(woken(&hq, t.rae, fix, WakeReason::ReviewRequested));
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, t.rae);
    // Shown once, in full where the review is asked: the review's briefing carries no thread
    // history (ADR 0142, amended 2026-10-30).
    assert_eq!(
        rae.told.matches(crate::domain::mechanical::HEADER).count(),
        1,
        "{}",
        rae.told
    );
    assert!(
        rae.told
            .contains("covers 1 of 3 defaults. Not named: media_data.")
            && rae.told.contains("These are facts HQ measured"),
        "{}",
        rae.told
    );
}

/// A branch that touches the owner's values is sent back by HQ, with the reason, and no
/// reviewer is woken: it never needed a model's judgment.
///
/// Given Bo's item whose branch changes `group_vars/all.yml`,
/// when Bo asks for review,
/// then the item is back with Bo (doing, woken as sent back, one failure), the thread says
/// why, and Rae is not woken.
#[test]
fn a_branch_that_touches_a_forbidden_path_is_sent_back_by_hq() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let t = team(&hq);
    let fix = item(&hq, &t, "Tidy the media role", "");
    let bo = turn_of(&hq, t.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    submit(
        &hq,
        &bo,
        &["roles/media/tasks/main.yml", "group_vars/all.yml"],
        &[],
    );
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "Tidied.",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    hq.runner().finish(bo.run, true);
    assert_eq!(
        hq.store().task(fix).unwrap().unwrap().status,
        TaskStatus::Doing
    );
    let posted = thread(&hq, fix).join("\n");
    assert!(
        posted.contains("FORBIDDEN (never to be touched): group_vars/all.yml"),
        "{posted}"
    );
    assert!(
        posted.contains("Sent back by HQ: the branch touches group_vars/all.yml"),
        "{posted}"
    );
    assert!(woken(&hq, t.bo, fix, WakeReason::SentBack));
    assert!(!woken(&hq, t.rae, fix, WakeReason::ReviewRequested));
    assert_eq!(hq.loop_failures(fix).unwrap(), 1);
}

/// Work with nothing to check mechanically (no quote, no branch) has no report posted.
#[test]
fn work_with_nothing_to_check_has_no_report() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let t = team(&hq);
    let fix = item(&hq, &t, "Fix the lint", "");
    let bo = turn_of(&hq, t.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": fix.0 })).unwrap();
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": fix.0, "status": "in_review", "reason": "The lint passes.",
                             "evidence": [format!("task {}", fix.0)] }),
    )
    .unwrap();
    let posted = thread(&hq, fix).join("\n");
    assert!(
        !posted.contains(crate::domain::mechanical::HEADER),
        "{posted}"
    );
    assert!(woken(&hq, t.rae, fix, WakeReason::ReviewRequested));
}

/// A quote is checked against a reading of this item's, and only this item's: a line quoted
/// from another item's reading, though it is word for word in that reading, proves nothing of
/// this one.
///
/// Given Bo's item and Rae's, and a reading Rae took on hers,
/// when Bo asks for review quoting that reading,
/// then HQ's checks say the receipt is not this item's.
#[test]
fn a_quote_from_another_items_reading_is_not_this_items() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let t = team(&hq);
    let mine = item(&hq, &t, "Document the media role", "");
    let hers = item(&hq, &t, "Tidy the compose files", "");
    hq.tell_mac(true, "ready");
    let bo = turn_of(&hq, t.bo);
    use_tool(&hq, &bo, "claim", &serde_json::json!({ "task": mine.0 })).unwrap();
    let rae = turn_of(&hq, t.rae);
    use_tool(&hq, &rae, "claim", &serde_json::json!({ "task": hers.0 })).unwrap();
    let theirs = receipt(&hq, &rae, "read_logs", "compose: all services up");
    use_tool(
        &hq,
        &bo,
        "set_status",
        &serde_json::json!({ "task": mine.0, "status": "in_review",
            "reason": format!("Documented. r{theirs} shows `all services up`."),
            "evidence": [format!("task {}", mine.0)] }),
    )
    .unwrap();
    let posted = thread(&hq, mine).join("\n");
    assert!(
        posted.contains("0 of 1 quoted lines are in their receipts"),
        "{posted}"
    );
    assert!(
        posted.contains(&format!(
            "r{theirs}: `all services up`: not this item's receipt"
        )),
        "{posted}"
    );
}
