//! A **synchronous** streamable-HTTP MCP client (ADR 0021).
//!
//! Speaks MCP over HTTP to a **networked** server — a sidecar container, or a Docker
//! MCP Gateway that aggregates several servers behind one endpoint. This is the
//! "connect to a server we don't host" path (the model-agnostic boundary), which
//! keeps Endora's own image lean instead of spawning subprocesses. Kept sync (`ureq`
//! is already a dependency) to match [`McpClient`]/[`CapabilityRunner`].
//!
//! Each request is one POST of a JSON-RPC message. Per the streamable-HTTP transport
//! the server replies with either a single JSON response or an SSE stream, and may
//! hand back an `Mcp-Session-Id` on `initialize` that we echo on later requests. The
//! JSON-RPC result shape is parsed by the same helpers the stdio client uses.

use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::infrastructure::{McpClient, McpToolInfo};
use crate::mcp_stdio::{text_from_call_result, tools_from_result};

/// The MCP protocol revision this client speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";
/// How long to wait for a reply (a networked server; a gateway may proxy to a slow
/// upstream, but a turn must not hang indefinitely — ADR 0021 health).
const TIMEOUT: Duration = Duration::from_secs(30);
/// Cap on a single response body.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// A connected HTTP MCP server (streamable-HTTP transport, ADR 0021).
pub struct HttpMcpClient {
    agent: ureq::Agent,
    url: String,
    /// Optional bearer token sent as `Authorization` (e.g. a Home Assistant
    /// long-lived token). A secret: held here, never logged or returned.
    auth: String,
    /// The session id the server assigned on `initialize`, echoed on later requests.
    session: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl HttpMcpClient {
    /// Connects to the MCP endpoint at `url` and completes the handshake.
    ///
    /// # Errors
    /// A human-readable message if the server can't be reached or replies badly —
    /// the same "unhealthy server ⇒ skipped" signal the adapter relies on.
    pub fn connect(url: &str) -> Result<Self, String> {
        Self::connect_with_auth(url, "")
    }

    /// Connects with a bearer token sent as `Authorization: Bearer …` (empty = none).
    ///
    /// # Errors
    /// A human-readable message if the server can't be reached or replies badly.
    pub fn connect_with_auth(url: &str, auth: &str) -> Result<Self, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        let client = Self {
            agent,
            url: url.to_owned(),
            auth: auth.trim().to_owned(),
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
        };
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "endora", "version": env!("CARGO_PKG_VERSION") },
        });
        client.request("initialize", Some(params))?;
        client.notify("notifications/initialized");
        Ok(client)
    }

    /// The current session id, if the server assigned one.
    fn session_id(&self) -> Option<String> {
        self.session.lock().ok().and_then(|g| g.clone())
    }

    /// POSTs a JSON-RPC request and returns its `result` value (or an error).
    fn request(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut body = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(p) = params {
            body["params"] = p;
        }
        let mut builder = self
            .agent
            .post(&self.url)
            .header("accept", "application/json, text/event-stream");
        if !self.auth.is_empty() {
            builder = builder.header("authorization", &format!("Bearer {}", self.auth));
        }
        if let Some(sid) = self.session_id() {
            builder = builder.header("mcp-session-id", &sid);
        }
        let mut resp = builder
            .send_json(&body)
            .map_err(|e| format!("MCP HTTP request failed: {e}"))?;
        // Capture a session id the server assigned (on initialize).
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
        {
            if let Ok(mut g) = self.session.lock() {
                *g = Some(sid);
            }
        }
        let is_sse = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|c| c.contains("text/event-stream"));
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(MAX_BYTES)
            .read_to_end(&mut buf)
            .map_err(|e| format!("MCP HTTP read failed: {e}"))?;
        let text = String::from_utf8_lossy(&buf);
        let msg = if is_sse {
            sse_message_for_id(&text, id)?
        } else {
            serde_json::from_str::<Value>(text.trim())
                .map_err(|e| format!("bad JSON from MCP server: {e}"))?
        };
        if let Some(err) = msg.get("error") {
            return Err(format!("MCP server error: {err}"));
        }
        Ok(msg.get("result").cloned().unwrap_or(Value::Null))
    }

    /// POSTs a JSON-RPC notification (no id, no reply expected). Best-effort.
    fn notify(&self, method: &str) {
        let mut builder = self
            .agent
            .post(&self.url)
            .header("accept", "application/json, text/event-stream");
        if !self.auth.is_empty() {
            builder = builder.header("authorization", &format!("Bearer {}", self.auth));
        }
        if let Some(sid) = self.session_id() {
            builder = builder.header("mcp-session-id", &sid);
        }
        let _ = builder.send_json(json!({ "jsonrpc": "2.0", "method": method }));
    }
}

/// Scans an SSE body's `data:` lines for the JSON-RPC message whose `id` matches.
fn sse_message_for_id(text: &str, id: u64) -> Result<Value, String> {
    for line in text.lines() {
        let Some(payload) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() {
            continue;
        }
        if let Ok(msg) = serde_json::from_str::<Value>(payload) {
            if msg.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(msg);
            }
        }
    }
    Err("no matching reply in the MCP server's event stream".to_owned())
}

impl McpClient for HttpMcpClient {
    fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
        Ok(tools_from_result(&self.request("tools/list", None)?))
    }

    fn call(&self, tool: &str, input_json: &str) -> Result<String, String> {
        let args: Value = serde_json::from_str(input_json.trim()).unwrap_or_else(|_| json!({}));
        let result = self.request(
            "tools/call",
            Some(json!({ "name": tool, "arguments": args })),
        )?;
        text_from_call_result(&result, tool)
    }
}

#[cfg(test)]
mod tests {
    use super::sse_message_for_id;

    #[test]
    fn sse_picks_the_reply_with_the_matching_id() {
        // A stream carrying a stray event then the real reply.
        let body = "event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/x\"}\n\
                    \n\
                    event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        let msg = sse_message_for_id(body, 7).unwrap();
        assert_eq!(msg["result"]["ok"], true);
        // No matching id ⇒ an error rather than a wrong message.
        assert!(sse_message_for_id(body, 99).is_err());
    }
}
