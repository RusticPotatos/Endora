//! The engineering wing's git (ADR 0115): how the team's code reaches the owner's fork of
//! ansible-nas, and nothing else. The node keeps a bare mirror of the fork under its data
//! folder, refreshed over plain HTTPS without a credential, and hands a turn's sandbox a bundle
//! of a branch to work on (`repo_checkout`). The sandbox sends its work back as a bundle of one
//! `hq/*` branch (`repo_submit`), which is accepted only when it applies on top of the fork's
//! default branch, is small, and every commit is HQ's own. The owner alone lets a branch leave
//! the house: on their approval the node pushes the stored submission to the fork with a
//! deploy key, never with force, never to the default branch. HQ opens no pull request and
//! merges nothing.
//!
//! Every refusal here is a rule in code: the model proposes a branch, this decides it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// The most a checkout bundle may be: the fork is about 6 MB.
pub const BUNDLE_MOST: usize = 32 * 1024 * 1024;
/// The most a submitted bundle may be: a branch of a few commits is kilobytes.
pub const SUBMISSION_MOST: usize = 8 * 1024 * 1024;
/// The most files a submitted branch may change.
pub const FILES_MOST: usize = 200;
/// The most a submitted branch's diff may run to, in bytes.
pub const DIFF_MOST: usize = 1024 * 1024;
/// The most of a diff shown in the task's document.
pub const SHOWN_MOST: usize = 64 * 1024;
/// How often the mirror is refreshed from the fork, at most.
pub const REFRESH_MS: i64 = 5 * 60 * 1000;
/// Where the team's branches live: under `hq/`, and nowhere else.
pub const BRANCH_PREFIX: &str = "hq/";
/// The most characters a branch's slug runs to.
pub const SLUG_MOST: usize = 40;
/// Who authors the team's commits.
pub const AUTHOR_NAME: &str = "Endora HQ";
/// The address the team's commits are authored with: a reserved domain, never a mailbox.
pub const AUTHOR_EMAIL: &str = "hq@endora.invalid";
/// The trailer every commit of the team's carries: `Made-by: Endora HQ (<members>) for task #N`.
pub const TRAILER: &str = "Made-by: Endora HQ";

/// GitHub's SSH host keys, as `https://api.github.com/meta` publishes them (`ssh_keys`), so a
/// push trusts GitHub and nothing that answers in its place. Checked in shape by a test; a
/// live comparison with `/meta` is not run here.
pub const GITHUB_KNOWN_HOSTS: &str = "\
github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl
github.com ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=
github.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAAgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6+PKCUXaDbC7qtbW8gIkhL7aGCsOr/C56SJMy/BCZfxd1nWzAOxSDPgVsmerOBYfNqltV9/hWCqBywINIR+5dIg6JTJ72pcEpEjcYgXkE2YEFXV1JHnsKgbLWNlhScqb2UmyRkQyytRLtL+38TGxkxCflmO+5Z8CSSNY7GidjMIZ7Q4zMjA2n1nGrlTDkzwDCsw+wqFPGQA179cnfGWOWRVruj16z6XyvxvjJwbz0wQZ75XK5tKSb7FNyeIEs4TT4jk+S4dhPeAUC5y+bDYirYgM4GC7uEnztnZyaVWQ7B381AK4Qdrwt51ZqExKbQpTUNn+EjqoTwvqNj4kqx5QUCI0ThS/YkOxJCXmPUWZbhjpCg56i+2aB6CmK2JGhn57K5mj0MNdBXA4/WnwH6XoPWJzK5Nyu2zB3nAZp+S5hpQs+p1vN1/wsjk=
";

/// A task's title as a branch slug: lowercase letters, digits and dashes, at most
/// [`SLUG_MOST`] characters, no dash at either end; `work` when nothing is left.
#[must_use]
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut dash = true;
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
            continue;
        }
        if !dash {
            out.push('-');
            dash = true;
        }
    }
    let mut out: String = out.chars().take(SLUG_MOST).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return "work".to_owned();
    }
    out
}

/// The branch a task's work goes on: `hq/task-<N>-<slug>`.
#[must_use]
pub fn branch_for(task: i64, title: &str) -> String {
    format!("{BRANCH_PREFIX}task-{task}-{}", slug(title))
}

