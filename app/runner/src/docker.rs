//! Docker, as the runner uses it: the few calls of the Engine API a turn needs, on Docker's
//! own socket. Behind a trait, so the loop is tested against a Docker that only pretends.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::http::{self, Answer};

/// The label every turn's container carries: its run.
pub const RUN_LABEL: &str = "endora.run";

/// The label every turn's container carries: the lease its turn was handed out with.
pub const LEASE_LABEL: &str = "endora.lease";

/// The label a check job's container carries: its check (ADR 0115).
pub const CHECK_LABEL: &str = "endora.check";

/// How long one call to Docker may take. Starting a container on a busy NAS is the slowest.
const ANSWERS_WITHIN: Duration = Duration::from_secs(30);

/// How a container is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Still going.
    Running,
    /// Ended, with its exit code.
    Exited(i64),
}

/// A turn's or a check's container, as Docker lists it: what a runner that restarts takes
/// back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Left {
    /// The container.
    pub id: String,
    /// Its run label, as it is written; empty for a check's container.
    pub run: String,
    /// Its lease label, as it is written; empty if it has none.
    pub lease: String,
    /// Its check label, as it is written; empty for a turn's container.
    pub check: String,
    /// When it was made, in seconds since the Unix epoch.
    pub created: u64,
}

/// What the runner asks of Docker, and nothing else.
pub trait Docker {
    /// Whether Docker answers.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn ping(&self) -> Result<(), String>;
    /// Makes a container from `spec`, the body of `POST /containers/create`; its id.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn create(&self, spec: &Value) -> Result<String, String>;
    /// Starts a container.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn start(&self, id: &str) -> Result<(), String>;
    /// How a container is; `None` if it is gone.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn inspect(&self, id: &str) -> Result<Option<State>, String>;
    /// Kills a container. One already stopped, or gone, is no failure.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn kill(&self, id: &str) -> Result<(), String>;
    /// The last `lines` lines a container wrote to stderr, as one text: what a worker said
    /// before it exited, read before the container is removed and its words with it.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn stderr_tail(&self, id: &str, lines: usize) -> Result<String, String>;
    /// Removes a container and the anonymous volumes it was given. One gone is no failure.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn remove(&self, id: &str) -> Result<(), String>;
    /// Every container labelled with a run, running or not.
    ///
    /// # Errors
    /// What Docker said, or that it could not be reached.
    fn left(&self) -> Result<Vec<Left>, String>;
}

/// Docker's Engine API on its unix socket.
pub struct Engine {
    socket: PathBuf,
}

impl Engine {
    /// Docker at `socket`.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    /// One call on a fresh connection to the socket.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Answer, String> {
        let mut socket = UnixStream::connect(&self.socket)
            .map_err(|e| format!("Docker cannot be reached: {e}"))?;
        socket
            .set_read_timeout(Some(ANSWERS_WITHIN))
            .and_then(|()| socket.set_write_timeout(Some(ANSWERS_WITHIN)))
            .map_err(|e| format!("Docker cannot be reached: {e}"))?;
        http::send(&mut socket, method, "localhost", path, None, body)
    }
}

/// What Docker said when it said no.
fn refusal(what: &str, answer: &Answer) -> String {
    let said = answer.json()["message"]
        .as_str()
        .map_or_else(String::new, |m| format!(": {m}"));
    format!("Docker would not {what} ({}){said}", answer.status)
}

/// A container's log as text. Docker sends the log of a container without a TTY as a
/// multiplexed stream: each frame an 8-byte header (the stream, 0, 1 or 2; three zero bytes;
/// the frame's size, big-endian) and then that many bytes. A body that does not start with
/// such a header is a TTY container's, written plain, and taken as it is.
fn demuxed(body: &[u8]) -> String {
    let mut text = Vec::new();
    let mut rest = body;
    while rest.len() >= 8 {
        let framed = rest[0] <= 2 && rest[1..4] == [0, 0, 0];
        if !framed {
            return String::from_utf8_lossy(body).into_owned();
        }
        let size = u32::from_be_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
        let frame = &rest[8..];
        text.extend_from_slice(&frame[..size.min(frame.len())]);
        rest = frame.get(size..).unwrap_or_default();
    }
    if text.is_empty() {
        return String::from_utf8_lossy(body).into_owned();
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// A container's id, as Docker writes one, so it goes into a path as nothing else.
fn checked(id: &str) -> Result<&str, String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("not a container's id".to_owned());
    }
    Ok(id)
}

impl Docker for Engine {
    fn ping(&self) -> Result<(), String> {
        let answer = self.call("GET", "/_ping", None)?;
        if answer.status != 200 {
            return Err(refusal("answer", &answer));
        }
        Ok(())
    }

