//! One turn of a team member (ADR 0102, phase 4), which is the worker's whole job. It opens
//! the team protocol on HQ with the turn's token, reads what the turn is for, and lets the
//! model work through the team's tools a round at a time, until it answers without calling
//! one or the rounds run out. Its last word goes in the task's thread.
//!
//! The token is the only credential here, for HQ and the node alike. A 401 or a 402 from
//! either means the turn is over (stopped, ended, or out of money), and the worker does
//! nothing more. Beside the team's tools the model is offered the node's contracts (ADR 0106),
//! narrow abilities on the home lab. Each tool call goes where its tool came from, HQ or the
//! node, which decides it by its own rules: the worker itself acts on nothing.

use std::time::Duration;

use serde_json::{Value, json};

/// The MCP version the worker speaks.
const PROTOCOL: &str = "2025-06-18";

/// How long HQ has to answer a protocol call. It is on the same network, so anything slower
/// is a fault.
const HQ_ANSWERS_WITHIN: Duration = Duration::from_secs(60);

/// How long one model call may take: a local model on a small machine is slow.
const LONGEST_CALL: Duration = Duration::from_secs(10 * 60);

/// The most of a reason the worker says.
const SAYS_AT_MOST: usize = 300;

/// The most HQ keeps of a post in a thread (the protocol's own limit).
const POST_AT_MOST: usize = 4000;

/// The most HQ keeps of a question to the owner.
const ASK_AT_MOST: usize = 1200;

/// The most calls the line saying what a turn did names; the rest are counted.
const NAMES_AT_MOST: usize = 12;

/// What a turn is taken with.
pub struct Setup {
    /// The turn's own token: the only credential the worker has.
    pub token: String,
    /// HQ's team protocol.
    pub mcp: String,
    /// The node's model relay, up to its `/v1`.
    pub model_url: String,
    /// The node's contracts.
    pub contracts: String,
    /// The model the worker names.
    pub model: String,
    /// The most rounds of the model a turn may take.
    pub most_rounds: u32,
}

/// How a turn was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// The model answered without calling a tool.
    Answered,
    /// The rounds ran out with the model still calling tools.
    OutOfRounds,
    /// The model asked the owner, which ends the turn: HQ wakes the member again when the
    /// owner has seen it.
    AskedTheOwner,
}

/// Why a turn stopped short. The reason never carries the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// HQ or the relay answered 401 or 402: the turn's token opens nothing now, or its money
    /// is spent.
    Over(String),
    /// Anything else, said shortly.
    Failed(String),
}

/// Takes one turn.
///
/// # Errors
/// [`Stopped::Over`] when HQ or the relay ends the turn; [`Stopped::Failed`] when either cannot
/// be reached or answers in a way the worker cannot use.
pub fn take(setup: &Setup) -> Result<Taken, Stopped> {
    turn(setup).map_err(|stopped| match stopped {
        Stopped::Over(why) => Stopped::Over(quiet(&why, &setup.token)),
        Stopped::Failed(why) => Stopped::Failed(quiet(&why, &setup.token)),
    })
}