/// A branch name as the team may use one: a plain name under `hq/`, nothing else.
///
/// # Errors
/// Why not, as the model reads it.
pub fn hq_branch(name: &str) -> Result<&str, String> {
    let name = name.trim();
    let bare = name.strip_prefix("refs/heads/").unwrap_or(name);
    if !bare.starts_with(BRANCH_PREFIX) || bare.len() <= BRANCH_PREFIX.len() {
        return Err(format!(
            "the team's branches are named hq/<task-N>-<slug>, e.g. hq/task-12-tidy-roles; not {name:?}"
        ));
    }
    if !crate::ansible_nas::plain_branch(bare) || bare.starts_with('+') {
        return Err(format!("{name:?} is not a plain branch name"));
    }
    Ok(bare)
}

/// Whether `url` names a remote without a credential in it: a `scheme://user:secret@host`
/// address is refused, and only `git@host:path` may carry a user.
///
/// # Errors
/// Why not.
pub fn plain_remote(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("no remote".to_owned());
    }
    if let Some((_, rest)) = url.split_once("://") {
        let authority = rest.split('/').next().unwrap_or("");
        if authority.contains('@') {
            return Err("a remote with a credential in its address is refused".to_owned());
        }
        return Ok(());
    }
    if url.starts_with("git@") || !url.contains('@') {
        return Ok(());
    }
    Err("a remote with a credential in its address is refused".to_owned())
}

/// One `git` run: its stdout, or its stderr as the error.
fn git(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    for (key, value) in env {
        command.env(key, value);
    }
    let out = command
        .output()
        .map_err(|e| format!("git could not be run: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let said: String = said
            .lines()
            .last()
            .unwrap_or("")
            .chars()
            .take(300)
            .collect();
        return Err(format!(
            "git {} failed: {said}",
            args.first().copied().unwrap_or("")
        ));
    }
    Ok(out.stdout)
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_owned()
}

/// A bundle of one branch, as a checkout is handed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The branch.
    pub branch: String,
    /// Its head.
    pub head: String,
    /// The bundle itself.
    pub bytes: Vec<u8>,
}

/// A branch the team submitted, once it passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submitted {
    /// The branch, under `hq/`.
    pub branch: String,
    /// Its head.
    pub head: String,
    /// The default branch's head it sits on.
    pub base: String,
    /// How many files it changes.
    pub files: usize,
    /// Lines added.
    pub plus: i64,
    /// Lines removed.
    pub minus: i64,
    /// `git diff --stat`.
    pub stat: String,
    /// The diff, held to [`SHOWN_MOST`] with a note when cut.
    pub diff: String,
    /// Where the bundle is kept.
    pub path: PathBuf,
}

impl Submitted {
    /// The document's title: `branch hq/x (3 files, +10 −2)`.
    #[must_use]
    pub fn title(&self) -> String {
        format!(
            "branch {} ({} file{}, +{} −{})",
            self.branch,
            self.files,
            if self.files == 1 { "" } else { "s" },
            self.plus,
            self.minus
        )
    }
}

/// The node's bare mirror of the fork, and the submissions kept beside it.
pub struct Mirror {
    dir: PathBuf,
    submissions: PathBuf,
    url: String,
    default_branch: String,
    refreshed: Mutex<Option<i64>>,
}

impl Mirror {
    /// The mirror of the repository at `url` (its default branch `default_branch`), kept under
    /// `data_dir/team-repo`.
    #[must_use]
    pub fn new(data_dir: &Path, url: &str, default_branch: &str) -> Self {
        let root = data_dir.join("team-repo");
        Self {
            dir: root.join("mirror.git"),
            submissions: root.join("submissions"),
            url: url.to_owned(),
            default_branch: default_branch.to_owned(),
            refreshed: Mutex::new(None),
        }
    }

    /// The fork's default branch, as the mirror knows it.
    #[must_use]
    pub fn default_branch(&self) -> &str {
        &self.default_branch
    }

