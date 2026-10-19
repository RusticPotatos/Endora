//! The team protocol (spec section 6): the tools every worker uses, whatever its runtime,
//! served by HQ over MCP. A call is made by a turn, known by its token; the member is that
//! turn's, so nobody acts as anyone else, and the engine's rules decide every call (claiming,
//! ranks, only the gate sets `done`). What touches a real system is never here: it is a
//! request the gateway decides.

use serde_json::{Value, json};

use super::engine::{Engine, EngineError, Proposal};
use super::ports::{Clock, NewTask, Run, TeamStore, WorkerAdapter};
use crate::domain::{
    Criterion, GoalId, Member, Micros, Priority, ProjectId, Ref, Refusal, Role, RunStatus, TaskId,
    TaskStatus,
};

/// What counts as evidence, as every tool that takes it says (ADR 0114).
const WHAT_COUNTS: &str = "Evidence is ids from HQ's record: receipts (r12) of your Endora \
    tool calls, which each tool result names, task numbers (task 12) and event ids (event \
    340). A model's word is not evidence; a reading is.";

/// An `evidence` parameter: one list of ids per criterion, in order (or one id each).
fn per_criterion(what: &str) -> Value {
    json!({
        "type": "array",
        "items": { "anyOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "string" } },
        ] },
        "description": format!("{what} One list per criterion of the task, in order. {WHAT_COUNTS}"),
    })
}

/// An `evidence` parameter that is one list of ids.
fn ids(what: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "description": format!("{what} {WHAT_COUNTS}"),
    })
}

/// One piece of evidence as the model gave it.
fn one_ref(v: &Value) -> Result<Ref, String> {
    let said = v.as_str().unwrap_or_default();
    Ref::parse(said).ok_or_else(|| {
        format!(
            "`{}` is not evidence: cite a receipt as r12, a task as task 12, an event as event 340",
            crate::domain::evidence::short(&v.to_string(), 40)
        )
    })
}

/// The `evidence` argument, one list per criterion: each item an id or a list of ids.
fn evidence_lists(args: &Value) -> Result<Vec<Vec<Ref>>, String> {
    let Some(items) = args["evidence"].as_array() else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| match item {
            Value::Array(list) => list.iter().map(one_ref).collect(),
            other => one_ref(other).map(|r| vec![r]),
        })
        .collect()
}

/// The `evidence` argument as one list.
fn evidence_flat(args: &Value) -> Result<Vec<Ref>, String> {
    Ok(evidence_lists(args)?.concat())
}

