//! The team protocol (spec section 6): the tools every worker uses, whatever its runtime,
//! served by HQ over MCP. A call is made by a turn, known by its token; the member is that
//! turn's, so nobody acts as anyone else, and the engine's rules decide every call (claiming,
//! ranks, only the gate sets `done`). What touches a real system is never here: it is a
//! request the gateway decides.

use serde_json::{Value, json};

use super::engine::{Engine, EngineError};
use super::ports::{Clock, NewTask, Run, TeamStore, WorkerAdapter};
use crate::domain::drain;
use crate::domain::review::SendBackReason;
use crate::domain::{Criterion, Member, Priority, Ref, Refusal, RunStatus, TaskId, TaskStatus};

/// A send-back's `reason` parameter (ADR 0132): the kind of failure HQ counts, so that a fall in
/// it shows the memory builds working.
fn reason_schema() -> Value {
    json!({
        "type": "string",
        "enum": SendBackReason::ALL.map(SendBackReason::name),
        "description": "For send_back only, when the work repeats something the team, the lab or the fork already has: exists_already. Say where it is in the notes. Leave it out for any other reason.",
    })
}

/// The `reason` argument of a verdict: the reason a send-back names, if it names one.
fn reason_arg(args: &Value, sending_back: bool) -> Result<Option<SendBackReason>, String> {
    let Some(said) = args["reason"]
        .as_str()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    else {
        return Ok(None);
    };
    if !sending_back {
        return Err("`reason` only goes with send_back".to_owned());
    }
    SendBackReason::from_name(said)
        .map(Some)
        .ok_or_else(|| format!("`reason` must be {}", SendBackReason::names()))
}

/// What counts as evidence, as every tool that takes it says (ADR 0114).
const WHAT_COUNTS: &str = "Ids from HQ's record: receipts (r12) your tool results name, tasks \
    (task 12), events (event 340). A model's word is not evidence; a reading is.";

/// An `evidence` parameter: one list of ids per criterion, in order (or one id each).
fn per_criterion(what: &str) -> Value {
    json!({
        "type": "array",
        "items": { "anyOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "string" } },
        ] },
        "description": format!("{what} One list per criterion, in order. {WHAT_COUNTS}"),
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

/// One piece of evidence as the model gave it: an id, a finding with ids in it, or an object
/// whose values are ids (`{"receipt": "r301", "task": 695}`, its keys naming the kind of a bare
/// number). A finding or an object holding no id is refused, as words are.
fn refs_of(v: &Value, wrote: &Wrote) -> Result<Vec<Ref>, String> {
    let read: Vec<Ref> = match v {
        Value::Object(fields) => fields
            .iter()
            .filter_map(|(kind, value)| {
                let said = match value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => return None,
                };
                Ref::parse(&said).or_else(|| Ref::parse(&format!("{kind} {said}")))
            })
            .collect(),
        // A reference as written, or every whole one in a finding (ADR 0114, amended).
        other => other.as_str().map_or_else(Vec::new, |said| {
            Ref::parse(said).map_or_else(|| Ref::all_in(said), |one| vec![one])
        }),
    };
    if read.is_empty() {
        let quoted = crate::domain::evidence::short(&v.to_string(), 40);
        if let Some(document) = document_in(&v.to_string()) {
            let cite = wrote(document).map_or_else(
                || {
                    "cite the receipt of the call that wrote it (r12, named in the same result)"
                        .to_owned()
                },
                |receipt| format!("cite r{receipt}, the receipt that wrote it"),
            );
            return Err(format!(
                "`{quoted}` is not evidence: d{document} is a document, not a receipt: {cite}"
            ));
        }
        return Err(format!(
            "`{quoted}` is not evidence: cite a receipt as r12, a task as task 12, an event as event 340"
        ));
    }
    Ok(read)
}

/// The receipt that wrote a document, by the document's number ([`Engine::receipt_of_document`]).
type Wrote<'a> = dyn Fn(i64) -> Option<i64> + 'a;

/// The first document a piece of evidence names as the tools do (`d241`), if any.
fn document_in(said: &str) -> Option<i64> {
    said.split(|c: char| !c.is_ascii_alphanumeric())
        .find_map(|w| w.strip_prefix('d').and_then(|n| n.parse().ok()))
        .filter(|n: &i64| *n > 0)
}

/// What names the receipt behind a document cited as evidence, for `engine`'s refusal.
fn wrote_by<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
) -> impl Fn(i64) -> Option<i64> + '_ {
    |document| engine.receipt_of_document(document).ok().flatten()
}

