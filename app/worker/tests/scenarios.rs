//! The worker's scenarios (docs/testing.md): `features/*.feature` in the owner's words, run
//! against the real worker binary, through its front door: a turn in the workbench whose model
//! (a script) runs one shell command, and what the model then reads of it.

use std::io::{Read as _, Write as _};
use std::process::Command;
use std::sync::{Arc, Mutex};

use cucumber::cli::Parser as _;
use cucumber::{World, given, then, when};
use serde_json::{Value, json};

/// A turn's token, for these scenarios only.
const TOKEN: &str = "a-turns-token-for-the-workers-scenarios-only";

const BRIEFING: &str = "You are Jane, the team's developer.\n\
Your task: Read the nightly report (#7, todo, normal)\n\
Nothing new in the thread since your last turn.\n";

type Seen = Arc<Mutex<Vec<Value>>>;
type Answer = Box<dyn Fn(&Value, usize) -> (u16, String) + Send>;

/// A tiny HTTP server on a local port answering each request's JSON body with `answer`.
fn fake(answer: Answer) -> (String, Seen) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
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
            let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            let mut body = vec![0u8; length];
            let _ = stream.read_exact(&mut body);
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let before = kept.lock().unwrap().len();
            let (status, reply) = answer(&body, before);
            kept.lock().unwrap().push(body);
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Answered\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (url, seen)
}

/// HQ's team protocol, as little of it as a turn needs.
fn hq() -> String {
    hq_answering(false).0
}

