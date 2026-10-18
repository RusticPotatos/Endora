//! The team protocol's tools for asking the past (ADR 0129): `ask_record`, which searches what
//! the team said and wakes no one; `consult`, a question to a member that takes one short turn of
//! theirs; and `answer_consult`, with which a consulted member answers. The engine's rules decide
//! every call.

use serde_json::{Value, json};

use super::engine::Engine;
use super::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::consult;
use crate::domain::record;
use crate::domain::{Member, TaskId};

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// The history's tools every member is offered (the CEO's consult is held to leads by the
/// engine, not by what it is offered).
#[must_use]
pub fn tools() -> Vec<Value> {
    vec![
        tool(
            "ask_record",
            "Search what the team said, for why something was decided or what was found: posts and last words, handover notes, world pages and their timelines, stories, notebooks and the conversations of turns, from the last 90 days. At most five excerpts, newest first, each with what to cite. It wakes no one and costs nothing, so ask it before you consult anyone. What comes back is testimony: cite it for what was said, never as a reading of the lab now.",
            json!({
                "question": { "type": "string", "maxLength": record::QUESTION_MOST, "description": "what you want to know, with its distinctive words (names, things, what was decided)" },
                "member": { "type": "string", "description": "only what this member said or wrote" },
                "task": { "type": "integer", "description": "only what was about this task" },
                "project": { "type": "integer", "description": "only what was about this project's tasks" },
            }),
            &["question"],
        ),
        tool(
            "consult",
            "Ask another member a question directly: they take one short turn, from what they know and the record, and their answer is posted in your task's thread as \"<name> answers (consulted): …\", which wakes you. At most two consults in a turn and six in a day, and the answer is paid from your share; the CEO consults leads only. A member who is paused or on furlough is never woken: the record answers for them. Try ask_record first. The owner is asked with ask_owner, not this.",
            json!({
                "member": { "type": "string", "description": "the member's name" },
                "question": { "type": "string", "maxLength": record::QUESTION_MOST },
                "task": { "type": "integer", "description": "the task the question is about (what they wrote on it is read to them); the answer goes in your own task's thread, or this one when your turn is about none" },
            }),
            &["member", "question"],
        ),
        tool(
            "answer_consult",
            "Answer the colleague who consulted you (your turn says so): in a few sentences, from what you know and the record; say so if you do not know. It is posted in their thread and wakes them. Then stop.",
            json!({ "answer": { "type": "string", "maxLength": consult::ANSWER_MOST } }),
            &["answer"],
        ),
    ]
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().unwrap_or_default()
}

/// Runs one of the history's tools, if `tool` is one; `None` for any other tool.
pub fn call<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    _member: &Member,
    tool: &str,
    args: &Value,
) -> Option<Result<String, String>> {
    let said = |e: super::engine::EngineError| e.to_string();
    Some(match tool {
        "ask_record" => engine
            .ask_record_in(
                run.id,
                text(args, "question"),
                args["member"].as_str(),
                args["task"].as_i64().map(TaskId),
                args["project"].as_i64(),
            )
            .map_err(said),
        "consult" => engine
            .consult_in(
                run.id,
                text(args, "member"),
                text(args, "question"),
                args["task"].as_i64().map(TaskId),
            )
            .map_err(said),
        "answer_consult" => engine
            .answer_consult_in(run.id, text(args, "answer"))
            .map(|()| "Your answer is posted and your colleague is woken. Stop here.".to_owned())
            .map_err(said),
        _ => return None,
    })
}
