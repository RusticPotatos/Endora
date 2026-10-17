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

/// The caller's headers [`HqClient::pass`] passes on, and no others.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Passing {
    /// The caller's own `Authorization`, untouched.
    pub authorization: Option<String>,
    /// Its `Content-Type`.
    pub content_type: Option<String>,
    /// Its `Accept`.
    pub accept: Option<String>,
}

/// HQ's answer to a request passed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passed {
    /// The status HQ answered with.
    pub status: u16,
    /// Its content type, if it said one.
    pub content_type: Option<String>,
    /// The whole body.
    pub body: Vec<u8>,
}

/// Where the client finds the token it signs with.
enum Token {
    /// Given once.
    Fixed(String),
    /// Read from a file HQ writes, each time: HQ may start after the node.
    File(std::path::PathBuf),
}

/// The node's client for HQ.
pub struct HqClient {
    url: String,
    token: Token,
    agent: ureq::Agent,
}

impl HqClient {
    /// A client for HQ at `url`, signing with `token`.
    #[must_use]
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_owned(),
            token: Token::Fixed(token.to_owned()),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(ANSWERS_WITHIN))
                .http_status_as_error(false)
                // HQ never redirects, and the team gateway passes callers to HQ and nowhere
                // else: a redirect is an answer, passed back.
                .max_redirects(0)
                .build()
                .into(),
        }
    }

    /// A client for HQ at `url`, signing with the token HQ keeps in `file`.
    #[must_use]
    pub fn with_token_file(url: &str, file: &std::path::Path) -> Self {
        let mut client = Self::new(url, "");
        client.token = Token::File(file.to_path_buf());
        client
    }

    fn token(&self) -> Result<String, HqError> {
        match &self.token {
            Token::Fixed(token) => Ok(token.clone()),
            Token::File(file) => std::fs::read_to_string(file)
                .map(|t| t.trim().to_owned())
                .map_err(|_| HqError::Unreachable("HQ has not made its token yet".to_owned())),
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
            .header("authorization", &format!("Bearer {}", self.token()?))
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
            .header("authorization", &format!("Bearer {}", self.token()?))
            .send_json(body);
        Self::read(response)
    }

    /// Passes a caller's request on to HQ as it came (ADR 0110, the team gateway): a GET, or a
    /// POST with its body, to `path` on HQ, signed with the caller's own `authorization` (a
    /// turn's or the runner's token) and never the node's. HQ's answer comes back whole, its
    /// status and content type with it, unless it is longer than `most` bytes.
    ///
    /// # Errors
    /// [`HqError::Unreachable`] if HQ cannot be reached or its answer is too long; a method
    /// other than GET or POST is [`HqError::Answered`] 405 without asking HQ.
    pub fn pass(
        &self,
        method: &str,
        path: &str,
        headers: &Passing,
        body: &[u8],
        most: u64,
    ) -> Result<Passed, HqError> {
        let url = format!("{}{path}", self.url);
        let mut request = match method {
            "GET" => self.agent.get(&url).force_send_body(),
            "POST" => self.agent.post(&url),
            _ => {
                return Err(HqError::Answered {
                    status: 405,
                    message: "the team gateway passes on GET and POST".to_owned(),
                });
            }
        };
        for (name, value) in [
            ("authorization", &headers.authorization),
            ("content-type", &headers.content_type),
            ("accept", &headers.accept),
        ] {
            if let Some(value) = value {
                request = request.header(name, value);
            }
        }
        let mut response = request
            .send(body)
            .map_err(|e| HqError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = response
            .body_mut()
            .with_config()
            .limit(most)
            .read_to_vec()
            .map_err(|e| HqError::Unreachable(e.to_string()))?;
        Ok(Passed {
            status,
            content_type,
            body,
        })
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
    fn the_token_is_read_from_hqs_file_each_time() {
        let (url, seen) = one_answer("200 OK", "{}");
        let dir = std::env::temp_dir().join(format!("endora-hq-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token");
        let _ = std::fs::remove_file(&file);
        let hq = HqClient::with_token_file(&url, &file);
        assert!(
            matches!(hq.get("/v1/snapshot"), Err(HqError::Unreachable(_))),
            "not made yet"
        );
        std::fs::write(&file, "made-by-hq-made-by-hq-made-by-hq-0123\n").unwrap();
        hq.get("/v1/snapshot").unwrap();
        assert!(
            seen.join()
                .unwrap()
                .to_lowercase()
                .contains("authorization: bearer made-by-hq-made-by-hq-made-by-hq-0123")
        );
        let _ = std::fs::remove_dir_all(&dir);
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
