//! A **synchronous** stdio MCP client (ADR 0054).
//!
//! Speaks the MCP stdio transport — newline-delimited JSON-RPC 2.0 — to a local
//! subprocess: `initialize` + `notifications/initialized`, then `tools/list` and
//! `tools/call`. Kept sync on purpose, to match [`McpClient`]/[`CapabilityRunner`]
//! ([`crate::infrastructure`]) without dragging an async runtime into this crate;
//! the wire format is small and stable enough to own (no new dependency).
//!
//! The protocol logic runs over a [`LineIo`] seam (send a request line, receive
//! reply lines), so the full handshake/list/call flow is tested hermetically with a
//! fake — no subprocess, no timing. [`StdioMcpClient`] is the real wiring: a child
//! process whose stdout is drained by a reader thread into a channel, so a hung or
//! chatty server can't block a turn past [`CALL_TIMEOUT`].

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::infrastructure::{McpClient, McpResource, McpToolInfo};

/// The MCP protocol revision this client speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";
/// How long to wait for any single server reply before giving up (ADR 0054 health:
/// a hung server fails this call rather than hanging the turn).
const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// A longer budget for the initial `initialize` reply: a `npx …` server downloads
/// its package on first run, which can take a while before it says a word.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(90);

/// A line-oriented duplex to an MCP server: send one request line, receive reply
/// lines. Abstracted so the session logic is testable without a real process.
trait LineIo {
    /// Sends one request line (the newline is added).
    fn send(&mut self, line: &str) -> Result<(), String>;
    /// Receives the next line the server emitted (blocks, up to `timeout`).
    fn recv(&mut self, timeout: Duration) -> Result<String, String>;
}

/// Sends a JSON-RPC request with `id` and reads reply lines until the response
/// carrying that same `id` arrives — skipping notifications and any other-id
/// messages the server interleaves. Returns the `result` value (or an error if the
/// server replied with one). Waits up to `timeout` for each reply line.
fn request(
    io: &mut dyn LineIo,
    id: u64,
    method: &str,
    params: Option<Value>,
    timeout: Duration,
) -> Result<Value, String> {
    let mut req = json!({ "jsonrpc": "2.0", "id": id, "method": method });
    if let Some(p) = params {
        req["params"] = p;
    }
    io.send(&req.to_string())?;
    loop {
        let line = io.recv(timeout)?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value =
            serde_json::from_str(line).map_err(|e| format!("bad JSON from MCP server: {e}"))?;
        // Skip notifications (no id) and replies to other requests.
        if msg.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(err) = msg.get("error") {
            return Err(format!("MCP server error: {err}"));
        }
        return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
    }
}

/// Sends a JSON-RPC notification (no id, no reply expected).
fn notify(io: &mut dyn LineIo, method: &str) -> Result<(), String> {
    io.send(&json!({ "jsonrpc": "2.0", "method": method }).to_string())
}

/// Runs the MCP handshake: `initialize`, then the `initialized` notification.
fn handshake(io: &mut dyn LineIo, next_id: &mut u64) -> Result<(), String> {
    let id = *next_id;
    *next_id += 1;
    let params = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": { "name": "endora", "version": env!("CARGO_PKG_VERSION") },
    });
    request(io, id, "initialize", Some(params), HANDSHAKE_TIMEOUT)?;
    notify(io, "notifications/initialized")
}

