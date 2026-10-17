//! The worker as the runner runs it: its own process, its environment and its exit code,
//! against a fake HQ and a fake model relay on local ports.

use std::io::{Read as _, Write as _};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// A turn's token, for these tests only.
const TOKEN: &str = "a-turns-token-for-these-tests-and-nothing-else";

/// What HQ's `my_turn` says.
const BRIEFING: &str = "You are Jane, the team's developer.\n\
Your task: Add a list of watches (#7, todo, normal)\n\
List what is being watched.\n\
Why it exists: Add a list of watches ← goal: Ship the watch tracker\n\
Nothing new in the thread since your last turn.\n";

/// One request a fake server was sent.
#[derive(Debug, Clone)]
struct Sent {
    path: String,
    /// Lower-cased names.
    headers: Vec<(String, String)>,
    body: Value,
}

impl Sent {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// `tools/call claim`, or just the method when it is not a tool call.
    fn method(&self) -> String {
        let method = self.body["method"].as_str().unwrap_or("");
        match self.body["params"]["name"].as_str() {
            Some(tool) => format!("{method} {tool}"),
            None => method.to_owned(),
        }
    }
}

type Seen = Arc<Mutex<Vec<Sent>>>;
/// Answers a request, given it and how many came before it: a status and a body.
type Answer = Box<dyn Fn(&Sent, usize) -> (u16, String) + Send>;

/// A tiny HTTP server on a local port that answers each request with `answer`.
fn fake(answer: Answer) -> (String, Seen) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut head = Vec::new();
            let mut byte = [0u8];
            while !head.ends_with(b"\r\n\r\n") {
                if stream.read_exact(&mut byte).is_err() {
                    break;
                }
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let mut lines = head.lines();
            let path = lines
                .next()
                .unwrap_or("")
                .split(' ')
                .nth(1)
                .unwrap_or("")
                .to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|l| l.split_once(": "))
                .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
                .collect();
            let length: usize = headers
                .iter()
                .find(|(k, _)| k == "content-length")
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(0);
            let mut body = vec![0u8; length];
            let _ = stream.read_exact(&mut body);
            let sent = Sent {
                path,
                headers,
                body: serde_json::from_slice(&body).unwrap_or(Value::Null),
            };
            let before = kept.lock().unwrap().len();
            let (status, reply) = answer(&sent, before);
            kept.lock().unwrap().push(sent);
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Answered\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (url, seen)
}

/// The team's tools, as HQ lists them (a few of them).
fn tools() -> Value {
    let task = json!({ "type": "integer", "description": "the task's number" });
    json!([
        { "name": "my_turn", "description": "What this turn is for. Call it first.",
          "inputSchema": { "type": "object", "properties": {}, "required": [] } },
        { "name": "claim", "description": "Take a task as yours.",
          "inputSchema": { "type": "object", "properties": { "task": task }, "required": ["task"] } },
        { "name": "post", "description": "Write in a task's thread.",
          "inputSchema": { "type": "object", "properties": { "task": task, "text": { "type": "string" } },
                           "required": ["task", "text"] } },
        { "name": "set_status", "description": "Move your task on, with a reason.",
          "inputSchema": { "type": "object", "properties": { "task": task, "status": { "type": "string" },
                           "reason": { "type": "string" } }, "required": ["task", "status", "reason"] } },
    ])
}

/// HQ's team protocol: `my_turn` says [`BRIEFING`], `claim` and `post` work, and anything else
/// is refused. The tool named in `stopped` answers 401, as for a turn HQ has just stopped.
fn hq(stopped: Option<&'static str>) -> (String, Seen) {
    fake(Box::new(move |sent, _| {
        let id = sent.body["id"].clone();
        let tool = sent.body["params"]["name"].as_str().unwrap_or("");
        if stopped == Some(tool) {
            return (
                401,
                json!({ "error": "no running turn has that token" }).to_string(),
            );
        }
        let result = match sent.body["method"].as_str().unwrap_or("") {
            "initialize" => json!({
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "endora-hq", "version": "0" },
            }),
            m if m.starts_with("notifications/") => return (202, String::new()),
            "tools/list" => json!({ "tools": tools() }),
            "tools/call" => {
                let (text, refused) = match tool {
                    "my_turn" => (BRIEFING, false),
                    "claim" => ("Task 7 is yours.", false),
                    "post" => ("Posted.", false),
                    "ask_owner" => (
                        "It is on the owner's desk, and this turn has ended: stop now.",
                        false,
                    ),
                    _ => ("`status` must be todo, doing, in_review or blocked", true),
                };
                json!({ "content": [{ "type": "text", "text": text }], "isError": refused })
            }
            _ => {
                let error = json!({ "code": -32601, "message": "no such method" });
                return (
                    200,
                    json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string(),
                );
            }
        };
        (
            200,
            json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
        )
    }))
}