/// The `evidence` argument, one list per criterion: each item an id or a list of ids.
fn evidence_lists(args: &Value, wrote: &Wrote) -> Result<Vec<Vec<Ref>>, String> {
    let Some(items) = args["evidence"].as_array() else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| match item {
            Value::Array(list) => list_refs(list, wrote),
            other => refs_of(other, wrote),
        })
        .collect()
}

/// One criterion's list: its ids, with words beside an id let go as that id's gloss
/// (`["r4031", "the :ro statement is in the thread"]`). A list of words alone, or one citing a
/// document, is refused as a single piece would be.
fn list_refs(list: &[Value], wrote: &Wrote) -> Result<Vec<Ref>, String> {
    let mut read = Vec::new();
    let mut words = None;
    for one in list {
        match refs_of(one, wrote) {
            Ok(refs) => read.extend(refs),
            Err(why) if document_in(&one.to_string()).is_some() => return Err(why),
            Err(why) => {
                words.get_or_insert(why);
            }
        }
    }
    match words {
        Some(why) if read.is_empty() => Err(why),
        _ => Ok(read),
    }
}

/// The `evidence` argument as one list.
fn evidence_flat(args: &Value, wrote: &Wrote) -> Result<Vec<Ref>, String> {
    Ok(evidence_lists(args, wrote)?.concat())
}

/// The `criteria` argument: each `{ text, live }`, or a line of text.
///
/// # Errors
/// A criterion past the most one may be ([`Criterion::written`]).
fn criteria_arg(args: &Value) -> Result<Vec<Criterion>, String> {
    let Some(all) = args["criteria"].as_array() else {
        return Ok(Vec::new());
    };
    let mut criteria = Vec::new();
    for c in all {
        let (text, live) = match c {
            Value::String(s) => (s.as_str(), false),
            other => match other["text"].as_str() {
                Some(text) => (text, other["live"].as_bool() == Some(true)),
                None => continue,
            },
        };
        if text.trim().is_empty() {
            continue;
        }
        let made = Criterion::written(criteria.len() + 1, text, live)?;
        // A wait, in minutes from the work's last change (ADR 0120).
        criteria.push(
            match c["after"].as_u64().and_then(|m| u32::try_from(m).ok()) {
                Some(minutes) => made.after(minutes),
                None => made,
            },
        );
    }
    Ok(criteria)
}