fn turn(setup: &Setup) -> Result<Taken, Stopped> {
    let mut hq = Hq::new(&setup.mcp, &setup.token);
    hq.open()?;
    let listed = hq.request("tools/list", json!({}))?;
    let mut offered: Vec<Value> = listed["tools"].as_array().cloned().unwrap_or_default();
    let node = Node::new(&setup.contracts, &setup.token);
    // A contract named like a team tool is left out: the name goes to HQ, as it always did.
    let contracts: Vec<Value> = node
        .list()?
        .into_iter()
        .filter(|c| !offered.iter().any(|t| t["name"] == c["name"]))
        .collect();
    let on_the_node: Vec<String> = contracts
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    // What only looks, as the node marks it: the line saying what the turn did tells a turn
    // that only read from one that acted.
    let mut did = Ledger {
        reads: contracts
            .iter()
            .filter(|c| c["kind"] == "read")
            .filter_map(|c| c["name"].as_str().map(str::to_owned))
            .chain(std::iter::once("my_turn".to_owned()))
            .collect(),
        calls: Vec::new(),
    };
    offered.extend(contracts);
    let tools = functions(&offered);
    let (briefing, refused) = hq.call("my_turn", &json!({}))?;
    if refused {
        return Err(Stopped::Failed(format!("HQ refused my_turn: {briefing}")));
    }
    let task = task_of(&briefing);
    // Claiming is the first step of every turn with a task, and a small model skips it: the
    // worker takes that step itself and tells the model what HQ said. A refusal (another
    // member has it) is news for the model, not a failure of the turn.
    let mut briefing = briefing;
    let mut answered = Answered::default();
    if let Some(task) = task {
        let claim = json!({ "task": task });
        let (said, _) = hq.call("claim", &claim)?;
        // A model that claims it again is answered from this, not sent to HQ twice.
        answered
            .given
            .insert(format!("claim {claim}"), said.clone());
        briefing = format!(
            "{}\n\nBefore your turn began, your task was claimed for you: {said}",
            briefing.trim_end()
        );
    }
    let model = Model::new(setup);
    let mut messages = vec![
        json!({ "role": "system", "content": instructions(task.is_some(), !on_the_node.is_empty()) }),
        json!({ "role": "user", "content": briefing }),
    ];
    for _ in 0..setup.most_rounds {
        let said = model.ask(&messages, &tools)?;
        let calls = said["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() {
            let answer = said["content"].as_str().unwrap_or("").trim();
            last_word(&mut hq, task, answer, &did)?;
            return Ok(Taken::Answered);
        }
        messages
            .push(json!({ "role": "assistant", "content": said["content"], "tool_calls": calls }));
        for call in &calls {
            let (content, asked) =
                run(&mut hq, &node, &on_the_node, &mut answered, &mut did, call)?;
            if asked {
                return Ok(Taken::AskedTheOwner);
            }
            messages
                .push(json!({ "role": "tool", "tool_call_id": call["id"], "content": content }));
        }
    }
    Ok(Taken::OutOfRounds)
}

/// How to take a good turn: the worker's own instructions, written for Endora. `posted` says
/// whether its last word goes in the task's thread for it; `contracts`, whether the node
/// offers it any.
fn instructions(posted: bool, contracts: bool) -> String {
    let mut told = String::from(
        "You are one member of a small team at Endora HQ, taking one turn of work. The first \
         message is what my_turn says: who you are, your task, why it exists, and what is new \
         in its thread. Read the thread: the owner may have told you something there.\n\
         \n\
         Take your turn in these steps:\n\
         1. Look: use the tools to find out what you need. You have no other way to see or do \
         anything. You cannot run commands, ping, open files or browse: never offer to.\n\
         2. Post in the task's thread what you found, briefly, so whoever comes next can go on \
         from there.\n\
         3. End the turn with exactly one of these: move the task on with set_status and say \
         why; create sub-tasks for members below you when the work needs them; or put a \
         concrete plan or question to the owner with ask_owner (asking ends your turn, and you \
         are woken when they answer).",
    );
    if contracts {
        told.push_str(
            "\n\nThe home lab tools are Endora's contracts:\n\
             - homelab_status and container_logs only look: use them freely to see what is \
             wrong and why.\n\
             - restart_container is a safe fix you may make on your own, within its limits (one \
             restart of a container an hour, three in all), and the owner is told: look and \
             diagnose first, and say why.\n\
             - After a fix, check back to confirm it held: call check_back with the minutes to \
             wait and what you will look at.\n\
             - Endora's other tools say whether they read or act; an action that needs the \
             owner's approval goes on their desk and ends your turn, and you are woken when they \
             decide.\n\
             - Anything else on the home lab is the owner's to decide: ask with ask_owner, and \
             never try to work around a refusal.",
        );
    }
    told.push_str(
        "\n\nNever set a task to done: only the gate does that, except a routine's check, which \
         you close as done once you have looked, saying what you found. Keep it short, and never say \
         you did what you did not do. When your part is done, stop calling tools and reply in \
         one or two sentences.",
    );
    if posted {
        told.push_str(" That reply goes in your task's thread.");
    }
    told
}

