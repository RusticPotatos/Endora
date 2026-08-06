//! A **synchronous** HTTP MCP client (ADR 0054) that speaks either transport.
//!
//! Connects to a **networked** server — a sidecar container, a Docker MCP Gateway, or
//! Home Assistant's MCP integration — the "connect to a server we don't host" path,
//! keeping Endora's own image lean. Kept sync (`ureq`, already a dependency) to match
//! [`McpClient`]/[`CapabilityRunner`].
//!
//! It **auto-detects** the transport on connect:
//! - **Streamable HTTP** (newer): each request is one POST; the reply is a single JSON
//!   body or an inline SSE body, and an `Mcp-Session-Id` from `initialize` is echoed on
//!   later requests.
//! - **HTTP+SSE** (older; what Home Assistant's `/mcp_server/sse` uses): a long-lived
//!   GET stream announces a POST endpoint via an `endpoint` event; requests are POSTed
//!   there and every reply arrives back on the stream.
//!
//! It tries the SSE stream first (a single GET); if that isn't an event stream it falls
//! back to POSTing. The JSON-RPC result shape is parsed by the same helpers the stdio
//! client uses.

use std::io::{BufRead, BufReader, Read};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::infrastructure::{McpClient, McpResource, McpToolInfo};
use crate::mcp_stdio::{text_from_call_result, tools_from_result};

/// The MCP protocol revision this client speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";
/// How long to wait for a reply (a networked server; a gateway may proxy to a slow
/// upstream, but a turn must not hang indefinitely — ADR 0054 health).
const TIMEOUT: Duration = Duration::from_secs(30);
/// Cap on a single response body.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Whether the streamable server has forgotten our session (MCP spec: unknown session
/// id ⇒ `404`, and the client starts a new one). Narrow for the same reason its SSE
/// sibling is: a 500 or a refusal is a server having a bad day, not a lost session.
fn forgot_the_session(why: &str) -> bool {
    let why = why.to_lowercase();
    why.contains("mcp http request failed") && (why.contains("404") || why.contains("session"))
}

/// Whether an error means the session is gone rather than the server being unwell.
///
/// Shape, not status alone: the post URL a session issued stops existing when the
/// server restarts, which is what installing a device in Home Assistant does. A `404`
/// on the session endpoint is that, and so is a server explicitly saying the session
/// is unknown. Anything else — a timeout, a 500, a refused connection — is a server
/// having a bad day, and reopening would be a reconnect storm rather than a fix.
fn session_is_gone(why: &str) -> bool {
    let why = why.to_lowercase();
    if !why.contains("mcp sse post failed") {
        return false;
    }
    why.contains("404") || why.contains("session")
}

/// A live HTTP+SSE connection: the POST endpoint the server announced, and the
/// receiver for messages the reader thread pulls off the event stream.
struct SseConn {
    post_url: String,
    rx: Mutex<Receiver<(String, String)>>,
}

/// A connected HTTP MCP server (ADR 0054). `sse` present ⇒ HTTP+SSE transport;
/// absent ⇒ streamable HTTP.
pub struct HttpMcpClient {
    agent: ureq::Agent,
    url: String,
    /// Optional bearer token sent as `Authorization` (e.g. a Home Assistant
    /// long-lived token). A secret: held here, never logged or returned.
    auth: String,
    /// The session id the server assigned on `initialize` (streamable transport).
    session: Mutex<Option<String>>,
    next_id: AtomicU64,
    /// The live SSE connection, replaceable: a session dies whenever the server
    /// restarts, and the fix is to open a new one rather than to stay broken
    /// (ADR 0073).
    sse: Mutex<Option<SseConn>>,
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
        let auth = auth.trim().to_owned();
        // Prefer the SSE transport if the endpoint offers one; otherwise POST.
        let sse = open_sse(url, &auth);
        let client = Self {
            agent,
            url: url.to_owned(),
            auth,
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
            sse: Mutex::new(sse),
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

    /// The current session id, if the server assigned one (streamable transport).
    fn session_id(&self) -> Option<String> {
        self.session.lock().ok().and_then(|g| g.clone())
    }

    /// Adds the auth header (and session id, streamable only) to a request builder.
    fn with_headers(
        &self,
        mut b: ureq::RequestBuilder<ureq::typestate::WithBody>,
    ) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
        if !self.auth.is_empty() {
            b = b.header("authorization", &format!("Bearer {}", self.auth));
        }
        if !self.on_sse() {
            if let Some(sid) = self.session_id() {
                b = b.header("mcp-session-id", &sid);
            }
        }
        b
    }