/// HQ's team protocol with its closing tools, keeping every request it is sent; with
/// `refuse_first_ask`, the first question to the owner is refused as HQ refuses one whose
/// evidence is not a receipt.
fn hq_answering(refuse_first_ask: bool) -> (String, Seen) {
    let asked = Arc::new(Mutex::new(0usize));
    fake(Box::new(move |sent, _| {
        let id = sent["id"].clone();
        let result = match sent["method"].as_str().unwrap_or("") {
            "initialize" => json!({ "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} }, "serverInfo": { "name": "endora-hq", "version": "0" } }),
            m if m.starts_with("notifications/") => return (202, String::new()),
            "tools/list" => json!({ "tools": [
                { "name": "my_turn", "description": "What this turn is for.",
                  "inputSchema": { "type": "object", "properties": {}, "required": [] } },
                { "name": "post", "description": "Write in a task's thread.",
                  "inputSchema": { "type": "object", "properties": {
                      "task": { "type": "integer" }, "text": { "type": "string" } },
                      "required": ["task", "text"] } },
                { "name": "set_status", "description": "Move a task on.",
                  "inputSchema": { "type": "object", "properties": {
                      "task": { "type": "integer" }, "status": { "type": "string" },
                      "reason": { "type": "string" } },
                      "required": ["task", "status", "reason"] } },
                { "name": "ask_owner", "description": "Put a question to the owner.",
                  "inputSchema": { "type": "object", "properties": {
                      "text": { "type": "string" } }, "required": ["text"] } } ] }),
            "tools/call" => {
                let params = &sent["params"];
                let (text, refused) = match params["name"].as_str().unwrap_or("") {
                    "my_turn" => (BRIEFING.to_owned(), false),
                    "set_status" => (
                        format!(
                            "Task {} is {}.",
                            params["arguments"]["task"], params["arguments"]["status"]
                        ),
                        false,
                    ),
                    "ask_owner" => {
                        let mut n = asked.lock().unwrap();
                        *n += 1;
                        if refuse_first_ask && *n == 1 {
                            (
                                "`a log line` is not evidence: cite a receipt as r12".to_owned(),
                                true,
                            )
                        } else {
                            ("It is on the owner's desk.".to_owned(), false)
                        }
                    }
                    _ => ("Posted.".to_owned(), false),
                };
                json!({ "content": [{ "type": "text", "text": text }], "isError": refused })
            }
            _ => {
                return (
                    200,
                    json!({ "jsonrpc": "2.0", "id": id,
                            "error": { "code": -32601, "message": "no such method" } })
                    .to_string(),
                );
            }
        };
        (
            200,
            json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
        )
    }))
}

/// The node, offering no contracts.
fn node() -> String {
    node_seeing(false).0
}

/// The node, offering no contracts, keeping every request it is sent (S1: a shell's receipts),
/// recording each shell report as receipt 61, or, with `refuse`, recording none.
fn node_seeing(refuse: bool) -> (String, Seen) {
    fake(Box::new(move |sent, _| {
        if sent["tool_name"] == "shell" {
            if refuse {
                return (
                    503,
                    json!({ "error": "HQ is not set up on this node" }).to_string(),
                );
            }
            return (200, json!({ "receipt": 61 }).to_string());
        }
        (200, json!({ "tools": [] }).to_string())
    }))
}

/// A node that offers the repository's two contracts, and answers nothing else.
fn node_with_repo() -> String {
    fake(Box::new(|_, _| {
        (
            200,
            json!({ "tools": [
                { "name": "repo_checkout", "kind": "read", "description": "Check out the fork." },
                { "name": "repo_submit", "kind": "act", "description": "Submit a branch." },
            ] })
            .to_string(),
        )
    }))
    .0
}

/// A node that offers the repository's contracts (only the checkout, to a turn that may only
/// read), and the home lab's tools with `lab`, and answers every checkout with `handed`, as the node's text with a receipt after it.
fn node_handing(handed: Value, read_only: bool, lab: bool, extra: Vec<String>) -> (String, Seen) {
    fake(Box::new(move |sent, _| {
        if sent.is_null() {
            let mut tools = vec![
                json!({ "name": "repo_checkout", "kind": "read", "description": "Check out." }),
            ];
            if lab {
                for name in ["homelab_status", "container_logs", "restart_container"] {
                    tools.push(json!({ "name": name, "kind": "read", "description": "The lab." }));
                }
            }
            // Climb 2, S4: a node tool the scenario names, offered beside the repository's.
            for name in &extra {
                tools.push(json!({ "name": name, "kind": "read", "description": "A node tool." }));
            }
            if !read_only {
                tools.push(
                    json!({ "name": "repo_submit", "kind": "act", "description": "Submit." }),
                );
            }
            return (200, json!({ "tools": tools }).to_string());
        }
        let text = format!("{handed} (receipt r12)");
        (200, json!({ "text": text }).to_string())
    }))
}

/// What the node hands out for a checkout, as it writes it: a bundle of a repository whose
/// default branch is `base`, with the team's kept `branch` on top of it when it is not the base,
/// and `outside` when the repository is one outside the lab.
fn handed_out(branch: &str, base: &str, outside: bool) -> Value {
    use base64::Engine as _;
    let dir = std::env::temp_dir().join(format!(
        "endora-worker-handed-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    git_in(&dir, &["init", "-q", "-b", base]);
    git_in(&dir, &["config", "user.name", "Jane Doe"]);
    git_in(&dir, &["config", "user.email", "jane@example.com"]);
    std::fs::write(dir.join("README.md"), "tiny\n").unwrap();
    git_in(&dir, &["add", "."]);
    git_in(&dir, &["commit", "-q", "-m", "the base"]);
    let kept = branch != base;
    let mut refs = vec![base.to_owned()];
    if kept {
        std::fs::write(dir.join("fix.py"), "fixed = True\n").unwrap();
        git_in(&dir, &["add", "."]);
        git_in(&dir, &["commit", "-q", "-m", "the fix"]);
        let head = git_in(&dir, &["rev-parse", "HEAD"]);
        git_in(&dir, &["reset", "-q", "--hard", "HEAD~1"]);
        let kept_ref = format!("refs/submissions/{branch}");
        git_in(&dir, &["update-ref", &kept_ref, &head]);
        refs.push(kept_ref);
    }
    let mut args = vec!["bundle", "create", "-q", "handed.bundle"];
    args.extend(refs.iter().map(String::as_str));
    git_in(&dir, &args);
    let bytes = std::fs::read(dir.join("handed.bundle")).unwrap();
    let mut handed = json!({ "branch": branch, "head": git_in(&dir, &["rev-parse", "HEAD"]),
        "base": base, "bundle": base64::engine::general_purpose::STANDARD.encode(bytes) });
    if kept {
        handed["kept"] = json!(true);
    }
    if outside {
        handed["outside"] = json!(true);
    }
    handed
}

/// `git` in `dir`, which must succeed; its output.
fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The model: runs `python3 report.py` in the shell, then says it is done.
fn model() -> (String, Seen) {
    fake(Box::new(|_, before| {
        let body = if before == 0 {
            json!({ "choices": [{ "index": 0, "finish_reason": "tool_calls", "message": {
                "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1",
                "type": "function", "function": { "name": "shell",
                "arguments": "{\"command\":\"python3 report.py\",\"why\":\"read the report\"}" } }] } }] })
        } else {
            json!({ "choices": [{ "index": 0, "finish_reason": "stop",
                "message": { "role": "assistant", "content": "Read it." } }] })
        };
        (200, body.to_string())
    }))
}

/// One call as a scenario writes it: `set_status 7 in_review`, `ask_owner` (`ask_owner r12`
/// citing a receipt), `post`,
/// `shell touch made.txt`.
fn call_of(written: &str, n: usize) -> Value {
    let words: Vec<&str> = written.split_whitespace().collect();
    let (name, arguments) = match words.as_slice() {
        ["set_status", task, status] => (
            "set_status",
            json!({ "task": task.parse::<i64>().unwrap(), "status": status, "reason": "as found" }),
        ),
        // A model that writes the worker's sandbox report itself (correction-placement r1).
        ["set_status", task, status, "claiming-an-empty-sandbox"] => (
            "set_status",
            json!({ "task": task.parse::<i64>().unwrap(), "status": status, "reason": "as found",
                    "sandbox": { "state": "read", "branches": [] } }),
        ),
        ["ask_owner"] => (
            "ask_owner",
            json!({ "text": "Account jdoe is locked: unlock it or leave it locked?" }),
        ),
        ["ask_owner", receipt] => (
            "ask_owner",
            json!({ "text": "Account jdoe is locked: unlock it or leave it locked?",
                    "evidence": [receipt] }),
        ),
        ["post"] => ("post", json!({ "task": 7, "text": "What I found." })),
        ["repo_checkout", branch, "refresh"] => (
            "repo_checkout",
            json!({ "branch": branch, "refresh": true, "why": "the node's latest" }),
        ),
        ["repo_submit", branch] => ("repo_submit", json!({ "branch": branch, "why": "the fix" })),
        ["repo_checkout", branch] => (
            "repo_checkout",
            json!({ "branch": branch, "why": "to go on" }),
        ),
        ["shell", command @ ..] => (
            "shell",
            json!({ "command": command.join(" "), "why": "as the scenario says" }),
        ),
        _ => panic!("a call the scenarios do not know: {written}"),
    };
    json!({ "id": format!("call_{n}"), "type": "function",
            "function": { "name": name, "arguments": arguments.to_string() } })
}

/// The model: each reply in `rounds` in turn (calls written as `call_of` reads them, joined
/// with ", "), then its last word.
fn model_replying(rounds: Vec<String>) -> (String, Seen) {
    fake(Box::new(move |_, before| {
        let body = match rounds.get(before) {
            Some(reply) => {
                let calls: Vec<Value> = reply
                    .split(", ")
                    .enumerate()
                    .map(|(i, c)| call_of(c, before * 10 + i))
                    .collect();
                json!({ "choices": [{ "index": 0, "finish_reason": "tool_calls", "message": {
                    "role": "assistant", "content": null, "tool_calls": calls } }] })
            }
            None => json!({ "choices": [{ "index": 0, "finish_reason": "stop",
                "message": { "role": "assistant", "content": "Done." } }] }),
        };
        (200, body.to_string())
    }))
}

#[derive(Debug, Default, World)]
struct Turn {
    /// The workbench's folder, where the shell runs.
    dir: Option<std::path::PathBuf>,
    /// What the model read of the command, exactly.
    read: Option<String>,
    /// Whether HQ refuses the turn's first question to the owner.
    refuse_first_ask: bool,
    /// The closing tools HQ was called with, in order, as `call_of` writes them.
    hq_called: Vec<String>,
    /// Every request the model was sent.
    to_model: Vec<Value>,
    /// What the worker said on its way out.
    stderr: String,
    /// The head of the checkout an earlier turn kept in the workbench, when one did.
    kept_head: Option<String>,
    /// What the node hands out for a checkout, when the scenario says.
    handed: Option<Value>,
    /// Whether the node offers the turn only the checkout, as it does a reviewer.
    read_only: bool,
    /// Whether the node offers the home lab's tools beside the repository's.
    lab_tools: bool,
    /// Node tools the scenario names, offered beside the repository's (S4).
    extra_tools: Vec<String>,
    /// Every request the node was sent, when the scenario's node keeps them (S1).
    node_sent: Vec<Value>,
    /// Whether the node records no receipt (S1).
    no_receipts: bool,
    /// The arguments of every set_status HQ was sent (correction-placement r1).
    set_status_sent: Vec<Value>,
}

/// The note the worker adds when it leaves part of what a command printed out:
/// `(left out, printed)`, in bytes.
fn note_of(read: &str) -> Option<(usize, usize, String)> {
    let line = read
        .lines()
        .find(|l| l.contains("bytes the command printed"))?;
    let words: Vec<&str> = line.split_whitespace().collect();
    let left_out = words.first()?.trim_start_matches('[').parse().ok()?;
    let at = words.iter().position(|w| *w == "the")?;
    let printed = words.get(at + 1)?.parse().ok()?;
    Some((left_out, printed, line.to_owned()))
}

#[given("a turn in the workbench")]
fn a_turn(w: &mut Turn) {
    let dir = std::env::temp_dir().join(format!(
        "endora-worker-scenario-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    w.dir = Some(dir);
}

/// Writes `report.py`, which prints exactly `bytes` bytes of filler lines (of `filler`), with
/// `line` as a line of its own in the middle, and runs a turn whose model runs it.
fn run_report(w: &mut Turn, bytes: usize, filler: &str, line: &str) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    let script = format!(
        r#"import sys
total, filler, needle = {bytes}, {filler:?}, {line:?}
out = []
size = 0
row = 0
middle = total // 2
placed = False
while size < total:
    if not placed and size >= middle:
        piece = needle + "\n"
        placed = True
    else:
        piece = ("row %05d " % row) + filler * 6 + "\n"
        row += 1
    data = piece.encode()
    if size + len(data) > total:
        data = b"x" * (total - size - 1) + b"\n" if total - size > 1 else b"\n"
    out.append(data)
    size += len(data)
sys.stdout.buffer.write(b"".join(out))
"#
    );
    std::fs::write(dir.join("report.py"), script).unwrap();
    let (model, to_model) = model();
    let env = [
        ("ENDORA_RUN_TOKEN", TOKEN.to_owned()),
        ("ENDORA_HQ_MCP", format!("{}/v2/team/mcp", hq())),
        ("ENDORA_MODEL_URL", format!("{model}/team-relay/local/v1")),
        (
            "ENDORA_CONTRACTS",
            format!("{}/team-relay/contracts", node()),
        ),
        ("ENDORA_WORKBENCH", "1".to_owned()),
        ("ENDORA_WORKBENCH_DIR", dir.display().to_string()),
        ("PATH", std::env::var("PATH").unwrap_or_default()),
    ];
    let out = Command::new(env!("CARGO_BIN_EXE_endora-worker"))
        .env_clear()
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sent = to_model.lock().unwrap();
    let messages = sent[1]["messages"].as_array().expect("a second round");
    let result = messages
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .expect("the shell's result");
    w.read = Some(result["content"].as_str().unwrap().to_owned());
}

#[when(
    expr = "the member runs a command that prints {int} bytes with the line {string} in the middle"
)]
fn runs_report(w: &mut Turn, bytes: usize, line: String) {
    run_report(w, bytes, "abcdefgh", &line);
}

#[when(
    expr = "the member runs a command that prints {int} bytes of {string} with the line {string} in the middle"
)]
fn runs_report_of(w: &mut Turn, bytes: usize, filler: String, line: String) {
    run_report(w, bytes, &filler, &line);
}

