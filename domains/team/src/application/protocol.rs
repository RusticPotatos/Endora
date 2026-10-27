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

/// One piece of evidence as the model gave it: an id, a finding with ids in it, or an object
/// whose values are ids (`{"receipt": "r301", "task": 695}`, its keys naming the kind of a bare
/// number). A finding or an object holding no id is refused, as words are.
fn refs_of(v: &Value) -> Result<Vec<Ref>, String> {
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
        return Err(format!(
            "`{}` is not evidence: cite a receipt as r12, a task as task 12, an event as event 340",
            crate::domain::evidence::short(&v.to_string(), 40)
        ));
    }
    Ok(read)
}

/// The `evidence` argument, one list per criterion: each item an id or a list of ids.
fn evidence_lists(args: &Value) -> Result<Vec<Vec<Ref>>, String> {
    let Some(items) = args["evidence"].as_array() else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| match item {
            Value::Array(list) => Ok(list
                .iter()
                .map(refs_of)
                .collect::<Result<Vec<_>, _>>()?
                .concat()),
            other => refs_of(other),
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

/// The `handover` a post may carry (ADR 0134): only when HQ has told the turn a restart is
/// coming. It is the same note work handed on needs, kept for the member's own next turn.
fn drain_handover_schema() -> Value {
    let mut schema = super::protocol_world::handover_schema();
    schema["description"] = json!(
        "Only when your turn says a restart is coming: your note for your next turn on this \
         task, each part said, all of it at most 1,200 characters. Posting it ends your turn; \
         the task stays yours."
    );
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
            "Write in a task's thread (yours, one under it, the one above it, or one where you were named): what you did, found or need. A mention hands no work to anyone. A finding cites its evidence; a post with none, in a turn with no receipts, is marked unsupported.",
            json!({ "task": task, "text": { "type": "string" }, "evidence": ids("What the post rests on, if anything."), "handover": drain_handover_schema() }),
            &["task", "text"]
        ),
        tool(
            "set_status",
            "Move your task on (todo, doing, in_review, blocked), with a reason. in_review cites evidence for each criterion (a live one: a reading from this turn); the member one rank up re-checks it, which is how work becomes done. A routine's check you close as done yourself, with evidence, once you have looked. For done, why: met_criteria, or superseded (name the successor) or not_needed.",
            json!({ "task": task, "status": { "type": "string", "enum": ["todo", "doing", "in_review", "blocked", "done"] }, "reason": { "type": "string" }, "evidence": per_criterion("For in_review, or a check closed as done: what shows each criterion holds."), "why": { "type": "string", "enum": ["met_criteria", "superseded", "not_needed"], "description": "for done: why it is done" }, "successor": { "type": "integer", "description": "for superseded: the task that carries the work on, which superseded needs" } }),
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
            "Put a decision on the owner's desk, citing evidence: only what the owner alone decides (money beyond the caps, credentials, accepting a risk, an irreversible step, or taste). Asking ends your turn; you are woken with the answer. Everything else you decide yourself; work that keeps failing goes a rank up on its own.",
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
        "description": "Your verdict on work in review, one rank up, after re-checking it yourself: take your own readings to confirm each piece of its evidence is real and still true. approve passes it on your evidence per criterion, with at least one reading of your own this turn; send_back returns it to its owner with exactly what does not hold. Sending back is a finished review too. Never your own work.",
        "inputSchema": { "type": "object", "properties": { "task": task, "verdict": { "type": "string", "enum": ["approve", "send_back"] }, "notes": { "type": "string", "description": "what you checked, or what does not hold" }, "reason": reason_schema(), "evidence": per_criterion("For approve: your readings and what else shows each criterion holds.") }, "required": ["task", "verdict", "notes"] },
    })
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
    // In the loops, the member one rank up reviews (ADR 0142): its review turn has the tool.
    let reviewing = engine
        .store()
        .wake(run.wake)
        .ok()
        .flatten()
        .is_some_and(|w| w.reason == crate::domain::WakeReason::ReviewRequested);
    if reviewing {
        if let Some(task) = run.task.and_then(|t| engine.store().task(t).ok().flatten()) {
            if engine
                .loop_reviewer_of(&task)
                .ok()
                .flatten()
                .is_some_and(|r| r.id == member.id)
            {
                list.push(review_tool());
            }
        }
    }
    // A scenario's turn acts in its world with its tool (the Holodeck plan's phases 5 and 6).
    if let Ok(Some(kind)) = engine.scenario_kind_of_run(run.id) {
        list.push(super::scenario_tool::tool(kind));
    }
    // The check after a deploy is offered its four tools alone (ADR 0115, amended); a turn on
    // the work of a project that lists its tools, those four and the list (the Holodeck plan's
    // phase 4).
    if engine.lab_tools_of(run.id).ok().flatten().is_some() {
        let given = if engine.in_deploy_check(run.id).unwrap_or(true) {
            Vec::new()
        } else {
            engine
                .lab_tools_of(run.id)
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        if !given.iter().any(|g| g == crate::domain::projects::TEAM) {
            list.retain(|t| {
                t["name"].as_str().is_some_and(|n| {
                    crate::domain::deploy::HQ_TOOLS.contains(&n) || given.iter().any(|g| g == n)
                })
            });
        }
    }
    all
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
    // Preserve the worker's first-line identity contract. Version guidance follows
    // the briefing, while the drain notice below remains the final instruction.
    let answer = if tool == "my_turn" {
        answer.map(|mut text| {
            text.push_str("\nEndora team protocol v2: one team of loops. Work the task, check it, and ask_owner only for the owner's decisions; three failures take the work a rank up.\n");
            text
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

fn call_tool<S: TeamStore, W: WorkerAdapter, C: Clock>(
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
    let refused = |reason: String| -> Result<String, String> { Err(reason) };
    // Downtime makes no work (ADR 0119): the tools that would are refused in its turns.
    if MAKES_WORK.contains(&tool) && engine.in_downtime(run.id).map_err(|e| said(&e))? {
        return refused(Refusal::InDowntime.to_string());
    }
    // The check after a deploy reads the lab and closes its check, and nothing more (ADR 0115,
    // amended): refused here, whatever the model was told.
    if let Some(refusal) = engine
        .deploy_check_refuses(run.id, tool, args)
        .map_err(|e| said(&e))?
    {
        return refused(refusal);
    }
    // A scenario is closed by its world, never by a member's word (the Holodeck plan's phase 5).
    if let Some(refusal) = engine.scenario_refuses(tool, args).map_err(|e| said(&e))? {
        return refused(refusal);
    }
    // A project that lists its tools gives its work those and the turn's own four (the
    // Holodeck plan's phase 4): refused here too, whatever the model was told.
    if let Some(refusal) = engine.project_refuses(run.id, tool).map_err(|e| said(&e))? {
        return refused(refusal);
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
        "my_turn" => {
            let mut out = run.told.clone();
            if let Some(task) = run.task {
                let thread = engine
                    .latest_task_posts(run.id, task, 12)
                    .map_err(|e| e.to_string())?;
                let members = engine.store().members().map_err(|e| e.to_string())?;
                if !thread.is_empty() {
                    out.push_str("\n\nThe thread, newest last:\n");
                    for input in &thread {
                        let p = &input.post;
                        let line = Engine::<S, W, C>::post_line(
                            &members,
                            &p.author,
                            &p.text,
                            p.unsupported,
                        );
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
                            return Err(format!(
                                "there is no task #{} to carry the work on",
                                next.0
                            ));
                        }
                    }
                    Some((closure, successor))
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
            let reason = reason_arg(args, !approve)?;
            let cited = evidence_lists(args)?;
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
        "add_to_map" => {
            let line = text_arg(args, "line")?;
            engine.add_to_map(run.id, line).map_err(|e| said(&e))
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