/// The `handover` a post may carry (ADR 0134): only when HQ has told the turn a restart is
/// coming. It is the same note work handed on needs, kept for the member's own next turn.
fn drain_handover_schema() -> Value {
    let mut schema = super::protocol_world::handover_schema();
    schema["description"] = json!(
        "Only when your turn says a restart is coming: a note for your next turn here, each \
         part said, at most 1,200 characters in all. Posting it ends your turn; the task stays \
         yours."
    );
    // Each part's name says what it holds: a turn reads this on every call, a drain is rare.
    if let Some(parts) = schema["properties"].as_object_mut() {
        for part in parts.values_mut() {
            if let Some(part) = part.as_object_mut() {
                part.remove("description");
            }
        }
    }
    schema
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

/// The tool a member asks for a kept branch to be checked again with (ADR 0115).
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

/// What a downtime turn may not do: it makes no work.
const MAKES_WORK: [&str; 1] = ["create_task"];

/// The tools every member is offered (ADR 0142), as MCP lists them: name, what it does, and its
/// input. There are no roles: the member one rank up is offered `review` in its review turn.
#[must_use]
pub fn tools() -> Value {
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
            "What this turn is for: who you are, the task and why it exists, what is new, and its thread. Call it first.",
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
            "Write in a task's thread (yours, one under or above it, or one naming you): what you did, found or need. A mention hands no work on. A finding cites its evidence; without any, in a turn with no receipts, it is marked unsupported.",
            json!({ "task": task, "text": { "type": "string" }, "evidence": ids("What it rests on."), "handover": drain_handover_schema() }),
            &["task", "text"]
        ),
        tool(
            "set_status",
            "Move your task on, with a reason. in_review cites evidence per criterion (a live one: a reading this turn); HQ's checks and the evidence decide it, and that is how work becomes done. A routine's check you close as done yourself, with evidence, once you have looked. done says why: met_criteria, superseded (name the successor) or not_needed.",
            json!({ "task": task, "status": { "type": "string", "enum": ["todo", "doing", "in_review", "blocked", "done"] }, "reason": { "type": "string" }, "evidence": per_criterion("For in_review or a check closed as done: what shows each criterion holds."), "why": { "type": "string", "enum": ["met_criteria", "superseded", "not_needed"], "description": "for done" }, "successor": { "type": "integer", "description": "for superseded: the task carrying the work on" } }),
            &["task", "status", "reason"]
        ),
        tool(
            "create_task",
            "Create a sub-task of your task, for yourself (work is never handed to another member), with the tasks it waits on and its acceptance criteria: what done means, a short list. Mark live any criterion about the lab as it is now.",
            json!({ "title": { "type": "string" }, "description": { "type": "string" }, "assignee": { "type": "string", "description": "your own name, or leave it out" }, "waits_on": { "type": "array", "items": { "type": "integer" } }, "criteria": { "type": "array", "items": { "type": "object", "properties": { "text": { "type": "string" }, "live": { "type": "boolean", "description": "about the lab now: shown only by a reading in the same turn" }, "after": { "type": "integer", "minimum": 1, "description": "minutes it waits from the work's last change before it can be shown, e.g. 60 for 'no new alarm since'" } }, "required": ["text"] } } }),
            &["title", "description"]
        ),
        tool(
            "ask_owner",
            // Climb 2, S3: when to ask is the kernel's one statement (briefly here, whole in the
            // worker's instructions), its category clause word for word in both.
            &format!(
                "Owner decisions, with evidence, {} It ends your turn. HQ asks the owner itself on \
                 repeat failure.",
                endora_kernel::escalation::when_to_ask_briefly()
            ),
            json!({ "text": { "type": "string" }, "evidence": ids("What it rests on.") }),
            &["text"]
        ),
        tool(
            "check_back",
            "Come back to your task later, once: after a fix, to confirm it held. You are woken on this task after the minutes you give (5 to 720), if it is still open.",
            json!({ "minutes": { "type": "integer", "minimum": 5, "maximum": 720 }, "why": { "type": "string", "description": "what you will look at when you come back" } }),
            &["minutes", "why"]
        ),
    ]);
    if let Some(list) = all.as_array_mut() {
        list.push(recheck_tool());
        list.push(read_document_tool());
        list.push(read_doc_tool());
        list.push(super::chess_tool::tool());
        list.push(super::protocol_world::read_world_tool());
        // Asking the past (ADR 0129).
        list.extend(super::protocol_history::tools());
        list.push(tool(
            "add_to_map",
            "Add one line to the team's map when you find where something lives (a setting, a file, a log, who or what owns it), so the next member need not look. The same line again confirms it; lines nobody confirms fade after 30 days.",
            json!({ "line": { "type": "string", "maxLength": 190, "description": "what, and where it is" } }),
            &["line"],
        ));
    }
    all
}

/// The member one rank up's verdict on work in review (ADR 0142), offered in its review turn.
fn review_tool() -> Value {
    let task = json!({ "type": "integer", "description": "the task's number" });
    json!({
        "name": "review",
        "description": "Your verdict on another member's work in review, once your own readings confirm each piece of its evidence is real and still true. approve passes it on your evidence per criterion, with at least one reading of yours this turn; send_back returns it with exactly what does not hold, and is a finished review too. Never your own work.",
        "inputSchema": { "type": "object", "properties": { "task": task, "verdict": { "type": "string", "enum": ["approve", "send_back"] }, "notes": { "type": "string", "description": "what you checked, or what does not hold" }, "reason": reason_schema(), "evidence": per_criterion("For approve: your readings and what else shows each criterion holds.") }, "required": ["task", "verdict", "notes"] },
    })
}

/// The mark (MCP's `_meta`) on a tool a turn is to be offered up front: what the turn's kind
/// is for (ADR 0142, amended 2026-10-30). A worker on a house model starts with HQ's core and
/// these, and finds the rest by name or search. The mark grants nothing: every call is decided
/// here, as before.
pub const UP_FRONT: &str = "endora/up_front";

/// Marks `tool` as one the turn is offered up front.
fn up_front(tool: &mut Value) {
    tool["_meta"] = json!({ UP_FRONT: true });
}

