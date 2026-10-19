//! `endora-hq drain` and `endora-hq resume` (ADR 0134): the command a deploy, or the NAS's
//! shutdown, runs inside the HQ container before it restarts anything. It talks to HQ's own API
//! on this machine, with HQ's own token, and prints what it did.
//!
//! ```text
//! endora-hq drain [--cap SECONDS] [--wait]   announce a restart; with --wait, return once no
//!                                            turn is running or the cap has passed
//! endora-hq resume                           lift the announcement
//! ```

use std::io::Write;

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    /// Announce a restart, and with `wait`, wait for the turns under way.
    Drain {
        /// How long a restart waits, in seconds; HQ's default when none.
        cap: Option<i64>,
        /// Whether to wait.
        wait: bool,
    },
    /// Lift the announcement.
    Resume,
}

/// What the program was started with: `None` for nothing, which starts HQ itself. Anything else
/// is a command or an error, and never starts a second HQ over the same database.
pub(crate) fn parse(args: &[String]) -> Result<Option<Command>, String> {
    let Some((first, rest)) = args.split_first() else {
        return Ok(None);
    };
    match first.as_str() {
        "resume" if rest.is_empty() => Ok(Some(Command::Resume)),
        "drain" => parse_drain(rest).map(Some),
        _ => Err(USAGE.to_owned()),
    }
}

const USAGE: &str = "usage: endora-hq [drain [--cap SECONDS] [--wait] | resume]";

fn parse_drain(args: &[String]) -> Result<Command, String> {
    let (mut cap, mut wait) = (None, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let seconds = match arg.as_str() {
            "--wait" => {
                wait = true;
                continue;
            }
            "--cap" => args.next().ok_or_else(|| USAGE.to_owned())?,
            other => other
                .strip_prefix("--cap=")
                .ok_or_else(|| USAGE.to_owned())?,
        };
        cap = Some(
            seconds
                .parse::<i64>()
                .ok()
                .filter(|s| *s > 0)
                .ok_or_else(|| USAGE.to_owned())?,
        );
    }
    Ok(Command::Drain { cap, wait })
}

/// A door into HQ: one request, answered with a status and a body.
pub(crate) trait Door {
    /// Sends `method` to `path` with `body`.
    fn send(&self, method: &str, path: &str, body: &str) -> Result<(u16, String), String>;
}

/// The world a command runs in: the door, the clock in milliseconds, and sleeping.
pub(crate) struct World<'a> {
    pub(crate) door: &'a dyn Door,
    pub(crate) now: &'a dyn Fn() -> i64,
    pub(crate) sleep: &'a dyn Fn(u64),
}

/// How often `--wait` looks, in milliseconds.
pub(crate) const LOOK_EVERY_MS: u64 = 5_000;

/// Runs `command`, saying what it does on `out`. The exit code: 0 when HQ was told, whatever the
/// turns did; 1 when HQ could not be.
pub(crate) fn run(command: &Command, world: &World<'_>, out: &mut dyn Write) -> i32 {
    let said = match command {
        Command::Drain { cap, wait } => drain(*cap, *wait, world, out),
        Command::Resume => resume(world, out),
    };
    match said {
        Ok(()) => 0,
        Err(why) => {
            let _ = writeln!(out, "drain: {why}");
            1
        }
    }
}

/// What HQ told, parsed.
fn answer(sent: Result<(u16, String), String>) -> Result<serde_json::Value, String> {
    let (status, body) = sent.map_err(|e| format!("could not reach HQ: {e}"))?;
    if status != 200 {
        return Err(format!("HQ answered {status}: {}", body.trim()));
    }
    serde_json::from_str(&body).map_err(|e| format!("HQ's answer was not understood: {e}"))
}

fn turns(n: u64) -> String {
    format!("{n} turn{}", if n == 1 { "" } else { "s" })
}

