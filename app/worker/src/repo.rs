//! The wing's git in the sandbox (ADR 0115): the fork, handed over by the node as a bundle,
//! unpacked into `/work/repo` with git set up the team's way (commits authored Endora HQ, the
//! trailer added by a hook so no model forgets it, no remote at all); and the member's branch
//! bundled back for the node to take. The model sees neither bundle: it reads where the
//! checkout is and what the node said of the submission.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where the fork is unpacked, under the workbench's `/work`.
pub const REPO_DIR: &str = "repo";
/// Where the team's branches live.
pub const BRANCH_PREFIX: &str = "hq/";
/// Who authors the team's commits.
pub const AUTHOR_NAME: &str = "Endora HQ";
/// The address they are authored with: a reserved domain, never a mailbox.
pub const AUTHOR_EMAIL: &str = "hq@endora.invalid";
/// The trailer every commit carries.
pub const TRAILER: &str = "Made-by: Endora HQ";
/// The most a submitted bundle may be.
pub const BUNDLE_MOST: usize = 8 * 1024 * 1024;

/// What a checkout came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOut {
    /// Where the repository is.
    pub path: PathBuf,
    /// The branch checked out.
    pub branch: String,
    /// Its head.
    pub head: String,
    /// How many files it holds.
    pub files: usize,
    /// Upstream ansible-nas as the sandbox names it (`upstream/master`), when the node's
    /// bundle carried it: read-only, for comparing with the fork offline.
    pub upstream: Option<String>,
}

