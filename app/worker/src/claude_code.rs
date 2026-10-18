//! A turn run by Claude Code (ADR 0117): the worker is still the container's entrypoint and
//! does HQ's bookkeeping (the protocol opened, `my_turn`, the claim, the fork checked out into
//! `/work/repo`), then starts `claude -p`, headless, in the workbench, and posts its last word
//! as it posts its own model's. Claude Code reaches Claude only through the node's Anthropic
//! relay, with the turn's token as its credential: never a subscription login, and no key
//! of the owner's is in the sandbox.
//!
//! Three processes of the same binary take part:
//! - the turn ([`take`]), which plans Claude Code's command line, runs it and reads its JSON;
//! - the bridge ([`serve`], `endora-worker --mcp`), the one MCP server Claude Code is given,
//!   over stdio: HQ's team tools and the node's contracts (the wing's git among them, carried
//!   out here as the worker carries them), each call sent where its tool came from, exactly as
//!   the worker's own loop sends them; it keeps the turn's line of what it did in a file;
//! - the hook ([`report`], `endora-worker --receipt`), which Claude Code's `PostToolUse` hook
//!   runs after each of its own tools: it posts what the tool did to the node, which keeps it
//!   as a receipt marked sandbox-reported.
//!
//! The token reaches Claude Code in its environment (as `ANTHROPIC_AUTH_TOKEN`, and as
//! `ENDORA_TURN_TOKEN` for the bridge's configuration, which names no secret itself). Never in
//! a file, an argument or anything said: the bridge takes it out of what the model sends, and
//! the worker out of the last word.

use std::io::{BufRead as _, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::repo;
use crate::turn::{
    self, Answered, Call, Hq, Ledger, Node, POST_AT_MOST, PROTOCOL, REPO_CHECKOUT, Reply, SHELL,
    Setup, Stopped, Taken, beside, in_the_repo, instructions, is_repo_tool, member_of, quiet, run,
    task_of,
};

/// Claude Code's command, unless `ENDORA_CLAUDE` names another (the tests' stand-in).
const CLAUDE: &str = "claude";

/// The bridge's name to Claude Code: its tools are `mcp__endora__<tool>`.
pub const SERVER: &str = "endora";

/// Claude Code's own tools the turn is given (`--tools`): files and a shell, nothing that
/// reaches out (no web fetch or search, no agents).
pub const BUILT_IN: [&str; 6] = ["Read", "Edit", "Write", "Glob", "Grep", "Bash"];

/// What its shell may run without asking, which in a headless turn means at all: git and the
/// lab's own checks, and looking.
pub const SHELL_ALLOWED: [&str; 6] = [
    "Bash(git *)",
    "Bash(ansible-lint *)",
    "Bash(yamllint *)",
    "Bash(ansible-playbook --syntax-check *)",
    "Bash(ls *)",
    "Bash(cat *)",
];

/// Refused outright, whatever else allows them.
pub const DISALLOWED: [&str; 2] = ["WebFetch", "WebSearch"];

/// How long Claude Code may take: inside the runner's thirty minutes, with room to post.
const TURN_WITHIN: Duration = Duration::from_secs(25 * 60);

/// Where the turn's own files go, under the workbench's `/work`: the bridge's configuration,
/// Endora's rules, the line of what the turn did.
const FILES: &str = ".endora";

/// The most of a hook's report sent to the node, each text.
const REPORT_TEXT_MOST: usize = 2_000;

/// The most of a hook's stdin read.
const HOOK_READ_MOST: u64 = 1024 * 1024;

/// What Claude Code is told beside Endora's rules: how its tools map onto the turn.
const CLAUDE_CODE_RULES: &str = "\n\nYou are running in Claude Code, headless, for Endora HQ. \
    Endora's team tools are yours as mcp__endora__<name> (mcp__endora__post, \
    mcp__endora__set_status, mcp__endora__repo_submit and the rest): use them for everything \
    the team and the house need. Your shell is Claude Code's Bash, limited to git, ansible-lint, \
    yamllint, ansible-playbook --syntax-check, ls and cat; edit files with Edit and Write. \
    Each of your own tool calls is recorded as a sandbox-reported receipt, which you may cite \
    for what you did in the sandbox, but never as a reading of the lab: a reading comes from \
    Endora's own tools. Nothing you leave in /work outlives the turn.";

/// How a Claude Code run ended, from its JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    /// How the turn was taken.
    pub taken: Taken,
    /// Its last word (`result`).
    pub text: String,
    /// How many rounds it took (`num_turns`).
    pub turns: u64,
    /// What it reckoned it cost (`total_cost_usd`, its own estimate at list prices; the relay's
    /// charge is what counts).
    pub cost: f64,
}

/// The relay's base for Claude Code (`ANTHROPIC_BASE_URL`): the worker's model URL without its
/// `/v1`, since Claude Code adds `/v1/messages` itself.
#[must_use]
pub fn relay_base(model_url: &str) -> String {
    let url = model_url.trim_end_matches('/');
    url.strip_suffix("/v1").unwrap_or(url).to_owned()
}