/// The model relay: its nth call answered with `replies[n]`, and the last one again after.
fn model(replies: Vec<(u16, Value)>) -> (String, Seen) {
    fake(Box::new(move |_, before| {
        let (status, body) = &replies[before.min(replies.len() - 1)];
        (*status, body.to_string())
    }))
}

/// A model's answer calling tools: each an id, a name and its arguments as the JSON text the
/// OpenAI format carries.
fn calls(list: &[(&str, &str, &str)]) -> Value {
    let calls: Vec<Value> = list
        .iter()
        .map(|(id, name, arguments)| {
            json!({ "id": id, "type": "function",
                    "function": { "name": name, "arguments": arguments } })
        })
        .collect();
    json!({ "choices": [{ "index": 0, "finish_reason": "tool_calls",
        "message": { "role": "assistant", "content": null, "tool_calls": calls } }] })
}

/// A model's answer in words, calling nothing.
fn says(text: &str) -> Value {
    json!({ "choices": [{ "index": 0, "finish_reason": "stop",
        "message": { "role": "assistant", "content": text } }] })
}

/// The node's contracts (ADR 0106): it lists `offered`, answers `homelab_status` and
/// `container_logs`, refuses a restart as its limits would (429), and answers `status` to every
/// call when given.
fn node(offered: Value, status: Option<u16>) -> (String, Seen) {
    fake(Box::new(move |sent, _| {
        if let Some(status) = status {
            return (
                status,
                json!({ "error": "no running turn has that token" }).to_string(),
            );
        }
        match sent.path.as_str() {
            "/team-relay/contracts" => (200, json!({ "tools": offered }).to_string()),
            "/team-relay/contracts/homelab_status" => (
                200,
                json!({ "text": "{\"stale\":false,\"services\":[]}" }).to_string(),
            ),
            "/team-relay/contracts/container_logs" => (
                200,
                json!({ "text": "10:00 started" }).to_string(),
            ),
            _ => (
                429,
                json!({ "error": "sonarr was not restarted: it was restarted less than an hour ago." })
                    .to_string(),
            ),
        }
    }))
}

/// Two of the node's contracts, as it lists them.
fn contracts() -> Value {
    json!([
        { "name": "homelab_status", "description": "Read the home lab's state.",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "restart_container", "description": "Restart one container.",
          "inputSchema": { "type": "object", "properties": { "container": { "type": "string" },
                           "why": { "type": "string" } }, "required": ["container", "why"] } },
    ])
}

/// The node's container contracts as it lists them, each marked read or act.
fn contracts_that_say_what_they_are() -> Value {
    json!([
        { "name": "homelab_status", "kind": "read", "description": "Read the home lab's state.",
          "inputSchema": { "type": "object", "properties": {} } },
        { "name": "container_logs", "kind": "read", "description": "Read a container's log.",
          "inputSchema": { "type": "object", "properties": { "container": { "type": "string" } },
                           "required": ["container"] } },
        { "name": "restart_container", "kind": "act", "description": "Restart one container.",
          "inputSchema": { "type": "object", "properties": { "container": { "type": "string" },
                           "why": { "type": "string" } }, "required": ["container", "why"] } },
    ])
}

/// The worker's environment, pointed at the fakes, with a node that offers no contracts.
fn usual(hq: &str, model: &str) -> Vec<(&'static str, String)> {
    let (node, _) = node(json!([]), None);
    with_node(hq, model, &node)
}

/// The worker's environment, pointed at the fakes and at `node` for the contracts.
fn with_node(hq: &str, model: &str, node: &str) -> Vec<(&'static str, String)> {
    vec![
        ("ENDORA_RUN_TOKEN", TOKEN.to_owned()),
        ("ENDORA_HQ_MCP", format!("{hq}/v1/team/mcp")),
        ("ENDORA_MODEL_URL", format!("{model}/team-relay/local/v1")),
        ("ENDORA_CONTRACTS", format!("{node}/team-relay/contracts")),
    ]
}

