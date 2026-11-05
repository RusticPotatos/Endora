//! The team protocol's tool for the team's world (ADR 0122): reading it, the house and lab pages
//! and the map; and the handover note that handing work on needs. Curating the world went with
//! the company (ADR 0142).

use serde_json::{Value, json};

use super::engine::Engine;
use super::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::Member;
use crate::domain::world::HandoverNote;

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// The `handover` parameter: what is done, what is not, what comes next, what to watch out for.
#[must_use]
pub fn handover_schema() -> Value {
    let part = |what: &str| json!({ "type": "string", "description": what });
    json!({
        "type": "object",
        "description": "The handover note, required when work is handed on or dropped: each part said, all of it at most 1,200 characters. The next member's first turn on the task starts with it.",
        "properties": {
            "done": part("what is done"),
            "not_done": part("what is not done yet"),
            "next": part("what comes next"),
            "watch_out": part("what to watch out for"),
        },
        "required": ["done", "not_done", "next", "watch_out"],
    })
}

/// The `handover` argument, if one was given.
///
/// # Errors
/// What the note refuses: a part left empty, or too long.
pub fn handover_arg(args: &Value) -> Result<Option<HandoverNote>, String> {
    let note = &args["handover"];
    if note.is_null() {
        return Ok(None);
    }
    let part = |key: &str| note[key].as_str().unwrap_or_default();
    HandoverNote::new(
        part("done"),
        part("not_done"),
        part("next"),
        part("watch_out"),
    )
    .map(Some)
    .map_err(|e| e.to_string())
}

/// The tool every member reads the team's world with.
#[must_use]
pub fn read_world_tool() -> Value {
    let address = json!({ "type": "string", "description": "world:house, world:lab, world:team, world:map, world:project/<n>, world:goal/<n>, any page with #<section>" });
    tool(
        "read_world",
        "Read the team's world, by address: the house (world:house), how the lab is built (world:lab), the team (world:team), where things are (world:map), a project's or a goal's page, or one of a page's sections (world:house#backups). Your briefing tells only what your task names; read what it left out here.",
        json!({ "address": address }),
        &["address"],
    )
}

/// Runs the world's tool, if `tool` is it; `None` for any other tool.
pub fn call<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    _member: &Member,
    tool: &str,
    args: &Value,
) -> Option<Result<String, String>> {
    if tool != "read_world" {
        return None;
    }
    let address = args["address"].as_str().unwrap_or_default();
    Some(
        engine
            .read_world_in(run.id, address)
            .map_err(|e| e.to_string()),
    )
}
