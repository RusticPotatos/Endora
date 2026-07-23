//! A **synchronous** stdio MCP client (ADR 0021).
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

use crate::infrastructure::{McpClient, McpToolInfo};

/// The MCP protocol revision this client speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";
/// How long to wait for any single server reply before giving up (ADR 0021 health:
/// a hung server fails this call rather than hanging the turn).
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// A line-oriented duplex to an MCP server: send one request line, receive reply
/// lines. Abstracted so the session logic is testable without a real process.
trait LineIo {
    /// Sends one request line (the newline is added).
    fn send(&mut self, line: &str) -> Result<(), String>;
    /// Receives the next line the server emitted (blocks, with a timeout).
    fn recv(&mut self) -> Result<String, String>;
}

/// Sends a JSON-RPC request with `id` and reads reply lines until the response
/// carrying that same `id` arrives — skipping notifications and any other-id
/// messages the server interleaves. Returns the `result` value (or an error if the
/// server replied with one).
fn request(
    io: &mut dyn LineIo,
    id: u64,
    method: &str,
    params: Option<Value>,
) -> Result<Value, String> {
    let mut req = json!({ "jsonrpc": "2.0", "id": id, "method": method });
    if let Some(p) = params {
        req["params"] = p;
    }
    io.send(&req.to_string())?;
    loop {
        let line = io.recv()?;
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
    request(io, id, "initialize", Some(params))?;
    notify(io, "notifications/initialized")
}

/// Lists the server's tools (`tools/list`).
fn list_tools(io: &mut dyn LineIo, next_id: &mut u64) -> Result<Vec<McpToolInfo>, String> {
    let id = *next_id;
    *next_id += 1;
    let result = request(io, id, "tools/list", None)?;
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_owned();
            let description = t
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            Some(McpToolInfo { name, description })
        })
        .collect())
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
    let result = request(io, id, "tools/call", Some(params))?;
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
    Ok(if text.is_empty() {
        result.to_string()
    } else {
        text
    })
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

    fn recv(&mut self) -> Result<String, String> {
        match self.rx.recv_timeout(CALL_TIMEOUT) {
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

/// A connected local MCP server, spoken to over stdio (ADR 0021). Constructing one
/// spawns the subprocess and completes the handshake; a shared [`Mutex`] serializes
/// the request/response cycle so it is `Send + Sync` behind [`McpClient`].
pub struct StdioMcpClient {
    io: Mutex<StdioIo>,
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
        let mut child = Command::new(command)
            .args(args)
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
        Ok(Self { io: Mutex::new(io) })
    }
}

impl McpClient for StdioMcpClient {
    fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
        let mut io = self
            .io
            .lock()
            .map_err(|_| "MCP client poisoned".to_owned())?;
        let mut id = io.next_id;
        let out = list_tools(&mut *io, &mut id);
        io.next_id = id;
        out
    }

    fn call(&self, tool: &str, input_json: &str) -> Result<String, String> {
        let mut io = self
            .io
            .lock()
            .map_err(|_| "MCP client poisoned".to_owned())?;
        let mut id = io.next_id;
        let out = call_tool(&mut *io, &mut id, tool, input_json);
        io.next_id = id;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{LineIo, call_tool, handshake, list_tools};
    use std::collections::VecDeque;

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
        fn recv(&mut self) -> Result<String, String> {
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