/// The task a turn is about, from HQ's briefing (what `my_turn` starts with): the number in
/// its line `Your task: <title> (#<task>, <status>, <priority>)`, which comes before the
/// description and the thread. `None` for a turn about no task, or a briefing not in that
/// shape (a title over two lines); the model is then left to post for itself.
///
/// A title written to look like that line's end can name another task: the last word then
/// lands in that task's thread, which the member could post in anyway.
fn task_of(briefing: &str) -> Option<i64> {
    let line = briefing.lines().find(|l| l.starts_with("Your task: "))?;
    let (_, inside) = line.strip_suffix(')')?.rsplit_once("(#")?;
    let mut parts = inside.split(", ");
    let task = parts.next()?.parse().ok()?;
    (parts.count() == 2).then_some(task)
}

/// The team's tools, as MCP lists them, offered to the model as OpenAI functions.
fn functions(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let parameters = match &tool["inputSchema"] {
                Value::Null => json!({ "type": "object", "properties": {} }),
                schema => schema.clone(),
            };
            json!({ "type": "function", "function": {
                "name": tool["name"],
                "description": tool["description"].as_str().unwrap_or(""),
                "parameters": parameters,
            } })
        })
        .collect()
}

/// A tool call's arguments. In OpenAI's format they are a JSON object written as text; an
/// object is taken as it is (some servers send one), and empty text as no arguments.
fn arguments(given: &Value) -> Result<Value, String> {
    let parsed = match given {
        Value::String(text) if text.trim().is_empty() => return Ok(json!({})),
        Value::String(text) => serde_json::from_str(text).map_err(|e| {
            format!("the arguments are not JSON ({e}); send them again as one JSON object")
        })?,
        other => other.clone(),
    };
    if !parsed.is_object() {
        return Err("the arguments must be one JSON object".to_owned());
    }
    Ok(parsed)
}

/// What the model is told when it asks again for what it already has.
const ALREADY: &str = "You already have this result; use it, or do something else";

/// The calls a turn has made, by tool and arguments, and what each last said: so the same
/// call asked again with nothing new is answered from it. A small model will otherwise ask
/// for the same thing round after round.
#[derive(Default)]
struct Answered {
    /// The call made last.
    last: Option<String>,
    /// What each call last said.
    said: std::collections::HashMap<String, String>,
    /// What the worker already asked for the model before its first round (the claim): the
    /// model asking the same is told what HQ said, once, without asking HQ again.
    given: std::collections::HashMap<String, String>,
}