    /// Sends a JSON-RPC request and returns its `result` value (or an error).
    fn request(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut body = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(p) = params {
            body["params"] = p;
        }
        if self.on_sse() {
            return self.request_sse_healing(id, &body);
        }
        self.request_streamable_healing(id, &body)
    }

    /// One streamable-HTTP request, starting a new session once if the server has
    /// forgotten the old one (ADR 0073).
    ///
    /// The MCP specification is explicit: a server that no longer knows a session id
    /// answers `404`, and the client is to begin a new session rather than keep
    /// presenting a dead one. Nothing on this install speaks this transport today —
    /// Home Assistant is SSE and the search server is stdio — so this is the same
    /// class fixed before it is met, on the strength of the spec rather than a
    /// screenshot.
    fn request_streamable_healing(&self, id: u64, body: &Value) -> Result<Value, String> {
        match self.request_streamable(id, body) {
            Err(why) if forgot_the_session(&why) => {
                // Drop the stale id first: the handshake must go out without it, or the
                // server is being asked to honour the very session it just disowned.
                if let Ok(mut g) = self.session.lock() {
                    *g = None;
                }
                let init_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                self.request_streamable(
                    init_id,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": init_id,
                        "method": "initialize",
                        "params": {
                            "protocolVersion": PROTOCOL_VERSION,
                            "capabilities": {},
                            "clientInfo": {
                                "name": "endora",
                                "version": env!("CARGO_PKG_VERSION"),
                            },
                        },
                    }),
                )?;
                self.notify("notifications/initialized");
                let retry_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                let mut retry = body.clone();
                retry["id"] = json!(retry_id);
                self.request_streamable(retry_id, &retry)
            }
            other => other,
        }
    }

    /// Whether this client speaks the HTTP+SSE transport.
    fn on_sse(&self) -> bool {
        self.sse.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// One SSE request, reopening the session once if the old one is gone (ADR 0073).
    ///
    /// A session belongs to the server that issued it, so a Home Assistant restart —
    /// which is what installing a device does — invalidates it. Every later call then
    /// posts to a URL that no longer exists and fails `404`, and nothing upstream
    /// notices: the tool list is cached from the last good connect, so the server keeps
    /// looking healthy while every action against the house fails. Observed live, and
    /// it persisted until the node was restarted.
    ///
    /// Healing here rather than upstream because this is the only layer that knows the
    /// session died. One retry, never a loop: if the fresh session fails too, the
    /// server is genuinely down and saying so is the honest answer.
    fn request_sse_healing(&self, id: u64, body: &Value) -> Result<Value, String> {
        let first = {
            let guard = self.sse.lock().map_err(|_| "MCP SSE lock poisoned")?;
            let Some(sse) = guard.as_ref() else {
                return Err("MCP SSE connection is gone".to_owned());
            };
            self.request_sse(sse, id, body)
        };
        match first {
            Err(why) if session_is_gone(&why) => {
                self.reopen()?;
                let guard = self.sse.lock().map_err(|_| "MCP SSE lock poisoned")?;
                let Some(sse) = guard.as_ref() else {
                    return Err("MCP SSE connection is gone".to_owned());
                };
                // A new session starts unhandshaken, so the id space restarts with it.
                let retry_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                let mut retry = body.clone();
                retry["id"] = json!(retry_id);
                self.request_sse(sse, retry_id, &retry)
            }
            other => other,
        }
    }

    /// Opens a fresh SSE session and re-runs the handshake on it.
    fn reopen(&self) -> Result<(), String> {
        let fresh = open_sse(&self.url, &self.auth)
            .ok_or("MCP SSE reconnect failed: the server did not offer a session")?;
        {
            let mut guard = self.sse.lock().map_err(|_| "MCP SSE lock poisoned")?;
            *guard = Some(fresh);
        }
        // A session that has not been initialized answers nothing useful, so the
        // handshake is part of reopening rather than a separate courtesy.
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "endora", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        {
            let guard = self.sse.lock().map_err(|_| "MCP SSE lock poisoned")?;
            let Some(sse) = guard.as_ref() else {
                return Err("MCP SSE connection is gone".to_owned());
            };
            self.request_sse(sse, id, &body)?;
        }
        self.notify("notifications/initialized");
        Ok(())
    }

    /// Streamable HTTP: POST the message, read the reply inline.
    fn request_streamable(&self, id: u64, body: &Value) -> Result<Value, String> {
        let builder = self
            .agent
            .post(&self.url)
            .header("accept", "application/json, text/event-stream");
        let mut resp = self
            .with_headers(builder)
            .send_json(body)
            .map_err(|e| format!("MCP HTTP request failed: {e}"))?;
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
        result_of(&msg)
    }

    /// HTTP+SSE: POST the message to the announced endpoint, then read the matching
    /// reply off the event stream.
    fn request_sse(&self, sse: &SseConn, id: u64, body: &Value) -> Result<Value, String> {
        let builder = self
            .agent
            .post(&sse.post_url)
            .header("content-type", "application/json");
        self.with_headers(builder)
            .send_json(body)
            .map_err(|e| format!("MCP SSE post failed: {e}"))?;
        let rx = sse
            .rx
            .lock()
            .map_err(|_| "MCP SSE channel poisoned".to_owned())?;
        loop {
            match rx.recv_timeout(TIMEOUT) {
                Ok((_event, data)) => {
                    if let Ok(msg) = serde_json::from_str::<Value>(&data) {
                        if msg.get("id").and_then(Value::as_u64) == Some(id) {
                            return result_of(&msg);
                        }
                    }
                }
                // Split, because the two mean opposite things (ADR 0073). A timeout is
                // a server thinking too long — reopening would be a storm. A closed
                // channel is the reader thread having ended, which happens when the
                // event stream itself died: that IS the session going away, and the
                // post is not always what notices first.
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err("the MCP server timed out".to_owned());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("MCP SSE post failed: the session's event stream closed".to_owned());
                }
            }
        }
    }

    /// Sends a JSON-RPC notification (no id, no reply expected). Best-effort.
    fn notify(&self, method: &str) {
        let msg = json!({ "jsonrpc": "2.0", "method": method });
        let post_url = self
            .sse
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.post_url.clone()))
            .unwrap_or_else(|| self.url.clone());
        let builder = self
            .agent
            .post(&post_url)
            .header("accept", "application/json, text/event-stream");
        let _ = self.with_headers(builder).send_json(msg);
    }
}

