//! The team protocol (spec section 6): the tools every worker uses, whatever its runtime,
//! served by HQ over MCP. A call is made by a turn, known by its token; the member is that
//! turn's, so nobody acts as anyone else, and the engine's rules decide every call (claiming,
//! ranks, only the gate sets `done`). What touches a real system is never here: it is a
//! request the gateway decides.

use serde_json::{Value, json};

use super::engine::{Author, Engine, EngineError};
use super::ports::{Clock, NewTask, Run, TeamStore, WorkerAdapter};
use crate::domain::{Member, Priority, Refusal, Role, RunStatus, TaskId, TaskStatus};

/// The tools a member of `role` is offered, as MCP lists them: name, what it does, and its
/// input. Only the reviewer is offered `review`. The CEO (ADR 0111) is offered the team's
/// tools but the gate and privileged actions, puts decisions to the board with `ask_owner`,
/// and keeps the house log and reads Endora's public records.
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
            "Write in a task's thread (yours, one under it, the one above it, or one where you were named): what you did, found or need. Mention a member by name to wake them.",
            json!({ "task": task, "text": { "type": "string" } }),
            &["task", "text"]
        ),
        tool(
            "set_status",
            "Move your task on (todo, doing, in_review, blocked), with a reason. Done is set only by the gate, except a routine's check: close that as done, with what you found, once you have looked.",
            json!({ "task": task, "status": { "type": "string", "enum": ["todo", "doing", "in_review", "blocked", "done"] }, "reason": { "type": "string" } }),
            &["task", "status", "reason"]
        ),
        tool(
            "create_task",
            "Create a sub-task of your task for another member (by name), with the tasks it waits on.",
            json!({ "title": { "type": "string" }, "description": { "type": "string" }, "assignee": { "type": "string", "description": "a member's name" }, "waits_on": { "type": "array", "items": { "type": "integer" } } }),
            &["title", "description"]
        ),
        tool(
            "ask_owner",
            "Ask a question or put a proposal about your task up the chain: to the lead, or the lead's to the CEO, in the task's thread (to the owner's desk only when the team has no CEO). Asking ends your turn: you are woken when they answer.",
            json!({ "text": { "type": "string" } }),
            &["text"]
        ),
        tool(
            "check_back",
            "Come back to your task later, once: after a fix, to confirm it held. You are woken on this task after the minutes you give (5 to 720), if it is still open.",
            json!({ "minutes": { "type": "integer", "minimum": 5, "maximum": 720 }, "why": { "type": "string", "description": "what you will look at when you come back" } }),
            &["minutes", "why"]
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
            list.push(tool(
                "review",
                "Your verdict on work in review, after checking every claim in its thread against your tools: approve what holds (it is done, unless it needs the gate), or send it back to its owner with exactly what does not hold. Never your own work.",
                json!({ "task": task, "verdict": { "type": "string", "enum": ["approve", "send_back"] }, "notes": { "type": "string", "description": "what you checked, or what does not hold" } }),
                &["task", "verdict", "notes"],
            ));
        }
    }
    if crate::domain::lessons::keeps_lessons(role) {
        if let Some(list) = all.as_array_mut() {
            list.push(tool(
                "note_lesson",
                "Keep a lesson for the whole team: one line, at most 300 characters, added to your task's goal's notes, which every member reads every turn. Use it when the owner corrects you, or you learn something about this house the team will need again. The owner sees and can edit it.",
                json!({ "lesson": { "type": "string", "maxLength": 300 } }),
                &["lesson"],
            ));
        }
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
    ];
    let mut list: Vec<Value> = all
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| keep.iter().any(|k| t["name"] == *k))
        .cloned()
        .collect();
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
        "Put a decision to the owner, who is the board: only what they alone decide (money beyond the pool, new goals and priorities, hiring or letting go a member, what only they can know). The reviewer checks it first. Asking ends your turn: you are woken when the board decides.",
        json!({
            "decision": part("what the board is to decide, as a question"),
            "recommendation": part("what you recommend, and the options"),
            "why": part("why, from what the team found"),
        }),
        &["decision", "recommendation", "why"],
    ));
    list.push(tool(
        "note_house",
        "Add one dated line to the house log, yours to keep, which you and the lead read every turn: what you learned about the house or decided, at most 500 characters.",
        json!({ "entry": { "type": "string", "maxLength": crate::domain::house::HOUSE_ENTRY_MOST } }),
        &["entry"],
    ));
    list.push(tool(
        "rewrite_house",
        "Rewrite the house log whole, compactly, keeping what still matters: at most 6,000 characters. Use it when note_house says the log is full.",
        json!({ "log": { "type": "string", "maxLength": crate::domain::house::HOUSE_LOG_MOST } }),
        &["log"],
    ));
    list.push(tool(
        "read_doc",
        "Read one of Endora's public decision records or plans: a path in docs/adr or docs/plans (that folder alone lists them), 20,000 characters a part.",
        json!({ "path": { "type": "string", "description": "e.g. docs/plans/endora-hq.md, or docs/adr to list the records" }, "part": { "type": "integer", "minimum": 1 } }),
        &["path"],
    ));
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
    match tool {
        "my_turn" => {
            let mut out = run.told.clone();
            if let Some(task) = run.task {
                let thread = engine.store().thread(task).map_err(|e| e.to_string())?;
                if !thread.is_empty() {
                    out.push_str("\n\nThe thread, newest last:\n");
                    for p in thread.iter().rev().take(12).rev() {
                        out.push_str(&format!("- {}: {}\n", p.author, p.text));
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
            engine.post_in(run.id, task, text).map_err(|e| said(&e))?;
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
            engine
                .set_status_in(run.id, task, to, reason)
                .map_err(|e| said(&e))?;
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
            let id = engine
                .add_task(
                    Author::Member(member.id),
                    NewTask {
                        goal,
                        parent: Some(parent),
                        title: text_arg(args, "title")?.to_owned(),
                        description: text_arg(args, "description")?.to_owned(),
                        assignee,
                        priority: Priority::Normal,
                        waits_on,
                    },
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
            match engine.ask_board(run.id, &decision).map_err(|e| said(&e))? {
                super::engine::Asked::Review(_) => Ok("It is with the reviewer, who checks it before it reaches the board, and this turn has ended: stop now. You are woken if it is sent back, or when the board decides.".to_owned()),
                _ => Ok("It is on the board's desk, and this turn has ended: stop now. You are woken when the board decides.".to_owned()),
            }
        }
        "note_house" => {
            let entry = text_arg(args, "entry")?;
            let line = engine.note_house(run.id, entry).map_err(|e| said(&e))?;
            Ok(format!("Noted in the house log: {line}"))
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
        "ask_owner" => {
            let text = text_arg(args, "text")?;
            match engine.ask_owner(run.id, text).map_err(|e| said(&e))? {
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
            match engine.review_in(run.id, task, approve, notes) {
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
            let line = engine.note_lesson(run.id, lesson).map_err(|e| said(&e))?;
            Ok(format!(
                "Noted in the goal's notes for the team, which every member reads: {line}"
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
