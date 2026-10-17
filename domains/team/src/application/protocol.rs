//! The team protocol (spec section 6): the tools every worker uses, whatever its runtime,
//! served by HQ over MCP. A call is made by a turn, known by its token; the member is that
//! turn's, so nobody acts as anyone else, and the engine's rules decide every call (claiming,
//! ranks, only the gate sets `done`). What touches a real system is never here: it is a
//! request the gateway decides.

use serde_json::{Value, json};

use super::engine::{Author, Engine, EngineError};
use super::ports::{Clock, NewTask, Run, TeamStore, WorkerAdapter};
use crate::domain::{Member, Priority, TaskId, TaskStatus};

/// The tools, as MCP lists them: name, what it does, and its input.
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
    json!([
        tool(
            "my_turn",
            "What this turn is for: your role, the task and why it exists, what is new, and its thread. Call it first.",
            json!({}),
            &[]
        ),
        tool(
            "claim",
            "Take a task as yours. Refused if another member has it.",
            json!({ "task": task }),
            &["task"]
        ),
        tool(
            "post",
            "Write in a task's thread: what you did, found or need. Mention a member by name to wake them.",
            json!({ "task": task, "text": { "type": "string" } }),
            &["task", "text"]
        ),
        tool(
            "set_status",
            "Move your task on (todo, doing, in_review, blocked), with a reason. Done is set only by the gate, never by a member.",
            json!({ "task": task, "status": { "type": "string", "enum": ["todo", "doing", "in_review", "blocked"] }, "reason": { "type": "string" } }),
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
            "Put a question or a proposal on the owner's desk, about your task. Then end your turn: you are woken when they have seen it.",
            json!({ "text": { "type": "string" } }),
            &["text"]
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
    ])
}

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
            engine.claim(member.id, task).map_err(|e| said(&e))?;
            Ok(format!("Task {} is yours.", task.0))
        }
        "post" => {
            let task = task_arg(args)?;
            let text = text_arg(args, "text")?;
            engine
                .post(Author::Member(member.id), task, text)
                .map_err(|e| said(&e))?;
            Ok("Posted.".to_owned())
        }
        "set_status" => {
            let task = task_arg(args)?;
            let to = args["status"]
                .as_str()
                .and_then(TaskStatus::from_name)
                .ok_or_else(|| "`status` must be todo, doing, in_review or blocked".to_owned())?;
            let reason = text_arg(args, "reason")?;
            engine
                .set_status(member.id, task, to, reason)
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
        "ask_owner" => {
            let text = text_arg(args, "text")?;
            engine.ask_owner(run.id, text).map_err(|e| said(&e))?;
            Ok("It is on the owner's desk. End your turn now; you are woken when they have seen it.".to_owned())
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