/// Runs one tool call the model asked for, and gives what the model reads back, and whether
/// it put a question on the owner's desk (which ends the turn). A refusal, HQ's or the
/// worker's own (arguments it cannot read), is said as one, so that the model can do
/// something about it. The same call asked again straight after is not sent at all; asked
/// again after something else, it is sent, and if it says just what it said before the model
/// is told it already has that.
fn run(
    hq: &mut Hq<'_>,
    node: &Node<'_>,
    on_the_node: &[String],
    answered: &mut Answered,
    did: &mut Ledger,
    call: &Value,
) -> Result<(String, bool), Stopped> {
    let tool = call["function"]["name"].as_str().unwrap_or("");
    let args = match arguments(&call["function"]["arguments"]) {
        Ok(args) => args,
        Err(why) => {
            did.unreadable(tool);
            return Ok((format!("Refused: {why}"), false));
        }
    };
    let key = format!("{tool} {args}");
    if let Some(said) = answered.given.remove(&key) {
        answered.said.insert(key.clone(), said.clone());
        answered.last = Some(key);
        return Ok((said, false));
    }
    if answered.last.as_deref() == Some(key.as_str()) && answered.said.contains_key(&key) {
        return Ok((ALREADY.to_owned(), false));
    }
    // A question to the owner carries what the turn actually did, so the reviewer and the
    // owner read the truth beside the claim.
    let sent = if tool == "ask_owner" {
        with_what_was_done(&args, did)
    } else {
        args.clone()
    };
    let (said, outcome, approval) = if on_the_node.iter().any(|c| c == tool) {
        node.call(tool, &sent)?
    } else {
        let (said, refused) = hq.call(tool, &sent)?;
        let outcome = if refused {
            Outcome::Refused
        } else {
            Outcome::Done
        };
        (said, outcome, false)
    };
    if tool != "ask_owner" {
        did.note(tool, &args, outcome);
    }
    let refused = outcome != Outcome::Done;
    let said = if refused {
        format!("Refused: {said}")
    } else {
        said
    };
    // A question to the owner ends the turn, and so does an action the node put on the
    // owner's desk for approval (ADR 0108): the member is woken when the owner decides.
    let asked = !refused && (tool == "ask_owner" || approval);
    let nothing_new = answered.said.get(&key) == Some(&said);
    answered.said.insert(key.clone(), said.clone());
    answered.last = Some(key);
    if nothing_new && !asked {
        return Ok((ALREADY.to_owned(), false));
    }
    Ok((said, asked))
}

/// Posts the model's last word in the task's thread, with a line saying what the turn
/// actually did beside it. With no task, or no words, there is nothing to post.
fn last_word(
    hq: &mut Hq<'_>,
    task: Option<i64>,
    answer: &str,
    did: &Ledger,
) -> Result<(), Stopped> {
    let Some(task) = task else {
        return Ok(());
    };
    if answer.is_empty() {
        return Ok(());
    }
    let text = beside(answer, &did.line(), POST_AT_MOST);
    let (said, refused) = hq.call("post", &json!({ "task": task, "text": text }))?;
    if refused {
        return Err(Stopped::Failed(format!(
            "HQ refused the last word for task {task}: {said}"
        )));
    }
    Ok(())
}

/// How a tool call came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// It did what it was asked.
    Done,
    /// HQ, the node or the worker said no.
    Refused,
    /// It could not be done: the node unreachable, or failing.
    Failed,
}

/// The tool calls a turn actually made, and how each came back: what the line beside its
/// claims is written from. A model may say it restarted something it only looked at; this
/// is what the audit would show.
struct Ledger {
    /// The tools that only look.
    reads: Vec<String>,
    /// Each call made, in order: what it was, whether it only looked, and how it came back.
    calls: Vec<(String, bool, Outcome)>,
}

impl Ledger {
    /// Notes a call that was made.
    fn note(&mut self, tool: &str, args: &Value, outcome: Outcome) {
        let (done, tried) = what_a_call_is(tool, args);
        let said = match outcome {
            Outcome::Done => done,
            Outcome::Refused => format!("tried to {tried}, refused"),
            Outcome::Failed => format!("tried to {tried}, failed"),
        };
        let read = self.reads.iter().any(|r| r == tool);
        self.calls.push((said, read, outcome));
    }

    /// Notes a call whose arguments could not be read, so it was never sent.
    fn unreadable(&mut self, tool: &str) {
        let read = self.reads.iter().any(|r| r == tool);
        self.calls.push((
            format!("tried to call {tool} with arguments that could not be read, refused"),
            read,
            Outcome::Refused,
        ));
    }

    /// The calls, one after another, the first [`NAMES_AT_MOST`] by name; `None` for none.
    fn list(&self) -> Option<String> {
        if self.calls.is_empty() {
            return None;
        }
        let mut named: Vec<&str> = self
            .calls
            .iter()
            .take(NAMES_AT_MOST)
            .map(|(said, _, _)| said.as_str())
            .collect();
        let more = self.calls.len().saturating_sub(NAMES_AT_MOST);
        let and_more = format!("and {more} more");
        if more > 0 {
            named.push(&and_more);
        }
        Some(named.join("; "))
    }