fn drain(
    cap: Option<i64>,
    wait: bool,
    world: &World<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let body = cap.map_or_else(|| "{}".to_owned(), |c| format!(r#"{{"cap_seconds":{c}}}"#));
    let mut shown = answer(world.door.send("POST", "/v1/team/drain", &body))?;
    let cap_seconds = shown["cap_seconds"].as_i64().unwrap_or(0);
    let mut running = shown["running"].as_u64().unwrap_or(0);
    let _ = writeln!(
        out,
        "drain: a restart is announced; {} running, waiting up to {cap_seconds} s",
        turns(running)
    );
    if !wait {
        return Ok(());
    }
    // The drain's own clock, not this command's: asking again never gives a restart more time.
    let deadline = shown["since"].as_i64().unwrap_or_else(|| (world.now)()) + cap_seconds * 1_000;
    while running > 0 {
        let left = deadline - (world.now)();
        if left <= 0 {
            let _ = writeln!(
                out,
                "drain: the cap of {cap_seconds} s passed with {} still running: the restart \
                 cuts them, and each is tried again",
                turns(running)
            );
            return Ok(());
        }
        (world.sleep)(LOOK_EVERY_MS.min(u64::try_from(left).unwrap_or(0)));
        shown = answer(world.door.send("GET", "/v1/team/drain", ""))?;
        let now_running = shown["running"].as_u64().unwrap_or(0);
        if now_running != running && now_running > 0 {
            let _ = writeln!(out, "drain: {} still running", turns(now_running));
        }
        running = now_running;
    }
    let _ = writeln!(out, "drain: no turn is running: the restart can go ahead");
    Ok(())
}

fn resume(world: &World<'_>, out: &mut dyn Write) -> Result<(), String> {
    let shown = answer(world.door.send("POST", "/v1/team/resume", ""))?;
    let last = &shown["last"];
    if last.is_null() {
        let _ = writeln!(out, "drain: lifted");
        return Ok(());
    }
    let _ = writeln!(
        out,
        "drain: lifted. The last drain: {} ended before it, {} cut",
        last["clean"].as_u64().unwrap_or(0),
        last["cut"].as_u64().unwrap_or(0)
    );
    Ok(())
}

/// HQ's address on this machine, from `HQ_ADDR` (0.0.0.0:8790 in the container).
pub(crate) fn address(hq_addr: Option<&str>) -> String {
    let Some((host, port)) = hq_addr.and_then(|a| a.rsplit_once(':')) else {
        return "127.0.0.1:8790".to_owned();
    };
    let host = match host {
        "" | "0.0.0.0" | "[::]" | "::" => "127.0.0.1",
        other => other,
    };
    format!("{host}:{port}")
}

/// The token HQ's own API takes: `HQ_TOKEN`, else the file `HQ_TOKEN_FILE`. Read, never made.
pub(crate) fn token(env: Option<&str>, file: Option<&str>) -> Result<String, String> {
    let given = env
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    let token = match (given, file.filter(|f| !f.trim().is_empty())) {
        (Some(token), _) => token,
        (None, Some(file)) => std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read the token in {file}: {e}"))?
            .trim()
            .to_owned(),
        (None, None) => return Err("neither HQ_TOKEN nor HQ_TOKEN_FILE is set".to_owned()),
    };
    if token.len() < 32 {
        return Err("HQ's token is shorter than 32 characters".to_owned());
    }
    Ok(token)
}

/// The door over a plain TCP connection to `addr`, signed with `token`.
pub(crate) struct Tcp {
    pub(crate) addr: String,
    pub(crate) token: String,
}

impl Door for Tcp {
    fn send(&self, method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
        use std::io::Read as _;
        use std::net::{TcpStream, ToSocketAddrs as _};
        use std::time::Duration;
        let found = self
            .addr
            .to_socket_addrs()
            .map_err(|e| format!("{}: {e}", self.addr))?
            .next()
            .ok_or_else(|| format!("{} is no address", self.addr))?;
        let mut stream = TcpStream::connect_timeout(&found, Duration::from_secs(5))
            .map_err(|e| format!("{}: {e}", self.addr))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|e| e.to_string())?;
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.addr,
            self.token,
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        read_reply(&String::from_utf8_lossy(&raw))
    }
}

/// A reply read off the wire: its status, and its body, whole.
fn read_reply(raw: &str) -> Result<(u16, String), String> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| "HQ's reply was cut short".to_owned())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| "HQ's reply had no status".to_owned())?;
    let chunked = head
        .lines()
        .any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.contains("chunked"));
    if chunked {
        return Ok((status, unchunk(body)?));
    }
    Ok((status, body.to_owned()))
}