/// Where the hook reports: the node's `/team-relay/receipt`, beside its contracts.
#[must_use]
pub fn receipt_url(contracts: &str) -> String {
    let url = contracts.trim_end_matches('/');
    let base = url.strip_suffix("/contracts").unwrap_or(url);
    format!("{base}/receipt")
}

/// The tools Claude Code may use without asking: the bridge's, by their MCP names; its own
/// files tools; its shell, for git and the checks.
#[must_use]
pub fn allowed_tools(offered: &[String]) -> Vec<String> {
    offered
        .iter()
        .map(|name| format!("mcp__{SERVER}__{name}"))
        .chain(
            ["Read", "Edit", "Write", "Glob", "Grep"]
                .iter()
                .map(|t| (*t).to_owned()),
        )
        .chain(SHELL_ALLOWED.iter().map(|t| (*t).to_owned()))
        .collect()
}

/// The bridge's configuration (`--mcp-config`): one stdio server, this binary with `--mcp`,
/// given where HQ and the node are and the turn's token by a name of Endora's own, expanded
/// by Claude Code from its environment. No secret is written in it.
#[must_use]
pub fn mcp_config(
    worker: &str,
    setup: &Setup,
    member: &str,
    task: Option<i64>,
    ledger: &Path,
) -> Value {
    json!({ "mcpServers": { SERVER: {
        "type": "stdio",
        "command": worker,
        "args": ["--mcp"],
        "env": {
            "ENDORA_TURN_TOKEN": "${ENDORA_TURN_TOKEN}",
            "ENDORA_HQ_MCP": setup.mcp,
            "ENDORA_CONTRACTS": setup.contracts,
            "ENDORA_WORKBENCH_DIR": setup.workbench.as_deref().unwrap_or(Path::new("/work")),
            "ENDORA_MEMBER": member,
            "ENDORA_TASK": task.map(|t| t.to_string()).unwrap_or_default(),
            "ENDORA_LEDGER": ledger,
        },
    } } })
}

/// Claude Code's command line for a turn: headless, JSON out, the member's brain and effort,
/// its rounds, nothing asked of anybody, only the bridge's MCP server, only the tools named,
/// no settings but the image's own (a fork's `.claude/` is never read), nothing kept for
/// later, and Endora's rules after its own.
#[must_use]
pub fn arguments(setup: &Setup, offered: &[String], mcp: &Path, rules: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "json",
        "--model",
        setup.model.as_str(),
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    if let Some(effort) = &setup.effort {
        args.extend(["--effort".to_owned(), effort.clone()]);
    }
    args.extend([
        "--max-turns".to_owned(),
        setup.most_rounds.to_string(),
        "--permission-mode".to_owned(),
        "dontAsk".to_owned(),
        "--permission-prompts".to_owned(),
        "none".to_owned(),
        "--setting-sources".to_owned(),
        "user".to_owned(),
        "--no-session-persistence".to_owned(),
        "--strict-mcp-config".to_owned(),
        "--mcp-config".to_owned(),
        mcp.display().to_string(),
        "--append-system-prompt-file".to_owned(),
        rules.display().to_string(),
    ]);
    args.push("--tools".to_owned());
    args.extend(BUILT_IN.iter().map(|t| (*t).to_owned()));
    args.push("--allowedTools".to_owned());
    args.extend(allowed_tools(offered));
    args.push("--disallowedTools".to_owned());
    args.extend(DISALLOWED.iter().map(|t| (*t).to_owned()));
    args
}

/// Claude Code's environment for a turn, beside what it inherits: the relay and the turn's
/// token for its calls, every model alias pointed at the member's brain (so nothing it sends
/// in the background names another), and the token again for the bridge.
#[must_use]
pub fn environment(setup: &Setup) -> Vec<(&'static str, String)> {
    vec![
        ("ANTHROPIC_BASE_URL", relay_base(&setup.model_url)),
        ("ANTHROPIC_AUTH_TOKEN", setup.token.clone()),
        ("ANTHROPIC_MODEL", setup.model.clone()),
        ("ANTHROPIC_SMALL_FAST_MODEL", setup.model.clone()),
        ("ANTHROPIC_DEFAULT_HAIKU_MODEL", setup.model.clone()),
        ("ANTHROPIC_DEFAULT_SONNET_MODEL", setup.model.clone()),
        ("ANTHROPIC_DEFAULT_OPUS_MODEL", setup.model.clone()),
        ("ENDORA_TURN_TOKEN", setup.token.clone()),
        ("ENDORA_RECEIPT_URL", receipt_url(&setup.contracts)),
    ]
}

/// The environment Claude Code is never given: the worker's own name for the token, and any
/// key or login of Anthropic's but the one set above.
const NOT_GIVEN: [&str; 4] = [
    "ENDORA_RUN_TOKEN",
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_OAUTH_TOKEN",
];