/// The tools one turn is offered (what `tools/list` answers): its member's role's, and, in the
/// turn that is for it, the tool a special turn answers with (ADR 0131, amended): `answer_quiz`
/// in the night's quiz turn and `answer_assistance` in an assigned expert turn. The
/// number of tools a model is offered is a measured failure mode for the house's model (ADR
/// 0085). A store that cannot say what the turn is offers it the role's, as every other turn.
#[must_use]
fn tools_for_turn<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
) -> Value {
    let mut all = tools();
    let Some(list) = all.as_array_mut() else {
        return all;
    };
    let woken_for = engine
        .store()
        .wake(run.wake)
        .ok()
        .flatten()
        .map(|w| w.reason);
    // A downtime turn is for a move (ADR 0119): its game is up front.
    if woken_for == Some(crate::domain::WakeReason::Downtime) {
        if let Some(chess) = list.iter_mut().find(|t| t["name"] == "chess") {
            up_front(chess);
        }
    }
    // In the loops, the member one rank up reviews (ADR 0142): its review turn has the tool.
    let reviewing = woken_for == Some(crate::domain::WakeReason::ReviewRequested);
    if reviewing {
        if let Some(task) = run.task.and_then(|t| engine.store().task(t).ok().flatten()) {
            if engine
                .loop_reviewer_of(&task)
                .ok()
                .flatten()
                .is_some_and(|r| r.id == member.id)
            {
                let mut review = review_tool();
                up_front(&mut review);
                list.push(review);
            }
        }
    }
    // A match's turn plays it with its tool (ADR 0135).
    if let Ok(Some(_)) = engine.scenario_kind_of_run(run.id) {
        let mut play = super::scenario_tool::tool();
        up_front(&mut play);
        list.push(play);
    }
    // A turn on the work of a project that lists its tools is offered HQ's four and the list
    // (the Holodeck plan's phase 4); a downtime turn is not narrowed here.
    if let Some(given) = engine
        .lab_tools_of(run.id)
        .ok()
        .flatten()
        .filter(|_| woken_for != Some(crate::domain::WakeReason::Downtime))
    {
        if !given.iter().any(|g| g == crate::domain::projects::TEAM) {
            list.retain(|t| {
                t["name"].as_str().is_some_and(|n| {
                    crate::domain::lab_tools::HQ_TOOLS.contains(&n) || given.iter().any(|g| g == n)
                })
            });
        }
    }
    // A downtime turn is a move and nothing else (30 Oct: office chess turns averaged 27,760
    // prompt tokens, the week's largest cost): it reads its turn and plays.
    if woken_for == Some(crate::domain::WakeReason::Downtime) {
        list.retain(|t| t["name"] == "my_turn" || t["name"] == "chess");
    }
    // No protocol moves (D30 of round 3): a turn on an item its member holds already has its
    // briefing (the worker reads `my_turn` for it before the first round) and the item, so it
    // is offered neither `my_turn` nor `claim`. HQ still answers both, for the worker.
    if holds_its_item(engine, run, member) {
        list.retain(|t| t["name"] != "my_turn" && t["name"] != "claim");
    }
    all
}

/// Whether `run` is on an item its `member` holds (D30 of round 3): one handed to it, or one it
/// goes on with after a handover.
fn holds_its_item<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
) -> bool {
    run.task
        .and_then(|t| engine.store().task(t).ok().flatten())
        .is_some_and(|task| task.holder() == Some(member.id))
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

/// Familiar tool-directory entry point, using the canonical scope policy.
#[must_use]
pub fn tools_in<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
) -> Value {
    tools_in_v2(engine, run, member)
}

/// Team protocol v2 advertises only the canonical conversation contract.
/// Retired legacy expert turns receive no tools.
#[must_use]
pub fn tools_in_v2<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
) -> Value {
    tools_for_turn(engine, run, member)
}

/// The node's tools a turn is to be offered up front, which HQ names in its listing's own
/// `_meta` (under [`UP_FRONT`]) since it does not list them itself (D27 of round 3): on a loop's
/// item, the fork's checkout and submit for the member who holds it, and the checkout for its
/// reviewer, unless the loop's own list of tools leaves the fork out. The worker loads them as
/// it loads what HQ marks of its own; the node decides every call, as before.
#[must_use]
pub fn node_tools_up_front<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
) -> Vec<&'static str> {
    const CHECKOUT: &str = "repo_checkout";
    let Some(task) = run.task.and_then(|t| engine.store().task(t).ok().flatten()) else {
        return Vec::new();
    };
    if !engine.loop_holds(&task).unwrap_or(false) {
        return Vec::new();
    }
    if engine
        .lab_tools_of(run.id)
        .ok()
        .flatten()
        .is_some_and(|given| !given.iter().any(|g| g == CHECKOUT))
    {
        return Vec::new();
    }
    if task.holder() == Some(member.id) {
        return vec![CHECKOUT, "repo_submit"];
    }
    let reviews = engine
        .loop_reviewer_of(&task)
        .ok()
        .flatten()
        .is_some_and(|r| r.id == member.id);
    if reviews { vec![CHECKOUT] } else { Vec::new() }
}