#[then(expr = "the line {string} is not shown")]
fn not_shown(w: &mut Turn, line: String) {
    assert!(!w.read.as_deref().unwrap().contains(&line));
}

#[then(expr = "the line {string} is shown")]
fn shown(w: &mut Turn, line: String) {
    assert!(w.read.as_deref().unwrap().contains(&line));
}

#[then(
    expr = "the model is told how many of the {int} bytes it printed are not shown, and that number is true"
)]
fn told_truly(w: &mut Turn, bytes: usize) {
    let read = w.read.as_deref().unwrap();
    let (left_out, printed, note) = note_of(read).unwrap_or_else(|| panic!("no note in:\n{read}"));
    assert_eq!(printed, bytes, "{note}");
    // What of the command's output the model is shown: all it read but the first line (how the
    // command ended) and the note itself.
    let shown = read.len() - read.find('\n').unwrap() - 1 - (note.len() + 1);
    assert_eq!(left_out, bytes - shown, "{note}\nshown {shown} of {bytes}");
}

#[then(expr = "the model is not told that {int} bytes are not shown")]
fn not_told(w: &mut Turn, bytes: usize) {
    let read = w.read.as_deref().unwrap();
    let (_, _, note) = note_of(read).expect("a note");
    assert!(!note.contains(&bytes.to_string()), "{note}");
}

