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
    branch.starts_with(BRANCH_PREFIX)
        && branch.len() > BRANCH_PREFIX.len()
        && !branch.contains("..")
        && !branch.starts_with('-')
        && branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
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
    // No remote: there is nowhere to push from the sandbox, and nothing to push with.
    git(&repo, &["remote", "remove", "origin"])?;
    git(&repo, &["config", "user.name", AUTHOR_NAME])?;
    git(&repo, &["config", "user.email", AUTHOR_EMAIL])?;
    git(&repo, &["config", "commit.gpgsign", "false"])?;
    git(&repo, &["config", "core.hooksPath", ".git/hooks"])?;
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
    .map_err(|e| format!("the checkout could not be noted: {e}"))?;
    let head = git(&repo, &["rev-parse", "HEAD"])?;
    let files = git(&repo, &["ls-files"])?.lines().count();
    let _ = std::fs::remove_file(&file);
    Ok(CheckedOut {
        path: repo,
        branch: branch.to_owned(),
        head,
        files,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

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