    /// Where the mirror is.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Clones the fork as a bare mirror, or fetches what changed, at most once in
    /// [`REFRESH_MS`]; the first call makes it.
    ///
    /// # Errors
    /// What git said, in a line.
    pub fn refresh(&self, now: i64) -> Result<(), String> {
        plain_remote(&self.url)?;
        let mut last = self
            .refreshed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last.is_some_and(|at| now - at < REFRESH_MS) && self.dir.join("HEAD").exists() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.submissions).map_err(|e| format!("no data folder: {e}"))?;
        let parent = self
            .dir
            .parent()
            .ok_or_else(|| "the mirror has no folder".to_owned())?;
        if self.dir.join("HEAD").exists() {
            git(&self.dir, &["fetch", "--prune", "--no-tags", "origin"], &[])?;
        } else {
            let target = self.dir.to_string_lossy().into_owned();
            git(
                parent,
                &["clone", "--mirror", "--no-tags", &self.url, &target],
                &[],
            )?;
        }
        *last = Some(now);
        Ok(())
    }

    /// A branch's head in the mirror.
    ///
    /// # Errors
    /// No such branch.
    pub fn head_of(&self, branch: &str) -> Result<String, String> {
        git(
            &self.dir,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/heads/{branch}^{{commit}}"),
            ],
            &[],
        )
        .map(text)
        .map_err(|_| format!("the fork has no branch {branch}"))
    }

    /// A bundle of `branch` (the default branch when `None`): the default branch, or one of the
    /// team's under `hq/`, with its whole history, at most [`BUNDLE_MOST`].
    ///
    /// # Errors
    /// A branch that is neither, one the fork does not have, or git failing.
    pub fn bundle(&self, branch: Option<&str>, now: i64) -> Result<Bundle, String> {
        let branch = match branch.map(str::trim).filter(|b| !b.is_empty()) {
            None => self.default_branch.clone(),
            Some(named) if named == self.default_branch => named.to_owned(),
            Some(named) => hq_branch(named)
                .map_err(|_| {
                    format!(
                        "check out the fork's {} or one of the team's hq/ branches, not {named:?}",
                        self.default_branch
                    )
                })?
                .to_owned(),
        };
        self.refresh(now)?;
        let head = self.head_of(&branch)?;
        let bytes = git(
            &self.dir,
            &["bundle", "create", "-", &format!("refs/heads/{branch}")],
            &[],
        )?;
        if bytes.len() > BUNDLE_MOST {
            return Err(format!(
                "the bundle of {branch} is {} MB, more than the {} MB a checkout may be",
                bytes.len() >> 20,
                BUNDLE_MOST >> 20
            ));
        }
        Ok(Bundle {
            branch,
            head,
            bytes,
        })
    }

    /// Where a submission of `branch` at `head` is kept, if it is.
    #[must_use]
    pub fn stored(&self, task: i64, branch: &str, head: &str) -> Option<PathBuf> {
        let path = self.submission_path(task, branch, head);
        path.exists().then_some(path)
    }

    fn submission_path(&self, task: i64, branch: &str, head: &str) -> PathBuf {
        let short: String = head.chars().take(12).collect();
        let name = branch.rsplit('/').next().unwrap_or(branch);
        self.submissions
            .join(format!("task-{task}"))
            .join(format!("{name}-{short}.bundle"))
    }

    /// Takes a submitted bundle of `branch` for `task`: it must hold exactly that one branch,
    /// under `hq/`, apply on top of the mirror's default branch (a fast-forward), change at
    /// most [`FILES_MOST`] files and [`DIFF_MOST`] bytes, and every commit of it must be HQ's
    /// (authored [`AUTHOR_EMAIL`], carrying the [`TRAILER`]). What passes is kept: the bundle
    /// under the task, and the branch as `refs/submissions/<branch>` in the mirror.
    ///
    /// # Errors
    /// Which rule refused it, as the model reads it.
    pub fn submit(
        &self,
        bytes: &[u8],
        branch: &str,
        task: i64,
        now: i64,
    ) -> Result<Submitted, String> {
        let branch = hq_branch(branch)?.to_owned();
        if bytes.is_empty() {
            return Err("the bundle is empty".to_owned());
        }
        if bytes.len() > SUBMISSION_MOST {
            return Err(format!(
                "the bundle is {} MB, more than the {} MB a submission may be",
                bytes.len() >> 20,
                SUBMISSION_MOST >> 20
            ));
        }
        self.refresh(now)?;
        let incoming = self
            .submissions
            .join(format!("incoming-{}-{now}.bundle", std::process::id()));
        std::fs::write(&incoming, bytes)
            .map_err(|e| format!("the bundle could not be kept: {e}"))?;
        let taken = self.take(&incoming, &branch, task);
        if taken.is_err() {
            let _ = std::fs::remove_file(&incoming);
        }
        taken
    }

    fn take(&self, incoming: &Path, branch: &str, task: i64) -> Result<Submitted, String> {
        let file = incoming.to_string_lossy().into_owned();
        git(&self.dir, &["bundle", "verify", &file], &[]).map_err(|_| {
            format!(
                "the bundle does not apply to the fork: its base is not in {}. Check out again and put your commits on top of it",
                self.default_branch
            )
        })?;
        let heads = text(git(&self.dir, &["bundle", "list-heads", &file], &[])?);
        let refs: Vec<&str> = heads
            .lines()
            .filter_map(|l| l.split_whitespace().nth(1))
            .filter(|r| *r != "HEAD")
            .collect();
        let wanted = format!("refs/heads/{branch}");
        if refs != [wanted.as_str()] {
            return Err(format!(
                "the bundle holds {}; submit exactly one branch, {branch}",
                if refs.is_empty() {
                    "no branch".to_owned()
                } else {
                    refs.join(", ")
                }
            ));
        }
        let kept = format!("refs/submissions/{branch}");
        git(
            &self.dir,
            &["fetch", "--no-tags", &file, &format!("+{wanted}:{kept}")],
            &[],
        )?;
        let head = text(git(&self.dir, &["rev-parse", &kept], &[])?);
        let base_ref = format!("refs/heads/{}", self.default_branch);
        let base = text(git(&self.dir, &["rev-parse", &base_ref], &[])?);
        if git(
            &self.dir,
            &["merge-base", "--is-ancestor", &base_ref, &kept],
            &[],
        )
        .is_err()
        {
            return Err(format!(
                "{branch} is not on top of {}: its head {} is {}. Rebase it on {} and submit again",
                self.default_branch,
                &base[..12.min(base.len())],
                "behind or beside the fork's",
                self.default_branch
            ));
        }
        if head == base {
            return Err(format!(
                "{branch} has no commits beyond {}: nothing to submit",
                self.default_branch
            ));
        }
        let range = format!("{base_ref}..{kept}");
        let commits = text(git(
            &self.dir,
            &["log", "--format=%H%x1f%ae%x1f%B%x1e", &range],
            &[],
        )?);
        for commit in commits.split('\u{1e}').filter(|c| !c.trim().is_empty()) {
            let mut parts = commit.trim_start().splitn(3, '\u{1f}');
            let sha: String = parts.next().unwrap_or("").chars().take(12).collect();
            let email = parts.next().unwrap_or("");
            let body = parts.next().unwrap_or("");
            if email != AUTHOR_EMAIL {
                return Err(format!(
                    "commit {sha} is authored by {email:?}: the team's commits are authored {AUTHOR_NAME} <{AUTHOR_EMAIL}> (repo_checkout sets that)"
                ));
            }
            if !body.lines().any(|l| l.trim_start().starts_with(TRAILER)) {
                return Err(format!(
                    "commit {sha} lacks its trailer: every commit ends \"{TRAILER} (<member names>) for task #N\""
                ));
            }
        }
        let names = text(git(&self.dir, &["diff", "--name-only", &base, &head], &[])?);
        let files = names.lines().filter(|l| !l.is_empty()).count();
        if files > FILES_MOST {
            return Err(format!(
                "{branch} changes {files} files, more than the {FILES_MOST} a submission may"
            ));
        }
        let diff = git(&self.dir, &["diff", &base, &head], &[])?;
        if diff.len() > DIFF_MOST {
            return Err(format!(
                "{branch}'s diff is {} KB, more than the {} KB a submission may be",
                diff.len() >> 10,
                DIFF_MOST >> 10
            ));
        }
        let (mut plus, mut minus) = (0_i64, 0_i64);
        for line in text(git(&self.dir, &["diff", "--numstat", &base, &head], &[])?).lines() {
            let mut cols = line.split('\t');
            plus += cols.next().and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
            minus += cols.next().and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
        }
        let stat = text(git(&self.dir, &["diff", "--stat=100", &base, &head], &[])?);
        let path = self.submission_path(task, branch, &head);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|e| format!("the bundle could not be kept: {e}"))?;
        }
        std::fs::rename(incoming, &path)
            .map_err(|e| format!("the bundle could not be kept: {e}"))?;
        Ok(Submitted {
            branch: branch.to_owned(),
            head,
            base,
            files,
            plus,
            minus,
            stat,
            diff: shown(&diff),
            path,
        })
    }

    /// Pushes the stored submission of `branch` at `head` to `remote`, as that branch: only a
    /// branch under `hq/`, only what was submitted (never the default branch, never with
    /// force), over SSH with the deploy key at `key` when the remote is an SSH address (with
    /// GitHub's own host keys and nothing else trusted). Returns what was pushed, for the
    /// owner.
    ///
    /// # Errors
    /// Which rule refused it, or what git said.
    pub fn push(
        &self,
        branch: &str,
        head: &str,
        remote: &str,
        key: Option<&Path>,
    ) -> Result<String, String> {
        let branch = hq_branch(branch)?;
        if branch == self.default_branch || branch.starts_with('+') {
            return Err(format!("{branch} is never pushed by HQ"));
        }
        plain_remote(remote)?;
        let kept = format!("refs/submissions/{branch}");
        let stored = git(
            &self.dir,
            &["rev-parse", "--verify", &format!("{kept}^{{commit}}")],
            &[],
        )
        .map(text)
        .map_err(|_| format!("no submission of {branch} is stored"))?;
        if !stored.starts_with(head.trim()) || head.trim().len() < 7 {
            return Err(format!(
                "the stored submission of {branch} is {}, not {head}: nothing was pushed",
                &stored[..12.min(stored.len())]
            ));
        }
        let over_ssh = remote.starts_with("git@") || remote.starts_with("ssh://");
        let mut env: Vec<(String, String)> = Vec::new();
        if over_ssh {
            let Some(key) = key.filter(|k| k.exists()) else {
                return Err(
                    "pushing is not set up: ENDORA_GIT_PUSH_KEY_FILE names no deploy key on the node"
                        .to_owned(),
                );
            };
            let known = self.submissions.join("github_known_hosts");
            std::fs::write(&known, GITHUB_KNOWN_HOSTS)
                .map_err(|e| format!("the known hosts could not be written: {e}"))?;
            env.push((
                "GIT_SSH_COMMAND".to_owned(),
                format!(
                    "ssh -i {} -o IdentitiesOnly=yes -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile={}",
                    shell_word(&key.to_string_lossy()),
                    shell_word(&known.to_string_lossy())
                ),
            ));
        }
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        // The refspec carries no `+` and the command no `--force`: a branch the fork has moved
        // is refused by git, and the owner hears why.
        let refspec = format!("{kept}:refs/heads/{branch}");
        git(&self.dir, &["push", "--no-tags", remote, &refspec], &env)?;
        Ok(format!(
            "pushed {branch} ({}) to {}",
            &stored[..12.min(stored.len())],
            remote
        ))
    }
}

