//! The check job (ADR 0115), the first piece of the lab: the workbench run with no model and
//! no turn, given a job token alone. It fetches the submitted branch's bundle from the node's
//! team listener, unpacks it, runs the fork's own checks (the playbook's syntax, ansible-lint,
//! yamllint, each within five minutes) and reports pass or fail, a line and each command's
//! last words to HQ through the gateway. A branch is judged against its base: when any check
//! fails, the same checks run on the commit the branch was cut from (the node names it with
//! the bundle), and the branch passes if none is worse than it was there. A fork whose own
//! develop already has findings is not failed for them; but a base on which a check could not
//! run (a missing tool, a kill, a timeout) was not read, and excuses nothing. The sandbox
//! stays offline: the bundle comes from the node, and nothing else is reached.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::json;

use crate::shell::Shell;

/// How long each check may run.
pub const EACH_WITHIN: Duration = Duration::from_secs(300);
/// The most of each command's output that goes into the report: its last words.
pub const TAIL_MOST: usize = 4 * 1024;
/// The most a bundle may be.
const BUNDLE_MOST: u64 = 64 * 1024 * 1024;
/// How long the node has to answer.
const ANSWERS_WITHIN: Duration = Duration::from_secs(120);

/// Where the image's lab checks live (the `lab-checks` stage's venv). The shell is a login
/// shell, which rebuilds `PATH` from `/etc/profile` and so drops the image's own `ENV PATH`.
const VENV_BIN: &str = "/opt/venv/bin";

/// What a check job is given.
pub struct Setup {
    /// The check's id, `c<id>`.
    pub id: String,
    /// The job's own token, which opens the bundle and signs the report.
    pub token: String,
    /// Where the bundle is: the node's `/team-relay/check/<id>/bundle`.
    pub bundle_url: String,
    /// Where the report goes: HQ's `/runner/check/<id>/done`, through the gateway.
    pub done_url: String,
    /// The sandbox's writable folder.
    pub dir: PathBuf,
}

/// The setup from the environment (`env` looks a name up).
///
/// # Errors
/// What is missing.
pub fn setup(env: impl Fn(&str) -> Option<String>) -> Result<Setup, String> {
    let given = |name: &str| {
        env(name)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("{name} is not set: a check runs only with it"))
    };
    let id = given("ENDORA_CHECK_ID")?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("ENDORA_CHECK_ID is not a number".to_owned());
    }
    Ok(Setup {
        id,
        token: given("ENDORA_CHECK_TOKEN")?,
        bundle_url: given("ENDORA_CHECK_BUNDLE_URL")?,
        done_url: given("ENDORA_CHECK_DONE_URL")?,
        dir: env("ENDORA_WORKBENCH_DIR")
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| "/work".to_owned())
            .into(),
    })
}

/// What one check came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Which: `syntax`, `ansible-lint` or `yamllint`.
    pub name: &'static str,
    /// Its exit code; `None` when it was killed.
    pub code: Option<i32>,
    /// Whether it outlived its five minutes.
    pub timed_out: bool,
    /// What it printed.
    pub output: String,
}

/// The fork's checks, as its CI runs them, in the repository at `repo`: the playbook's syntax
/// against the integration inventory when the fork has one, ansible-lint on the playbook, and
/// yamllint over everything.
#[must_use]
pub fn commands(repo: &Path) -> Vec<(&'static str, String)> {
    let inventory = repo.join("tests/inventories/integration_testing");
    let syntax = if inventory.exists() {
        "ansible-playbook --syntax-check nas.yml -i tests/inventories/integration_testing"
    } else {
        "ansible-playbook --syntax-check nas.yml"
    };
    vec![
        ("syntax", syntax.to_owned()),
        // Offline: the sandbox cannot fetch requirements.yml; the image bakes them in.
        ("ansible-lint", "ansible-lint --offline nas.yml".to_owned()),
        ("yamllint", YAMLLINT.to_owned()),
    ]
}

/// yamllint over everything, its exit code kept, and its error lines counted in a last line of
/// its own: the shell keeps only the head and tail of a long output, and a count taken from
/// what is left would be the same on a branch and its base.
const YAMLLINT: &str = r#"set -o pipefail; yamllint . | awk '/  error  /{n++} 1; END{printf "yamllint: %d error(s)\n", n}'"#;

/// `command` with the lab venv first on `PATH`, set after the login shell has had its say, so
/// the tools and the ones they run themselves (ansible-lint runs ansible-playbook and yamllint)
/// are found.
fn on_the_venv(command: &str) -> String {
    format!("export PATH={VENV_BIN}:\"$PATH\"; {command}")
}