#[then("what the model reads is whole characters")]
fn whole_characters(w: &mut Turn) {
    assert!(!w.read.as_deref().unwrap().contains('\u{FFFD}'));
}

#[then("the model is told nothing is left out")]
fn nothing_left_out(w: &mut Turn) {
    let read = w.read.as_deref().unwrap();
    assert!(
        !read.contains("left out") && !read.contains("bytes cut"),
        "{read}"
    );
}

#[given("HQ refuses the first question to the owner as not evidence")]
fn refuses_first_ask(w: &mut Turn) {
    w.refuse_first_ask = true;
}

/// Runs a turn whose model sends `rounds`, keeping what HQ and the model were sent.
fn run_rounds(w: &mut Turn, rounds: Vec<String>) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    let (hq, to_hq) = hq_answering(w.refuse_first_ask);
    let (model, to_model) = model_replying(rounds);
    let mut node_seen: Option<Seen> = None;
    let contracts = match (&w.handed, w.kept_head.is_some()) {
        (Some(handed), _) => {
            let (url, seen) = node_handing(
                handed.clone(),
                w.read_only,
                w.lab_tools,
                w.extra_tools.clone(),
            );
            node_seen = Some(seen);
            url
        }
        (None, true) => node_with_repo(),
        (None, false) => {
            let (url, seen) = node_seeing(w.no_receipts);
            node_seen = Some(seen);
            url
        }
    };
    let env = [
        ("ENDORA_RUN_TOKEN", TOKEN.to_owned()),
        ("ENDORA_HQ_MCP", format!("{hq}/v2/team/mcp")),
        ("ENDORA_MODEL_URL", format!("{model}/team-relay/local/v1")),
        (
            "ENDORA_CONTRACTS",
            format!("{contracts}/team-relay/contracts"),
        ),
        ("ENDORA_WORKBENCH", "1".to_owned()),
        ("ENDORA_WORKBENCH_DIR", dir.display().to_string()),
        ("PATH", std::env::var("PATH").unwrap_or_default()),
    ];
    let out = Command::new(env!("CARGO_BIN_EXE_endora-worker"))
        .env_clear()
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap();
    w.stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{}", w.stderr);
    w.hq_called = to_hq
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s["method"] == "tools/call")
        .filter_map(|s| {
            let p = &s["params"];
            match p["name"].as_str()? {
                "set_status" => Some(format!(
                    "set_status {} {}",
                    p["arguments"]["task"],
                    p["arguments"]["status"].as_str().unwrap_or("")
                )),
                name @ ("ask_owner" | "post") => Some(name.to_owned()),
                _ => None,
            }
        })
        .collect();
    w.set_status_sent = to_hq
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s["method"] == "tools/call" && s["params"]["name"] == "set_status")
        .map(|s| s["params"]["arguments"].clone())
        .collect();
    w.to_model = to_model.lock().unwrap().clone();
    w.node_sent = node_seen
        .map(|s| s.lock().unwrap().clone())
        .unwrap_or_default();
}