/// One `git` run: its stdout, or its stderr's last line as the error.
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
        let said: String = said
            .lines()
            .last()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect();
        return Err(format!(
            "git {} failed: {said}",
            args.first().copied().unwrap_or("")
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// `git` with `input` on its stdin.
fn git_stdin(dir: &Path, args: &[&str], input: &str) -> Result<String, String> {
    use std::io::Write as _;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .spawn()
        .map_err(|e| format!("git could not be run: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("git has no stdin")?
        .write_all(input.as_bytes())
        .map_err(|e| format!("git could not be fed: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("git did not finish: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed",
            args.first().copied().unwrap_or("")
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Whether `branch` is a plain name under `hq/`, safe to hand to git as a ref: nothing that
/// climbs (`..`), nothing that git would read as an option, only the characters a slug has.
fn is_team_branch(branch: &str) -> bool {
    branch.starts_with(BRANCH_PREFIX) && branch.len() > BRANCH_PREFIX.len() && is_plain(branch)
}

/// A word in a shell single-quoted string.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// Empties `dir` of everything in it and keeps the folder itself, so a shell standing in it
/// (Claude Code's, after a second checkout) still stands in a folder that exists. A link is
/// removed, never followed.
fn emptied(dir: &Path) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let gone = if entry.file_type().is_ok_and(|t| t.is_dir()) {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        gone.map_err(|e| format!("the old checkout stays: {e}"))?;
    }
    Ok(())
}

/// Unpacks `bundle` into `work/repo`, fresh, and sets git up the team's way: the author, no
/// remote, and a `prepare-commit-msg` hook that adds the trailer naming `member` and `task`
/// when a commit lacks it. `base`, the branch the wing works from, is kept beside it, so a
/// submission is bundled from there.
///
/// The bundle is either the one `branch` with its history, cloned as it is, or, when `kept`
/// (the team's own submission, handed back by the node), the `base` and the stored
/// submission as `refs/submissions/<branch>`: the base is cloned, and the submission fetched
/// as a head of its own and checked out, with `base` still a head behind it, so what is made of
/// it can be submitted again. The folder is emptied, not replaced.
///
/// # Errors
/// What git said, in a line.
pub fn checkout(
    work: &Path,
    bundle: &[u8],
    branch: &str,
    base: &str,
    kept: bool,
    member: &str,
    task: Option<i64>,
) -> Result<CheckedOut, String> {
    std::fs::create_dir_all(work).map_err(|e| format!("no {}: {e}", work.display()))?;
    if kept && !is_team_branch(branch) {
        return Err(format!("{branch:?} is not one of the team's branches"));
    }
    let file = work.join("checkout.bundle");
    std::fs::write(&file, bundle).map_err(|e| format!("the bundle could not be written: {e}"))?;
    let repo = work.join(REPO_DIR);
    emptied(&repo)?;
    let start = if kept { base } else { branch };
    git(
        work,
        &[
            "clone",
            "-q",
            "-b",
            start,
            &file.to_string_lossy(),
            REPO_DIR,
        ],
    )?;
    if kept {
        git(
            &repo,
            &[
                "fetch",
                "-q",
                &file.to_string_lossy(),
                &format!("refs/submissions/{branch}:refs/heads/{branch}"),
            ],
        )
        .map_err(|_| format!("the bundle holds no kept {branch}"))?;
        git(&repo, &["checkout", "-q", branch])?;
    }
    let upstream = unpack_upstream(&repo, &file)?;
    // No remote: there is nowhere to push from the sandbox, and nothing to push with.
    git(&repo, &["remote", "remove", "origin"])?;
    git(&repo, &["config", "user.name", AUTHOR_NAME])?;
    git(&repo, &["config", "user.email", AUTHOR_EMAIL])?;
    git(&repo, &["config", "commit.gpgsign", "false"])?;
    git(&repo, &["config", "core.hooksPath", ".git/hooks"])?;
    // Python's byproducts are ignored here, in the clone alone, so the model's own `git add -A`
    // never commits them (set 4's #1059); a file the project tracks is unaffected.
    std::fs::write(
        repo.join(".git").join("info").join("exclude"),
        "__pycache__/\n*.pyc\n",
    )
    .map_err(|e| format!("the clone's ignore list could not be written: {e}"))?;
    noted(work, branch, base, member, task)?;
    let head = git(&repo, &["rev-parse", "HEAD"])?;
    let files = git(&repo, &["ls-files"])?.lines().count();
    let _ = std::fs::remove_file(&file);
    Ok(CheckedOut {
        path: repo,
        branch: branch.to_owned(),
        head,
        files,
        upstream,
    })
}

/// Notes the checkout (`checkout.json`: the branch handed over, its base, and the trailer) and
/// writes the commit hook that stamps that trailer, naming `member` and `task`: at checkout, and
/// again when a later turn on the item, perhaps another member's, finds the checkout kept.
fn noted(
    work: &Path,
    branch: &str,
    base: &str,
    member: &str,
    task: Option<i64>,
) -> Result<(), String> {
    let repo = work.join(REPO_DIR);
    let for_task = task.map_or_else(|| "for the wing".to_owned(), |t| format!("for task #{t}"));
    let trailer = format!("{TRAILER} ({}) {for_task}", member.trim());
    let hook = format!(
        "#!/bin/sh\n# Endora HQ (ADR 0115): every commit carries its trailer.\nf=\"$1\"\ngrep -q '^{TRAILER}' \"$f\" || printf '\\n%s\\n' {} >> \"$f\"\n",
        quoted(&trailer)
    );
    let hooks = repo.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).map_err(|e| format!("no hooks folder: {e}"))?;
    let path = hooks.join("prepare-commit-msg");
    std::fs::write(&path, hook).map_err(|e| format!("the hook could not be written: {e}"))?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("the hook could not be made runnable: {e}"))?;
    }
    std::fs::write(
        work.join("checkout.json"),
        serde_json::json!({ "branch": branch, "base": base, "trailer": trailer }).to_string(),
    )
    .map_err(|e| format!("the checkout could not be noted: {e}"))
}

/// A checkout an earlier turn on the item left in the sandbox, which keeps it between turns
/// (ADR 0142, amended 2026-11-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlreadyThere {
    /// Where it is.
    pub path: PathBuf,
    /// The branch it is on (`HEAD` when on none).
    pub on: String,
    /// Its head.
    pub head: String,
    /// How many paths `git status` shows changed and not committed.
    pub uncommitted: usize,
}

/// The checkout already in `work/repo` that serves a `repo_checkout` of `asked` (the fork's
/// default branch when `None`): any checkout for no branch named, else one that has `asked` as
/// a branch of its own. `None` when there is no checkout, or it lacks that branch.
#[must_use]
pub fn already_there(work: &Path, asked: Option<&str>) -> Option<AlreadyThere> {
    let repo = work.join(REPO_DIR);
    if !repo.join(".git").exists() {
        return None;
    }
    if let Some(branch) = asked {
        if !is_plain(branch) {
            return None;
        }
        git(
            &repo,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/heads/{branch}^{{commit}}"),
            ],
        )
        .ok()?;
    }
    let on = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    let head = git(&repo, &["rev-parse", "HEAD"]).ok()?;
    let uncommitted = git(&repo, &["status", "--porcelain"])
        .ok()?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    Some(AlreadyThere {
        path: repo,
        on,
        head,
        uncommitted,
    })
}

/// What the sandbox holds of `task`'s work, for a closing call (correction-placement review):
/// every `hq/task-<task>-*` branch in `work/repo` with its full head, read with git, never from
/// what the checkout noted (which the model can write). No checkout is a reading of no branches;
/// a checkout git cannot enumerate is unknown, never an empty list.
#[must_use]
pub fn sandbox_reading(work: &Path, task: i64) -> serde_json::Value {
    let repo = work.join(REPO_DIR);
    // Only a checkout known to be absent is no work; one that cannot be looked at is unknown.
    match std::fs::symlink_metadata(&repo) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return serde_json::json!({ "state": "read", "branches": [] });
        }
        Err(e) => {
            let why = format!("the checkout could not be looked at: {e}");
            return serde_json::json!({ "state": "unknown", "why": why });
        }
    }
    // git matches a pattern whole or up to a slash, so the team's branches are listed and the
    // task's picked from them.
    let own = format!("{BRANCH_PREFIX}task-{task}-");
    let listed = git(
        &repo,
        &[
            "for-each-ref",
            "--format=%(refname:strip=2) %(objectname)",
            &format!("refs/heads/{BRANCH_PREFIX}"),
        ],
    );
    let listed = match listed {
        Ok(listed) => listed,
        Err(why) => return serde_json::json!({ "state": "unknown", "why": why }),
    };
    let mut branches = Vec::new();
    for line in listed.lines().filter(|l| !l.trim().is_empty()) {
        let Some((branch, head)) = line.split_once(' ') else {
            let why = format!("git listed a branch it could not name: {line}");
            return serde_json::json!({ "state": "unknown", "why": why });
        };
        if branch.starts_with(&own) {
            branches.push(serde_json::json!({ "branch": branch, "head": head }));
        }
    }
    serde_json::json!({ "state": "read", "branches": branches })
}

