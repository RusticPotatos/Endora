//! The workbench's shell (ring 1): a real `bash` in the disposable sandbox the worker runs in,
//! offered only there. It is not a contract and goes to neither HQ nor the node: the command
//! runs right here, because the sandbox itself is the boundary. The sandbox is offline: it
//! reaches nothing but the team gateway (the egress proxy is deferred, ADR 0110); the house's
//! own services are not reachable from it, and are used through the bridge and the contracts.
//!
//! A command runs as `bash -lc` in the workbench's directory, with stdout and stderr together,
//! for at most [`LONGEST`]; what it printed is kept to [`OUTPUT_AT_MOST`] bytes, its head and
//! its tail, and its exit code is said. A command that outlives its time is killed with every
//! process it started.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a command has unless the model says otherwise.
pub const USUAL: Duration = Duration::from_secs(60);

/// The longest a command may have.
pub const LONGEST: Duration = Duration::from_secs(300);

/// The most of a command's output the model reads: half from its start, half from its end.
pub const OUTPUT_AT_MOST: usize = 16 * 1024;

/// How long the output is waited for once the command has ended or been killed: a process it
/// left behind holding the pipe open does not hold the turn.
const DRAINED_WITHIN: Duration = Duration::from_secs(2);

/// What a command printed and what the shell cut of it, as the text it is part of holds it: the
/// bytes printed, the bytes the shell left out, and where in that text the shell's note is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Earlier {
    /// Bytes the command printed.
    pub printed: usize,
    /// Of those, bytes the shell left out.
    pub cut: usize,
    /// Where the shell's note of the cut is, in the text; `None` when it cut nothing.
    pub marker: Option<std::ops::Range<usize>>,
}

/// The worker's own environment that a command is not given.
const NOT_GIVEN: [&str; 1] = ["ENDORA_RUN_TOKEN"];

/// The shell of the sandbox the worker runs in.
pub struct Shell {
    /// Where commands run: the workbench's writable home, `/work`.
    dir: PathBuf,
    /// What never leaves in a command's output: the turn's token (which a proxy's address
    /// carries), replaced wherever it appears.
    hidden: String,
}

/// What a command came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// Its exit code; `None` when it was killed.
    pub code: Option<i32>,
    /// Whether it outlived its time and was killed.
    pub timed_out: bool,
    /// What it printed, stdout and stderr together, kept to [`OUTPUT_AT_MOST`].
    pub output: String,
    /// How many bytes it printed in all.
    pub printed: usize,
    /// How many of those the shell left out of `output`: none when it was within the limit.
    pub cut: usize,
    /// Where in `output` the shell's own note of its cut is, when it cut: known here, never
    /// read back from the text, so nothing a command prints can pass for it.
    pub marker: Option<std::ops::Range<usize>>,
}

impl Ran {
    /// What the model reads: how it ended, then what it printed.
    #[must_use]
    pub fn said(&self, within: Duration) -> String {
        let ended = match (self.timed_out, self.code) {
            (true, _) => format!("timed out after {} s, and was stopped", within.as_secs()),
            (false, Some(code)) => format!("exit {code}"),
            (false, None) => "killed by a signal".to_owned(),
        };
        if self.output.is_empty() {
            return format!("{ended}\n(no output)");
        }
        format!("{ended}\n{}", self.output)
    }

    /// What was printed and what the shell cut, placed in [`Ran::said`]'s text: for whoever
    /// cuts that text again.
    #[must_use]
    pub fn cut_in_said(&self, said: &str) -> Earlier {
        // `said` is how it ended, a line, then the output.
        let at = said.len() - self.output.len();
        Earlier {
            printed: self.printed,
            cut: self.cut,
            marker: self.marker.clone().map(|m| m.start + at..m.end + at),
        }
    }

    /// Whether it went well: it ended, with 0.
    #[must_use]
    pub fn ok(&self) -> bool {
        !self.timed_out && self.code == Some(0)
    }
}

impl Shell {
    /// A shell that runs commands in `dir`, keeping `hidden` out of what they print.
    #[must_use]
    pub fn new(dir: PathBuf, hidden: &str) -> Self {
        Self {
            dir,
            hidden: hidden.to_owned(),
        }
    }

    /// How long a call's command has: `seconds` if given, within one and [`LONGEST`].
    #[must_use]
    pub fn within(seconds: Option<u64>) -> Duration {
        seconds.map_or(USUAL, |s| {
            Duration::from_secs(s).clamp(Duration::from_secs(1), LONGEST)
        })
    }