/// How a Claude Code run ended: its JSON on stdout (`result`), and its exit code.
///
/// # Errors
/// [`Stopped::Over`] when the relay or HQ ended the turn (401, 402); [`Stopped::Failed`] for
/// an error, or output that is not Claude Code's JSON.
pub fn finished(stdout: &str, code: Option<i32>, stderr: &str) -> Result<Finished, Stopped> {
    let parsed: Option<Value> = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter(|v| v["type"] == "result");
    let Some(result) = parsed else {
        let said = stderr.trim().lines().last().unwrap_or("").to_owned();
        return Err(Stopped::Failed(format!(
            "Claude Code ended ({}) without its result{}",
            code.map_or_else(|| "killed".to_owned(), |c| format!("exit {c}")),
            if said.is_empty() {
                String::new()
            } else {
                format!(": {said}")
            }
        )));
    };
    let status = result["api_error_status"].as_u64();
    if matches!(status, Some(401 | 402)) {
        return Err(Stopped::Over(format!(
            "the relay answered Claude Code {}",
            status.unwrap_or(0)
        )));
    }
    let text = result["result"].as_str().unwrap_or("").to_owned();
    let turns = result["num_turns"].as_u64().unwrap_or(0);
    let cost = result["total_cost_usd"].as_f64().unwrap_or(0.0);
    let subtype = result["subtype"].as_str().unwrap_or("");
    if subtype == "error_max_turns" {
        return Ok(Finished {
            taken: Taken::OutOfRounds,
            text,
            turns,
            cost,
        });
    }
    if result["is_error"] == true || subtype != "success" {
        let said: String = text.chars().take(200).collect();
        return Err(Stopped::Failed(format!(
            "Claude Code failed ({subtype}): {said}"
        )));
    }
    Ok(Finished {
        taken: Taken::Answered,
        text,
        turns,
        cost,
    })
}

/// What the bridge leaves for the turn: the line of what it did, and whether the turn ended
/// in it (a question to the owner; HQ or the node saying it is over).
#[derive(Default)]
struct Kept {
    did: Option<Ledger>,
    asked: bool,
    over: Option<String>,
}

fn read_kept(path: &Path) -> Kept {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Kept::default();
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Kept {
        did: Some(Ledger::from_kept(&v["calls"])),
        asked: v["asked"] == true,
        over: v["over"].as_str().map(str::to_owned),
    }
}

fn write_kept(path: &Path, did: &Ledger, asked: bool, over: Option<&str>) {
    let kept = json!({ "calls": did.kept(), "asked": asked, "over": over });
    // Written whole, then moved into place: the turn never reads half of it.
    let next = path.with_extension("next");
    if std::fs::write(&next, kept.to_string()).is_ok() {
        let _ = std::fs::rename(&next, path);
    }
}

/// The tools a turn is offered: HQ's, and the node's contracts not named like one (the wing's
/// git among them), as the worker's own loop offers them in the workbench, without its shell
/// (Claude Code has its own). With them, the names of the node's and those that only look.
fn offered(hq: &mut Hq<'_>, node: &Node<'_>) -> Result<Offered, Stopped> {
    let listed = hq.request("tools/list", json!({}))?;
    let mut tools: Vec<Value> = listed["tools"].as_array().cloned().unwrap_or_default();
    let contracts: Vec<Value> = node
        .list()?
        .into_iter()
        .filter(|c| !tools.iter().any(|t| t["name"] == c["name"]))
        .collect();
    let name = |c: &Value| c["name"].as_str().map(str::to_owned);
    let on_the_node: Vec<String> = contracts.iter().filter_map(name).collect();
    let reads: Vec<String> = contracts
        .iter()
        .filter(|c| c["kind"] == "read")
        .filter_map(name)
        .chain(std::iter::once("my_turn".to_owned()))
        .collect();
    tools.extend(contracts);
    tools.retain(|t| t["name"] != SHELL);
    Ok(Offered {
        tools,
        on_the_node,
        reads,
    })
}

/// What a turn is offered, as [`offered`] lists it.
#[derive(Default)]
struct Offered {
    /// Every tool, as MCP lists it.
    tools: Vec<Value>,
    /// The names of the node's contracts among them.
    on_the_node: Vec<String>,
    /// The names of those that only look.
    reads: Vec<String>,
}

/// Takes one turn with Claude Code.
///
/// # Errors
/// As the worker's own turn: [`Stopped::Over`] when HQ or the relay ends it,
/// [`Stopped::Failed`] when something cannot be done.
pub fn take(setup: &Setup) -> Result<Taken, Stopped> {
    turn_with_claude_code(setup).map_err(|stopped| match stopped {
        Stopped::Over(why) => Stopped::Over(quiet(&why, &setup.token)),
        Stopped::Failed(why) => Stopped::Failed(quiet(&why, &setup.token)),
    })
}