/// Run a v2 tool through the existing token, authority, lineage and drain gates.
/// This adapter cannot select the legacy task-thread consultation lifecycle.
/// # Errors
/// Retired/invalid arguments, a legacy expert turn, or a policy refusal.
pub fn call_v2<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
    tool: &str,
    args: &Value,
) -> Result<String, String> {
    call_for(engine, run, member, tool, args)
}

/// Runs one tool for a turn. `Ok` is what the worker reads back; `Err` is a refusal it reads
/// as a tool error, said so that it can do something about it.
///
/// While a drain lasts (ADR 0134), every answer a running turn gets, a refusal as much as a
/// reading, ends with the notice that a restart is coming. This is the one place the protocol's
/// answers pass through, so it is the one place that says it.
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
    call_v2(engine, run, member, tool, args)
}

fn call_for<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
    tool: &str,
    args: &Value,
) -> Result<String, String> {
    let answer = call_tool(engine, run, member, tool, args);
    // Legacy tool replies do not all carry source-run lineage. Until they do, an admitted
    // model-visible reply makes this turn ineligible to label a new message Internal.
    // A successful canonical consult returns only the typed assistance acceptance.
    // claim returns only a typed task-ID acknowledgment, not newly read content.
    // Other unclassified tool results still follow the conservative lineage rule below.
    if answer.is_ok()
        && !matches!(
            tool,
            "my_turn"
                | "claim"
                | "record_assistance_finding"
                | "read_notebook"
                | "read_doc"
                | "read_document"
                | "read_world"
                | "ask_record"
                | "read_messages"
                | "send_message"
                | "request_assistance"
                | "answer_assistance"
                | "read_assistance_source"
                | "cancel_assistance"
        )
    {
        engine
            .unclassified_message_input(run.id)
            .map_err(|e| e.to_string())?;
    }
    // The version guidance opens the briefing: the same bytes every turn, so it is part of the
    // prompt's fixed opening a provider caches (ADR 0142, amended 2026-10-31). The worker finds
    // who the member is on the line starting "You are ". The drain notice below remains the
    // final instruction.
    let answer = if tool == "my_turn" {
        answer.map(|text| {
            format!("Endora team protocol v2: one team of loops. Work the task, check it, and ask_owner only for the owner's decisions; three failures take the work a rank up.\n{text}")
        })
    } else {
        answer
    };
    // A turn the call ended has nothing more to read: it was told when it ended it.
    let over = engine
        .store()
        .run(run.id)
        .ok()
        .flatten()
        .is_none_or(|r| r.status != RunStatus::Running);
    if over || !engine.draining() {
        return answer;
    }
    let notice = format!("\n\n{} {}", drain::NOTICE, drain::HOW_TO_LEAVE_IT);
    match answer {
        Ok(text) => Ok(text + &notice),
        Err(text) => Err(text + &notice),
    }
}

/// Why this call is refused before any tool runs, if it is: whatever the model was told, these
/// hold here.
fn refusal<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    tool: &str,
    args: &Value,
) -> Result<Option<String>, String> {
    // A call that set out while the turn ran, and arrived after it ended, does nothing.
    let still = engine.store().run(run.id).map_err(|e| e.to_string())?;
    if still.is_none_or(|r| r.status != RunStatus::Running) {
        return Ok(Some(Refusal::TurnOver.to_string()));
    }
    // Downtime makes no work (ADR 0119): the tools that would are refused in its turns.
    if MAKES_WORK.contains(&tool) && engine.in_downtime(run.id).map_err(|e| said(&e))? {
        return Ok(Some(Refusal::InDowntime.to_string()));
    }
    // A scenario is closed by its world, never by a member's word (the Holodeck plan's phase 5).
    if let Some(refusal) = engine.scenario_refuses(tool, args).map_err(|e| said(&e))? {
        return Ok(Some(refusal));
    }
    // A project that lists its tools gives its work those and the turn's own four (the
    // Holodeck plan's phase 4): refused here too, whatever the model was told.
    if let Some(refusal) = engine.project_refuses(run.id, tool).map_err(|e| said(&e))? {
        return Ok(Some(refusal));
    }
    Ok(None)
}