/// Runs the worker with only `env` in its environment.
fn worker(env: &[(&'static str, String)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_endora-worker"))
        .env_clear()
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap()
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn methods(seen: &Seen) -> Vec<String> {
    seen.lock().unwrap().iter().map(Sent::method).collect()
}

#[test]
fn a_turn_reads_my_turn_works_through_the_tools_and_posts_its_last_word() {
    let (hq_url, to_hq) = hq(None);
    let (model_url, to_model) = model(vec![
        (200, calls(&[("call_1", "claim", r#"{"task": 7}"#)])),
        (200, says("I took task 7 and listed what is watched.")),
    ]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    assert_eq!(
        methods(&to_hq),
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/call my_turn",
            "tools/call claim",
            "tools/call post",
        ]
    );
    let to_hq = to_hq.lock().unwrap().clone();
    assert!(to_hq.iter().all(|s| s.path == "/v1/team/mcp"));
    assert_eq!(to_hq[0].body["params"]["protocolVersion"], "2025-06-18");
    assert_eq!(to_hq[4].body["params"]["arguments"], json!({ "task": 7 }));
    assert_eq!(
        to_hq[5].body["params"]["arguments"],
        json!({ "task": 7, "text": "I took task 7 and listed what is watched.\n\nThis turn made no tool calls." }),
        "the last word goes in the task's thread, with what the turn did: the claim was the \
         worker's own, before the model's first round"
    );

    let to_model = to_model.lock().unwrap().clone();
    assert_eq!(to_model.len(), 2);
    let first = &to_model[0];
    assert_eq!(first.path, "/team-relay/local/v1/chat/completions");
    assert_eq!(first.body["model"], "local");
    assert_eq!(first.body["tools"].as_array().unwrap().len(), 4);
    assert_eq!(
        first.body["tools"][1],
        json!({ "type": "function", "function": {
            "name": "claim",
            "description": "Take a task as yours.",
            "parameters": tools()[1]["inputSchema"],
        } }),
        "an MCP tool, as an OpenAI function"
    );
    assert_eq!(first.body["messages"][0]["role"], "system");
    assert_eq!(
        first.body["messages"][1],
        json!({ "role": "user", "content": format!(
            "{}\n\nBefore your turn began, your task was claimed for you: Task 7 is yours.",
            BRIEFING.trim_end()
        ) }),
        "the briefing, with the claim the worker made before asking the model"
    );
    let then = &to_model[1].body["messages"];
    assert_eq!(then[2]["role"], "assistant");
    assert_eq!(then[2]["tool_calls"][0]["id"], "call_1");
    assert_eq!(
        then[3],
        json!({ "role": "tool", "tool_call_id": "call_1", "content": "Task 7 is yours." })
    );

    for sent in to_hq.iter().chain(to_model.iter()) {
        assert_eq!(
            sent.header("authorization"),
            Some(format!("Bearer {TOKEN}").as_str()),
            "the turn's token, as a bearer, to both"
        );
        assert!(!sent.body.to_string().contains(TOKEN), "and in no body");
    }
    assert!(!said(&out).contains(TOKEN), "and never printed");
}

#[test]
fn a_refusal_and_arguments_that_are_not_json_go_back_to_the_model() {
    let (hq_url, to_hq) = hq(None);
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[
                (
                    "call_1",
                    "set_status",
                    r#"{"task": 7, "status": "done", "reason": "it works"}"#,
                ),
                ("call_2", "post", r#"{"task": 7, "text": "#),
            ]),
        ),
        (200, says("Task 7 waits for the gate.")),
    ]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    let then = to_model.lock().unwrap()[1].body["messages"].clone();
    assert_eq!(
        then[3],
        json!({ "role": "tool", "tool_call_id": "call_1",
                "content": "Refused: `status` must be todo, doing, in_review or blocked" })
    );
    assert_eq!(then[4]["tool_call_id"], "call_2");
    assert!(
        then[4]["content"]
            .as_str()
            .unwrap()
            .starts_with("Refused: "),
        "{}",
        then[4]
    );
    let to_hq = to_hq.lock().unwrap().clone();
    let posts: Vec<&Value> = to_hq
        .iter()
        .filter(|s| s.method() == "tools/call post")
        .map(|s| &s.body["params"]["arguments"])
        .collect();
    assert_eq!(
        posts,
        [
            &json!({ "task": 7, "text": "Task 7 waits for the gate.\n\nThis turn did: tried to set \
            task 7 to done, refused; tried to call post with arguments that could not be read, \
            refused." })
        ],
        "the broken call never reached HQ; the last word did, with what came of each call"
    );
}

#[test]
fn a_turns_claims_sit_beside_what_it_actually_did() {
    // Live, a lead posted "Restarted owntone-spotify and owntone" in a turn that only read.
    let (hq_url, to_hq) = hq(None);
    let (node_url, _) = node(contracts_that_say_what_they_are(), None);
    let (model_url, _) = model(vec![
        (
            200,
            calls(&[
                ("call_1", "homelab_status", ""),
                ("call_2", "container_logs", r#"{"container": "owntone"}"#),
            ]),
        ),
        (200, says("Restarted owntone-spotify and owntone.")),
    ]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let last = to_hq
        .lock()
        .unwrap()
        .iter()
        .rfind(|s| s.method() == "tools/call post")
        .map(|s| s.body["params"]["arguments"]["text"].clone());
    assert_eq!(
        last,
        Some(json!(
            "Restarted owntone-spotify and owntone.\n\nThis turn did nothing but read: read the \
             home lab's state; read owntone's log."
        ))
    );

    // An act refused by the node is said as tried and refused, never as done.
    let (hq_url, to_hq) = hq(None);
    let (node_url, _) = node(contracts_that_say_what_they_are(), None);
    let (model_url, _) = model(vec![
        (
            200,
            calls(&[
                ("call_1", "homelab_status", ""),
                (
                    "call_2",
                    "restart_container",
                    r#"{"container": "sonarr", "why": "it is unhealthy"}"#,
                ),
            ]),
        ),
        (200, says("Restarted sonarr.")),
    ]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let last = to_hq
        .lock()
        .unwrap()
        .iter()
        .rfind(|s| s.method() == "tools/call post")
        .map(|s| s.body["params"]["arguments"]["text"].clone());
    assert_eq!(
        last,
        Some(json!(
            "Restarted sonarr.\n\nThis turn did: read the home lab's state; tried to restart \
             sonarr, refused."
        ))
    );
}

#[test]
fn a_question_to_the_owner_carries_what_the_turn_actually_did() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, _) = node(contracts_that_say_what_they_are(), None);
    let (model_url, _) = model(vec![
        (200, calls(&[("call_1", "homelab_status", "")])),
        (
            200,
            calls(&[(
                "call_2",
                "ask_owner",
                r#"{"text": "I restarted owntone; should it stay up?"}"#,
            )]),
        ),
    ]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let asked = to_hq
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.method() == "tools/call ask_owner")
        .map(|s| s.body["params"]["arguments"].clone());
    assert_eq!(
        asked,
        Some(
            json!({ "text": "I restarted owntone; should it stay up?\n\nWhat I actually did \
            this turn: nothing but read: read the home lab's state." })
        )
    );

    // With no calls at all, the question says so.
    let (hq_url, to_hq) = hq(None);
    let (model_url, _) = model(vec![(
        200,
        calls(&[(
            "call_1",
            "ask_owner",
            r#"{"text": "Shall I restart owntone?"}"#,
        )]),
    )]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let asked = to_hq
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.method() == "tools/call ask_owner")
        .map(|s| s.body["params"]["arguments"]["text"].clone());
    assert_eq!(
        asked,
        Some(json!(
            "Shall I restart owntone?\n\nWhat I actually did this turn: no tool calls."
        ))
    );
}

#[test]
fn a_402_from_the_relay_ends_the_turn_at_once() {
    let (hq_url, to_hq) = hq(None);
    let (model_url, to_model) = model(vec![(
        402,
        json!({ "error": { "message": "this turn's budget is spent" } }),
    )]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(3), "{}", said(&out));
    assert_eq!(to_model.lock().unwrap().len(), 1);
    assert_eq!(
        methods(&to_hq).last().map(String::as_str),
        Some("tools/call claim"),
        "nothing more is sent to HQ after the claim the worker makes before asking the model"
    );
}

#[test]
fn a_401_from_hq_mid_turn_ends_it_at_once() {
    let (hq_url, to_hq) = hq(Some("post"));
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[("call_1", "post", r#"{"task": 7, "text": "Looked."}"#)]),
        ),
        (200, says("Done.")),
    ]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(3), "{}", said(&out));
    assert_eq!(
        to_model.lock().unwrap().len(),
        1,
        "the model is not asked again"
    );
    assert_eq!(
        methods(&to_hq).last().map(String::as_str),
        Some("tools/call post")
    );
}

#[test]
fn asking_the_owner_ends_the_turn_there_and_exits_0() {
    let (hq_url, to_hq) = hq(None);
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[
                ("call_1", "ask_owner", r#"{"text": "Steel or titanium?"}"#),
                ("call_2", "claim", r#"{"task": 7}"#),
            ]),
        ),
        (200, says("Done.")),
    ]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(0), "a turn taken: {}", said(&out));
    assert_eq!(
        to_model.lock().unwrap().len(),
        1,
        "the model is not asked again"
    );
    assert_eq!(
        methods(&to_hq).last().map(String::as_str),
        Some("tools/call ask_owner"),
        "nothing after the question, not even a last word"
    );
}

#[test]
fn an_action_put_on_the_owners_desk_for_approval_ends_the_turn_as_a_question_does() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, to_node) = fake(Box::new(|sent, _| match sent.path.as_str() {
        "/team-relay/contracts" => (
            200,
            json!({ "tools": [{ "name": "sonarr_remove_and_search", "kind": "act",
                "description": "Remove a stuck download and search again.",
                "inputSchema": { "type": "object", "properties": {
                    "item": { "type": "integer" }, "why": { "type": "string" } },
                    "required": ["item", "why"] } }] })
            .to_string(),
        ),
        _ => (
            200,
            json!({ "text": "On the owner's desk for approval; you are woken when they decide.",
                    "asked_owner": true })
            .to_string(),
        ),
    }));
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[
                (
                    "call_1",
                    "sonarr_remove_and_search",
                    r#"{"item": 102, "why": "it is stuck"}"#,
                ),
                ("call_2", "claim", r#"{"task": 7}"#),
            ]),
        ),
        (200, says("Done.")),
    ]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "a turn taken: {}", said(&out));
    assert_eq!(
        to_model.lock().unwrap().len(),
        1,
        "the model is not asked again"
    );
    assert_eq!(
        to_node.lock().unwrap().last().map(|s| s.path.clone()),
        Some("/team-relay/contracts/sonarr_remove_and_search".to_owned())
    );
    assert!(
        !methods(&to_hq).contains(&"tools/call post".to_owned()),
        "nothing after it, not even a last word"
    );
}

