//! The node's client for Endora HQ (ADR 0102): HQ answers the node only, and the node signs
//! every request with the token the two share. The owner never talks to HQ directly.

use std::time::Duration;

use serde_json::Value;

/// How long HQ has to answer. It is on the same host, so anything slower is a fault.
const ANSWERS_WITHIN: Duration = Duration::from_secs(10);

/// Why HQ did not do what the node asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HqError {
    /// HQ could not be reached.
    Unreachable(String),
    /// HQ answered, and said no.
    Answered {
        /// The HTTP status it answered with.
        status: u16,
        /// What it said.
        message: String,
    },
}

impl core::fmt::Display for HqError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "HQ is not answering ({why})"),
            Self::Answered { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for HqError {}

/// The node's client for HQ.
pub struct HqClient {
    url: String,
    token: String,
    agent: ureq::Agent,
}

impl HqClient {
    /// A client for HQ at `url`, signing with `token`.
    #[must_use]
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(ANSWERS_WITHIN))
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }

    /// GETs a path from HQ.
    ///
    /// # Errors
    /// [`HqError`] if HQ cannot be reached or says no.
    pub fn get(&self, path: &str) -> Result<Value, HqError> {
        let response = self
            .agent
            .get(&format!("{}{path}", self.url))
            .header("authorization", &format!("Bearer {}", self.token))
            .call();
        Self::read(response)
    }

    /// POSTs a JSON body to a path on HQ.
    ///
    /// # Errors
    /// [`HqError`] if HQ cannot be reached or says no.
    pub fn post(&self, path: &str, body: &Value) -> Result<Value, HqError> {
        let response = self
            .agent
            .post(&format!("{}{path}", self.url))
            .header("authorization", &format!("Bearer {}", self.token))
            .send_json(body);
        Self::read(response)
    }

    fn read(
        response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Value, HqError> {
        let mut response = response.map_err(|e| HqError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| HqError::Unreachable(e.to_string()))?;
        let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(body);
        }
        let message = body["error"]
            .as_str()
            .map_or_else(|| format!("HQ answered {status}"), str::to_owned);
        Err(HqError::Answered { status, message })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};

    /// Answers one request with `status` and `body`, and hands back what it was sent.
    fn one_answer(
        status: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).unwrap();
            request
        });
        (url, seen)
    }

    #[test]
    fn every_request_is_signed_and_an_answer_is_read() {
        let (url, seen) = one_answer("200 OK", r#"{"stopped":false}"#);
        let hq = HqClient::new(&url, "a-token-of-at-least-thirty-two-characters");
        assert_eq!(hq.get("/v1/snapshot").unwrap()["stopped"], false);
        let request = seen.join().unwrap();
        assert!(request.starts_with("GET /v1/snapshot "));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer a-token-of-at-least-thirty-two-characters")
        );
    }

    #[test]
    fn a_refusal_carries_what_hq_said_and_its_status() {
        let (url, _seen) = one_answer("409 Conflict", r#"{"error":"this task is over"}"#);
        let hq = HqClient::new(&url, "t");
        assert_eq!(
            hq.post("/v1/desk/1", &serde_json::json!({ "decision": "approve" })),
            Err(HqError::Answered {
                status: 409,
                message: "this task is over".to_owned()
            })
        );
    }

    #[test]
    fn hq_that_is_not_there_is_unreachable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert!(matches!(
            HqClient::new(&url, "t").get("/v1/snapshot"),
            Err(HqError::Unreachable(_))
        ));
    }
}