fn turn_with_claude_code(setup: &Setup) -> Result<Taken, Stopped> {
    let Some(work) = setup.workbench.clone() else {
        return Err(Stopped::Failed(
            "Claude Code runs only in the workbench".to_owned(),
        ));
    };
    let mut hq = Hq::new(&setup.mcp, &setup.token);
    hq.open()?;
    let node = Node::new(&setup.contracts, &setup.token);
    let Offered {
        tools, on_the_node, ..
    } = offered(&mut hq, &node)?;
    let names: Vec<String> = tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect();
    let lessons = names.iter().any(|n| n == "note_lesson");
    let (briefing, refused) = hq.call("my_turn", &json!({}))?;
    if refused {
        return Err(Stopped::Failed(format!("HQ refused my_turn: {briefing}")));
    }
    let task = task_of(&briefing);
    let who = member_of(&briefing).unwrap_or_else(|| "a member".to_owned());
    let mut briefing = briefing.trim_end().to_owned();
    if let Some(task) = task {
        let (said, _) = hq.call("claim", &json!({ "task": task }))?;
        briefing.push_str(&format!(
            "\n\nBefore your turn began, your task was claimed for you: {said}"
        ));
    }
    // The fork is checked out before Claude Code starts, so it works in /work/repo.
    let mut cwd = work.clone();
    if on_the_node.iter().any(|n| n == REPO_CHECKOUT) {
        let mut did = Ledger {
            reads: Vec::new(),
            calls: Vec::new(),
        };
        let call = Call {
            id: Value::Null,
            name: REPO_CHECKOUT.to_owned(),
            arguments: json!({ "why": "the turn starts in the fork" }),
        };
        let reply = in_the_repo(&node, &work, &who, task, &mut did, &call)?;
        if !reply.refused {
            cwd = work.join(repo::REPO_DIR);
        }
        briefing.push_str(&format!(
            "\n\nBefore your turn began, the fork was checked out: {}",
            reply.text
        ));
    }
    let files = work.join(FILES);
    std::fs::create_dir_all(&files)
        .map_err(|e| Stopped::Failed(format!("{} could not be made: {e}", files.display())))?;
    let ledger = files.join("did.json");
    let rules = files.join("rules.md");
    let mcp = files.join("mcp.json");
    let told = format!(
        "{}{CLAUDE_CODE_RULES}",
        instructions(task.is_some(), &on_the_node, lessons, true)
    );
    let worker = std::env::current_exe()
        .map_or_else(|_| "endora-worker".to_owned(), |p| p.display().to_string());
    let write = |path: &Path, text: &str| {
        std::fs::write(path, text)
            .map_err(|e| Stopped::Failed(format!("{} could not be written: {e}", path.display())))
    };
    write(&rules, &told)?;
    write(
        &mcp,
        &mcp_config(&worker, setup, &who, task, &ledger).to_string(),
    )?;
    // The briefing's map, kept for the hook that gives it back after a compaction (ADR 0126).
    let map_file = files.join("map.md");
    if let Some(block) = map_block(&briefing) {
        write(&map_file, &block)?;
    }
    let claude = std::env::var("ENDORA_CLAUDE").unwrap_or_else(|_| CLAUDE.to_owned());
    let mut command = Command::new(claude);
    command
        .args(arguments(setup, &names, &mcp, &rules))
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in NOT_GIVEN {
        command.env_remove(name);
    }
    for (name, value) in environment(setup) {
        command.env(name, value);
    }
    command.env("ENDORA_MAP_FILE", &map_file);
    let (stdout, code, stderr) = run_within(command, &briefing, TURN_WITHIN)?;
    let kept = read_kept(&ledger);
    if let Some(why) = kept.over {
        return Err(Stopped::Over(why));
    }
    if kept.asked {
        return Ok(Taken::AskedTheOwner);
    }
    let done = finished(&stdout, code, &stderr)?;
    if done.taken != Taken::Answered {
        return Ok(done.taken);
    }
    let did = kept.did.unwrap_or(Ledger {
        reads: Vec::new(),
        calls: Vec::new(),
    });
    let line = format!(
        "{} Claude Code took {} rounds (about ${:.4} by its own reckoning); its own tools' \
         calls are its sandbox-reported receipts.",
        did.line(),
        done.turns,
        done.cost
    );
    let Some(task) = task else {
        return Ok(Taken::Answered);
    };
    let answer = done.text.replace(&setup.token, "[the turn's token]");
    if answer.trim().is_empty() {
        return Ok(Taken::Answered);
    }
    let text = beside(answer.trim(), &line, POST_AT_MOST);
    let (said, refused) = hq.call(
        "post",
        &json!({ "task": task, "text": text, "last_word": true }),
    )?;
    if refused {
        return Err(Stopped::Failed(format!(
            "HQ refused the last word for task {task}: {said}"
        )));
    }
    Ok(Taken::Answered)
}