/// The `criteria` argument: each `{ text, live }`, or a line of text.
fn criteria_arg(args: &Value) -> Vec<Criterion> {
    args["criteria"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|c| {
                    let (text, live) = match c {
                        Value::String(s) => (s.as_str(), false),
                        other => (
                            other["text"].as_str()?,
                            other["live"].as_bool() == Some(true),
                        ),
                    };
                    if text.trim().is_empty() {
                        return None;
                    }
                    let made = Criterion::new(text, live);
                    // A wait, in minutes from the work's last change (ADR 0120).
                    Some(
                        match c["after"].as_u64().and_then(|m| u32::try_from(m).ok()) {
                            Some(minutes) => made.after(minutes),
                            None => made,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The tool a manager hands a subordinate's task on with (ADR 0114).
fn hand_to_tool() -> Value {
    json!({
        "name": "hand_to",
        "description": "Hand a task held by someone below you in the chain (who reports to you, or to someone who does) to another member: yourself, or one below you. Say why, and give the handover note (done, not done, next, watch out): the why goes in the thread, the note starts their first turn on it, and they are woken.",
        "inputSchema": { "type": "object", "properties": {
            "task": { "type": "integer", "description": "the task's number" },
            "to": { "type": "string", "description": "the member's name" },
            "why": { "type": "string" },
            "handover": super::protocol_world::handover_schema(),
        }, "required": ["task", "to", "why", "handover"] },
    })
}

/// The tool a lead and the CEO accept or send back tested work with (ADR 0114).
fn accept_tool() -> Value {
    json!({
        "name": "accept",
        "description": "Accept work in review against its criteria (it is done), or send it back to its member with exactly what does not hold. Only once the tester has passed it (when there is a tester who did not do it), and never your own work. Accepting cites evidence per criterion: the tester's readings count as fresh.",
        "inputSchema": { "type": "object", "properties": {
            "task": { "type": "integer", "description": "the task's number" },
            "verdict": { "type": "string", "enum": ["accept", "send_back"] },
            "notes": { "type": "string" },
            "evidence": per_criterion("For accept: what shows each criterion holds."),
        }, "required": ["task", "verdict", "notes"] },
    })
}

/// The tool a lead and the CEO propose a project with (ADR 0113).
fn propose_project_tool() -> Value {
    let ids = |what: &str| json!({ "type": "array", "items": { "type": "integer" }, "description": what });
    json!({
        "name": "propose_project",
        "description": "Propose a project: a piece of one goal with its own lead, for work that keeps coming back in your area. Cite its evidence from HQ's record: task numbers and alarm event ids (at least one, each checked). It waits for the CEO, who decides it (yes, no or not now). A monthly cap, if you ask one, is the owner's to grant.",
        "inputSchema": { "type": "object", "properties": {
            "goal": { "type": "integer", "description": "the goal it serves (default: your task's)" },
            "title": { "type": "string" },
            "reason": { "type": "string", "description": "why, from what the team found" },
            "lead": { "type": "string", "description": "the member who leads it, a lead or the CEO (default: you)" },
            "cap": { "type": "number", "description": "a monthly cap in dollars, if one is wanted" },
            "tasks": ids("task numbers it rests on"),
            "events": ids("alarm event ids it rests on"),
        }, "required": ["title", "reason"] },
    })
}

/// The tool every member reads a task's documents with (ADR 0115): a branch's diff, a
/// check's report.
fn read_document_tool() -> Value {
    json!({
        "name": "read_document",
        "description": "Read a document on your task (or one under or above it), as the thread names them (d12): a submitted branch's diff and stat, or a check's report. 16,000 characters a part.",
        "inputSchema": { "type": "object", "properties": {
            "document": { "type": "integer", "description": "the document's number, 12 for d12" },
            "part": { "type": "integer", "minimum": 1 },
        }, "required": ["document"] },
    })
}

/// The tool a lead or a developer asks for a kept branch to be checked again with (ADR 0115).
fn recheck_tool() -> Value {
    json!({
        "name": "recheck",
        "description": "Queue the lab's checks again on the newest branch your work has submitted (the node keeps it), without submitting it again and without a sandbox. Use it when a check failed for the check box's own reasons (a tool missing, a timeout, a fix to the check image since) or to see it pass again, instead of recreating the branch. One at a time: refused while a check of that branch is waiting or running. The result comes back as a receipt in the task's thread, judged against develop as the first check was.",
        "inputSchema": { "type": "object", "properties": {
            "task": { "type": "integer", "description": "the task whose branch to check (default: your turn's task, and the work under it)" },
            "why": { "type": "string", "description": "why, in a sentence: it goes in the thread" },
        }, "required": ["why"] },
    })
}

/// The tool every member reads Endora's public library with (ADR 0111, 0116): the decision
/// records, the plans, the drafts the team proposes, and the team's standards.
fn read_doc_tool() -> Value {
    json!({
        "name": "read_doc",
        "description": "Read Endora's public library: the team's standards (docs/standards.md), a decision record or plan (docs/adr, docs/plans), or a draft the team proposes (docs/drafts). A folder alone lists it. 20,000 characters a part.",
        "inputSchema": { "type": "object", "properties": {
            "path": { "type": "string", "description": "e.g. docs/standards.md, docs/plans/endora-hq.md, or docs/adr to list the records" },
            "part": { "type": "integer", "minimum": 1 },
        }, "required": ["path"] },
    })
}

/// The tool a lead asks the owner to hire with (ADR 0115).
fn request_hire_tool() -> Value {
    json!({
        "name": "request_hire",
        "description": "Ask the owner to hire a member who will report to you: a role below yours, a brain that role may think with, why, and evidence from HQ's record. In the engineering wing the seats are fixed: one lead and at most five developers on Claude Haiku or the house's model; a request beyond them is refused with the count. The request waits on the owner's desk; approving hires. One request at a time.",
        "inputSchema": { "type": "object", "properties": {
            "name": { "type": "string", "description": "a name for the new member (optional)" },
            "role": { "type": "string", "enum": ["developer", "researcher", "reviewer"] },
            "brain": { "type": "string", "description": "local, claude-haiku-5-5 or claude-sonnet-5-5, as the role allows" },
            "why": { "type": "string" },
            "evidence": ids("What shows the need: at least one."),
        }, "required": ["role", "brain", "why", "evidence"] },
    })
}

/// The tool a lead asks for staff with when its project has a backlog (ADR 0118).
fn request_staff_tool() -> Value {
    json!({
        "name": "request_staff",
        "description": "Ask the owner for more hands on one of your projects, when its work is piling up: the project, why, and the project's open tasks as evidence (task numbers; at least twice the developers the project has at work, each checked to be open and in the project). A developer on furlough is offered back first; with none, it becomes a hire request under the same seats. The owner approves either; the member reports to you. One request at a time.",
        "inputSchema": { "type": "object", "properties": {
            "project": { "type": "integer", "description": "the project's number" },
            "why": { "type": "string" },
            "evidence": ids("The project's open tasks that wait for hands, as task numbers."),
        }, "required": ["project", "why", "evidence"] },
    })
}

/// The tool every member decorates and plans the office with, together (ADR 0119).
fn office_tool() -> Value {
    let kinds: Vec<&str> = crate::domain::office::CATALOG
        .iter()
        .map(|s| s.name)
        .collect();
    json!({
        "name": "office",
        "description": format!("The office is yours to shape, with the team. propose: one to {} changes (place a thing, move one by its number, remove one) and why; it is posted for the team, your yes is counted, and it carries on a second yes, or after six hours if nobody objects, and is dropped if more say no than yes. react: vote yes or no, once, on an open proposal. look: the things on the floor with their numbers, and the open proposals. Kinds: {}. The rules refuse a change off the floor or on a wall, a door, the desks, a walking lane, a room or another thing; the floor holds {} things.",
            crate::domain::office::MOST_CHANGES, kinds.join(", "), crate::domain::office::MOST_ITEMS),
        "inputSchema": { "type": "object", "properties": {
            "action": { "type": "string", "enum": ["propose", "react", "look"] },
            "changes": { "type": "array", "maxItems": crate::domain::office::MOST_CHANGES, "items": { "type": "object", "properties": {
                "op": { "type": "string", "enum": ["place", "move", "remove"] },
                "kind": { "type": "string", "enum": kinds, "description": "for place" },
                "item": { "type": "integer", "description": "for move and remove: the thing's number from look" },
                "x": { "type": "integer", "description": "the left tile, 0 to 31" },
                "y": { "type": "integer", "description": "the top tile, 0 to 19; wall things at y 1" },
            }, "required": ["op"] }, "description": "for propose" },
            "why": { "type": "string", "maxLength": crate::domain::office::WHY_MOST, "description": "for propose, and optionally react: why" },
            "proposal": { "type": "integer", "description": "for react: the proposal's number" },
            "vote": { "type": "string", "enum": ["yes", "no"], "description": "for react" },
        }, "required": ["action"] },
    })
}

/// One change as a member wrote it: `{ op, kind, item, x, y }`.
fn change_arg(v: &Value) -> Result<crate::domain::office::Change, String> {
    use crate::domain::office::{CATALOG, Change, Kind};
    let tile = |key: &str| {
        v[key].as_i64().ok_or_else(|| {
            "a place or a move gives `x` and `y`, the tile, as whole numbers".to_owned()
        })
    };
    let item = || {
        v["item"].as_i64().ok_or_else(|| {
            "a move or a remove gives `item`, the thing's number (see look)".to_owned()
        })
    };
    match v["op"].as_str().map(str::to_lowercase).as_deref() {
        Some("place") => {
            let said = v["kind"].as_str().unwrap_or_default();
            let kind = Kind::from_name(said).ok_or_else(|| {
                let kinds: Vec<&str> = CATALOG.iter().map(|s| s.name).collect();
                format!(
                    "there is no kind of thing called `{said}` to place: the kinds are {}",
                    kinds.join(", ")
                )
            })?;
            Ok(Change::Place {
                kind,
                x: tile("x")?,
                y: tile("y")?,
            })
        }
        Some("move") => Ok(Change::Move {
            item: item()?,
            x: tile("x")?,
            y: tile("y")?,
        }),
        Some("remove") => Ok(Change::Remove { item: item()? }),
        _ => Err("each change has `op`: place, move or remove".to_owned()),
    }
}

/// The `office` tool: propose, react or look.
fn office_call<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
    use crate::domain::office;
    match args["action"].as_str() {
        Some("propose") => {
            let changes: Vec<office::Change> = args["changes"]
                .as_array()
                .map(|all| all.iter().map(change_arg).collect::<Result<_, _>>())
                .unwrap_or_else(|| Ok(Vec::new()))?;
            let why = args["why"].as_str().unwrap_or("");
            let id = engine
                .propose_office_in(run.id, &changes, why)
                .map_err(|e| said(&e))?;
            let items = engine.store().office_items().map_err(|e| e.to_string())?;
            let words: Vec<String> = changes
                .iter()
                .map(|c| office::describe(&items, c))
                .collect();
            Ok(format!(
                "Proposal {id} is made and posted for the team: {}. Your yes is counted. It carries \
                 when one more member says yes, or after six hours if nobody objects; the owner \
                 may veto it.",
                words.join("; ")
            ))
        }
        Some("react") => {
            let proposal = match &args["proposal"] {
                Value::Number(n) => n.as_i64(),
                Value::String(s) => s.trim().trim_start_matches(['p', 'P']).parse().ok(),
                _ => None,
            }
            .ok_or_else(|| "which proposal? Give its number as `proposal`".to_owned())?;
            let yes = match args["vote"].as_str() {
                Some("yes") => true,
                Some("no") => false,
                _ => return Err("`vote` is yes or no".to_owned()),
            };
            let why = args["why"].as_str().unwrap_or("");
            let standing = engine
                .react_office_in(run.id, proposal, yes, why)
                .map_err(|e| said(&e))?;
            Ok(match standing {
                super::engine::Standing::Open => format!(
                    "Your {} is counted on proposal {proposal}. It stays open.",
                    if yes { "yes" } else { "no" }
                ),
                super::engine::Standing::Applied => {
                    format!("Your yes carried proposal {proposal}: the office has changed.")
                }
                super::engine::Standing::Dropped(why) => {
                    format!("Proposal {proposal} is dropped: {why}.")
                }
            })
        }
        Some("look") => engine.look_office_in(run.id).map_err(|e| said(&e)),
        _ => Err("`action` is propose, react or look".to_owned()),
    }
}

/// A lead's verdict on a guard's finding in its area (ADR 0124).
fn judge_finding_tool() -> Value {
    json!({
        "name": "judge_finding",
        "description": "Say whether one of HQ's guard findings in your area was real, when HQ names one in a thread (\"finding N\"): real, false_positive (the guard fired on healthy work), not_enough (the record does not say), or resolved (real, and put right). Say whether the fix worked, if you know. Your verdicts are how HQ tunes its guards.",
        "inputSchema": { "type": "object", "properties": {
            "finding": { "type": "integer", "description": "the finding's number" },
            "verdict": { "type": "string", "enum": ["real", "false_positive", "not_enough", "resolved"] },
            "fix_worked": { "type": "boolean", "description": "whether what was done about it worked" },
        }, "required": ["finding", "verdict"] },
    })
}

/// What a downtime turn may not do: it makes no work.
const MAKES_WORK: [&str; 6] = [
    "create_task",
    "hand_to",
    "propose_project",
    "propose_goal",
    "request_hire",
    "request_staff",
];

/// The tools a member of `role` is offered, as MCP lists them: name, what it does, and its
/// input. Only the reviewer is offered `review`. The CEO (ADR 0111, 0120) sets direction:
/// it directs the leads (`direct`), decides their proposals, puts decisions to the board with
/// `ask_owner`, keeps the house log and reads; it neither proposes projects nor accepts work.
#[must_use]
pub fn tools(role: Role) -> Value {
    if role == Role::Ceo {
        return ceos_tools();
    }
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": properties, "required": required },
        })
    };
    let task = json!({ "type": "integer", "description": "the task's number" });
    let mut all = json!([
        tool(
            "my_turn",
            "What this turn is for: your role, the task and why it exists, what is new, and its thread. Call it first.",
            json!({}),
            &[]
        ),
        tool(
            "claim",
            "Take a task as yours: your turn's task or one under it. Refused if another member has it.",
            json!({ "task": task }),
            &["task"]
        ),
        tool(
            "post",
            "Write in a task's thread (yours, one under it, the one above it, or one where you were named): what you did, found or need. Mention a member by name to wake them. A finding cites its evidence; a post that cites none in a turn with no receipts is marked unsupported.",
            json!({ "task": task, "text": { "type": "string" }, "evidence": ids("What the post rests on, if anything.") }),
            &["task", "text"]
        ),
        tool(
            "set_status",
            "Move your task on (todo, doing, in_review, blocked), with a reason. Submitting it (in_review) cites evidence for each of its criteria, and a live criterion a reading from this turn; the tester then re-checks it and its lead accepts it. Done is set only that way, except a routine's check: close that as done, with its evidence, once you have looked. Done met its criteria unless you say why otherwise: superseded (name the successor task) or not_needed.",
            json!({ "task": task, "status": { "type": "string", "enum": ["todo", "doing", "in_review", "blocked", "done"] }, "reason": { "type": "string" }, "evidence": per_criterion("For in_review, or a check closed as done: what shows each criterion holds."), "why": { "type": "string", "enum": ["met_criteria", "superseded", "not_needed"], "description": "for done: why it is done" }, "successor": { "type": "integer", "description": "for superseded: the task that carries the work on" } }),
            &["task", "status", "reason"]
        ),
        tool(
            "create_task",
            "Create a sub-task of your task for another member (by name), with the tasks it waits on and its acceptance criteria: what done means, a short list, required for work handed to someone else. Mark live any criterion about the lab as it is now. Say which of your task's criteria it serves (serves: its number or its words); a task holds at most five open sub-tasks, and work goes at most three deep.",
            json!({ "title": { "type": "string" }, "description": { "type": "string" }, "assignee": { "type": "string", "description": "a member's name" }, "waits_on": { "type": "array", "items": { "type": "integer" } }, "serves": { "type": "string", "description": "which of your task's criteria this serves: its number (1, 2…) or its words" }, "criteria": { "type": "array", "items": { "type": "object", "properties": { "text": { "type": "string" }, "live": { "type": "boolean", "description": "about the lab now: shown only by a reading in the same turn" }, "after": { "type": "integer", "minimum": 1, "description": "minutes it waits from the work's last change before it can be shown, e.g. 60 for 'no new alarm since'" } }, "required": ["text"] } } }),
            &["title", "description"]
        ),
        tool(
            "ask_owner",
            "Ask a question or put a proposal about your task up the chain: to the lead (on a project's task, that project's lead), or the lead's to the CEO, in the task's thread (to the owner's desk only when the team has no CEO). The lead's to the CEO, and a tester's passing on the CEO's decision, cite evidence. Asking ends your turn: you are woken when they answer.",
            json!({ "text": { "type": "string" }, "evidence": ids("What it rests on.") }),
            &["text"]
        ),
        tool(
            "check_back",
            "Come back to your task later, once: after a fix, to confirm it held. You are woken on this task after the minutes you give (5 to 720), if it is still open.",
            json!({ "minutes": { "type": "integer", "minimum": 5, "maximum": 720 }, "why": { "type": "string", "description": "what you will look at when you come back" } }),
            &["minutes", "why"]
        ),
        tool(
            "note",
            "Add a note to YOUR notebook (nobody else's): something you learned that you or your next turn will need again, at most 300 characters, resting on evidence from HQ's record (at least one receipt, task or event; each is checked). Your notes are in your briefing, newest first. Keep them honest: a note you cannot back is a hunch.",
            json!({ "text": { "type": "string", "maxLength": crate::domain::notebook::NOTE_MOST }, "evidence": ids("What the note rests on: at least one.") }),
            &["text", "evidence"]
        ),
        tool(
            "forget",
            "Remove one of YOUR notes that is wrong or no longer true, saying why. Forgetting counts as tidying your notebook.",
            json!({ "note": { "type": "integer", "description": "the note's number, 12 for n12" }, "reason": { "type": "string" } }),
            &["note", "reason"]
        ),
        tool(
            "read_notebook",
            "Read all of YOUR notes, newest first, with the evidence each rests on (the briefing shows only the newest dozen).",
            json!({}),
            &[]
        ),
        tool(
            "notebook_checked",
            "Say you have looked over YOUR notebook and nothing needs changing. It counts as tidying it, which you are asked to do about once a week.",
            json!({}),
            &[]
        ),
        tool(
            "huddle_view",
            "Give your view on stuck work, when a huddle asks you (your turn says so): three sentences at most, on evidence from HQ's record. One short turn: give it and stop; the area's lead then decides.",
            json!({ "task": task, "view": { "type": "string", "maxLength": crate::domain::ladder::VIEW_MOST }, "evidence": ids("What your view rests on: at least one.") }),
            &["task", "view", "evidence"]
        ),
        tool(
            "run_gate",
            "Run the task's tests and build on a runner and get the result.",
            json!({ "task": task }),
            &["task"]
        ),
        tool(
            "request_action",
            "Ask for a privileged action, such as opening a pull request. The gateway decides.",
            json!({ "action": { "type": "string" }, "detail": { "type": "string" } }),
            &["action", "detail"]
        ),
    ]);
    if role == Role::Reviewer {
        if let Some(list) = all.as_array_mut() {
            // The tester passes or sends back, and never makes work (ADR 0114): she is not
            // offered the tool the engine would refuse her anyway.
            list.retain(|t| t["name"] != "create_task");
            list.push(tool(
                "review",
                "You are the tester. Your verdict on work in review, after re-checking it yourself: take your own readings to confirm each piece of its evidence is real and still true. approve passes it on your evidence per criterion, with at least one reading of your own this turn (its lead then accepts it); send_back returns it to its owner with exactly what does not hold. Sending back is a finished test too. Never your own work. You pass or send back; you never create or assign work.",
                json!({ "task": task, "verdict": { "type": "string", "enum": ["approve", "send_back"] }, "notes": { "type": "string", "description": "what you checked, or what does not hold" }, "evidence": per_criterion("For approve: your readings and what else shows each criterion holds.") }),
                &["task", "verdict", "notes"],
            ));
        }
    }
    if role == Role::Lead {
        if let Some(list) = all.as_array_mut() {
            list.push(propose_project_tool());
            list.push(accept_tool());
            list.push(hand_to_tool());
            list.push(request_hire_tool());
            list.push(request_staff_tool());
            let report_part = |what: &str| json!({ "type": "string", "maxLength": crate::domain::areas::REPORT_PART_MOST, "description": what });
            list.push(tool(
                "report_area",
                "Report on your area to the CEO, in your \"State of my area\" turn: four short parts, any of them \"nothing\". It is kept as your area's report, which the CEO's daily review reads, and closes the report's task.",
                json!({
                    "moved": report_part("what moved, with task numbers"),
                    "stuck": report_part("what is stuck, and on what"),
                    "need": report_part("what you need, and from whom"),
                    "proposals": report_part("what you propose (make each with propose_project)"),
                }),
                &["moved", "stuck", "need", "proposals"],
            ));
            list.push(tool(
                "decide_stuck",
                "Decide stuck work in your area, when HQ asks you (your turn says it is stuck): proceed (say with what), drop, hand_to a member of your team (`to`), needs_ceo (money, a new goal, another area), or needs_board (the owner alone: money beyond the caps, a hire, the owner's repos). On evidence. Dropping it or handing it on needs the handover note; dropping it says why (obsolete, duplicate with duplicate_of, wont_do, blocked_forever) and names the successor task if one carries the work on. Only a board matter reaches the owner, through the CEO.",
                json!({
                    "task": { "type": "integer", "description": "the task's number" },
                    "decision": { "type": "string", "enum": ["proceed", "drop", "hand_to", "needs_ceo", "needs_board"] },
                    "detail": { "type": "string", "description": "what, and why, in a sentence or two" },
                    "to": { "type": "string", "description": "for hand_to: the member's name" },
                    "why": { "type": "string", "enum": ["obsolete", "duplicate", "wont_do", "blocked_forever"], "description": "for drop: why it is dropped" },
                    "duplicate_of": { "type": "integer", "description": "for a duplicate: the task it duplicates" },
                    "successor": { "type": "integer", "description": "for drop: the task that carries the work on, if one does" },
                    "evidence": ids("What the decision rests on: at least one."),
                    "handover": super::protocol_world::handover_schema(),
                }),
                &["task", "decision", "detail", "evidence"],
            ));
            list.push(judge_finding_tool());
        }
    }
    if matches!(role, Role::Lead | Role::Developer) {
        if let Some(list) = all.as_array_mut() {
            list.push(recheck_tool());
        }
    }
    if let Some(list) = all.as_array_mut() {
        list.push(read_document_tool());
        list.push(read_doc_tool());
        list.push(office_tool());
        list.push(super::chess_tool::tool());
        // The team's world (ADR 0122): a lesson is a story now, which tell_story keeps;
        // note_lesson is no longer offered.
        list.extend(super::protocol_world::tools(role));
        // Asking the past (ADR 0129).
        list.extend(super::protocol_history::tools());
    }
    all
}

/// The CEO's tools (ADR 0111).
fn ceos_tools() -> Value {
    let all = tools(Role::Lead);
    let keep = [
        "my_turn",
        "claim",
        "post",
        "set_status",
        "create_task",
        "check_back",
        "note",
        "forget",
        "read_notebook",
        "notebook_checked",
        "office",
        "judge_finding",
    ];
    let mut list: Vec<Value> = all
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| keep.iter().any(|k| t["name"] == *k))
        .cloned()
        .collect();
    // The CEO delegates to leads from its own work (ADR 0120); in a lead's area it directs.
    for listed in &mut list {
        if listed["name"] == "create_task" {
            listed["description"] = json!(
                "Hand a lead new work from one of your own tasks (your review, your planning, a project you lead), with its acceptance criteria. Only to a lead: the lead plans it and hands its parts on in its area. On a task in a lead's area, use direct instead."
            );
        }
    }
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": properties, "required": required },
        })
    };
    let part = |what: &str| json!({ "type": "string", "maxLength": crate::domain::chain::DECISION_PART_MOST, "description": what });
    list.push(tool(
        "ask_owner",
        "Put a decision to the owner, who is the board: only what they alone decide (money beyond the pool, new goals and priorities, hiring or letting go a member, what only they can know), on evidence. The reviewer checks it first. Asking ends your turn: you are woken when the board decides.",
        json!({
            "decision": part("what the board is to decide, as a question"),
            "recommendation": part("what you recommend, and the options"),
            "why": part("why, from what the team found"),
            "evidence": ids("What it rests on: at least one."),
        }),
        &["decision", "recommendation", "why", "evidence"],
    ));
    list.push(tool(
        "note_house",
        "Add one dated line to the house page's timeline (world:house), yours to keep: what you learned about the house or decided, at most 500 characters. What the house treats as true now goes in a section, with write_world.",
        json!({ "entry": { "type": "string", "maxLength": crate::domain::house::HOUSE_ENTRY_MOST } }),
        &["entry"],
    ));
    // The team's world (ADR 0122): the CEO keeps the house page.
    list.extend(super::protocol_world::tools(Role::Ceo));
    // Asking the past (ADR 0129): the CEO consults leads only, which the engine holds it to.
    list.extend(super::protocol_history::tools());
    list.push(tool(
        "direct",
        "Give the lead of a task's area a direction: what you want done, looked at or weighed, on evidence from HQ's record. It is posted in the task's thread and wakes the lead, who decides what to do with it. On a task in a lead's area you read and talk, and direct; you never claim, move, close, accept or send back its work.",
        json!({
            "task": { "type": "integer", "description": "the task's number" },
            "direction": { "type": "string", "maxLength": super::engine::DIRECTION_MOST },
            "evidence": ids("What it rests on: at least one."),
        }),
        &["task", "direction", "evidence"],
    ));
    let strategy_part = |what: &str| json!({ "type": "string", "maxLength": crate::domain::areas::STRATEGY_PART_MOST, "description": what });
    list.push(tool(
        "set_strategy",
        "Write a goal's strategy, which you own: give only the parts you change. purpose is the goal of the goal; success is what success looks like; measures are the named numbers you steer by, one a line, from those HQ computes (unplanned outages over 10 min, median alarm to closed, repeat alarms within 24 h, median branch submitted to push approved, branches passing the tester first time); bets are the active projects and why; not_now is what you chose not to do. On evidence from HQ's record.",
        json!({
            "goal": { "type": "integer", "description": "the goal's number" },
            "purpose": strategy_part("the goal of the goal"),
            "success": strategy_part("what success looks like"),
            "measures": strategy_part("the measures, one a line"),
            "bets": strategy_part("the current bets: the active projects, and why"),
            "not_now": strategy_part("what we chose not to do now"),
            "evidence": ids("What it rests on: at least one."),
        }),
        &["goal", "evidence"],
    ));
    list.push(tool(
        "decide_project",
        "Decide a project a lead proposed: accept it (it starts, and its lead is handed its first task) or send it back, saying why; your why is posted to whoever proposed it. A cap it asks, or one you give, goes to the owner with your why as the recommendation.",
        json!({
            "project": { "type": "integer", "description": "the project's number" },
            "verdict": { "type": "string", "enum": ["accept", "send_back"] },
            "why": { "type": "string" },
            "cap": { "type": "number", "description": "a monthly cap in dollars to recommend to the owner" },
            "evidence": ids("What your decision rests on: at least one."),
        }),
        &["project", "verdict", "why", "evidence"],
    ));
    list.push(tool(
        "propose_goal",
        "Propose a new goal to the owner, who is the board: what, why, and the evidence from HQ's record it rests on (task numbers, alarm event ids). It waits on the board's desk.",
        json!({
            "title": { "type": "string" },
            "reason": { "type": "string" },
            "tasks": { "type": "array", "items": { "type": "integer" } },
            "events": { "type": "array", "items": { "type": "integer" } },
        }),
        &["title", "reason"],
    ));
    let mut hand_to = hand_to_tool();
    hand_to["description"] = json!(
        "Hand a task of your own, or one held by someone below you, to a lead (never to anyone else: the lead hands it on in its area). Say why; it goes in the thread, and the lead is woken."
    );
    list.push(hand_to);
    list.push(read_document_tool());
    list.push(read_doc_tool());
    Value::Array(list)
}

