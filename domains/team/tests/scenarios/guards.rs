//! The steps of `features/guards.feature` (ADR 0124): process health over the record, the
//! review carousel, retracted claims and evidence that is already enough.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::application::TeamStore;
use endora_team::domain::WakeReason;
use endora_team::domain::guards::THRESHOLDS;
use serde_json::json;

use super::Office;

impl Office {
    /// The guards, as the HQ screen's Guards card reads them.
    fn guards(&self) -> serde_json::Value {
        self.hq.snapshot().expect("the snapshot is built")["guards"].clone()
    }
}

#[given(regex = r#"^the project "(.+)" led by (\w+)$"#)]
fn project_led_by(w: &mut Office, title: String, lead: String) {
    let lead = w.member(&lead);
    let goal = w.goal();
    let id =
        w.hq.add_project(goal, &title, "", lead, None)
            .expect("the owner adds a project");
    w.projects.insert(title, id);
}

/// `turns` turns of the member's on the task, each woken by a mention, each only a post.
#[when(regex = r#"^(\w+) talks about "(.+)" for (\d+) turns and changes nothing$"#)]
fn talks(w: &mut Office, name: String, title: String, turns: usize) {
    let member = w.member(&name);
    let task = w.task(&title);
    for i in 0..turns {
        w.hq.wake(member, Some(task), WakeReason::Mentioned)
            .expect("a wake has a receipt");
        w.use_tool(
            &name,
            "post",
            &json!({ "task": task.0, "text": format!("Still weighing how to do it ({i}).") }),
        )
        .expect("a member may post in its own task");
        super::turn_ends(w, name.clone());
    }
}

#[when("HQ looks at how the team is doing")]
fn looks(w: &mut Office) {
    w.clock
        .0
        .fetch_add(THRESHOLDS.look_every_ms, Ordering::SeqCst);
    w.hq.tick().expect("the tick runs");
}

#[then(expr = "the owner's desk holds a finding {string}")]
fn desk_holds_finding(w: &mut Office, what: String) {
    let found = w.waiting_on_desk("guard");
    assert!(
        found.iter().any(|d| d.text.starts_with(&what)),
        "on the desk: {found:?}"
    );
}

#[then(expr = "the guards card says {string}")]
fn guards_card_says(w: &mut Office, said: String) {
    let card = w.guards().to_string();
    assert!(card.contains(&said), "the guards: {card}");
}

#[when("the owner says the finding was real")]
fn owner_says_real(w: &mut Office) {
    let item = w
        .waiting_on_desk("guard")
        .pop()
        .expect("a finding on the desk");
    let decided = w.hq.decide(item.id, "real");
    w.note(decided);
}

#[then(regex = r#"^the guard "(\w+)" is (\d+)% real$"#)]
fn guard_precision(w: &mut Office, guard: String, percent: u64) {
    let card = w.guards();
    let found = card["precision"]
        .as_array()
        .expect("each guard's precision")
        .iter()
        .find(|p| p["guard"] == guard.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("no guard called {guard}"));
    assert_eq!(found["precision"], json!(percent), "{found}");
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

#[then(regex = r#"^a huddle is called on "(.+)", because it was "(.+)"$"#)]
fn huddle_called_because(w: &mut Office, title: String, why: String) {
    let task = w.task(&title);
    let called: Vec<serde_json::Value> =
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .into_iter()
            .filter(|e| e.kind == "huddle.called" && e.data["task"] == task.0)
            .map(|e| e.data)
            .collect();
    assert!(
        called
            .iter()
            .any(|d| d["why"].as_str().is_some_and(|w| w.contains(&why))),
        "huddles on {title}: {called:?}"
    );
}
