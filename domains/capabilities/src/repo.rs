//! The engineering wing's git (ADR 0115): how the team's code reaches the owner's fork of
//! ansible-nas, and nothing else. The node keeps a bare mirror of the fork under its data
//! folder, refreshed over plain HTTPS without a credential, and hands a turn's sandbox a bundle
//! of a branch to work on (`repo_checkout`): the fork's, or the team's own kept submission,
//! which only HQ's record says a turn may have. The sandbox sends its work back as a bundle of
//! one `hq/task-<N>-*` branch (`repo_submit`), which is accepted only when it is named for its
//! task, applies on top of the fork's
//! default branch, is small, and every commit is HQ's own. The owner alone lets a branch leave
//! the house: on their approval the node pushes the stored submission to the fork with a
//! deploy key, never with force, never to the default branch. HQ opens no pull request and
//! merges nothing.
//!
//! Upstream ansible-nas is kept beside the fork, read-only (ADR 0106, amended 2026-10-31): its
//! default branch is fetched into `refs/upstream/<branch>` on the fork's schedule and carried in
//! every checkout, so an offline sandbox can compare the two. It is never a base and never a
//! push target: no remote names it, and a branch named for it or carrying its commits is refused.
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
/// The most of one file at a submission's head handed to HQ's mechanical checks, in bytes: a
/// longer one is not handed over, never cut.
pub const CHECKED_TEXT_MOST: usize = 32 * 1024;
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
/// push trusts GitHub and nothing that answers in its place. Checked in shape by a test, and
/// compared by hand with `/meta` on 2026-10-17 (the RSA key had been wrong).
pub const GITHUB_KNOWN_HOSTS: &str = "\
github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl
github.com ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=
github.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6+PKCUXaDbC7qtbW8gIkhL7aGCsOr/C56SJMy/BCZfxd1nWzAOxSDPgVsmerOBYfNqltV9/hWCqBywINIR+5dIg6JTJ72pcEpEjcYgXkE2YEFXV1JHnsKgbLWNlhScqb2UmyRkQyytRLtL+38TGxkxCflmO+5Z8CSSNY7GidjMIZ7Q4zMjA2n1nGrlTDkzwDCsw+wqFPGQA179cnfGWOWRVruj16z6XyvxvjJwbz0wQZ75XK5tKSb7FNyeIEs4TT4jk+S4dhPeAUC5y+bDYirYgM4GC7uEnztnZyaVWQ7B381AK4Qdrwt51ZqExKbQpTUNn+EjqoTwvqNj4kqx5QUCI0ThS/YkOxJCXmPUWZbhjpCg56i+2aB6CmK2JGhn57K5mj0MNdBXA4/WnwH6XoPWJzK5Nyu2zB3nAZp+S5hpQs+p1vN1/wsjk=
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

pub use endora_kernel::lab::forbidden_path;

/// The files at a submission's head HQ's mechanical checks read (ADR 0142, amended
/// 2026-10-30): each role README the branch changes (`roles/x/README.md`) and that role's
/// `defaults/main.yml`, each once, and never a forbidden path.
#[must_use]
pub fn read_for_checks(changed: &[String]) -> Vec<String> {
    let mut wanted: Vec<String> = Vec::new();
    for file in changed {
        let Some((dir, name)) = file.rsplit_once('/') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("readme.md") || !dir.starts_with("roles/") {
            continue;
        }
        for path in [file.clone(), format!("{dir}/defaults/main.yml")] {
            if !forbidden_path(&path) && !wanted.contains(&path) {
                wanted.push(path);
            }
        }
    }
    wanted
}

/// Where GitHub keeps a repository's Actions workflows.
pub const WORKFLOWS_DIR: &str = ".github/workflows/";

/// The most workflows a submission hands HQ.
pub const WORKFLOWS_MOST: usize = 12;