/// Pulls the `result` out of a JSON-RPC reply, or an error if it carried one.
fn result_of(msg: &Value) -> Result<Value, String> {
    if let Some(err) = msg.get("error") {
        return Err(format!("MCP server error: {err}"));
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

/// Opens the HTTP+SSE stream at `url`: GETs the event stream, drains it on a reader
/// thread, and waits for the `endpoint` event that names where to POST messages.
/// Returns `None` when the endpoint isn't an event stream (so the caller uses the
/// streamable POST transport instead) or the handshake stalls.
fn open_sse(url: &str, auth: &str) -> Option<SseConn> {
    // A dedicated agent with NO global timeout — the GET is a long-lived stream.
    let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
    let mut builder = agent.get(url).header("accept", "text/event-stream");
    if !auth.is_empty() {
        builder = builder.header("authorization", &format!("Bearer {auth}"));
    }
    let resp = builder.call().ok()?;
    let is_stream = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.contains("text/event-stream"));
    if !is_stream {
        return None;
    }
    let (tx, rx) = mpsc::channel::<(String, String)>();
    thread::spawn(move || {
        let mut resp = resp;
        let reader = BufReader::new(resp.body_mut().as_reader());
        let mut event = String::new();
        let mut data = String::new();
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.is_empty() {
                // Blank line ends an event; dispatch what we have.
                if !data.is_empty() {
                    let ev = if event.is_empty() {
                        "message".to_owned()
                    } else {
                        std::mem::take(&mut event)
                    };
                    if tx.send((ev, std::mem::take(&mut data))).is_err() {
                        break; // consumer gone
                    }
                }
                event.clear();
                data.clear();
            } else if let Some(v) = line.strip_prefix("event:") {
                event = v.trim().to_owned();
            } else if let Some(v) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(v.strip_prefix(' ').unwrap_or(v));
            }
        }
    });
    // The first meaningful event names the POST endpoint.
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok((event, data)) => {
                if event == "endpoint" {
                    return Some(SseConn {
                        post_url: resolve_url(url, data.trim()),
                        rx: Mutex::new(rx),
                    });
                }
                // Ignore anything before the endpoint (comments/keep-alives).
            }
            Err(_) => return None,
        }
    }
}