fn call_tool<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    member: &Member,
    tool: &str,
    args: &Value,
) -> Result<String, String> {
    if let Some(refusal) = refusal(engine, run, tool, args)? {
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
        "chess" => super::chess_tool::call(engine, run, args),
        "scenario" => super::scenario_tool::call(engine, run, args),
        "my_turn" => tool_my_turn(engine, run),
        "claim" => {
            let task = task_arg(args)?;
            engine.claim_in(run.id, task).map_err(|e| said(&e))?;
            Ok(format!("Task {} is yours.", task.0))
        }
        "post" => tool_post(engine, run, args),
        "set_status" => tool_set_status(engine, run, args),
        "create_task" => tool_create_task(engine, run, args),
        "read_doc" => {
            let path = text_arg(args, "path")?;
            let part = args["part"]
                .as_u64()
                .and_then(|p| usize::try_from(p).ok())
                .unwrap_or(1);
            engine.read_doc(run.id, path, part).map_err(|e| said(&e))
        }
        "read_document" => tool_read_document(engine, run, args),
        "recheck" => tool_recheck(engine, run, args),
        "ask_owner" => tool_ask_owner(engine, run, args),
        "review" => tool_review(engine, run, args),
        "add_to_map" => {
            let line = text_arg(args, "line")?;
            engine.add_to_map(run.id, line).map_err(|e| said(&e))
        }
        "check_back" => tool_check_back(engine, run, args),
        // The node's model relay charges every call a turn makes, as it makes it; a worker that
        // reported its own use too would pay twice. Kept as a word, not a tool, for a worker
        // that was told of it before.
        "report_usage" => Ok(
            "Nothing to report: the model relay already charges every call this turn makes."
                .to_owned(),
        ),
        _ => Err(format!("there is no tool called `{tool}`")),
    }
}

fn tool_post<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
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
        // Lines it marks as lessons wait for the item's verdict (ADR 0142, amended 2026-10-31).
        engine.offer_lessons(task, text).map_err(|e| said(&e))?;
        // The worker's word that the turn's prompt reached its budget (ADR 0142, amended
        // 2026-10-30): the item goes on with the same member in a fresh turn, when HQ sees the
        // turn did read near its budget. The mark alone authorizes nothing.
        if args["continues"].as_bool() == Some(true)
            && !engine.out_of_room_in(run.id, task).map_err(|e| said(&e))?
        {
            return Ok(
                "Posted. The turn is not continued: HQ did not see its prompt reach \
                       its budget."
                    .to_owned(),
            );
        }
        return Ok("Posted.".to_owned());
    }
    let cited = evidence_flat(args, &wrote_by(engine))?;
    // A handover note with the post (ADR 0134) is a wrap-up for a restart: the note is
    // read, and refused, before anything is posted; the turn ends and keeps its task.
    let wrap = super::protocol_world::handover_arg(args)?;
    if wrap.is_some() && !engine.draining() {
        return Err(Refusal::NoRestartComing.to_string());
    }
    engine
        .post_citing(run.id, task, text, &cited)
        .map_err(|e| said(&e))?;
    let Some(note) = wrap else {
        return Ok("Posted.".to_owned());
    };
    engine
        .wrap_up_in(run.id, task, &note)
        .map_err(|e| said(&e))?;
    Ok("Posted, and your handover note is kept. This turn has ended: stop now. The task is still yours, and goes on after the restart with your note.".to_owned())
}