/// Parses a `tools/list` result into tool infos. Shared with the HTTP transport, so
/// the wire shape lives in one place.
pub(crate) fn tools_from_result(result: &Value) -> Vec<McpToolInfo> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?.to_owned();
                    let description = t
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    // The model needs the input shape to actually call the tool. MCP
                    // spells it `inputSchema`; keep it verbatim (an object) if present.
                    let input_schema = t.get("inputSchema").filter(|s| s.is_object()).cloned();
                    Some(McpToolInfo {
                        name,
                        description,
                        input_schema,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Extracts the text content of a `tools/call` result (joining text parts), or an
/// error if the server marked the result `isError`. Shared with the HTTP transport.
pub(crate) fn text_from_call_result(result: &Value, tool: &str) -> Result<String, String> {
    // MCP results carry a `content` array of typed parts; we relay the text parts.
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|c| c.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(if text.is_empty() {
            format!("the '{tool}' tool reported an error")
        } else {
            text
        });
    }
    // Fall back to the raw result if the server returned no text content.
    let text = if text.is_empty() {
        result.to_string()
    } else {
        text
    };
    // Render a machine-shaped envelope into a plain sentence, if we recognise one.
    match summarize_assist(&text) {
        Some(rendered) => rendered,
        None => Ok(text),
    }
}

/// Names the targets an Assist response acted on, **with what they are**:
/// `Kitchen (area)`, `Kitchen Table (light)`.
///
/// The entity kind comes from the domain prefix of HA's id (`light.kitchen_table`).
/// Including it is not decoration: given only bare names, a model fills the gap from
/// whatever vocabulary is nearby, and this server exposes ten media-player tools. It
/// duly reported adjusting "the media player in the Kitchen area" after a call to
/// `HassLightSet`. Naming the domain removes the room for that invention.
fn assist_targets(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|t| {
                    let name = t.get("name").and_then(Value::as_str)?;
                    let kind = t.get("type").and_then(Value::as_str).unwrap_or_default();
                    if kind == "area" {
                        return Some(format!("{name} (area)"));
                    }
                    let domain = t
                        .get("id")
                        .and_then(Value::as_str)
                        .and_then(|id| id.split_once('.'))
                        .map(|(domain, _)| domain.replace('_', " "));
                    Some(
                        domain
                            .map_or_else(|| name.to_owned(), |domain| format!("{name} ({domain})")),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Renders a Home Assistant **Assist** response into a sentence.
///
/// HA answers with a machine-shaped envelope — `response_type` plus
/// `data.success`/`data.failed` — and frequently an **empty** `speech` object. Handed
/// to a model raw, a completed action reads like a search result. Observed live: the
/// butler turned the kitchen lights off, got back
/// `{"speech":{},"response_type":"action_done","data":{"success":[…],"failed":[]}}`,
/// and told the person *"I found some items related to the kitchen area… would you
/// like me to perform an action?"* — after already having performed it.
///
/// This describes **what the tool returned**, not what the butler should say. The
/// butler still writes its own reply in its own voice; ADR 0053's objection is to
/// replacing that voice with canned strings, not to a tool adapter making its own
/// output legible — which is exactly what the built-in capabilities' `summarize`
/// already does. A model cannot be grounded in a result it cannot parse.
///
/// Returns `None` for anything that is not an Assist envelope, so other MCP servers
/// pass through untouched.
fn summarize_assist(text: &str) -> Option<Result<String, String>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let response_type = value.get("response_type").and_then(Value::as_str)?;
    // HA's own sentence, when it bothered to write one, is the most faithful thing
    // we can pass on.
    let spoken = value
        .get("speech")
        .and_then(|s| s.get("plain"))
        .and_then(|p| p.get("speech"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let data = value.get("data");
    let failed = data.map(|d| assist_targets(d.get("failed").unwrap_or(&Value::Null)));
    let succeeded = data.map(|d| assist_targets(d.get("success").unwrap_or(&Value::Null)));

    if response_type == "error" {
        let detail = spoken.map_or_else(
            || {
                data.and_then(|d| d.get("code"))
                    .and_then(Value::as_str)
                    .unwrap_or("no reason given")
                    .to_owned()
            },
            std::borrow::ToOwned::to_owned,
        );
        return Some(Err(format!("Home Assistant refused the request: {detail}")));
    }

    // A partial or total failure must reach the turn as an error, so the failure
    // path engages rather than the model deciding how bad it was.
    if let Some(failed) = failed.as_ref().filter(|f| !f.is_empty()) {
        return Some(Err(format!(
            "Home Assistant could not act on: {}",
            failed.join(", ")
        )));
    }

    match response_type {
        "action_done" => {
            let targets = succeeded.unwrap_or_default();
            Some(Ok(if targets.is_empty() {
                spoken.map_or_else(
                    || "The action completed, but Home Assistant named no targets.".to_owned(),
                    std::borrow::ToOwned::to_owned,
                )
            } else {
                format!(
                    "The action completed successfully on: {}.{}",
                    targets.join(", "),
                    spoken.map_or(String::new(), |s| format!(" {s}"))
                )
            }))
        }
        "query_answer" => spoken.map_or_else(
            || {
                let targets = succeeded.unwrap_or_default();
                (!targets.is_empty())
                    .then(|| Ok(format!("Home Assistant reports: {}.", targets.join(", "))))
            },
            |s| Some(Ok(s.to_owned())),
        ),
        _ => spoken.map(|s| Ok(s.to_owned())),
    }
}

/// Lists the server's tools (`tools/list`).
fn list_tools(io: &mut dyn LineIo, next_id: &mut u64) -> Result<Vec<McpToolInfo>, String> {
    let id = *next_id;
    *next_id += 1;
    let result = request(io, id, "tools/list", None, CALL_TIMEOUT)?;
    Ok(tools_from_result(&result))
}

/// Parses a `resources/list` result. Shared with the HTTP transport so the wire shape lives
/// in one place, exactly as `tools_from_result` does.
pub(crate) fn resources_from_result(result: &Value) -> Vec<McpResource> {
    result
        .get("resources")
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|r| {
                    // The uri is the only thing required: a resource nobody can address
                    // cannot be read, so it is not a resource.
                    let uri = r.get("uri")?.as_str()?.to_owned();
                    Some(McpResource {
                        uri,
                        name: r
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        description: r
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parses a `resources/read` result into text.
///
/// A resource may arrive in several parts. Text parts are joined; a `blob` is skipped, because
/// binary has no reading a state comparison could use.
pub(crate) fn contents_from_result(result: &Value) -> String {
    result
        .get("contents")
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|c| c.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

/// Lists what the server offers to be read (`resources/list`).
///
/// A server that does not implement the method answers with a JSON-RPC error, and that is
/// **not** a failure — it is the majority of servers saying they only have tools. Returning
/// an empty list keeps a tools-only server from looking broken.
fn list_resources(io: &mut dyn LineIo, next_id: &mut u64) -> Result<Vec<McpResource>, String> {
    let id = *next_id;
    *next_id += 1;
    match request(io, id, "resources/list", None, CALL_TIMEOUT) {
        Ok(result) => Ok(resources_from_result(&result)),
        Err(_) => Ok(Vec::new()),
    }
}

/// Reads one resource (`resources/read`).
fn read_resource(io: &mut dyn LineIo, next_id: &mut u64, uri: &str) -> Result<String, String> {
    let id = *next_id;
    *next_id += 1;
    let result = request(
        io,
        id,
        "resources/read",
        Some(json!({ "uri": uri })),
        CALL_TIMEOUT,
    )?;
    Ok(contents_from_result(&result))
}

/// Calls one tool (`tools/call`), returning its text content. `input_json` is the
/// arguments object; blank/invalid input becomes `{}`.
fn call_tool(
    io: &mut dyn LineIo,
    next_id: &mut u64,
    tool: &str,
    input_json: &str,
) -> Result<String, String> {
    let id = *next_id;
    *next_id += 1;
    let args: Value = serde_json::from_str(input_json.trim()).unwrap_or_else(|_| json!({}));
    let params = json!({ "name": tool, "arguments": args });
    let result = request(io, id, "tools/call", Some(params), CALL_TIMEOUT)?;
    text_from_call_result(&result, tool)
}

/// The real line I/O: writes to the child's stdin, and reads lines the reader thread
/// drained from its stdout into a channel (so a hung server times out).
struct StdioIo {
    stdin: ChildStdin,
    rx: Receiver<String>,
    next_id: u64,
    child: Child,
}

impl LineIo for StdioIo {
    fn send(&mut self, line: &str) -> Result<(), String> {
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.write_all(b"\n"))
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("failed to send to MCP server: {e}"))
    }

    fn recv(&mut self, timeout: Duration) -> Result<String, String> {
        match self.rx.recv_timeout(timeout) {
            Ok(line) => Ok(line),
            Err(RecvTimeoutError::Timeout) => Err("the MCP server timed out".to_owned()),
            Err(RecvTimeoutError::Disconnected) => {
                Err("the MCP server closed the connection".to_owned())
            }
        }
    }
}

impl Drop for StdioIo {
    fn drop(&mut self) {
        // End the subprocess when the client goes away — no leaked servers.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A connected local MCP server, spoken to over stdio (ADR 0054). Constructing one
/// spawns the subprocess and completes the handshake; a shared [`Mutex`] serializes
/// the request/response cycle so it is `Send + Sync` behind [`McpClient`].
pub struct StdioMcpClient {
    io: Mutex<StdioIo>,
    /// What it takes to start this server again. A subprocess is a session like any
    /// other and dies like one — crashed, killed, out of memory — and the pipes it
    /// left behind fail every call afterwards (ADR 0073). Held so a death can be
    /// answered rather than merely reported.
    how_to_start: (
        String,
        Vec<String>,
        std::collections::BTreeMap<String, String>,
    ),
}

impl StdioMcpClient {
    /// Spawns `command args…`, drains its stdout on a reader thread, and runs the
    /// MCP handshake.
    ///
    /// # Errors
    /// A human-readable message if the process can't be started or the handshake
    /// fails (which is exactly the "unhealthy server ⇒ skipped" signal the adapter
    /// relies on).
    pub fn spawn(command: &str, args: &[String]) -> Result<Self, String> {
        Self::spawn_with_env(command, args, &std::collections::BTreeMap::new())
    }

    /// Spawns the server with extra environment for the child — how most servers take
    /// their credentials (e.g. `GITHUB_TOKEN`). Otherwise identical to [`Self::spawn`].
    ///
    /// # Errors
    /// A human-readable message if the process can't be started or the handshake
    /// fails (the "unhealthy server ⇒ skipped" signal the adapter relies on).
    pub fn spawn_with_env(
        command: &str,
        args: &[String],
        env: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to start MCP server '{command}': {e}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "MCP server has no stdin".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "MCP server has no stdout".to_owned())?;
        let (tx, rx) = channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break; // client gone
                        }
                    }
                    Err(_) => break, // stdout closed
                }
            }
        });
        let mut io = StdioIo {
            stdin,
            rx,
            next_id: 1,
            child,
        };
        let mut id = io.next_id;
        handshake(&mut io, &mut id)?;
        io.next_id = id;
        Ok(Self {
            io: Mutex::new(io),
            how_to_start: (command.to_owned(), args.to_vec(), env.clone()),
        })
    }

    /// Whether an error means the subprocess is gone rather than merely slow.
    ///
    /// A closed channel is the reader thread having ended, which only happens when
    /// stdout closed — the process is over. A broken pipe on the way in says the same
    /// from the other side. A timeout is a server thinking, and respawning on that
    /// would kill work in progress.
    fn process_is_gone(why: &str) -> bool {
        let why = why.to_lowercase();
        why.contains("closed the connection")
            || why.contains("broken pipe")
            || why.contains("failed to send to mcp server")
    }

    /// Runs one call, restarting the server once if it turns out to have died.
    ///
    /// The heartbeat cannot notice this: the tool list is cached from the last good
    /// connect, so a dead server keeps advertising everything it used to do while
    /// every call fails. Restarting where the death is visible is the same answer
    /// ADR 0073 gives the HTTP transports.
    fn healing<T>(&self, run: impl Fn(&mut StdioIo) -> Result<T, String>) -> Result<T, String> {
        let first = {
            let mut io = self
                .io
                .lock()
                .map_err(|_| "MCP client poisoned".to_owned())?;
            run(&mut io)
        };
        let Err(why) = first else {
            return first;
        };
        if !Self::process_is_gone(&why) {
            return Err(why);
        }
        let (command, args, env) = &self.how_to_start;
        // A fresh process is a fresh handshake; `spawn_with_env` does both.
        let fresh = Self::spawn_with_env(command, args, env)
            .map_err(|e| format!("{why} (restarting it failed too: {e})"))?;
        let mut mine = self
            .io
            .lock()
            .map_err(|_| "MCP client poisoned".to_owned())?;
        let theirs = fresh
            .io
            .into_inner()
            .map_err(|_| "MCP client poisoned".to_owned())?;
        *mine = theirs;
        run(&mut mine)
    }
}

impl McpClient for StdioMcpClient {
    fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
        self.healing(|io| {
            let mut id = io.next_id;
            let out = list_tools(io, &mut id);
            io.next_id = id;
            out
        })
    }

    fn call(&self, tool: &str, input_json: &str) -> Result<String, String> {
        self.healing(|io| {
            let mut id = io.next_id;
            let out = call_tool(io, &mut id, tool, input_json);
            io.next_id = id;
            out
        })
    }

    fn list_resources(&self) -> Result<Vec<McpResource>, String> {
        let mut io = self
            .io
            .lock()
            .map_err(|_| "MCP client poisoned".to_owned())?;
        let mut id = io.next_id;
        let out = list_resources(&mut *io, &mut id);
        io.next_id = id;
        out
    }

    fn read_resource(&self, uri: &str) -> Result<String, String> {
        let mut io = self
            .io
            .lock()
            .map_err(|_| "MCP client poisoned".to_owned())?;
        let mut id = io.next_id;
        let out = read_resource(&mut *io, &mut id, uri);
        io.next_id = id;
        out
    }
}

#[cfg(test)]
mod a_dead_subprocess {
    //! ADR 0073, stdio half. A subprocess is a session and dies like one; the pipes it
    //! leaves behind fail every call, while the cached tool list keeps the server
    //! looking healthy to the heartbeat.

    use super::StdioMcpClient;

    #[test]
    fn a_gone_process_is_told_from_a_slow_one() {
        // Both directions the transport actually produces.
        assert!(StdioMcpClient::process_is_gone(
            "the MCP server closed the connection"
        ));
        assert!(StdioMcpClient::process_is_gone(
            "failed to send to MCP server: Broken pipe (os error 32)"
        ));
        // A server thinking is not a server gone — restarting would kill work in
        // progress and lose whatever it was about to answer.
        for slow in [
            "the MCP server timed out",
            "MCP server error: tool not found",
            "bad JSON from MCP server: expected value",
        ] {
            assert!(
                !StdioMcpClient::process_is_gone(slow),
                "would have restarted on: {slow}"
            );
        }
    }

    #[test]
    fn a_server_that_will_not_start_says_both_what_failed_and_why_it_could_not_recover() {
        // The honest end of the road: the original failure is not swallowed by the
        // recovery attempt's own failure.
        let why = match StdioMcpClient::spawn("definitely-not-a-real-command-xyz", &[]) {
            Err(why) => why,
            Ok(_) => panic!("a missing command must not appear to start"),
        };
        assert!(
            why.contains("failed to start MCP server"),
            "unhelpful failure: {why}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{LineIo, call_tool, handshake, list_resources, list_tools, read_resource};
    use std::collections::VecDeque;
    use std::time::Duration;

    /// A scripted server: records the lines we send, and hands back pre-canned reply
    /// lines in order.
    struct FakeIo {
        sent: Vec<String>,
        replies: VecDeque<String>,
    }

    impl FakeIo {
        fn new(replies: &[&str]) -> Self {
            Self {
                sent: Vec::new(),
                replies: replies.iter().map(|s| (*s).to_owned()).collect(),
            }
        }
    }

    impl LineIo for FakeIo {
        fn send(&mut self, line: &str) -> Result<(), String> {
            self.sent.push(line.to_owned());
            Ok(())
        }
        fn recv(&mut self, _timeout: Duration) -> Result<String, String> {
            self.replies
                .pop_front()
                .ok_or_else(|| "no more replies scripted".to_owned())
        }
    }

    #[test]
    fn full_session_handshake_list_and_call() {
        // Server replies, in the order the client will read them. The tools/list read
        // is preceded by a stray notification to prove it's skipped.
        let mut io = FakeIo::new(&[
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/message","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"read_file","description":"reads a file"}]}}"#,
            r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"hello"}]}}"#,
        ]);
        let mut id = 1;

        handshake(&mut io, &mut id).unwrap();
        let tools = list_tools(&mut io, &mut id).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[0].description, "reads a file");

        let out = call_tool(&mut io, &mut id, "read_file", "{\"path\":\"/x\"}").unwrap();
        assert_eq!(out, "hello");

        // The client sent the right methods, in order, with incrementing ids, plus
        // the initialized notification after the handshake.
        assert!(
            io.sent[0].contains("\"method\":\"initialize\"") && io.sent[0].contains("\"id\":1")
        );
        assert!(io.sent[1].contains("notifications/initialized") && !io.sent[1].contains("\"id\""));
        assert!(
            io.sent[2].contains("\"method\":\"tools/list\"") && io.sent[2].contains("\"id\":2")
        );
        assert!(
            io.sent[3].contains("\"method\":\"tools/call\"") && io.sent[3].contains("\"id\":3")
        );
        assert!(io.sent[3].contains("read_file") && io.sent[3].contains("/x"));
    }

    // --- Resources (ADR 0058 amendment) ---
    //
    // MCP standardises `resources/list` and `resources/read`, which is the state read the
    // watch loop needs. Endora spoke only the tools half of the protocol, so every service it
    // wanted to *watch* had to be written in Rust here. These tests come first because the
    // wire shape is the whole feature.

    #[test]
    fn a_server_lists_its_resources() {
        let mut io = FakeIo::new(&[r#"{"jsonrpc":"2.0","id":1,"result":{"resources":[
                {"uri":"house://light.kitchen","name":"Kitchen light","description":"a lamp","mimeType":"text/plain"},
                {"uri":"house://person.john","name":"john"}
            ]}}"#]);
        let mut id = 1;
        let found = list_resources(&mut io, &mut id).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].uri, "house://light.kitchen");
        assert_eq!(found[0].name, "Kitchen light");
        assert_eq!(found[0].description, "a lamp");
        assert_eq!(found[1].uri, "house://person.john");
        assert!(
            found[1].description.is_empty(),
            "a missing description is empty, not a failure"
        );
        assert!(io.sent[0].contains("\"method\":\"resources/list\""));
    }

    #[test]
    fn a_resource_without_a_uri_is_dropped() {
        // A resource nobody can address cannot be read, so it is not a resource.
        let mut io = FakeIo::new(&[
            r#"{"jsonrpc":"2.0","id":1,"result":{"resources":[{"name":"nameless"}]}}"#,
        ]);
        let mut id = 1;
        assert!(list_resources(&mut io, &mut id).unwrap().is_empty());
    }

    #[test]
    fn a_server_with_no_resources_is_not_an_error() {
        // Most MCP servers expose tools only. Not supporting resources is the ordinary case
        // and must read as "nothing to watch", never as a broken server — otherwise every
        // tools-only server would start reporting trouble.
        let mut io = FakeIo::new(&[r#"{"jsonrpc":"2.0","id":1,"result":{}}"#]);
        let mut id = 1;
        assert!(list_resources(&mut io, &mut id).unwrap().is_empty());

        let mut io = FakeIo::new(&[
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#,
        ]);
        let mut id = 1;
        assert!(
            list_resources(&mut io, &mut id).unwrap().is_empty(),
            "an unimplemented method means no resources, not a failure"
        );
    }

    #[test]
    fn reading_a_resource_returns_its_text() {
        let mut io = FakeIo::new(&[r#"{"jsonrpc":"2.0","id":1,"result":{"contents":[
                {"uri":"house://light.kitchen","mimeType":"text/plain","text":"on"}
            ]}}"#]);
        let mut id = 1;
        let text = read_resource(&mut io, &mut id, "house://light.kitchen").unwrap();
        assert_eq!(text, "on");
        assert!(io.sent[0].contains("\"method\":\"resources/read\""));
        assert!(io.sent[0].contains("house://light.kitchen"));
    }

    #[test]
    fn several_text_parts_are_joined_and_binary_ones_are_skipped() {
        // A resource may come back in parts, and a blob has no text a state reader could use.
        let mut io = FakeIo::new(&[r#"{"jsonrpc":"2.0","id":1,"result":{"contents":[
                {"uri":"x","text":"on"},
                {"uri":"x","blob":"aGVsbG8="},
                {"uri":"x","text":"since 9am"}
            ]}}"#]);
        let mut id = 1;
        assert_eq!(
            read_resource(&mut io, &mut id, "x").unwrap(),
            "on since 9am"
        );
    }

    #[test]
    fn a_resource_that_cannot_be_read_says_so() {
        // Unlike listing, a failed *read* is a real failure: something was named and could
        // not be fetched, which the watch loop should see rather than silently treat as empty.
        let mut io = FakeIo::new(&[
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32002,"message":"no such resource"}}"#,
        ]);
        let mut id = 1;
        let failed = read_resource(&mut io, &mut id, "house://gone");
        // The exact wording is `request`'s, shared with every other call on this transport;
        // what matters here is that the reason reaches the caller rather than being swallowed
        // the way a failed *list* deliberately is.
        assert!(
            failed
                .as_ref()
                .is_err_and(|e| e.contains("no such resource")),
            "expected the server's reason to surface, got {failed:?}"
        );
    }

    #[test]
    fn a_tool_error_surfaces_as_an_err() {
        let mut io = FakeIo::new(&[
            r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"boom"}],"isError":true}}"#,
        ]);
        let mut id = 1;
        let out = call_tool(&mut io, &mut id, "risky", "{}");
        assert_eq!(out, Err("boom".to_owned()));
    }

    #[test]
    fn tools_from_result_keeps_the_input_schema() {
        let result = serde_json::json!({
            "tools": [
                {
                    "name": "HassTurnOn",
                    "description": "Turns on a device",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"]
                    }
                },
                { "name": "GetDateTime", "description": "the time" }
            ]
        });
        let tools = super::tools_from_result(&result);
        assert_eq!(tools.len(), 2);
        // The schema is retained so the model can learn how to call the tool.
        let schema = tools[0].input_schema.as_ref().expect("schema kept");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        // A tool with no schema simply has none.
        assert!(tools[1].input_schema.is_none());
    }

    #[test]
    fn a_jsonrpc_error_reply_surfaces_as_an_err() {
        let mut io = FakeIo::new(&[
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found"}}"#,
        ]);
        let mut id = 1;
        assert!(list_tools(&mut io, &mut id).is_err());
    }

    // The real spawn/reader-thread/handshake plumbing is thin and standard; it is
    // exercised end-to-end against an actual MCP server when the node connects one
    // (slice 2b-ii). A shell-based subprocess mock proved too sensitive to the CI
    // shell's stdout buffering (bash vs dash) to be a reliable unit test, so the
    // protocol is covered hermetically above via the LineIo seam instead.
}