    /// Runs `command`, for at most `within`.
    ///
    /// # Errors
    /// When `bash` cannot be started at all.
    pub fn run(&self, command: &str, within: Duration) -> Result<Ran, String> {
        let (reader, writer) = std::io::pipe().map_err(|e| format!("no pipe: {e}"))?;
        let both = writer.try_clone().map_err(|e| format!("no pipe: {e}"))?;
        let mut bash = Command::new("bash");
        bash.arg("-lc")
            .arg(command)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(writer)
            .stderr(both);
        for name in NOT_GIVEN {
            bash.env_remove(name);
        }
        // Its own process group, so that a command out of time is killed with all it started.
        std::os::unix::process::CommandExt::process_group(&mut bash, 0);
        let mut child = bash
            .spawn()
            .map_err(|e| format!("bash could not be started: {e}"))?;
        // The writing ends were moved into `bash`, and are closed here once it has them: the
        // output ends when the command's last process does.
        drop(bash);
        let (sent, output) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sent.send(kept(reader));
        });
        let deadline = Instant::now() + within;
        let (code, timed_out) = loop {
            if let Ok(Some(status)) = child.try_wait() {
                break (status.code(), false);
            }
            if Instant::now() >= deadline {
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let kept = output
            .recv_timeout(DRAINED_WITHIN)
            .unwrap_or_else(|_| Kept {
                head: "(its output could not be read: a process it started still holds it)"
                    .to_owned(),
                ..Kept::default()
            });
        // The token is hidden in each part on its own, so the note stays where the shell put it.
        let hide = |part: String| {
            if self.hidden.is_empty() {
                part
            } else {
                part.replace(&self.hidden, "[the turn's token]")
            }
        };
        let (head, tail) = (hide(kept.head), hide(kept.tail));
        let (output, marker) = if kept.cut == 0 {
            (format!("{head}{tail}"), None)
        } else {
            let note = format!("\n[… {} bytes cut …]\n", kept.cut);
            let marker = head.len()..head.len() + note.len();
            (format!("{head}{note}{tail}"), Some(marker))
        };
        Ok(Ran {
            code,
            timed_out,
            output,
            printed: kept.printed,
            cut: kept.cut,
            marker,
        })
    }
}