fn tool_set_status<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
    let task = task_arg(args)?;
    let to = args["status"]
        .as_str()
        .and_then(TaskStatus::from_name)
        .ok_or_else(|| {
            "`status` must be todo, doing, in_review, blocked, or done for a routine's check"
                .to_owned()
        })?;
    let reason = text_arg(args, "reason")?;
    let cited = evidence_lists(args, &wrote_by(engine))?;
    // Done says why when it is not "met its criteria" (ADR 0124): checked first, so a
    // reason not in the set, a superseded that names no successor (or itself, or a task
    // that is not there) changes nothing.
    let why = match (to, args["why"].as_str()) {
        (TaskStatus::Done, Some(w)) if w != "met_criteria" => {
            let (closure, successor) = closure_arg(args, TaskStatus::Done)?;
            crate::domain::closure::may_close(task, closure, successor)
                .map_err(|r| r.to_string())?;
            if let Some(next) = successor {
                let there = engine.store().task(next).map_err(|e| e.to_string())?;
                if there.is_none() {
                    return Err(format!("there is no task #{} to carry the work on", next.0));
                }
            }
            Some((closure, successor))
        }
        _ => None,
    };
    let left = closing_over_sandbox(engine, run, task, to, args)?;
    engine
        .set_status_citing_noting(run.id, task, to, reason, &cited, &left)
        .map_err(|e| said(&e))?;
    // Moving an item on is often a turn's last word: lines it marks as lessons wait for the
    // item's next verdict (ADR 0142, amended 2026-10-31).
    engine.offer_lessons(task, reason).map_err(|e| said(&e))?;
    if let Some((closure, successor)) = why {
        engine
            .done_because_in(run.id, task, closure, successor)
            .map_err(|e| said(&e))?;
    }
    // HQ may move it on as it is set (a loop worked alone closes an item set ready): the answer
    // says what it is now, never only what was asked (run 5316).
    let now = engine
        .store()
        .task(task)
        .map_err(|e| e.to_string())?
        .map_or(to, |t| t.status);
    if now != to {
        // Done says how (ADR 0124): a completion nothing checked is said to be one.
        let how = engine
            .store()
            .closure(task)
            .map_err(|e| e.to_string())?
            .map_or_else(String::new, |c| format!(": {}", c.closure.plainly()));
        return Ok(format!(
            "Task {} is {}, not {}{how}. HQ moved it on as you set it (its thread says why).",
            task.0,
            now.name(),
            to.name()
        ));
    }
    Ok(format!("Task {} is {}.", task.0, to.name()))
}

fn tool_create_task<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
    let parent = run
        .task
        .ok_or_else(|| "a sub-task is created from your task, and this turn has none".to_owned())?;
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
            &criteria_arg(args)?,
        )
        .map_err(|e| said(&e))?;
    Ok(format!("Created task {}.", id.0))
}

fn tool_read_document<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
    let id = match &args["document"] {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().trim_start_matches('d').parse().ok(),
        _ => None,
    }
    .ok_or_else(|| "which document? Give its number as `document` (12 for d12)".to_owned())?;
    let part = args["part"]
        .as_u64()
        .and_then(|p| usize::try_from(p).ok())
        .unwrap_or(1);
    engine.read_document(run.id, id, part).map_err(|e| said(&e))
}

fn tool_recheck<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
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

fn tool_ask_owner<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
    let text = text_arg(args, "text")?;
    let cited = evidence_flat(args, &wrote_by(engine))?;
    match engine
        .ask_owner_citing(run.id, text, &cited)
        .map_err(|e| said(&e))?
    {
        super::engine::Asked::Desk(_) => Ok("It is on the owner's desk, and this turn has ended: stop now. You are woken when they have seen it.".to_owned()),
    }
}

fn tool_review<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
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
    let reason = reason_arg(args, !approve)?;
    let cited = evidence_lists(args, &wrote_by(engine))?;
    match engine.test_in(run.id, task, approve, notes, &cited, reason) {
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

fn tool_check_back<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    args: &Value,
) -> Result<String, String> {
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

fn tool_my_turn<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
) -> Result<String, String> {
    let mut out = run.told.clone();
    if let Some(task) = run.task {
        // The thread a turn reads: the newest handover, not every one (ADR 0142, amended
        // 2026-10-31), and not again the one its briefing tells it.
        let (thread, left_out) = engine
            .thread_for_turn(run, run.member, task, 12)
            .map_err(|e| e.to_string())?;
        let members = engine.store().members().map_err(|e| e.to_string())?;
        if !thread.is_empty() {
            out.push_str("\n\nThe thread, newest last:\n");
            for input in &thread {
                let p = &input.post;
                let line =
                    Engine::<S, W, C>::post_line(&members, &p.author, &p.text, p.unsupported);
                out.push_str(&format!("- {line}\n"));
            }
        }
        if let Some(left_out) = left_out {
            out.push_str(&left_out);
        }
    }
    Ok(out)
}