/// Takes over the checkout kept in `work` for `member`'s turn on `task`: its commits carry
/// this member's trailer from now on. What it was checked out from is kept.
///
/// # Errors
/// What could not be written.
pub fn adopt(work: &Path, member: &str, task: Option<i64>) -> Result<(), String> {
    let was: serde_json::Value = std::fs::read_to_string(work.join("checkout.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let base = base_of(work);
    let branch = was["branch"].as_str().unwrap_or(&base).to_owned();
    noted(work, &branch, &base, member, task)
}

/// Fetches what the node handed over into the checkout already kept in `work`, beside the work
/// in it, never over it: the bundle's branches as `node/<branch>` (a kept submission's too), and
/// upstream's as `upstream/<branch>`. Nothing checked out, committed or not, changes: the member
/// rebases on it if it wants it. The names fetched, for the reply.
///
/// # Errors
/// No checkout, or what git said, in a line.
pub fn refresh(work: &Path, bundle: &[u8]) -> Result<Vec<String>, String> {
    let repo = work.join(REPO_DIR);
    if !repo.join(".git").exists() {
        return Err("nothing is checked out to refresh".to_owned());
    }
    let file = work.join("checkout.bundle");
    std::fs::write(&file, bundle).map_err(|e| format!("the bundle could not be written: {e}"))?;
    let fetched = (|| {
        let path = file.to_string_lossy();
        git(
            &repo,
            &[
                "fetch",
                "-q",
                &path,
                "+refs/heads/*:refs/remotes/node/*",
                "+refs/submissions/*:refs/remotes/node/*",
            ],
        )?;
        unpack_upstream(&repo, &file)?;
        let names = git(
            &repo,
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/remotes/node/",
            ],
        )?;
        Ok(names.lines().map(str::to_owned).collect())
    })();
    let _ = std::fs::remove_file(&file);
    fetched
}

/// Upstream's branch from the bundle (`refs/upstream/<branch>`, ADR 0106, amended 2026-10-31),
/// fetched as `refs/remotes/upstream/<branch>` so `git log upstream/<branch>` reads it. No
/// remote is made for it: it is read, never pushed to. `None` when the bundle has none.
fn unpack_upstream(repo: &Path, file: &Path) -> Result<Option<String>, String> {
    let file = file.to_string_lossy();
    let heads = git(repo, &["bundle", "list-heads", &file])?;
    let Some(branch) = heads
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .find_map(|r| r.strip_prefix("refs/upstream/"))
        .filter(|b| is_plain(b))
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    git(
        repo,
        &[
            "fetch",
            "-q",
            &file,
            &format!("refs/upstream/{branch}:refs/remotes/upstream/{branch}"),
        ],
    )?;
    Ok(Some(format!("upstream/{branch}")))
}

/// Whether `name` is a plain branch name, safe to hand to git: nothing that climbs, nothing git
/// reads as an option, only the characters a branch name has.
fn is_plain(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

/// The default branch the checkout was made against, as noted then.
fn base_of(work: &Path) -> String {
    std::fs::read_to_string(work.join("checkout.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v["base"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "main".to_owned())
}

/// The trailer this checkout stamps, as noted at checkout.
fn trailer_of(work: &Path) -> Option<String> {
    std::fs::read_to_string(work.join("checkout.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v["trailer"].as_str().map(str::to_owned))
}

/// Gives every commit of `base..branch` that lacks the trailer its trailer, rewriting only
/// those (and the commits above them, whose parents move), with git's plumbing: no hook, no
/// script, nothing to execute. The commit hook does the same at commit time, but the sandbox's
/// `/work` is a tmpfs that Docker mounts `noexec`, where git cannot run it; so the trailer is
/// guaranteed here, on the member's own branch, before it is bundled. A branch that already
/// complies is left exactly as it was. Author, dates, trees and any other header are kept.
fn stamp(repo: &Path, base: &str, branch: &str, trailer: &str) -> Result<(), String> {
    let range = format!("refs/heads/{base}..refs/heads/{branch}");
    let order = git(repo, &["rev-list", "--reverse", "--topo-order", &range])?;
    let mut moved: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for old in order.lines().filter(|l| !l.is_empty()) {
        let raw = git(repo, &["cat-file", "commit", old])?;
        let (headers, message) = raw.split_once("\n\n").unwrap_or((raw.as_str(), ""));
        let lacks = !message.lines().any(|l| l.trim_start().starts_with(TRAILER));
        let reparented: Vec<String> = headers
            .lines()
            .map(|line| {
                line.strip_prefix("parent ")
                    .and_then(|p| moved.get(p))
                    .map_or_else(|| line.to_owned(), |new| format!("parent {new}"))
            })
            .collect();
        if !lacks && reparented.iter().map(String::as_str).eq(headers.lines()) {
            continue;
        }
        let mut body = message.trim_end().to_owned();
        if lacks {
            body.push_str(&format!("\n\n{trailer}"));
        }
        let text = format!("{}\n\n{body}\n", reparented.join("\n"));
        let new = git_stdin(
            repo,
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            &text,
        )?;
        moved.insert(old.to_owned(), new);
    }
    let Some(top) = order.lines().last().and_then(|old| moved.get(old)) else {
        return Ok(());
    };
    git(repo, &["update-ref", &format!("refs/heads/{branch}"), top])?;
    Ok(())
}

/// Bundles `branch` (under `hq/`, committed in `work/repo`) from the default branch, for the
/// node to take: the bytes and the branch's head.
///
/// # Errors
/// A branch not under `hq/` or not there, no checkout, or a bundle past [`BUNDLE_MOST`].
pub fn bundle(work: &Path, branch: &str) -> Result<(Vec<u8>, String), String> {
    let branch = branch.trim();
    if !is_team_branch(branch) {
        return Err(format!(
            "the branch is named hq/task-N-<slug>, e.g. hq/task-12-tidy-roles, not {branch:?}"
        ));
    }
    let repo = work.join(REPO_DIR);
    if !repo.join(".git").exists() {
        return Err("nothing is checked out: call repo_checkout first".to_owned());
    }
    git(
        &repo,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )
    .map_err(|_| format!("there is no branch {branch} in /work/repo: git branch lists them"))?;
    let base = base_of(work);
    if git(
        &repo,
        &["rev-parse", "--verify", &format!("refs/heads/{base}")],
    )
    .is_err()
    {
        return Err(format!(
            "the checkout has no {base} to bundle from: check out again"
        ));
    }
    let dirty = git(&repo, &["status", "--porcelain"])?;
    if !dirty.trim().is_empty() {
        return Err(format!(
            "uncommitted changes in /work/repo: commit them (or stash them) first:\n{}",
            dirty.lines().take(10).collect::<Vec<_>>().join("\n")
        ));
    }
    // The trailer is guaranteed here, not left to a hook the sandbox may not be able to run.
    if let Some(trailer) = trailer_of(work) {
        stamp(&repo, &base, branch, &trailer)?;
    }
    let head = git(
        &repo,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )?;
    let file = work.join("submit.bundle");
    git(
        &repo,
        &[
            "bundle",
            "create",
            "-q",
            &file.to_string_lossy(),
            &format!("{base}..{branch}"),
        ],
    )
    .map_err(|why| {
        if why.contains("empty bundle") {
            return format!("{branch} has no commits beyond {base}: commit something first");
        }
        why
    })?;
    let bytes = std::fs::read(&file).map_err(|e| format!("the bundle could not be read: {e}"))?;
    let _ = std::fs::remove_file(&file);
    if bytes.len() > BUNDLE_MOST {
        return Err(format!(
            "the bundle is {} MB, more than the {} MB a submission may be",
            bytes.len() >> 20,
            BUNDLE_MOST >> 20
        ));
    }
    Ok((bytes, head))
}

/// What a work in progress's commit says.
pub const IN_PROGRESS_MESSAGE: &str =
    "Work in progress, kept as the turn's prompt reached its budget";

/// What running Python leaves beside the work (D28 of round 3), as pathspecs left out of a keep:
/// the fork has no ignore entry for them.
const BYPRODUCTS: [&str; 2] = [
    ":(exclude,glob)**/__pycache__/**",
    ":(exclude,glob)**/*.pyc",
];

/// Whether a path is one of Python's byproducts ([`BYPRODUCTS`]).
fn a_byproduct(path: &str) -> bool {
    path.ends_with(".pyc") || path.starts_with("__pycache__/") || path.contains("/__pycache__/")
}

/// Commits what the sandbox's checkout holds as work in progress for `task`, as a turn runs out
/// of room (ADR 0142, amended 2026-10-31), on the branch it is on when that is the task's own
/// (`hq/task-<task>-…`), else on a new `hq/task-<task>-work` from where it stands. Returns the
/// branch to bundle, or `None` when there is no checkout or nothing beyond the base to keep.
/// Python's byproducts are never added, and a branch whose only change from its base is
/// byproducts keeps nothing (D28). The model takes no part: the worker does this after the
/// model's last call.
///
/// # Errors
/// What git said, in a line.
pub fn work_in_progress(work: &Path, task: i64) -> Result<Option<String>, String> {
    let repo = work.join(REPO_DIR);
    if !repo.join(".git").exists() {
        return Ok(None);
    }
    let base = base_of(work);
    let own = format!("{BRANCH_PREFIX}task-{task}-");
    let on = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = if on.starts_with(&own) && is_team_branch(&on) {
        on
    } else {
        let made = format!("{own}work");
        git(&repo, &["checkout", "-q", "-b", &made])?;
        made
    };
    let mut add = vec!["add", "-A", "--", "."];
    add.extend(BYPRODUCTS);
    git(&repo, &add)?;
    if !git(&repo, &["diff", "--cached", "--name-only"])?
        .trim()
        .is_empty()
    {
        git(
            &repo,
            &["commit", "-q", "--no-verify", "-m", IN_PROGRESS_MESSAGE],
        )?;
    }
    let changed = git(
        &repo,
        &[
            "diff",
            "--name-only",
            &format!("refs/heads/{base}...refs/heads/{branch}"),
        ],
    )?;
    if changed
        .lines()
        .all(|f| f.trim().is_empty() || a_byproduct(f.trim()))
    {
        return Ok(None);
    }
    Ok(Some(branch))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Correction-placement review: only a checkout known to be absent reads as no work; one
    /// that cannot be looked at (here `work` is a file, so the path fails with "not a
    /// directory", which `Path::exists` would have taken for absence) is unknown.
    #[test]
    fn a_sandbox_that_cannot_be_looked_at_is_unknown_and_an_absent_one_holds_nothing() {
        let root = std::env::temp_dir().join(format!(
            "endora-worker-reading-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let empty = sandbox_reading(&root, 7);
        assert_eq!(
            empty,
            serde_json::json!({ "state": "read", "branches": [] })
        );
        let file = root.join("work");
        std::fs::write(&file, "not a folder").unwrap();
        assert!(
            !file.join(REPO_DIR).exists(),
            "exists() cannot tell the two apart"
        );
        let unknown = sandbox_reading(&file, 7);
        assert_eq!(unknown["state"], "unknown", "{unknown}");
        assert!(
            unknown["why"].as_str().is_some_and(|w| !w.is_empty()),
            "{unknown}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn run(dir: &Path, args: &[&str]) -> String {
        git(dir, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"))
    }

    /// A fork with one commit on main, bundled as the node hands it over.
    fn a_bundle(name: &str) -> (PathBuf, Vec<u8>) {
        let root = std::env::temp_dir().join(format!(
            "endora-worker-repo-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("seed")).unwrap();
        let seed = root.join("seed");
        run(&seed, &["init", "-q", "-b", "main"]);
        std::fs::write(seed.join("nas.yml"), "---\n- hosts: all\n").unwrap();
        run(&seed, &["add", "."]);
        run(
            &seed,
            &[
                "-c",
                "user.name=John Doe",
                "-c",
                "user.email=john@example.com",
                "commit",
                "-q",
                "-m",
                "the lab",
            ],
        );
        let file = root.join("fork.bundle");
        run(
            &seed,
            &["bundle", "create", "-q", file.to_str().unwrap(), "main"],
        );
        let bytes = std::fs::read(&file).unwrap();
        (root, bytes)
    }

    /// What the node hands over for a kept submission (ADR 0115, amended): the base branch and
    /// the kept ref, `refs/submissions/hq/task-12-tidy`, whole history, in one bundle. Returns
    /// the folder, the bundle and the kept head.
    fn a_kept_bundle(name: &str) -> (PathBuf, Vec<u8>, String) {
        let (root, _) = a_bundle(name);
        let seed = root.join("seed");
        run(&seed, &["checkout", "-q", "-b", "hq/task-12-tidy"]);
        std::fs::write(seed.join("kept.yml"), "---\n").unwrap();
        run(&seed, &["add", "."]);
        run(
            &seed,
            &[
                "-c",
                "user.name=Endora HQ",
                "-c",
                "user.email=hq@endora.invalid",
                "commit",
                "-q",
                "-m",
                "kept\n\nMade-by: Endora HQ (Rae) for task #12",
            ],
        );
        let head = run(&seed, &["rev-parse", "HEAD"]);
        run(
            &seed,
            &["update-ref", "refs/submissions/hq/task-12-tidy", &head],
        );
        let file = root.join("kept.bundle");
        run(
            &seed,
            &[
                "bundle",
                "create",
                "-q",
                file.to_str().unwrap(),
                "refs/heads/main",
                "refs/submissions/hq/task-12-tidy",
            ],
        );
        (root, std::fs::read(&file).unwrap(), head)
    }

    /// D12 (ADR 0142, amended 2026-10-31): what a turn left in its sandbox is committed as
    /// work in progress on the task's branch, or on one made for it, and bundled for the node;
    /// a checkout with nothing beyond its base keeps nothing.
    #[test]
    fn work_left_in_the_sandbox_is_committed_to_the_tasks_branch() {
        let (root, fork) = a_bundle("wip");
        let work = root.join("work");
        let out = checkout(&work, &fork, "main", "main", false, "Kit", Some(7)).unwrap();
        assert_eq!(work_in_progress(&work, 7).unwrap(), None, "nothing to keep");
        // Edits left uncommitted on the base: a branch of the task's is made for them.
        std::fs::write(out.path.join("rule.py"), "class SecretDefault: ...\n").unwrap();
        let kept = work_in_progress(&work, 7).unwrap();
        assert_eq!(kept.as_deref(), Some("hq/task-7-work"));
        assert_eq!(run(&out.path, &["status", "--porcelain"]), "");
        assert_eq!(
            run(&out.path, &["log", "-1", "--format=%s"]),
            IN_PROGRESS_MESSAGE
        );
        let (_, head) = bundle(&work, "hq/task-7-work").unwrap();
        assert!(
            run(&out.path, &["log", "-1", "--format=%B"]).contains(TRAILER),
            "stamped with its trailer when bundled"
        );
        assert_eq!(run(&out.path, &["rev-parse", "HEAD"]), head);
        // On the task's own branch with a commit and more edits: kept on it.
        run(
            &out.path,
            &["checkout", "-q", "-b", "hq/task-7-secret-defaults"],
        );
        std::fs::write(out.path.join("test_rule.py"), "def test(): ...\n").unwrap();
        assert_eq!(
            work_in_progress(&work, 7).unwrap().as_deref(),
            Some("hq/task-7-secret-defaults")
        );
        // Another task's branch is not this task's: the work goes on one of its own.
        run(&out.path, &["checkout", "-q", "-b", "hq/task-8-other"]);
        std::fs::write(out.path.join("more.py"), "x = 1\n").unwrap();
        assert!(
            work_in_progress(&work, 7).is_err(),
            "hq/task-7-work exists: refused"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D28 of round 3: every head kept on 950's work branch added only `__pycache__/*.pyc`,
    /// and the handover called it new work. Python's byproducts are left out of the keep, and a
    /// sandbox with nothing else beyond its base keeps nothing.
    #[test]
    fn python_byproducts_are_not_kept_as_work() {
        let (root, fork) = a_bundle("pyc");
        let work = root.join("work");
        let out = checkout(&work, &fork, "main", "main", false, "Kit", Some(7)).unwrap();
        let cache = out.path.join("tests").join("__pycache__");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("test_rules.cpython-312.pyc"), b"\x00\x01").unwrap();
        std::fs::write(out.path.join("loose.pyc"), b"\x00\x02").unwrap();
        assert_eq!(
            work_in_progress(&work, 7).unwrap(),
            None,
            "byproducts alone"
        );
        // With real work beside them, the work is kept and the byproducts are not.
        std::fs::write(out.path.join("rule.py"), "class SecretDefault: ...\n").unwrap();
        assert_eq!(
            work_in_progress(&work, 7).unwrap().as_deref(),
            Some("hq/task-7-work")
        );
        let kept = run(&out.path, &["show", "--name-only", "--format=", "HEAD"]);
        assert_eq!(kept, "rule.py", "{kept}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D13 (task 936): the sandbox is offline, so upstream ansible-nas comes in the node's
    /// bundle as `refs/upstream/<branch>`. It is unpacked as `upstream/<branch>`, to read with
    /// git and nothing else: no remote, no branch of the member's, and never in a submission
    /// bundled from the base.
    #[test]
    fn upstream_in_the_bundle_is_read_as_upstream_slash_its_branch() {
        let (root, _) = a_bundle("upstream");
        let seed = root.join("seed");
        run(&seed, &["checkout", "-q", "-b", "up"]);
        std::fs::write(seed.join("jellyfin.yml"), "image: jellyfin:10\n").unwrap();
        run(&seed, &["add", "."]);
        run(
            &seed,
            &[
                "-c",
                "user.name=John Doe",
                "-c",
                "user.email=john@example.com",
                "commit",
                "-q",
                "-m",
                "add jellyfin",
            ],
        );
        let up = run(&seed, &["rev-parse", "HEAD"]);
        run(&seed, &["update-ref", "refs/upstream/master", &up]);
        let file = root.join("with-upstream.bundle");
        run(
            &seed,
            &[
                "bundle",
                "create",
                "-q",
                file.to_str().unwrap(),
                "refs/heads/main",
                "refs/upstream/master",
            ],
        );
        let work = root.join("work");
        let bytes = std::fs::read(&file).unwrap();
        let out = checkout(&work, &bytes, "main", "main", false, "Ada", Some(936)).unwrap();
        assert_eq!(out.upstream.as_deref(), Some("upstream/master"));
        assert_eq!(run(&out.path, &["rev-parse", "upstream/master"]), up);
        assert_eq!(
            run(&out.path, &["log", "--format=%s", "main..upstream/master"]),
            "add jellyfin"
        );
        assert_eq!(run(&out.path, &["remote"]), "", "no remote to push to");
        assert_eq!(
            run(&out.path, &["branch", "--format=%(refname:short)"]),
            "main"
        );
        // A plain checkout carries none, and says so.
        let (_, plain) = a_bundle("no-upstream");
        let out = checkout(&work, &plain, "main", "main", false, "Ada", Some(936)).unwrap();
        assert_eq!(out.upstream, None);
    }

    /// Live (task 670): a branch existed only in the turn that made it, so a follow-up turn
    /// recreated the work. A kept branch is checked out as a head of its own on top of its base
    /// (which is still a head too), and can be added to and submitted again.
    #[test]
    fn a_kept_branch_is_checked_out_on_top_of_its_base_and_can_be_submitted_again() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let (root, kept, head) = a_kept_bundle("kept");
        let work = root.join("work");
        let out = checkout(
            &work,
            &kept,
            "hq/task-12-tidy",
            "main",
            true,
            "Rae",
            Some(12),
        )
        .unwrap();
        assert_eq!(
            (out.branch.as_str(), out.head.as_str()),
            ("hq/task-12-tidy", head.as_str())
        );
        assert_eq!(out.files, 2, "the base's file and the branch's");
        assert_eq!(
            run(&out.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "hq/task-12-tidy"
        );
        assert_eq!(run(&out.path, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            run(&out.path, &["rev-list", "--count", "main..hq/task-12-tidy"]),
            "1",
            "main is a head of its own, one commit behind the kept branch"
        );
        assert_eq!(run(&out.path, &["remote"]), "", "no remote to push to");
        assert!(
            !work.join("checkout.bundle").exists(),
            "the bundle is not left about"
        );
        // Added to and bundled from main, with both commits and nothing of main's: the node
        // takes it as a submission again.
        std::fs::write(out.path.join("more.yml"), "---\n").unwrap();
        run(&out.path, &["add", "-A"]);
        run(&out.path, &["commit", "-q", "-m", "more"]);
        let (bytes, new_head) = bundle(&work, "hq/task-12-tidy").unwrap();
        assert_ne!(new_head, head);
        assert_eq!(
            run(&out.path, &["rev-list", "--count", "main..hq/task-12-tidy"]),
            "2"
        );
        let file = root.join("out.bundle");
        std::fs::write(&file, &bytes).unwrap();
        let heads = run(&out.path, &["bundle", "list-heads", file.to_str().unwrap()]);
        assert!(heads.ends_with("refs/heads/hq/task-12-tidy"), "{heads}");
        // Checked out again over the old one: the folder is emptied, not replaced, so a shell
        // that is inside it (Claude Code's) still stands in it; and it is the kept head again.
        // (A mode only this folder has, since a folder made again would have the default one.)
        std::fs::set_permissions(&out.path, std::fs::Permissions::from_mode(0o750)).unwrap();
        let folder = std::fs::metadata(&out.path).unwrap().ino();
        let again = checkout(
            &work,
            &kept,
            "hq/task-12-tidy",
            "main",
            true,
            "Rae",
            Some(12),
        )
        .unwrap();
        let after = std::fs::metadata(&again.path).unwrap();
        assert_eq!((after.ino(), after.mode() & 0o777), (folder, 0o750));
        assert_eq!(again.head, head, "none of the commits made since");
        assert!(!again.path.join("more.yml").exists());
        // A bundle that is said to be kept and holds no kept ref is no checkout.
        let (_, plain) = a_bundle("plain");
        let why = checkout(
            &work,
            &plain,
            "hq/task-12-tidy",
            "main",
            true,
            "Rae",
            Some(12),
        )
        .unwrap_err();
        assert!(why.contains("hq/task-12-tidy"), "{why}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Set 4's #1059 committed `tests/lint_rules/__pycache__/*.pyc`: the model's own
    /// `git add -A`, as its instructions say, took what running the tests left, in a fork with no
    /// ignore entry for it. A checkout ignores Python's byproducts locally, so it never can.
    #[test]
    fn the_models_own_add_all_leaves_pythons_byproducts_out() {
        let (root, fork) = a_bundle("byproducts");
        let work = root.join("work");
        let out = checkout(&work, &fork, "main", "main", false, "Rae", Some(12)).unwrap();
        let cache = out.path.join("tests").join("__pycache__");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("test_rules.cpython-311.pyc"), b"\x00\x01").unwrap();
        std::fs::write(out.path.join("loose.pyc"), b"\x00\x02").unwrap();
        std::fs::write(
            out.path.join("tests").join("test_rules.py"),
            "import unittest\n",
        )
        .unwrap();
        run(&out.path, &["add", "-A"]);
        let staged = run(&out.path, &["diff", "--cached", "--name-only"]);
        assert_eq!(staged.trim(), "tests/test_rules.py", "{staged}");
        assert_eq!(
            run(&out.path, &["status", "--porcelain"]).trim(),
            "A  tests/test_rules.py"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_checkout_sets_git_up_the_teams_way_and_a_commit_gets_its_trailer() {
        let (root, fork) = a_bundle("checkout");
        let work = root.join("work");
        let out = checkout(&work, &fork, "main", "main", false, "Rae", Some(12)).unwrap();
        assert_eq!(out.path, work.join("repo"));
        assert_eq!((out.branch.as_str(), out.files), ("main", 1));
        assert_eq!(out.head.len(), 40);
        assert!(
            !work.join("checkout.bundle").exists(),
            "the bundle is not left about"
        );
        assert_eq!(run(&out.path, &["remote"]), "", "no remote to push to");
        assert_eq!(run(&out.path, &["config", "user.email"]), AUTHOR_EMAIL);
        // The model commits as it likes; the trailer is there all the same, once.
        run(&out.path, &["checkout", "-q", "-b", "hq/task-12-tidy"]);
        std::fs::write(out.path.join("nas.yml"), "---\n- hosts: nas\n").unwrap();
        run(&out.path, &["add", "-A"]);
        run(&out.path, &["commit", "-q", "-m", "tidy the play"]);
        let message = run(&out.path, &["log", "-1", "--format=%B%x00%an <%ae>"]);
        assert!(
            message.contains("tidy the play\n\nMade-by: Endora HQ (Rae) for task #12"),
            "{message}"
        );
        assert!(
            message.ends_with("Endora HQ <hq@endora.invalid>"),
            "{message}"
        );
        std::fs::write(out.path.join("more.yml"), "---\n").unwrap();
        run(&out.path, &["add", "-A"]);
        run(
            &out.path,
            &[
                "commit",
                "-q",
                "-m",
                "more\n\nMade-by: Endora HQ (Rae, Zed) for task #12",
            ],
        );
        let message = run(&out.path, &["log", "-1", "--format=%B"]);
        assert_eq!(
            message.matches(TRAILER).count(),
            1,
            "not added twice: {message}"
        );
        // The branch bundles from main, with both commits and nothing of main's.
        let (bytes, head) = bundle(&work, "hq/task-12-tidy").unwrap();
        assert_eq!(head, run(&out.path, &["rev-parse", "hq/task-12-tidy"]));
        let file = root.join("out.bundle");
        std::fs::write(&file, &bytes).unwrap();
        let heads = run(&out.path, &["bundle", "list-heads", file.to_str().unwrap()]);
        assert!(heads.ends_with("refs/heads/hq/task-12-tidy"), "{heads}");
        // Checked out again, over the old one: fresh.
        let again = checkout(&work, &fork, "main", "main", false, "Rae", None).unwrap();
        assert!(run(&again.path, &["branch", "--list", "hq/*"]).is_empty());
        std::fs::write(again.path.join("x.yml"), "---\n").unwrap();
        run(&again.path, &["add", "-A"]);
        run(&again.path, &["commit", "-q", "-m", "x"]);
        assert!(
            run(&again.path, &["log", "-1", "--format=%B"]).contains("(Rae) for the wing"),
            "no task: for the wing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// ADR 0142, amended 2026-11-01: the sandbox keeps an item's work between its turns, so a
    /// later turn finds the checkout where the last one left it, uncommitted work and all. It
    /// serves a checkout of no branch, or of a branch it has; the next member's commits carry
    /// that member's trailer; and a refresh fetches the node's latest beside it, never over it.
    #[test]
    fn a_kept_checkout_is_found_taken_over_and_refreshed_beside_the_work() {
        let (root, fork) = a_bundle("kept-checkout");
        let work = root.join("work");
        assert_eq!(already_there(&work, None), None, "nothing checked out");
        let out = checkout(&work, &fork, "main", "main", false, "Rae", Some(12)).unwrap();
        run(&out.path, &["checkout", "-q", "-b", "hq/task-12-tidy"]);
        std::fs::write(out.path.join("nas.yml"), "---\n- hosts: nas\n").unwrap();

        // The next turn on the item, Bo's.
        let there = already_there(&work, None).expect("kept");
        assert_eq!(there.on, "hq/task-12-tidy");
        assert_eq!(there.uncommitted, 1);
        assert_eq!(there.path, out.path);
        assert!(already_there(&work, Some("hq/task-12-tidy")).is_some());
        assert!(
            already_there(&work, Some("main")).is_some(),
            "a branch here"
        );
        assert_eq!(already_there(&work, Some("hq/task-12-other")), None);
        assert_eq!(already_there(&work, Some("--all")), None);
        adopt(&work, "Bo", Some(12)).unwrap();
        run(&out.path, &["add", "-A"]);
        run(&out.path, &["commit", "-q", "-m", "tidy the play"]);
        let message = run(&out.path, &["log", "-1", "--format=%B"]);
        assert!(
            message.contains("Made-by: Endora HQ (Bo) for task #12"),
            "{message}"
        );
        assert_eq!(base_of(&work), "main", "what it came from is kept");

        // The node's main moves on; a refresh fetches it as node/main, and nothing of Bo's
        // changes.
        let seed = root.join("seed");
        std::fs::write(seed.join("new.yml"), "---\n").unwrap();
        run(&seed, &["add", "."]);
        run(
            &seed,
            &[
                "-c",
                "user.name=John Doe",
                "-c",
                "user.email=john@example.com",
                "commit",
                "-q",
                "-m",
                "moved on",
            ],
        );
        let file = root.join("newer.bundle");
        run(
            &seed,
            &["bundle", "create", "-q", file.to_str().unwrap(), "main"],
        );
        let mine = run(&out.path, &["rev-parse", "HEAD"]);
        let fetched = refresh(&work, &std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(fetched, ["node/main"]);
        assert_eq!(
            run(&out.path, &["rev-parse", "HEAD"]),
            mine,
            "nothing over it"
        );
        assert_eq!(
            run(&out.path, &["rev-parse", "node/main"]),
            run(&seed, &["rev-parse", "main"])
        );
        assert!(!work.join("checkout.bundle").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_commit_made_without_the_hook_is_stamped_when_the_branch_is_bundled() {
        // The sandbox's /work is a tmpfs, mounted noexec by default, so git cannot run the hook
        // there: the trailer must not depend on it.
        let (root, fork) = a_bundle("stamp");
        let work = root.join("work");
        let out = checkout(&work, &fork, "main", "main", false, "Rae", Some(12)).unwrap();
        run(&out.path, &["checkout", "-q", "-b", "hq/task-12-tidy"]);
        let commit = |file: &str, message: &str| {
            std::fs::write(out.path.join(file), "---\n").unwrap();
            run(&out.path, &["add", "-A"]);
            run(
                &out.path,
                &[
                    "-c",
                    "core.hooksPath=/dev/null",
                    "commit",
                    "-q",
                    "-m",
                    message,
                ],
            );
        };
        commit("a.yml", "first, no trailer");
        commit(
            "b.yml",
            "second\n\nMade-by: Endora HQ (Rae, Zed) for task #12",
        );
        commit("c.yml", "third, no trailer");
        let before_tree = run(&out.path, &["rev-parse", "HEAD^{tree}"]);
        let second = run(&out.path, &["rev-parse", "HEAD^"]);
        let (bytes, head) = bundle(&work, "hq/task-12-tidy").unwrap();
        // The branch itself carries the stamped commits, and the head returned is theirs.
        assert_eq!(head, run(&out.path, &["rev-parse", "hq/task-12-tidy"]));
        assert_eq!(before_tree, run(&out.path, &["rev-parse", "HEAD^{tree}"]));
        let log = run(
            &out.path,
            &[
                "log",
                "--format=%B%x1e%an <%ae>%x1f",
                "main..hq/task-12-tidy",
            ],
        );
        let commits: Vec<&str> = log
            .split('\u{1f}')
            .filter(|c| !c.trim().is_empty())
            .collect();
        assert_eq!(commits.len(), 3, "{log}");
        for c in &commits {
            assert_eq!(c.matches(TRAILER).count(), 1, "one trailer each: {c}");
            assert!(
                c.trim_end().ends_with("Endora HQ <hq@endora.invalid>"),
                "{c}"
            );
        }
        assert!(
            log.contains("first, no trailer\n\nMade-by: Endora HQ (Rae) for task #12"),
            "{log}"
        );
        assert!(
            log.contains("second\n\nMade-by: Endora HQ (Rae, Zed) for task #12"),
            "{log}"
        );
        // The one that had its trailer is stamped the same, so its parent chain moved only
        // because the first did; a branch that already complies is left exactly as it was.
        assert_ne!(second, run(&out.path, &["rev-parse", "HEAD^"]));
        let heads = run(
            &out.path,
            &["bundle", "list-heads", &{
                let f = root.join("o.bundle");
                std::fs::write(&f, &bytes).unwrap();
                f.to_string_lossy().into_owned()
            }],
        );
        assert!(heads.contains(&head), "{heads}");
        let (_, again) = bundle(&work, "hq/task-12-tidy").unwrap();
        assert_eq!(again, head, "already stamped: nothing is rewritten");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_submission_is_refused_off_hq_uncommitted_or_with_nothing_to_send() {
        let (root, fork) = a_bundle("refuse");
        let work = root.join("work");
        assert!(
            bundle(&work, "hq/task-1-x")
                .unwrap_err()
                .contains("repo_checkout first")
        );
        let out = checkout(&work, &fork, "main", "main", false, "Rae", Some(1)).unwrap();
        for bad in ["main", "feature/x", "hq/", "hq/../main", "-hq/x", "hq/a b"] {
            assert!(
                bundle(&work, bad).unwrap_err().contains("hq/task-N"),
                "{bad}"
            );
        }
        assert!(
            bundle(&work, "hq/task-1-none")
                .unwrap_err()
                .contains("no branch")
        );
        run(&out.path, &["checkout", "-q", "-b", "hq/task-1-same"]);
        let why = bundle(&work, "hq/task-1-same").unwrap_err();
        assert!(why.contains("no commits beyond main"), "{why}");
        std::fs::write(out.path.join("nas.yml"), "changed\n").unwrap();
        let why = bundle(&work, "hq/task-1-same").unwrap_err();
        assert!(why.contains("uncommitted"), "{why}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
