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
    fake(Box::new(|sent, _| {
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
                      "required": ["task", "text"] } } ] }),
            "tools/call" => {
                let text = match sent["params"]["name"].as_str().unwrap_or("") {
                    "my_turn" => BRIEFING,
                    _ => "Posted.",
                };
                json!({ "content": [{ "type": "text", "text": text }], "isError": false })
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
    .0
}

/// The node, offering no contracts.
fn node() -> String {
    fake(Box::new(|_, _| (200, json!({ "tools": [] }).to_string()))).0
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

#[derive(Debug, Default, World)]
struct Turn {
    /// The workbench's folder, where the shell runs.
    dir: Option<std::path::PathBuf>,
    /// What the model read of the command, exactly.
    read: Option<String>,
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