/// Resolves an endpoint value against the SSE URL: absolute URLs pass through; a
/// path is joined to the stream's scheme+host.
fn resolve_url(base: &str, endpoint: &str) -> String {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return endpoint.to_owned();
    }
    let origin = base.find("://").map_or(base, |i| {
        let after = &base[i + 3..];
        after.find('/').map_or(base, |j| &base[..i + 3 + j])
    });
    if endpoint.starts_with('/') {
        format!("{origin}{endpoint}")
    } else {
        format!("{origin}/{endpoint}")
    }
}

/// Scans an inline SSE body's `data:` lines for the message whose `id` matches.
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

    fn list_resources(&self) -> Result<Vec<McpResource>, String> {
        // A server that does not implement the method is the ordinary case, not a fault —
        // see the port's own note. Same shape as the stdio transport.
        Ok(self
            .request("resources/list", None)
            .map(|r| crate::mcp_stdio::resources_from_result(&r))
            .unwrap_or_default())
    }

    fn read_resource(&self, uri: &str) -> Result<String, String> {
        let result = self.request("resources/read", Some(json!({ "uri": uri })))?;
        Ok(crate::mcp_stdio::contents_from_result(&result))
    }
}

#[cfg(test)]
mod session_death {
    //! ADR 0073. A Home Assistant restart — which is what installing a device does —
    //! invalidates the SSE session, and every later action failed `404` until the node
    //! itself was restarted.

    #[test]
    fn a_dead_session_is_told_from_an_unwell_server() {
        use super::session_is_gone;
        // The observed failure, verbatim from the person's screen.
        assert!(session_is_gone("MCP SSE post failed: http status: 404"));
        assert!(session_is_gone(
            "MCP SSE post failed: unknown session id 7f3a"
        ));
        // A server having a bad day is not a dead session: reopening on these would be
        // a reconnect storm against something already struggling.
        for unwell in [
            "MCP SSE post failed: http status: 500",
            "MCP SSE post failed: connection refused",
            "the MCP server timed out",
            "MCP HTTP request failed: http status: 404",
        ] {
            assert!(!session_is_gone(unwell), "would have stormed on: {unwell}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_url, sse_message_for_id};

    #[test]
    fn sse_picks_the_reply_with_the_matching_id() {
        let body = "event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/x\"}\n\
                    \n\
                    event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        let msg = sse_message_for_id(body, 7).unwrap();
        assert_eq!(msg["result"]["ok"], true);
        assert!(sse_message_for_id(body, 99).is_err());
    }

    #[test]
    fn endpoint_urls_resolve_against_the_stream_origin() {
        // The common Home-Assistant-style case: an absolute path with a session token.
        assert_eq!(
            resolve_url(
                "http://ha.local:8123/mcp_server/sse",
                "/mcp_server/messages/abc"
            ),
            "http://ha.local:8123/mcp_server/messages/abc"
        );
        // A relative value gets a slash; an absolute URL passes through untouched.
        assert_eq!(
            resolve_url("https://gw:9/sse", "messages?s=1"),
            "https://gw:9/messages?s=1"
        );
        assert_eq!(
            resolve_url("https://gw:9/sse", "https://other/post"),
            "https://other/post"
        );
    }
}