/// A chunked body, whole: each chunk is its length in hex, a line end, that many bytes, a line end.
fn unchunk(mut rest: &str) -> Result<String, String> {
    let mut whole = String::new();
    loop {
        let (size, after) = rest
            .split_once("\r\n")
            .ok_or_else(|| "HQ's reply was cut short".to_owned())?;
        let size = usize::from_str_radix(size.trim(), 16)
            .map_err(|_| "HQ's reply was not understood".to_owned())?;
        if size == 0 {
            return Ok(whole);
        }
        let chunk = after
            .get(..size)
            .ok_or_else(|| "HQ's reply was cut short".to_owned())?;
        whole.push_str(chunk);
        rest = after
            .get(size..)
            .and_then(|r| r.strip_prefix("\r\n"))
            .ok_or_else(|| "HQ's reply was cut short".to_owned())?;
    }
}

/// The program's entry for a command line: `None` when there is none and HQ is to start; else
/// the exit code of the command, which is never a second HQ.
pub(crate) fn handle(args: &[String]) -> Option<i32> {
    match parse(args) {
        Ok(None) => None,
        Ok(Some(command)) => Some(command_line(&command)),
        Err(why) => {
            eprintln!("{why}");
            Some(2)
        }
    }
}

fn command_line(command: &Command) -> i32 {
    let token = match token(
        std::env::var("HQ_TOKEN").ok().as_deref(),
        std::env::var("HQ_TOKEN_FILE").ok().as_deref(),
    ) {
        Ok(token) => token,
        Err(why) => {
            eprintln!("drain: {why}");
            return 1;
        }
    };
    let door = Tcp {
        addr: address(std::env::var("HQ_ADDR").ok().as_deref()),
        token,
    };
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    };
    let sleep = |ms: u64| std::thread::sleep(std::time::Duration::from_millis(ms));
    run(
        command,
        &World {
            door: &door,
            now: &now,
            sleep: &sleep,
        },
        &mut std::io::stdout(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_arguments_start_hq_and_every_other_is_a_command_or_an_error() {
        assert_eq!(parse(&args(&[])), Ok(None));
        assert_eq!(
            parse(&args(&["drain"])),
            Ok(Some(Command::Drain {
                cap: None,
                wait: false
            }))
        );
        assert_eq!(
            parse(&args(&["drain", "--cap", "120", "--wait"])),
            Ok(Some(Command::Drain {
                cap: Some(120),
                wait: true
            }))
        );
        assert_eq!(
            parse(&args(&["drain", "--wait", "--cap=45"])),
            Ok(Some(Command::Drain {
                cap: Some(45),
                wait: true
            }))
        );
        assert_eq!(parse(&args(&["resume"])), Ok(Some(Command::Resume)));
        for bad in [
            vec!["drian"],
            vec!["--serve"],
            vec!["drain", "--cap"],
            vec!["drain", "--cap", "soon"],
            vec!["drain", "--cap", "0"],
            vec!["drain", "--cap", "-3"],
            vec!["drain", "--nope"],
            vec!["resume", "--wait"],
        ] {
            let said = parse(&args(&bad)).unwrap_err();
            assert!(said.contains("usage"), "{bad:?}: {said}");
        }
    }

    #[test]
    fn only_no_arguments_start_hq_and_a_mistake_is_never_a_second_hq() {
        assert_eq!(handle(&args(&[])), None);
        assert_eq!(handle(&args(&["drian"])), Some(2));
        assert_eq!(handle(&args(&["--help"])), Some(2));
    }

    #[test]
    fn hq_is_reached_on_this_machine_at_the_port_it_listens_on() {
        assert_eq!(address(Some("0.0.0.0:8790")), "127.0.0.1:8790");
        assert_eq!(address(Some("127.0.0.1:9000")), "127.0.0.1:9000");
        assert_eq!(address(Some("[::]:8790")), "127.0.0.1:8790");
        assert_eq!(address(Some("hq.example.com:8790")), "hq.example.com:8790");
        assert_eq!(address(None), "127.0.0.1:8790");
    }

    #[test]
    fn the_token_is_read_never_made_and_never_short() {
        let long = "a-token-of-at-least-thirty-two-characters";
        assert_eq!(token(Some(long), None).unwrap(), long);
        assert_eq!(token(Some(&format!(" {long}\n")), None).unwrap(), long);
        let dir = std::env::temp_dir().join(format!("endora-hq-drain-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token");
        std::fs::write(&file, format!("{long}\n")).unwrap();
        assert_eq!(token(None, Some(file.to_str().unwrap())).unwrap(), long);
        let missing = dir.join("nothing-here");
        assert!(token(None, Some(missing.to_str().unwrap())).is_err());
        assert!(!missing.exists(), "a missing token file is never made");
        std::fs::write(&file, "short").unwrap();
        assert!(token(None, Some(file.to_str().unwrap())).is_err());
        assert!(token(None, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reply_is_read_whole_whether_or_not_it_came_in_chunks() {
        let plain = "HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"ok\":true}\n";
        assert_eq!(read_reply(plain), Ok((200, "{\"ok\":true}\n".to_owned())));
        let chunked = "HTTP/1.1 401 Unauthorized\r\nTransfer-Encoding: chunked\r\n\r\n\
                       5\r\n{\"err\r\n7\r\nor\":1}\n\r\n0\r\n\r\n";
        assert_eq!(read_reply(chunked), Ok((401, "{\"error\":1}\n".to_owned())));
        assert!(
            read_reply("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\n{\"e").is_err()
        );
        assert!(read_reply("nothing").is_err());
    }

    /// HQ as a script: each request is answered with the next reply, and recorded.
    struct Script {
        replies: RefCell<Vec<Result<(u16, String), String>>>,
        asked: RefCell<Vec<(String, String, String)>>,
    }

    impl Script {
        fn new(replies: Vec<Result<(u16, String), String>>) -> Self {
            Self {
                replies: RefCell::new(replies),
                asked: RefCell::default(),
            }
        }
    }

    impl Door for Script {
        fn send(&self, method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
            self.asked
                .borrow_mut()
                .push((method.to_owned(), path.to_owned(), body.to_owned()));
            let mut replies = self.replies.borrow_mut();
            assert!(
                !replies.is_empty(),
                "asked more than the script has: {method} {path}"
            );
            replies.remove(0)
        }
    }

    fn drain(running: u64, since: i64, cap: i64) -> Result<(u16, String), String> {
        Ok((
            200,
            format!(
                r#"{{"draining":true,"since":{since},"cap_seconds":{cap},"running":{running},"waiting":0,"last":null}}"#
            ),
        ))
    }

    /// Runs `command` against `door` with a clock that starts at 1,000,000 ms and moves only
    /// when the command sleeps. Returns the exit code, what it said, and how long it slept.
    fn go(command: &Command, door: &Script) -> (i32, String, u64) {
        let clock = Cell::new(1_000_000_i64);
        let slept = Cell::new(0_u64);
        let now = || clock.get();
        let sleep = |ms: u64| {
            slept.set(slept.get() + ms);
            clock.set(clock.get() + i64::try_from(ms).unwrap());
        };
        let mut out = Vec::new();
        let code = run(
            command,
            &World {
                door,
                now: &now,
                sleep: &sleep,
            },
            &mut out,
        );
        (code, String::from_utf8(out).unwrap(), slept.get())
    }

    #[test]
    fn a_drain_without_waiting_announces_the_restart_and_returns() {
        let hq = Script::new(vec![drain(2, 1_000_000, 300)]);
        let (code, said, slept) = go(
            &Command::Drain {
                cap: None,
                wait: false,
            },
            &hq,
        );
        assert_eq!(code, 0, "{said}");
        assert_eq!(slept, 0);
        assert_eq!(
            hq.asked.borrow().as_slice(),
            [(
                "POST".to_owned(),
                "/v1/team/drain".to_owned(),
                "{}".to_owned()
            )]
        );
        assert!(
            said.contains("2 turns running") && said.contains("300 s"),
            "{said}"
        );
    }

    #[test]
    fn a_cap_is_sent_as_asked() {
        let hq = Script::new(vec![drain(0, 1_000_000, 90)]);
        go(
            &Command::Drain {
                cap: Some(90),
                wait: false,
            },
            &hq,
        );
        assert_eq!(hq.asked.borrow()[0].2, r#"{"cap_seconds":90}"#);
    }

    #[test]
    fn waiting_returns_as_soon_as_no_turn_is_running() {
        let hq = Script::new(vec![
            drain(2, 1_000_000, 300),
            drain(2, 1_000_000, 300),
            drain(1, 1_000_000, 300),
            drain(0, 1_000_000, 300),
        ]);
        let (code, said, slept) = go(
            &Command::Drain {
                cap: None,
                wait: true,
            },
            &hq,
        );
        assert_eq!(code, 0, "{said}");
        assert_eq!(
            slept,
            3 * LOOK_EVERY_MS,
            "it looked three more times, not once more"
        );
        assert!(said.contains("1 turn still running"), "{said}");
        assert!(said.contains("no turn is running"), "{said}");
        assert!(said.contains("can go ahead"), "{said}");
        let asked = hq.asked.borrow();
        assert_eq!(asked[1].0, "GET");
        assert_eq!(asked[1].1, "/v1/team/drain");
    }

    #[test]
    fn waiting_gives_up_at_the_cap_and_says_what_the_restart_will_cut() {
        // 60 s cap: looks at 5 s intervals; a turn that never ends.
        let mut replies = vec![drain(1, 1_000_000, 60)];
        replies.extend((0..30).map(|_| drain(1, 1_000_000, 60)));
        let hq = Script::new(replies);
        let (code, said, slept) = go(
            &Command::Drain {
                cap: Some(60),
                wait: true,
            },
            &hq,
        );
        assert_eq!(
            code, 0,
            "the restart goes ahead whatever the turns did: {said}"
        );
        assert_eq!(slept, 60_000, "no longer than the cap");
        assert!(said.contains("cap of 60 s"), "{said}");
        assert!(said.contains("1 turn still running"), "{said}");
        assert!(said.contains("cut"), "{said}");
    }

    #[test]
    fn a_drain_begun_earlier_keeps_its_own_clock() {
        // Begun 290 s ago with a cap of 300: ten seconds are left, not three hundred.
        let hq = Script::new(vec![
            drain(1, 1_000_000 - 290_000, 300),
            drain(1, 1_000_000 - 290_000, 300),
            drain(1, 1_000_000 - 290_000, 300),
            drain(1, 1_000_000 - 290_000, 300),
        ]);
        let (_, said, slept) = go(
            &Command::Drain {
                cap: None,
                wait: true,
            },
            &hq,
        );
        assert_eq!(slept, 10_000, "{said}");
    }

    #[test]
    fn a_door_that_does_not_open_is_an_error_to_say_and_a_failure_to_return() {
        let down = Script::new(vec![Err("connection refused".to_owned())]);
        let (code, said, _) = go(
            &Command::Drain {
                cap: None,
                wait: true,
            },
            &down,
        );
        assert_eq!(code, 1);
        assert!(said.contains("connection refused"), "{said}");

        let refused = Script::new(vec![Ok((401, r#"{"error":"no"}"#.to_owned()))]);
        let (code, said, _) = go(&Command::Resume, &refused);
        assert_eq!(code, 1);
        assert!(said.contains("401"), "{said}");
    }

    #[test]
    fn resume_lifts_the_drain_and_says_what_it_came_to() {
        let hq = Script::new(vec![Ok((
            200,
            r#"{"draining":false,"since":null,"cap_seconds":null,"running":0,"waiting":1,"last":{"since":1,"by":"resume","held":2,"clean":2,"cut":0}}"#
                .to_owned(),
        ))]);
        let (code, said, _) = go(&Command::Resume, &hq);
        assert_eq!(code, 0, "{said}");
        assert_eq!(
            hq.asked.borrow().as_slice(),
            [(
                "POST".to_owned(),
                "/v1/team/resume".to_owned(),
                String::new()
            )]
        );
        assert!(said.contains("lifted"), "{said}");
        assert!(said.contains("2 ended") && said.contains("0 cut"), "{said}");
    }
}