/// Runs `command` with `prompt` on its stdin for at most `within`: its stdout, exit code and
/// stderr. Out of time, it is killed, and the turn fails.
fn run_within(
    mut command: Command,
    prompt: &str,
    within: Duration,
) -> Result<(String, Option<i32>, String), Stopped> {
    let mut child = command
        .spawn()
        .map_err(|e| Stopped::Failed(format!("Claude Code could not be started: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < within => {
                std::thread::sleep(Duration::from_millis(200))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Stopped::Failed(format!(
                    "Claude Code ran past {} minutes and was stopped",
                    within.as_secs() / 60
                )));
            }
            Err(e) => {
                return Err(Stopped::Failed(format!(
                    "Claude Code could not be watched: {e}"
                )));
            }
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    Ok((stdout, status.code(), stderr))
}

/// The bridge's setup, from the environment Claude Code gives it.
struct Bridge {
    token: String,
    mcp: String,
    contracts: String,
    dir: PathBuf,
    member: String,
    task: Option<i64>,
    ledger: PathBuf,
}

fn bridge_setup(env: impl Fn(&str) -> Option<String>) -> Result<Bridge, String> {
    let given = |name: &str| {
        env(name)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let need = |name: &str| given(name).ok_or_else(|| format!("{name} is not set"));
    Ok(Bridge {
        token: need("ENDORA_TURN_TOKEN")?,
        mcp: need("ENDORA_HQ_MCP")?,
        contracts: need("ENDORA_CONTRACTS")?,
        dir: given("ENDORA_WORKBENCH_DIR")
            .unwrap_or_else(|| "/work".to_owned())
            .into(),
        member: given("ENDORA_MEMBER").unwrap_or_else(|| "a member".to_owned()),
        task: given("ENDORA_TASK").and_then(|t| t.parse().ok()),
        ledger: need("ENDORA_LEDGER")?.into(),
    })
}

/// `value` with every string in it rid of `token`: what the model sends never carries it on.
fn without(value: &Value, token: &str) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace(token, "[the turn's token]")),
        Value::Array(items) => Value::Array(items.iter().map(|v| without(v, token)).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), without(v, token)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The bridge (`endora-worker --mcp`): an MCP server on stdin and stdout, one JSON-RPC message
/// a line, offering HQ's tools and the node's contracts and sending each call where its tool
/// came from. Its exit code is 0, or 1 when it cannot start.
pub fn serve() -> i32 {
    let setup = match bridge_setup(|name| std::env::var(name).ok()) {
        Ok(setup) => setup,
        Err(why) => {
            eprintln!("bridge: {why}");
            return 1;
        }
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut answer = |reply: Value| {
        let _ = writeln!(stdout, "{reply}");
        let _ = stdout.flush();
    };
    let mut hq = Hq::new(&setup.mcp, &setup.token);
    let node = Node::new(&setup.contracts, &setup.token);
    let opened = hq.open().and_then(|()| offered(&mut hq, &node));
    let Offered {
        tools,
        on_the_node,
        reads,
    } = match opened {
        Ok(listed) => listed,
        Err(stopped) => {
            let why = match stopped {
                Stopped::Over(why) | Stopped::Failed(why) => quiet(&why, &setup.token),
            };
            eprintln!("bridge: {why}");
            Offered::default()
        }
    };
    let mut did = Ledger {
        reads,
        calls: Vec::new(),
    };
    let mut answered = Answered::default();
    let mut asked = false;
    let mut over: Option<String> = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let method = msg["method"].as_str().unwrap_or("");
        let result = match method {
            "initialize" => json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER, "version": env!("CARGO_PKG_VERSION") },
            }),
            "ping" => json!({}),
            "tools/list" => {
                let listed: Vec<Value> = tools
                    .iter()
                    .map(|t| json!({ "name": t["name"], "description": t["description"].as_str().unwrap_or(""),
                                      "inputSchema": turn::schema_of(t) }))
                    .collect();
                json!({ "tools": listed })
            }
            "tools/call" => {
                let call = Call {
                    id: Value::Null,
                    name: msg["params"]["name"].as_str().unwrap_or("").to_owned(),
                    arguments: without(
                        &msg["params"]
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                        &setup.token,
                    ),
                };
                let reply = if asked || over.is_some() {
                    Reply {
                        text: "Your turn has ended: stop calling tools and reply in one sentence."
                            .to_owned(),
                        refused: true,
                        asked: false,
                    }
                } else {
                    let done = if is_repo_tool(&call.name) {
                        in_the_repo(
                            &node,
                            &setup.dir,
                            &setup.member,
                            setup.task,
                            &mut did,
                            &call,
                        )
                    } else {
                        run(&mut hq, &node, &on_the_node, &mut answered, &mut did, &call)
                    };
                    match done {
                        Ok(reply) => reply,
                        Err(Stopped::Over(why)) => {
                            over = Some(quiet(&why, &setup.token));
                            Reply {
                                text: "The turn is over: stop now.".to_owned(),
                                refused: true,
                                asked: false,
                            }
                        }
                        Err(Stopped::Failed(why)) => Reply {
                            text: format!("Failed: {}", quiet(&why, &setup.token)),
                            refused: true,
                            asked: false,
                        },
                    }
                };
                let mut text = reply.text.replace(&setup.token, "[the turn's token]");
                if reply.asked {
                    asked = true;
                    text.push_str("\n\nThat ends your turn: reply in one sentence and stop.");
                }
                write_kept(&setup.ledger, &did, asked, over.as_deref());
                json!({ "content": [{ "type": "text", "text": text }], "isError": reply.refused })
            }
            _ => {
                answer(
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "no such method" } }),
                );
                continue;
            }
        };
        answer(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }
    0
}

/// A text cut to [`REPORT_TEXT_MOST`] characters.
fn cut(text: &str) -> String {
    text.chars().take(REPORT_TEXT_MOST).collect()
}