    /// Whether every call only looked.
    fn only_read(&self) -> bool {
        self.calls.iter().all(|(_, read, _)| *read)
    }

    /// The line under a turn's last word.
    fn line(&self) -> String {
        let Some(list) = self.list() else {
            return "This turn made no tool calls.".to_owned();
        };
        if self.only_read() {
            return format!("This turn did nothing but read: {list}.");
        }
        format!("This turn did: {list}.")
    }

    /// The line under a question to the owner.
    fn for_the_owner(&self) -> String {
        let Some(list) = self.list() else {
            return "What I actually did this turn: no tool calls.".to_owned();
        };
        if self.only_read() {
            return format!("What I actually did this turn: nothing but read: {list}.");
        }
        format!("What I actually did this turn: {list}.")
    }
}

/// A call in plain words: as done, and as tried. The tool is named, with its main argument.
fn what_a_call_is(tool: &str, args: &Value) -> (String, String) {
    let arg = |key: &str| -> String {
        let text = match &args[key] {
            Value::String(s) => s.trim().to_owned(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        text.chars().filter(|c| !c.is_control()).take(60).collect()
    };
    let both = |s: String| (s.clone(), s);
    match tool {
        "my_turn" => both("read what the turn is for".to_owned()),
        "homelab_status" => both("read the home lab's state".to_owned()),
        "container_logs" => both(format!("read {}'s log", arg("container"))),
        "restart_container" => (
            format!("restarted {}", arg("container")),
            format!("restart {}", arg("container")),
        ),
        "claim" => (
            format!("claimed task {}", arg("task")),
            format!("claim task {}", arg("task")),
        ),
        "post" => (
            format!("posted in task {}", arg("task")),
            format!("post in task {}", arg("task")),
        ),
        "set_status" => (
            format!("set task {} to {}", arg("task"), arg("status")),
            format!("set task {} to {}", arg("task"), arg("status")),
        ),
        "create_task" => (
            format!("created the task \"{}\"", arg("title")),
            format!("create the task \"{}\"", arg("title")),
        ),
        "check_back" => (
            format!("set a check-back in {} minutes", arg("minutes")),
            format!("set a check-back in {} minutes", arg("minutes")),
        ),
        _ => {
            let main = ["container", "item", "name", "id", "task", "query"]
                .iter()
                .map(|k| arg(k))
                .find(|v| !v.is_empty());
            let call = match main {
                Some(main) => format!("{tool} ({main})"),
                None => tool.to_owned(),
            };
            (format!("called {call}"), format!("call {call}"))
        }
    }
}

/// `text`, then a blank line and `line`, within `most` characters: the text is cut to make
/// room, never the line.
fn beside(text: &str, line: &str, most: usize) -> String {
    let room = most.saturating_sub(line.chars().count() + 2);
    let kept: String = text.trim().chars().take(room).collect();
    format!("{}\n\n{line}", kept.trim_end())
}

/// A question's arguments with what the turn actually did under its text.
fn with_what_was_done(args: &Value, did: &Ledger) -> Value {
    let mut sent = args.clone();
    if let Some(text) = args["text"].as_str() {
        sent["text"] = json!(beside(text, &did.for_the_owner(), ASK_AT_MOST));
    }
    sent
}

/// A reason as the worker may say it: never the token, and short. The token goes first, so
/// that cutting it short can never leave a piece of it.
fn quiet(why: &str, token: &str) -> String {
    let why = if token.is_empty() {
        why.to_owned()
    } else {
        why.replace(token, "[the turn's token]")
    };
    why.chars().take(SAYS_AT_MOST).collect()
}

/// An HTTP client that hands every status back to be read here, follows no redirect and goes
/// through no proxy: the token goes to the address it was given, or nowhere.
fn agent(within: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(within))
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .build()
        .into()
}

/// What HQ or the relay said with a refusal. Both put it in `error`: HQ as text, the relay as
/// OpenAI does, in its `message`.
fn what_it_said(body: &Value) -> String {
    let said = body["error"]
        .as_str()
        .or_else(|| body["error"]["message"].as_str())
        .unwrap_or("");
    if said.is_empty() {
        return String::new();
    }
    format!(": {said}")
}

/// Reads an answer from HQ or the relay (`who`): its JSON body (`null` when there is none), or
/// why the turn stops there.
fn read(
    who: &str,
    sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Value, Stopped> {
    let mut response =
        sent.map_err(|e| Stopped::Failed(format!("{who} could not be reached: {e}")))?;
    let status = response.status().as_u16();
    if matches!(status, 401 | 402) {
        return Err(Stopped::Over(format!("{who} answered {status}")));
    }
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Stopped::Failed(format!("{who}'s answer could not be read: {e}")))?;
    let body = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::Null)
    };
    if !(200..300).contains(&status) {
        return Err(Stopped::Failed(format!(
            "{who} answered {status}{}",
            what_it_said(&body)
        )));
    }
    if body.is_null() && !text.trim().is_empty() {
        return Err(Stopped::Failed(format!("{who}'s answer is not JSON")));
    }
    Ok(body)
}