#[test]
fn the_rounds_run_out() {
    let (hq_url, to_hq) = hq(None);
    let (model_url, to_model) = model(vec![(200, calls(&[("call_1", "claim", r#"{"task": 7}"#)]))]);
    let mut env = usual(&hq_url, &model_url);
    env.push(("ENDORA_MOST_ROUNDS", "3".to_owned()));
    let out = worker(&env);
    assert_eq!(out.status.code(), Some(0), "a turn taken: {}", said(&out));
    assert_eq!(to_model.lock().unwrap().len(), 3);
    let called = methods(&to_hq);
    assert_eq!(
        called.iter().filter(|m| *m == "tools/call claim").count(),
        1,
        "the same claim asked again is answered from what the turn has"
    );
    assert!(
        !called.contains(&"tools/call post".to_owned()),
        "no last word to post"
    );
}

#[test]
fn a_failure_exits_1_and_never_prints_the_token() {
    let (hq_url, _) = hq(None);
    // A relay that says back the token it was sent, early in its refusal.
    let (model_url, _) = fake(Box::new(|sent, _| {
        let echo = format!("you sent {}", sent.header("authorization").unwrap_or(""));
        (500, json!({ "error": { "message": echo } }).to_string())
    }));
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(1));
    let said = said(&out);
    assert!(said.contains("500"), "says what went wrong: {said}");
    assert!(!said.contains(TOKEN), "never the token: {said}");
}

#[test]
fn the_nodes_contracts_are_offered_beside_hqs_tools_and_called_on_the_node() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, to_node) = node(contracts(), None);
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[
                ("call_1", "homelab_status", ""),
                (
                    "call_2",
                    "restart_container",
                    r#"{"container": "sonarr", "why": "it is unhealthy"}"#,
                ),
            ]),
        ),
        (
            200,
            says("sonarr is unhealthy and was restarted within the hour; I told the owner."),
        ),
    ]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    let to_model = to_model.lock().unwrap().clone();
    let offered: Vec<&str> = to_model[0].body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    assert_eq!(
        offered,
        [
            "my_turn",
            "claim",
            "post",
            "set_status",
            "homelab_status",
            "restart_container"
        ]
    );
    let told = to_model[0].body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(told.contains("restart_container"), "{told}");
    let then = &to_model[1].body["messages"];
    assert_eq!(
        then[3],
        json!({ "role": "tool", "tool_call_id": "call_1", "content": "{\"stale\":false,\"services\":[]}" })
    );
    assert_eq!(
        then[4]["content"],
        "Refused: sonarr was not restarted: it was restarted less than an hour ago.",
        "a contract's refusal is a tool error"
    );

    let to_node = to_node.lock().unwrap().clone();
    let paths: Vec<&str> = to_node.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/team-relay/contracts",
            "/team-relay/contracts/homelab_status",
            "/team-relay/contracts/restart_container",
        ]
    );
    assert_eq!(to_node[1].body, json!({}));
    assert_eq!(
        to_node[2].body,
        json!({ "container": "sonarr", "why": "it is unhealthy" })
    );
    for sent in &to_node {
        assert_eq!(
            sent.header("authorization"),
            Some(format!("Bearer {TOKEN}").as_str())
        );
    }
    assert!(
        !methods(&to_hq)
            .iter()
            .any(|m| m.contains("homelab_status") || m.contains("restart")),
        "a contract never goes to HQ"
    );
}

