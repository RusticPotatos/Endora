//! The steps of `features/guards.feature` (ADR 0124): retracted claims and evidence that is
//! already enough.

use cucumber::{given, then, when};
use endora_team::application::{Author, NewTask, TeamStore};
use endora_team::domain::{Priority, WakeReason};
use serde_json::json;

use super::Office;

impl Office {}

/// `turns` turns of the member's on the task, each a review asked for (judgment, so on its Claude
/// and paid for), each only a post, each costing `dollars`.
#[when(
    regex = r#"^(\w+) reviews "(.+)" (\d+) times at \$(\d+(?:\.\d+)?) a turn and changes nothing$"#
)]
fn reviews_and_changes_nothing(
    w: &mut Office,
    name: String,
    title: String,
    turns: usize,
    dollars: f64,
) {
    let member = w.member(&name);
    let task = w.task(&title);
    for i in 0..turns {
        w.hq.wake(member, Some(task), WakeReason::ReviewRequested)
            .expect("a wake has a receipt");
        let context = w.turn_of(&name);
        w.hq.report_usage(context.run, super::micros(dollars))
            .expect("the ledger takes the spend");
        w.use_tool(
            &name,
            "post",
            &json!({ "task": task.0, "text": format!("Still reading it ({i}).") }),
        )
        .expect("a member may post in the task its turn is on");
        super::turn_ends(w, name.clone());
    }
}

#[given(regex = r#"^(\w+) has the task "(.+)", done when "(.+)"$"#)]
fn has_task_done_when(w: &mut Office, name: String, title: String, criterion: String) {
    let to = w.member(&name);
    let goal = w.goal();
    let made = w.hq.add_task_with(
        endora_team::application::Author::Owner,
        endora_team::application::NewTask {
            goal,
            project: None,
            parent: None,
            title,
            description: String::new(),
            assignee: Some(to),
            priority: endora_team::domain::Priority::Normal,
            waits_on: Vec::new(),
        },
        &[endora_team::domain::Criterion::new(&criterion, false)],
    );
    w.note(made).expect("the owner hands it out");
}

/// The owner's task for a member with two things that must hold.
#[given(regex = r#"^(\w+) has the task "(.+)" that is done when both "(.+)" and "(.+)" hold$"#)]
fn has_task_done_when_both(
    w: &mut Office,
    name: String,
    title: String,
    first: String,
    second: String,
) {
    let to = w.member(&name);
    let goal = w.goal();
    let made = w.hq.add_task_with(
        endora_team::application::Author::Owner,
        endora_team::application::NewTask {
            goal,
            project: None,
            parent: None,
            title,
            description: String::new(),
            assignee: Some(to),
            priority: endora_team::domain::Priority::Normal,
            waits_on: Vec::new(),
        },
        &[
            endora_team::domain::Criterion::new(&first, false),
            endora_team::domain::Criterion::new(&second, false),
        ],
    );
    w.note(made).expect("the owner hands it out");
}

#[then(regex = r#"^the task "(.+)" is done when "(.+)"$"#)]
fn task_is_done_when(w: &mut Office, title: String, criterion: String) {
    let task = w.task(&title);
    let criteria: Vec<String> =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("the task is there")
            .criteria
            .into_iter()
            .map(|c| c.text)
            .collect();
    assert_eq!(criteria, [criterion]);
}

impl Office {
    /// The node's receipt of a reading of sonarr's logs in `member`'s running turn.
    fn reads_the_logs(&mut self, member: &str) {
        let context = self.turn_of(member);
        self.hq
            .record_receipt(
                &context.token,
                &endora_team::application::engine::Called {
                    tool: "container_logs".to_owned(),
                    args: "{\"name\":\"sonarr\"}".to_owned(),
                    excerpt: "sonarr: killed, out of memory".to_owned(),
                    ok: true,
                    act: false,
                    confidential: false,
                },
            )
            .expect("the node's receipt is kept");
    }
}

#[when(regex = r#"^(\w+) takes "(.+)" and reads sonarr's logs$"#)]
fn takes_and_reads(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its task");
    w.reads_the_logs(&name);
    super::turn_ends(w, name);
}

#[when(regex = r"^(\w+) is woken and reads sonarr's logs again$")]
fn reads_again(w: &mut Office, name: String) {
    let member = w.member(&name);
    let task = w
        .turns
        .get(&name)
        .and_then(|t| t.task)
        .expect("a turn on a task before");
    w.hq.wake(member, Some(task), WakeReason::Mentioned)
        .expect("a wake has a receipt");
    w.reads_the_logs(&name);
    super::turn_ends(w, name);
}

#[then(regex = r#"^HQ says in "(.+)": "(.+)"$"#)]
fn hq_says_in(w: &mut Office, title: String, said: String) {
    let task = w.task(&title);
    let thread = w.hq.store().thread(task).expect("the store answers");
    assert!(
        thread
            .iter()
            .any(|p| p.author == "endora" && p.text.contains(&said)),
        "the thread: {thread:?}"
    );
}

impl Office {}

// --- The closure contract (ADR 0124, amended) -----------------------------------------------

/// The owner hands a member a task that waits on another.
#[when(regex = r#"^the owner hands (\w+) the task "(.+)", waiting on "(.+)"$"#)]
fn owner_hands_a_task_that_waits(w: &mut Office, name: String, title: String, waits_on: String) {
    let to = w.member(&name);
    let goal = w.goal();
    let wait = w.task(&waits_on);
    w.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: None,
            parent: None,
            title,
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: vec![wait],
        },
    );
}