#[cfg(test)]
mod assist_tests {
    use super::text_from_call_result;
    use serde_json::json;

    /// Wraps a payload the way an MCP server returns it: a `content` array of text
    /// parts, which is how HA's Assist envelope actually arrives.
    fn mcp_text(payload: &str) -> serde_json::Value {
        json!({ "content": [ { "type": "text", "text": payload } ] })
    }

    /// The exact response captured from the live deployment, where the butler had
    /// just turned the kitchen lights off and then told the person it had "found
    /// some items" and asked whether to perform an action.
    const LIVE_ACTION_DONE: &str = r#"{"speech": {}, "response_type": "action_done", "data": {"success": [{"name": "Kitchen", "type": "area", "id": "kitchen"}, {"name": "Kitchen Table", "type": "entity", "id": "light.kitchen_table"}, {"name": "Kitchen", "type": "entity", "id": "light.kitchen"}], "failed": []}}"#;

    #[test]
    fn a_completed_action_reads_as_completed_not_as_a_search_result() {
        let out = text_from_call_result(&mcp_text(LIVE_ACTION_DONE), "HassLightSet").unwrap();
        assert!(
            out.contains("completed successfully"),
            "should read as a completed action, got: {out}"
        );
        assert!(out.contains("Kitchen (area)"), "names the area: {out}");
        assert!(
            out.contains("Kitchen Table (light)"),
            "names the entity AND what it is, so the model cannot guess: {out}"
        );
        assert!(
            !out.to_lowercase().contains("media"),
            "nothing here should suggest a media player: {out}"
        );
        // The raw envelope must not survive — it is what the model misread.
        assert!(!out.contains("response_type"), "raw envelope leaked: {out}");
        assert!(!out.contains("\"success\""), "raw envelope leaked: {out}");
    }