#[test]
fn a_401_from_the_node_ends_the_turn_and_a_node_without_contracts_leaves_hqs_tools() {
    let (hq_url, _) = hq(None);
    let (node_url, _) = node(json!([]), Some(401));
    let (model_url, to_model) = model(vec![(200, says("Hello."))]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(3), "{}", said(&out));
    assert!(to_model.lock().unwrap().is_empty(), "the turn is over");

    let (hq_url, to_hq) = hq(None);
    let (node_url, _) = node(json!([]), Some(404));
    let (model_url, to_model) = model(vec![(200, says("Task 7 is underway."))]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(
        to_model.lock().unwrap()[0].body["tools"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        methods(&to_hq).last().map(String::as_str),
        Some("tools/call post")
    );
}

#[test]
fn the_same_call_again_with_nothing_new_is_answered_from_what_it_already_has() {
    let (hq_url, _) = hq(None);
    let (node_url, to_node) = node(contracts(), None);
    let status = ("homelab_status", "{}");
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[
                ("call_1", status.0, status.1),
                ("call_2", status.0, ""),
                ("call_3", status.0, " { } "),
            ]),
        ),
        (
            200,
            calls(&[
                (
                    "call_4",
                    "restart_container",
                    r#"{"container": "sonarr", "why": "unwell"}"#,
                ),
                ("call_5", status.0, status.1),
            ]),
        ),
        (200, says("Nothing more to do.")),
    ]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let already = "You already have this result; use it, or do something else";
    let first = to_model.lock().unwrap()[1].body["messages"].clone();
    assert_eq!(first[3]["content"], "{\"stale\":false,\"services\":[]}");
    assert_eq!(
        first[4]["content"], already,
        "asked again at once: not sent"
    );
    assert_eq!(first[5]["content"], already);
    let then = to_model.lock().unwrap()[2].body["messages"].clone();
    assert_eq!(
        then[8]["content"], already,
        "sent again after another call, but it says nothing new"
    );
    let paths: Vec<String> = to_node
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.path.clone())
        .collect();
    assert_eq!(
        paths,
        [
            "/team-relay/contracts",
            "/team-relay/contracts/homelab_status",
            "/team-relay/contracts/restart_container",
            "/team-relay/contracts/homelab_status",
        ]
    );
}