/// Kills a process group, with the system's `kill` (no new dependency for one signal).
fn kill_group(leader: u32) {
    let _ = Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{leader}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// What [`kept`] kept of a command's output: its start and its end, how much it printed and how
/// much was left out between them, in bytes.
#[derive(Debug, Default)]
struct Kept {
    head: String,
    tail: String,
    printed: usize,
    cut: usize,
}

impl Kept {
    /// The two parts with the shell's note between them, as the model reads them.
    #[cfg(test)]
    fn text(&self) -> String {
        if self.cut == 0 {
            return format!("{}{}", self.head, self.tail);
        }
        format!("{}\n[… {} bytes cut …]\n{}", self.head, self.cut, self.tail)
    }
}

/// Reads all of `from`, keeping its first and last halves of [`OUTPUT_AT_MOST`]; the halves end
/// and start on whole UTF-8 characters, and what is between them is counted as cut.
fn kept(mut from: impl std::io::Read) -> Kept {
    let half = OUTPUT_AT_MOST / 2;
    let mut head: Vec<u8> = Vec::new();
    let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
    let mut total = 0usize;
    let mut buffer = [0u8; 8192];
    loop {
        let n = match from.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        total += n;
        for &byte in &buffer[..n] {
            if head.len() < half {
                head.push(byte);
                continue;
            }
            if tail.len() == half {
                tail.pop_front();
            }
            tail.push_back(byte);
        }
    }
    let mut tail: Vec<u8> = tail.into_iter().collect();
    if total <= OUTPUT_AT_MOST {
        head.extend_from_slice(&tail);
        return Kept {
            head: String::from_utf8_lossy(&head).into_owned(),
            tail: String::new(),
            printed: total,
            cut: 0,
        };
    }
    // A character split by either end of the cut goes with the cut, whole.
    let ends_whole = std::str::from_utf8(&head).map_or_else(|e| e.valid_up_to(), |_| head.len());
    if head.len() - ends_whole < 4 {
        head.truncate(ends_whole);
    }
    let starts_whole = tail
        .iter()
        .take(3)
        .take_while(|b| (0x80..0xC0).contains(*b))
        .count();
    tail.drain(..starts_whole);
    let cut = total - head.len() - tail.len();
    Kept {
        head: String::from_utf8_lossy(&head).into_owned(),
        tail: String::from_utf8_lossy(&tail).into_owned(),
        printed: total,
        cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here() -> Shell {
        Shell::new(std::env::temp_dir(), "a-turns-token")
    }

    #[test]
    fn a_command_says_its_output_and_its_exit_code() {
        let ran = here().run("echo out; echo err >&2; exit 3", USUAL).unwrap();
        assert_eq!(ran.code, Some(3));
        assert!(!ran.timed_out);
        assert_eq!(
            ran.output, "out\nerr\n",
            "stdout and stderr together, in order"
        );
        assert!(!ran.ok());
        assert_eq!(ran.said(USUAL), "exit 3\nout\nerr\n");
        let ran = here().run("true", USUAL).unwrap();
        assert!(ran.ok());
        assert_eq!(ran.said(USUAL), "exit 0\n(no output)");
    }

    #[test]
    fn it_runs_in_the_workbench_without_the_turns_token() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let shell = Shell::new(dir.clone(), "a-turns-token");
        let ran = shell
            .run("pwd -P; echo \"[$ENDORA_RUN_TOKEN]\"", USUAL)
            .unwrap();
        assert_eq!(ran.output, format!("{}\n[]\n", dir.display()));
        let ran = shell
            .run("echo proxy=http://turn:a-turns-token@proxy", USUAL)
            .unwrap();
        assert_eq!(ran.output, "proxy=http://turn:[the turn's token]@proxy\n");
    }

    #[test]
    fn a_command_out_of_time_is_stopped_with_all_it_started() {
        let started = Instant::now();
        let ran = here()
            .run(
                "echo before; sleep 30 & sleep 30; echo never",
                Duration::from_secs(3),
            )
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert!(ran.timed_out);
        assert_eq!(ran.code, None);
        assert_eq!(
            ran.output, "before\n",
            "the background sleep was killed too"
        );
        assert_eq!(
            ran.said(Duration::from_secs(3)),
            "timed out after 3 s, and was stopped\nbefore\n"
        );
    }

    #[test]
    fn the_time_is_a_minute_unless_told_and_never_more_than_five() {
        assert_eq!(Shell::within(None), Duration::from_secs(60));
        assert_eq!(Shell::within(Some(5)), Duration::from_secs(5));
        assert_eq!(Shell::within(Some(0)), Duration::from_secs(1));
        assert_eq!(Shell::within(Some(10_000)), Duration::from_secs(300));
    }

    #[test]
    fn a_long_output_keeps_its_head_and_its_tail() {
        let ran = here()
            .run(
                "echo first; head -c 100000 /dev/zero | tr '\\0' x; echo; echo last",
                USUAL,
            )
            .unwrap();
        assert_eq!(ran.code, Some(0));
        assert!(
            ran.output.starts_with("first\nxxx"),
            "{}",
            &ran.output[..20]
        );
        assert!(ran.output.ends_with("xxx\nlast\n"));
        let cut = 6 + 100_000 + 1 + 5 - OUTPUT_AT_MOST;
        assert!(
            ran.output.contains(&format!("\n[… {cut} bytes cut …]\n")),
            "{}",
            ran.output.len()
        );
        assert!(ran.output.len() < OUTPUT_AT_MOST + 100);
        // Exactly at the limit, nothing is cut.
        assert_eq!(
            kept(&vec![b'y'; OUTPUT_AT_MOST][..]).text().len(),
            OUTPUT_AT_MOST
        );
    }

    /// The bytes of `ran.output` that are the command's own, not the shell's note of its cut.
    fn its_own(ran: &Ran) -> usize {
        ran.output.len() - ran.marker.as_ref().map_or(0, std::ops::Range::len)
    }

    #[test]
    fn a_cut_output_says_what_was_printed_what_was_cut_and_where_its_note_is() {
        let ran = here()
            .run(
                "echo first; head -c 100000 /dev/zero | tr '\\0' x; echo; echo last",
                USUAL,
            )
            .unwrap();
        assert_eq!(ran.printed, 6 + 100_000 + 1 + 5);
        assert_eq!(ran.cut, ran.printed - its_own(&ran));
        let marker = ran.marker.clone().expect("the shell's note of its cut");
        assert_eq!(
            &ran.output[marker],
            format!("\n[… {} bytes cut …]\n", ran.cut)
        );
    }

    #[test]
    fn a_cut_keeps_whole_characters_and_counts_bytes() {
        // 30,000 characters of two bytes each: the cut never splits one.
        let ran = here()
            .run("python3 -c \"print('é' * 30000, end='')\"", USUAL)
            .unwrap();
        assert_eq!(ran.printed, 60_000);
        assert!(!ran.output.contains('\u{FFFD}'), "a character was split");
        assert_eq!(ran.cut, ran.printed - its_own(&ran));
    }

    #[test]
    fn output_within_the_limit_is_whole_and_has_no_note() {
        let ran = here().run("echo short", USUAL).unwrap();
        assert_eq!((ran.printed, ran.cut, ran.marker.clone()), (6, 0, None));
    }

    #[test]
    fn a_note_a_command_prints_itself_is_not_the_shells() {
        let ran = here().run("echo '[… 99999 bytes cut …]'", USUAL).unwrap();
        assert_eq!((ran.cut, ran.marker.clone()), (0, None));
    }
}