    fn create(&self, spec: &Value) -> Result<String, String> {
        let answer = self.call("POST", "/containers/create", Some(spec))?;
        if answer.status != 201 {
            return Err(refusal("create it", &answer));
        }
        answer.json()["Id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "Docker made it but did not say its id".to_owned())
    }

    fn start(&self, id: &str) -> Result<(), String> {
        let answer = self.call("POST", &format!("/containers/{}/start", checked(id)?), None)?;
        // 304: it was started already.
        if answer.status == 204 || answer.status == 304 {
            return Ok(());
        }
        Err(refusal("start it", &answer))
    }

    fn inspect(&self, id: &str) -> Result<Option<State>, String> {
        let answer = self.call("GET", &format!("/containers/{}/json", checked(id)?), None)?;
        if answer.status == 404 {
            return Ok(None);
        }
        if answer.status != 200 {
            return Err(refusal("describe it", &answer));
        }
        let state = &answer.json()["State"];
        if state["Running"].as_bool() == Some(true) {
            return Ok(Some(State::Running));
        }
        // Made but never started: its exit code is 0, and it succeeded at nothing.
        if state["Status"] == "created" {
            return Ok(Some(State::Exited(-1)));
        }
        Ok(Some(State::Exited(
            state["ExitCode"].as_i64().unwrap_or(-1),
        )))
    }

    fn kill(&self, id: &str) -> Result<(), String> {
        let answer = self.call("POST", &format!("/containers/{}/kill", checked(id)?), None)?;
        // 404: gone; 409: not running. Either way it is not running now.
        if matches!(answer.status, 204 | 404 | 409) {
            return Ok(());
        }
        Err(refusal("kill it", &answer))
    }

    fn stderr_tail(&self, id: &str, lines: usize) -> Result<String, String> {
        let path = format!("/containers/{}/logs?stderr=1&tail={lines}", checked(id)?);
        let answer = self.call("GET", &path, None)?;
        if answer.status != 200 {
            return Err(refusal("give its log", &answer));
        }
        Ok(demuxed(&answer.body))
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        // The worker image declares a volume, so each container is given an anonymous one:
        // `v` removes it with the container, or every turn would leave one behind.
        let path = format!("/containers/{}?v=true&force=true", checked(id)?);
        let answer = self.call("DELETE", &path, None)?;
        // 404: gone already; 409: being removed already.
        if matches!(answer.status, 204 | 404 | 409) {
            return Ok(());
        }
        Err(refusal("remove it", &answer))
    }

    fn left(&self) -> Result<Vec<Left>, String> {
        let mut all: Vec<Left> = Vec::new();
        for label in [RUN_LABEL, CHECK_LABEL] {
            // `filters={"label":["endora.run"]}`, written for a URL.
            let path = format!(
                "/containers/json?all=true&filters=%7B%22label%22%3A%5B%22{label}%22%5D%7D"
            );
            let answer = self.call("GET", &path, None)?;
            if answer.status != 200 {
                return Err(refusal("list them", &answer));
            }
            all.extend(
                answer
                    .json()
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| {
                        let labels = &c["Labels"];
                        let run = labels[RUN_LABEL].as_str().unwrap_or("");
                        let check = labels[CHECK_LABEL].as_str().unwrap_or("");
                        if run.is_empty() && check.is_empty() {
                            return None;
                        }
                        Some(Left {
                            id: c["Id"].as_str()?.to_owned(),
                            run: run.to_owned(),
                            lease: labels[LEASE_LABEL].as_str().unwrap_or("").to_owned(),
                            check: check.to_owned(),
                            // Unknown is as old as can be: one still running is past its time.
                            created: c["Created"].as_u64().unwrap_or(0),
                        })
                    }),
            );
        }
        // Each container once, whatever labels it carries.
        let mut seen = std::collections::BTreeSet::new();
        all.retain(|left| seen.insert(left.id.clone()));
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};

    /// What the pretend Docker was asked: the request line and the body.
    type Asked = Arc<Mutex<Vec<(String, String)>>>;