#[test]
fn without_a_token_there_is_no_turn() {
    let (hq_url, to_hq) = hq(None);
    let (model_url, _) = model(vec![(200, says("Hello."))]);
    let env: Vec<_> = usual(&hq_url, &model_url)
        .into_iter()
        .filter(|(k, _)| *k != "ENDORA_RUN_TOKEN")
        .collect();
    let out = worker(&env);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ENDORA_RUN_TOKEN"));
    assert!(to_hq.lock().unwrap().is_empty());
}

/// The worker's environment in the workbench (ring 1): the usual one, with its shell running
/// in `dir` instead of the sandbox's `/work`.
fn in_the_workbench(hq: &str, model: &str, dir: &std::path::Path) -> Vec<(&'static str, String)> {
    let mut env = usual(hq, model);
    env.push(("ENDORA_WORKBENCH", "1".to_owned()));
    env.push(("ENDORA_WORKBENCH_DIR", dir.display().to_string()));
    env.push(("PATH", std::env::var("PATH").unwrap_or_default()));
    env
}

/// The tools offered in the model's first round, by name.
fn offered(to_model: &Seen) -> Vec<String> {
    to_model.lock().unwrap()[0].body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn the_shell_is_offered_only_in_the_workbench() {
    let (hq_url, _) = hq(None);
    let (model_url, to_model) = model(vec![(200, says("Done."))]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(!offered(&to_model).contains(&"shell".to_owned()));
    let system = to_model.lock().unwrap()[0].body["messages"][0]["content"].clone();
    assert!(system.as_str().unwrap().contains("You cannot run commands"));

    // ENDORA_WORKBENCH set to anything but 1 is not the workbench.
    let (hq_url, _) = hq(None);
    let (model_url, to_model) = model(vec![(200, says("Done."))]);
    let mut env = usual(&hq_url, &model_url);
    env.push(("ENDORA_WORKBENCH", "yes".to_owned()));
    let out = worker(&env);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(!offered(&to_model).contains(&"shell".to_owned()));

    let (hq_url, _) = hq(None);
    let (model_url, to_model) = model(vec![(200, says("Done."))]);
    let out = worker(&in_the_workbench(
        &hq_url,
        &model_url,
        &std::env::temp_dir(),
    ));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(offered(&to_model).last().map(String::as_str), Some("shell"));
    let first = to_model.lock().unwrap()[0].body.clone();
    let shell = &first["tools"].as_array().unwrap().last().unwrap()["function"];
    assert_eq!(
        shell["parameters"]["required"],
        json!(["command", "why"]),
        "{shell}"
    );
    let system = first["messages"][0]["content"].as_str().unwrap().to_owned();
    assert!(!system.contains("You cannot run commands"), "{system}");
    assert!(
        system.contains("You have a real shell in a disposable sandbox"),
        "{system}"
    );
    assert!(
        system.contains("Anything you leave in /work is gone when the turn ends"),
        "{system}"
    );
    assert!(
        !system.contains("note_lesson"),
        "a member without note_lesson is not told to use it: {system}"
    );
}

#[test]
fn a_shell_call_runs_in_the_sandbox_and_the_turn_says_what_it_ran() {
    let dir = std::env::temp_dir().join(format!("endora-workbench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let (hq_url, to_hq) = hq(None);
    let long = format!("echo {}", "a".repeat(80));
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[
                (
                    "call_1",
                    "shell",
                    r#"{"command": "pwd -P; echo oops >&2; exit 2", "why": "to look"}"#,
                ),
                ("call_2", "shell", r#"{"command": "true"}"#),
            ]),
        ),
        (
            200,
            calls(&[(
                "call_3",
                "shell",
                &json!({ "command": long, "why": "to look", "seconds": 5 }).to_string(),
            )]),
        ),
        (200, says("I looked around the sandbox.")),
    ]);
    let out = worker(&in_the_workbench(&hq_url, &model_url, &dir));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    let to_model = to_model.lock().unwrap().clone();
    let then = &to_model[1].body["messages"];
    assert_eq!(
        then[3],
        json!({ "role": "tool", "tool_call_id": "call_1",
                "content": format!("exit 2\n{}\noops\n", dir.display()) })
    );
    assert_eq!(
        then[4]["content"], "Refused: say why you run it, in `why`",
        "a command without its reason is not run"
    );
    let then = &to_model[2].body["messages"];
    assert_eq!(then[6]["content"], format!("exit 0\n{}\n", "a".repeat(80)));

    let to_hq = to_hq.lock().unwrap().clone();
    assert!(
        to_hq.iter().all(|s| s.body["params"]["name"] != "shell"),
        "the shell never goes to HQ"
    );
    let last = to_hq
        .iter()
        .rfind(|s| s.method() == "tools/call post")
        .map(|s| s.body["params"]["arguments"]["text"].clone());
    assert_eq!(
        last,
        Some(json!(format!(
            "I looked around the sandbox.\n\nThis turn did: ran `pwd -P; echo oops >&2; exit 2` \
             (exit 2); tried to run `true`, refused; ran `echo {}…`.",
            "a".repeat(55)
        )))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A model's answer in Anthropic's Messages form: `text`, then each call a `tool_use` block.
fn claude(text: &str, list: &[(&str, &str, Value)]) -> Value {
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(json!({ "type": "text", "text": text }));
    }
    for (id, name, input) in list {
        content.push(json!({ "type": "tool_use", "id": id, "name": name, "input": input }));
    }
    let stop = if list.is_empty() {
        "end_turn"
    } else {
        "tool_use"
    };
    json!({ "type": "message", "role": "assistant", "model": "claude-sonnet-5-5",
            "content": content, "stop_reason": stop,
            "usage": { "input_tokens": 1000, "output_tokens": 100 } })
}

/// The worker's environment for a member thinking with Claude, as the runner gives it.
fn on_claude(hq: &str, relay: &str, node: &str) -> Vec<(&'static str, String)> {
    vec![
        ("ENDORA_RUN_TOKEN", TOKEN.to_owned()),
        ("ENDORA_HQ_MCP", format!("{hq}/v1/team/mcp")),
        ("ENDORA_MODEL_URL", format!("{relay}/team-relay/v1")),
        ("ENDORA_MODEL", "claude-sonnet-5-5".to_owned()),
        ("ENDORA_CONTRACTS", format!("{node}/team-relay/contracts")),
    ]
}

#[test]
fn a_cloud_brain_speaks_anthropics_messages_api_through_the_same_loop() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, to_node) = node(contracts_that_say_what_they_are(), None);
    let (relay_url, to_relay) = model(vec![
        (
            200,
            claude(
                "Looking first.",
                &[
                    ("toolu_1", "homelab_status", json!({})),
                    (
                        "toolu_2",
                        "set_status",
                        json!({ "task": 7, "status": "done", "reason": "fine" }),
                    ),
                ],
            ),
        ),
        (200, claude("", &[("toolu_3", "homelab_status", json!({}))])),
        (200, claude("Everything is running.", &[])),
    ]);
    let out = worker(&on_claude(&hq_url, &relay_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    let to_relay = to_relay.lock().unwrap().clone();
    assert_eq!(to_relay.len(), 3);
    let first = &to_relay[0];
    assert_eq!(first.path, "/team-relay/v1/messages");
    assert_eq!(
        first.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(first.header("anthropic-version"), Some("2023-06-01"));
    let body = &first.body;
    let mut fields: Vec<&String> = body.as_object().unwrap().keys().collect();
    fields.sort();
    assert_eq!(
        fields,
        ["max_tokens", "messages", "model", "system", "tools"],
        "only what the relay meters"
    );
    assert_eq!(body["model"], "claude-sonnet-5-5");
    assert_eq!(body["max_tokens"], 4096);
    assert!(
        body["system"]
            .as_str()
            .unwrap()
            .starts_with("You are one member of a small team")
    );
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["role"], "user");
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("your task was claimed for you")
    );
    assert_eq!(
        body["tools"][1],
        json!({ "name": "claim", "description": "Take a task as yours.",
                "input_schema": tools()[1]["inputSchema"] }),
        "an MCP tool, as an Anthropic tool"
    );
    assert!(
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t.get("kind").is_none() && t.get("type").is_none()),
        "the worker's own tools, nothing the relay would refuse"
    );

    // The model's turn goes back as it said it, and every tool_use gets its tool_result in
    // one user message, a refusal marked as an error.
    let second = &to_relay[1].body["messages"];
    assert_eq!(second[1]["role"], "assistant");
    assert_eq!(
        second[1]["content"],
        claude(
            "Looking first.",
            &[
                ("toolu_1", "homelab_status", json!({})),
                (
                    "toolu_2",
                    "set_status",
                    json!({ "task": 7, "status": "done", "reason": "fine" })
                ),
            ]
        )["content"]
    );
    assert_eq!(
        second[2],
        json!({ "role": "user", "content": [
            { "type": "tool_result", "tool_use_id": "toolu_1",
              "content": "{\"stale\":false,\"services\":[]}" },
            { "type": "tool_result", "tool_use_id": "toolu_2", "is_error": true,
              "content": "Refused: `status` must be todo, doing, in_review or blocked" },
        ] })
    );
    // The repeat guard is the same loop's: asked again after another call, it is sent, and
    // saying nothing new, the model is told it already has it.
    assert_eq!(
        to_relay[2].body["messages"][4]["content"][0]["content"],
        "You already have this result; use it, or do something else"
    );
    assert_eq!(
        to_node
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == "/team-relay/contracts/homelab_status")
            .count(),
        2
    );

    // The same last word and the same line saying what the turn did.
    let last = to_hq
        .lock()
        .unwrap()
        .iter()
        .rfind(|s| s.method() == "tools/call post")
        .map(|s| s.body["params"]["arguments"]["text"].clone());
    assert_eq!(
        last,
        Some(json!(
            "Everything is running.\n\nThis turn did: read the home lab's state; tried to set \
             task 7 to done, refused; read the home lab's state."
        ))
    );
    for sent in &to_relay {
        assert!(!sent.body.to_string().contains(TOKEN), "never in a body");
    }
}