/// The workflows at a submission's head HQ's check of a CI criterion reads (ADR 0142, amended
/// 2026-10-31), from `git ls-tree -z --name-only` of [`WORKFLOWS_DIR`]: its `.yml` and `.yaml`
/// files, never a forbidden path, at most [`WORKFLOWS_MOST`].
#[must_use]
pub fn workflow_paths(listing: &str) -> Vec<String> {
    listing
        .split('\0')
        .filter(|p| p.starts_with(WORKFLOWS_DIR))
        .filter(|p| {
            let p = p.to_ascii_lowercase();
            p.ends_with(".yml") || p.ends_with(".yaml")
        })
        .filter(|p| !forbidden_path(p))
        .take(WORKFLOWS_MOST)
        .map(str::to_owned)
        .collect()
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

/// A branch name as a turn on `task` may submit it: `hq/task-<task>-<slug>`, a plain name under
/// `hq/` that begins with its own task. A submission is kept as `refs/submissions/<name>`, so a
/// name that carries its task is a key no other task can write: a later submission of the
/// same name from another task used to overwrite the kept branch (the push was then refused,
/// or the check's bundle gone). A sub-task that goes on with its parent's branch submits it
/// under its own name.
///
/// # Errors
/// Why not, as the model reads it.
pub fn task_branch(name: &str, task: i64) -> Result<&str, String> {
    let bare = hq_branch(name)?;
    let prefix = format!("{BRANCH_PREFIX}task-{task}-");
    if bare.len() > prefix.len() && bare.starts_with(&prefix) {
        return Ok(bare);
    }
    Err(format!(
        "this turn is on task #{task}: its branch is named hq/task-{task}-<slug>, e.g. {}; not {name:?}",
        branch_for(task, "tidy roles")
    ))
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

/// Whether `name` names upstream ansible-nas rather than a branch of the fork: `upstream`,
/// `upstream/<branch>`, or that under `refs/`, `refs/heads/` or `refs/remotes/`. Upstream is
/// read-only (ADR 0106, amended 2026-10-31): never checked out alone, submitted or pushed.
#[must_use]
pub fn names_upstream(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    let bare = ["refs/remotes/", "refs/heads/", "refs/"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    bare == "upstream" || bare.starts_with("upstream/")
}

/// The one rule of which repositories outside the lab may be named: a public
/// `https://github.com/<owner>/<name>`, the kernel's, which HQ holds the owner to as well.
pub use endora_kernel::lab::outside_repo;

/// Why a branch of a repository outside the lab is not pushed or merged (ADR 0115, amended
/// 2026-11-01).
pub const OUTSIDE_KEPT: &str = "a branch of a repository outside the lab is kept on the node for \
     judging and is never pushed anywhere";

/// The default branch of the repository at `url`, asked of it (`git ls-remote --symref`), with
/// `dir` as git's working folder.
///
/// # Errors
/// What git said, or a repository that names no plain default branch.
pub fn default_branch(dir: &Path, url: &str) -> Result<String, String> {
    plain_remote(url)?;
    let said = text(git(dir, &["ls-remote", "--symref", url, "HEAD"], &[])?);
    said.lines()
        .find_map(|l| l.strip_prefix("ref: refs/heads/"))
        .and_then(|r| r.split('\t').next())
        .filter(|b| crate::ansible_nas::plain_branch(b))
        .map(str::to_owned)
        .ok_or_else(|| "the repository names no default branch".to_owned())
}

/// A remote's address as one key, so the same repository is known by any of its addresses:
/// GitHub's HTTPS and SSH forms of `owner/name` are the same key; any other address is itself.
fn repo_key(url: &str) -> String {
    let lower = url.trim().to_ascii_lowercase();
    let bare = [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
        "git://github.com/",
    ]
    .iter()
    .find_map(|p| lower.strip_prefix(p))
    .unwrap_or(&lower)
    .trim_end_matches('/');
    bare.strip_suffix(".git").unwrap_or(bare).to_owned()
}

/// Whether two remotes' addresses name the same repository.
#[must_use]
pub fn same_repo(a: &str, b: &str) -> bool {
    repo_key(a) == repo_key(b)
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
    /// Upstream's default branch, carried in the bundle as `refs/upstream/<branch>` to be read
    /// in the sandbox as `upstream/<branch>`; `None` when the node has not fetched it.
    pub upstream: Option<Upstream>,
}

/// Upstream ansible-nas's default branch as the node last fetched it (ADR 0106, amended
/// 2026-10-31): read-only, never a base and never a push target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    /// Upstream's default branch, e.g. `master`.
    pub branch: String,
    /// Its head.
    pub head: String,
}

impl Upstream {
    /// What the sandbox calls it: `upstream/<branch>`.
    #[must_use]
    pub fn named(&self) -> String {
        format!("upstream/{}", self.branch)
    }
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
    /// Every file it changes (`git diff --name-only`).
    pub changed: Vec<String>,
    /// `(path, text)` at its head of what [`read_for_checks`] names, for HQ's mechanical
    /// checks: what exists there and fits [`CHECKED_TEXT_MOST`].
    pub texts: Vec<(String, String)>,
    /// `(path, text)` at its head of each workflow [`workflow_paths`] names, for HQ's check of
    /// a CI criterion; one that cannot be read or does not fit [`CHECKED_TEXT_MOST`] is handed
    /// over empty, so HQ cannot tell rather than think it is not there.
    pub workflows: Vec<(String, String)>,
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
    base: String,
    upstream: Option<String>,
    /// A repository outside the lab (ADR 0115, amended 2026-11-01): its branches are kept for
    /// judging and never pushed or merged anywhere.
    outside: bool,
    refreshed: Mutex<Option<i64>>,
}

impl Mirror {
    /// The mirror of the repository at `url`, worked from `base`, kept under
    /// `data_dir/team-repo`.
    #[must_use]
    pub fn new(data_dir: &Path, url: &str, base: &str) -> Self {
        let root = data_dir.join("team-repo");
        Self {
            dir: root.join("mirror.git"),
            submissions: root.join("submissions"),
            url: url.to_owned(),
            base: base.to_owned(),
            upstream: None,
            outside: false,
            refreshed: Mutex::new(None),
        }
    }

    /// The mirror of a repository outside the lab at `url`, worked from `base` (its default
    /// branch), kept under `data_dir/outside-repos/<its address as a folder name>` beside the
    /// fork's (ADR 0115, amended 2026-11-01: round 4's F4). Checked out and submitted to as the
    /// fork is, with no upstream; what is submitted stays in it, since [`Self::push`] and
    /// [`Self::merge_into_base`] refuse every branch of it. The caller has held `url` to
    /// `endora_kernel::lab::outside_repo`.
    #[must_use]
    pub fn outside(data_dir: &Path, url: &str, base: &str) -> Self {
        let folder: String = repo_key(url)
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let root = data_dir.join("outside-repos").join(folder);
        Self {
            dir: root.join("mirror.git"),
            submissions: root.join("submissions"),
            url: url.trim().to_owned(),
            base: base.to_owned(),
            upstream: None,
            outside: true,
            refreshed: Mutex::new(None),
        }
    }

    /// Whether this is the mirror of a repository outside the lab, whose branches stay here.
    #[must_use]
    pub fn is_outside(&self) -> bool {
        self.outside
    }

    /// With upstream at `url` kept beside the fork, read-only (ADR 0106, amended 2026-10-31):
    /// its default branch is fetched into `refs/upstream/<branch>` on the fork's schedule, and
    /// every checkout carries it. An upstream that is the fork itself is not kept twice.
    #[must_use]
    pub fn with_upstream(mut self, url: &str) -> Self {
        if !url.trim().is_empty() && !same_repo(url, &self.url) {
            self.upstream = Some(url.trim().to_owned());
        }
        self
    }

    /// Upstream's default branch as last fetched, if it has been.
    #[must_use]
    pub fn upstream(&self) -> Option<Upstream> {
        let said = git(
            &self.dir,
            &[
                "for-each-ref",
                "--format=%(refname:strip=2) %(objectname)",
                "refs/upstream/",
            ],
            &[],
        )
        .map(text)
        .ok()?;
        let (branch, head) = said.lines().next()?.split_once(' ')?;
        Some(Upstream {
            branch: branch.to_owned(),
            head: head.to_owned(),
        })
    }

    /// Fetches upstream's default branch (asked of upstream each time, so a renamed default is
    /// followed) into `refs/upstream/<branch>`, the only ref kept under `refs/upstream/`. No
    /// remote is configured for it: there is nothing named to push to.
    fn fetch_upstream(&self, url: &str) -> Result<(), String> {
        plain_remote(url)?;
        let said = text(git(
            &self.dir,
            &["ls-remote", "--symref", url, "HEAD"],
            &[],
        )?);
        let branch = said
            .lines()
            .find_map(|l| l.strip_prefix("ref: refs/heads/"))
            .and_then(|r| r.split('\t').next())
            .filter(|b| crate::ansible_nas::plain_branch(b))
            .ok_or_else(|| "upstream names no default branch".to_owned())?;
        let kept = format!("refs/upstream/{branch}");
        git(
            &self.dir,
            &[
                "fetch",
                "--no-tags",
                url,
                &format!("+refs/heads/{branch}:{kept}"),
            ],
            &[],
        )?;
        let refs = text(git(
            &self.dir,
            &["for-each-ref", "--format=%(refname)", "refs/upstream/"],
            &[],
        )?);
        for old in refs.lines().filter(|r| *r != kept) {
            git(&self.dir, &["update-ref", "-d", old], &[])?;
        }
        Ok(())
    }

    /// The base branch the wing works from: `ENDORA_ANSIBLE_NAS_REF` when the owner set it
    /// (the fork's own line, `develop`, with `main` only mirroring upstream), else the fork's
    /// default branch. A checkout is a bundle of it, a submission must sit on top of it, and
    /// `hq/*` branches are cut from it.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Where the mirror is.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Clones the fork as a bare mirror, or fetches what changed, at most once in
    /// [`REFRESH_MS`]; the first call makes it. Upstream, when kept, is fetched with it, and an
    /// upstream that cannot be reached leaves what was last fetched (or nothing) without
    /// failing the fork's refresh.
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
            // The fork's branches only: a mirror's own refspec (`refs/*`) would prune the
            // submissions kept under refs/submissions/ on the next fetch.
            git(
                &self.dir,
                &[
                    "config",
                    "remote.origin.fetch",
                    "+refs/heads/*:refs/heads/*",
                ],
                &[],
            )?;
        }
        if let Some(url) = &self.upstream {
            if let Err(why) = self.fetch_upstream(url) {
                eprintln!("team repo: upstream ansible-nas was not fetched: {why}");
            }
        }
        *last = Some(now);
        Ok(())
    }

    /// The bundle's refs: `refs`, and upstream's when it is kept, with what upstream is.
    fn with_upstream_ref(&self, mut refs: Vec<String>) -> (Vec<String>, Option<Upstream>) {
        let upstream = self.upstream();
        if let Some(up) = &upstream {
            refs.push(format!("refs/upstream/{}", up.branch));
        }
        (refs, upstream)
    }

    /// The whole history of a stored submission, as one bundle the check's sandbox can fetch
    /// from (a submitted bundle alone holds only the branch's own commits). `None` when no
    /// submission of `branch` at `head` is stored.
    ///
    /// # Errors
    /// Git failing.
    pub fn submission_bundle(&self, branch: &str, head: &str) -> Result<Option<Vec<u8>>, String> {
        let branch = hq_branch(branch)?;
        let kept = format!("refs/submissions/{branch}");
        let Ok(stored) = git(
            &self.dir,
            &["rev-parse", "--verify", &format!("{kept}^{{commit}}")],
            &[],
        )
        .map(text) else {
            return Ok(None);
        };
        if head.trim().len() < 7 || !stored.starts_with(head.trim()) {
            return Ok(None);
        }
        let bytes = git(&self.dir, &["bundle", "create", "-", &kept], &[])?;
        Ok(Some(bytes))
    }

    /// The newest stored submission bundle of `branch` for `task`, whatever its head: what the
    /// owner downloads when the node cannot push.
    #[must_use]
    pub fn stored_latest(&self, task: i64, branch: &str) -> Option<PathBuf> {
        let name = branch.rsplit('/').next().unwrap_or(branch);
        let folder = self.submissions.join(format!("task-{task}"));
        let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(folder)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with(&format!("{name}-")) && n.ends_with(".bundle"))
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        found.sort();
        found.pop().map(|(_, p)| p)
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
    /// team's under `hq/`, with its whole history, at most [`BUNDLE_MOST`]. Upstream's default
    /// branch comes with it as `refs/upstream/<branch>`, when the node keeps it.
    ///
    /// # Errors
    /// A branch that is neither (upstream among them), one the fork does not have, or git
    /// failing.
    pub fn bundle(&self, branch: Option<&str>, now: i64) -> Result<Bundle, String> {
        let branch = match branch.map(str::trim).filter(|b| !b.is_empty()) {
            None => self.base.clone(),
            Some(named) if named == self.base => named.to_owned(),
            Some(named) if names_upstream(named) => {
                let _ = self.refresh(now);
                let upstream = self
                    .upstream()
                    .map_or_else(|| "upstream/<its default branch>".to_owned(), |u| u.named());
                return Err(format!(
                    "upstream ansible-nas is not checked out on its own: every checkout of {base} carries it as {upstream}, read-only. Check out {base} and read it there, e.g. git log --oneline {base}..{upstream}",
                    base = self.base
                ));
            }
            Some(named) => hq_branch(named)
                .map_err(|_| {
                    format!(
                        "check out the fork's {} or one of the team's hq/ branches, not {named:?}",
                        self.base
                    )
                })?
                .to_owned(),
        };
        self.refresh(now)?;
        let head = self.head_of(&branch)?;
        let (refs, upstream) = self.with_upstream_ref(vec![format!("refs/heads/{branch}")]);
        let mut args = vec!["bundle", "create", "-"];
        args.extend(refs.iter().map(String::as_str));
        let bytes = git(&self.dir, &args, &[])?;
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
            upstream,
        })
    }

    /// The team's own submission of `branch`, as a checkout hands it back (ADR 0115, amended).
    /// A submission is kept as `refs/submissions/<branch>` and reaches the fork's heads only
    /// when the owner's approved push lands, so [`Self::bundle`] cannot hand it out. This is one
    /// bundle of the default branch and the stored submission, whole history behind each: the
    /// sandbox clones the first and fetches the second as a head of its own, so what it makes
    /// of it can be submitted again. Only at `head`, the one HQ's record names: a branch
    /// submitted again since is refused, never swapped for the newer. The caller has asked HQ
    /// whether this turn may have it; the node decides nothing about whose it is.
    ///
    /// # Errors
    /// Not under `hq/`; no stored submission; a head that is not the stored one; git failing; a
    /// bundle past [`BUNDLE_MOST`].
    pub fn kept_bundle(&self, branch: &str, head: &str, now: i64) -> Result<Bundle, String> {
        let branch = hq_branch(branch)?;
        // The work is on the node: a fork that cannot be reached now leaves the base as it was
        // last fetched, and does not take the checkout with it.
        if let Err(why) = self.refresh(now) {
            if !self.dir.join("HEAD").exists() {
                return Err(why);
            }
        }
        let kept = format!("refs/submissions/{branch}");
        let stored = git(
            &self.dir,
            &["rev-parse", "--verify", &format!("{kept}^{{commit}}")],
            &[],
        )
        .map(text)
        .map_err(|_| format!("no submission of {branch} is stored"))?;
        let head = head.trim();
        if head.len() < 7 {
            return Err("name the head to check out: at least seven hex digits of it".to_owned());
        }
        if !stored.starts_with(head) {
            return Err(format!(
                "{branch} was submitted again: HQ recorded {} and the node keeps {}. Ask again",
                &head[..12.min(head.len())],
                &stored[..12.min(stored.len())]
            ));
        }
        self.head_of(&self.base)?;
        let (refs, upstream) =
            self.with_upstream_ref(vec![format!("refs/heads/{}", self.base), kept]);
        let mut args = vec!["bundle", "create", "-"];
        args.extend(refs.iter().map(String::as_str));
        let bytes = git(&self.dir, &args, &[])?;
        if bytes.len() > BUNDLE_MOST {
            return Err(format!(
                "the bundle of {branch} is {} MB, more than the {} MB a checkout may be",
                bytes.len() >> 20,
                BUNDLE_MOST >> 20
            ));
        }
        Ok(Bundle {
            branch: branch.to_owned(),
            head: stored,
            bytes,
            upstream,
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
    /// named for the task (`hq/task-<task>-<slug>`), apply on top of the mirror's default
    /// branch (a fast-forward), change at
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
        if names_upstream(branch) {
            return Err(format!(
                "upstream ansible-nas is read-only: nothing is submitted or pushed to it. Work on hq/task-{task}-<slug> from {}, and make what you take from upstream your own commits",
                self.base
            ));
        }
        let branch = task_branch(branch, task)?.to_owned();
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
                self.base
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
        // The submission is staged apart from the kept ref and moved there only once every rule
        // has passed: a refused attempt (a missing trailer was the commonest live) must leave
        // the branch the node keeps as it was, since the push and the checkout of a kept
        // branch are bound to its head.
        let staged = format!("refs/incoming/{branch}");
        git(
            &self.dir,
            &["fetch", "--no-tags", &file, &format!("+{wanted}:{staged}")],
            &[],
        )?;
        let vetted = self.vet(&staged, incoming, branch, task);
        let kept = vetted.as_ref().map_or(Ok(()), |taken| {
            git(
                &self.dir,
                &[
                    "update-ref",
                    &format!("refs/submissions/{branch}"),
                    &taken.head,
                ],
                &[],
            )
            .map(|_| ())
        });
        let _ = git(&self.dir, &["update-ref", "-d", &staged], &[]);
        kept?;
        vetted
    }

    /// Every rule a staged submission must pass: on top of the default branch, something
    /// beyond it, every commit HQ's own, nothing of the owner's values, a file or more but at
    /// most [`FILES_MOST`], a diff within [`DIFF_MOST`]. What passes has its bundle kept under
    /// the task.
    fn vet(
        &self,
        staged: &str,
        incoming: &Path,
        branch: &str,
        task: i64,
    ) -> Result<Submitted, String> {
        let head = text(git(&self.dir, &["rev-parse", staged], &[])?);
        let base_ref = format!("refs/heads/{}", self.base);
        let base = text(git(&self.dir, &["rev-parse", &base_ref], &[])?);
        if git(
            &self.dir,
            &["merge-base", "--is-ancestor", &base_ref, staged],
            &[],
        )
        .is_err()
        {
            return Err(format!(
                "{branch} is not on top of {}: its head {} is {}. Rebase it on {} and submit again",
                self.base,
                &base[..12.min(base.len())],
                "behind or beside the fork's",
                self.base
            ));
        }
        if head == base {
            return Err(format!(
                "{branch} has no commits beyond {}: nothing to submit",
                self.base
            ));
        }
        let range = format!("{base_ref}..{staged}");
        self.not_upstreams(&range, branch)?;
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
        // Both sides of a move (a rename is listed by its new name alone otherwise), and every
        // name as it is (git quotes one outside ASCII otherwise), so no owner's file is missed.
        let names = text(git(
            &self.dir,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-renames",
                "--name-only",
                "-z",
                &base,
                &head,
            ],
            &[],
        )?);
        let changed: Vec<String> = names
            .split('\0')
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        if let Some(touched) = changed
            .iter()
            .find(|l| endora_kernel::lab::refused_as_owners_values(l, self.outside))
        {
            return Err(format!(
                "{branch} touches {touched}, which holds the owner's own values: never edit, cite or copy from group_vars/*.yml, host_vars or an inventory. Drop that change and submit again"
            ));
        }
        let files = changed.len();
        // Commits that change nothing (an empty one, a change undone) are no branch to review:
        // a probe with no file in it was accepted, checked, and would have been offered.
        if files == 0 {
            return Err(format!(
                "{branch} changes no file beyond {}: nothing to submit",
                self.base
            ));
        }
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
        // A file the head does not have (a role with no defaults) is simply not handed over.
        let texts = read_for_checks(&changed)
            .into_iter()
            .filter_map(|path| {
                let bytes = git(&self.dir, &["show", &format!("{head}:{path}")], &[]).ok()?;
                (bytes.len() <= CHECKED_TEXT_MOST)
                    .then(|| (path, String::from_utf8_lossy(&bytes).into_owned()))
            })
            .collect();
        let listing = git(
            &self.dir,
            &["ls-tree", "-z", "--name-only", &head, WORKFLOWS_DIR],
            &[],
        )
        .map(text)
        .unwrap_or_default();
        let workflows = workflow_paths(&listing)
            .into_iter()
            .map(|path| {
                let bytes =
                    git(&self.dir, &["show", &format!("{head}:{path}")], &[]).unwrap_or_default();
                if bytes.len() > CHECKED_TEXT_MOST {
                    return (path, String::new());
                }
                (path, String::from_utf8_lossy(&bytes).into_owned())
            })
            .collect();
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
            changed,
            texts,
            workflows,
        })
    }

    /// Refuses a branch built on upstream (ADR 0106, amended 2026-10-31): any commit of `range`
    /// that upstream has (a merge of it, a branch cut from it) makes upstream a base, which it
    /// never is. A change read from upstream and made as HQ's own commit is not upstream's.
    fn not_upstreams(&self, range: &str, branch: &str) -> Result<(), String> {
        let Some(up) = self.upstream() else {
            return Ok(());
        };
        let count = |extra: &[&str]| -> Result<i64, String> {
            let mut args = vec!["rev-list", "--count", range];
            args.extend(extra);
            Ok(text(git(&self.dir, &args, &[])?).parse().unwrap_or(0))
        };
        let not_upstream = format!("^refs/upstream/{}", up.branch);
        let carried = count(&[])? - count(&[&not_upstream])?;
        if carried > 0 {
            return Err(format!(
                "{branch} carries upstream's commits ({carried} of {}): upstream ansible-nas is read-only and never a base. Branch from {} and make what you take from upstream your own commits (copy the change, then commit it)",
                up.named(),
                self.base
            ));
        }
        Ok(())
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
        if self.outside {
            return Err(OUTSIDE_KEPT.to_owned());
        }
        if self
            .upstream
            .as_deref()
            .is_some_and(|up| same_repo(up, remote))
        {
            return Err(
                "upstream ansible-nas is read-only: HQ never pushes to it, only to the fork"
                    .to_owned(),
            );
        }
        let branch = hq_branch(branch)?;
        if branch == self.base || branch.starts_with('+') {
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
        let env = self.push_env(remote, key)?;
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

    /// What a push to `remote` runs with: over SSH, the deploy key at `key` and GitHub's own host
    /// keys, nothing else trusted.
    fn push_env(&self, remote: &str, key: Option<&Path>) -> Result<Vec<(String, String)>, String> {
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
        Ok(env)
    }

    /// The stored submission of `branch`, if it is at `head`.
    fn stored_at(&self, branch: &str, head: &str) -> Result<String, String> {
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
                "the stored submission of {branch} is {}, not {head}",
                &stored[..12.min(stored.len())]
            ));
        }
        Ok(stored)
    }

    /// What a stored submission would carry out of the house (ADR 0115, amended 2026-10-31):
    /// its whole diff against where it left the base (no renames, so both sides of a move are
    /// read), and each of its commits' messages, for the privacy scan before a push.
    ///
    /// # Errors
    /// No such submission at `head`, or git failing.
    pub fn outgoing(&self, branch: &str, head: &str) -> Result<Outgoing, String> {
        let branch = hq_branch(branch)?;
        let stored = self.stored_at(branch, head)?;
        let base = format!("refs/heads/{}", self.base);
        let diff = git(
            &self.dir,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-renames",
                "--no-color",
                &format!("{base}...{stored}"),
            ],
            &[],
        )?;
        let log = git(
            &self.dir,
            &["log", "--format=%H%x00%B%x1e", &format!("{base}..{stored}")],
            &[],
        )?;
        let messages = String::from_utf8_lossy(&log)
            .split('\x1e')
            .filter_map(|entry| {
                let (sha, message) = entry.trim_start().split_once('\0')?;
                Some((sha.to_owned(), message.trim().to_owned()))
            })
            .collect();
        Ok(Outgoing {
            diff: String::from_utf8_lossy(&diff).into_owned(),
            messages,
        })
    }

    /// Merges a pushed submission into the fork's base branch (its develop) and pushes the base
    /// (ADR 0115, amended 2026-10-31, behind the owner's setting): a fast-forward when the base
    /// has not moved past where the branch left it, else one merge commit authored by HQ, never
    /// a force. A conflict merges nothing. `main` and `master` are never the target, nor is
    /// upstream. Call after [`Self::refresh`], so the base is the fork's as it stands.
    ///
    /// # Errors
    /// Which rule refused it, a conflict, or what git said.
    pub fn merge_into_base(
        &self,
        branch: &str,
        head: &str,
        remote: &str,
        key: Option<&Path>,
    ) -> Result<String, String> {
        if self.outside {
            return Err(OUTSIDE_KEPT.to_owned());
        }
        if self
            .upstream
            .as_deref()
            .is_some_and(|up| same_repo(up, remote))
        {
            return Err("upstream ansible-nas is read-only: HQ never merges into it".to_owned());
        }
        if matches!(self.base.as_str(), "main" | "master") {
            return Err(format!(
                "{} mirrors upstream: HQ merges into develop only",
                self.base
            ));
        }
        let branch = hq_branch(branch)?;
        plain_remote(remote)?;
        let stored = self.stored_at(branch, head)?;
        let base_ref = format!("refs/heads/{}", self.base);
        let tip = git(&self.dir, &["rev-parse", "--verify", &base_ref], &[]).map(text)?;
        let is_ancestor =
            |a: &str, b: &str| git(&self.dir, &["merge-base", "--is-ancestor", a, b], &[]).is_ok();
        if is_ancestor(&stored, &tip) {
            return Ok(format!(
                "{branch} is already in {} ({})",
                self.base,
                &tip[..12.min(tip.len())]
            ));
        }
        let (target, how) = if is_ancestor(&tip, &stored) {
            (stored.clone(), "fast-forward")
        } else {
            let tree = git(
                &self.dir,
                &["merge-tree", "--write-tree", "--no-messages", &tip, &stored],
                &[],
            )
            .map(text)
            .map_err(|_| {
                format!(
                    "{branch} does not merge cleanly into {}: nothing was merged; merge it on GitHub",
                    self.base
                )
            })?;
            let tree = tree.lines().next().unwrap_or("").to_owned();
            let message = format!("Merge {branch} into {}\n\n{TRAILER} (merge)", self.base);
            let identity = [
                ("GIT_AUTHOR_NAME", AUTHOR_NAME),
                ("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL),
                ("GIT_COMMITTER_NAME", AUTHOR_NAME),
                ("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL),
            ];
            let commit = git(
                &self.dir,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &tip,
                    "-p",
                    &stored,
                    "-m",
                    &message,
                ],
                &identity,
            )
            .map(text)?;
            (commit, "merge commit")
        };
        let env = self.push_env(remote, key)?;
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        // No `+` and no `--force`: a develop that moved since the refresh is refused by git.
        let refspec = format!("{target}:{base_ref}");
        git(&self.dir, &["push", "--no-tags", remote, &refspec], &env)?;
        Ok(format!(
            "merged {branch} into {} ({how}, now {})",
            self.base,
            &target[..12.min(target.len())]
        ))
    }
}