/// The sandbox receipt the node was sent for the shell's `command` (S1).
fn shell_receipt(w: &Turn, command: &str) -> Value {
    w.node_sent
        .iter()
        .find(|b| b["tool_name"] == "shell" && b["tool_input"]["command"] == command)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "no sandbox receipt for {command:?}; the node was sent: {:#?}",
                w.node_sent
            )
        })
}

#[then(expr = "the node was sent a sandbox receipt of the shell's {string}")]
fn sent_a_receipt(w: &mut Turn, command: String) {
    shell_receipt(w, &command);
}

#[then(expr = "the receipt of {string} says the command completed")]
fn receipt_completed(w: &mut Turn, command: String) {
    let r = shell_receipt(w, &command);
    assert_eq!(r["hook_event_name"], "PostToolUse", "{r:#}");
    assert_eq!(r["tool_response"]["timed_out"], false, "{r:#}");
    assert_eq!(r["tool_response"]["killed"], false, "{r:#}");
}

#[then(expr = "the receipt of {string} records exit code {int}")]
fn receipt_exit(w: &mut Turn, command: String, code: i64) {
    let r = shell_receipt(w, &command);
    assert_eq!(r["tool_response"]["exit_code"], code, "{r:#}");
}

#[given("the node cannot record receipts")]
fn node_cannot_record(w: &mut Turn) {
    w.no_receipts = true;
}