/// Runs each command in the shell, within [`EACH_WITHIN`], and keeps what each came to.
#[must_use]
pub fn run_checks(shell: &Shell, commands: &[(&'static str, String)]) -> Vec<Outcome> {
    commands
        .iter()
        .map(
            |(name, command)| match shell.run(&on_the_venv(command), EACH_WITHIN) {
                Ok(ran) => Outcome {
                    name,
                    code: ran.code,
                    timed_out: ran.timed_out,
                    output: ran.output,
                },
                Err(why) => Outcome {
                    name,
                    code: None,
                    timed_out: false,
                    output: format!("could not run: {why}"),
                },
            },
        )
        .collect()
}

/// How many findings ansible-lint reported, from its summary line ("Failed after min
/// profile: 2 failure(s), 1 warning(s) on 12 files."), if it said.
fn findings(output: &str) -> Option<u32> {
    output.lines().rev().find_map(|line| {
        let (before, _) = line.split_once(" failure(s)")?;
        before.rsplit(' ').next()?.parse().ok()
    })
}

/// Whether a check says nothing about the code it ran on, because the box could not run it:
/// its tool was not found (127) or not executable (126), it was killed or never started (no
/// exit code), or it ran out of time. A broken box is not a finding, and nothing excuses it.
fn broken(o: &Outcome) -> bool {
    o.timed_out || matches!(o.code, None | Some(126 | 127))
}

/// Pass or fail, and the line HQ keeps: "syntax ok, ansible-lint 2 findings, yamllint ok".
#[must_use]
pub fn summarise(outcomes: &[Outcome]) -> (bool, String) {
    let mut ok = true;
    let parts: Vec<String> = outcomes
        .iter()
        .map(|o| {
            let passed = !o.timed_out && o.code == Some(0);
            ok &= passed;
            let said = match (passed, o.timed_out, o.name) {
                (true, _, _) => "ok".to_owned(),
                (false, true, _) => "timed out".to_owned(),
                (false, false, _) if broken(o) => "could not run".to_owned(),
                (false, false, "ansible-lint") => findings(&o.output).map_or_else(
                    || "failed".to_owned(),
                    |n| format!("{n} finding{}", if n == 1 { "" } else { "s" }),
                ),
                (false, false, _) => "failed".to_owned(),
            };
            format!("{} {said}", o.name)
        })
        .collect();
    (ok, parts.join(", "))
}

/// Whether a check passed: it ended, with exit 0.
fn passed(o: &Outcome) -> bool {
    !o.timed_out && o.code == Some(0)
}

/// How much a failed check found, if it said: ansible-lint's findings, yamllint's errors (from
/// the count the command ends with, else the error lines themselves). A check that passed
/// found nothing. `None` when a count cannot be read, and the exit code stands alone.
fn measure(o: &Outcome) -> Option<u32> {
    if passed(o) {
        return Some(0);
    }
    match o.name {
        "ansible-lint" => findings(&o.output),
        "yamllint" => {
            let counted = o.output.lines().rev().find_map(|line| {
                line.strip_prefix("yamllint: ")?
                    .strip_suffix(" error(s)")?
                    .parse()
                    .ok()
            });
            let lines = || {
                o.output
                    .lines()
                    .filter(|l| {
                        let mut words = l.split_whitespace();
                        words.next().is_some_and(|w| w.contains(':'))
                            && words.next() == Some("error")
                    })
                    .count()
            };
            counted.or_else(|| u32::try_from(lines()).ok().filter(|n| *n > 0))
        }
        _ => None,
    }
}

/// What one failed check says, with its count: "ansible-lint 3 findings", "yamllint 1 error".
fn said(o: &Outcome) -> String {
    if o.timed_out {
        return "timed out".to_owned();
    }
    if broken(o) {
        return "could not run".to_owned();
    }
    let unit = match o.name {
        "ansible-lint" => "finding",
        "yamllint" => "error",
        _ => return "failed".to_owned(),
    };
    measure(o).map_or_else(
        || "failed".to_owned(),
        |n| format!("{n} {unit}{}", if n == 1 { "" } else { "s" }),
    )
}

/// Why develop's outcomes cannot excuse a branch: the first of its checks that said nothing
/// ([`broken`]), in a few words. `None` when every one of them was read.
fn unread(base: &[Outcome]) -> Option<String> {
    let o = base.iter().find(|o| broken(o))?;
    let how = match (o.timed_out, o.code) {
        (true, _) => "timed out".to_owned(),
        (false, Some(127)) => "exited 127 (not found)".to_owned(),
        (false, Some(code)) => format!("exited {code} (not executable)"),
        (false, None) => "could not run or was killed".to_owned(),
    };
    Some(format!("{} {how} on develop", o.name))
}

/// The line for a failure that develop cannot excuse, and why.
fn unchecked(plain: &str, why: &str) -> String {
    format!("{plain} (develop could not be checked: {why})")
}

/// Pass or fail against the branch's base ("no worse than develop"), and the line HQ keeps.
/// A branch with no failure passes as it is, in [`summarise`]'s words, without the base being
/// asked for. Otherwise each check is held to what the base did: its count no higher, or,
/// where no count can be read, a failure only where the base passed. A check that could not run
/// on the branch ([`broken`]: a timeout, a missing tool) always fails, as does any failure when
/// a check of the base could not run: a base that was not read excuses nothing, so a box
/// without its tools, broken on both sides, passes no branch. `base` is the base's outcomes, or
/// why it could not be checked, in which case the failure stands as it is and the line says so.
#[must_use]
pub fn judge(branch: &[Outcome], base: Result<&[Outcome], String>) -> (bool, String) {
    let (clean, plain) = summarise(branch);
    if clean {
        return (true, plain);
    }
    let base = match base {
        Ok(base) => base,
        Err(why) => return (false, unchecked(&plain, &why)),
    };
    if let Some(why) = unread(base) {
        return (false, unchecked(&plain, &why));
    }
    let mut worse: Vec<&str> = Vec::new();
    let parts: Vec<String> = branch
        .iter()
        .map(|o| {
            if passed(o) {
                return format!("{} ok", o.name);
            }
            let theirs = base.iter().find(|b| b.name == o.name);
            let no_worse = !broken(o)
                && theirs.is_some_and(|b| match (measure(o), measure(b)) {
                    (Some(mine), Some(theirs)) => mine <= theirs,
                    _ => !passed(b),
                });
            if !no_worse {
                worse.push(o.name);
            }
            let mine = said(o);
            match theirs {
                Some(b) if passed(b) => format!("{} {mine} (develop ok)", o.name),
                // The base's count alone: the unit was said just before it.
                Some(b) => match measure(b).filter(|_| !b.timed_out) {
                    Some(n) => format!("{} {mine} (develop {n})", o.name),
                    None => format!("{} {mine} (develop {})", o.name, said(b)),
                },
                None => format!("{} {mine}", o.name),
            }
        })
        .collect();
    let line = parts.join(", ");
    if worse.is_empty() {
        return (true, format!("{line}: no worse than develop"));
    }
    (
        false,
        format!("{line}: worse than develop on {}", worse.join(", ")),
    )
}

/// The report HQ keeps as a document: each check's command, how it ended and its last words.
#[must_use]
pub fn report(outcomes: &[Outcome], commands: &[(&'static str, String)]) -> String {
    outcomes
        .iter()
        .map(|o| {
            let command = commands
                .iter()
                .find(|(n, _)| *n == o.name)
                .map_or("", |(_, c)| c.as_str());
            let ended = match (o.timed_out, o.code) {
                (true, _) => format!("timed out after {} s", EACH_WITHIN.as_secs()),
                (false, Some(code)) => format!("exit {code}"),
                (false, None) => "killed".to_owned(),
            };
            let tail = tail(&o.output);
            format!("## {} ({ended})\n$ {command}\n{tail}\n", o.name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The last [`TAIL_MOST`] bytes of an output, on a character's edge.
fn tail(output: &str) -> &str {
    if output.len() <= TAIL_MOST {
        return output;
    }
    let mut from = output.len() - TAIL_MOST;
    while !output.is_char_boundary(from) {
        from += 1;
    }
    &output[from..]
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .map_err(|e| format!("git could not be run: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or(""),
            said.lines().last().unwrap_or("")
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The report with the base's outcomes after the branch's, when the base was checked.
#[must_use]
pub fn full_report(
    outcomes: &[Outcome],
    commands: &[(&'static str, String)],
    base: Option<(&str, &[Outcome])>,
) -> String {
    let ours = report(outcomes, commands);
    let Some((sha, theirs)) = base else {
        return ours;
    };
    let short = &sha[..sha.len().min(12)];
    format!(
        "{ours}\n# develop at {short}\n\n{}",
        report(theirs, commands)
    )
}

/// Checks out the branch's base, the commit it was cut from, beside `repo` as `dir/base`. The
/// commit is in the unpacked history: the node bundles the whole of it behind the branch.
///
/// # Errors
/// No base given, one that is not a commit hash, or not in the history.
pub fn checkout_base(dir: &Path, repo: &Path, base: Option<&str>) -> Result<PathBuf, String> {
    let sha = base.map(str::trim).unwrap_or("");
    if sha.is_empty() {
        return Err("no base was given".to_owned());
    }
    if sha.len() < 7 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the base is not a commit hash".to_owned());
    }
    git(repo, &["cat-file", "-e", &format!("{sha}^{{commit}}")])
        .map_err(|_| "the base is not in the bundle's history".to_owned())?;
    let there = dir.join("base");
    if there.exists() {
        std::fs::remove_dir_all(&there).map_err(|e| format!("the old base stays: {e}"))?;
    }
    git(repo, &["worktree", "prune"])?;
    git(
        repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &there.to_string_lossy(),
            sha,
        ],
    )?;
    Ok(there)
}

/// Unpacks the bundle at `file` (its one ref, whatever it is called) into `dir/repo`, checked
/// out as `check`.
///
/// # Errors
/// What git said.
pub fn unpack(dir: &Path, file: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("no {}: {e}", dir.display()))?;
    let repo = dir.join("repo");
    if repo.exists() {
        std::fs::remove_dir_all(&repo).map_err(|e| format!("the old repo stays: {e}"))?;
    }
    git(dir, &["init", "-q", "repo"])?;
    let heads = git(dir, &["bundle", "list-heads", &file.to_string_lossy()])?;
    let reference = heads
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .find(|r| *r != "HEAD")
        .ok_or_else(|| "the bundle names no branch".to_owned())?
        .to_owned();
    git(
        &repo,
        &[
            "fetch",
            "-q",
            &file.to_string_lossy(),
            &format!("{reference}:refs/heads/check"),
        ],
    )?;
    git(&repo, &["checkout", "-q", "check"])?;
    Ok(repo)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(ANSWERS_WITHIN))
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .build()
        .into()
}

/// Fetches the bundle with the job's token, and the base the node says it sits on.
fn fetch(setup: &Setup) -> Result<(Vec<u8>, Option<String>), String> {
    let mut response = agent()
        .get(&setup.bundle_url)
        .header("authorization", &format!("Bearer {}", setup.token))
        .call()
        .map_err(|e| format!("the node could not be reached for the bundle: {e}"))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(format!("the node answered {status} for the bundle"));
    }
    let base = response
        .headers()
        .get("x-endora-base")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = response
        .body_mut()
        .with_config()
        .limit(BUNDLE_MOST)
        .read_to_vec()
        .map_err(|e| format!("the bundle could not be read: {e}"))?;
    Ok((bytes, base))
}

/// Tells HQ, with the job's token.
fn tell(setup: &Setup, ok: bool, summary: &str, report: &str) -> Result<(), String> {
    let response = agent()
        .post(&setup.done_url)
        .header("authorization", &format!("Bearer {}", setup.token))
        .send_json(json!({ "ok": ok, "summary": summary, "report": report }))
        .map_err(|e| format!("HQ could not be reached: {e}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HQ answered {status}"));
    }
    Ok(())
}

/// The whole job: fetch, unpack, check, report. Returns the exit code: 0 once HQ has the
/// report (pass or fail), 1 when it could not be told.
#[must_use]
pub fn run(setup: &Setup) -> i32 {
    let (ok, summary, text) = match checked(setup) {
        Ok((ok, summary, text)) => (ok, summary, text),
        Err(why) => (false, format!("the check could not run: {why}"), why),
    };
    match tell(setup, ok, &summary, &text) {
        Ok(()) => {
            println!("check c{}: {summary}", setup.id);
            0
        }
        Err(why) => {
            eprintln!("check c{}: {summary}; not told: {why}", setup.id);
            1
        }
    }
}

fn checked(setup: &Setup) -> Result<(bool, String, String), String> {
    std::fs::create_dir_all(&setup.dir).map_err(|e| format!("no {}: {e}", setup.dir.display()))?;
    let (bytes, base) = fetch(setup)?;
    let file = setup.dir.join("check.bundle");
    std::fs::write(&file, &bytes).map_err(|e| format!("the bundle could not be written: {e}"))?;
    let repo = unpack(&setup.dir, &file)?;
    let _ = std::fs::remove_file(&file);
    let commands = commands(&repo);
    let outcomes = run_checks(&Shell::new(repo.clone(), &setup.token), &commands);
    // Judged against the base only when the branch has a failure to excuse.
    if outcomes.iter().all(passed) {
        let (ok, summary) = summarise(&outcomes);
        return Ok((ok, summary, report(&outcomes, &commands)));
    }
    let base_outcomes = checkout_base(&setup.dir, &repo, base.as_deref()).map(|there| {
        let commands = self::commands(&there);
        run_checks(&Shell::new(there, &setup.token), &commands)
    });
    let (ok, summary) = judge(&outcomes, base_outcomes.as_deref().map_err(Clone::clone));
    let sha = base.as_deref().unwrap_or("");
    let text = full_report(
        &outcomes,
        &commands,
        base_outcomes.as_ref().ok().map(|b| (sha, b.as_slice())),
    );
    Ok((ok, summary, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn a_check_needs_its_id_token_and_both_addresses() {
        let all = [
            ("ENDORA_CHECK_ID", "9"),
            ("ENDORA_CHECK_TOKEN", "a-jobs-token"),
            (
                "ENDORA_CHECK_BUNDLE_URL",
                "http://192.168.1.10:8788/team-relay/check/9/bundle",
            ),
            (
                "ENDORA_CHECK_DONE_URL",
                "http://192.168.1.10:8788/team-gateway/runner/check/9/done",
            ),
        ];
        let s = setup(env(&all)).unwrap();
        assert_eq!((s.id.as_str(), s.dir.to_str()), ("9", Some("/work")));
        for missing in 0..all.len() {
            let fewer: Vec<(&str, &str)> = all
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != missing)
                .map(|(_, p)| *p)
                .collect();
            assert!(setup(env(&fewer)).is_err(), "{missing}");
        }
        let mut odd = all.to_vec();
        odd[0] = ("ENDORA_CHECK_ID", "nine");
        assert!(setup(env(&odd)).is_err());
    }

    #[test]
    fn the_summary_says_each_check_in_a_word_and_counts_lints_findings() {
        let shell = Shell::new(std::env::temp_dir(), "a-jobs-token");
        let commands = vec![
            ("syntax", "echo playbook: nas.yml".to_owned()),
            (
                "ansible-lint",
                "echo 'Failed after min profile: 2 failure(s), 1 warning(s) on 12 files.' >&2; exit 2".to_owned(),
            ),
            ("yamllint", "true".to_owned()),
        ];
        let outcomes = run_checks(&shell, &commands);
        assert_eq!(outcomes.len(), 3);
        assert_eq!(
            (outcomes[0].code, outcomes[1].code, outcomes[2].code),
            (Some(0), Some(2), Some(0))
        );
        let (ok, summary) = summarise(&outcomes);
        assert!(!ok);
        assert_eq!(summary, "syntax ok, ansible-lint 2 findings, yamllint ok");
        let text = report(&outcomes, &commands);
        assert!(
            text.starts_with("## syntax (exit 0)\n$ echo playbook: nas.yml\nplaybook: nas.yml\n"),
            "{text}"
        );
        assert!(text.contains("## ansible-lint (exit 2)\n$ echo"), "{text}");
        let all_well: Vec<Outcome> = outcomes
            .iter()
            .map(|o| Outcome {
                code: Some(0),
                ..o.clone()
            })
            .collect();
        assert_eq!(
            summarise(&all_well),
            (true, "syntax ok, ansible-lint ok, yamllint ok".to_owned())
        );
        let failed = vec![Outcome {
            name: "ansible-lint",
            code: Some(1),
            timed_out: false,
            output: "boom".to_owned(),
        }];
        assert_eq!(summarise(&failed).1, "ansible-lint failed");
        let late = vec![Outcome {
            name: "yamllint",
            code: None,
            timed_out: true,
            output: String::new(),
        }];
        assert_eq!(summarise(&late).1, "yamllint timed out");
        assert!(report(&late, &[]).contains("timed out after 300 s"));
        assert_eq!(
            findings("Failed after min profile: 1 failure(s), 0 warning(s) on 3 files."),
            Some(1)
        );
        assert_eq!(findings("nothing here"), None);
        let long = "x".repeat(TAIL_MOST * 2);
        assert_eq!(tail(&long).len(), TAIL_MOST);
    }

    fn out(name: &'static str, code: i32, output: &str) -> Outcome {
        Outcome {
            name,
            code: Some(code),
            timed_out: false,
            output: output.to_owned(),
        }
    }

    fn lint(n: u32) -> Outcome {
        let said = format!("Failed after min profile: {n} failure(s), 0 warning(s) on 9 files.");
        if n == 0 {
            return out("ansible-lint", 0, "");
        }
        out("ansible-lint", 2, &said)
    }

    fn yaml(n: u32) -> Outcome {
        if n == 0 {
            return out("yamllint", 0, "");
        }
        out(
            "yamllint",
            1,
            &format!(
                "./a.yml\n  1:1       warning  missing document start \"---\"  (document-start)\n  3:5       error    wrong indentation  (indentation)\nyamllint: {n} error(s)\n"
            ),
        )
    }

    fn ran(syntax: i32, l: u32, y: u32) -> Vec<Outcome> {
        vec![out("syntax", syntax, ""), lint(l), yaml(y)]
    }

    #[test]
    fn a_branch_no_worse_than_its_base_passes_and_says_both() {
        let (ok, said) = judge(&ran(0, 25, 14), Ok(&ran(0, 25, 14)));
        assert!(ok, "{said}");
        assert_eq!(
            said,
            "syntax ok, ansible-lint 25 findings (develop 25), yamllint 14 errors (develop 14): no worse than develop"
        );
        // Fewer is better, and a base that was worse on syntax is tolerated.
        let (ok, said) = judge(&ran(1, 24, 0), Ok(&ran(1, 25, 14)));
        assert!(ok, "{said}");
        assert!(said.contains("syntax failed (develop failed)"), "{said}");
        assert!(said.ends_with(": no worse than develop"), "{said}");
    }

    #[test]
    fn a_branch_worse_than_its_base_fails_and_says_where() {
        let (ok, said) = judge(&ran(0, 26, 14), Ok(&ran(0, 25, 14)));
        assert!(!ok);
        assert!(
            said.ends_with(": worse than develop on ansible-lint"),
            "{said}"
        );
        let (ok, said) = judge(&ran(1, 0, 15), Ok(&ran(0, 0, 14)));
        assert!(!ok);
        assert!(
            said.ends_with(": worse than develop on syntax, yamllint"),
            "{said}"
        );
        assert!(said.contains("yamllint 15 errors (develop 14)"), "{said}");
        // A clean base makes any failure worse, even one whose count cannot be read.
        let mut bare = ran(0, 0, 0);
        bare[1] = out("ansible-lint", 1, "boom");
        let (ok, said) = judge(&bare, Ok(&ran(0, 0, 0)));
        assert!(!ok);
        assert!(said.contains("ansible-lint failed (develop ok)"), "{said}");
    }

    #[test]
    fn a_timeout_on_the_branch_always_fails() {
        let mut branch = ran(0, 25, 14);
        branch[2] = Outcome {
            name: "yamllint",
            code: None,
            timed_out: true,
            output: String::new(),
        };
        // Develop's yamllint was read (14 errors) and the branch's ran out of time: worse.
        let (ok, said) = judge(&branch, Ok(&ran(0, 25, 14)));
        assert!(!ok);
        assert!(said.contains("yamllint timed out"), "{said}");
        assert!(said.ends_with("worse than develop on yamllint"), "{said}");
        // Out of time on develop too: develop was not read, so it excuses nothing, and the line
        // says that rather than that the branch is worse than a develop nobody saw.
        let mut base = ran(0, 25, 14);
        base[2] = branch[2].clone();
        let (ok, said) = judge(&branch, Ok(&base));
        assert!(!ok);
        assert!(
            said.contains("yamllint timed out")
                && said.contains("develop could not be checked: yamllint timed out on develop"),
            "{said}"
        );
    }

    /// A check whose tool was not there, as the shell reports it.
    fn gone(name: &'static str, code: i32) -> Outcome {
        out(name, code, &format!("bash: {name}: command not found"))
    }

    /// Live (checks c1 and c2): the box lacked its tools, so every check exited 127 on the
    /// branch and on develop. With no count to compare, "develop failed too" read as "no
    /// worse", and a box with no tools would have passed every branch it was given.
    #[test]
    fn a_check_box_without_its_tools_fails_on_both_and_passes_nothing() {
        let all_127 = || {
            vec![
                gone("syntax", 127),
                gone("ansible-lint", 127),
                gone("yamllint", 127),
            ]
        };
        let (ok, said) = judge(&all_127(), Ok(&all_127()));
        assert!(!ok, "{said}");
        assert!(
            said.contains("develop could not be checked") && said.contains("exited 127"),
            "{said}"
        );
        assert!(!said.contains("no worse than develop"), "{said}");
        // Not executable (126), killed or never started (no code), or timed out: the same. One
        // broken check on develop is enough, even where the branch's own failure is a finding.
        let timed_out = Outcome {
            name: "yamllint",
            code: None,
            timed_out: true,
            output: String::new(),
        };
        let killed = Outcome {
            name: "ansible-lint",
            code: None,
            timed_out: false,
            output: "could not run: no shell".to_owned(),
        };
        for broken_base in [
            ran_with(gone("ansible-lint", 126)),
            ran_with(timed_out),
            ran_with(killed),
            ran_with(gone("syntax", 127)),
        ] {
            let (ok, said) = judge(&ran(0, 25, 14), Ok(&broken_base));
            assert!(!ok, "a base that was not read excuses nothing: {said}");
            // The branch's own findings are still said, as they were found.
            assert!(
                said.starts_with("syntax ok, ansible-lint 25 findings, yamllint failed (develop could not be checked: "),
                "{said}"
            );
        }
        // A base read properly still excuses what it also had (nothing changed there).
        assert!(judge(&ran(0, 25, 14), Ok(&ran(0, 25, 14))).0);
    }

    #[test]
    fn a_branch_check_that_could_not_run_is_no_finding_for_a_failing_base_to_excuse() {
        // The branch's ansible-lint never ran (127), while develop's ran and found 25: with no
        // count of its own, the branch used to be "no worse" because develop also failed.
        let mut branch = ran(0, 0, 0);
        branch[1] = gone("ansible-lint", 127);
        let (ok, said) = judge(&branch, Ok(&ran(0, 25, 0)));
        assert!(!ok, "{said}");
        assert!(
            said.contains("ansible-lint could not run") && said.ends_with("on ansible-lint"),
            "{said}"
        );
        // Killed with no code: the same.
        branch[1] = Outcome {
            name: "ansible-lint",
            code: None,
            timed_out: false,
            output: String::new(),
        };
        assert!(!judge(&branch, Ok(&ran(0, 25, 0))).0);
        // And in the plain line of a branch that is checked on its own.
        assert_eq!(
            summarise(&[gone("syntax", 127), lint(2), yaml(0)]).1,
            "syntax could not run, ansible-lint 2 findings, yamllint ok"
        );
    }

    /// Develop as it stands in the other tests (25 findings, 14 errors), with the check of the
    /// same name as `with` replaced by it.
    fn ran_with(with: Outcome) -> Vec<Outcome> {
        let mut all = ran(0, 25, 14);
        let at = all.iter().position(|o| o.name == with.name).unwrap();
        all[at] = with;
        all
    }

    #[test]
    fn a_clean_branch_keeps_the_plain_summary_and_costs_no_base() {
        let clean = ran(0, 0, 0);
        assert_eq!(
            judge(&clean, Err("never looked".to_owned())),
            (true, "syntax ok, ansible-lint ok, yamllint ok".to_owned())
        );
    }

    #[test]
    fn without_a_base_the_failure_stands_and_the_summary_says_why() {
        let (ok, said) = judge(&ran(0, 2, 0), Err("no base was given".to_owned()));
        assert!(!ok);
        assert_eq!(
            said,
            "syntax ok, ansible-lint 2 findings, yamllint ok (develop could not be checked: no base was given)"
        );
    }

    #[test]
    fn yamllints_errors_are_counted_from_its_own_line_or_its_output() {
        let counted = out(
            "yamllint",
            1,
            "  3:5  error  x  (y)\nyamllint: 7 error(s)\n",
        );
        assert_eq!(measure(&counted), Some(7));
        let plain = out(
            "yamllint",
            1,
            "./a.yml\n  1:1       warning  w  (a)\n  3:5       error    e  (b)\n  4:5       error    e  (b)\n",
        );
        assert_eq!(measure(&plain), Some(2));
        assert_eq!(measure(&out("yamllint", 1, "")), None);
        assert_eq!(measure(&out("syntax", 1, "")), None);
    }

    #[test]
    fn the_bases_outcomes_ride_in_the_report() {
        let commands = vec![("syntax", "echo s".to_owned())];
        let text = full_report(
            &[out("syntax", 1, "mine")],
            &commands,
            Some(("0123456789abcdef", &[out("syntax", 0, "theirs")])),
        );
        assert!(
            text.starts_with("## syntax (exit 1)\n$ echo s\nmine\n"),
            "{text}"
        );
        assert!(
            text.contains("\n# develop at 0123456789ab\n\n## syntax (exit 0)\n$ echo s\ntheirs\n"),
            "{text}"
        );
        assert_eq!(
            full_report(&[out("syntax", 1, "mine")], &commands, None),
            report(&[out("syntax", 1, "mine")], &commands)
        );
    }

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "endora-check-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn commit(repo: &Path, file: &str, message: &str) -> String {
        std::fs::write(repo.join(file), "---\n").unwrap();
        git(repo, &["add", "."]).unwrap();
        git(
            repo,
            &[
                "-c",
                "user.name=John Doe",
                "-c",
                "user.email=john@example.com",
                "commit",
                "-q",
                "-m",
                message,
            ],
        )
        .unwrap();
        git(repo, &["rev-parse", "HEAD"]).unwrap()
    }

    #[test]
    fn the_base_is_found_in_the_unpacked_history_and_checked_out_beside_the_branch() {
        let root = scratch("base");
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "-q", "-b", "develop"]).unwrap();
        let base = commit(&seed, "nas.yml", "base");
        git(&seed, &["checkout", "-q", "-b", "hq/task-1-x"]).unwrap();
        commit(&seed, "added.md", "branch");
        // As the node bundles it: the branch's ref alone, whole history behind it.
        git(
            &seed,
            &["update-ref", "refs/submissions/hq/task-1-x", "HEAD"],
        )
        .unwrap();
        let file = root.join("whole.bundle");
        git(
            &seed,
            &[
                "bundle",
                "create",
                "-q",
                file.to_str().unwrap(),
                "refs/submissions/hq/task-1-x",
            ],
        )
        .unwrap();
        let work = root.join("work");
        let repo = unpack(&work, &file).unwrap();
        let there = checkout_base(&work, &repo, Some(&base)).unwrap();
        assert!(there.join("nas.yml").exists());
        assert!(!there.join("added.md").exists(), "the base, not the branch");
        assert!(repo.join("added.md").exists());
        // The same checks run on both, and a second job's leftovers do not stand in the way.
        let shell = Shell::new(there.clone(), "a-jobs-token");
        let ran = run_checks(&shell, &[("syntax", "test -f nas.yml".to_owned())]);
        assert_eq!(ran[0].code, Some(0));
        assert!(checkout_base(&work, &repo, Some(&base)).is_ok());
        // Nothing to find: no base given, an odd one, or one the history lacks.
        assert!(checkout_base(&work, &repo, None).is_err());
        assert!(checkout_base(&work, &repo, Some("")).is_err());
        assert!(checkout_base(&work, &repo, Some("--help")).is_err());
        assert!(checkout_base(&work, &repo, Some(&"0".repeat(40))).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_checks_run_with_the_lab_venv_first_on_the_path_whatever_a_login_shell_resets() {
        // The shell is `bash -lc`, and a login shell rebuilds PATH from /etc/profile, dropping
        // the image's /opt/venv/bin: the checks exited 127 until it was put back in front.
        let shell = Shell::new(std::env::temp_dir(), "a-jobs-token");
        let commands = vec![("path", "echo \"$PATH\"".to_owned())];
        let outcomes = run_checks(&shell, &commands);
        let first = outcomes[0]
            .output
            .trim()
            .split(':')
            .next()
            .unwrap()
            .to_owned();
        assert_eq!(first, VENV_BIN, "{}", outcomes[0].output);
        // The report still shows the command as the fork's CI runs it.
        assert!(report(&outcomes, &commands).contains("$ echo \"$PATH\"\n"));
    }

    #[test]
    fn the_forks_commands_use_its_integration_inventory_when_it_has_one() {
        let root = std::env::temp_dir().join(format!("endora-check-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tests/inventories/integration_testing")).unwrap();
        let with = commands(&root);
        assert_eq!(
            with[0].1,
            "ansible-playbook --syntax-check nas.yml -i tests/inventories/integration_testing"
        );
        assert_eq!(with[1].1, "ansible-lint --offline nas.yml");
        // yamllint's own exit code stays the check's, and its errors are counted at the end,
        // where the shell's head-and-tail trimming of a long output cannot lose them.
        assert!(
            with[2].1.starts_with("set -o pipefail; yamllint . | "),
            "{}",
            with[2].1
        );
        assert!(with[2].1.contains("error(s)"), "{}", with[2].1);
        let without = commands(&root.join("elsewhere"));
        assert_eq!(without[0].1, "ansible-playbook --syntax-check nas.yml");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bundle_of_one_branch_however_named_is_unpacked_as_check() {
        let root = std::env::temp_dir().join(format!(
            "endora-check-unpack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("seed")).unwrap();
        let seed = root.join("seed");
        git(&seed, &["init", "-q", "-b", "main"]).unwrap();
        std::fs::write(seed.join("nas.yml"), "---\n").unwrap();
        git(&seed, &["add", "."]).unwrap();
        git(
            &seed,
            &[
                "-c",
                "user.name=John Doe",
                "-c",
                "user.email=john@example.com",
                "commit",
                "-q",
                "-m",
                "x",
            ],
        )
        .unwrap();
        // As the node bundles a submission: under refs/submissions/, not refs/heads/.
        git(
            &seed,
            &["update-ref", "refs/submissions/hq/task-3-x", "HEAD"],
        )
        .unwrap();
        let file = root.join("whole.bundle");
        git(
            &seed,
            &[
                "bundle",
                "create",
                "-q",
                file.to_str().unwrap(),
                "refs/submissions/hq/task-3-x",
            ],
        )
        .unwrap();
        let repo = unpack(&root.join("work"), &file).unwrap();
        assert_eq!(
            git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
            "check"
        );
        assert!(repo.join("nas.yml").exists());
        assert!(unpack(&root.join("work"), &root.join("missing.bundle")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