/// Every string in `value` cut to [`REPORT_TEXT_MOST`], and a long list cut to its first few.
fn trimmed(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(cut(s)),
        Value::Array(items) => Value::Array(items.iter().take(20).map(trimmed).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .take(40)
                .map(|(k, v)| (k.clone(), trimmed(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// What the hook sends the node of one call, from what Claude Code gave it: the call, cut
/// short. `None` for the bridge's own tools, which HQ and the node record as they answer.
#[must_use]
pub fn reported(hook: &Value) -> Option<Value> {
    let tool = hook["tool_name"].as_str()?;
    if tool.starts_with("mcp__") {
        return None;
    }
    let mut sent = json!({
        "hook_event_name": hook["hook_event_name"],
        "tool_name": tool,
        "tool_input": trimmed(&hook["tool_input"]),
        "tool_response": trimmed(&hook["tool_response"]),
        "tool_use_id": hook["tool_use_id"],
        "duration_ms": hook["duration_ms"],
    });
    if !hook["error"].is_null() {
        sent["error"] = trimmed(&hook["error"]);
    }
    Some(sent)
}

/// The first words of the map block of a briefing, and the line that ends it (ADR 0126): the team
/// engine writes them (`domain::map`), and a test reads a real one.
const MAP_STARTS: &str = "Your map of the team's world";
const MAP_ENDS: &str = "(End of your map.)";

/// The map block of a turn's briefing, from its first line to the line that ends it, if the
/// briefing opens with one.
#[must_use]
pub fn map_block(briefing: &str) -> Option<String> {
    let start = briefing.find(MAP_STARTS)?;
    let rest = &briefing[start..];
    let mut end = rest.find(MAP_ENDS)? + MAP_ENDS.len();
    if rest[end..].starts_with('\n') {
        end += 1;
    }
    Some(rest[..end].to_owned())
}

/// What the compaction hook says: the map as it stood at the start of the turn, in its own words,
/// from the file the turn kept it in. Nothing without a file.
#[must_use]
pub fn reminder(file: Option<&str>) -> Option<String> {
    let map = std::fs::read_to_string(file?).ok()?;
    if map.trim().is_empty() {
        return None;
    }
    Some(format!(
        "Your context was compacted. Your map of the team's world, as it stood when this turn \
         began (read_world any address on it; report_gap what is not there):\n{map}"
    ))
}

/// The hook (`endora-worker --map`): Claude Code's `SessionStart` hook after a compaction. What it
/// prints is added to the conversation, so the turn does not lose the way to what the team knows.
/// It never blocks the turn: whatever happens it exits 0.
pub fn remind() -> i32 {
    let file = std::env::var("ENDORA_MAP_FILE").ok();
    if let Some(said) = reminder(file.as_deref()) {
        print!("{said}");
    }
    0
}

/// The hook (`endora-worker --receipt`): reads what Claude Code says of one call on stdin and
/// posts it to the node with the turn's token. It never blocks the turn: whatever happens it
/// exits 0 and prints nothing, and a report the node does not take is said on stderr.
pub fn report() -> i32 {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let (Some(url), Some(token)) = (env("ENDORA_RECEIPT_URL"), env("ENDORA_TURN_TOKEN")) else {
        return 0;
    };
    let mut input = String::new();
    let _ = std::io::stdin()
        .take(HOOK_READ_MOST)
        .read_to_string(&mut input);
    let Ok(hook) = serde_json::from_str::<Value>(&input) else {
        return 0;
    };
    let Some(sent) = reported(&hook) else {
        return 0;
    };
    let agent = turn::agent(Duration::from_secs(10));
    let posted = agent
        .post(&url)
        .header("authorization", &format!("Bearer {token}"))
        .send_json(&sent);
    match posted {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => eprintln!("receipt: the node answered {}", response.status().as_u16()),
        Err(e) => eprintln!(
            "receipt: the node could not be reached: {}",
            quiet(&e.to_string(), &token)
        ),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::{Api, Outcome};

    fn setup() -> Setup {
        Setup {
            token: "a".repeat(64),
            mcp: "http://192.168.1.10:8788/team-gateway/mcp".to_owned(),
            model_url: "http://192.168.1.10:8788/team-relay/v1".to_owned(),
            contracts: "http://192.168.1.10:8788/team-relay/contracts".to_owned(),
            model: "claude-haiku-5-5".to_owned(),
            api: Api::Anthropic,
            most_rounds: 30,
            workbench: Some("/work".into()),
            effort: Some("low".to_owned()),
            claude_code: true,
        }
    }

    #[test]
    fn claude_code_reaches_the_relay_and_reports_beside_the_contracts() {
        assert_eq!(
            relay_base("http://192.168.1.10:8788/team-relay/v1"),
            "http://192.168.1.10:8788/team-relay"
        );
        assert_eq!(
            relay_base("http://192.168.1.10:8788/team-relay/v1/"),
            "http://192.168.1.10:8788/team-relay"
        );
        assert_eq!(
            receipt_url("http://192.168.1.10:8788/team-relay/contracts"),
            "http://192.168.1.10:8788/team-relay/receipt"
        );
    }

    #[test]
    fn the_command_line_is_headless_held_to_the_brain_and_names_every_tool_it_may_use() {
        let s = setup();
        let args = arguments(
            &s,
            &["my_turn".to_owned(), "repo_submit".to_owned()],
            Path::new("/work/.endora/mcp.json"),
            Path::new("/work/.endora/rules.md"),
        );
        let at = |flag: &str| {
            let i = args
                .iter()
                .position(|a| a == flag)
                .unwrap_or_else(|| panic!("{flag}"));
            args[i + 1].as_str()
        };
        assert_eq!(args[0], "-p");
        assert_eq!(at("--output-format"), "json");
        assert_eq!(at("--model"), "claude-haiku-5-5");
        assert_eq!(at("--effort"), "low");
        assert_eq!(at("--max-turns"), "30");
        assert_eq!(at("--permission-mode"), "dontAsk");
        assert_eq!(at("--permission-prompts"), "none");
        assert_eq!(
            at("--setting-sources"),
            "user",
            "a fork's own settings are never read"
        );
        assert_eq!(at("--mcp-config"), "/work/.endora/mcp.json");
        assert_eq!(at("--append-system-prompt-file"), "/work/.endora/rules.md");
        assert!(args.contains(&"--strict-mcp-config".to_owned()));
        assert!(args.contains(&"--no-session-persistence".to_owned()));
        let list = |flag: &str| -> Vec<&str> {
            let i = args.iter().position(|a| a == flag).unwrap();
            args[i + 1..]
                .iter()
                .take_while(|a| !a.starts_with("--"))
                .map(String::as_str)
                .collect()
        };
        assert_eq!(list("--tools"), BUILT_IN);
        assert_eq!(
            list("--allowedTools"),
            [
                "mcp__endora__my_turn",
                "mcp__endora__repo_submit",
                "Read",
                "Edit",
                "Write",
                "Glob",
                "Grep",
                "Bash(git *)",
                "Bash(ansible-lint *)",
                "Bash(yamllint *)",
                "Bash(ansible-playbook --syntax-check *)",
                "Bash(ls *)",
                "Bash(cat *)",
            ]
        );
        assert_eq!(list("--disallowedTools"), ["WebFetch", "WebSearch"]);
        assert!(
            !args.iter().any(|a| a.contains(&s.token)),
            "never the token in an argument"
        );
        let mut no_effort = setup();
        no_effort.effort = None;
        assert!(
            !arguments(&no_effort, &[], Path::new("m"), Path::new("r"))
                .contains(&"--effort".to_owned())
        );
    }

    #[test]
    fn claude_codes_environment_points_every_model_at_the_brain_and_the_token_at_the_relay() {
        let s = setup();
        let env = environment(&s);
        let of = |name: &str| {
            env.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(
            of("ANTHROPIC_BASE_URL"),
            Some("http://192.168.1.10:8788/team-relay")
        );
        assert_eq!(of("ANTHROPIC_AUTH_TOKEN"), Some(s.token.as_str()));
        for alias in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
        ] {
            assert_eq!(of(alias), Some("claude-haiku-5-5"), "{alias}");
        }
        assert_eq!(of("ANTHROPIC_API_KEY"), None, "never a key");
        assert!(
            NOT_GIVEN.contains(&"ANTHROPIC_API_KEY")
                && NOT_GIVEN.contains(&"CLAUDE_CODE_OAUTH_TOKEN")
        );
        let config = mcp_config(
            "/usr/local/bin/endora-worker",
            &s,
            "Rae",
            Some(3),
            Path::new("/work/.endora/did.json"),
        );
        let server = &config["mcpServers"]["endora"];
        assert_eq!(server["type"], "stdio");
        assert_eq!(server["args"], json!(["--mcp"]));
        assert_eq!(
            server["env"]["ENDORA_TURN_TOKEN"], "${ENDORA_TURN_TOKEN}",
            "a name, never the secret"
        );
        assert_eq!(server["env"]["ENDORA_TASK"], "3");
        assert!(!config.to_string().contains(&s.token));
    }

    #[test]
    fn claude_codes_result_is_read_as_the_turn_it_was() {
        // As Claude Code 2.1.296 printed it, cut down.
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"Tidied the role and submitted hq/task-3-tidy.","session_id":"s","total_cost_usd":0.0123,"num_turns":9,"api_error_status":null}"#;
        let done = finished(ok, Some(0), "").unwrap();
        assert_eq!(done.taken, Taken::Answered);
        assert_eq!(done.text, "Tidied the role and submitted hq/task-3-tidy.");
        assert_eq!((done.turns, done.cost), (9, 0.0123));
        let rounds =
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"num_turns":30}"#;
        assert_eq!(
            finished(rounds, Some(1), "").unwrap().taken,
            Taken::OutOfRounds
        );
        for status in [401, 402] {
            let over = format!(
                r#"{{"type":"result","subtype":"success","is_error":true,"result":"API Error","api_error_status":{status}}}"#
            );
            assert!(
                matches!(finished(&over, Some(1), ""), Err(Stopped::Over(_))),
                "{status}"
            );
        }
        let failed = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom"}"#;
        assert!(matches!(
            finished(failed, Some(1), ""),
            Err(Stopped::Failed(_))
        ));
        let Err(Stopped::Failed(why)) = finished("not json", Some(2), "a\nError: no config") else {
            panic!("not Claude Code's JSON");
        };
        assert!(
            why.contains("exit 2") && why.contains("Error: no config"),
            "{why}"
        );
    }

    #[test]
    fn the_hook_reports_claude_codes_own_tools_cut_short_and_never_the_bridges() {
        let long = "x".repeat(10_000);
        let hook = json!({
            "session_id": "s", "transcript_path": "/work/.claude/t.jsonl", "cwd": "/work/repo",
            "hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_input": { "command": "git diff", "description": "look" },
            "tool_response": { "stdout": long, "stderr": "", "interrupted": false },
            "tool_use_id": "toolu_1", "duration_ms": 40,
        });
        let sent = reported(&hook).unwrap();
        assert_eq!(sent["tool_name"], "Bash");
        assert_eq!(sent["tool_input"]["command"], "git diff");
        assert_eq!(
            sent["tool_response"]["stdout"].as_str().unwrap().len(),
            REPORT_TEXT_MOST
        );
        assert_eq!(
            (sent["tool_use_id"].clone(), sent["duration_ms"].clone()),
            (json!("toolu_1"), json!(40))
        );
        assert!(sent.get("transcript_path").is_none(), "only the call");
        assert_eq!(reported(&json!({ "tool_name": "mcp__endora__post" })), None);
        assert_eq!(reported(&json!({})), None);
    }

    #[test]
    fn what_the_model_sends_on_never_carries_the_token() {
        let token = "f".repeat(64);
        let sent =
            json!({ "task": 3, "text": format!("my token is {token}"), "list": [token.clone()] });
        let clean = without(&sent, &token);
        assert!(!clean.to_string().contains(&token));
        assert_eq!(clean["task"], 3);
    }

    #[test]
    fn the_bridges_line_of_what_it_did_survives_the_file_between_the_processes() {
        let dir = std::env::temp_dir().join(format!("endora-kept-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("did.json");
        assert!(read_kept(&path).did.is_none(), "nothing yet");
        let did = Ledger {
            reads: Vec::new(),
            calls: vec![
                ("read the home lab's state".to_owned(), true, Outcome::Done),
                (
                    "tried to submit branch hq/x, refused".to_owned(),
                    false,
                    Outcome::Refused,
                ),
            ],
        };
        write_kept(&path, &did, true, None);
        let kept = read_kept(&path);
        assert!(kept.asked && kept.over.is_none());
        assert_eq!(kept.did.unwrap().line(), did.line());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_map_is_found_in_a_real_briefing_exactly_as_the_team_engine_composes_it() {
        use endora_team::domain::map::{self, Entry};
        use endora_team::domain::situations::Weight;
        let entries = vec![
            Entry::new(
                "world:role/developer",
                "How a developer works here.",
                Weight::Core,
            ),
            Entry::new("world:area/2", "Ada's area. Kept by Ada.", Weight::Core),
            Entry::new("world:area/9", "Another area.", Weight::Optional),
        ];
        let composed = map::compose(entries, 200);
        let briefing = format!(
            "You are Linus, the team's developer.\n{}Your task: fix the proxy (#3).\n",
            composed.text
        );
        let found = map_block(&briefing).expect("the map is in the briefing");
        assert_eq!(
            found, composed.text,
            "the worker finds what the engine wrote"
        );
        assert_eq!(Some(found.clone()), map::map_block(&briefing));
        assert!(found.contains("world:area/2") && !found.contains("Your task"));
        assert_eq!(map_block("You are Linus.\nYour task: fix it.\n"), None);
    }

    #[test]
    fn after_a_compaction_the_hook_gives_the_turn_its_map_back_and_says_nothing_without_one() {
        let dir = std::env::temp_dir().join(format!("endora-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("map.md");
        std::fs::write(
            &path,
            "Your map of the team's world (...):\n- world:house\n(End of your map.)\n",
        )
        .unwrap();
        let said = reminder(path.to_str()).expect("a reminder");
        assert!(
            said.contains("compacted") && said.contains("- world:house"),
            "{said}"
        );
        assert_eq!(reminder(None), None, "no map file named");
        assert_eq!(
            reminder(dir.join("none.md").to_str()),
            None,
            "no map file kept"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_workbench_hands_the_map_back_when_claude_code_compacts_and_still_reports_receipts() {
        let settings: Value = serde_json::from_str(include_str!(
            "../../../deploy/workbench/managed-settings.json"
        ))
        .expect("the managed settings are JSON");
        let start = &settings["hooks"]["SessionStart"];
        assert_eq!(start[0]["matcher"], "compact", "only after a compaction");
        assert_eq!(
            start[0]["hooks"][0]["command"],
            "/usr/local/bin/endora-worker --map"
        );
        assert_eq!(
            settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "/usr/local/bin/endora-worker --receipt"
        );
    }
}