/// A path as one word for `sh`, which `GIT_SSH_COMMAND` is run through.
fn shell_word(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// A diff held to [`SHOWN_MOST`] bytes on a line's edge, saying so when it is cut.
#[must_use]
pub fn shown(diff: &[u8]) -> String {
    let whole = String::from_utf8_lossy(diff);
    if whole.len() <= SHOWN_MOST {
        return whole.into_owned();
    }
    let mut end = SHOWN_MOST;
    while !whole.is_char_boundary(end) {
        end -= 1;
    }
    let kept = whole[..end]
        .rsplit_once('\n')
        .map_or(&whole[..end], |(k, _)| k);
    format!(
        "{kept}\n[cut: the whole diff is {} KB; the stored bundle has all of it]",
        whole.len() >> 10
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> String {
        text(git(dir, args, &[]).unwrap_or_else(|e| panic!("git {args:?}: {e}")))
    }

    fn commit(dir: &Path, message: &str, author: &str) {
        git(
            dir,
            &[
                "-c",
                &format!("user.name={}", author.split(" <").next().unwrap()),
                "-c",
                &format!(
                    "user.email={}",
                    author.split(" <").nth(1).unwrap().trim_end_matches('>')
                ),
                "commit",
                "-q",
                "-m",
                message,
            ],
            &[],
        )
        .unwrap();
    }

    const HQ: &str = "Endora HQ <hq@endora.invalid>";
    const TRAILED: &str = "tidy the deluge role\n\nMade-by: Endora HQ (Rae) for task #3";

    /// A fresh scene: the fork as a bare repo with `main`, a mirror of it, and a working clone
    /// of the mirror's bundle, as the sandbox gets one.
    struct Scene {
        root: PathBuf,
        fork: PathBuf,
        mirror: Mirror,
        work: PathBuf,
    }

    fn scene(name: &str) -> Scene {
        let root = std::env::temp_dir().join(format!(
            "endora-repo-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let fork = root.join("fork.git");
        run(&root, &["init", "-q", "--bare", "-b", "main", "fork.git"]);
        let seed = root.join("seed");
        run(&root, &["clone", "-q", fork.to_str().unwrap(), "seed"]);
        std::fs::write(seed.join("nas.yml"), "---\n- hosts: all\n").unwrap();
        std::fs::create_dir_all(seed.join("roles/deluge")).unwrap();
        std::fs::write(seed.join("roles/deluge/README.md"), "# deluge\n").unwrap();
        run(&seed, &["add", "."]);
        commit(&seed, "the lab", "John Doe <john@example.com>");
        run(&seed, &["push", "-q", "origin", "HEAD:main"]);
        let mirror = Mirror::new(&root.join("data"), fork.to_str().unwrap(), "main");
        let bundle = mirror.bundle(None, 0).unwrap();
        assert_eq!(bundle.branch, "main");
        let file = root.join("checkout.bundle");
        std::fs::write(&file, &bundle.bytes).unwrap();
        run(
            &root,
            &["clone", "-q", "-b", "main", file.to_str().unwrap(), "work"],
        );
        let work = root.join("work");
        run(&work, &["remote", "remove", "origin"]);
        Scene {
            root,
            fork,
            mirror,
            work,
        }
    }

    impl Scene {
        /// A branch of one commit in the working clone, changing `file`, authored `author`.
        fn branch(&self, name: &str, file: &str, message: &str, author: &str) {
            run(&self.work, &["checkout", "-q", "-B", name, "main"]);
            std::fs::write(self.work.join(file), format!("# {name}\n")).unwrap();
            run(&self.work, &["add", "."]);
            commit(&self.work, message, author);
        }

        fn delta(&self, name: &str) -> Vec<u8> {
            let out = self.root.join("delta.bundle");
            run(
                &self.work,
                &[
                    "bundle",
                    "create",
                    out.to_str().unwrap(),
                    &format!("main..{name}"),
                ],
            );
            std::fs::read(out).unwrap()
        }
    }

    #[test]
    fn a_branch_is_named_for_its_task_under_hq_and_nothing_else_passes() {
        assert_eq!(slug("Tidy the Deluge role!"), "tidy-the-deluge-role");
        assert_eq!(slug("  --  "), "work");
        assert_eq!(slug(&"long ".repeat(20)).len(), SLUG_MOST - 1);
        assert_eq!(
            branch_for(12, "Plan the clean-up: read the fork's docs"),
            "hq/task-12-plan-the-clean-up-read-the-fork-s-docs"
        );
        assert!(hq_branch(&branch_for(12, "x")).is_ok());
        assert_eq!(hq_branch("refs/heads/hq/task-1-a"), Ok("hq/task-1-a"));
        for refused in [
            "main",
            "master",
            "hq",
            "hq/",
            "feature/x",
            "+hq/task-1-a",
            "hq/../main",
            "hq/task 1",
            "refs/tags/hq/x",
            "hq/.hidden",
        ] {
            assert!(hq_branch(refused).is_err(), "{refused}");
        }
        assert!(plain_remote("git@github.com:owner/ansible-nas.git").is_ok());
        assert!(plain_remote("https://github.com/owner/ansible-nas.git").is_ok());
        assert!(plain_remote("/tmp/fork.git").is_ok());
        assert!(plain_remote("https://user:token@example.com/owner/x.git").is_err());
        assert!(plain_remote("").is_err());
    }

    #[test]
    fn githubs_host_keys_are_the_three_github_publishes_in_shape() {
        let lines: Vec<&str> = GITHUB_KNOWN_HOSTS.lines().collect();
        assert_eq!(lines.len(), 3);
        for (line, kind) in lines
            .iter()
            .zip(["ssh-ed25519", "ecdsa-sha2-nistp256", "ssh-rsa"])
        {
            let mut parts = line.split(' ');
            assert_eq!(parts.next(), Some("github.com"));
            assert_eq!(parts.next(), Some(kind));
            let key = parts.next().unwrap();
            assert!(
                key.len() > 40
                    && key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='),
                "{kind}"
            );
        }
        // A live comparison with https://api.github.com/meta is not run here (no network).
    }

    #[test]
    fn the_mirror_hands_out_the_default_branch_and_refreshes_at_most_every_five_minutes() {
        let s = scene("bundle");
        let before = s.mirror.head_of("main").unwrap();
        // The fork moves on; within five minutes the mirror does not look.
        let seed = s.root.join("seed");
        std::fs::write(seed.join("new.yml"), "---\n").unwrap();
        run(&seed, &["add", "."]);
        commit(&seed, "more", "John Doe <john@example.com>");
        run(&seed, &["push", "-q", "origin", "HEAD:main"]);
        assert_eq!(s.mirror.bundle(None, REFRESH_MS - 1).unwrap().head, before);
        let after = s.mirror.bundle(Some("main"), REFRESH_MS).unwrap();
        assert_ne!(after.head, before, "five minutes on, it fetched");
        assert!(after.bytes.len() > 100);
        // Only the default branch or a team branch is handed out.
        let refused = s.mirror.bundle(Some("develop"), REFRESH_MS).unwrap_err();
        assert!(refused.contains("hq/"), "{refused}");
        let missing = s
            .mirror
            .bundle(Some("hq/task-9-none"), REFRESH_MS)
            .unwrap_err();
        assert!(missing.contains("no branch"), "{missing}");
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn a_submitted_branch_is_taken_when_it_sits_on_main_and_is_hqs_own() {
        let s = scene("submit");
        s.branch("hq/task-3-tidy", "roles/deluge/README.md", TRAILED, HQ);
        let taken = s
            .mirror
            .submit(&s.delta("hq/task-3-tidy"), "hq/task-3-tidy", 3, 0)
            .unwrap();
        assert_eq!(taken.branch, "hq/task-3-tidy");
        assert_eq!((taken.files, taken.plus, taken.minus), (1, 1, 1));
        assert_eq!(taken.title(), "branch hq/task-3-tidy (1 file, +1 −1)");
        assert!(taken.stat.contains("README.md"), "{}", taken.stat);
        assert!(taken.diff.contains("+# hq/task-3-tidy"), "{}", taken.diff);
        assert!(taken.path.exists() && taken.path.to_string_lossy().contains("task-3"));
        assert_eq!(
            s.mirror.stored(3, "hq/task-3-tidy", &taken.head),
            Some(taken.path.clone())
        );
        assert_eq!(
            run(
                &s.mirror.dir,
                &["rev-parse", "refs/submissions/hq/task-3-tidy"]
            ),
            taken.head
        );
        // Submitted again with one more commit: the stored branch moves with it.
        std::fs::write(s.work.join("roles/deluge/x.yml"), "---\n").unwrap();
        run(&s.work, &["add", "."]);
        commit(&s.work, TRAILED, HQ);
        let again = s
            .mirror
            .submit(&s.delta("hq/task-3-tidy"), "hq/task-3-tidy", 3, 1)
            .unwrap();
        assert_eq!(again.files, 2);
        assert_ne!(again.head, taken.head);
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn a_submission_is_refused_off_hq_off_main_too_big_or_not_hqs_own() {
        let s = scene("refuse");
        // Not under hq/.
        s.branch("feature/x", "a.yml", TRAILED, HQ);
        let why = s
            .mirror
            .submit(&s.delta("feature/x"), "feature/x", 3, 0)
            .unwrap_err();
        assert!(why.contains("hq/<task-N>-<slug>"), "{why}");
        // The bundle holds another branch than the one named.
        s.branch("hq/task-3-a", "a.yml", TRAILED, HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-a"), "hq/task-3-b", 3, 0)
            .unwrap_err();
        assert!(why.contains("exactly one branch"), "{why}");
        // Not authored by HQ, or without the trailer.
        s.branch(
            "hq/task-3-who",
            "a.yml",
            TRAILED,
            "Jane Doe <jane@example.com>",
        );
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-who"), "hq/task-3-who", 3, 0)
            .unwrap_err();
        assert!(why.contains("authored by"), "{why}");
        s.branch("hq/task-3-bare", "a.yml", "no trailer here", HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-bare"), "hq/task-3-bare", 3, 0)
            .unwrap_err();
        assert!(why.contains("trailer"), "{why}");
        // Too many files.
        run(&s.work, &["checkout", "-q", "-B", "hq/task-3-many", "main"]);
        std::fs::create_dir_all(s.work.join("many")).unwrap();
        for i in 0..=FILES_MOST {
            std::fs::write(s.work.join(format!("many/{i}.yml")), "---\n").unwrap();
        }
        run(&s.work, &["add", "."]);
        commit(&s.work, TRAILED, HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-many"), "hq/task-3-many", 3, 0)
            .unwrap_err();
        assert!(why.contains("201 files"), "{why}");
        // Too big a diff.
        run(&s.work, &["checkout", "-q", "-B", "hq/task-3-big", "main"]);
        std::fs::write(s.work.join("big.txt"), "x".repeat(DIFF_MOST + 1)).unwrap();
        run(&s.work, &["add", "."]);
        commit(&s.work, TRAILED, HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-big"), "hq/task-3-big", 3, 0)
            .unwrap_err();
        assert!(why.contains("KB"), "{why}");
        // Nothing beyond main.
        run(&s.work, &["checkout", "-q", "-B", "hq/task-3-same", "main"]);
        let out = s.root.join("same.bundle");
        run(
            &s.work,
            &["bundle", "create", out.to_str().unwrap(), "hq/task-3-same"],
        );
        let why = s
            .mirror
            .submit(&std::fs::read(out).unwrap(), "hq/task-3-same", 3, 0)
            .unwrap_err();
        assert!(why.contains("no commits beyond"), "{why}");
        // Garbage, and nothing.
        assert!(
            s.mirror
                .submit(b"not a bundle", "hq/task-3-x", 3, 0)
                .unwrap_err()
                .contains("does not apply")
        );
        assert!(s.mirror.submit(b"", "hq/task-3-x", 3, 0).is_err());
        assert!(
            s.mirror.stored(3, "hq/task-3-a", "0000000").is_none(),
            "nothing refused is kept"
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn a_branch_behind_the_forks_main_is_sent_back_to_rebase() {
        let s = scene("behind");
        s.branch("hq/task-4-old", "a.yml", TRAILED, HQ);
        let delta = s.delta("hq/task-4-old");
        // The fork's main moves on before the submission arrives, and the mirror sees it.
        let seed = s.root.join("seed");
        std::fs::write(seed.join("nas.yml"), "---\n- hosts: nas\n").unwrap();
        run(&seed, &["add", "."]);
        commit(
            &seed,
            "the owner's own change",
            "John Doe <john@example.com>",
        );
        run(&seed, &["push", "-q", "origin", "HEAD:main"]);
        let why = s
            .mirror
            .submit(&delta, "hq/task-4-old", 4, REFRESH_MS)
            .unwrap_err();
        assert!(why.contains("Rebase it on main"), "{why}");
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn a_stored_branch_is_pushed_as_itself_and_never_main_force_or_a_stranger() {
        let s = scene("push");
        s.branch("hq/task-5-push", "a.yml", TRAILED, HQ);
        let taken = s
            .mirror
            .submit(&s.delta("hq/task-5-push"), "hq/task-5-push", 5, 0)
            .unwrap();
        let fork = s.fork.to_str().unwrap();
        let said = s
            .mirror
            .push("hq/task-5-push", &taken.head, fork, None)
            .unwrap();
        assert!(said.starts_with("pushed hq/task-5-push ("), "{said}");
        assert_eq!(
            run(&s.fork, &["rev-parse", "refs/heads/hq/task-5-push"]),
            taken.head
        );
        assert_eq!(
            run(&s.fork, &["rev-parse", "refs/heads/main"]),
            taken.base,
            "main untouched"
        );
        // Pushed again as it is: nothing to do, no error. Moved on the fork since: refused,
        // since HQ never forces.
        s.mirror
            .push("hq/task-5-push", &taken.head, fork, None)
            .unwrap();
        let other = s.root.join("other");
        run(
            &s.root,
            &["clone", "-q", "-b", "hq/task-5-push", fork, "other"],
        );
        std::fs::write(other.join("b.yml"), "---\n").unwrap();
        run(&other, &["add", "."]);
        commit(
            &other,
            "the owner's own commit on the branch",
            "John Doe <john@example.com>",
        );
        run(&other, &["push", "-q", "origin", "HEAD:hq/task-5-push"]);
        s.branch("hq/task-5-push", "c.yml", TRAILED, HQ);
        let moved = s
            .mirror
            .submit(&s.delta("hq/task-5-push"), "hq/task-5-push", 5, 1)
            .unwrap();
        let why = s
            .mirror
            .push("hq/task-5-push", &moved.head, fork, None)
            .unwrap_err();
        assert!(why.contains("push") && why.contains("failed"), "{why}");
        // The refusals that need no git: not hq/, main, a force, a head not stored, a
        // credential in the remote, and SSH with no key.
        for (branch, head) in [
            ("main", taken.head.as_str()),
            ("+hq/task-5-push", taken.head.as_str()),
            ("hq/task-9-none", "abcdef0"),
            ("hq/task-5-push", "0000000"),
        ] {
            let why = s.mirror.push(branch, head, fork, None).unwrap_err();
            assert!(!why.contains("failed"), "{branch}: {why}");
        }
        assert!(
            s.mirror
                .push(
                    "hq/task-5-push",
                    &moved.head,
                    "https://u:p@example.com/o/r.git",
                    None
                )
                .unwrap_err()
                .contains("credential")
        );
        let why = s
            .mirror
            .push(
                "hq/task-5-push",
                &moved.head,
                "git@github.com:owner/ansible-nas.git",
                None,
            )
            .unwrap_err();
        assert!(why.contains("ENDORA_GIT_PUSH_KEY_FILE"), "{why}");
        let _ = std::fs::remove_dir_all(&s.root);
    }

    #[test]
    fn a_long_diff_is_cut_on_a_line_and_says_so() {
        let diff: Vec<u8> = (0..3000)
            .flat_map(|i| format!("line {i} {}\n", "x".repeat(30)).into_bytes())
            .collect();
        let cut = shown(&diff);
        assert!(cut.len() <= SHOWN_MOST + 100);
        assert!(
            cut.contains("\n[cut: the whole diff is"),
            "{}",
            &cut[cut.len() - 100..]
        );
        assert_eq!(shown(b"short"), "short");
        assert_eq!(shell_word("/a b/it's"), "'/a b/it'\\''s'");
    }
}