#[then(expr = "the workbench file {string} holds {int} line(s)")]
fn workbench_file_lines(w: &mut Turn, name: String, lines: usize) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    let text = std::fs::read_to_string(dir.join(&name)).expect("the file");
    assert_eq!(text.lines().count(), lines, "{text}");
}

#[when(expr = "the model replies {string}")]
fn replies(w: &mut Turn, reply: String) {
    run_rounds(w, vec![reply]);
}

#[when(expr = "the model replies {string}, then {string}")]
fn replies_then(w: &mut Turn, first: String, second: String) {
    run_rounds(w, vec![first, second]);
}

#[when(expr = "the model replies {string}, then {string}, then {string}")]
fn replies_then_then(w: &mut Turn, first: String, second: String, third: String) {
    run_rounds(w, vec![first, second, third]);
}

/// More replies than any turn has rounds.
#[when(expr = "the model replies {string} every round")]
fn replies_every_round(w: &mut Turn, reply: String) {
    run_rounds(w, vec![reply; 64]);
}

/// What every tool result told the model after its `nth` reply holds `words`.
fn told_after(w: &Turn, nth: usize, words: &str) {
    let messages = w.to_model[nth]["messages"]
        .as_array()
        .expect("a round after that reply");
    let last_reply = messages
        .iter()
        .rposition(|m| m["role"] == "assistant")
        .expect("the reply");
    let results: Vec<&str> = messages[last_reply..]
        .iter()
        .filter(|m| m["role"] == "tool")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(!results.is_empty(), "no results told");
    for result in results {
        assert!(result.contains(words), "{result}");
    }
}

/// An earlier turn's checkout, kept in the workbench as the sandbox keeps it: the fork's
/// `develop`, then `branch` with one commit of the member's beyond it, nothing uncommitted.
#[given(expr = "the workbench kept a checkout on {string} with a commit since its base")]
fn kept_checkout(w: &mut Turn, branch: String) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git_in(&repo, &["init", "-q", "-b", "develop"]);
    git_in(&repo, &["config", "user.name", "Endora HQ"]);
    git_in(&repo, &["config", "user.email", "hq@endora.invalid"]);
    std::fs::write(repo.join("nas.yml"), "---\n").unwrap();
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "the lab"]);
    git_in(&repo, &["checkout", "-q", "-b", &branch]);
    std::fs::write(repo.join("fix.yml"), "fixed: true\n").unwrap();
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "the fix"]);
    std::fs::write(
        dir.join("checkout.json"),
        json!({ "branch": "develop", "base": "develop",
                "trailer": "Made-by: Endora HQ (Kit) for task #7" })
        .to_string(),
    )
    .unwrap();
    w.kept_head = Some(git_in(&repo, &["rev-parse", "HEAD"]));
}

#[given(expr = "the node hands out {string} of a repository outside the lab")]
fn hands_out_outside(w: &mut Turn, branch: String) {
    w.handed = Some(handed_out(&branch, &branch, true));
}

#[given(expr = "the node hands out {string} of the lab's fork")]
fn hands_out_fork(w: &mut Turn, branch: String) {
    w.handed = Some(handed_out(&branch, &branch, false));
}

#[given(
    expr = "the node hands out the team's kept branch {string} of a repository outside the lab"
)]
fn hands_out_kept_outside(w: &mut Turn, branch: String) {
    w.handed = Some(handed_out(&branch, "main", true));
}

#[given(expr = "the node hands out the team's kept branch {string} of the lab's fork")]
fn hands_out_kept_fork(w: &mut Turn, branch: String) {
    w.handed = Some(handed_out(&branch, "develop", false));
}