    /// A pretend Docker on a socket of its own, answering each request line with `answer`.
    fn docker(name: &str, answer: fn(&str) -> String) -> (Engine, Asked) {
        let socket =
            std::env::temp_dir().join(format!("endora-runner-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let asked: Asked = Arc::new(Mutex::new(Vec::new()));
        let kept = asked.clone();
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
                let head = String::from_utf8_lossy(&head).into_owned();
                let line = head.lines().next().unwrap_or("").to_owned();
                let length: usize = head
                    .lines()
                    .filter_map(|l| l.split_once(": "))
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0u8; length];
                let _ = stream.read_exact(&mut body);
                let reply = answer(&line);
                kept.lock()
                    .unwrap()
                    .push((line, String::from_utf8_lossy(&body).into_owned()));
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (Engine::new(socket), asked)
    }

    fn whole(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// Docker's description of a container is long, so it comes in chunks.
    fn chunked(body: &str) -> String {
        let (a, b) = body.split_at(body.len() / 2);
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
            a.len(),
            b.len()
        )
    }

    #[test]
    fn a_turn_is_made_started_watched_killed_and_removed_through_dockers_api() {
        let (engine, asked) = docker("api", |line| match line {
            "GET /_ping HTTP/1.1" => "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK".to_owned(),
            "POST /containers/create HTTP/1.1" => {
                whole("201 Created", r#"{"Id":"c0ffee","Warnings":[]}"#)
            }
            "POST /containers/c0ffee/start HTTP/1.1" => {
                "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_owned()
            }
            "GET /containers/c0ffee/json HTTP/1.1" => chunked(
                r#"{"Id":"c0ffee","State":{"Status":"exited","Running":false,"ExitCode":3}}"#,
            ),
            "GET /containers/ended/json HTTP/1.1" => {
                chunked(r#"{"State":{"Status":"exited","Running":false,"ExitCode":0}}"#)
            }
            "GET /containers/never/json HTTP/1.1" => {
                chunked(r#"{"State":{"Status":"created","Running":false,"ExitCode":0}}"#)
            }
            "GET /containers/going/json HTTP/1.1" => {
                chunked(r#"{"State":{"Status":"running","Running":true,"ExitCode":0}}"#)
            }
            "GET /containers/gone/json HTTP/1.1" => {
                whole("404 Not Found", r#"{"message":"No such container: gone"}"#)
            }
            "POST /containers/c0ffee/kill HTTP/1.1" => whole(
                "409 Conflict",
                r#"{"message":"container c0ffee is not running"}"#,
            ),
            "DELETE /containers/c0ffee?v=true&force=true HTTP/1.1" => {
                "HTTP/1.1 204 No Content\r\n\r\n".to_owned()
            }
            // Two frames of stderr (stream 2), as Docker multiplexes a container's log.
            "GET /containers/c0ffee/logs?stderr=1&tail=8 HTTP/1.1" => whole(
                "200 OK",
                "\u{2}\0\0\0\0\0\0\u{15}worker: round 1 done\n\u{2}\0\0\0\0\0\0\u{1a}worker: the model said no\n",
            ),
            "GET /containers/tty/logs?stderr=1&tail=8 HTTP/1.1" => {
                whole("200 OK", "plain words from a tty\n")
            }
            line if line.starts_with("GET /containers/json?") => whole(
                "200 OK",
                r#"[{"Id":"old","Created":1700000000,"Labels":{"endora.run":"7","endora.lease":"1007"},"State":"running"}]"#,
            ),
            _ => whole(
                "500 Internal Server Error",
                r#"{"message":"not pretended"}"#,
            ),
        });
        engine.ping().unwrap();
        let spec = json!({ "Image": "endora-node:latest" });
        assert_eq!(engine.create(&spec).unwrap(), "c0ffee");
        engine.start("c0ffee").unwrap();
        assert_eq!(engine.inspect("c0ffee").unwrap(), Some(State::Exited(3)));
        assert_eq!(engine.inspect("ended").unwrap(), Some(State::Exited(0)));
        assert_eq!(
            engine.inspect("never").unwrap(),
            Some(State::Exited(-1)),
            "made but never started is no success"
        );
        assert_eq!(engine.inspect("going").unwrap(), Some(State::Running));
        assert_eq!(engine.inspect("gone").unwrap(), None);
        engine.kill("c0ffee").unwrap();
        assert_eq!(
            engine.stderr_tail("c0ffee", 8).unwrap(),
            "worker: round 1 done\nworker: the model said no\n",
            "the frames' words, without their headers"
        );
        assert_eq!(
            engine.stderr_tail("tty", 8).unwrap(),
            "plain words from a tty\n",
            "a tty container's log is not framed"
        );
        engine.remove("c0ffee").unwrap();
        assert_eq!(
            engine.left().unwrap(),
            vec![Left {
                id: "old".to_owned(),
                run: "7".to_owned(),
                lease: "1007".to_owned(),
                check: String::new(),
                created: 1_700_000_000
            }]
        );

        let asked = asked.lock().unwrap().clone();
        assert_eq!(
            asked[1].1,
            spec.to_string(),
            "the spec is the body, as it is"
        );
        let lines: Vec<&str> = asked.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            &lines[lines.len() - 2..],
            [
                "GET /containers/json?all=true&filters=%7B%22label%22%3A%5B%22endora.run%22%5D%7D HTTP/1.1",
                "GET /containers/json?all=true&filters=%7B%22label%22%3A%5B%22endora.check%22%5D%7D HTTP/1.1",
            ],
            "only the runner's own containers, turns and checks, stopped ones too"
        );
    }

    #[test]
    fn what_docker_refuses_is_said_and_an_odd_id_is_never_sent() {
        let (engine, asked) = docker("refused", |_| {
            whole(
                "404 Not Found",
                r#"{"message":"No such image: endora-node:latest"}"#,
            )
        });
        let refused = engine.create(&json!({})).unwrap_err();
        assert!(refused.contains("No such image"), "{refused}");
        for odd in ["../images", "c?all=1", "", "c d"] {
            assert!(engine.kill(odd).is_err());
            assert!(engine.stderr_tail(odd, 8).is_err());
            assert!(engine.remove(odd).is_err());
        }
        assert_eq!(asked.lock().unwrap().len(), 1, "only the create was sent");
        let nowhere = Engine::new("/nonexistent/docker.sock");
        assert!(nowhere.ping().is_err());
        assert!(nowhere.inspect("c0ffee").is_err());
    }
}
