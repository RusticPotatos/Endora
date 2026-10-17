//! HTTP/1.1, written by hand: one request on a fresh connection, closed after its answer.
//! Enough for Docker's API on its socket and HQ's three runner routes, and no more, so the
//! program that holds Docker's socket needs no HTTP library.

use std::io::{Read, Write};

use serde_json::Value;

/// The most of an answer the runner reads. Docker's description of one container is a few
/// kilobytes; anything near this is not an answer the runner asked for.
const LARGEST_ANSWER: u64 = 4 * 1024 * 1024;

/// An answer: its status, and its whole body.
#[derive(Debug)]
pub struct Answer {
    /// The HTTP status.
    pub status: u16,
    /// The body, put back together if it came in chunks.
    pub body: Vec<u8>,
}

impl Answer {
    /// The body as JSON; null if it is not.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Sends one request on `stream` and reads its whole answer. `bearer` is sent as
/// `Authorization: Bearer`, and `body` as JSON.
///
/// # Errors
/// A line break where one would end a header (nothing is sent then), the connection failed,
/// or the answer is not HTTP.
pub fn send<S: Read + Write>(
    stream: &mut S,
    method: &str,
    host: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&Value>,
) -> Result<Answer, String> {
    let breaks = |s: &str| s.contains(['\r', '\n']);
    if breaks(method) || breaks(host) || breaks(path) || bearer.is_some_and(breaks) {
        return Err("a line break in a request header".to_owned());
    }
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    let sent = body.map(Value::to_string);
    if let Some(sent) = &sent {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            sent.len()
        ));
    } else if method != "GET" {
        request.push_str("Content-Length: 0\r\n");
    }
    request.push_str("\r\n");
    request.push_str(sent.as_deref().unwrap_or(""));
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|e| format!("could not send: {e}"))?;

    let mut raw = Vec::new();
    stream
        .take(LARGEST_ANSWER + 1)
        .read_to_end(&mut raw)
        .map_err(|e| format!("no answer: {e}"))?;
    if raw.len() as u64 > LARGEST_ANSWER {
        return Err("an answer too large to be one".to_owned());
    }
    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err("an answer that is not HTTP".to_owned());
    };
    let head = String::from_utf8_lossy(&raw[..end]);
    let rest = &raw[end + 4..];
    let status = head
        .split("\r\n")
        .next()
        .filter(|l| l.starts_with("HTTP/1."))
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("an answer that is not HTTP")?;
    let header = |name: &str| {
        head.split("\r\n")
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_owned())
    };
    if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        return Ok(Answer {
            status,
            body: unchunk(rest)?,
        });
    }
    let length = header("content-length").and_then(|v| v.parse::<usize>().ok());
    let body = match length {
        Some(length) if rest.len() < length => return Err("an answer cut short".to_owned()),
        Some(length) => rest[..length].to_vec(),
        // No length and not chunked: the body is what came before the connection closed.
        None => rest.to_vec(),
    };
    Ok(Answer { status, body })
}

/// Puts a chunked body back together.
fn unchunk(mut rest: &[u8]) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let Some(end) = rest.windows(2).position(|w| w == b"\r\n") else {
            return Err("a chunk without its size".to_owned());
        };
        let size = std::str::from_utf8(&rest[..end])
            .ok()
            .and_then(|line| line.split(';').next())
            .and_then(|hex| usize::from_str_radix(hex.trim(), 16).ok())
            .ok_or("a chunk without its size")?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Ok(body);
        }
        if rest.len() < size {
            return Err("a chunk cut short".to_owned());
        }
        body.extend_from_slice(&rest[..size]);
        rest = rest.get(size + 2..).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    /// A connection that answers with `answer` and keeps what was written to it.
    struct Wire {
        answer: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl Wire {
        fn answering(answer: &str) -> Self {
            Self {
                answer: Cursor::new(answer.as_bytes().to_vec()),
                written: Vec::new(),
            }
        }
        fn written(&self) -> String {
            String::from_utf8_lossy(&self.written).into_owned()
        }
    }

    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.answer.read(buf)
        }
    }

    impl Write for Wire {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_request_is_one_plain_http_11_message_closed_after_its_answer() {
        let mut wire = Wire::answering(
            "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"Id\":\"c\"}\ntrailing",
        );
        let answer = send(
            &mut wire,
            "POST",
            "localhost",
            "/containers/create",
            Some("abc"),
            Some(&json!({ "Image": "x" })),
        )
        .unwrap();
        assert_eq!(
            wire.written(),
            "POST /containers/create HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAuthorization: Bearer abc\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"Image\":\"x\"}"
        );
        assert_eq!(answer.status, 201);
        assert_eq!(
            answer.json(),
            json!({ "Id": "c" }),
            "only its length is the body"
        );

        let mut wire = Wire::answering("HTTP/1.1 204 No Content\r\n\r\n");
        let answer = send(&mut wire, "GET", "hq:8790", "/v1/runner/stops", None, None).unwrap();
        assert_eq!(
            wire.written(),
            "GET /v1/runner/stops HTTP/1.1\r\nHost: hq:8790\r\nConnection: close\r\n\r\n",
            "no body, no length"
        );
        assert_eq!(answer.status, 204);
        assert!(answer.body.is_empty());
    }

    #[test]
    fn an_answer_in_chunks_comes_back_whole() {
        let mut wire = Wire::answering(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"State\r\nb;x=y\r\n\":{\"Code\":3\r\n2\r\n}}\r\n0\r\n\r\n",
        );
        let answer = send(
            &mut wire,
            "GET",
            "localhost",
            "/containers/c/json",
            None,
            None,
        )
        .unwrap();
        assert_eq!(answer.status, 200);
        assert_eq!(answer.json(), json!({ "State": { "Code": 3 } }));

        let mut cut =
            Wire::answering("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nff\r\nshort");
        assert!(
            send(&mut cut, "GET", "localhost", "/x", None, None).is_err(),
            "a chunk cut short is not an answer"
        );
        let mut nonsense = Wire::answering("SSH-2.0-OpenSSH\r\n\r\n");
        assert!(send(&mut nonsense, "GET", "localhost", "/x", None, None).is_err());
    }

    #[test]
    fn a_line_break_is_never_sent() {
        for (path, bearer) in [
            ("/x\r\nX-Evil: 1", None),
            ("/x", Some("token\r\nX-Evil: 1")),
            ("/x", Some("token\n")),
        ] {
            let mut wire = Wire::answering("HTTP/1.1 200 OK\r\n\r\n");
            assert!(send(&mut wire, "GET", "localhost", path, bearer, None).is_err());
            assert!(wire.written.is_empty(), "nothing was sent");
        }
    }
}
