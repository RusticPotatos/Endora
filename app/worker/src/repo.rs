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

/// A word in a shell single-quoted string.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// Unpacks `bundle` (one branch, `branch`, with its history) into `work/repo`, fresh, and sets
/// git up the team's way: the author, no remote, and a `prepare-commit-msg` hook that adds
/// the trailer naming `member` and `task` when a commit lacks it. `base`, the branch the wing
/// works from, is kept beside it, so a submission is bundled from there.
///
/// # Errors
/// What git said, in a line.
pub fn checkout(
    work: &Path,
    bundle: &[u8],
    branch: &str,
    base: &str,
    member: &str,
    task: Option<i64>,
) -> Result<CheckedOut, String> {
    std::fs::create_dir_all(work).map_err(|e| format!("no {}: {e}", work.display()))?;
    let file = work.join("checkout.bundle");
    std::fs::write(&file, bundle).map_err(|e| format!("the bundle could not be written: {e}"))?;
    let repo = work.join(REPO_DIR);
    if repo.exists() {
        std::fs::remove_dir_all(&repo).map_err(|e| format!("the old checkout stays: {e}"))?;
    }
    git(
        work,
        &[
            "clone",
            "-q",
            "-b",
            branch,
            &file.to_string_lossy(),
            REPO_DIR,
        ],
    )?;
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
        serde_json::json!({ "branch": branch, "base": base }).to_string(),
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

/// Bundles `branch` (under `hq/`, committed in `work/repo`) from the default branch, for the
/// node to take: the bytes and the branch's head.
///
/// # Errors
/// A branch not under `hq/` or not there, no checkout, or a bundle past [`BUNDLE_MOST`].
pub fn bundle(work: &Path, branch: &str) -> Result<(Vec<u8>, String), String> {
    let branch = branch.trim();
    if !branch.starts_with(BRANCH_PREFIX)
        || branch.len() <= BRANCH_PREFIX.len()
        || branch.contains("..")
        || branch.starts_with('-')
        || !branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(format!(
            "the branch is named hq/task-N-<slug>, e.g. hq/task-12-tidy-roles, not {branch:?}"
        ));
    }
    let repo = work.join(REPO_DIR);
    if !repo.join(".git").exists() {
        return Err("nothing is checked out: call repo_checkout first".to_owned());
    }
    let head = git(
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

    #[test]
    fn a_checkout_sets_git_up_the_teams_way_and_a_commit_gets_its_trailer() {
        let (root, fork) = a_bundle("checkout");
        let work = root.join("work");
        let out = checkout(&work, &fork, "main", "main", "Rae", Some(12)).unwrap();
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
        let again = checkout(&work, &fork, "main", "main", "Rae", None).unwrap();
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
    fn a_submission_is_refused_off_hq_uncommitted_or_with_nothing_to_send() {
        let (root, fork) = a_bundle("refuse");
        let work = root.join("work");
        assert!(
            bundle(&work, "hq/task-1-x")
                .unwrap_err()
                .contains("repo_checkout first")
        );
        let out = checkout(&work, &fork, "main", "main", "Rae", Some(1)).unwrap();
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
