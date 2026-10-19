//! The steps of `features/guards.feature` (ADR 0124): process health over the record, the
//! review carousel, retracted claims and evidence that is already enough.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::application::{Author, NewTask, TeamStore};
use endora_team::domain::guards::THRESHOLDS;
use endora_team::domain::{Priority, TaskId, TaskStatus, WakeReason};
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

#[when("HQ looks at how the team is doing")]
fn looks(w: &mut Office) {
    w.clock
        .0
        .fetch_add(THRESHOLDS.look_every_ms, Ordering::SeqCst);
    w.tick();
}

#[then(expr = "the owner's desk holds a finding {string}")]
fn desk_holds_finding(w: &mut Office, what: String) {
    let found = w.waiting_on_desk("guard");
    assert!(
        found.iter().any(|d| d.text.starts_with(&what)),
        "on the desk: {found:?}"
    );
}

/// What the finding on the owner's desk says, in its own words.
#[then(expr = "the finding says {string}")]
fn the_finding_says(w: &mut Office, said: String) {
    let found = w.waiting_on_desk("guard");
    assert!(
        found.iter().any(|d| d.text.contains(&said)),
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

/// A member tries to narrow what done means for a task, in its running turn, as its worker would
/// with the `narrow_criteria` tool; what was refused is what the member reads.
#[when(regex = r#"^(\w+) tries to narrow what done means for "(.+)" to "(.+)"$"#)]
fn tries_to_narrow(w: &mut Office, name: String, title: String, now: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "narrow_criteria",
        &json!({ "task": task.0, "criteria": [{ "text": now }] }),
    );
}

/// The same, widened: three criteria where there were two.
#[when(regex = r#"^(\w+) tries to give "(.+)" three criteria$"#)]
fn tries_to_widen(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.use_tool(
        &name,
        "narrow_criteria",
        &json!({ "task": task.0,
                 "criteria": [{ "text": "one" }, { "text": "two" }, { "text": "three" }] }),
    );
}

#[when(regex = r#"^(\w+) narrows what done means for "(.+)" to "(.+)"$"#)]
fn narrows(w: &mut Office, name: String, title: String, now: String) {
    tries_to_narrow(w, name, title, now);
    assert_eq!(w.refused, None, "the lead of the area may narrow it");
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

/// A note on the member's own turn's task, which it rests on.
#[when(regex = r#"^(\w+) writes the note "(.+)"$"#)]
fn writes_note(w: &mut Office, name: String, text: String) {
    let task = w.turn_of(&name).task.expect("a turn on a task");
    w.use_tool(
        &name,
        "note",
        &json!({ "text": text, "evidence": [format!("task {}", task.0)] }),
    );
}

impl Office {
    /// The note of `name`'s that says `text`.
    fn note_saying(&self, name: &str, text: &str) -> i64 {
        let member = self.member(name);
        self.hq
            .store()
            .notes(member)
            .expect("the store answers")
            .into_iter()
            .find(|n| n.text == text)
            .unwrap_or_else(|| panic!("{name} has no note {text:?}"))
            .id
    }
}

#[when(regex = r#"^the owner says (\w+)'s note "(.+)" is not true$"#)]
fn owner_retracts(w: &mut Office, name: String, text: String) {
    let note = w.note_saying(&name, &text);
    let said =
        w.hq.retract(&format!("note:{note}"))
            .expect("the owner retracts it");
    w.retraction = Some(said);
}

#[then(regex = r#"^the sweep finds (\w+)'s note "(.+)"$"#)]
fn sweep_finds(w: &mut Office, name: String, text: String) {
    let note = w.note_saying(&name, &text);
    let said = w.retraction.clone().expect("a retraction");
    let found: Vec<&str> = said["sweep"]
        .as_array()
        .expect("its sweep")
        .iter()
        .filter_map(|s| s["target"].as_str())
        .collect();
    assert_eq!(found, [format!("note:{note}").as_str()], "{said}");
}

#[when("the owner forgets what the sweep found")]
fn forgets_swept(w: &mut Office) {
    let said = w.retraction.clone().expect("a retraction");
    let targets: Vec<String> = said["sweep"]
        .as_array()
        .expect("its sweep")
        .iter()
        .filter_map(|s| s["target"].as_str().map(str::to_owned))
        .collect();
    let id = said["retraction"].as_i64().expect("its id");
    w.hq.forget_swept(id, &targets).expect("the owner forgets");
}

#[then(regex = r"^(\w+) has no notes$")]
fn no_notes(w: &mut Office, name: String) {
    let member = w.member(&name);
    assert!(
        w.hq.store()
            .notes(member)
            .expect("the store answers")
            .is_empty()
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

/// The wing's lead closes his plan as done, saying it was superseded, and by what if he does.
fn closes_the_plan_superseded(w: &mut Office, name: &str, successor: Option<TaskId>) {
    let plan = w.wing.as_ref().expect("the scenario has the wing").task;
    let member = w.member(name);
    let held =
        w.hq.store()
            .task(plan)
            .expect("the store answers")
            .expect("the plan");
    if held.owner != Some(member) {
        w.use_tool(name, "claim", &json!({ "task": plan.0 }))
            .expect("the lead takes his plan");
    }
    // One list of evidence for each of the plan's criteria: the plan itself will do.
    let cited = vec![vec![format!("task {}", plan.0)]; held.criteria.len().max(1)];
    w.use_tool(
        name,
        "set_status",
        &json!({ "task": plan.0, "status": "done", "reason": "the branches overtook the plan",
                 "why": "superseded", "successor": successor.map(|t| t.0),
                 "evidence": cited }),
    );
}

#[when(regex = r"^(\w+) closes the plan as superseded, naming no successor$")]
fn closes_the_plan_without_a_successor(w: &mut Office, name: String) {
    closes_the_plan_superseded(w, &name, None);
}

#[when(regex = r#"^(\w+) closes the plan as superseded by "(.+)"$"#)]
fn closes_the_plan_superseded_by(w: &mut Office, name: String, successor: String) {
    let successor = w.task(&successor);
    closes_the_plan_superseded(w, &name, Some(successor));
}

#[then(regex = r"^the plan is (still open|done)$")]
fn the_plan_is(w: &mut Office, which: String) {
    let plan = w.wing.as_ref().expect("the scenario has the wing").task;
    let status =
        w.hq.store()
            .task(plan)
            .expect("the store answers")
            .expect("the plan")
            .status;
    assert_eq!(
        status == TaskStatus::Done,
        which == "done",
        "the plan is {status:?}"
    );
}
