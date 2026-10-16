//! The node's client for endora-sound (ADR 0104): the hub answers whoever holds its token, and
//! the node is its remote. The owner never talks to the hub directly; away from home they
//! reach it only through the node's sign-in.

use std::time::Duration;

use serde_json::Value;

/// How long the hub has to answer. It is on the same host, so anything slower is a fault.
const ANSWERS_WITHIN: Duration = Duration::from_secs(10);

/// How long the hub has to start a stream. The stream itself then runs as long as it is heard.
const STARTS_WITHIN: Duration = Duration::from_secs(10);

/// Why the hub did not do what the node asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundError {
    /// The hub could not be reached.
    Unreachable(String),
    /// The hub answered, and said no.
    Answered {
        /// The HTTP status it answered with.
        status: u16,
        /// What it said.
        message: String,
    },
}

impl core::fmt::Display for SoundError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "the sound hub is not answering ({why})"),
            Self::Answered { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for SoundError {}

/// Where the client finds the token it signs with.
enum Token {
    /// Given once.
    Fixed(String),
    /// Read from a file the hub writes, each time: the hub may start after the node.
    File(std::path::PathBuf),
}

/// The HTTP methods the node uses on the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Read.
    Get,
    /// Add.
    Post,
    /// Set.
    Put,
    /// Take away.
    Delete,
}

/// The node's client for the sound hub.
pub struct SoundClient {
    url: String,
    token: Token,
    agent: ureq::Agent,
    streaming: ureq::Agent,
}

impl SoundClient {
    /// A client for the hub at `url`, signing with `token`.
    #[must_use]
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_owned(),
            token: Token::Fixed(token.to_owned()),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(ANSWERS_WITHIN))
                .http_status_as_error(false)
                .build()
                .into(),
            streaming: ureq::Agent::config_builder()
                .timeout_connect(Some(STARTS_WITHIN))
                .timeout_recv_response(Some(STARTS_WITHIN))
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }

    /// A client for the hub at `url`, signing with the token the hub keeps in `file`.
    #[must_use]
    pub fn with_token_file(url: &str, file: &std::path::Path) -> Self {
        let mut client = Self::new(url, "");
        client.token = Token::File(file.to_path_buf());
        client
    }

    fn token(&self) -> Result<String, SoundError> {
        match &self.token {
            Token::Fixed(token) => Ok(token.clone()),
            Token::File(file) => std::fs::read_to_string(file)
                .map(|t| t.trim().to_owned())
                .map_err(|_| {
                    SoundError::Unreachable("the hub has not made its token yet".to_owned())
                }),
        }
    }

    /// Sends a request to the hub, with a JSON body when there is one.
    ///
    /// # Errors
    /// [`SoundError`] if the hub cannot be reached or says no.
    pub fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, SoundError> {
        let url = format!("{}{path}", self.url);
        let signed = format!("Bearer {}", self.token()?);
        let response = match (method, body) {
            (Method::Get, _) => self.agent.get(&url).header("authorization", &signed).call(),
            (Method::Delete, _) => self
                .agent
                .delete(&url)
                .header("authorization", &signed)
                .call(),
            (Method::Post, Some(b)) => self
                .agent
                .post(&url)
                .header("authorization", &signed)
                .send_json(b),
            (Method::Post, None) => self
                .agent
                .post(&url)
                .header("authorization", &signed)
                .send_empty(),
            (Method::Put, Some(b)) => self
                .agent
                .put(&url)
                .header("authorization", &signed)
                .send_json(b),
            (Method::Put, None) => self
                .agent
                .put(&url)
                .header("authorization", &signed)
                .send_empty(),
        };
        let mut response = response.map_err(|e| SoundError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| SoundError::Unreachable(e.to_string()))?;
        let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(body);
        }
        let message = body["error"]
            .as_str()
            .map_or_else(|| format!("the sound hub answered {status}"), str::to_owned);
        Err(SoundError::Answered { status, message })
    }

    /// Opens a stream speaker's live MP3 stream, to be read for as long as it is listened to.
    ///
    /// # Errors
    /// [`SoundError`] if the hub cannot be reached or will not stream that speaker.
    pub fn listen(&self, speaker: u32) -> Result<Box<dyn std::io::Read + Send>, SoundError> {
        let response = self
            .streaming
            .get(&format!("{}/v1/listen/{speaker}", self.url))
            .header("authorization", &format!("Bearer {}", self.token()?))
            .call()
            .map_err(|e| SoundError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let mut response = response;
            let text = response.body_mut().read_to_string().unwrap_or_default();
            let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let message = body["error"]
                .as_str()
                .map_or_else(|| format!("the sound hub answered {status}"), str::to_owned);
            return Err(SoundError::Answered { status, message });
        }
        Ok(Box::new(response.into_body().into_reader()))
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
    fn every_request_is_signed_and_its_method_kept() {
        for (method, word) in [
            (Method::Get, "GET"),
            (Method::Put, "PUT"),
            (Method::Delete, "DELETE"),
        ] {
            let (url, seen) = one_answer("200 OK", "{}");
            let client = SoundClient::new(&url, "the-token");
            let body = serde_json::json!({ "level": 10 });
            client.send(method, "/v1/state", Some(&body)).unwrap();
            let request = seen.join().unwrap();
            assert!(
                request.starts_with(&format!("{word} /v1/state ")),
                "{request}"
            );
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer the-token"),
                "{request}"
            );
        }
    }

    #[test]
    fn what_the_hub_refuses_comes_back_as_what_it_said() {
        let (url, _seen) = one_answer(
            "409 Conflict",
            r#"{"error":"every AirPlay sender is busy with another mix"}"#,
        );
        let got = SoundClient::new(&url, "t").send(Method::Put, "/v1/wires", None);
        assert_eq!(
            got,
            Err(SoundError::Answered {
                status: 409,
                message: "every AirPlay sender is busy with another mix".to_owned()
            })
        );
    }

    #[test]
    fn a_hub_that_is_not_there_is_unreachable_not_a_refusal() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert!(matches!(
            SoundClient::new(&url, "t").send(Method::Get, "/v1/state", None),
            Err(SoundError::Unreachable(_))
        ));
    }

    #[test]
    fn a_token_the_hub_has_not_made_yet_is_said_plainly() {
        let client = SoundClient::with_token_file(
            "http://127.0.0.1:1",
            std::path::Path::new("/nonexistent/token"),
        );
        let got = client.send(Method::Get, "/v1/state", None);
        assert_eq!(
            got,
            Err(SoundError::Unreachable(
                "the hub has not made its token yet".to_owned()
            ))
        );
    }
}