/// The most a member writes in one post: a thread is read by every turn after it.
const LONGEST_POST: usize = 4000;

fn said(e: &EngineError) -> String {
    e.to_string()
}

fn task_arg(args: &Value) -> Result<TaskId, String> {
    args["task"]
        .as_i64()
        .map(TaskId)
        .ok_or_else(|| "which task? Give its number as `task`".to_owned())
}

/// The integers in an array argument, if any.
fn numbers(args: &Value, key: &str) -> Vec<i64> {
    args[key]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

/// A monthly cap in dollars, if one was given.
fn cap_arg(args: &Value) -> Result<Option<Micros>, String> {
    let said = match &args["cap"] {
        Value::Null => return Ok(None),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    };
    crate::domain::projects::read_dollars(&said)
        .map(Some)
        .ok_or_else(|| "`cap` is a monthly amount in dollars, like 10 or 12.50".to_owned())
}

fn member_named<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    name: &str,
) -> Result<Member, String> {
    engine
        .store()
        .members()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| format!("no member called {name}"))
}

/// Why work closes as `status`, from `why` (one of the set's names), with `duplicate_of` for a
/// duplicate and `successor` for what carries the work on (ADR 0124).
fn closure_arg(
    args: &Value,
    status: TaskStatus,
) -> Result<(crate::domain::closure::Closure, Option<TaskId>), String> {
    let why = args["why"].as_str().unwrap_or("");
    let duplicate_of = args["duplicate_of"].as_i64().map(TaskId);
    let closure = crate::domain::closure::Closure::read(status, why, duplicate_of)
        .map_err(|r| r.to_string())?;
    Ok((closure, args["successor"].as_i64().map(TaskId)))
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| format!("`{key}` is needed"))
}