    #[test]
    fn home_assistants_own_sentence_wins_when_it_wrote_one() {
        let payload = json!({
            "speech": { "plain": { "speech": "Turned off the kitchen lights" } },
            "response_type": "action_done",
            "data": { "success": [ { "name": "Kitchen", "type": "area" } ], "failed": [] }
        })
        .to_string();
        let out = text_from_call_result(&mcp_text(&payload), "HassTurnOff").unwrap();
        assert!(out.contains("Turned off the kitchen lights"), "got: {out}");
    }

    #[test]
    fn a_partial_failure_is_an_error_not_a_success_story() {
        // If some targets failed, the turn's failure path must engage rather than
        // the model deciding how bad it was.
        let payload = json!({
            "speech": {},
            "response_type": "action_done",
            "data": {
                "success": [ { "name": "Kitchen Table", "type": "entity" } ],
                "failed":  [ { "name": "Hallway", "type": "entity" } ]
            }
        })
        .to_string();
        let err = text_from_call_result(&mcp_text(&payload), "HassTurnOff").unwrap_err();
        assert!(err.contains("Hallway"), "names what failed: {err}");
        assert!(err.contains("could not act"), "got: {err}");
    }

    #[test]
    fn an_assist_error_response_becomes_an_error() {
        let payload = json!({
            "speech": { "plain": { "speech": "Sorry, I am not aware of any device called that" } },
            "response_type": "error",
            "data": { "code": "no_intent_match" }
        })
        .to_string();
        let err = text_from_call_result(&mcp_text(&payload), "HassTurnOn").unwrap_err();
        assert!(err.contains("not aware of any device"), "got: {err}");
    }

    #[test]
    fn a_query_answer_is_relayed() {
        let payload = json!({
            "speech": { "plain": { "speech": "The kitchen light is on" } },
            "response_type": "query_answer",
            "data": { "success": [], "failed": [] }
        })
        .to_string();
        let out = text_from_call_result(&mcp_text(&payload), "HassGetState").unwrap();
        assert_eq!(out, "The kitchen light is on");
    }

    #[test]
    fn other_mcp_servers_pass_through_untouched() {
        // Only Assist envelopes are recognised; anything else keeps its own text.
        let out = text_from_call_result(&mcp_text("plain tool output"), "search").unwrap();
        assert_eq!(out, "plain tool output");
        let json_out = text_from_call_result(&mcp_text(r#"{"rows":[1,2,3]}"#), "query").unwrap();
        assert_eq!(json_out, r#"{"rows":[1,2,3]}"#);
    }
}
