//! Explicit historical search and canonical colleague assistance (ADRs 0129/0138).
//! Historical testimony never substitutes automatically for a live peer answer.

use serde_json::{Value, json};

use super::engine::Engine;
use super::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::record;
use crate::domain::{Member, TaskId};

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// Explicit historical search and the canonical colleague-assistance adapter.
#[must_use]
pub fn tools() -> Vec<Value> {
    vec![tool(
        "ask_record",
        "Search what the team said, for why something was decided or what was found: posts and last words, handover notes, world pages and their timelines, stories, notebooks and the conversations of turns, from the last 90 days. At most five excerpts, newest first, each with what to cite. It wakes no one and costs nothing. It finds what was said before; a colleague can answer what the record cannot. What comes back is testimony: cite it for what was said, never as a reading of the lab now.",
        json!({
            "question": { "type": "string", "maxLength": record::QUESTION_MOST, "description": "what you want to know, with its distinctive words (names, things, what was decided)" },
            "member": { "type": "string", "description": "only what this member said or wrote" },
            "task": { "type": "integer", "description": "only what was about this task" },
            "project": { "type": "integer", "description": "only what was about this project's tasks" },
        }),
        &["question"],
    )]
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
        _ => return None,
    })
}