#[given("the node offers the home lab's tools too")]
fn offers_lab_tools(w: &mut Turn) {
    w.lab_tools = true;
}

#[given(expr = "the node offers {string} too")]
fn node_offers_too(w: &mut Turn, name: String) {
    w.extra_tools.push(name);
}

#[given("the turn may only read")]
fn may_only_read(w: &mut Turn) {
    w.read_only = true;
}

#[then(expr = "the model was not told {string} after its first reply")]
fn not_told_after_first(w: &mut Turn, words: String) {
    let messages = w.to_model[1]["messages"]
        .as_array()
        .expect("a round after that reply");
    let last_reply = messages
        .iter()
        .rposition(|m| m["role"] == "assistant")
        .expect("the reply");
    for result in messages[last_reply..]
        .iter()
        .filter(|m| m["role"] == "tool")
        .filter_map(|m| m["content"].as_str())
    {
        assert!(!result.contains(&words), "{result}");
    }
}

/// The turn's instructions: the system message of its first request to the model.
fn instructions_of(w: &Turn) -> String {
    w.to_model[0]["messages"]
        .as_array()
        .and_then(|m| m.iter().find(|m| m["role"] == "system"))
        .and_then(|m| m["content"].as_str())
        .expect("the turn's instructions")
        .to_owned()
}

#[then(expr = "the model's instructions say {string}")]
fn instructions_say(w: &mut Turn, words: String) {
    let told = instructions_of(w);
    assert!(told.contains(&words), "{told}");
}

#[then(expr = "the model's instructions do not say {string}")]
fn instructions_do_not_say(w: &mut Turn, words: String) {
    let told = instructions_of(w);
    assert!(!told.contains(&words), "{told}");
}

#[then("the model was told the kept branch's full head after its first reply")]
fn told_kept_head(w: &mut Turn) {
    let head = w.kept_head.clone().expect("a kept checkout");
    told_after(w, 1, &head);
}

#[then("the kept checkout still holds its commit")]
fn still_holds(w: &mut Turn) {
    let repo = w.dir.clone().unwrap().join("repo");
    assert_eq!(Some(git_in(&repo, &["rev-parse", "HEAD"])), w.kept_head);
}

#[then(expr = "the model was told {string} after its first reply")]
fn told_after_first(w: &mut Turn, words: String) {
    told_after(w, 1, &words);
}

#[then(expr = "the model was told {string} after its second reply")]
fn told_after_second(w: &mut Turn, words: String) {
    told_after(w, 2, &words);
}

#[then("the turn ran out of rounds")]
fn ran_out(w: &mut Turn) {
    assert!(w.stderr.contains("rounds ran out"), "{}", w.stderr);
}

#[then(expr = "HQ was called with {string}")]
fn hq_called_with(w: &mut Turn, calls: String) {
    assert_eq!(w.hq_called.join(", "), calls, "{}", w.stderr);
}

#[then("the model was told that nothing in its first reply ran")]
fn told_nothing_ran(w: &mut Turn) {
    let messages = w.to_model[1]["messages"]
        .as_array()
        .expect("a second round");
    let results: Vec<&str> = messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(!results.is_empty(), "no results told");
    for result in results {
        assert!(result.contains("nothing in this reply ran"), "{result}");
    }
}

#[then("the turn ended asking the owner")]
fn ended_asking(w: &mut Turn) {
    assert!(w.stderr.contains("asked the owner"), "{}", w.stderr);
}

#[then("the turn ended moving its task on")]
fn ended_moving_on(w: &mut Turn) {
    assert!(w.stderr.contains("moved its task on"), "{}", w.stderr);
}

#[then(expr = "the workbench holds no file {string}")]
fn holds_no_file(w: &mut Turn, name: String) {
    assert!(!w.dir.clone().unwrap().join(name).exists());
}

/// Cucumber's options from the command line, without the flags `cargo test` passes every test
/// binary, which cucumber does not know (as `domains/team/tests/scenarios.rs` does).
fn options() -> cucumber::cli::Opts<
    cucumber::parser::basic::Cli,
    cucumber::runner::basic::Cli,
    cucumber::writer::basic::Cli,