/// HQ's team protocol: JSON-RPC over HTTP, opened by the turn's token.
struct Hq<'a> {
    agent: ureq::Agent,
    url: &'a str,
    token: &'a str,
    /// The last request's id.
    id: u64,
    /// The protocol version HQ agreed to, sent with every message after `initialize`.
    version: Option<String>,
}

impl<'a> Hq<'a> {
    fn new(url: &'a str, token: &'a str) -> Self {
        Self {
            agent: agent(HQ_ANSWERS_WITHIN),
            url,
            token,
            id: 0,
            version: None,
        }
    }

    fn send(&self, message: &Value) -> Result<Value, Stopped> {
        let mut request = self
            .agent
            .post(self.url)
            .header("authorization", &format!("Bearer {}", self.token))
            .header("accept", "application/json");
        if let Some(version) = &self.version {
            request = request.header("mcp-protocol-version", version);
        }
        read("HQ", request.send_json(message))
    }

    /// A request, and its result.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, Stopped> {
        self.id += 1;
        let reply = self.send(&json!({
            "jsonrpc": "2.0", "id": self.id, "method": method, "params": params,
        }))?;
        if let Some(message) = reply["error"]["message"].as_str() {
            return Err(Stopped::Failed(format!("HQ refused {method}: {message}")));
        }
        Ok(reply["result"].clone())
    }

    /// Opens the protocol: `initialize`, then the notification that the worker is ready.
    fn open(&mut self) -> Result<(), Stopped> {
        let agreed = self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": { "name": "endora-worker", "version": env!("CARGO_PKG_VERSION") },
            }),
        )?;
        let version = agreed["protocolVersion"].as_str().unwrap_or(PROTOCOL);
        self.version = Some(version.to_owned());
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        Ok(())
    }

    /// Calls a tool: what it said, and whether that was a refusal.
    fn call(&mut self, tool: &str, arguments: &Value) -> Result<(String, bool), Stopped> {
        let result = self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )?;
        let text: Vec<&str> = result["content"]
            .as_array()
            .map(|items| items.iter().filter_map(|i| i["text"].as_str()).collect())
            .unwrap_or_default();
        Ok((
            text.join("\n"),
            result["isError"].as_bool().unwrap_or(false),
        ))
    }
}

/// The node's contracts (ADR 0106): listed as MCP lists tools, each called with its arguments
/// as a JSON body, opened by the same token.
struct Node<'a> {
    agent: ureq::Agent,
    url: &'a str,
    token: &'a str,
}

impl<'a> Node<'a> {
    fn new(url: &'a str, token: &'a str) -> Self {
        Self {
            agent: agent(HQ_ANSWERS_WITHIN),
            url: url.trim_end_matches('/'),
            token,
        }
    }

