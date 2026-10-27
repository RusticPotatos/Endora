//! The steps of `features/the-world.feature` (ADR 0122): the house log and the lessons moved
//! into the team's world, canon the owner's alone, trust read from dated evidence, and each
//! turn's situation, budget and handover.

use cucumber::{then, when};
use serde_json::json;

use super::Office;

impl Office {
    fn briefed(&self, member: &str) -> endora_team::domain::situations::Briefed {
        let run = self
            .turns
            .get(member)
            .unwrap_or_else(|| panic!("{member}'s turn has not started"))
            .run;
        self.hq
            .briefed(run)
            .expect("the store answers")
            .expect("the turn's briefing is recorded")
    }
}

// --- Moving the old record in ----------------------------------------------------------------

// --- Stories ----------------------------------------------------------------------------------

#[when(
    expr = "{word} retells {string}: when {string}, tempting {string}, instead {string}, because {string}"
)]
fn retells(
    w: &mut Office,
    name: String,
    handle: String,
    moment: String,
    reflex: String,
    instead: String,
    because: String,
) {
    let task = w.turn_of(&name).task.expect("a turn on a task");
    let told = w.use_tool(
        &name,
        "tell_story",
        &json!({ "handle": handle, "moment": moment, "reflex": reflex, "instead": instead,
                 "because": because, "evidence": [format!("task {}", task.0)] }),
    );
    assert!(told.is_some(), "refused: {:?}", w.refused);
}

// --- The house page and canon -----------------------------------------------------------------

// --- Situations, budgets and handovers ----------------------------------------------------------

#[then(regex = r"^(\w+)'s briefing keeps to its budget of (\d+) tokens$")]
fn keeps_budget(w: &mut Office, name: String, budget: usize) {
    let briefed = w.briefed(&name);
    assert_eq!(briefed.budget, budget);
    assert!(briefed.tokens <= budget, "{briefed:?}");
    assert!(!briefed.over_budget);
}

#[when(
    expr = "{word} hands {string} to {word}, saying done {string}, not done {string}, next {string}, watch out {string}"
)]
#[allow(clippy::too_many_arguments)]
fn hands_with(
    w: &mut Office,
    name: String,
    title: String,
    to: String,
    done: String,
    not_done: String,
    next: String,
    watch_out: String,
) {
    let task = w.task(&title);
    let handed = w.use_tool(
        &name,
        "hand_to",
        &json!({ "task": task.0, "to": to, "why": "Zed is busy",
                 "handover": { "done": done, "not_done": not_done, "next": next,
                               "watch_out": watch_out } }),
    );
    assert!(handed.is_some(), "refused: {:?}", w.refused);
}