> {
    const WITH_A_VALUE: &[&str] = &["--test-threads", "--skip", "--format", "--color", "-Z"];
    let mut args = std::env::args();
    let mut kept = vec![args.next().unwrap_or_default()];
    while let Some(arg) = args.next() {
        if WITH_A_VALUE.contains(&arg.as_str()) {
            args.next();
            continue;
        }
        if arg.starts_with("--test-threads=") || (arg.starts_with('-') && !arg.starts_with("--")) {
            continue;
        }
        if arg.starts_with("--") {
            if arg.starts_with("--name") || arg.starts_with("--tags") {
                kept.push(arg);
            }
            continue;
        }
        kept.push("--name".to_owned());
        kept.push(arg);
    }
    cucumber::cli::Opts::try_parse_from(kept).expect("cucumber's own options")
}

fn main() {
    futures::executor::block_on(
        Turn::cucumber()
            .with_cli(options())
            .fail_on_skipped()
            .run_and_exit("features"),
    );
}

// ---- A closing set_status carries the worker's own reading of its sandbox (correction-placement
// r1, qualification only) ----------------------------------------------------------------------

fn sandbox_told(w: &Turn) -> Value {
    let sent = w.set_status_sent.last().expect("HQ was sent a set_status");
    sent["sandbox"].clone()
}

#[given("the workbench's checkout cannot be read")]
fn checkout_unreadable(w: &mut Turn) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    std::fs::write(dir.join("repo").join(".git").join("HEAD"), "not a ref\n").unwrap();
}

#[then(expr = "HQ was told the sandbox holds {string} at the kept head")]
fn told_holds_kept(w: &mut Turn, branch: String) {
    let head = w.kept_head.clone().expect("a kept checkout");
    let told = sandbox_told(w);
    assert_eq!(told["state"], "read", "{told}");
    let branches = told["branches"].as_array().cloned().unwrap_or_default();
    assert!(
        branches
            .iter()
            .any(|b| b["branch"] == branch.as_str() && b["head"] == head.as_str()),
        "{told}"
    );
}

#[then("HQ was told the sandbox holds no work of the task")]
fn told_holds_nothing(w: &mut Turn) {
    let told = sandbox_told(w);
    assert_eq!(told["state"], "read", "{told}");
    assert_eq!(told["branches"], json!([]), "{told}");
}

#[then("HQ was told the sandbox could not be read")]
fn told_unknown(w: &mut Turn) {
    let told = sandbox_told(w);
    assert_eq!(told["state"], "unknown", "{told}");
    assert!(
        told["why"].as_str().is_some_and(|w| !w.is_empty()),
        "{told}"
    );
}

#[given(expr = "the workbench's checkout also names its kept head {string}")]
fn alias_at_head(w: &mut Turn, alias: String) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    git_in(&dir.join("repo"), &["branch", &alias, "HEAD"]);
}

#[given("the workbench's base branch was moved onto the kept head")]
fn base_moved(w: &mut Turn) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    git_in(&dir.join("repo"), &["branch", "-f", "develop", "HEAD"]);
}

#[given("the workbench's task branch is at its base, with no commit of its own")]
fn empty_task_branch(w: &mut Turn) {
    let dir = w.dir.clone().expect("a turn in the workbench");
    let repo = dir.join("repo");
    git_in(&repo, &["reset", "-q", "--hard", "develop"]);
    w.kept_head = Some(git_in(&repo, &["rev-parse", "HEAD"]));
}

/// The head the worker sent the node with its submission: after the trailer was stamped.
#[then(expr = "HQ was told the sandbox holds {string} at the head the node was sent")]
fn told_holds_submitted(w: &mut Turn, branch: String) {
    let sent = w
        .node_sent
        .iter()
        .find(|b| b["head"].is_string() && b["bundle"].is_string())
        .unwrap_or_else(|| panic!("no submission reached the node: {:#?}", w.node_sent));
    let head = sent["head"].as_str().unwrap().to_owned();
    assert_ne!(
        Some(head.as_str()),
        w.kept_head.as_deref(),
        "the trailer was stamped, so the head moved"
    );
    let told = sandbox_told(w);
    let branches = told["branches"].as_array().cloned().unwrap_or_default();
    assert!(
        branches
            .iter()
            .any(|b| b["branch"] == branch.as_str() && b["head"] == head.as_str()),
        "{told}"
    );
}