    /// The contracts offered. A node that offers none, or cannot say, leaves the turn to the
    /// team's tools: only a 401 or a 402 ends it.
    fn list(&self) -> Result<Vec<Value>, Stopped> {
        let sent = self
            .agent
            .get(self.url)
            .header("authorization", &format!("Bearer {}", self.token))
            .call();
        match read("the node", sent) {
            Ok(listed) => Ok(listed["tools"].as_array().cloned().unwrap_or_default()),
            Err(Stopped::Over(why)) => Err(Stopped::Over(why)),
            Err(Stopped::Failed(why)) => {
                eprintln!("worker: no contracts this turn: {why}");
                Ok(Vec::new())
            }
        }
    }

    /// Calls a contract: what it said, how it came back (a 4xx is a refusal; a node that
    /// cannot be reached, or a 5xx, a failure), and whether the node put it on the owner's
    /// desk for approval, which ends the turn. Anything but a 401 or a 402 is the tool's own
    /// answer, for the model to read.
    fn call(&self, name: &str, arguments: &Value) -> Result<(String, Outcome, bool), Stopped> {
        let sent = self
            .agent
            .post(&format!("{}/{name}", self.url))
            .header("authorization", &format!("Bearer {}", self.token))
            .send_json(arguments);
        let mut response = match sent {
            Ok(response) => response,
            Err(e) => {
                return Ok((
                    format!("the node could not be reached: {e}"),
                    Outcome::Failed,
                    false,
                ));
            }
        };
        let status = response.status().as_u16();
        if matches!(status, 401 | 402) {
            return Err(Stopped::Over(format!("the node answered {status}")));
        }
        let body: Value = response
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok((
                body["text"].as_str().unwrap_or("").to_owned(),
                Outcome::Done,
                body["asked_owner"] == true,
            ));
        }
        let said = body["error"]
            .as_str()
            .map_or_else(|| format!("the node answered {status}"), str::to_owned);
        let outcome = if status >= 500 {
            Outcome::Failed
        } else {
            Outcome::Refused
        };
        Ok((said, outcome, false))
    }
}

/// The node's model relay: OpenAI's chat completions, opened by the same token.
struct Model<'a> {
    agent: ureq::Agent,
    url: String,
    token: &'a str,
    model: &'a str,
}

impl<'a> Model<'a> {
    fn new(setup: &'a Setup) -> Self {
        Self {
            agent: agent(LONGEST_CALL),
            url: format!("{}/chat/completions", setup.model_url.trim_end_matches('/')),
            token: &setup.token,
            model: &setup.model,
        }
    }

