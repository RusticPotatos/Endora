//! The steps of `features/a-model-change-keeps-a-member-able-to-start.feature` (ADR 0117,
//! amended) and `features/a-loop-is-offered-what-goes-with-its-tools.feature` (ADR 0113,
//! amended): the owner's setup of a member and a loop never strands a turn.

use cucumber::{given, then, when};

use super::Office;

/// What the owner reads as a member's runtime, by its label.
fn runtime_label(w: &Office, name: &str) -> &'static str {
    w.hq.runtime_of(w.member(name))
        .expect("the store answers")
        .label()
}

#[given(expr = "{word} runs on Claude Code")]
fn runs_on_claude_code(w: &mut Office, name: String) {
    let id = w.member(&name);
    w.hq.set_runtime(id, "claude-code")
        .expect("a developer on Claude may run on Claude Code");
}

#[given(expr = "{word}'s model is {string}")]
fn model_is(w: &mut Office, name: String, model: String) {
    let id = w.member(&name);
    w.hq.set_model(id, &model).expect("a model Endora has");
}

#[when(expr = "the owner sets {word}'s model to {string}")]
fn owner_sets_model(w: &mut Office, name: String, model: String) {
    let id = w.member(&name);
    let set = w.hq.set_model(id, &model);
    w.answer = w.note(set).and_then(|set| set.said);
}

#[when(expr = "the owner sets {word}'s runtime to {string}")]
fn owner_sets_runtime(w: &mut Office, name: String, runtime: String) {
    let id = w.member(&name);
    let set = w.hq.set_runtime(id, &runtime);
    w.note(set);
}

#[then(expr = "the owner is told {string}")]
fn owner_is_told(w: &mut Office, said: String) {
    assert_eq!(w.refused, None, "it was refused");
    assert_eq!(w.answer.as_deref(), Some(said.as_str()));
}

#[then(expr = "the owner is told nothing about {word}'s runtime")]
fn owner_told_nothing(w: &mut Office, _name: String) {
    assert_eq!(w.refused, None, "it was refused");
    assert_eq!(w.answer, None);
}

#[then(regex = r"^(\w+) runs on (Endora's worker|Claude Code)$")]
fn runs_on(w: &mut Office, name: String, label: String) {
    assert_eq!(runtime_label(w, &name), label);
}

#[then(regex = r"^(\w+)'s turn is handed to the runner on (Endora's worker|Claude Code)$")]
fn turn_handed_on(w: &mut Office, name: String, label: String) {
    let run = w.turns.get(&name).expect("the turn started").run;
    assert_eq!(
        w.hq.runtime_of_turn(run)
            .expect("the store answers")
            .label(),
        label
    );
}

// --- A loop's tools -----------------------------------------------------------------------

/// A list as the owner writes it: names split at commas.
fn names(list: &str) -> Vec<String> {
    list.split(',')
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .collect()
}

#[given(expr = "the project {string} gives its work {string}")]
fn project_gives(w: &mut Office, title: String, tools: String) {
    let id = w.project(&title).id;
    w.hq.set_project_tools(id, Some(names(&tools)))
        .expect("the owner sets a project's tools");
}

/// What the node is told the turn may use (`only`); `None` when HQ does not narrow it.
fn node_only(w: &Office, name: &str) -> Vec<String> {
    let run = w.turns.get(name).expect("the turn started").run;
    w.hq.lab_tools_of(run)
        .expect("the store answers")
        .expect("the project narrows the turn's tools")
}

#[then(expr = "{word}'s turn may use the node's {string}")]
fn may_use(w: &mut Office, name: String, tools: String) {
    let only = node_only(w, &name);
    let refused = {
        let run = w.turns.get(&name).expect("the turn started").run;
        w.hq.lab_tools_refused(run).expect("the store answers")
    };
    for tool in names(&tools) {
        assert!(only.contains(&tool), "{tool} is not offered: {only:?}");
        assert!(!refused.contains(&tool), "{tool} is refused: {refused:?}");
    }
}

#[then(expr = "{word}'s turn may not use the node's {string}")]
fn may_not_use(w: &mut Office, name: String, tools: String) {
    let only = node_only(w, &name);
    for tool in names(&tools) {
        assert!(!only.contains(&tool), "{tool} is offered: {only:?}");
    }
}

#[then(expr = "{word}'s turn is refused the node's {string}")]
fn is_refused(w: &mut Office, name: String, tools: String) {
    let run = w.turns.get(&name).expect("the turn started").run;
    let refused = w.hq.lab_tools_refused(run).expect("the store answers");
    for tool in names(&tools) {
        assert!(
            refused.contains(&tool),
            "{tool} is not refused: {refused:?}"
        );
    }
}