/// What a submission would carry out of the house: its diff and its commits' messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// The unified diff against where it left the base.
    pub diff: String,
    /// `(sha, message)` of each of its commits.
    pub messages: Vec<(String, String)>,
}

/// Bytes as base64, for a bundle carried in JSON.
#[must_use]
pub fn to_base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Bytes from base64, as a bundle arrives in JSON; `None` for text that is not base64.
#[must_use]
pub fn from_base64(text: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(clean).ok()
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
        base: String,
    }

    fn scene(name: &str) -> Scene {
        scene_on(name, "main")
    }

    /// As [`scene`], with the fork's line on `base`.
    fn scene_on(name: &str, base: &str) -> Scene {
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
        run(&root, &["init", "-q", "--bare", "-b", base, "fork.git"]);
        let seed = root.join("seed");
        run(&root, &["clone", "-q", fork.to_str().unwrap(), "seed"]);
        std::fs::write(seed.join("nas.yml"), "---\n- hosts: all\n").unwrap();
        std::fs::create_dir_all(seed.join("roles/deluge")).unwrap();
        std::fs::write(seed.join("roles/deluge/README.md"), "# deluge\n").unwrap();
        std::fs::create_dir_all(seed.join("roles/deluge/defaults")).unwrap();
        std::fs::write(
            seed.join("roles/deluge/defaults/main.yml"),
            "---\ndeluge_port: 8112\n",
        )
        .unwrap();
        std::fs::create_dir_all(seed.join(".github/workflows")).unwrap();
        std::fs::write(
            seed.join(".github/workflows/ci.yml"),
            "on:\n  push:\n    branches: [main, develop]\n",
        )
        .unwrap();
        std::fs::write(
            seed.join(".github/workflows/README.txt"),
            "not a workflow\n",
        )
        .unwrap();
        run(&seed, &["add", "."]);
        commit(&seed, "the lab", "John Doe <john@example.com>");
        run(&seed, &["push", "-q", "origin", &format!("HEAD:{base}")]);
        let mirror = Mirror::new(&root.join("data"), fork.to_str().unwrap(), base);
        let bundle = mirror.bundle(None, 0).unwrap();
        assert_eq!(bundle.branch, base);
        let file = root.join("checkout.bundle");
        std::fs::write(&file, &bundle.bytes).unwrap();
        run(
            &root,
            &["clone", "-q", "-b", base, file.to_str().unwrap(), "work"],
        );
        let work = root.join("work");
        run(&work, &["remote", "remove", "origin"]);
        Scene {
            root,
            fork,
            mirror,
            work,
            base: base.to_owned(),
        }
    }

    impl Scene {
        /// A branch of one commit in the working clone, changing `file`, authored `author`.
        fn branch(&self, name: &str, file: &str, message: &str, author: &str) {
            run(&self.work, &["checkout", "-q", "-B", name, &self.base]);
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
                    &format!("{}..{name}", self.base),
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
    fn upstream_is_known_by_its_name_and_by_any_of_its_addresses() {
        for named in [
            "upstream",
            "upstream/master",
            "Upstream/main",
            "refs/remotes/upstream/master",
            "refs/heads/upstream/master",
            "refs/upstream/master",
        ] {
            assert!(names_upstream(named), "{named}");
        }
        for not in [
            "develop",
            "hq/task-936-upstream-diff",
            "upstreamish",
            "main",
        ] {
            assert!(!names_upstream(not), "{not}");
        }
        let up = "https://github.com/davestephens/ansible-nas.git";
        for same in [
            "https://github.com/davestephens/ansible-nas",
            "git@github.com:davestephens/ansible-nas.git",
            "ssh://git@github.com/DaveStephens/ansible-nas.git/",
        ] {
            assert!(same_repo(up, same), "{same}");
        }
        assert!(!same_repo(up, "git@github.com:example/ansible-nas.git"));
        // The fork is never kept twice as its own upstream.
        let fork = Mirror::new(Path::new("/nowhere"), up, "master").with_upstream(up);
        assert_eq!(fork.upstream, None);
    }

    #[test]
    fn the_checks_read_each_role_readme_and_its_defaults_and_nothing_forbidden() {
        let changed: Vec<String> = [
            "roles/media/README.md",
            "roles/media/tasks/main.yml",
            "docs/README.md",
            "roles/vault-thing/README.md",
            "roles/media/readme.md",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            read_for_checks(&changed),
            [
                "roles/media/README.md",
                "roles/media/defaults/main.yml",
                "roles/media/readme.md"
            ]
        );
    }

    #[test]
    fn the_workflows_handed_over_are_the_yaml_files_in_github_s_folder_alone() {
        let listing = ".github/workflows/ci.yml\0.github/workflows/docs.yaml\0\
                       .github/workflows/README.md\0.github/workflows/vault.yml\0";
        assert_eq!(
            workflow_paths(listing),
            [".github/workflows/ci.yml", ".github/workflows/docs.yaml"]
        );
        let many: String = (0..30)
            .map(|i| format!(".github/workflows/w{i}.yml\0"))
            .collect();
        assert_eq!(workflow_paths(&many).len(), WORKFLOWS_MOST);
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
    fn the_mirror_hands_out_the_base_branch_and_refreshes_at_most_every_five_minutes() {
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
        // HQ's mechanical checks are handed the file names and the role's README and
        // defaults as they are at the head (ADR 0142, amended 2026-10-30).
        assert_eq!(taken.changed, ["roles/deluge/README.md"]);
        assert_eq!(
            taken.texts,
            [
                (
                    "roles/deluge/README.md".to_owned(),
                    "# hq/task-3-tidy\n".to_owned()
                ),
                (
                    "roles/deluge/defaults/main.yml".to_owned(),
                    "---\ndeluge_port: 8112\n".to_owned()
                ),
            ]
        );
        // And every GitHub Actions workflow at the head, for the check of a CI criterion
        // (ADR 0142, amended 2026-10-31), whether the branch changes it or not.
        assert_eq!(
            taken.workflows,
            [(
                ".github/workflows/ci.yml".to_owned(),
                "on:\n  push:\n    branches: [main, develop]\n".to_owned()
            )]
        );
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
        // A refresh five minutes on keeps the submission: the fork is fetched by its heads.
        s.mirror.refresh(REFRESH_MS).unwrap();
        assert_eq!(
            run(
                &s.mirror.dir,
                &["rev-parse", "refs/submissions/hq/task-3-tidy"]
            ),
            taken.head
        );
        // The check's sandbox gets the whole history, fetchable on its own.
        let whole = s
            .mirror
            .submission_bundle("hq/task-3-tidy", &taken.head)
            .unwrap()
            .unwrap();
        let file = s.root.join("whole.bundle");
        std::fs::write(&file, &whole).unwrap();
        let heads = run(&s.root, &["bundle", "list-heads", file.to_str().unwrap()]);
        assert!(
            heads.ends_with("refs/submissions/hq/task-3-tidy"),
            "{heads}"
        );
        run(&s.root, &["init", "-q", "check"]);
        run(
            &s.root.join("check"),
            &[
                "fetch",
                "-q",
                file.to_str().unwrap(),
                "refs/submissions/hq/task-3-tidy:refs/heads/check",
            ],
        );
        assert_eq!(
            run(&s.root.join("check"), &["rev-parse", "refs/heads/check"]),
            taken.head
        );
        assert!(
            s.mirror
                .submission_bundle("hq/task-3-tidy", "0000000")
                .unwrap()
                .is_none()
        );
        assert!(
            s.mirror
                .submission_bundle("hq/task-9-none", &taken.head)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            s.mirror.stored_latest(3, "hq/task-3-tidy"),
            Some(taken.path.clone())
        );
        assert_eq!(s.mirror.stored_latest(9, "hq/task-3-tidy"), None);
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

    /// The owner's values may not leave by a side door (ADR 0115): a branch that moves
    /// `group_vars/all.yml` elsewhere touches it, though git lists a move by its new name, and a
    /// file under `group_vars/` with a name outside ASCII is still under `group_vars/`, though
    /// git quotes such a name.
    #[test]
    fn a_branch_that_moves_an_owners_file_or_names_one_outside_ascii_is_refused() {
        let s = scene("side-door");
        run(&s.work, &["checkout", "-q", "main"]);
        std::fs::create_dir_all(s.work.join("group_vars")).unwrap();
        std::fs::write(s.work.join("group_vars/all.yml"), "---\nlab_domain: x\n").unwrap();
        run(&s.work, &["add", "."]);
        commit(&s.work, "the owner's values", "John Doe <john@example.com>");
        run(
            &s.work,
            &["push", "-q", s.fork.to_str().unwrap(), "HEAD:main"],
        );
        let later = 2 * REFRESH_MS;
        run(&s.work, &["checkout", "-q", "-B", "hq/task-3-move", "main"]);
        run(&s.work, &["mv", "group_vars/all.yml", "roles/all.yml"]);
        commit(&s.work, TRAILED, HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-move"), "hq/task-3-move", 3, later)
            .unwrap_err();
        assert!(why.contains("touches group_vars/all.yml"), "{why}");
        s.branch("hq/task-3-accent", "group_vars/\u{e9}.yml", TRAILED, HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-accent"), "hq/task-3-accent", 3, later)
            .unwrap_err();
        assert!(why.contains("touches group_vars/\u{e9}.yml"), "{why}");
        let _ = std::fs::remove_dir_all(&s.root);
    }

    /// Live (2026-10-18): a submitted `hq/` branch is kept as `refs/submissions/<name>`, but
    /// `repo_checkout` looked only at the fork's heads, so a member or the tester could never
    /// have it back, and recreated the work (task 670's re-cut, 675's `-v2`).
    #[test]
    fn a_kept_submission_is_handed_out_with_its_base_and_only_at_its_recorded_head() {
        let s = scene("kept");
        s.branch("hq/task-3-tidy", "roles/deluge/README.md", TRAILED, HQ);
        let taken = s
            .mirror
            .submit(&s.delta("hq/task-3-tidy"), "hq/task-3-tidy", 3, 0)
            .unwrap();
        // The fork has no such branch (the owner has not pushed it): the old path is closed.
        assert!(s.mirror.bundle(Some("hq/task-3-tidy"), 0).is_err());
        let kept = s
            .mirror
            .kept_bundle("hq/task-3-tidy", &taken.head, 0)
            .unwrap();
        assert_eq!(
            (kept.branch.as_str(), kept.head.as_str()),
            ("hq/task-3-tidy", taken.head.as_str())
        );
        let file = s.root.join("kept.bundle");
        std::fs::write(&file, &kept.bytes).unwrap();
        // Both refs, whole history: the base to start from, and the kept submission.
        let heads = run(&s.root, &["bundle", "list-heads", file.to_str().unwrap()]);
        assert!(
            heads.contains("refs/heads/main") && heads.contains("refs/submissions/hq/task-3-tidy"),
            "{heads}"
        );
        // As the sandbox unpacks it: clone the base, fetch the kept ref as a head, check it out.
        run(
            &s.root,
            &["clone", "-q", "-b", "main", file.to_str().unwrap(), "kept"],
        );
        let there = s.root.join("kept");
        run(
            &there,
            &[
                "fetch",
                "-q",
                file.to_str().unwrap(),
                "refs/submissions/hq/task-3-tidy:refs/heads/hq/task-3-tidy",
            ],
        );
        run(&there, &["checkout", "-q", "hq/task-3-tidy"]);
        assert_eq!(run(&there, &["rev-parse", "HEAD"]), taken.head);
        assert_eq!(run(&there, &["rev-parse", "main"]), taken.base);
        // One more commit on it, bundled from main, is taken by the mirror: it can be submitted
        // again, which moves the kept ref.
        std::fs::write(there.join("roles/deluge/more.yml"), "---\n").unwrap();
        run(&there, &["add", "."]);
        commit(&there, TRAILED, HQ);
        let out = s.root.join("again.bundle");
        run(
            &there,
            &[
                "bundle",
                "create",
                out.to_str().unwrap(),
                "main..hq/task-3-tidy",
            ],
        );
        let again = s
            .mirror
            .submit(&std::fs::read(&out).unwrap(), "hq/task-3-tidy", 3, 1)
            .unwrap();
        assert_eq!(again.files, 2);
        // HQ's record named the first head: the ref moved since, so that head is refused, and
        // the new one is handed out. An abbreviation of a head is the head.
        let stale = s
            .mirror
            .kept_bundle("hq/task-3-tidy", &taken.head, 1)
            .unwrap_err();
        assert!(stale.contains("submitted again"), "{stale}");
        assert!(
            s.mirror
                .kept_bundle("hq/task-3-tidy", &again.head[..12], 1)
                .is_ok()
        );
        // Nothing else is handed out this way: no submission of that name, no head, not hq/.
        for (branch, head) in [
            ("hq/task-9-none", again.head.as_str()),
            ("hq/task-3-tidy", ""),
            ("hq/task-3-tidy", "abc"),
            ("main", again.head.as_str()),
        ] {
            assert!(
                s.mirror.kept_bundle(branch, head, 1).is_err(),
                "{branch} {head:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&s.root);
    }

    /// A kept ref is keyed by the branch's name alone, so a later submission of the same name
    /// from another task overwrote it (the push was then refused, or the check's bundle gone).
    /// A branch is named for the task it is submitted on, so no task can reach another's.
    #[test]
    fn a_submission_is_named_for_its_task_so_no_task_overwrites_anothers_kept_branch() {
        let s = scene("named");
        s.branch("hq/task-4-tidy", "a.yml", TRAILED, HQ);
        let delta = s.delta("hq/task-4-tidy");
        for task in [9, 40, 1] {
            let why = s
                .mirror
                .submit(&delta, "hq/task-4-tidy", task, 0)
                .unwrap_err();
            assert!(
                why.contains(&format!("hq/task-{task}-<slug>")) && why.contains("hq/task-4-tidy"),
                "task {task}: {why}"
            );
        }
        // A bare task number is no name, and a longer number does not borrow a shorter one's.
        s.branch("hq/task-12", "b.yml", TRAILED, HQ);
        assert!(
            s.mirror
                .submit(&s.delta("hq/task-12"), "hq/task-12", 12, 0)
                .is_err()
        );
        s.branch("hq/task-12-tidy", "b.yml", TRAILED, HQ);
        assert!(
            s.mirror
                .submit(&s.delta("hq/task-12-tidy"), "hq/task-12-tidy", 1, 0)
                .is_err()
        );
        let taken = s
            .mirror
            .submit(&s.delta("hq/task-12-tidy"), "hq/task-12-tidy", 12, 0)
            .unwrap();
        assert_eq!(taken.branch, "hq/task-12-tidy");
        assert_eq!(branch_for(4, "Tidy"), "hq/task-4-tidy");
        assert!(task_branch("hq/task-4-tidy", 4).is_ok());
        let _ = std::fs::remove_dir_all(&s.root);
    }

    /// Live (task 679): a probe branch with no file in it was accepted, because `take` refused
    /// only a head equal to the base; its check ran, and it would have been offered to the owner
    /// as a push of nothing. Commits that change nothing are no branch to review.
    #[test]
    fn an_empty_branch_is_refused_at_submit() {
        let s = scene("empty");
        let empty = |message: &str| {
            run(
                &s.work,
                &[
                    "-c",
                    "user.name=Endora HQ",
                    "-c",
                    "user.email=hq@endora.invalid",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    message,
                ],
            );
        };
        // A commit with no change in it.
        run(
            &s.work,
            &["checkout", "-q", "-B", "hq/task-3-probe", "main"],
        );
        empty(TRAILED);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-probe"), "hq/task-3-probe", 3, 0)
            .unwrap_err();
        assert!(why.contains("changes no file"), "{why}");
        // A change and its undoing: commits beyond main that add up to nothing.
        run(
            &s.work,
            &["checkout", "-q", "-B", "hq/task-3-undone", "main"],
        );
        std::fs::write(s.work.join("a.yml"), "---\n").unwrap();
        run(&s.work, &["add", "."]);
        commit(&s.work, TRAILED, HQ);
        run(&s.work, &["rm", "-q", "a.yml"]);
        commit(&s.work, TRAILED, HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-undone"), "hq/task-3-undone", 3, 0)
            .unwrap_err();
        assert!(why.contains("changes no file"), "{why}");
        assert!(
            s.mirror
                .kept_bundle("hq/task-3-probe", &run(&s.work, &["rev-parse", "main"]), 0)
                .is_err(),
            "nothing refused is kept"
        );
        // One real file is a branch.
        s.branch("hq/task-3-real", "a.yml", TRAILED, HQ);
        assert!(
            s.mirror
                .submit(&s.delta("hq/task-3-real"), "hq/task-3-real", 3, 0)
                .is_ok()
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }

    /// The kept ref is what the push and the checkout of a kept branch are bound to. A
    /// submission that is refused (a missing trailer was the commonest live: five times) must
    /// leave it as it was, or a good branch is lost to a bad second attempt of the same name.
    #[test]
    fn a_refused_submission_leaves_the_kept_branch_as_it_was() {
        let s = scene("clobber");
        s.branch("hq/task-3-tidy", "roles/deluge/README.md", TRAILED, HQ);
        let good = s
            .mirror
            .submit(&s.delta("hq/task-3-tidy"), "hq/task-3-tidy", 3, 0)
            .unwrap();
        let kept = |s: &Scene| {
            run(
                &s.mirror.dir,
                &["rev-parse", "refs/submissions/hq/task-3-tidy"],
            )
        };
        assert_eq!(kept(&s), good.head);
        // The same name again, one commit more, without its trailer: refused...
        std::fs::write(s.work.join("roles/deluge/more.yml"), "---\n").unwrap();
        run(&s.work, &["add", "."]);
        commit(&s.work, "no trailer here", HQ);
        let why = s
            .mirror
            .submit(&s.delta("hq/task-3-tidy"), "hq/task-3-tidy", 3, 1)
            .unwrap_err();
        assert!(why.contains("trailer"), "{why}");
        // ... and the branch the node keeps is still the good one, whole.
        assert_eq!(
            kept(&s),
            good.head,
            "a refused submission moved the kept ref"
        );
        assert!(
            s.mirror
                .kept_bundle("hq/task-3-tidy", &good.head, 1)
                .is_ok()
        );
        assert!(
            s.mirror
                .submission_bundle("hq/task-3-tidy", &good.head)
                .unwrap()
                .is_some()
        );
        // Nor is anything left staged: the mirror holds the kept ref and the fork's heads.
        let refs = run(&s.mirror.dir, &["for-each-ref", "--format=%(refname)"]);
        assert!(!refs.contains("refs/incoming"), "{refs}");
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

    /// What a branch would carry out: its diff against where it left develop, and its commits'
    /// messages, for the privacy scan (ADR 0115, amended 2026-10-31).
    #[test]
    fn what_a_branch_carries_out_is_its_diff_and_its_messages() {
        let s = scene_on("outgoing", "develop");
        s.branch("hq/task-6-out", "roles/deluge/x.yml", TRAILED, HQ);
        let taken = s
            .mirror
            .submit(&s.delta("hq/task-6-out"), "hq/task-6-out", 6, 0)
            .unwrap();
        let out = s.mirror.outgoing("hq/task-6-out", &taken.head).unwrap();
        assert!(
            out.diff.contains("+++ b/roles/deluge/x.yml"),
            "{}",
            out.diff
        );
        assert!(out.diff.contains("+# hq/task-6-out"), "{}", out.diff);
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].0, taken.head);
        assert_eq!(out.messages[0].1, TRAILED);
        assert!(s.mirror.outgoing("hq/task-6-out", "0000000").is_err());
        let _ = std::fs::remove_dir_all(&s.root);
    }

    /// Merging into develop (ADR 0115, amended 2026-10-31): a fast-forward when develop has not
    /// moved, a merge commit by HQ when it has, nothing on a conflict, never main, never a force.
    #[test]
    fn a_pushed_branch_is_merged_into_develop_by_fast_forward_or_merge_never_force() {
        let s = scene_on("merge", "develop");
        let fork = s.fork.to_str().unwrap();
        // Two branches cut from the same develop; a third that clashes with the first.
        let mut taken = Vec::new();
        for (name, file, task) in [
            ("hq/task-7-ff", "a.yml", 7),
            ("hq/task-8-merge", "b.yml", 8),
            ("hq/task-9-clash", "a.yml", 9),
        ] {
            s.branch(name, file, TRAILED, HQ);
            taken.push(s.mirror.submit(&s.delta(name), name, task, 0).unwrap());
        }
        let said = s
            .mirror
            .merge_into_base("hq/task-7-ff", &taken[0].head, fork, None)
            .unwrap();
        assert!(said.contains("fast-forward"), "{said}");
        assert_eq!(
            run(&s.fork, &["rev-parse", "refs/heads/develop"]),
            taken[0].head
        );
        // Develop moved past the second branch's base: one merge commit, by HQ, both parents.
        s.mirror.refresh(REFRESH_MS * 2).unwrap();
        let said = s
            .mirror
            .merge_into_base("hq/task-8-merge", &taken[1].head, fork, None)
            .unwrap();
        assert!(said.contains("merge commit"), "{said}");
        let parents = run(&s.fork, &["rev-list", "--parents", "-n", "1", "develop"]);
        assert!(
            parents.contains(&taken[0].head) && parents.contains(&taken[1].head),
            "{parents}"
        );
        let author = run(&s.fork, &["log", "-1", "--format=%an <%ae>", "develop"]);
        assert_eq!(author, HQ);
        // Again: already in develop, nothing pushed.
        s.mirror.refresh(REFRESH_MS * 3).unwrap();
        let again = s
            .mirror
            .merge_into_base("hq/task-8-merge", &taken[1].head, fork, None)
            .unwrap();
        assert!(again.contains("already in develop"), "{again}");
        // A clash merges nothing.
        let before = run(&s.fork, &["rev-parse", "refs/heads/develop"]);
        let why = s
            .mirror
            .merge_into_base("hq/task-9-clash", &taken[2].head, fork, None)
            .unwrap_err();
        assert!(why.contains("does not merge cleanly"), "{why}");
        assert_eq!(run(&s.fork, &["rev-parse", "refs/heads/develop"]), before);
        // The refusals: a head not stored, and main, which mirrors upstream.
        assert!(
            s.mirror
                .merge_into_base("hq/task-7-ff", "0000000", fork, None)
                .is_err()
        );
        let on_main = scene("merge-main");
        on_main.branch("hq/task-9-m", "c.yml", TRAILED, HQ);
        let m = on_main
            .mirror
            .submit(&on_main.delta("hq/task-9-m"), "hq/task-9-m", 9, 0)
            .unwrap();
        let why = on_main
            .mirror
            .merge_into_base("hq/task-9-m", &m.head, on_main.fork.to_str().unwrap(), None)
            .unwrap_err();
        assert!(why.contains("develop only"), "{why}");
        let _ = std::fs::remove_dir_all(&s.root);
        let _ = std::fs::remove_dir_all(&on_main.root);
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
