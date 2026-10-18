//! The check job (ADR 0115), the first piece of the lab: the workbench run with no model and
//! no turn, given a job token alone. It fetches the submitted branch's bundle from the node's
//! team listener, unpacks it, runs the fork's own checks (the playbook's syntax, ansible-lint,
//! yamllint, each within five minutes) and reports pass or fail, a line and each command's
//! last words to HQ through the gateway. The sandbox stays offline: the bundle comes from the
//! node, and nothing else is reached.

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
        ("yamllint", "yamllint .".to_owned()),
    ]
}

/// Runs each command in the shell, within [`EACH_WITHIN`], and keeps what each came to.
#[must_use]
pub fn run_checks(shell: &Shell, commands: &[(&'static str, String)]) -> Vec<Outcome> {
    commands
        .iter()
        .map(|(name, command)| match shell.run(command, EACH_WITHIN) {
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
        })
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

/// Fetches the bundle with the job's token.
fn fetch(setup: &Setup) -> Result<Vec<u8>, String> {
    let mut response = agent()
        .get(&setup.bundle_url)
        .header("authorization", &format!("Bearer {}", setup.token))
        .call()
        .map_err(|e| format!("the node could not be reached for the bundle: {e}"))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(format!("the node answered {status} for the bundle"));
    }
    response
        .body_mut()
        .with_config()
        .limit(BUNDLE_MOST)
        .read_to_vec()
        .map_err(|e| format!("the bundle could not be read: {e}"))
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
    let bytes = fetch(setup)?;
    let file = setup.dir.join("check.bundle");
    std::fs::write(&file, &bytes).map_err(|e| format!("the bundle could not be written: {e}"))?;
    let repo = unpack(&setup.dir, &file)?;
    let _ = std::fs::remove_file(&file);
    let commands = commands(&repo);
    let outcomes = run_checks(&Shell::new(repo, &setup.token), &commands);
    let (ok, summary) = summarise(&outcomes);
    Ok((ok, summary, report(&outcomes, &commands)))
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
        assert_eq!(with[2].1, "yamllint .");
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
