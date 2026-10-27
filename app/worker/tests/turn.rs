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
        { "name": "consult", "description": "Ask a colleague.",
          "inputSchema": { "type": "object", "properties": { "member": { "type": "string" },
                           "question": { "type": "string" } }, "required": ["member", "question"] } },
        { "name": "chess", "description": "Your game of chess.",
          "inputSchema": { "type": "object", "properties": { "action": { "type": "string" } }, "required": ["action"] } },
        { "name": "answer_assistance", "description": "Answer the request assigned to this turn.",
          "inputSchema": { "type": "object", "properties": { "answer": { "type": "string" } }, "required": ["answer"] } },
    ])
}

/// HQ's team protocol: `my_turn` says [`BRIEFING`], `claim` and `post` work, and anything else
/// is refused. The tool named in `stopped` answers 401, as for a turn HQ has just stopped.
fn hq(stopped: Option<&'static str>) -> (String, Seen) {
    hq_moving(stopped, false)
}

/// The same, and with `moves`, `set_status` does what it is asked, as HQ says it.
fn hq_moving(stopped: Option<&'static str>, moves: bool) -> (String, Seen) {
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
                    "set_status" if moves => ("Task 7 is moved on.", false),
                    "scenario" => (
                        "You fit a new bulb. The lamp lights. Solved in 20 minutes. The world has \
                         closed your task, and this turn has ended: stop now.",
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

#[test]
fn local_model_replies_are_explicitly_token_bounded() {
    let (hq_url, _) = hq(None);
    let (model_url, to_model) = model(vec![(200, says("The bounded turn finished."))]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let sent = to_model.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].body["max_tokens"], 8192,
        "local completion must be explicitly bounded (house models get twice the cloud's)"
    );
}

#[test]
fn an_empty_reply_at_the_token_limit_is_not_a_successful_turn() {
    let (hq_url, to_hq) = hq(None);
    let mut limited = says("");
    limited["choices"][0]["finish_reason"] = json!("length");
    let (model_url, to_model) = model(vec![(200, limited)]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(said(&out).contains("token limit"), "{}", said(&out));
    assert_eq!(to_model.lock().unwrap().len(), 1);
    assert!(
        !to_hq
            .lock()
            .unwrap()
            .iter()
            .any(|sent| sent.method() == "tools/call post"),
        "an empty limited reply must not become a task answer"
    );
}

#[test]
fn a_limited_reply_cannot_execute_even_a_complete_tool_call() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, to_node) = fake(Box::new(|sent, _| match sent.path.as_str() {
        "/team-relay/contracts" => (200, json!({"tools":[]}).to_string()),
        "/team-relay/transcript" => (200, json!({"kept":true}).to_string()),
        _ => (404, "{}".to_owned()),
    }));
    let mut limited = calls(&[(
        "limited-post",
        "post",
        r#"{"task":7,"text":"Partial model finding"}"#,
    )]);
    limited["choices"][0]["finish_reason"] = json!("length");
    let (model_url, to_model) = model(vec![(200, limited), (200, says("Done."))]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(said(&out).contains("token limit"), "{}", said(&out));
    assert_eq!(to_model.lock().unwrap().len(), 1);
    assert!(
        !to_hq
            .lock()
            .unwrap()
            .iter()
            .any(|sent| sent.method() == "tools/call post"),
        "limited model output must not execute a proposed tool call"
    );
    assert!(
        to_node.lock().unwrap().iter().any(|sent| {
            sent.path == "/team-relay/transcript"
                && sent.body["text"].as_str().is_some_and(|text| {
                    text.contains("--- call: post ---") && text.contains("Partial model finding")
                })
        }),
        "the limited proposal is retained for inspection"
    );
}

#[test]
fn a_claude_reply_at_its_token_limit_cannot_execute_a_tool_call() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, _) = node(json!([]), None);
    let mut limited = claude(
        "Partial finding",
        &[(
            "limited-post",
            "post",
            json!({"task":7,"text":"Partial finding"}),
        )],
    );
    limited["stop_reason"] = json!("max_tokens");
    let (model_url, to_model) = model(vec![(200, limited), (200, claude("Done.", &[]))]);
    let out = worker(&on_claude(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(said(&out).contains("token limit"), "{}", said(&out));
    assert_eq!(to_model.lock().unwrap().len(), 1);
    assert!(
        !to_hq
            .lock()
            .unwrap()
            .iter()
            .any(|sent| sent.method() == "tools/call post")
    );
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
        ("ENDORA_HQ_MCP", format!("{hq}/v2/team/mcp")),
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
    assert!(to_hq.iter().all(|s| s.path == "/v2/team/mcp"));
    assert_eq!(to_hq[0].body["params"]["protocolVersion"], "2025-06-18");
    assert_eq!(to_hq[4].body["params"]["arguments"], json!({ "task": 7 }));
    assert_eq!(
        to_hq[5].body["params"]["arguments"],
        json!({ "task": 7, "text": "I took task 7 and listed what is watched.\n\nThis turn made no tool calls.", "last_word": true }),
        "the last word goes in the task's thread, with what the turn did: the claim was the \
         worker's own, before the model's first round"
    );

    let to_model = to_model.lock().unwrap().clone();
    assert_eq!(to_model.len(), 2);
    let first = &to_model[0];
    assert_eq!(first.path, "/team-relay/local/v1/chat/completions");
    assert_eq!(first.body["model"], "local");
    assert_eq!(first.body["tools"].as_array().unwrap().len(), 7);
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
            refused.", "last_word": true })
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

// Run 1831 set its task blocked, then called set_status six times more, to "already have
// this" and "blocked to blocked": the instructions end a turn with set_status, and the loop
// went on. Moving its own task on to in_review, blocked or done ends the turn there.
#[test]
fn moving_its_own_task_on_ends_the_turn_there_and_exits_0() {
    for to in ["in_review", "blocked", "done"] {
        let (hq_url, to_hq) = hq_moving(None, true);
        let (model_url, to_model) = model(vec![
            (
                200,
                calls(&[
                    (
                        "call_1",
                        "set_status",
                        &format!(r#"{{"task": 7, "status": "{to}", "reason": "r1 shows it"}}"#),
                    ),
                    (
                        "call_2",
                        "set_status",
                        r#"{"task": 7, "status": "blocked", "reason": "again"}"#,
                    ),
                ]),
            ),
            (200, says("Done.")),
        ]);
        let (node_url, to_node) = node(json!([]), None);
        let out = worker(&with_node(&hq_url, &model_url, &node_url));
        assert_eq!(out.status.code(), Some(0), "{to}: {}", said(&out));
        assert_eq!(
            to_model.lock().unwrap().len(),
            1,
            "{to}: the model is not asked again"
        );
        let sent = methods(&to_hq);
        assert_eq!(
            sent.iter()
                .filter(|m| *m == "tools/call set_status")
                .count(),
            1,
            "{to}: nothing after it, not even a last word: {sent:?}"
        );
        assert_eq!(
            sent.last().map(String::as_str),
            Some("tools/call set_status")
        );
        // What HQ answered is in the turn's transcript, as every other call's is: live, the
        // turns that ended on set_status were kept with the call and no reply.
        let told = to_node.lock().unwrap().clone();
        let transcript = told
            .iter()
            .find(|s| s.path == "/team-relay/transcript")
            .expect("the transcript is told");
        let text = transcript.body["text"].as_str().unwrap();
        assert!(
            text.contains("--- result ---\nTask 7 is moved on."),
            "{to}: {text}"
        );
    }
}

// A reply from HQ that says the turn has ended ends it, whatever the tool: the world's last
// word when a scenario is over, as the drain's wrap-up already said it. Live, three of five
// scenario turns went on to set_status once the world had closed their task.
#[test]
fn hq_saying_the_turn_has_ended_ends_it_there() {
    let (hq_url, to_hq) = hq_moving(None, true);
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[
                (
                    "call_1",
                    "scenario",
                    r#"{"action": "replace_part", "part": "bulb"}"#,
                ),
                (
                    "call_2",
                    "set_status",
                    r#"{"task": 7, "status": "done", "reason": "lit"}"#,
                ),
            ]),
        ),
        (200, says("Done.")),
    ]);
    let out = worker(&usual(&hq_url, &model_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(
        to_model.lock().unwrap().len(),
        1,
        "the model is not asked again"
    );
    assert_eq!(
        methods(&to_hq).last().map(String::as_str),
        Some("tools/call scenario")
    );
}

#[test]
fn starting_work_moving_a_sub_task_or_being_refused_does_not_end_the_turn() {
    for (moves, call) in [
        (true, r#"{"task": 7, "status": "doing", "reason": "on it"}"#),
        (
            true,
            r#"{"task": 9, "status": "in_review", "reason": "the sub-task is ready"}"#,
        ),
        (
            false,
            r#"{"task": 7, "status": "blocked", "reason": "waiting"}"#,
        ),
    ] {
        let (hq_url, _) = hq_moving(None, moves);
        let (model_url, to_model) = model(vec![
            (200, calls(&[("call_1", "set_status", call)])),
            (200, says("Done.")),
        ]);
        let out = worker(&usual(&hq_url, &model_url));
        assert!(out.status.success(), "{call}: {}", said(&out));
        assert_eq!(
            to_model.lock().unwrap().len(),
            2,
            "{call}: the turn goes on"
        );
    }
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
    // The last thing it does is tell the node what it said (ADR 0129), after the action.
    let paths: Vec<String> = to_node
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.path.clone())
        .collect();
    assert_eq!(
        paths.iter().rev().nth(1),
        Some(&"/team-relay/contracts/sonarr_remove_and_search".to_owned())
    );
    assert_eq!(
        paths.last().map(String::as_str),
        Some("/team-relay/transcript")
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
fn the_rounds_run_out_and_what_the_model_said_on_the_way_is_its_last_word() {
    // Live, a turn that ran out of rounds went silently: what it had found was in no thread,
    // and the next turn started again from the thread alone.
    let saying = |text: &str| {
        let mut said = calls(&[("call_1", "claim", r#"{"task": 7}"#)]);
        said["choices"][0]["message"]["content"] = json!(text);
        (200, said)
    };
    let (hq_url, to_hq) = hq(None);
    let (model_url, _) = model(vec![
        saying("Sonarr is down; claiming first."),
        saying(""),
        saying("Its log says the disk is full."),
    ]);
    let mut env = usual(&hq_url, &model_url);
    env.push(("ENDORA_MOST_ROUNDS", "3".to_owned()));
    let out = worker(&env);
    assert_eq!(out.status.code(), Some(0), "a turn taken: {}", said(&out));
    assert_eq!(methods(&to_hq).last().unwrap(), "tools/call post");
    let posted = to_hq.lock().unwrap().last().unwrap().body["params"]["arguments"].clone();
    assert_eq!(
        posted,
        json!({ "task": 7, "last_word": true, "text": "Sonarr is down; claiming first.\n\n\
            Its log says the disk is full.\n\nThe turn's 3 rounds ran out before it finished: \
            the words above are what it said on the way, not an answer. This turn made no tool \
            calls." }),
        "its words, and that the rounds ran out, beside what it did"
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
            "consult",
            "chess",
            "answer_assistance",
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
            "/team-relay/transcript",
        ]
    );
    // What it sent is the whole conversation: the briefing, the calls and what they came to.
    let said = to_node.last().unwrap();
    assert_eq!(said.body["runtime"], "worker");
    assert_eq!(
        said.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    let text = said.body["text"].as_str().unwrap();
    for part in [
        "--- user ---\nYou are Jane, the team's developer.",
        "--- call: homelab_status ---",
        "--- result ---\n{\"stale\":false,\"services\":[]}",
        "--- call: restart_container ---",
        "Refused: sonarr was not restarted",
    ] {
        assert!(text.contains(part), "{part:?} in\n{text}");
    }
    assert!(!text.contains(TOKEN), "the token is never in what it tells");
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
        7
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
            "/team-relay/transcript",
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
        !system.contains("tell_story"),
        "a member without tell_story is not told to use it: {system}"
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
        ("ENDORA_HQ_MCP", format!("{hq}/v2/team/mcp")),
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
        [
            "cache_control",
            "max_tokens",
            "messages",
            "model",
            "system",
            "tools"
        ],
        "only what the relay meters, and no effort when HQ names none"
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
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "claim")
            .unwrap(),
        &json!({ "name": "claim", "description": "Take a task as yours.",
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
fn a_cloud_turns_rounds_share_one_prefix_each_asking_to_cache_it_at_hqs_effort() {
    let (hq_url, _) = hq(None);
    let (node_url, _) = node(contracts_that_say_what_they_are(), None);
    let (relay_url, to_relay) = model(vec![
        (
            200,
            claude("Looking.", &[("toolu_1", "homelab_status", json!({}))]),
        ),
        (
            200,
            claude(
                "",
                &[(
                    "toolu_2",
                    "container_logs",
                    json!({ "container": "sonarr" }),
                )],
            ),
        ),
        (200, claude("All is well.", &[])),
    ]);
    let mut env = on_claude(&hq_url, &relay_url, &node_url);
    env.push(("ENDORA_EFFORT", "medium".to_owned()));
    let out = worker(&env);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    let bodies: Vec<Value> = to_relay
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.body.clone())
        .collect();
    assert_eq!(bodies.len(), 3);
    for body in &bodies {
        assert_eq!(
            body["cache_control"],
            json!({ "type": "ephemeral" }),
            "every round asks to cache its whole prompt"
        );
        assert_eq!(body["output_config"], json!({ "effort": "medium" }));
    }
    for pair in bodies.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        for fixed in ["model", "system", "tools", "output_config", "cache_control"] {
            assert_eq!(before[fixed], after[fixed], "{fixed} never changes");
        }
        let before = before["messages"].as_array().unwrap();
        let after = after["messages"].as_array().unwrap();
        assert_eq!(after.len(), before.len() + 2, "its answer and the results");
        assert_eq!(
            serde_json::to_string(&after[..before.len()]).unwrap(),
            serde_json::to_string(&before[..]).unwrap(),
            "each round goes on from the one before, unchanged"
        );
    }
    let names: Vec<&str> = bodies[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(
        names, sorted,
        "the tools in one order, whatever order they came in"
    );
}

#[test]
fn the_local_relay_is_sent_no_effort_and_no_cache_mark() {
    let (hq_url, _) = hq(None);
    let (model_url, to_model) = model(vec![(200, says("Done."))]);
    let mut env = usual(&hq_url, &model_url);
    env.push(("ENDORA_EFFORT", "high".to_owned()));
    let out = worker(&env);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let body = to_model.lock().unwrap()[0].body.clone();
    let mut fields: Vec<&String> = body.as_object().unwrap().keys().collect();
    fields.sort();
    assert_eq!(fields, ["max_tokens", "messages", "model", "tools"]);
    // A house model costs nothing to let think: chess turns on the local brains reached the
    // cloud's 4096 and failed (8 of 441 house turns, 23-26 Oct), so they get twice that.
    assert_eq!(body["max_tokens"], 8192);
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

/// The node's team listener as a cloud turn reaches it: the Anthropic relay and the contracts
/// on one address. The contracts list `offered`, `homelab_status` answers, and the relay's nth
/// call is answered with `replies[n]`, the last one again after.
fn team_listener(offered: Value, replies: Vec<Value>) -> (String, Seen) {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    fake(Box::new(move |sent, _| {
        match sent.path.as_str() {
        "/team-relay/contracts" => (200, json!({ "tools": offered }).to_string()),
        "/team-relay/contracts/homelab_status" => (
            200,
            json!({ "text": "{\"stale\":false,\"services\":[{\"name\":\"docker\",\"items\":[{\"name\":\"deluge\",\"state\":\"running\"}]}]}" })
                .to_string(),
        ),
        "/team-relay/v1/messages" => {
            let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (200, replies[n.min(replies.len() - 1)].to_string())
        }
        _ => (404, json!({ "error": "no such route" }).to_string()),
    }
    }))
}

#[test]
fn a_lead_on_claude_is_offered_the_home_lab_and_told_it_has_it_as_the_runner_starts_it() {
    // Live, 2026-10-17: the lead on Claude Sonnet said three times that its tools could not
    // read the home lab. Started as the runner starts a cloud turn (the Anthropic relay's URL
    // and the model, and no contracts' address: the worker finds them beside the relay), the
    // node's homelab_status reaches the model's tools, and its instructions name it as its own.
    let (hq_url, _) = hq(None);
    let (listener, to_node) = team_listener(
        contracts_that_say_what_they_are(),
        vec![
            claude(
                "Looking first.",
                &[("toolu_1", "homelab_status", json!({}))],
            ),
            claude("deluge is running again.", &[]),
        ],
    );
    let out = worker(&[
        ("ENDORA_RUN_TOKEN", TOKEN.to_owned()),
        ("ENDORA_HQ_MCP", format!("{hq_url}/v2/team/mcp")),
        ("ENDORA_MODEL_URL", format!("{listener}/team-relay/v1")),
        ("ENDORA_MODEL", "claude-sonnet-5-5".to_owned()),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let to_node = to_node.lock().unwrap().clone();
    let paths: Vec<&str> = to_node.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/team-relay/contracts",
            "/team-relay/v1/messages",
            "/team-relay/contracts/homelab_status",
            "/team-relay/v1/messages",
            "/team-relay/transcript",
        ]
    );
    let first = &to_node[1].body;
    let tools: Vec<&str> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(tools.contains(&"homelab_status"), "{tools:?}");
    let system = first["system"].as_str().unwrap();
    assert!(
        system.contains(
            "The home lab tools are Endora's contracts, and this turn they are yours: \
             homelab_status, container_logs, restart_container. Whatever your role, look with \
             them yourself before you hand anything on, and never say you cannot read the home \
             lab."
        ),
        "{system}"
    );
}

// ---- Claude Code as the runtime (ADR 0117) ----------------------------------------------------

/// A folder of a test's own, fresh.
fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("endora-cc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A stand-in for Claude Code: a script that keeps its arguments, environment, stdin and
/// folder in `dir`, and prints `result` as Claude Code's JSON.
fn stand_in(dir: &std::path::Path, result: &Value) -> std::path::PathBuf {
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > {d}/args\nenv > {d}/env\npwd > {d}/cwd\ncat > {d}/stdin\nprintf '%s\\n' '{r}'\n",
            d = dir.display(),
            r = result.to_string().replace('\'', "")
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    script
}

/// The worker's environment for a Claude Code turn in the workbench, as the runner gives it.
fn with_claude_code(
    hq: &str,
    relay: &str,
    node: &str,
    work: &std::path::Path,
    claude: &std::path::Path,
) -> Vec<(&'static str, String)> {
    let mut env = on_claude(hq, relay, node);
    env.retain(|(k, _)| *k != "ENDORA_MODEL");
    env.extend([
        ("ENDORA_MODEL", "claude-haiku-5-5".to_owned()),
        ("ENDORA_EFFORT", "low".to_owned()),
        ("ENDORA_WORKBENCH", "1".to_owned()),
        ("ENDORA_WORKBENCH_DIR", work.display().to_string()),
        ("ENDORA_RUNTIME", "claude-code".to_owned()),
        ("ENDORA_CLAUDE", claude.display().to_string()),
        ("PATH", std::env::var("PATH").unwrap_or_default()),
    ]);
    env
}

#[test]
fn a_claude_code_turn_is_hqs_bookkeeping_then_claude_code_headless_then_its_last_word() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, to_node) = node(contracts_that_say_what_they_are(), None);
    let dir = folder("turn");
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let result = json!({ "type": "result", "subtype": "success", "is_error": false,
        "result": format!("Listed what is watched; nothing about {TOKEN}."), "session_id": "s",
        "total_cost_usd": 0.0042, "num_turns": 6 });
    let claude = stand_in(&dir, &result);
    let out = worker(&with_claude_code(
        &hq_url,
        "http://192.168.1.10:8788",
        &node_url,
        &work,
        &claude,
    ));
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
        ],
        "the worker's own bookkeeping; the model's calls go through the bridge"
    );
    let last = to_hq.lock().unwrap()[5].body["params"]["arguments"].clone();
    let text = last["text"].as_str().unwrap();
    assert!(
        text.starts_with("Listed what is watched; nothing about [the turn's token]."),
        "{text}"
    );
    assert!(
        text.contains("This turn made no tool calls.")
            && text.contains("Claude Code took 6 rounds"),
        "{text}"
    );
    assert_eq!(last["last_word"], true);
    // What the worker has of the session is told to the node, for HQ to keep (ADR 0129): the
    // briefing it was given, its last word, and that the session itself is not kept.
    let told: Vec<Sent> = to_node
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.path == "/team-relay/transcript")
        .cloned()
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].body["runtime"], "claude-code");
    let talk = told[0].body["text"].as_str().unwrap();
    assert!(
        talk.starts_with("--- user ---\nYou are Jane, the team's developer."),
        "{talk}"
    );
    assert!(
        talk.contains("--- assistant ---\nListed what is watched;"),
        "{talk}"
    );
    assert!(
        talk.contains("keeps no session") && talk.contains("6 rounds"),
        "{talk}"
    );

    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    let args: Vec<String> = read("args").lines().map(str::to_owned).collect();
    let at = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(args[0], "-p");
    assert_eq!(at("--model"), "claude-haiku-5-5");
    assert_eq!(at("--effort"), "low");
    assert_eq!(at("--max-turns"), "30");
    // The shell is Claude Code's whole Bash (ADR 0117): the container is the boundary, and a
    // list of prefixes denied the compound commands a model writes.
    for tool in [
        "mcp__endora__post",
        "mcp__endora__homelab_status",
        "mcp__endora__restart_container",
        "Bash",
    ] {
        assert!(args.contains(&tool.to_owned()), "{tool}: {args:?}");
    }
    assert!(
        !args.iter().any(|a| a.starts_with("Bash(")),
        "no pattern rule on Bash: {args:?}"
    );
    assert!(
        !args.iter().any(|a| a.contains(TOKEN)),
        "no token in an argument"
    );
    let env = read("env");
    assert!(
        env.contains("ANTHROPIC_BASE_URL=http://192.168.1.10:8788/team-relay\n"),
        "{env}"
    );
    assert!(env.contains(&format!("ANTHROPIC_AUTH_TOKEN={TOKEN}\n")));
    assert!(env.contains("ANTHROPIC_SMALL_FAST_MODEL=claude-haiku-5-5\n"));
    assert!(
        !env.contains("ENDORA_RUN_TOKEN="),
        "the worker's own name for it is not passed on"
    );
    let stdin = read("stdin");
    assert!(
        stdin.starts_with("You are Jane, the team's developer."),
        "{stdin}"
    );
    assert!(
        stdin.contains("your task was claimed for you: Task 7 is yours."),
        "{stdin}"
    );
    let files = work.join(".endora");
    let mcp = std::fs::read_to_string(files.join("mcp.json")).unwrap();
    assert!(
        !mcp.contains(TOKEN) && mcp.contains("${ENDORA_TURN_TOKEN}"),
        "{mcp}"
    );
    let rules = std::fs::read_to_string(files.join("rules.md")).unwrap();
    assert!(
        rules.contains("mcp__endora__") && rules.contains("sandbox-reported"),
        "{rules}"
    );
    assert_eq!(
        read("cwd").trim(),
        std::fs::canonicalize(&work).unwrap().display().to_string()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_claude_code_turn_the_relay_ended_exits_3_and_a_broken_one_1() {
    for (result, code) in [
        (
            json!({ "type": "result", "subtype": "success", "is_error": true, "result": "API Error: 402",
                 "api_error_status": 402 }),
            3,
        ),
        (
            json!({ "type": "result", "subtype": "error_max_turns", "is_error": true, "num_turns": 30 }),
            0,
        ),
        (json!({ "type": "nothing" }), 1),
    ] {
        let (hq_url, to_hq) = hq(None);
        let (node_url, _) = node(json!([]), None);
        let dir = folder(&format!("ended-{code}"));
        let claude = stand_in(&dir, &result);
        let out = worker(&with_claude_code(
            &hq_url,
            "http://127.0.0.1:9",
            &node_url,
            &dir,
            &claude,
        ));
        assert_eq!(out.status.code(), Some(code), "{result}: {}", said(&out));
        assert!(!said(&out).contains(TOKEN));
        assert!(
            !methods(&to_hq).contains(&"tools/call post".to_owned()),
            "no last word: {result}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
    // Outside the workbench, or on the house's model, there is no Claude Code.
    let (hq_url, _) = hq(None);
    let mut env = usual(&hq_url, "http://127.0.0.1:9");
    env.push(("ENDORA_RUNTIME", "claude-code".to_owned()));
    assert_eq!(worker(&env).status.code(), Some(1));
}

/// Runs the worker as Claude Code's bridge, with `lines` on its stdin: what it answered.
fn bridge(env: &[(&'static str, String)], lines: &[Value]) -> (Vec<Value>, Output) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_endora-worker"))
        .arg("--mcp")
        .env_clear()
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let answers = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    (answers, out)
}

#[test]
fn the_bridge_serves_hqs_tools_and_the_nodes_and_a_question_ends_the_turn() {
    let (hq_url, to_hq) = hq(None);
    let (node_url, to_node) = node(contracts_that_say_what_they_are(), None);
    let dir = folder("bridge");
    let ledger = dir.join("did.json");
    let env = vec![
        ("ENDORA_TURN_TOKEN", TOKEN.to_owned()),
        ("ENDORA_HQ_MCP", format!("{hq_url}/v2/team/mcp")),
        (
            "ENDORA_CONTRACTS",
            format!("{node_url}/team-relay/contracts"),
        ),
        ("ENDORA_WORKBENCH_DIR", dir.display().to_string()),
        ("ENDORA_MEMBER", "Jane".to_owned()),
        ("ENDORA_TASK", "7".to_owned()),
        ("ENDORA_LEDGER", ledger.display().to_string()),
    ];
    let rpc = |id: u64, method: &str, params: Value| json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let call = |id: u64, name: &str, arguments: Value| {
        rpc(
            id,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
    };
    let (answers, out) = bridge(
        &env,
        &[
            rpc(
                1,
                "initialize",
                json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code" } }),
            ),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            rpc(2, "tools/list", json!({})),
            call(3, "homelab_status", json!({})),
            call(
                4,
                "post",
                json!({ "task": 7, "text": format!("found it, token {TOKEN}") }),
            ),
            call(5, "ask_owner", json!({ "text": "May I restart sonarr?" })),
            call(6, "post", json!({ "task": 7, "text": "more" })),
            rpc(7, "resources/list", json!({})),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(
        answers.len(),
        7,
        "a notification takes no answer: {answers:?}"
    );
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], "endora");
    let listed: Vec<&str> = answers[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        listed,
        [
            "my_turn",
            "claim",
            "post",
            "set_status",
            "consult",
            "chess",
            "answer_assistance",
            "homelab_status",
            "container_logs",
            "restart_container"
        ]
    );
    assert_eq!(answers[2]["result"]["isError"], false);
    assert!(
        answers[2]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("services")
    );
    assert_eq!(answers[4]["result"]["isError"], false);
    assert!(
        answers[4]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("That ends your turn")
    );
    assert_eq!(
        answers[5]["result"]["isError"], true,
        "nothing after a question"
    );
    assert_eq!(answers[6]["error"]["code"], -32601);
    let posted: Vec<Value> = to_hq
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.method() == "tools/call post")
        .map(|s| s.body["params"]["arguments"].clone())
        .collect();
    assert_eq!(posted.len(), 1);
    assert_eq!(
        posted[0]["text"], "found it, token [the turn's token]",
        "never the token on"
    );
    assert!(
        to_node
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.path == "/team-relay/contracts/homelab_status")
    );
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&ledger).unwrap()).unwrap();
    assert_eq!(kept["asked"], true);
    assert_eq!(kept["calls"][0][0], "read the home lab's state");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_hook_posts_claude_codes_own_tools_to_the_node_and_never_blocks_the_turn() {
    let (node_url, to_node) = fake(Box::new(|_, _| (200, json!({ "receipt": 52 }).to_string())));
    let hook = |env: &[(&'static str, String)], input: &Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_endora-worker"))
            .arg("--receipt")
            .env_clear()
            .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let env = vec![
        ("ENDORA_TURN_TOKEN", TOKEN.to_owned()),
        (
            "ENDORA_RECEIPT_URL",
            format!("{node_url}/team-relay/receipt"),
        ),
    ];
    let bash = json!({ "session_id": "s", "transcript_path": "/work/.claude/x.jsonl", "cwd": "/work/repo",
        "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": { "command": "git status" },
        "tool_response": { "stdout": "clean", "stderr": "", "interrupted": false }, "tool_use_id": "toolu_1" });
    let out = hook(&env, &bash);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "a hook that prints says nothing to Claude Code"
    );
    let mcp = json!({ "hook_event_name": "PostToolUse", "tool_name": "mcp__endora__post", "tool_input": {} });
    assert_eq!(hook(&env, &mcp).status.code(), Some(0));
    let sent = to_node.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "the bridge's own tools are not reported");
    assert_eq!(sent[0].path, "/team-relay/receipt");
    assert_eq!(
        sent[0].header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(sent[0].body["tool_name"], "Bash");
    assert_eq!(sent[0].body["tool_input"]["command"], "git status");
    assert!(sent[0].body.get("transcript_path").is_none());
    // A node that is not there leaves the turn going, and the token unsaid.
    let gone = vec![
        ("ENDORA_TURN_TOKEN", TOKEN.to_owned()),
        (
            "ENDORA_RECEIPT_URL",
            "http://127.0.0.1:9/team-relay/receipt".to_owned(),
        ),
    ];
    let out = hook(&gone, &bash);
    assert_eq!(out.status.code(), Some(0));
    assert!(!said(&out).contains(TOKEN));
}

/// Twelve of the node's contracts, more than a local model should read every call.
fn many_contracts() -> Value {
    let named = [
        ("homelab_status", "Read the home lab's state."),
        ("container_logs", "Read a container's log."),
        ("restart_container", "Restart one container."),
        (
            "sonarr_queue",
            "Read Sonarr's download queue and stuck imports.",
        ),
        ("radarr_queue", "Read Radarr's download queue."),
        ("plex_sessions", "Read who is watching on Plex."),
        ("pihole_summary", "Read Pi-hole's blocking summary."),
        ("unifi_clients", "Read the network's clients from UniFi."),
        ("nas_status", "Read the NAS's load, memory and disks."),
        ("ha_lights", "Turn Home Assistant lights on or off."),
        ("ha_climate", "Set the thermostat in Home Assistant."),
        ("web_search", "Search the web."),
    ];
    Value::Array(
        named
            .iter()
            .map(|(name, what)| {
                json!({ "name": name, "kind": "read", "description": what,
                        "inputSchema": { "type": "object", "properties": {} } })
            })
            .collect(),
    )
}

/// Anthropic and OpenAI both advise a small first set and deferring the rest behind a search
/// (2026-10-25): a local turn offered many tools starts with its core and find_tools, and a
/// tool it finds is offered from its next call, so a slow model reads a short prompt.
#[test]
fn a_local_turn_with_many_tools_starts_small_and_finds_the_rest() {
    let (hq_url, _) = hq(None);
    let (node_url, _) = node(many_contracts(), None);
    let (model_url, to_model) = model(vec![
        (
            200,
            calls(&[("c1", "find_tools", r#"{"query":"sonarr queue"}"#)]),
        ),
        (200, calls(&[("c2", "sonarr_queue", "{}")])),
        (200, says("Read the queue.")),
    ]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    let to_model = to_model.lock().unwrap().clone();
    let names = |i: usize| -> Vec<String> {
        to_model[i].body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(str::to_owned))
            .collect()
    };
    let first = names(0);
    for core in [
        "my_turn",
        "claim",
        "post",
        "set_status",
        "homelab_status",
        // The game a turn is woken to move in is offered up front: behind a search, no move was
        // ever made.
        "chess",
        "find_tools",
    ] {
        assert!(first.iter().any(|n| n == core), "{core} missing: {first:?}");
    }
    for later in ["sonarr_queue", "ha_lights", "web_search", "plex_sessions"] {
        assert!(
            !first.iter().any(|n| n == later),
            "{later} offered up front: {first:?}"
        );
    }
    assert!(
        first.len() <= 11,
        "the first call offers {} tools",
        first.len()
    );
    let second = names(1);
    assert!(second.iter().any(|n| n == "sonarr_queue"), "{second:?}");
    assert!(!second.iter().any(|n| n == "ha_lights"), "{second:?}");
    let found = to_model[1].body["messages"].to_string();
    assert!(
        found.contains("sonarr_queue"),
        "the model is told what was added: {found}"
    );
}

/// Live, 27 Oct 2026: a Claude turn's opening call sent about 26,000 tokens, nearly all tool
/// definitions it never used, and wrote them to the cache afresh every turn ($0.066 on Sonnet).
/// With many tools, a Claude turn now loads its core and leaves the rest to Anthropic's tool
/// search: every definition is still sent, the rest marked `defer_loading`, and the search's
/// own blocks go back in the conversation as Claude said them, so the cached prompt holds.
#[test]
fn a_claude_turn_with_many_tools_loads_its_core_and_searches_for_the_rest() {
    let (hq_url, _) = hq(None);
    let (node_url, _) = node(many_contracts(), None);
    let searched = json!({ "type": "message", "role": "assistant", "model": "claude-sonnet-5-5",
        "content": [
            { "type": "server_tool_use", "id": "srvtoolu_1", "name": "tool_search_tool_bm25",
              "input": { "query": "sonarr queue" } },
            { "type": "tool_search_tool_result", "tool_use_id": "srvtoolu_1",
              "content": { "type": "tool_search_tool_search_result",
                "tool_references": [{ "type": "tool_reference", "tool_name": "sonarr_queue" }] } },
            { "type": "tool_use", "id": "toolu_1", "name": "sonarr_queue", "input": {} },
        ],
        "stop_reason": "tool_use", "usage": { "input_tokens": 1000, "output_tokens": 100 } });
    let (relay_url, to_relay) = model(vec![
        (200, searched.clone()),
        (200, claude("Read the queue.", &[])),
    ]);
    let out = worker(&on_claude(&hq_url, &relay_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));

    let to_relay = to_relay.lock().unwrap().clone();
    let tools = to_relay[0].body["tools"].as_array().unwrap().clone();
    let named = |name: &str| tools.iter().find(|t| t["name"] == name).cloned();
    assert_eq!(
        named("tool_search_tool_bm25"),
        Some(json!({ "type": "tool_search_tool_bm25_20251119", "name": "tool_search_tool_bm25" }))
    );
    for core in [
        "my_turn",
        "claim",
        "post",
        "set_status",
        "homelab_status",
        "chess",
    ] {
        let tool = named(core).unwrap_or_else(|| panic!("{core} missing"));
        assert!(
            tool.get("defer_loading").is_none(),
            "{core} is loaded: {tool}"
        );
    }
    for later in ["sonarr_queue", "ha_lights", "web_search", "plex_sessions"] {
        let tool = named(later).unwrap_or_else(|| panic!("{later} is still sent"));
        assert_eq!(tool["defer_loading"], true, "{later}");
    }
    assert!(
        !tools.iter().any(|t| t["name"] == "find_tools"),
        "Claude searches with Anthropic's tool, not the worker's"
    );
    assert_eq!(
        to_relay[1].body["tools"], to_relay[0].body["tools"],
        "the same tools every call, so the cached prompt holds"
    );
    let second = &to_relay[1].body["messages"];
    assert_eq!(
        second[1]["content"], searched["content"],
        "the search goes back as said"
    );
    assert_eq!(second[2]["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(
        second[2]["content"].as_array().unwrap().len(),
        1,
        "a result for the tool it ran, none for the search"
    );
}

/// A turn with few tools is offered all of them, as before: deferring would only cost a call.
#[test]
fn a_local_turn_with_few_tools_is_offered_them_all() {
    let (hq_url, _) = hq(None);
    let (node_url, _) = node(contracts(), None);
    let (model_url, to_model) = model(vec![(200, says("Done."))]);
    let out = worker(&with_node(&hq_url, &model_url, &node_url));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let tools = to_model.lock().unwrap()[0].body["tools"].to_string();
    assert!(!tools.contains("find_tools"), "{tools}");
    assert!(tools.contains("restart_container"), "{tools}");
}