#[test]
fn a_cloud_turn_ends_on_402_from_the_relay_and_on_a_question_to_the_owner() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, _) = node(json!([]), None);
    let (relay_url, to_relay) = model(vec![(
        402,
        json!({ "type": "error", "error": { "type": "billing_error",
                "message": "this turn's budget is spent" } }),
    )]);
    let out = worker(&on_claude(&hq_url, &relay_url, &node_url));
    assert_eq!(out.status.code(), Some(3), "{}", said(&out));
    assert_eq!(to_relay.lock().unwrap().len(), 1);
    assert_eq!(
        methods(&to_hq).last().map(String::as_str),
        Some("tools/call claim")
    );

    let (hq_url, to_hq) = hq(None);
    let (relay_url, to_relay) = model(vec![
        (
            200,
            claude(
                "",
                &[(
                    "toolu_1",
                    "ask_owner",
                    json!({ "text": "Shall I restart sonarr?" }),
                )],
            ),
        ),
        (200, claude("Done.", &[])),
    ]);
    let out = worker(&on_claude(&hq_url, &relay_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(to_relay.lock().unwrap().len(), 1, "not asked again");
    assert_eq!(
        methods(&to_hq).last().map(String::as_str),
        Some("tools/call ask_owner")
    );
}

#[test]
fn a_cloud_turn_runs_out_of_rounds_as_a_local_one_does() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, _) = node(json!([]), None);
    let (relay_url, to_relay) = model(vec![(
        200,
        claude("", &[("toolu_1", "claim", json!({ "task": 7 }))]),
    )]);
    let mut env = on_claude(&hq_url, &relay_url, &node_url);
    env.push(("ENDORA_MOST_ROUNDS", "2".to_owned()));
    let out = worker(&env);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(to_relay.lock().unwrap().len(), 2);
    assert!(!methods(&to_hq).contains(&"tools/call post".to_owned()));
}
