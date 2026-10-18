//! The team protocol's tools for the team's world (ADR 0122): reading it, writing the house
//! page, confirming and retiring what a member stewards, proposing canon, and telling stories;
//! and the handover note that handing work on needs. The engine's rules decide every call.

use serde_json::{Value, json};

use super::engine::{Engine, StoryTelling, WorldWrite};
use super::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::situations::Situation;
use crate::domain::world::{self, HandoverNote, Stage, WorldRefusal};
use crate::domain::{Member, Ref, Role};

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

fn evidence(what: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "description": format!("{what} Ids from HQ's record: receipts (r12), tasks (task 12), events (event 340)."),
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

/// The `handover` argument, which must be there.
///
/// # Errors
/// [`WorldRefusal::HandoverNeeded`] when it is not; what the note refuses.
pub fn handover_needed(args: &Value) -> Result<HandoverNote, String> {
    handover_arg(args)?.ok_or_else(|| WorldRefusal::HandoverNeeded.to_string())
}

/// The world's tools a member of `role` is offered: everyone reads it and tells stories; the
/// CEO writes the house page; the CEO and the leads confirm and retire what they steward and
/// propose canon.
#[must_use]
pub fn tools(role: Role) -> Vec<Value> {
    let address = json!({ "type": "string", "description": "world:house, world:house#<section>, world:lab, world:team, or story:<handle>" });
    let part = |what: &str| json!({ "type": "string", "maxLength": world::STORY_PART_MOST, "description": what });
    let mut all = vec![
        tool(
            "read_world",
            "Read the team's world: the house page (world:house) or one of its sections (world:house#backups), how the lab is built (world:lab), the team (world:team), or a story (story:<handle>). Each piece says its stage: an observation, an insight, canon (the owner's), or unconfirmed since a date. Your briefing names what it left out; read it here.",
            json!({ "address": address }),
            &["address"],
        ),
        tool(
            "tell_story",
            "Tell a lesson as a story, in your scope (the CEO the house's, a lead its area's, the tester her lead's area's, anyone else their own), so the next member recognises the moment: when it matters, the tempting move, what to do instead, and why, on evidence; under a handle of two or three words (closed-too-soon) that a post can cite in its place. A lesson marked to retell is retold under its own handle. An insight is its scope's steward's, on evidence from two turns or a reading the node witnessed this turn.",
            json!({
                "handle": { "type": "string", "description": "two or three short words, like closed-too-soon" },
                "moment": part("when this matters"),
                "reflex": part("the tempting wrong move"),
                "instead": part("what to do instead"),
                "because": part("why: the failure or the evidence"),
                "evidence": evidence("What it rests on: at least one."),
                "stage": { "type": "string", "enum": ["observation", "insight"] },
                "warrant": { "type": "string", "maxLength": world::WARRANT_MOST, "description": "for an insight: why it is believed, in one line" },
                "taught": { "type": "array", "items": { "type": "string", "enum": ["fresh", "continuing", "after_gap", "handover", "recalled"] }, "description": "when a briefing tells it whole (default: fresh, after_gap, handover, recalled)" },
            }),
            &[
                "handle", "moment", "reflex", "instead", "because", "evidence",
            ],
        ),
    ];
    if matches!(role, Role::Ceo | Role::Lead) {
        all.push(tool(
            "verify_world",
            "Confirm a section or story you steward on fresh evidence (a reading the node witnessed this turn, or references from two turns): it is trusted again from now. An insight or canon not confirmed within its horizon (the house page 90 days, an area's stories 30) reads as unconfirmed.",
            json!({ "address": address, "evidence": evidence("The fresh evidence.") }),
            &["address", "evidence"],
        ));
        all.push(tool(
            "retire_world",
            "Retire a section or story you steward that is no longer true or needed, saying why; or supersede it, naming the address of what replaces it. It still resolves, and says where to go. Canon is the owner's to change.",
            json!({ "address": address, "why": { "type": "string", "maxLength": world::WHY_MOST }, "superseded_by": address }),
            &["address", "why"],
        ));
        all.push(tool(
            "propose_canon",
            "Propose a trusted insight (a section or a story) for canon, with its warrant: it goes on the owner's desk, who alone makes something canon.",
            json!({ "address": address, "warrant": { "type": "string", "maxLength": world::WARRANT_MOST, "description": "why it should be what the house treats as established" } }),
            &["address", "warrant"],
        ));
    }
    if role == Role::Ceo {
        all.push(tool(
            "write_world",
            "Write or revise a section of the house page, yours to keep: a heading (its address comes from it once, and never changes), a one-line headline the briefings show, a short body, at a stage your evidence warrants: an observation on one reference, an insight on references from two turns or a reading the node witnessed this turn, with a warrant. Only the owner makes a section canon, and changes it once it is.",
            json!({
                "page": { "type": "string", "enum": ["house"] },
                "anchor": { "type": "string", "description": "the section to revise, as in world:house#<anchor>; a new one takes its anchor from its heading" },
                "heading": { "type": "string", "maxLength": world::HEADING_MOST },
                "headline": { "type": "string", "maxLength": world::HEADLINE_MOST },
                "body": { "type": "string", "maxLength": world::BODY_MOST, "description": "Markdown, without # or ## headings" },
                "stage": { "type": "string", "enum": ["observation", "insight"] },
                "warrant": { "type": "string", "maxLength": world::WARRANT_MOST },
                "evidence": evidence("What it rests on: at least one."),
            }),
            &["heading", "headline", "stage", "evidence"],
        ));
    }
    all
}

fn refs(args: &Value) -> Result<Vec<Ref>, String> {
    args["evidence"]
        .as_array()
        .map(|all| {
            all.iter()
                .map(|v| {
                    let said = v.as_str().unwrap_or_default();
                    Ref::parse(said).ok_or_else(|| {
                        format!("`{said}` is not evidence: cite a receipt as r12, a task as task 12, an event as event 340")
                    })
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().unwrap_or_default()
}

fn stage(args: &Value) -> Result<Stage, String> {
    match args["stage"].as_str().map(str::trim) {
        None | Some("") => Ok(Stage::Observation),
        Some(name) => Stage::from_name(name)
            .ok_or_else(|| WorldRefusal::NotAWritableStage(name.to_owned()).to_string()),
    }
}

/// Runs one of the world's tools, if `tool` is one; `None` for any other tool.
pub fn call<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    _member: &Member,
    tool: &str,
    args: &Value,
) -> Option<Result<String, String>> {
    let said = |e: super::engine::EngineError| e.to_string();
    let answer = match tool {
        "read_world" => engine
            .read_world_in(run.id, text(args, "address"))
            .map_err(said),
        "write_world" => (|| {
            let write = WorldWrite {
                page: args["page"].as_str().unwrap_or("house").to_owned(),
                anchor: args["anchor"].as_str().map(str::to_owned),
                heading: text(args, "heading").to_owned(),
                headline: text(args, "headline").to_owned(),
                body: text(args, "body").to_owned(),
                stage: stage(args)?,
                warrant: text(args, "warrant").to_owned(),
                evidence: refs(args)?,
            };
            let address = engine.write_world_in(run.id, &write).map_err(said)?;
            Ok(format!(
                "Written: {address}. Only the owner makes it canon."
            ))
        })(),
        "verify_world" => (|| {
            let address = text(args, "address");
            engine
                .verify_world_in(run.id, address, &refs(args)?)
                .map_err(said)?;
            Ok(format!("Confirmed: {address} is trusted again from now."))
        })(),
        "retire_world" => {
            let address = text(args, "address");
            engine
                .retire_world_in(
                    run.id,
                    address,
                    text(args, "why"),
                    args["superseded_by"].as_str(),
                )
                .map(|()| format!("{address} is retired; it still resolves and says why."))
                .map_err(said)
        }
        "propose_canon" => engine
            .propose_canon_in(run.id, text(args, "address"), text(args, "warrant"))
            .map(|_| "It is on the owner's desk: only the owner makes something canon.".to_owned())
            .map_err(said),
        "tell_story" => (|| {
            let taught: Vec<Situation> = args["taught"]
                .as_array()
                .map(|all| {
                    all.iter()
                        .filter_map(|s| s.as_str().and_then(Situation::from_name))
                        .collect()
                })
                .unwrap_or_default();
            let told = StoryTelling {
                handle: text(args, "handle").to_owned(),
                moment: text(args, "moment").to_owned(),
                reflex: text(args, "reflex").to_owned(),
                instead: text(args, "instead").to_owned(),
                because: text(args, "because").to_owned(),
                evidence: refs(args)?,
                stage: stage(args)?,
                warrant: text(args, "warrant").to_owned(),
                taught,
            };
            let address = engine.tell_story_in(run.id, &told).map_err(said)?;
            Ok(format!(
                "Told: {address}. Cite it by its handle in a post or a send-back instead of retelling it."
            ))
        })(),
        _ => return None,
    };
    Some(answer)
}
