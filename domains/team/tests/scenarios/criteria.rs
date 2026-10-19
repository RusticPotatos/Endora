//! The steps of `features/what-done-means-comes-first.feature` (ADR 0124, amended): the owner's
//! work says what done means, and a lead's split of it says which criterion each part serves.

use cucumber::{then, when};
use endora_team::application::{Author, NewTask};
use endora_team::domain::{Criterion, Priority};
use serde_json::json;

use super::Office;

impl Office {
    /// The owner hands `to` the work `title` by the front door Give work and the API use, which
    /// asks what done means.
    fn owner_gives(&mut self, to: &str, title: &str, criteria: &[Criterion]) {
        let to = self.member(to);
        let goal = self.goal();
        let made = self.hq.add_task_with(
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
            criteria,
        );
        self.note(made);
    }

    /// `name`, in its running turn on the task `of`, hands `to` a part of it with `create_task`,
    /// as its worker would, saying which criterion of the task the part serves if it does.
    fn hands_a_part(
        &mut self,
        name: &str,
        to: &str,
        part: &str,
        of: &str,
        done: &str,
        serves: Option<&str>,
    ) {
        let task = self.task(of);
        let context = self.turn_of(name);
        assert_eq!(context.task, Some(task), "{name}'s turn is on another task");
        self.use_tool(
            name,
            "create_task",
            &json!({ "title": part, "description": "a part of the task", "assignee": to,
                     "criteria": [{ "text": done }], "serves": serves }),
        );
    }

    /// `name`, in its running turn, asks the owner to approve running `tool`, as the node does
    /// for it when a bridged act asks, saying which criterion of its task the request serves if
    /// it does.
    fn asks_for_a_yes(&mut self, name: &str, tool: &str, why: &str, serves: Option<&str>) {
        let context = self.turn_of(name);
        let (run, _) = self
            .hq
            .turn_of(&context.token)
            .expect("the store answers")
            .expect("a running turn");
        let asked = self.hq.ask_approval(
            run.id,
            &format!("{name} asks to run {tool}. Why: {why}."),
            serves,
        );
        self.note(asked);
    }
}

#[when(regex = r#"^the owner gives (\w+) the work "([^"]+)" with no criteria$"#)]
fn gives_with_no_criteria(w: &mut Office, to: String, title: String) {
    w.owner_gives(&to, &title, &[]);
}

#[when(regex = r#"^the owner gives (\w+) the work "([^"]+)", done when "([^"]+)"$"#)]
fn gives_done_when(w: &mut Office, to: String, title: String, done: String) {
    w.owner_gives(&to, &title, &[Criterion::new(&done, false)]);
}

#[when(regex = r#"^(\w+) hands (\w+) the part "([^"]+)" of "([^"]+)", done when "([^"]+)"$"#)]
fn hands_a_part(w: &mut Office, by: String, to: String, part: String, of: String, done: String) {
    w.hands_a_part(&by, &to, &part, &of, &done, None);
}

#[when(
    regex = r#"^(\w+) hands (\w+) the part "([^"]+)" of "([^"]+)", done when "([^"]+)", serving "([^"]+)"$"#
)]
fn hands_a_part_serving(
    w: &mut Office,
    by: String,
    to: String,
    part: String,
    of: String,
    done: String,
    serves: String,
) {
    w.hands_a_part(&by, &to, &part, &of, &done, Some(&serves));
}

#[when(regex = r#"^(\w+) asks the owner to approve "([^"]+)" because "([^"]+)"$"#)]
fn asks_for_a_yes(w: &mut Office, name: String, tool: String, why: String) {
    w.asks_for_a_yes(&name, &tool, &why, None);
}

#[when(
    regex = r#"^(\w+) asks the owner to approve "([^"]+)" because "([^"]+)", serving "([^"]+)"$"#
)]
fn asks_for_a_yes_serving(w: &mut Office, name: String, tool: String, why: String, serves: String) {
    w.asks_for_a_yes(&name, &tool, &why, Some(&serves));
}

#[then(regex = r#"^the owner's desk holds an approval of "([^"]+)", serving "([^"]+)"$"#)]
fn the_desk_holds_an_approval(w: &mut Office, tool: String, criterion: String) {
    let snapshot = w.hq.snapshot().expect("the snapshot is built");
    let card = snapshot["desk"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|d| {
            d["kind"] == "approval" && d["text"].as_str().is_some_and(|text| text.contains(&tool))
        })
        .unwrap_or_else(|| panic!("no approval of {tool} on the desk: {}", snapshot["desk"]));
    assert_eq!(card["serves"]["text"], json!(criterion), "{card}");
}