/// The interlock on closing over a sandbox's unsubmitted work (correction-placement review). It
/// applies to a closing call (`in_review`, or `done` for a routine's check) of the member's own
/// task that carries the worker's `sandbox` report, on an item the node recorded a successful
/// checkout for; a call with no report is left as it was (the owner, HQ, older workers). It
/// refuses when a reported task-branch head is neither a finished submission of this item at
/// that exact head nor the default branch's revision the node handed over, unless the call names
/// it in `unsubmitted` with a reason. Returns the thread lines saying what is left unsubmitted,
/// written before the status changes (and the change refused if they cannot be).
fn closing_over_sandbox<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    run: &Run,
    task: TaskId,
    to: TaskStatus,
    args: &Value,
) -> Result<Vec<String>, String> {
    use crate::domain::wing::{self, LeftUnsubmitted, SandboxReport};
    let Some(reported) = args.get("sandbox") else {
        return Ok(Vec::new());
    };
    // Only the turn's own task: the worker reads the sandbox for that one alone.
    if run.task != Some(task) || !matches!(to, TaskStatus::InReview | TaskStatus::Done) {
        return Ok(Vec::new());
    }
    let store = engine.store();
    let owned = store
        .task(task)
        .map_err(|e| e.to_string())?
        .is_some_and(|t| t.owner == Some(run.member));
    if !owned {
        return Ok(Vec::new());
    }
    let Some(bases) = checkout_bases(engine, task)? else {
        return Ok(Vec::new());
    };
    let report = sandbox_report(reported);
    let (mut finished, mut in_progress) = (Vec::new(), Vec::new());
    for b in store.branches().map_err(|e| e.to_string())? {
        if b.task != task {
            continue;
        }
        let pair = (b.name, b.head);
        if b.status == wing::IN_PROGRESS {
            in_progress.push(pair);
        } else {
            finished.push(pair);
        }
    }
    let left: Vec<LeftUnsubmitted> = args["unsubmitted"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|l| {
                    Some(LeftUnsubmitted {
                        branch: l["branch"].as_str()?.to_owned(),
                        head: l["head"].as_str()?.to_owned(),
                        why: l["why"].as_str()?.trim().chars().take(300).collect(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(why) = wing::outstanding(&report, &finished, &in_progress, &bases, &left) {
        return Err(why);
    }
    let SandboxReport::Read(held) = report else {
        return Ok(Vec::new());
    };
    let name = store
        .members()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.id == run.member)
        .map_or_else(|| "The member".to_owned(), |m| m.name);
    Ok(left
        .iter()
        .filter(|l| {
            !l.why.is_empty()
                && held
                    .iter()
                    .any(|h| h.branch == l.branch && h.head == l.head)
        })
        .map(|l| {
            format!(
                "{name} is closing it and leaves {} at {} unsubmitted: {}. That revision is not \
                 delivered; sandbox-only work may disappear when the item closes.",
                l.branch, l.head, l.why
            )
        })
        .collect())
}

/// The default-branch revisions the node recorded handing over for `task`'s checkouts, or `None`
/// when the node recorded no successful checkout for it at all (the interlock does not apply).
fn checkout_bases<S: TeamStore, W: WorkerAdapter, C: Clock>(
    engine: &Engine<S, W, C>,
    task: TaskId,
) -> Result<Option<Vec<String>>, String> {
    let store = engine.store();
    let mut checked_out = false;
    let mut bases = Vec::new();
    for other in store.runs().map_err(|e| e.to_string())? {
        if other.task != Some(task) {
            continue;
        }
        for r in store
            .call_receipts_of(other.id)
            .map_err(|e| e.to_string())?
        {
            if r.tool != "repo_checkout" || !r.ok || r.sandbox {
                continue;
            }
            checked_out = true;
            if let Some(base) = crate::domain::wing::base_revision(&r.excerpt) {
                bases.push(base.to_owned());
            }
        }
    }
    Ok(checked_out.then_some(bases))
}

/// The worker's `sandbox` report as HQ reads it: anything it cannot understand is unknown.
fn sandbox_report(reported: &Value) -> crate::domain::wing::SandboxReport {
    use crate::domain::wing::{HeldBranch, SandboxReport};
    let unclear = || SandboxReport::Unknown("its report could not be understood".to_owned());
    match (reported["state"].as_str(), reported["branches"].as_array()) {
        (Some("read"), Some(branches)) => branches
            .iter()
            .map(|b| {
                Some(HeldBranch {
                    branch: b["branch"].as_str()?.to_owned(),
                    head: b["head"].as_str()?.to_owned(),
                })
            })
            .collect::<Option<Vec<_>>>()
            .map_or_else(unclear, SandboxReport::Read),
        (Some("unknown"), _) => SandboxReport::Unknown(
            reported["why"]
                .as_str()
                .unwrap_or("the worker gave no reason")
                .chars()
                .take(200)
                .collect(),
        ),
        _ => unclear(),
    }
}