/// Runs one tool for a turn. `Ok` is what the worker reads back; `Err` is a refusal it reads
/// as a tool error, said so that it can do something about it.
///
/// # Errors
/// What the rules refuse, an argument that is missing, or a tool not there yet.
pub fn call<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
    tool: &str,
    args: &Value,
) -> Result<String, String> {
    // A call that set out while the turn ran, and arrived after it ended, does nothing.
    let still = engine.store().run(run.id).map_err(|e| e.to_string())?;
    if still.is_none_or(|r| r.status != RunStatus::Running) {
        return Err(Refusal::TurnOver.to_string());
    }
    // Downtime makes no work (ADR 0119): the tools that would are refused in its turns.
    if MAKES_WORK.contains(&tool) && engine.in_downtime(run.id).map_err(|e| said(&e))? {
        return Err(Refusal::InDowntime.to_string());
    }
    // A consulted turn reads and answers, and has a few rounds to do it in (ADR 0129).
    if let Some(refusal) = engine
        .consulted_turn_refuses(run.id, tool)
        .map_err(|e| said(&e))?
    {
        return Err(refusal);
    }
    // The team's world (ADR 0122), and asking the past (ADR 0129).
    if let Some(answer) = super::protocol_world::call(engine, run, member, tool, args) {
        return answer;
    }
    if let Some(answer) = super::protocol_history::call(engine, run, member, tool, args) {
        return answer;
    }
    match tool {
        "office" => office_call(engine, run, args),
        "chess" => super::chess_tool::call(engine, run, args),
        "my_turn" => {
            let mut out = run.told.clone();
            if let Some(task) = run.task {
                let thread = engine.store().thread(task).map_err(|e| e.to_string())?;
                if !thread.is_empty() {
                    out.push_str("\n\nThe thread, newest last:\n");
                    for p in thread.iter().rev().take(12).rev() {
                        let line = Engine::<S, W, C>::post_line(&p.author, &p.text, p.unsupported);
                        out.push_str(&format!("- {line}\n"));
                    }
                }
            }
            Ok(out)
        }
        "claim" => {
            let task = task_arg(args)?;
            engine.claim_in(run.id, task).map_err(|e| said(&e))?;
            Ok(format!("Task {} is yours.", task.0))
        }
        "post" => {
            let task = task_arg(args)?;
            let text = text_arg(args, "text")?;
            if text.chars().count() > LONGEST_POST {
                return Err(format!(
                    "a post is at most {LONGEST_POST} characters: say it shorter, or in parts"
                ));
            }
            // The worker's own last word for the model is marked so: posted, not progress.
            if args["last_word"].as_bool() == Some(true) {
                engine
                    .last_word_in(run.id, task, text)
                    .map_err(|e| said(&e))?;
                return Ok("Posted.".to_owned());
            }
            let cited = evidence_flat(args)?;
            engine
                .post_citing(run.id, task, text, &cited)
                .map_err(|e| said(&e))?;
            Ok("Posted.".to_owned())
        }
        "set_status" => {
            let task = task_arg(args)?;
            let to = args["status"]
                .as_str()
                .and_then(TaskStatus::from_name)
                .ok_or_else(|| {
                    "`status` must be todo, doing, in_review, blocked, or done for a routine's check"
                        .to_owned()
                })?;
            let reason = text_arg(args, "reason")?;
            let cited = evidence_lists(args)?;
            // Done says why when it is not "met its criteria" (ADR 0124): checked first, so a
            // reason not in the set changes nothing.
            let why = match (to, args["why"].as_str()) {
                (TaskStatus::Done, Some(w)) if w != "met_criteria" => {
                    Some(closure_arg(args, TaskStatus::Done)?)
                }
                _ => None,
            };
            engine
                .set_status_citing(run.id, task, to, reason, &cited)
                .map_err(|e| said(&e))?;
            if let Some((closure, successor)) = why {
                engine
                    .done_because_in(run.id, task, closure, successor)
                    .map_err(|e| said(&e))?;
            }
            Ok(format!("Task {} is {}.", task.0, to.name()))
        }
        "create_task" => {
            let parent = run.task.ok_or_else(|| {
                "a sub-task is created from your task, and this turn has none".to_owned()
            })?;
            let goal = engine
                .store()
                .task(parent)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "your task is gone".to_owned())?
                .goal;
            let assignee = match args["assignee"]
                .as_str()
                .map(str::trim)
                .filter(|a| !a.is_empty())
            {
                None => None,
                Some(name) => Some(
                    engine
                        .store()
                        .members()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|m| m.name.eq_ignore_ascii_case(name))
                        .ok_or_else(|| format!("no member called {name}"))?
                        .id,
                ),
            };
            let waits_on = args["waits_on"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_i64).map(TaskId).collect())
                .unwrap_or_default();
            // Which of the parent's criteria it serves (ADR 0124): its number, or its words.
            let serves = match &args["serves"] {
                Value::Number(n) => Some(n.to_string()),
                Value::String(s) => Some(s.clone()),
                _ => None,
            };
            let id = engine
                .create_task_in(
                    run.id,
                    NewTask {
                        goal,
                        project: None,
                        parent: Some(parent),
                        title: text_arg(args, "title")?.to_owned(),
                        description: text_arg(args, "description")?.to_owned(),
                        assignee,
                        priority: Priority::Normal,
                        waits_on,
                    },
                    &criteria_arg(args),
                    serves.as_deref(),
                )
                .map_err(|e| said(&e))?;
            Ok(format!("Created task {}.", id.0))
        }
        "ask_owner" if member.role == Role::Ceo => {
            let decision = crate::domain::chain::Decision::new(
                args["decision"].as_str().unwrap_or(""),
                args["recommendation"].as_str().unwrap_or(""),
                args["why"].as_str().unwrap_or(""),
            )
            .map_err(|e| e.to_string())?;
            let cited = evidence_flat(args)?;
            match engine
                .ask_board(run.id, &decision, &cited)
                .map_err(|e| said(&e))?
            {
                super::engine::Asked::Review(_) => Ok("It is with the reviewer, who checks it before it reaches the board, and this turn has ended: stop now. You are woken if it is sent back, or when the board decides.".to_owned()),
                _ => Ok("It is on the board's desk, and this turn has ended: stop now. You are woken when the board decides.".to_owned()),
            }
        }
        "propose_project" => {
            let goal = match args["goal"].as_i64() {
                Some(goal) => GoalId(goal),
                None => {
                    let task = run.task.ok_or_else(|| {
                        "which goal? Give its number as `goal`: this turn has no task".to_owned()
                    })?;
                    engine
                        .store()
                        .task(task)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "your task is gone".to_owned())?
                        .goal
                }
            };
            let lead = match args["lead"].as_str().filter(|l| !l.trim().is_empty()) {
                Some(name) => member_named(engine, name)?.id,
                None => member.id,
            };
            let proposal = Proposal {
                goal,
                title: text_arg(args, "title")?.to_owned(),
                reason: text_arg(args, "reason")?.to_owned(),
                lead,
                cap: cap_arg(args)?,
                tasks: numbers(args, "tasks").into_iter().map(TaskId).collect(),
                events: numbers(args, "events"),
            };
            let id = engine
                .propose_project(run.id, &proposal)
                .map_err(|e| said(&e))?;
            Ok(format!(
                "Project {} is proposed: it waits for the CEO, whose word comes back in this thread.",
                id.0
            ))
        }
        "decide_project" => {
            let project = args["project"]
                .as_i64()
                .map(ProjectId)
                .ok_or_else(|| "which project? Give its number as `project`".to_owned())?;
            let accept = match args["verdict"].as_str() {
                Some("accept") => true,
                Some("send_back") => false,
                _ => return Err("`verdict` must be accept or send_back".to_owned()),
            };
            let why = text_arg(args, "why")?;
            let now = engine
                .decide_project(
                    run.id,
                    project,
                    accept,
                    why,
                    cap_arg(args)?,
                    &evidence_flat(args)?,
                )
                .map_err(|e| said(&e))?;
            Ok(format!("Project {} is {}.", project.0, now.name()))
        }
        "huddle_view" => {
            let task = task_arg(args)?;
            engine
                .huddle_view_in(run.id, task, text_arg(args, "view")?, &evidence_flat(args)?)
                .map_err(|e| said(&e))?;
            Ok("Your view is with the huddle: your turn may end now.".to_owned())
        }
        "decide_stuck" => {
            let task = task_arg(args)?;
            let decision = args["decision"]
                .as_str()
                .and_then(crate::domain::ladder::Decision::from_name)
                .ok_or_else(|| Refusal::NotADecisionForStuck.to_string())?;
            let to = match args["to"].as_str().filter(|t| !t.trim().is_empty()) {
                Some(name) => Some(member_named(engine, name)?.id),
                None => None,
            };
            let handover = super::protocol_world::handover_arg(args)?;
            // A drop's reason is the engine's to refuse, after whether the decision is the
            // lead's to make at all.
            let dropped = match decision {
                crate::domain::ladder::Decision::Drop => {
                    closure_arg(args, TaskStatus::Dropped).ok()
                }
                _ => None,
            };
            engine
                .decide_stuck_in(
                    run.id,
                    task,
                    decision,
                    text_arg(args, "detail")?,
                    to,
                    &evidence_flat(args)?,
                    handover.as_ref(),
                    dropped,
                )
                .map_err(|e| said(&e))?;
            Ok(format!(
                "Task {} is decided ({}): it is in its thread.",
                task.0,
                decision.name().replace('_', " ")
            ))
        }
        "judge_finding" => {
            let id = args["finding"]
                .as_i64()
                .ok_or_else(|| "which finding? Give its number as `finding`".to_owned())?;
            let verdict = args["verdict"]
                .as_str()
                .and_then(crate::domain::guards::Verdict::from_name)
                .ok_or_else(|| {
                    "`verdict` is real, false_positive, not_enough or resolved".to_owned()
                })?;
            engine
                .judge_finding_in(run.id, id, verdict, args["fix_worked"].as_bool())
                .map_err(|e| said(&e))?;
            Ok(format!("Finding {id} is judged {}.", verdict.name()))
        }
        "set_strategy" => {
            let goal = args["goal"]
                .as_i64()
                .map(GoalId)
                .ok_or_else(|| "which goal? Give its number as `goal`".to_owned())?;
            let part = |key: &str| args[key].as_str().map(str::to_owned);
            let edit = crate::domain::areas::StrategyEdit {
                purpose: part("purpose"),
                success: part("success"),
                measures: part("measures"),
                bets: part("bets"),
                not_now: part("not_now"),
            };
            engine
                .set_strategy_in(run.id, goal, &edit, &evidence_flat(args)?)
                .map_err(|e| said(&e))?;
            Ok(format!(
                "Goal {}'s strategy is written; the owner sees it on the goal's card.",
                goal.0
            ))
        }
        "report_area" => {
            let part = |key: &str| args[key].as_str().unwrap_or("");
            let id = engine
                .report_area_in(
                    run.id,
                    [
                        part("moved"),
                        part("stuck"),
                        part("need"),
                        part("proposals"),
                    ],
                )
                .map_err(|e| said(&e))?;
            Ok(format!(
                "Your area report is kept (a{id}) and its task closed: the CEO reads it in the daily review. Your turn may end."
            ))
        }
        "direct" => {
            let task = task_arg(args)?;
            let direction = text_arg(args, "direction")?;
            let lead = engine
                .direct_in(run.id, task, direction, &evidence_flat(args)?)
                .map_err(|e| said(&e))?;
            let name = engine
                .store()
                .members()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|m| m.id == lead)
                .map_or_else(|| "the lead".to_owned(), |m| m.name);
            Ok(format!(
                "Your direction is in task {}'s thread, and {name} is woken: {name} decides.",
                task.0
            ))
        }
        "propose_goal" => {
            let tasks: Vec<TaskId> = numbers(args, "tasks").into_iter().map(TaskId).collect();
            let goal = engine
                .propose_goal(
                    run.id,
                    text_arg(args, "title")?,
                    text_arg(args, "reason")?,
                    &tasks,
                    &numbers(args, "events"),
                )
                .map_err(|e| said(&e))?;
            Ok(format!(
                "Goal {} is proposed: it waits on the board's desk.",
                goal.0
            ))
        }
        "note_house" => {
            let entry = text_arg(args, "entry")?;
            let line = engine.note_house(run.id, entry).map_err(|e| said(&e))?;
            Ok(format!("Noted on the house page's timeline: {line}"))
        }
        "rewrite_house" => {
            let log = args["log"].as_str().unwrap_or("");
            let kept = engine.rewrite_house(run.id, log).map_err(|e| said(&e))?;
            Ok(format!("The house log is rewritten: {kept} characters."))
        }
        "read_doc" => {
            let path = text_arg(args, "path")?;
            let part = args["part"]
                .as_u64()
                .and_then(|p| usize::try_from(p).ok())
                .unwrap_or(1);
            engine.read_doc(run.id, path, part).map_err(|e| said(&e))
        }
        "read_document" => {
            let id = match &args["document"] {
                Value::Number(n) => n.as_i64(),
                Value::String(s) => s.trim().trim_start_matches('d').parse().ok(),
                _ => None,
            }
            .ok_or_else(|| {
                "which document? Give its number as `document` (12 for d12)".to_owned()
            })?;
            let part = args["part"]
                .as_u64()
                .and_then(|p| usize::try_from(p).ok())
                .unwrap_or(1);
            engine.read_document(run.id, id, part).map_err(|e| said(&e))
        }
        "recheck" => {
            let task = match args["task"].as_i64() {
                Some(task) => TaskId(task),
                None => run.task.ok_or_else(|| {
                    "which task? Give its number as `task`: this turn has none".to_owned()
                })?,
            };
            let why = text_arg(args, "why")?;
            let check = engine.recheck_in(run.id, task, why).map_err(|e| said(&e))?;
            Ok(format!(
                "Check c{check} is queued again on the newest branch of task {}: no new \
                 submission. The dev node's runner takes it; its result comes back as a \
                 receipt in the task's thread, and you are woken when it does.",
                task.0
            ))
        }
        "request_hire" => {
            let role = Role::from_name(text_arg(args, "role")?)
                .filter(|r| *r != Role::Ceo)
                .ok_or_else(|| "`role` is developer, researcher or reviewer".to_owned())?;
            let item = engine
                .request_hire(
                    run.id,
                    args["name"].as_str().filter(|n| !n.trim().is_empty()),
                    role,
                    text_arg(args, "brain")?,
                    text_arg(args, "why")?,
                    &evidence_flat(args)?,
                )
                .map_err(|e| said(&e))?;
            Ok(format!(
                "The request is on the owner's desk (item {}): they hire or decline, and you hear in this thread. Go on with your turn.",
                item.0
            ))
        }
        "note" => {
            let text = text_arg(args, "text")?;
            let cited = evidence_flat(args)?;
            let id = engine.note_in(run.id, text, &cited).map_err(|e| said(&e))?;
            Ok(format!("Noted as n{id} in your notebook."))
        }
        "forget" => {
            let id = match &args["note"] {
                Value::Number(n) => n.as_i64(),
                Value::String(s) => super::engine::note_number(s),
                _ => None,
            }
            .ok_or_else(|| "which note? Give its number as `note` (12 for n12)".to_owned())?;
            let reason = args["reason"].as_str().unwrap_or("");
            let gone = engine.forget_in(run.id, id, reason).map_err(|e| said(&e))?;
            // What was built on it is named (ADR 0124).
            let built = engine.built_on(id).map_err(|e| said(&e))?;
            Ok(format!("n{id} is forgotten: {}{built}", gone.text))
        }
        "read_notebook" => engine.read_notebook_in(run.id).map_err(|e| said(&e)),
        "notebook_checked" => {
            engine.notebook_checked_in(run.id).map_err(|e| said(&e))?;
            Ok("Your notebook is checked: nothing to tidy for a week.".to_owned())
        }
        "request_staff" => {
            let project = args["project"]
                .as_i64()
                .map(ProjectId)
                .ok_or_else(|| "which project? Give its number as `project`".to_owned())?;
            let item = engine
                .request_staff(
                    run.id,
                    project,
                    text_arg(args, "why")?,
                    &evidence_flat(args)?,
                )
                .map_err(|e| said(&e))?;
            Ok(format!(
                "The request is on the owner's desk (item {}): they bring a member back or hire, and you hear in this thread. Go on with your turn.",
                item.0
            ))
        }
        "ask_owner" => {
            let text = text_arg(args, "text")?;
            let cited = evidence_flat(args)?;
            match engine
                .ask_owner_citing(run.id, text, &cited)
                .map_err(|e| said(&e))?
            {
                super::engine::Asked::Up(to) => {
                    let to = engine
                        .store()
                        .members()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|m| m.id == to)
                        .map_or_else(|| "the one above you".to_owned(), |m| m.name);
                    Ok(format!("It is with {to}, in the task's thread, and this turn has ended: stop now. You are woken when they answer."))
                }
                super::engine::Asked::Desk(_) => Ok("It is on the owner's desk, and this turn has ended: stop now. You are woken when they have seen it.".to_owned()),
                super::engine::Asked::Review(_) => Ok("It is with the reviewer, who checks it before it reaches the owner's desk, and this turn has ended: stop now. You are woken if it is sent back, or when the owner has seen it.".to_owned()),
            }
        }
        "review" => {
            let task = task_arg(args)?;
            let approve = match args["verdict"].as_str() {
                Some("approve") => true,
                Some("send_back") => false,
                _ => return Err("`verdict` must be approve or send_back".to_owned()),
            };
            let notes = text_arg(args, "notes")?;
            if notes.chars().count() > LONGEST_POST {
                return Err(format!("notes are at most {LONGEST_POST} characters"));
            }
            let cited = evidence_lists(args)?;
            match engine.test_in(run.id, task, approve, notes, &cited) {
                Ok(()) if approve => Ok(format!("Task {} is approved.", task.0)),
                Ok(()) => Ok(format!(
                    "Task {} is sent back to its owner, who is woken to put it right.",
                    task.0
                )),
                Err(super::engine::EngineError::Refused(Refusal::GateNotPassed)) => Err(
                    "this work needs the gate (tests, lint and build), which has not passed: it stays in review. Say so in its thread; the owner decides"
                        .to_owned(),
                ),
                Err(e) => Err(said(&e)),
            }
        }
        "hand_to" => {
            let task = task_arg(args)?;
            let to = member_named(engine, text_arg(args, "to")?)?;
            let why = text_arg(args, "why")?;
            let handover = super::protocol_world::handover_needed(args)?;
            engine
                .hand_to_in(run.id, task, to.id, why, &handover)
                .map_err(|e| said(&e))?;
            Ok(format!(
                "Task {} is {}'s now; they are woken.",
                task.0, to.name
            ))
        }
        "accept" => {
            let task = task_arg(args)?;
            let accept = match args["verdict"].as_str() {
                Some("accept") => true,
                Some("send_back") => false,
                _ => return Err("`verdict` must be accept or send_back".to_owned()),
            };
            let notes = text_arg(args, "notes")?;
            let cited = evidence_lists(args)?;
            match engine.accept_in(run.id, task, accept, notes, &cited) {
                Ok(()) if accept => Ok(format!("Task {} is accepted: it is done.", task.0)),
                Ok(()) => Ok(format!(
                    "Task {} is sent back to its member, who is woken to put it right.",
                    task.0
                )),
                Err(EngineError::Refused(Refusal::GateNotPassed)) => Err(
                    "this work needs the gate (tests, lint and build), which has not passed: it stays in review"
                        .to_owned(),
                ),
                Err(e) => Err(said(&e)),
            }
        }
        "check_back" => {
            let minutes = args["minutes"]
                .as_i64()
                .ok_or_else(|| "how long? Give `minutes`, 5 to 720".to_owned())?;
            let why = text_arg(args, "why")?;
            engine
                .check_back(run.id, minutes, why)
                .map_err(|e| said(&e))?;
            let waits = crate::domain::routines::check_back_minutes(minutes);
            Ok(format!(
                "You will be woken on this task in {waits} minutes, if it is still open: {why}"
            ))
        }
        "note_lesson" => {
            let lesson = text_arg(args, "lesson")?;
            let handle = engine.note_lesson(run.id, lesson).map_err(|e| said(&e))?;
            Ok(format!(
                "Kept as story:{handle}, marked to retell: lessons are stories now. Tell it whole \
                 with tell_story under the same handle (when it matters, the tempting move, what \
                 to do instead, and why)."
            ))
        }
        // The node's model relay charges every call a turn makes, as it makes it; a worker that
        // reported its own use too would pay twice. Kept as a word, not a tool, for a worker
        // that was told of it before.
        "report_usage" => Ok(
            "Nothing to report: the model relay already charges every call this turn makes."
                .to_owned(),
        ),
        // Not built yet (spec section 15, phase 5): said plainly, so a worker does not pretend.
        "run_gate" | "request_action" => Err(format!(
            "`{tool}` is not available yet: say in the thread what you would have done, and ask the owner"
        )),
        _ => Err(format!("there is no tool called `{tool}`")),
    }
}