    /// One round: the conversation so far and the tools, and the model's message back.
    fn ask(&self, messages: &[Value], tools: &[Value]) -> Result<Value, Stopped> {
        let sent = self
            .agent
            .post(&self.url)
            .header("authorization", &format!("Bearer {}", self.token))
            .send_json(json!({ "model": self.model, "messages": messages, "tools": tools }));
        let reply = read("the model relay", sent)?;
        let message = &reply["choices"][0]["message"];
        if !message.is_object() {
            return Err(Stopped::Failed(
                "the model relay's answer has no message".to_owned(),
            ));
        }
        Ok(message.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use endora_team::application::{Author, Clock, Engine, NewTask, Settings};
    use endora_team::domain::{Priority, Role};
    use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};

    struct Noon;
    impl Clock for Noon {
        fn now(&self) -> i64 {
            1_792_166_400_000
        }
    }

    #[test]
    fn the_task_is_read_from_the_engines_own_briefing() {
        let hq = Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            SimulatedRunner::new(4),
            Noon,
            Settings::default(),
        );
        let dev = hq.hire("Jane", Role::Developer, 40, 2, 0).unwrap();
        let goal = hq
            .set_goal("Ship the watch tracker", "asked for", Author::Owner)
            .unwrap();
        // A title that looks like the line's own end, or a description that looks like the
        // line, does not fool it.
        for title in ["Add a list of watches", "Fix (#3, todo, normal) for good"] {
            hq.add_task(
                Author::Owner,
                NewTask {
                    goal,
                    parent: None,
                    title: title.to_owned(),
                    description: "Your task: not this (#99, todo, normal)".to_owned(),
                    assignee: Some(dev),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        }
        hq.tick().unwrap();
        let started = hq.runner().started();
        assert_eq!(started.len(), 2, "two slots, two turns");
        for turn in started {
            assert_eq!(task_of(&turn.told), turn.task.map(|t| t.0), "{}", turn.told);
        }
    }

    #[test]
    fn a_briefing_without_its_task_line_names_no_task() {
        assert_eq!(
            task_of(
                "You are Jane, the team's lead.\nThis turn is about no task: look over the team's work.\n"
            ),
            None
        );
        assert_eq!(
            task_of(
                "You are Jane, the team's developer.\nYour task: Add\na list (#7, todo, normal)\n"
            ),
            None,
            "a title over two lines is not guessed at"
        );
    }

    #[test]
    fn arguments_are_a_json_object_or_refused() {
        assert_eq!(
            arguments(&json!(r#"{"task": 7}"#)),
            Ok(json!({ "task": 7 }))
        );
        assert_eq!(arguments(&json!("")), Ok(json!({})), "no arguments at all");
        assert_eq!(arguments(&json!({ "task": 7 })), Ok(json!({ "task": 7 })));
        assert!(arguments(&json!(r#"{"task": "#)).is_err());
        assert!(arguments(&json!("[7]")).is_err());
        assert!(arguments(&Value::Null).is_err());
    }

    #[test]
    fn a_tool_with_no_input_schema_takes_an_empty_object() {
        assert_eq!(
            functions(&[json!({ "name": "my_turn" })]),
            [json!({ "type": "function", "function": {
                "name": "my_turn",
                "description": "",
                "parameters": { "type": "object", "properties": {} },
            } })]
        );
    }

    #[test]
    fn the_instructions_read_as_paragraphs_then_one_step_or_rule_a_line() {
        let told = instructions(true, false);
        let parts: Vec<&str> = told.split("\n\n").collect();
        assert_eq!(parts.len(), 3, "{told}");
        assert!(!parts[0].contains('\n'), "{told}");
        let steps: Vec<&str> = parts[1].lines().collect();
        assert_eq!(steps[0], "Take your turn in these steps:");
        assert_eq!(steps.len(), 4, "{told}");
        for (n, step) in steps[1..].iter().enumerate() {
            assert!(step.starts_with(&format!("{}. ", n + 1)), "{step}");
        }
        assert!(told.contains("You cannot run commands"));
        assert!(!told.contains('\\') && !told.contains("  "), "{told}");
        assert!(told.ends_with("one or two sentences. That reply goes in your task's thread."));
        assert!(instructions(false, false).ends_with("one or two sentences."));
        // With the node's contracts, a paragraph more: what they may do, and what is the
        // owner's, one rule a line.
        let told = instructions(false, true);
        let parts: Vec<&str> = told.split("\n\n").collect();
        assert_eq!(parts.len(), 4, "{told}");
        let rules: Vec<&str> = parts[2].lines().collect();
        assert_eq!(rules.len(), 6, "{told}");
        assert!(told.contains("owner's approval"), "{told}");
        assert!(rules[1..].iter().all(|r| r.starts_with("- ")));
        assert!(told.contains("restart_container"));
        assert!(
            told.contains("After a fix, check back to confirm it held"),
            "{told}"
        );
        assert!(told.contains("a routine's check"), "{told}");
        assert!(!told.contains("  "), "{told}");
    }

    #[test]
    fn a_reason_never_carries_the_token_not_even_cut_short() {
        let token = "secret-token-of-this-turn";
        let why = format!("{}Bearer {token}", "x".repeat(SAYS_AT_MOST - 10));
        let said = quiet(&why, token);
        assert!(!said.contains("sec"), "{said}");
        assert!(said.chars().count() <= SAYS_AT_MOST);
        assert_eq!(quiet("HQ answered 500", ""), "HQ answered 500");
    }
}
