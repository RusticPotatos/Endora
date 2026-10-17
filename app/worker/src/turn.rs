//! One turn of a team member (ADR 0102, phase 4), which is the worker's whole job. It opens
//! the team protocol on HQ with the turn's token, reads what the turn is for, and lets the
//! model work through the team's tools a round at a time, until it answers without calling
//! one or the rounds run out. Its last word goes in the task's thread.
//!
//! The token is the only credential here, for HQ and the node's model relay alike. A 401 or a
//! 402 from either means the turn is over (stopped, ended, or out of money), and the worker
//! does nothing more. Every tool call goes to HQ, which decides it by the team's rules: the
//! worker itself acts on nothing.

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

/// What a turn is taken with.
pub struct Setup {
    /// The turn's own token: the only credential the worker has.
    pub token: String,
    /// HQ's team protocol.
    pub mcp: String,
    /// The node's model relay, up to its `/v1`.
    pub model_url: String,
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
    let tools = functions(listed["tools"].as_array().map_or(&[][..], Vec::as_slice));
    let (briefing, refused) = hq.call("my_turn", &json!({}))?;
    if refused {
        return Err(Stopped::Failed(format!("HQ refused my_turn: {briefing}")));
    }
    let task = task_of(&briefing);
    let model = Model::new(setup);
    let mut messages = vec![
        json!({ "role": "system", "content": instructions(task.is_some()) }),
        json!({ "role": "user", "content": briefing }),
    ];
    for _ in 0..setup.most_rounds {
        let said = model.ask(&messages, &tools)?;
        let calls = said["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() {
            let answer = said["content"].as_str().unwrap_or("").trim();
            last_word(&mut hq, task, answer)?;
            return Ok(Taken::Answered);
        }
        messages
            .push(json!({ "role": "assistant", "content": said["content"], "tool_calls": calls }));
        for call in &calls {
            let (content, asked) = run(&mut hq, call)?;
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
/// whether its last word goes in the task's thread for it.
fn instructions(posted: bool) -> String {
    let mut told = String::from(
        "You are one member of a small team at Endora HQ, taking one turn of work. The first \
         message is what my_turn says: who you are, your task, why it exists, and what is new \
         in its thread.\n\
         \n\
         How to take a good turn:\n\
         - Use only the team's tools. They are your only way to act: nothing you merely say \
         is done.\n\
         - Claim your task before you work on it. If another member has it, leave it to them.\n\
         - Post in the task's thread what you did and what you found, so whoever comes next \
         can go on from there.\n\
         - Move the task on with set_status, and say why. Never try to set it to done: only \
         the gate does that.\n\
         - Create a sub-task only for a member below you, and only when the work needs one.\n\
         - Ask the owner with ask_owner only when you truly need them. Asking ends your \
         turn: you are woken when they have answered.\n\
         - Keep it short, and never say you did what you did not do.\n\
         - When your part is done, stop calling tools and reply in one or two sentences.",
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

/// Runs one tool call the model asked for, and gives what the model reads back, and whether
/// it put a question on the owner's desk (which ends the turn). A refusal, HQ's or the
/// worker's own (arguments it cannot read), is said as one, so that the model can do
/// something about it.
fn run(hq: &mut Hq<'_>, call: &Value) -> Result<(String, bool), Stopped> {
    let tool = call["function"]["name"].as_str().unwrap_or("");
    let args = match arguments(&call["function"]["arguments"]) {
        Ok(args) => args,
        Err(why) => return Ok((format!("Refused: {why}"), false)),
    };
    let (said, refused) = hq.call(tool, &args)?;
    if refused {
        return Ok((format!("Refused: {said}"), false));
    }
    Ok((said, tool == "ask_owner"))
}

/// Posts the model's last word in the task's thread. With no task, or no words, there is
/// nothing to post.
fn last_word(hq: &mut Hq<'_>, task: Option<i64>, answer: &str) -> Result<(), Stopped> {
    let Some(task) = task else {
        return Ok(());
    };
    if answer.is_empty() {
        return Ok(());
    }
    let (said, refused) = hq.call("post", &json!({ "task": task, "text": answer }))?;
    if refused {
        return Err(Stopped::Failed(format!(
            "HQ refused the last word for task {task}: {said}"
        )));
    }
    Ok(())
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
    fn the_instructions_read_as_a_paragraph_then_one_rule_a_line() {
        let told = instructions(true);
        let (opening, rules) = told.split_once("\n\nHow to take a good turn:\n").unwrap();
        assert!(!opening.contains('\n'), "{opening}");
        let rules: Vec<&str> = rules.lines().collect();
        assert_eq!(rules.len(), 8, "{told}");
        assert!(
            rules
                .iter()
                .all(|r| r.starts_with("- ") && !r.contains("  "))
        );
        assert!(!told.contains('\\'));
        assert!(told.ends_with("one or two sentences. That reply goes in your task's thread."));
        assert!(instructions(false).ends_with("one or two sentences."));
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
